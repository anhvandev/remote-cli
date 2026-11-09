//! Install scripts and per-OS/arch build downloads (ticket 17), public like the update key:
//! the one-line install has no credentials before registration. The Dashboard origin proxies these paths.

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};

use super::ApiError;
use crate::updates;

pub fn routes() -> Router<super::State> {
    Router::new()
        .route("/install.sh", get(install_sh))
        .route("/install.ps1", get(install_ps1))
        .route("/download/{os}/{arch}", get(download))
}

/// The origin the script was fetched from, so its download URL works through the Dashboard origin too.
/// A proxy in front should set `X-Forwarded-Host` and `X-Forwarded-Proto` (a wrong value only misleads the
/// client that sent it, since the script is rendered per request).
fn origin(headers: &HeaderMap) -> String {
    let get = |name: &str| headers.get(name).and_then(|v| v.to_str().ok());
    let host = get("x-forwarded-host").or_else(|| get("host")).unwrap_or("localhost");
    let scheme = get("x-forwarded-proto").unwrap_or("http");
    format!("{scheme}://{host}")
}

/// The first `host:port` of the (comma-separated) Server Address: the one `rcli agent install` dials.
fn first_address(address: &str) -> &str {
    address.split(',').next().unwrap_or("").trim()
}

async fn install_sh(State(state): State<super::State>, headers: HeaderMap) -> Response {
    let script = INSTALL_SH
        .replace("{{ORIGIN}}", &origin(&headers))
        .replace("{{SERVER}}", first_address(&state.settings.address().await));
    ([("content-type", "text/x-shellscript; charset=utf-8")], script).into_response()
}

async fn install_ps1(State(state): State<super::State>, headers: HeaderMap) -> Response {
    let script = INSTALL_PS1
        .replace("{{ORIGIN}}", &origin(&headers))
        .replace("{{SERVER}}", first_address(&state.settings.address().await));
    ([("content-type", "text/plain; charset=utf-8")], script).into_response()
}

/// The build bytes of the currently published (rolled-out) build for `os-arch`, signed by the update key.
async fn download(State(state): State<super::State>, Path((os, arch)): Path<(String, String)>) -> Result<Response, ApiError> {
    let target = format!("{os}-{arch}");
    let (version, signature, build) = updates::latest(&state, &target)
        .await?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "no_build", format!("no build for {target}")))?;
    Ok((
        [
            ("x-rcli-version", version),
            ("x-rcli-signature", hex::encode(signature)),
            ("content-type", "application/octet-stream".to_owned()),
        ],
        build,
    )
        .into_response())
}

// shortcut: `rcli agent install` (which installs the service) lands with tickets 16/22/24; until then the last
// step of this script fails with "no such command" on a real machine.
const INSTALL_SH: &str = r#"#!/bin/sh
# Installs the rcli Agent on this machine and registers it with the Server.
# Usage: curl -fsSL {{ORIGIN}}/install.sh | sudo sh -s -- --key <Server Key>
set -eu
key=""
while [ "$#" -gt 0 ]; do
    case "$1" in
        --key) key="${2:-}"; shift 2 ;;
        *) echo "usage: install.sh --key <Server Key>" >&2; exit 2 ;;
    esac
done
if [ -z "$key" ]; then
    echo "install.sh: give --key <Server Key> (the Connect page shows it)" >&2
    exit 2
fi
os=$(uname -s | tr '[:upper:]' '[:lower:]')
arch=$(uname -m)
case "$arch" in
    x86_64 | amd64) arch=x86_64 ;;
    aarch64 | arm64) arch=aarch64 ;;
esac
echo "downloading {{ORIGIN}}/download/$os/$arch"
curl -fsSL "{{ORIGIN}}/download/$os/$arch" -o /tmp/rcli
chmod +x /tmp/rcli
/tmp/rcli agent install --server {{SERVER}} --key "$key"
"#;

// shortcut: same as install.sh — `rcli agent install` lands with ticket 22, which also finishes this script
// (service install, Start Menu). One-line usage:
//   iwr {{ORIGIN}}/install.ps1 -OutFile install.ps1; .\install.ps1 -Key <Server Key>
const INSTALL_PS1: &str = r#"# Installs the rcli Agent on this machine and registers it with the Server.
# Usage: .\install.ps1 -Key <Server Key>
param([string]$Key = "")
$ErrorActionPreference = "Stop"
if (-not $Key) { throw "give -Key <Server Key> (the Connect page shows it)" }
$arch = switch ($env:PROCESSOR_ARCHITECTURE) {
    "ARM64" { "aarch64" }
    default { "x86_64" }
}
$url = "{{ORIGIN}}/download/windows/$arch"
Write-Host "downloading $url"
$tmp = Join-Path $env:TEMP "rcli.exe"
Invoke-WebRequest -Uri $url -OutFile $tmp
& $tmp agent install --server {{SERVER}} --key $Key
"#;
