# Lock screen / login screen control — research notes

Status: partial. Windows and part of macOS verified against vendor docs; everything else UNVERIFIED.

## Windows (verified)

- WinSta0 has three desktops: Default, ScreenSaver, Winlogon. The system switches to Winlogon for logon, Ctrl+Alt+Del and UAC. Only a narrow set of accounts, including LocalSystem, can access it. https://learn.microsoft.com/en-us/windows/win32/winstation/desktops
- Since Vista, services run in Session 0 and cannot interact with the user. The recommended approach is a helper process launched with `CreateProcessAsUser` in the user's session, talking to the service over IPC. https://learn.microsoft.com/en-us/windows/win32/services/interactive-services
- `CreateProcessAsUser` usually needs `SE_INCREASE_QUOTA_NAME`, sometimes also `SE_ASSIGNPRIMARYTOKEN_NAME`. The new process's session comes from the token (changeable with `SetTokenInformation`). Set `lpDesktop="winsta0\default"` for interactive use. https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-createprocessasusera
- `WTSGetActiveConsoleSessionId` returns the session attached to the physical console, or 0xFFFFFFFF while a session switch is in progress. https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-wtsgetactiveconsolesessionid
- `OpenInputDesktop` opens the desktop currently receiving input; `SwitchDesktop` changes desktops. https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-openinputdesktop
- DXGI `DuplicateOutput`:
  - returns `E_ACCESSDENIED` without rights to the current desktop ("only LOCAL_SYSTEM can access the secure desktop");
  - returns `DXGI_ERROR_UNSUPPORTED` on a desktop without DWM, in which case wait for `EVENT_SYSTEM_DESKTOPSWITCH` and retry;
  - allows at most 4 duplicating processes per session by default.

  https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_2/nf-dxgi1_2-idxgioutput1-duplicateoutput
- `SendInput` is blocked silently by UIPI. It can only inject into processes at the same or a lower integrity level. https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendinput
- `SendSAS` (simulates Ctrl+Alt+Del), supported since Windows 7:
  - the caller must be a service, or a `uiAccess=true` app that is Authenticode-signed and installed in Program Files or system32;
  - the GPO "Disable or enable software Secure Attention Sequence" must allow it.

  https://learn.microsoft.com/en-us/windows/win32/api/sas/nf-sas-sendsas
- UAC: `PromptOnSecureDesktop=1` is the default. `EnableUIADesktopToggle` (off by default) lets UIAccess apps show the prompt on the normal desktop. https://learn.microsoft.com/en-us/windows/security/application-security/application-control/user-account-control/settings-and-configuration

## macOS (partially verified)

- Pre-login agents use `LimitLoadToSessionType=LoginWindow` and run as root in the loginwindow context. With both `LoginWindow` and `Aqua`, the login-window instance is terminated and a new one starts when the user logs in. Connecting to the window server before login requires this kind of agent. https://developer.apple.com/library/archive/technotes/tn2083/_index.html

## UNVERIFIED (not yet researched)

- macOS: ScreenCaptureKit/CGEvent at the login window, TCC permissions, FileVault pre-boot unlock.
- Linux X11: capture and input on the display manager's X server (GDM/LightDM/SDDM).
- Linux Wayland: uinput at greeter and lock screens; portal ScreenCast + `restore_token` while locked or at the greeter; DRM/KMS (kmsgrab) capture and its limits.
- Windows: default value of the software SAS GPO; `SetThreadDesktop` docs.
- Prior art: RustDesk and MeshCentral feature coverage, licenses, paid-only features.
