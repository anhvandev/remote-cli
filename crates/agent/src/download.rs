//! Downloads an Agent build from the Server's build lookup endpoint to disk, resuming a partial download with
//! `Range`, then checks its size and sha256. `rcli upgrade` and the Agent's update task share it.

use std::path::{Path, PathBuf};

use sha2::Digest;
use tokio::io::AsyncWriteExt;

/// A downloaded, verified build. The caller installs from `path`, then deletes it (an install step that never
/// returns deletes it itself once the binary is swapped).
pub struct Build {
    pub version: String,
    pub path: PathBuf,
    /// Lowercase hex.
    pub sha256: String,
    pub size: u64,
}

/// Fetches `url` with the Server Key into `<dir>/updates/<sha256>.part`, resuming from that file's length. A file
/// that fails the size or sha256 check is deleted and downloaded once more from zero; a second mismatch is an error.
/// Any error that is not `transient` deletes the file too.
pub async fn fetch_build(url: &str, server_key: &str, dir: &Path) -> anyhow::Result<Build> {
    let client = reqwest::Client::new();
    let get = |from: u64| {
        let req = client.get(url).header("x-rcli-server-key", server_key);
        match from {
            0 => req,
            from => req.header("range", format!("bytes={from}-")),
        }
        .send()
    };
    let resp = get(0).await?;
    if resp.status().is_server_error() {
        // A `reqwest::Error`, like a network failure: the Agent retries both (`transient`).
        return Err(resp.error_for_status().unwrap_err().into());
    }
    if !resp.status().is_success() {
        let body: serde_json::Value = resp.json().await.unwrap_or_default();
        anyhow::bail!("{}", body["error"]["message"].as_str().unwrap_or("no build for this OS/arch"));
    }
    let header = |name: &str| resp.headers().get(name).and_then(|v| v.to_str().ok()).unwrap_or_default().to_owned();
    let version = header("x-rcli-version");
    let sha256 = header("x-rcli-sha256");
    // It names a file, so nothing but a sha256 may get through.
    anyhow::ensure!(
        sha256.len() == 64 && sha256.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')),
        "the Server sent no valid x-rcli-sha256 for the build"
    );
    let size = resp.content_length().ok_or_else(|| anyhow::anyhow!("the Server sent no size for the build"))?;

    let updates = dir.join("updates");
    tokio::fs::create_dir_all(&updates).await?;
    let path = updates.join(format!("{sha256}.part"));
    if let Err(e) = save_verified(get, resp, &path, size, &sha256).await {
        if !transient(&e) {
            let _ = tokio::fs::remove_file(&path).await;
        }
        return Err(e);
    }
    Ok(Build { version, path, sha256, size })
}

/// Completes `path` until it is `size` bytes hashing to `sha256`: from `fresh` (the plain GET's response) or by
/// resuming with `get(from)`. A file already `size` bytes long is checked before anything is downloaded.
async fn save_verified<F>(get: impl Fn(u64) -> F, fresh: reqwest::Response, path: &Path, size: u64, sha256: &str) -> anyhow::Result<()>
where
    F: std::future::Future<Output = reqwest::Result<reqwest::Response>>,
{
    let mut from = tokio::fs::metadata(path).await.map_or(0, |m| m.len());
    if from == size && matches(path, size, sha256).await? {
        return Ok(());
    }
    if from >= size {
        from = 0;
    }
    let (mut fresh, mut retried) = (Some(fresh), false);
    loop {
        let resp = match (from, fresh.take()) {
            (0, Some(resp)) => resp,
            // A 5xx becomes a `reqwest::Error` with its status, so `transient`.
            (from, _) => get(from).await?.error_for_status()?,
        };
        // A Server ignoring `Range` sends the whole file (200): write it from zero.
        if resp.status() != reqwest::StatusCode::PARTIAL_CONTENT {
            from = 0;
        }
        save(resp, path, from).await?;
        if matches(path, size, sha256).await? {
            return Ok(());
        }
        tokio::fs::remove_file(path).await?;
        anyhow::ensure!(!retried, "the downloaded build does not match its size and sha256");
        (from, retried) = (0, true);
    }
}

/// Whether `e` is worth retrying later: a network failure or a 5xx from the Server (not a request that cannot be
/// built, such as a relative URL).
pub fn transient(e: &anyhow::Error) -> bool {
    e.chain().any(|c| {
        c.downcast_ref::<reqwest::Error>().is_some_and(|r| !r.is_builder() && r.status().is_none_or(|s| s.is_server_error()))
    })
}

/// Streams `resp` into `path`: appended after the first `from` bytes, or replacing the file when `from` is 0.
async fn save(mut resp: reqwest::Response, path: &Path, from: u64) -> anyhow::Result<()> {
    let mut file = tokio::fs::OpenOptions::new().create(true).write(true).append(from > 0).truncate(from == 0).open(path).await?;
    while let Some(chunk) = resp.chunk().await? {
        file.write_all(&chunk).await?;
    }
    file.flush().await?;
    Ok(())
}

async fn matches(path: &Path, size: u64, sha256: &str) -> anyhow::Result<bool> {
    let (path, sha256) = (path.to_owned(), sha256.to_owned());
    tokio::task::spawn_blocking(move || {
        let mut hasher = sha2::Sha256::new();
        let read = std::io::copy(&mut std::fs::File::open(path)?, &mut hasher)?;
        Ok(read == size && hex::encode(hasher.finalize()) == sha256)
    })
    .await?
}
