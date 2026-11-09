use std::time::Duration;

use base64::Engine;
use rcli_testkit::{FakePlatform, SessionState, StatusCode, TestSystem};
use serde_json::{json, Value};

/// An open interactive shell: the NDJSON output stream plus the session id for input/resize.
struct Shell {
    base: String,
    token: String,
    session: String,
    resp: reqwest::Response,
    buf: Vec<u8>,
    /// Decoded terminal output so far.
    screen: String,
}

impl Shell {
    async fn open(sys: &TestSystem, token: &str, device: &str, body: Value) -> Result<(Shell, Value), (StatusCode, Value)> {
        let base = format!("http://{}", sys.server.http_addr);
        let resp = reqwest::Client::new()
            .post(format!("{base}/api/v1/devices/{device}/shell"))
            .bearer_auth(token)
            .json(&body)
            .send()
            .await
            .unwrap();
        if resp.status() != StatusCode::OK {
            let status = resp.status();
            return Err((status, resp.json().await.unwrap_or(Value::Null)));
        }
        let mut shell = Shell { base, token: token.into(), session: String::new(), resp, buf: vec![], screen: String::new() };
        let first = shell.line().await.expect("first line");
        shell.session = first["session"].as_str().expect("session id").into();
        Ok((shell, first))
    }

    /// Next NDJSON line; output lines are appended to `screen`. None = stream ended.
    async fn line(&mut self) -> Option<Value> {
        loop {
            if let Some(i) = self.buf.iter().position(|b| *b == b'\n') {
                let line: Value = serde_json::from_slice(&self.buf[..i]).unwrap();
                self.buf.drain(..=i);
                if let Some(data) = line["output"].as_str() {
                    let bytes = base64::engine::general_purpose::STANDARD.decode(data).unwrap();
                    self.screen += &String::from_utf8_lossy(&bytes);
                }
                return Some(line);
            }
            let chunk = tokio::time::timeout(Duration::from_secs(10), self.resp.chunk()).await.expect("output in time");
            self.buf.extend_from_slice(&chunk.unwrap()?);
        }
    }

    /// Reads output until the screen contains `text`.
    async fn expect(&mut self, text: &str) {
        while !self.screen.contains(text) {
            let line = self.line().await;
            assert!(line.as_ref().is_some_and(|l| l.get("output").is_some()), "waiting for {text:?}, got {line:?}; screen {:?}", self.screen);
        }
    }

    /// Reads until the stream ends; returns the last line.
    async fn finish(&mut self) -> Value {
        let mut last = Value::Null;
        while let Some(l) = self.line().await {
            last = l;
        }
        last
    }

    async fn post(&self, what: &str, body: reqwest::Body, token: &str) -> StatusCode {
        let url = format!("{}/api/v1/shell-sessions/{}/{what}", self.base, self.session);
        reqwest::Client::new().post(url).bearer_auth(token).body(body).send().await.unwrap().status()
    }

    async fn input(&self, bytes: &str) {
        assert_eq!(self.post("input", bytes.to_owned().into(), &self.token).await, StatusCode::NO_CONTENT);
    }

    async fn resize(&self, cols: u32, rows: u32) {
        let body = json!({ "cols": cols, "rows": rows }).to_string();
        assert_eq!(self.post("resize", body.into(), &self.token).await, StatusCode::NO_CONTENT);
    }
}

async fn audit(sys: &TestSystem) -> Vec<Value> {
    let (status, body) = sys.api.get("/api/v1/audit").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body.as_array().unwrap().iter().filter(|e| e["action"].as_str().unwrap().starts_with("shell.")).cloned().collect()
}

async fn online(sys: &TestSystem, platform: FakePlatform) -> (String, rcli_testkit::TestAgent) {
    let agent = sys.start_agent_on(platform).await;
    let dev = agent.device_id().to_owned();
    sys.wait_device(&dev, |d| d["status"] == "Online").await;
    (dev, agent)
}

#[tokio::test]
async fn interactive_shell_runs_commands_as_the_logged_in_user_and_audits_start_and_end() {
    let sys = TestSystem::start().await;
    let (dev, _agent) = online(&sys, FakePlatform::new("lab-pc")).await;
    let alice = sys.create_user("alice", "operator").await;
    // Someone else holds the Control Lock: the terminal is not exclusive.
    assert_eq!(sys.api.post(&format!("/api/v1/devices/{dev}/lock")).await.0, StatusCode::OK);

    let (mut sh, first) = Shell::open(&sys, &alice, &dev, json!({ "cols": 80, "rows": 24 })).await.unwrap();
    assert_eq!(first["as"], "user", "{first}");
    sh.expect("$ ").await;
    sh.input("whoami\r").await;
    sh.expect("whoami\r\nuser\r\n$ ").await;
    sh.input("exit\r").await;
    assert_eq!(sh.finish().await, json!({ "exit_code": 0 }));

    let entries = audit(&sys).await;
    assert_eq!(entries.len(), 2, "{entries:?}");
    assert_eq!(entries[0]["action"], "shell.start");
    assert_eq!(entries[0]["user"], "alice");
    assert_eq!(entries[0]["result"], json!({ "session": sh.session, "as": "user", "error": null }));
    assert_eq!(entries[1]["action"], "shell.end");
    assert_eq!(entries[1]["params"], json!({ "session": sh.session }));
    assert_eq!(entries[1]["result"], json!({ "exit_code": 0, "reason": "exited" }));
}

#[tokio::test]
async fn the_terminal_resizes_and_ctrl_c_reaches_the_shell() {
    let sys = TestSystem::start().await;
    let (dev, _agent) = online(&sys, FakePlatform::new("lab-pc")).await;

    let (mut sh, _) = Shell::open(&sys, &sys.admin_token, &dev, json!({ "cols": 100, "rows": 30 })).await.unwrap();
    sh.input("stty size\r").await;
    sh.expect("30 100\r\n").await;
    sh.resize(132, 43).await;
    sh.input("stty size\r").await;
    sh.expect("43 132\r\n").await;
    sh.input("half a line\x03").await;
    sh.expect("half a line^C\r\n$ ").await;
    sh.input("exit 3\r").await;
    assert_eq!(sh.finish().await, json!({ "exit_code": 3 }));
}

#[tokio::test]
async fn root_shells_are_not_gated_by_server_roles() {
    let sys = TestSystem::start().await;
    let (dev, _agent) = online(&sys, FakePlatform::new("lab-pc")).await;
    let alice = sys.create_user("alice", "operator").await;

    // The Device's Device Settings decide root access (ticket 07), not the Server role.
    let (mut sh, first) = Shell::open(&sys, &alice, &dev, json!({ "as": "root" })).await.unwrap();
    assert_eq!(first["as"], "root");
    sh.input("whoami\r").await;
    sh.expect("root\r\n").await;
    sh.input("exit\r").await;
    assert_eq!(sh.finish().await, json!({ "exit_code": 0 }));

    let platform = FakePlatform::new("kiosk");
    platform.set_session_state(SessionState::LoginScreen);
    let (kiosk, _kiosk_agent) = online(&sys, platform).await;
    let (mut sh, first) = Shell::open(&sys, &alice, &kiosk, json!({})).await.unwrap();
    assert_eq!(first["as"], "root", "with nobody logged in, commands run as root");
    sh.input("whoami\r").await;
    sh.expect("root\r\n").await;
    sh.input("exit\r").await;
    assert_eq!(sh.finish().await, json!({ "exit_code": 0 }));

    let starts: Vec<Value> = audit(&sys).await.into_iter().filter(|e| e["action"] == "shell.start").collect();
    assert_eq!(starts.len(), 2, "{starts:?}");
    assert_eq!(starts[0]["params"], json!({ "as": "root" }));
    assert_eq!(starts[0]["user"], "alice");
}

#[tokio::test]
async fn closing_the_stream_ends_the_session_and_only_its_owner_can_type() {
    let sys = TestSystem::start().await;
    let (dev, _agent) = online(&sys, FakePlatform::new("lab-pc")).await;
    let alice = sys.create_user("alice", "operator").await;

    let (mut sh, _) = Shell::open(&sys, &alice, &dev, json!({})).await.unwrap();
    sh.expect("$ ").await;
    assert_eq!(sh.post("input", "whoami\r".into(), &sys.admin_token).await, StatusCode::NOT_FOUND);
    let session = sh.session.clone();
    let (base, token) = (sh.base.clone(), sh.token.clone());
    drop(sh);

    let ends = rcli_testkit::eventually(|| async {
        let ends: Vec<Value> = audit(&sys).await.into_iter().filter(|e| e["action"] == "shell.end").collect();
        (!ends.is_empty()).then_some(ends)
    })
    .await;
    assert_eq!(ends[0]["result"], json!({ "exit_code": null, "reason": "closed" }));
    let url = format!("{base}/api/v1/shell-sessions/{session}/input");
    let status = reqwest::Client::new().post(url).bearer_auth(token).body("x").send().await.unwrap().status();
    assert_eq!(status, StatusCode::NOT_FOUND, "an ended session takes no input");
}

#[tokio::test]
async fn the_stream_reports_a_device_disconnect() {
    let sys = TestSystem::start().await;
    let (dev, mut agent) = online(&sys, FakePlatform::new("lab-pc")).await;
    let (mut sh, _) = Shell::open(&sys, &sys.admin_token, &dev, json!({})).await.unwrap();
    sh.expect("$ ").await;
    agent.stop();
    assert_eq!(sh.finish().await["error"]["code"], "device_disconnected");
}

#[tokio::test]
async fn rcli_shell_relays_keystrokes_and_exits_with_the_remote_code() {
    let sys = TestSystem::start().await;
    let (_dev, _agent) = online(&sys, FakePlatform::new("lab-pc")).await;
    let alice = sys.create_user("alice", "operator").await;
    let base = format!("http://{}", sys.server.http_addr);
    let rcli = |token: &str, args: &[&str], keys: &[&str]| {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        keys.iter().for_each(|k| tx.send(k.as_bytes().to_vec()).unwrap());
        let argv: Vec<String> = ["rcli", "--server", &base, "--token", token].iter().chain(args).map(|s| s.to_string()).collect();
        async move {
            let mut out = Vec::new();
            let result = rcli::run_with_input(argv, Some(rx), &mut out).await;
            (result, String::from_utf8(out).unwrap())
        }
    };

    let (result, out) = rcli(&alice, &["shell", "lab-pc"], &["whoami\r", "exit 5\r"]).await;
    assert!(out.contains("whoami\r\nuser\r\n"), "{out:?}");
    let exit = result.unwrap_err();
    assert_eq!(exit.downcast_ref::<rcli::RemoteExit>().map(|e| e.exit_code()), Some(5), "{exit:#}");

    let (result, _) = rcli(&sys.admin_token, &["shell", "lab-pc", "--as", "root"], &["exit\r"]).await;
    result.unwrap();

    // Server roles no longer gate Device work; the Device's settings decide root access (ticket 07).
    let (result, out) = rcli(&alice, &["shell", "lab-pc", "--as", "root"], &["whoami\r", "exit\r"]).await;
    result.unwrap();
    assert!(out.contains("root\r\n"), "{out:?}");
}
