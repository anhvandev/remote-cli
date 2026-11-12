//! Downloads an Agent build from the Server's build lookup endpoint to disk, resuming a partial download with
//! `Range`, then checks its size, sha256 and update signature. `rcli upgrade` and the Agent's update task share it.

use std::path::{Path, PathBuf};

use sha2::Digest;
use tokio::io::AsyncWriteExt;

/// A downloaded, verified build. The caller installs from `path`, then deletes it.
pub struct Build {
    pub version: String,
    pub path: PathBuf,
}

/// Fetches `url` with the Server Key into `<dir>/updates/<sha256>.part`, resuming from that file's length, and
/// verifies it with the pinned update `key` for `target`. A file that fails a check is deleted: a size or sha256
/// mismatch after a resume is downloaded again from zero; one after a fresh download or a bad signature is an error.
pub async fn build(url: &str, server_key: &str, dir: &Path, key: &[u8], target: &str) -> anyhow::Result<Build> {
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
    if !resp.status().is_success() {
        let body: serde_json::Value = resp.json().await.unwrap_or_default();
        anyhow::bail!("{}", body["error"]["message"].as_str().unwrap_or("no build for this OS/arch"));
    }
    let header = |name: &str| resp.headers().get(name).and_then(|v| v.to_str().ok()).unwrap_or_default().to_owned();
    let version = header("x-rcli-version");
    let signature = hex::decode(header("x-rcli-signature")).unwrap_or_default();
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
    let mut from = tokio::fs::metadata(&path).await.map_or(0, |m| m.len());
    if from >= size {
        from = 0;
    }
    let mut first = Some(resp);
    loop {
        let resp = match (from, first.take()) {
            (0, Some(resp)) => resp,
            (0, None) => get(0).await?.error_for_status()?,
            (from, _) => {
                let resp = get(from).await?;
                anyhow::ensure!(resp.status() == reqwest::StatusCode::PARTIAL_CONTENT, "resuming the build download failed: {}", resp.status());
                resp
            }
        };
        save(resp, &path, from).await?;
        if matches(&path, size, &sha256).await? {
            break;
        }
        tokio::fs::remove_file(&path).await?;
        anyhow::ensure!(from > 0, "the downloaded build does not match its size and sha256");
        from = 0;
    }
    if !rcli_protocol::update::verify(key, &version, target, &sha256, &signature) {
        let _ = tokio::fs::remove_file(&path).await;
        anyhow::bail!("bad signature: the build is not signed with this install's pinned update key; the old binary is untouched");
    }
    Ok(Build { version, path })
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
