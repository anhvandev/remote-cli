//! Windows transport for the daemon ↔ session helper split (see `session.rs`) and the helper launcher (ticket 22).
//! The installed Agent is a SYSTEM service in session 0, which cannot see or type on the interactive desktops. The
//! service launches `rcli agent session --pipe …` in the active console session with a SYSTEM token moved into that
//! session (the token winlogon itself runs with there: `DuplicateTokenEx` + `SetTokenInformation(TokenSessionId)`,
//! with `SeTcbPrivilege`), which is the only kind allowed to open the `Winlogon` and secure desktops. The helper does
//! displays, capture, input, UI Automation and the session state. The two talk over a named pipe any local user may
//! open (`\\.\pipe\rcli-agent-session`). `WTSQueryUserToken` is the fallback if duplicating fails.
//!
//! The helper is relaunched when the console session or its user changes (logon, logoff, fast user switching), and
//! again after a crash. Locking and unlocking keep the same helper: it follows the input desktop itself, so the lock
//! screen, the sign-in screen and a UAC prompt are all observable and controllable.

use std::{
    fs::File,
    os::windows::io::FromRawHandle,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use windows::{
    core::{w, PCWSTR, PWSTR},
    Win32::{
        Foundation::{CloseHandle, GetLastError, LocalFree, HANDLE, HLOCAL, WAIT_TIMEOUT},
        Security::{
            AdjustTokenPrivileges,
            Authorization::{ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1},
            DuplicateTokenEx, LookupPrivilegeValueW, SetTokenInformation, TokenPrimary, TokenSessionId, PSECURITY_DESCRIPTOR,
            SECURITY_ATTRIBUTES, SE_ASSIGNPRIMARYTOKEN_NAME, SE_INCREASE_QUOTA_NAME, SE_PRIVILEGE_ENABLED, SE_TCB_NAME,
            TOKEN_ADJUST_PRIVILEGES, TOKEN_ALL_ACCESS, TOKEN_ASSIGN_PRIMARY, TOKEN_DUPLICATE, TOKEN_PRIVILEGES, TOKEN_QUERY,
        },
        Storage::FileSystem::PIPE_ACCESS_DUPLEX,
        System::{
            Pipes::{
                ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE,
                PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
            },
            RemoteDesktop::{
                ProcessIdToSessionId, WTSFreeMemory, WTSGetActiveConsoleSessionId, WTSQuerySessionInformationW, WTSQueryUserToken,
                WTSSessionInfoEx, WTSINFOEXW, WTS_CURRENT_SERVER_HANDLE,
            },
            Threading::{
                CreateProcessAsUserW, GetCurrentProcess, GetCurrentProcessId, OpenProcessToken, TerminateProcess, WaitForSingleObject,
                PROCESS_INFORMATION, STARTUPINFOW,
            },
        },
    },
};

use crate::platform::Platform;
pub use crate::session::{Helper, SessionPlatform};

/// The pipe both sides agree on (public: `Docs` and the installer's service config use it).
pub const PIPE: &str = r"\\.\pipe\rcli-agent-session";

impl SessionPlatform {
    /// Accepts helpers on `pipe`. Windows lets the helper type at the sign-in and UAC screens, so input is not
    /// refused at the login screen (unlike macOS).
    pub fn listen_pipe(inner: Arc<dyn Platform>, pipe: &str) -> anyhow::Result<Arc<Self>> {
        let platform = Arc::new(SessionPlatform::new(inner, true));
        let helpers = platform.helpers();
        let name = pipe.to_owned();
        std::thread::spawn(move || loop {
            match accept(&name) {
                Ok((reader, writer)) => helpers.lock().unwrap().push(Helper::new(Box::new(reader), Box::new(writer))),
                Err(e) => {
                    tracing::warn!("session pipe: {e:#}");
                    std::thread::sleep(Duration::from_secs(1));
                }
            }
        });
        Ok(platform)
    }
}

/// One accepted helper: a pipe instance waited for a client, then handed over as a reader/writer pair.
fn accept(pipe: &str) -> anyhow::Result<(File, File)> {
    unsafe {
        let name: Vec<u16> = pipe.encode_utf16().chain(std::iter::once(0)).collect();
        let (attributes, descriptor) = pipe_security()?;
        let handle = CreateNamedPipeW(
            PCWSTR(name.as_ptr()),
            PIPE_ACCESS_DUPLEX,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            PIPE_UNLIMITED_INSTANCES,
            1 << 20,
            1 << 20,
            0,
            Some(&attributes),
        );
        // The API copies the descriptor into the pipe's security; ours is no longer needed either way.
        let _ = LocalFree(Some(HLOCAL(descriptor.0)));
        anyhow::ensure!(!handle.is_invalid(), "cannot create {}: {}", pipe, std::io::Error::last_os_error());
        match ConnectNamedPipe(handle, None) {
            // ERROR_PIPE_CONNECTED: a client was already waiting, which is not a failure.
            Err(e) if e.code().0 == 0x8007_0217u32 as i32 => {}
            Err(e) => {
                let _ = CloseHandle(handle);
                return Err(e.into());
            }
            Ok(()) => {}
        }
        let file = File::from_raw_handle(handle.0);
        let writer = file.try_clone()?;
        Ok((file, writer))
    }
}

/// A DACL that lets SYSTEM and Administrators through and gives other local users read/write (the helper may run as
/// the console user). Remote clients are rejected by the pipe mode.
fn pipe_security() -> anyhow::Result<(SECURITY_ATTRIBUTES, PSECURITY_DESCRIPTOR)> {
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            w!("D:(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;WD)"),
            SDDL_REVISION_1,
            &mut descriptor,
            None,
        )?;
    }
    let attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: false.into(),
    };
    Ok((attributes, descriptor))
}

/// The helper (`rcli agent session --pipe …`): connects to the service and answers its screen calls, reconnecting
/// whenever the service restarts. `on_console` says whether this session is the one on the console.
pub fn run_helper(pipe: &str, platform: &dyn Platform, on_console: &dyn Fn() -> bool) -> ! {
    loop {
        match std::fs::OpenOptions::new().read(true).write(true).open(pipe) {
            Ok(file) => {
                let result = file
                    .try_clone()
                    .map_err(anyhow::Error::from)
                    .and_then(|writer| crate::session::answer(Box::new(file), Box::new(writer), platform, on_console));
                if let Err(e) = result {
                    tracing::debug!("session helper connection ended: {e:#}");
                }
            }
            Err(e) => tracing::debug!("session pipe {pipe}: {e}"),
        }
        std::thread::sleep(Duration::from_secs(2));
    }
}

/// Whether this helper's session is the active console session (the state the daemon shows).
pub fn on_console() -> bool {
    unsafe {
        let mut session = 0u32;
        ProcessIdToSessionId(GetCurrentProcessId(), &mut session).is_ok() && session == WTSGetActiveConsoleSessionId()
    }
}

/// Starts a helper in the active console session now, and again whenever the session or its user changes. Polls
/// instead of `WTSRegisterSessionNotification`: that needs a window and a message loop, which a service does not have.
pub fn watch_helpers(exe: PathBuf, pipe: String) {
    std::thread::spawn(move || {
        let mut current: Option<HelperProcess> = None;
        loop {
            let target = console_target();
            let stale = match (&current, &target) {
                (None, None) => false,
                (Some(process), Some(target)) => process.target != *target || !process.alive(),
                _ => true,
            };
            if stale {
                if let Some(mut process) = current.take() {
                    process.stop();
                }
                if let Some(target) = target {
                    match spawn_helper(&exe, &pipe, target.session) {
                        Ok(process) => {
                            tracing::info!(
                                "session helper for session {} ({})",
                                target.session,
                                if target.user.is_empty() { "sign-in screen" } else { &target.user }
                            );
                            current = Some(process);
                        }
                        Err(e) => tracing::warn!("cannot start the session helper in session {}: {e:#}", target.session),
                    }
                }
            }
            std::thread::sleep(Duration::from_secs(2));
        }
    });
}

/// The console session and the user logged into it ("" while nobody is, i.e. the sign-in screen).
#[derive(Clone, Debug, PartialEq, Eq)]
struct Target {
    session: u32,
    user: String,
}

fn console_target() -> Option<Target> {
    let session = unsafe { WTSGetActiveConsoleSessionId() };
    // 0xFFFFFFFF: there is no console session at all (a Server Core box, or a machine still booting).
    (session != u32::MAX).then(|| Target { session, user: console_user(session).unwrap_or_default() })
}

struct HelperProcess {
    target: Target,
    process: HANDLE,
}

impl HelperProcess {
    fn alive(&self) -> bool {
        matches!(unsafe { WaitForSingleObject(self.process, 0) }, WAIT_TIMEOUT)
    }

    fn stop(&mut self) {
        unsafe {
            let _ = TerminateProcess(self.process, 0);
            let _ = WaitForSingleObject(self.process, 5000);
            let _ = CloseHandle(self.process);
        }
        self.process = HANDLE::default();
    }
}

impl Drop for HelperProcess {
    fn drop(&mut self) {
        if !self.process.is_invalid() {
            let _ = unsafe { CloseHandle(self.process) };
        }
    }
}

/// Starts one helper as `session`'s user with `CreateProcessAsUser`, or as SYSTEM in that session when nobody is
/// logged in (the sign-in screen), which is what the winlogon token amounts to there.
fn spawn_helper(exe: &Path, pipe: &str, session: u32) -> anyhow::Result<HelperProcess> {
    unsafe {
        // Needed by WTSQueryUserToken, CreateProcessAsUser and the token duplication below.
        for privilege in [SE_TCB_NAME, SE_ASSIGNPRIMARYTOKEN_NAME, SE_INCREASE_QUOTA_NAME] {
            if let Err(e) = enable_privilege(privilege) {
                tracing::debug!("cannot enable a service privilege: {e:#}");
            }
        }
        let token = session_token(session)?;
        let command = format!("\"{}\" agent session --pipe \"{pipe}\"", exe.display());
        let mut command: Vec<u16> = command.encode_utf16().chain(std::iter::once(0)).collect();
        // A service's child would otherwise inherit the non-interactive service window station; WinSta0 is where
        // the user's desktops (Default, Winlogon, the secure desktop) live.
        let mut desktop: Vec<u16> = "winsta0\\default\0".encode_utf16().collect();
        let startup = STARTUPINFOW { cb: size_of::<STARTUPINFOW>() as u32, lpDesktop: PWSTR(desktop.as_mut_ptr()), ..Default::default() };
        let mut process = PROCESS_INFORMATION::default();
        let created = CreateProcessAsUserW(
            Some(token),
            PCWSTR::null(),
            Some(PWSTR(command.as_mut_ptr())),
            None,
            None,
            false,
            windows::Win32::System::Threading::CREATE_NO_WINDOW,
            None,
            PCWSTR::null(),
            &startup,
            &mut process,
        );
        let _ = CloseHandle(token);
        created?;
        let _ = CloseHandle(process.hThread);
        Ok(HelperProcess { target: Target { session, user: console_user(session).unwrap_or_default() }, process: process.hProcess })
    }
}

/// The token to start the helper with: a duplicate of this SYSTEM service token moved into `session`, i.e. the token
/// winlogon runs with in that session. A user token cannot open the `Winlogon` desktop or the UAC secure desktop, so
/// it would lose the lock, sign-in and UAC screens; `WTSQueryUserToken` is only the fallback for a service that is
/// not SYSTEM (a manual `agent session` from a shell, or a future non-SYSTEM install).
fn session_token(session: u32) -> anyhow::Result<HANDLE> {
    match system_token(session) {
        Ok(token) => Ok(token),
        Err(e) => {
            tracing::debug!("cannot move the service token into session {session}, trying the logged-on user: {e:#}");
            let mut token = HANDLE::default();
            unsafe { WTSQueryUserToken(session, &mut token)? };
            Ok(token)
        }
    }
}

fn system_token(session: u32) -> anyhow::Result<HANDLE> {
    unsafe {
        let mut system = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_DUPLICATE | TOKEN_QUERY | TOKEN_ASSIGN_PRIMARY | TOKEN_ADJUST_PRIVILEGES, &mut system)?;
        let mut primary = HANDLE::default();
        let result =
            DuplicateTokenEx(system, TOKEN_ALL_ACCESS, None, windows::Win32::Security::SecurityImpersonation, TokenPrimary, &mut primary);
        let _ = CloseHandle(system);
        result?;
        if let Err(e) =
            SetTokenInformation(primary, TokenSessionId, &session as *const u32 as *const core::ffi::c_void, size_of::<u32>() as u32)
        {
            let _ = CloseHandle(primary);
            return Err(e.into());
        }
        Ok(primary)
    }
}

/// Enables one privilege in this process's token; the service starts with them disabled.
fn enable_privilege(name: PCWSTR) -> anyhow::Result<()> {
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY | TOKEN_ADJUST_PRIVILEGES, &mut token)?;
        let mut luid = windows::Win32::Foundation::LUID::default();
        let looked_up = LookupPrivilegeValueW(PCWSTR::null(), name, &mut luid);
        if looked_up.is_err() {
            let _ = CloseHandle(token);
            return Err(looked_up.err().unwrap().into());
        }
        let privileges = TOKEN_PRIVILEGES {
            PrivilegeCount: 1,
            Privileges: [windows::Win32::Security::LUID_AND_ATTRIBUTES { Luid: luid, Attributes: SE_PRIVILEGE_ENABLED }],
        };
        let result = AdjustTokenPrivileges(token, false, Some(&privileges), 0, None, None);
        // AdjustTokenPrivileges "succeeds" when the privilege is not held: the last error says so. Not fatal here:
        // LocalSystem holds all three, and a missing one only makes WTSQueryUserToken fall back to our own token.
        let last = GetLastError();
        let _ = CloseHandle(token);
        result?;
        if last.0 != 0 {
            tracing::debug!("privilege {name:?} not assigned: {}", last.0);
        }
        Ok(())
    }
}

/// The user logged into `session`, if any ("" at the sign-in screen).
fn console_user(session: u32) -> Option<String> {
    unsafe {
        let mut info = windows::core::PWSTR::null();
        let mut len = 0;
        WTSQuerySessionInformationW(Some(WTS_CURRENT_SERVER_HANDLE), session, WTSSessionInfoEx, &mut info, &mut len).ok()?;
        let ex = &*(info.0 as *const WTSINFOEXW);
        let user = String::from_utf16_lossy(&ex.Data.WTSInfoExLevel1.UserName).trim_end_matches('\0').to_owned();
        WTSFreeMemory(info.0 as _);
        Some(user)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pipe_accepts_the_service_and_any_local_user() {
        let (attributes, descriptor) = pipe_security().unwrap();
        assert!(!attributes.lpSecurityDescriptor.is_null());
        assert_eq!(attributes.nLength as usize, size_of::<SECURITY_ATTRIBUTES>());
        assert!(!descriptor.0.is_null());
        unsafe {
            let _ = LocalFree(Some(HLOCAL(descriptor.0)));
        }
    }
}
