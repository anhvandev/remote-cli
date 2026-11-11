//! Install scripts and per-OS/arch build downloads (ticket 17), public like the update key:
//! the one-line install has no credentials before registration. The Dashboard origin proxies these paths.

use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, put},
    Json, Router,
};
use rcli_protocol::update::MAX_BUILD_BYTES;
use serde_json::{json, Value};

use super::{bad_request, Admin, ApiError};
use crate::updates;

pub fn routes() -> Router<super::State> {
    Router::new()
        .route("/install.sh", get(install_sh))
        .route("/install.ps1", get(install_ps1))
        .route("/download/{os}/{arch}", get(download))
        .route("/download/package/{kind}/{arch}", get(download_package))
        .route("/api/v1/packages/{kind}/{arch}", put(publish_package).layer(DefaultBodyLimit::max(MAX_BUILD_BYTES)))
}

/// The package kinds the Connect guide offers.
const PACKAGE_KINDS: [&str; 3] = ["dmg", "deb", "rpm"];

/// Body: the package file. Replaces the one stored for this kind and arch.
async fn publish_package(
    Admin(user): Admin,
    State(state): State<super::State>,
    Path((kind, arch)): Path<(String, String)>,
    package: Bytes,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    if !PACKAGE_KINDS.contains(&kind.as_str()) {
        return Err(bad_request(format!("the package kind must be one of {}", PACKAGE_KINDS.join(", "))));
    }
    if arch.is_empty() || arch.len() > 32 || !arch.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(bad_request("arch must be 1-32 chars of [A-Za-z0-9_], e.g. x86_64 or aarch64"));
    }
    if package.is_empty() {
        return Err(bad_request("empty package"));
    }
    sqlx::query(
        "INSERT INTO install_packages (kind, arch, package, uploaded_at, uploaded_by) VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (kind, arch) DO UPDATE SET package = $3, uploaded_at = $4, uploaded_by = $5",
    )
    .bind(&kind)
    .bind(&arch)
    .bind(package.as_ref())
    .bind(state.clock.now())
    .bind(&user.name)
    .execute(&state.db)
    .await
    .map_err(anyhow::Error::from)?;
    Ok((StatusCode::CREATED, Json(json!({ "kind": kind, "arch": arch, "size": package.len() }))))
}

/// The uploaded package, as a download named `rcli-<arch>.<kind>`.
async fn download_package(State(state): State<super::State>, Path((kind, arch)): Path<(String, String)>) -> Result<Response, ApiError> {
    let package: Option<Vec<u8>> = sqlx::query_scalar("SELECT package FROM install_packages WHERE kind = $1 AND arch = $2")
        .bind(&kind)
        .bind(&arch)
        .fetch_optional(&state.db)
        .await
        .map_err(anyhow::Error::from)?;
    let package = package.ok_or_else(|| {
        ApiError::new(
            StatusCode::NOT_FOUND,
            "no_package",
            format!("no .{kind} for {arch} is uploaded yet (an Admin runs `rcli package publish`)"),
        )
    })?;
    // `kind` and `arch` matched a stored row, so they are the validated characters only.
    let disposition = format!("attachment; filename=\"rcli-{arch}.{kind}\"");
    Ok(([("content-type", "application/octet-stream".to_owned()), ("content-disposition", disposition)], package).into_response())
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

/// The first `host:port` of the (comma-separated) Server Address: the one `rcli install` dials.
fn first_address(address: &str) -> &str {
    address.split(',').next().unwrap_or("").trim()
}

async fn install_sh(State(state): State<super::State>, headers: HeaderMap) -> Response {
    let script = INSTALL_SH
        .replace("{{ORIGIN}}", &origin(&headers))
        .replace("{{SERVER}}", first_address(&state.settings.address().await));
    // no-store: the script is rendered per request from the requesting origin; a shared cache must not serve
    // one client's rendering to the next (that would redirect their download).
    ([("content-type", "text/x-shellscript; charset=utf-8"), ("cache-control", "no-store")], script).into_response()
}

async fn install_ps1(State(state): State<super::State>, headers: HeaderMap) -> Response {
    let script = INSTALL_PS1
        .replace("{{ORIGIN}}", &origin(&headers))
        .replace("{{SERVER}}", first_address(&state.settings.address().await));
    ([("content-type", "text/plain; charset=utf-8"), ("cache-control", "no-store")], script).into_response()
}

/// The build bytes of the fleet's build for `os-arch` (the one last rolled out to every Device), signed by the
/// update key.
async fn download(State(state): State<super::State>, Path((os, arch)): Path<(String, String)>) -> Result<Response, ApiError> {
    let target = format!("{os}-{arch}");
    let (version, signature, build) = updates::fleet_build(&state, &target)
        .await?
        .ok_or_else(|| super::updates::no_build(&target))?;
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

// The last step installs the service: macOS (ticket 24) and Windows (ticket 22, install.ps1 below) do it today;
// Linux waits for ticket 16's systemd unit, where this line fails with "not available on this OS yet".
const INSTALL_SH: &str = r#"#!/bin/sh
# Installs rcli: makes this machine a Device (prints its Device ID and Device Password) that can also control others.
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
os=$(uname -s)
case "$os" in
    Linux) os=linux ;;
    Darwin) os=macos ;; # uname says darwin; the build targets are named macos
    *) echo "install.sh: unsupported OS: $os" >&2; exit 1 ;;
esac
arch=$(uname -m)
case "$arch" in
    x86_64 | amd64) arch=x86_64 ;;
    aarch64 | arm64) arch=aarch64 ;;
esac
# A fresh private directory: a fixed path in /tmp could be swapped by another local user before root runs it.
dir=$(mktemp -d)
trap 'rm -rf "$dir"' EXIT
echo "downloading {{ORIGIN}}/download/$os/$arch"
if ! curl -fsSL "{{ORIGIN}}/download/$os/$arch" -o "$dir/rcli"; then
    echo "install.sh: no rcli build for $os-$arch is rolled out" >&2
    exit 1
fi
chmod +x "$dir/rcli"
"$dir"/rcli install --server {{SERVER}} --key "$key"
"#;

// The PowerShell one-liner (ticket 22): download, then install elevated. An unelevated shell re-launches the same
// install through UAC and waits, so a failed install still sets a non-zero exit code. The Server Key never goes into
// the installed service's arguments; it is read by the installer here and written to a SYSTEM-only config file.
// Usage: iwr {{ORIGIN}}/install.ps1 -OutFile install.ps1; .\install.ps1 -Key <Server Key>
const INSTALL_PS1: &str = r#"# Installs rcli: makes this machine a Device (prints its Device ID and Device Password) that can also control others.
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
$dir = Join-Path $env:TEMP "rcli-install"
New-Item -ItemType Directory -Force -Path $dir | Out-Null
$exe = Join-Path $dir "rcli.exe"
Invoke-WebRequest -Uri $url -OutFile $exe
# Installing a service needs Administrator. Ask for elevation once and wait for that install, so the exit code and
# any error text are the elevated install's. The elevated process cannot write to this console, so a hidden cmd.exe
# writes its output (the Device ID and Device Password) to a file printed here.
$principal = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    Write-Host "elevating (accept the UAC prompt)"
    $log = Join-Path $dir "install.log"
    $elevated = Start-Process -FilePath "cmd.exe" -Verb RunAs -Wait -PassThru -WindowStyle Hidden -ArgumentList @(
        "/c", "`"`"$exe`" install --server {{SERVER}} --key $Key > `"$log`" 2>&1`""
    )
    Get-Content $log
    Remove-Item $log
    exit $elevated.ExitCode
}
& $exe install --server {{SERVER}} --key $Key
exit $LASTEXITCODE
"#;
