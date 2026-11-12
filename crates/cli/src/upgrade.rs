//! `rcli upgrade`: fetch the build the Server is rolling out for this OS/arch, verify it with the update key pinned
//! at `rcli install`, and replace the running binary. As root/Administrator on an installed Device the Agent service is
//! restarted onto it too.

use std::path::{Path, PathBuf};

use crate::config::Machine;

pub struct Upgraded {
    pub version: String,
    pub target: String,
    pub path: std::path::PathBuf,
    pub restarted: bool,
}

/// `data_dir` keeps the download (`updates/<sha256>.part`) until it is installed, so an interrupted run resumes.
pub async fn run(config: &Machine, path: &Path, socket: Option<PathBuf>, data_dir: &Path) -> anyhow::Result<Upgraded> {
    let key = hex::decode(config.update_key.trim()).unwrap_or_default();
    anyhow::ensure!(!key.is_empty(), "no update key is pinned; run `sudo rcli install` again");

    let target = rcli_agent::target();
    // A running Agent names this Device, so the Server serves the build rolled out to it; a Controller-only install
    // has no Device row and gets the newest published build.
    let device = local_device(&socket.unwrap_or_else(rcli_agent::ipc::default_socket)).await;
    let url = format!("{}/api/v1/agent-builds/{target}", crate::config::api_address(config)?.trim_end_matches('/'));
    let url = match &device {
        Some(device) => format!("{url}?device={device}"),
        None => url,
    };
    let build = rcli_agent::download::build(&url, &config.server_key, data_dir, &key, &target).await?;
    let installed = install(path, &build.path);
    let _ = std::fs::remove_file(&build.path);
    Ok(Upgraded { version: build.version, target, path: path.to_owned(), restarted: installed? })
}

/// Puts the verified `build` at `path`, restarting the Agent service onto it when this upgrade owns one.
fn install(path: &Path, build: &Path) -> anyhow::Result<bool> {
    // Check the service first: on a root install, refusing after the swap would leave a new binary unstarted.
    let service = service_to_restart()?;
    if service {
        // Only Windows needs the stop: it cannot replace an image the service is running.
        service::stop()?;
    }
    if let Err(e) = replace(path, build) {
        // A service stopped for the swap must not stay down because the swap failed.
        if service {
            let _ = service::restart();
        }
        return Err(e);
    }
    if service {
        service::restart()?;
    }
    Ok(service)
}

/// Replaces `path` atomically, so a crash mid-write never leaves a half-written binary behind.
#[cfg(not(windows))]
fn replace(path: &Path, build: &Path) -> anyhow::Result<()> {
    let dir = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
    let temp = dir.join(format!(".{}.new", path.file_name().unwrap_or_default().to_string_lossy()));
    std::fs::copy(build, &temp)?;
    #[cfg(unix)]
    std::fs::set_permissions(&temp, std::os::unix::fs::PermissionsExt::from_mode(0o755))?;
    std::fs::rename(&temp, path)?;
    Ok(())
}

/// Windows cannot overwrite (or rename over) the image of a running process, but it can rename the image itself:
/// move the old binary aside, put the new one in place, and put the old one back if that fails. The service was
/// stopped first, so the aside copy is normally deleted right away; a copy a just-stopped process still maps is left
/// for the next install to overwrite (and is harmless).
#[cfg(windows)]
fn replace(path: &Path, build: &Path) -> anyhow::Result<()> {
    let dir = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let temp = dir.join(format!(".{name}.new"));
    std::fs::copy(build, &temp)?;
    if !path.exists() {
        std::fs::rename(&temp, path)?;
        return Ok(());
    }
    let old = dir.join(format!(".{name}.old-{}", std::process::id()));
    let _ = std::fs::remove_file(&old);
    std::fs::rename(path, &old)?;
    if let Err(e) = std::fs::rename(&temp, path) {
        let _ = std::fs::rename(&old, path);
        return Err(e.into());
    }
    let _ = std::fs::remove_file(&old);
    Ok(())
}

/// Root on Unix, an elevated (Administrator/SYSTEM) token on Windows.
pub(crate) fn is_root() -> bool {
    #[cfg(windows)]
    {
        use windows::Win32::{
            Foundation::HANDLE,
            Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY},
            System::Threading::{GetCurrentProcess, OpenProcessToken},
        };
        let mut token = HANDLE::default();
        let elevated = unsafe {
            OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_ok()
                && {
                    let mut elevation = TOKEN_ELEVATION::default();
                    let mut len = 0u32;
                    GetTokenInformation(
                        token,
                        TokenElevation,
                        Some(&mut elevation as *mut _ as *mut _),
                        size_of::<TOKEN_ELEVATION>() as u32,
                        &mut len as *mut u32,
                    )
                    .is_ok()
                        && elevation.TokenIsElevated != 0
                }
        };
        if !token.is_invalid() {
            let _ = unsafe { windows::Win32::Foundation::CloseHandle(token) };
        }
        elevated
    }
    #[cfg(not(windows))]
    std::process::Command::new("id").arg("-u").output().is_ok_and(|o| String::from_utf8_lossy(&o.stdout).trim() == "0")
}

/// The Device ID of the local Agent, when one is running: `rcli upgrade` then fetches the build rolled out to it.
async fn local_device(socket: &Path) -> Option<String> {
    let info = rcli_agent::ipc::send(socket, &rcli_agent::ipc::Request::Info).await.ok()?;
    Some(info["device_id"].as_str()?.to_owned())
}

/// Whether this upgrade replaces an installed Agent's binary, so its service must be restarted onto it. Root (an
/// Administrator on Windows) on an installed Device only: a Controller-only install never touches a service, and a
/// root upgrade with no service installed is refused *before* the swap (a new binary nobody starts is worse).
fn service_to_restart() -> anyhow::Result<bool> {
    if !is_root() {
        return Ok(false);
    }
    service::installed()?;
    Ok(true)
}

/// The installed Agent service in the platform's own terms. Each implements the same three steps: find it (so an
/// upgrade can refuse before replacing anything), stop it for the swap when needed, and start it onto the new binary.
#[cfg(target_os = "linux")]
mod service {
    /// The same unit `rcli install` wrote (ticket 16 owns the name).
    const UNIT: &str = crate::linux_install::UNIT;

    pub fn installed() -> anyhow::Result<()> {
        let found = std::process::Command::new("systemctl").args(["cat", UNIT]).output();
        if !matches!(found, Ok(ref out) if out.status.success()) {
            anyhow::bail!("no {UNIT} service is installed, so the new binary was not restarted");
        }
        Ok(())
    }

    /// The unit restarts from the replaced file; nothing has to stop first on Linux.
    pub fn stop() -> anyhow::Result<()> {
        Ok(())
    }

    pub fn restart() -> anyhow::Result<()> {
        let out = std::process::Command::new("systemctl").args(["restart", UNIT]).output()?;
        anyhow::ensure!(out.status.success(), "restarting the {UNIT} service failed: {}", String::from_utf8_lossy(&out.stderr).trim());
        Ok(())
    }
}

#[cfg(target_os = "macos")]
mod service {
    use std::path::Path;

    /// The root LaunchDaemon `rcli install` wrote (ticket 24 owns the label).
    const DAEMON: &str = crate::macos_install::DAEMON;

    pub fn installed() -> anyhow::Result<()> {
        let plist = Path::new("/Library/LaunchDaemons").join(format!("{DAEMON}.plist"));
        if !plist.exists() {
            anyhow::bail!("no {DAEMON} LaunchDaemon is installed ({} is missing), so the new binary was not restarted", plist.display());
        }
        Ok(())
    }

    /// launchd restarts the job from the replaced file; nothing has to stop first on macOS.
    pub fn stop() -> anyhow::Result<()> {
        Ok(())
    }

    /// `kickstart -k` kills the running Agent and starts it again from the (new) binary. It runs as root in the
    /// system domain, so this needs the root `rcli upgrade`.
    pub fn restart() -> anyhow::Result<()> {
        let out = std::process::Command::new("/bin/launchctl").args(["kickstart", "-k", &format!("system/{DAEMON}")]).output()?;
        anyhow::ensure!(
            out.status.success(),
            "restarting the {DAEMON} LaunchDaemon failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
        Ok(())
    }
}

#[cfg(windows)]
mod service {
    use windows::{
        core::PCWSTR,
        Win32::System::Services::{
            CloseServiceHandle, ControlService, OpenSCManagerW, OpenServiceW, QueryServiceStatus, StartServiceW,
            SC_HANDLE, SC_MANAGER_CONNECT, SERVICE_CONTROL_STOP, SERVICE_QUERY_STATUS, SERVICE_START, SERVICE_STATUS,
            SERVICE_STOP, SERVICE_STOP_PENDING, SERVICE_STOPPED,
        },
    };

    /// The same SCM service `rcli install` created (ticket 22 owns the name).
    const NAME: &str = crate::windows_install::SERVICE;
    /// How long the SCM may take to report STOPPED after a stop request (it applies the request asynchronously).
    const STOP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);
    /// `ERROR_SERVICE_ALREADY_RUNNING` (1056) as an HRESULT: the SCM restarted the service by itself after the stop.
    const ALREADY_RUNNING: i32 = 0x8007_0420u32 as i32;

    pub fn installed() -> anyhow::Result<()> {
        // `sc query` succeeds only for a service that exists; its output is localized, the exit code is not.
        let found = std::process::Command::new("sc.exe").args(["query", NAME]).output();
        if !matches!(found, Ok(ref out) if out.status.success()) {
            anyhow::bail!("no {NAME} service is installed, so the new binary was not restarted");
        }
        Ok(())
    }

    /// Stops the service and waits for it to report STOPPED: Windows refuses to replace the exe it is running.
    pub fn stop() -> anyhow::Result<()> {
        let (_manager, service) = open(SERVICE_STOP | SERVICE_QUERY_STATUS)?;
        let mut status = SERVICE_STATUS::default();
        unsafe {
            QueryServiceStatus(service.0, &mut status)?;
            if status.dwCurrentState != SERVICE_STOPPED {
                // A stop already in flight needs no second request (it would be refused).
                if status.dwCurrentState != SERVICE_STOP_PENDING {
                    ControlService(service.0, SERVICE_CONTROL_STOP, &mut status)?;
                }
                let deadline = std::time::Instant::now() + STOP_TIMEOUT;
                while std::time::Instant::now() < deadline {
                    std::thread::sleep(std::time::Duration::from_millis(200));
                    QueryServiceStatus(service.0, &mut status)?;
                    if status.dwCurrentState == SERVICE_STOPPED {
                        return Ok(());
                    }
                }
            } else {
                return Ok(());
            }
        }
        anyhow::bail!("the {NAME} service did not stop, so the new binary was not replaced")
    }

    /// Starts the service again; it reads the new binary from `%ProgramFiles%\rcli\rcli.exe`.
    pub fn restart() -> anyhow::Result<()> {
        let (_manager, service) = open(SERVICE_START | SERVICE_QUERY_STATUS)?;
        match unsafe { StartServiceW(service.0, None) } {
            Ok(()) => Ok(()),
            // The stop never took effect, or the SCM restarted the service by itself: it is running, which is all
            // this upgrade needs.
            Err(e) if e.code().0 == ALREADY_RUNNING => Ok(()),
            Err(e) => anyhow::bail!("starting the {NAME} service failed: {e}"),
        }
    }

    /// One handle pair closed on drop, so every early return releases the SCM and the service.
    struct Handle(SC_HANDLE);
    impl Drop for Handle {
        fn drop(&mut self) {
            let _ = unsafe { CloseServiceHandle(self.0) };
        }
    }
    struct Opened(SC_HANDLE);
    impl Drop for Opened {
        fn drop(&mut self) {
            let _ = unsafe { CloseServiceHandle(self.0) };
        }
    }

    fn open(access: u32) -> anyhow::Result<(Opened, Handle)> {
        unsafe {
            let manager = Opened(OpenSCManagerW(PCWSTR::null(), PCWSTR::null(), SC_MANAGER_CONNECT)?);
            let name: Vec<u16> = NAME.encode_utf16().chain(std::iter::once(0)).collect();
            let service = Handle(OpenServiceW(manager.0, PCWSTR(name.as_ptr()), access)?);
            Ok((manager, service))
        }
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
mod service {
    pub fn installed() -> anyhow::Result<()> {
        anyhow::bail!("restarting the Agent service is not supported on this OS yet")
    }
    pub fn stop() -> anyhow::Result<()> {
        Ok(())
    }
    pub fn restart() -> anyhow::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rcli-upgrade-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_replaced_binary_has_the_new_bytes_and_no_leftovers() {
        let dir = temp_dir("replace");
        let path = dir.join("rcli");
        std::fs::write(&path, b"old build").unwrap();
        let build = temp_dir("replace-build").join("build");
        std::fs::write(&build, b"new build").unwrap();
        replace(&path, &build).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"new build");
        let leftovers: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(leftovers, vec![std::ffi::OsString::from("rcli")], "a temp file is left behind");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_non_root_upgrade_never_touches_the_service() {
        if is_root() {
            return; // the root path needs a real installed service; it is checked on real hardware (ticket 13)
        }
        assert!(!service_to_restart().unwrap());
    }
}
