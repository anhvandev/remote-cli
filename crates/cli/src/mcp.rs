//! `rcli mcp`: the Controller API as MCP tools over stdio. Each tool is one API call; Observations come back
//! as inline images (clips as an embedded `video/mp4` blob) plus the rest of the JSON as structured data.

use std::{collections::HashMap, sync::Arc, sync::Mutex};

use rmcp::{
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation, ListToolsResult,
        PaginatedRequestParams, ResourceContents, ServerCapabilities, ServerConfig, Tool,
    },
    service::RequestContext,
    ErrorData, RoleServer, ServerHandler, ServiceExt,
};
use serde_json::{json, Map, Value};
use tokio::io::{AsyncRead, AsyncWrite};

use crate::{config, path_segment, Api};

/// Serves MCP on `read`/`write` (stdin/stdout for `rcli mcp`) until the client disconnects.
pub async fn serve<R, W>(api: Api, read: R, write: W) -> anyhow::Result<()>
where
    R: AsyncRead + Send + Unpin + 'static,
    W: AsyncWrite + Send + Unpin + 'static,
{
    let mcp = Mcp { api, watches: Default::default() };
    mcp.serve((read, write)).await?.waiting().await?;
    Ok(())
}

struct Mcp {
    api: Api,
    /// Open `lock/watch` streams per Device id while we hold its Control Lock: if this process dies the
    /// Server sees the holder disconnect and frees the lock after 60 s instead of the 10 min idle timer.
    watches: Mutex<HashMap<String, reqwest::Response>>,
}

const INSTRUCTIONS: &str = "Drive remote Devices. Take the Control Lock (lock_acquire) before act/batch; \
observe needs no lock. Coordinates are in the pixel space of the returned images. Observe with marks=true to \
get numbered Marks, then target them with {\"mark\": N, \"observation\": ID} or {\"text\": \"OK\"}. \
Release the lock when done.";

const ACTION: &str = "An Action: {\"type\":\"move|click|double_click|right_click\", TARGET} | \
{\"type\":\"drag\", TARGET, \"to\":TARGET} | {\"type\":\"scroll\", TARGET, \"dx\":0, \"dy\":3} | \
{\"type\":\"type\",\"text\":\"..\",\"secret\":false,\"into\":TARGET?} | {\"type\":\"key\",\"keys\":\"ctrl+c\"} | \
{\"type\":\"wait\",\"ms\":500}. TARGET = \"x\",\"y\" (image pixels) with optional \"display\" (default 0), or \
\"mark\":N,\"observation\":ID, or \"text\":\"visible text\".";

fn tools() -> Vec<Tool> {
    let device = json!({ "type": "string", "description": "Device id or name" });
    let observe = |modes: &str| {
        json!({
            "observe": { "type": "string", "enum": modes.split(' ').collect::<Vec<_>>(),
                "description": "shot = one settled screenshot (default); frames = screenshots of each change; clip = short MP4; none" },
            "display": { "description": "Display index to observe, or \"all\"; default: the display last acted on" },
            "marks": { "type": "boolean", "description": "Number the detected UI elements (Marks) and return their table" },
            "max_frames": { "type": "integer", "minimum": 1, "maximum": 30, "description": "frames: at most this many per display (default 10)" },
        })
    };
    let with = |mut extra: Value, base: Value| {
        extra.as_object_mut().unwrap().extend(base.as_object().unwrap().clone());
        extra
    };
    let devices_only = json!({ "device": device });
    [
        ("devices", "List Devices with their Device Status (Online, Offline, Waking).", json!({}), vec![]),
        ("device", "One Device: details, inventory and current resources (CPU, RAM, disks, session).", devices_only.clone(), vec!["device"]),
        ("lock_acquire", "Take a Device's Control Lock (needed for act/batch). Fails if someone else holds it.", devices_only.clone(), vec!["device"]),
        ("lock_release", "Release a Device's Control Lock.", devices_only.clone(), vec!["device"]),
        ("lock_status", "Who holds a Device's Control Lock and until when.", devices_only.clone(), vec!["device"]),
        ("observe", "Look at a Device's screen without acting (no lock needed).", with(devices_only.clone(), observe("shot frames clip")), vec!["device"]),
        (
            "act",
            "Send one mouse/keyboard Action (needs the Control Lock) and observe the result.",
            with(json!({ "device": device, "action": { "type": "object", "description": ACTION } }), observe("shot frames clip none")),
            vec!["device", "action"],
        ),
        (
            "batch",
            "Send an Action Batch: several Actions (incl. waits) in one request (needs the Control Lock), then observe.",
            with(json!({ "device": device, "actions": { "type": "array", "items": { "type": "object" }, "description": ACTION } }), observe("shot frames clip none")),
            vec!["device", "actions"],
        ),
        (
            "exec",
            "Run a one-shot shell command on a Device; returns exit_code, stdout, stderr, timed_out.",
            json!({
                "device": device,
                "command": { "type": "string" },
                "as": { "type": "string", "enum": ["root"], "description": "Run as root/SYSTEM (the Device's root setting decides); default: the Device's Terminal User" },
                "timeout_secs": { "type": "integer", "minimum": 1, "maximum": 3600, "description": "default 60" },
            }),
            vec!["device", "command"],
        ),
        ("wake", "Wake an Offline Device through a Wake Relay on its LAN.", devices_only, vec!["device"]),
        (
            "power",
            "Restart, shut down, sleep or lock the screen of a Device. While someone else holds the Control Lock, \
             only lock works (admins excepted).",
            json!({ "device": device, "action": { "type": "string", "enum": ["restart", "shutdown", "sleep", "lock"] } }),
            vec!["device", "action"],
        ),
    ]
    .into_iter()
    .map(|(name, description, properties, required)| {
        let schema = json!({ "type": "object", "properties": properties, "required": required });
        Tool::new(name, description, Arc::new(schema.as_object().unwrap().clone()))
    })
    .collect()
}

/// Copies the given keys of `args` that are present into a new object.
fn pick(args: &Value, keys: &[&str]) -> Map<String, Value> {
    keys.iter().filter_map(|k| Some((k.to_string(), args.get(*k)?.clone()))).collect()
}

/// Observation JSON -> image blocks (and a clip blob) + the remaining JSON (Mark table etc.) as structured data.
fn observation(mut obs: Value) -> CallToolResult {
    let mut media = vec![];
    for i in obs.get_mut("images").and_then(Value::as_array_mut).into_iter().flatten() {
        if let Some(Value::String(data)) = i.get_mut("data").map(Value::take) {
            media.push(ContentBlock::image(data, i["mime"].as_str().unwrap_or("image/jpeg")));
        }
    }
    if let Some(Value::String(data)) = obs.get_mut("clip").and_then(|c| c.get_mut("data")).map(Value::take) {
        media.push(ContentBlock::resource(ResourceContents::blob(data, "rcli://clip.mp4").with_mime_type("video/mp4")));
    }
    let mut result = CallToolResult::structured(obs);
    result.content.extend(media);
    result
}

impl Mcp {
    async fn call(&self, tool: &str, args: &Value) -> anyhow::Result<CallToolResult> {
        use reqwest::Method;
        let name = args["device"].as_str().unwrap_or_default();
        let dev = format!("/api/v1/devices/{}", path_segment(name));
        if name.is_empty() && tool != "devices" {
            anyhow::bail!("`device` is required");
        }
        let act = |actions: Value| {
            let mut body = pick(args, &["observe", "display", "marks", "max_frames"]);
            body.insert("actions".into(), actions);
            Value::Object(body)
        };
        let structured = CallToolResult::structured;
        Ok(match tool {
            "devices" => structured(json!({ "devices": self.api.call(Method::GET, "/api/v1/devices", None).await? })),
            "device" => structured(self.api.call(Method::GET, &dev, None).await?),
            "lock_status" => structured(self.api.call(Method::GET, &format!("{dev}/lock"), None).await?),
            "lock_acquire" => {
                let lock = self.api.call(Method::POST, &format!("{dev}/lock"), None).await?;
                // Without the watch the lock still works, just with the 10 min idle timer only.
                if let Ok(watch) = self.watch(&format!("{dev}/lock/watch")).await {
                    let id = self.device_id(&dev).await?;
                    self.watches.lock().unwrap().insert(id, watch);
                }
                structured(lock)
            }
            "lock_release" => {
                let lock = self.api.call(Method::POST, &format!("{dev}/lock/release"), None).await?;
                let id = self.device_id(&dev).await?;
                self.watches.lock().unwrap().remove(&id);
                structured(lock)
            }
            "observe" => observation(self.api.observe(name, &act(json!([]))).await?),
            "act" => observation(self.api.observe(name, &act(json!([args["action"]]))).await?),
            "batch" => observation(self.api.observe(name, &act(args["actions"].clone())).await?),
            "exec" => {
                let mut body = pick(args, &["command", "as", "timeout_secs"]);
                // `rcli root on <Device ID>` sets exec's default too.
                if body.get("as").is_none() && config::root_default(&self.api.dir, name) {
                    body.insert("as".into(), Value::String("root".into()));
                }
                let body = Value::Object(body);
                structured(self.api.call(Method::POST, &format!("{dev}/exec"), Some(&body)).await?)
            }
            "wake" => structured(self.api.call(Method::POST, &format!("{dev}/wake"), None).await?),
            "power" => {
                let body = Value::Object(pick(args, &["action"]));
                structured(self.api.call(Method::POST, &format!("{dev}/power"), Some(&body)).await?)
            }
            _ => anyhow::bail!("unknown tool {tool}"),
        })
    }

    /// The id of the Device at `dev` (`/api/v1/devices/{id or name}`), so a name and an id share one watch.
    async fn device_id(&self, dev: &str) -> anyhow::Result<String> {
        let d = self.api.call(reqwest::Method::GET, dev, None).await?;
        Ok(d["id"].as_str().unwrap_or_default().to_owned())
    }

    /// Opens the NDJSON lock watch and waits for its first line, so the Server counts us as connected.
    async fn watch(&self, path: &str) -> anyhow::Result<reqwest::Response> {
        let url = format!("{}{path}", self.api.base.trim_end_matches('/'));
        let mut resp = self.api.auth(self.api.http.get(url), Some(path))?.send().await?.error_for_status()?;
        resp.chunk().await?;
        Ok(resp)
    }
}

impl ServerHandler for Mcp {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build()).with_instructions(INSTRUCTIONS)
            .with_server_info(Implementation::new("rcli", env!("CARGO_PKG_VERSION")))
    }

    async fn list_tools(
        &self,
        _: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult { tools: tools(), ..Default::default() })
    }

    async fn call_tool(&self, req: CallToolRequestParams, _: RequestContext<RoleServer>) -> Result<CallToolResponse, ErrorData> {
        let args = Value::Object(req.arguments.unwrap_or_default());
        // API errors (e.g. control_lock_held with holder and expiry) become readable tool errors.
        Ok(self.call(&req.name, &args).await.unwrap_or_else(|e| CallToolResult::error(vec![ContentBlock::text(format!("{e:#}"))])).into())
    }
}
