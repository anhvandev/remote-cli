//! Platform adapter for Windows 10/11, run inside the logged-in user's session (ticket 21).
//! Capture: xcap (GDI). Input: enigo. Accessibility: UI Automation. OCR: Windows.Media.Ocr.
//! Inventory, metrics and processes: sysinfo. Shell: PowerShell. PTY: ConPTY through portable-pty.
// shortcut: crates.io xcap/enigo instead of RustDesk's git scrap/enigo (ADR 0002): RustDesk's scrap needs vcpkg
// libvpx/aom/libyuv plus the hbb_common submodule. Switch when ticket 22 brings the SYSTEM service and a Windows build host.

use std::{
    cell::RefCell,
    io::Read,
    os::windows::process::CommandExt,
    process::{Command, Stdio},
    sync::{Arc, Condvar, Mutex},
    time::{Duration, Instant},
};

use enigo::{Axis, Direction, Enigo, Keyboard, Mouse, Settings};
use portable_pty::{CommandBuilder, MasterPty, PtySize};
use windows::{
    Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap},
    Media::Ocr::OcrEngine,
    Security::Cryptography::CryptographicBuffer,
    Win32::{
        Foundation::{HANDLE, HWND, POINT, RECT},
        Security::Authentication::Identity::SendSAS,
        System::{
            Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED},
            Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS},
            RemoteDesktop::{
                WTSFreeMemory, WTSGetActiveConsoleSessionId, WTSQuerySessionInformationW, WTSSessionInfoEx, WTSINFOEXW,
                WTS_CURRENT_SERVER_HANDLE,
            },
            StationsAndDesktops::{
                CloseDesktop, GetUserObjectInformationW, OpenInputDesktop, SetThreadDesktop, DESKTOP_ACCESS_FLAGS, DESKTOP_ENUMERATE,
                DESKTOP_READOBJECTS, DESKTOP_SWITCHDESKTOP, DESKTOP_WRITEOBJECTS, DF_ALLOWOTHERACCOUNTHOOK, HDESK, UOI_NAME,
            },
            Variant::VARIANT,
        },
        UI::{
            Accessibility::*,
            HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2},
            Input::KeyboardAndMouse::{
                SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_MOVE, MOUSEEVENTF_VIRTUALDESK, MOUSEINPUT,
            },
            WindowsAndMessaging::{
                GetAncestor, GetForegroundWindow, GetSystemMetrics, GetWindowRect, WindowFromPoint, GA_ROOT, SM_CXVIRTUALSCREEN,
                SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
            },
        },
    },
};

use crate::platform::*;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// Makes PowerShell's redirected output UTF-8 instead of the OEM code page.
const UTF8: &str = "[Console]::OutputEncoding=[Text.Encoding]::UTF8;";
/// PowerShell exits 1 when the last command failed; this passes a failed program's own exit code through instead.
const EXIT_CODE: &str = "if (-not $?) { if ($LASTEXITCODE) { exit $LASTEXITCODE } else { exit 1 } }";

pub struct WindowsPlatform {
    /// The Device name `rcli agent run --name` registers with.
    name: String,
    system: Mutex<Sampler>,
}

/// sysinfo state kept between samples: CPU, disk and network figures are deltas since the previous refresh.
struct Sampler {
    system: sysinfo::System,
    disks: sysinfo::Disks,
    networks: sysinfo::Networks,
    users: sysinfo::Users,
    last: Instant,
}

impl WindowsPlatform {
    pub fn new(name: impl Into<String>) -> anyhow::Result<Self> {
        // Real pixels everywhere: monitor sizes, capture, the mouse and UI Automation boxes.
        unsafe {
            let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        }
        let sampler = Sampler {
            system: sysinfo::System::new_all()?,
            disks: sysinfo::Disks::new_with_refreshed_list()?,
            networks: sysinfo::Networks::new_with_refreshed_list()?,
            users: sysinfo::Users::new_with_refreshed_list()?,
            last: Instant::now(),
        };
        Ok(Self { name: name.into(), system: Mutex::new(sampler) })
    }

    fn display(&self, index: u32) -> anyhow::Result<DisplayInfo> {
        self.displays()?.into_iter().find(|d| d.index == index).ok_or_else(|| anyhow::anyhow!("no display {index}"))
    }
}

impl Platform for WindowsPlatform {
    fn hostname(&self) -> String {
        self.name.clone()
    }

    fn displays(&self) -> anyhow::Result<Vec<DisplayInfo>> {
        Ok(xcap::Monitor::all()?
            .iter()
            .zip(0..)
            .map(|(m, index)| DisplayInfo {
                index,
                x: m.x().unwrap_or(0),
                y: m.y().unwrap_or(0),
                width: m.width().unwrap_or(0),
                height: m.height().unwrap_or(0),
                scale: m.scale_factor().unwrap_or(1.0).into(),
                primary: m.is_primary().unwrap_or(false),
            })
            .collect())
    }

    fn capture(&self, display: u32) -> anyhow::Result<Frame> {
        // The input desktop changes on lock, login and a UAC prompt; GDI captures whatever desktop this thread is
        // on, so follow the input desktop first (the session helper runs as SYSTEM in the console session).
        switch_to_input_desktop()?;
        let monitor = xcap::Monitor::all()?.into_iter().nth(display as usize).ok_or_else(|| anyhow::anyhow!("no display {display}"))?;
        let image = monitor.capture_image()?;
        Ok(Frame { width: image.width(), height: image.height(), rgba: image.into_raw() })
    }

    fn input(&self, input: Input) -> anyhow::Result<()> {
        // Ctrl+Alt+Del is the secure attention sequence: Windows produces it only through SendSAS, from the SYSTEM
        // helper in the console session (`asuser = false`), and only when the software-SAS policy allows it.
        if input == Input::Sas {
            unsafe { SendSAS(false) };
            return Ok(());
        }
        switch_to_input_desktop()?;
        // A fresh Enigo per primitive: held keys must stay down between KeyDown and KeyUp calls.
        let mut enigo = Enigo::new(&Settings { release_keys_when_dropped: false, ..Default::default() })?;
        match input {
            Input::Move { display, x, y } => {
                let d = self.display(display)?;
                move_pointer(d.x + x as i32, d.y + y as i32)?;
            }
            Input::Press { button, down } => {
                let button = match button {
                    Button::Left => enigo::Button::Left,
                    Button::Right => enigo::Button::Right,
                    Button::Middle => enigo::Button::Middle,
                };
                enigo.button(button, if down { Direction::Press } else { Direction::Release })?;
            }
            Input::Scroll { dx, dy } => {
                if dy != 0 {
                    enigo.scroll(dy, Axis::Vertical)?;
                }
                if dx != 0 {
                    enigo.scroll(dx, Axis::Horizontal)?;
                }
            }
            Input::Text(text) => enigo.text(&text)?,
            Input::Key(name) => enigo.key(key(&name)?, Direction::Click)?,
            Input::KeyDown(name) => enigo.key(key(&name)?, Direction::Press)?,
            Input::KeyUp(name) => enigo.key(key(&name)?, Direction::Release)?,
            Input::Sas => unreachable!("handled before the input desktop switch"),
        }
        Ok(())
    }

    // shortcut: only the foreground window (or, if it is not on `display`, the window at the display's centre);
    // other windows on the display are not walked. Walk all top-level windows if Marks miss things there.
    fn accessibility(&self, display: u32) -> anyhow::Result<Option<Vec<UiElement>>> {
        switch_to_input_desktop()?;
        let d = self.display(display)?;
        com_init();
        unsafe {
            let uia: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)?;
            let mut window = GetForegroundWindow();
            if window.is_invalid() || !overlaps(window_rect(window), &d) {
                let centre = POINT { x: d.x + d.width as i32 / 2, y: d.y + d.height as i32 / 2 };
                window = GetAncestor(WindowFromPoint(centre), GA_ROOT);
            }
            if window.is_invalid() {
                return Ok(Some(vec![]));
            }
            let root = uia.ElementFromHandle(window)?;
            let cache = uia.CreateCacheRequest()?;
            for property in [UIA_ControlTypePropertyId, UIA_NamePropertyId, UIA_BoundingRectanglePropertyId] {
                cache.AddProperty(property)?;
            }
            let visible = uia.CreatePropertyCondition(UIA_IsOffscreenPropertyId, &VARIANT::from(false))?;
            let found = root.FindAllBuildCache(TreeScope_Subtree, &visible, &cache)?;
            let mut elements = vec![];
            for i in 0..found.Length()? {
                let e = found.GetElement(i)?;
                let Some(role) = role(e.CachedControlType()?) else { continue };
                let text = e.CachedName()?.to_string();
                if role == "text" && text.trim().is_empty() {
                    continue;
                }
                if let Some((x, y, width, height)) = clip(e.CachedBoundingRectangle()?, &d) {
                    elements.push(UiElement { role: role.into(), text, x, y, width, height });
                }
            }
            Ok(Some(elements))
        }
    }

    fn recognize_text(&self, frame: &Frame) -> anyhow::Result<Vec<UiElement>> {
        com_init();
        let engine = OcrEngine::TryCreateFromUserProfileLanguages()?;
        // OCR refuses images larger than MaxImageDimension (about 2600 px): scale down, then scale the boxes back up.
        let max = OcrEngine::MaxImageDimension()?;
        let scale = (frame.width.max(frame.height) as f32 / max as f32).max(1.0);
        let image = image::RgbaImage::from_raw(frame.width, frame.height, frame.rgba.clone())
            .ok_or_else(|| anyhow::anyhow!("frame size mismatch"))?;
        let image = if scale > 1.0 {
            let (w, h) = ((frame.width as f32 / scale) as u32, (frame.height as f32 / scale) as u32);
            image::imageops::resize(&image, w, h, image::imageops::FilterType::Triangle)
        } else {
            image
        };
        let (w, h) = image.dimensions();
        let mut bgra = image.into_raw();
        bgra.as_chunks_mut::<4>().0.iter_mut().for_each(|p| p.swap(0, 2));
        let buffer = CryptographicBuffer::CreateFromByteArray(&bgra)?;
        let bitmap = SoftwareBitmap::CreateCopyFromBuffer(&buffer, BitmapPixelFormat::Bgra8, w as i32, h as i32)?;
        let result = engine.RecognizeAsync(&bitmap)?.join()?;
        let mut lines = vec![];
        for line in result.Lines()? {
            let (mut x1, mut y1, mut x2, mut y2) = (f32::MAX, f32::MAX, 0f32, 0f32);
            for word in line.Words()? {
                let r = word.BoundingRect()?;
                (x1, y1, x2, y2) = (x1.min(r.X), y1.min(r.Y), x2.max(r.X + r.Width), y2.max(r.Y + r.Height));
            }
            if x2 > x1 && y2 > y1 {
                let px = |v: f32| (v * scale).round() as u32;
                let (x, y) = (px(x1), px(y1));
                let text = line.Text()?.to_string();
                lines.push(UiElement { role: "text".into(), text, x, y, width: px(x2) - x, height: px(y2) - y });
            }
        }
        Ok(lines)
    }

    fn session_state(&self) -> anyhow::Result<SessionState> {
        let (user, locked) = console_session()?;
        Ok(match (user.is_empty(), locked) {
            (true, _) => SessionState::LoginScreen,
            (false, true) => SessionState::Locked,
            (false, false) => SessionState::LoggedIn,
        })
    }

    fn inventory(&self) -> anyhow::Result<Inventory> {
        let s = self.system.lock().unwrap();
        let interfaces = s
            .networks
            .iter()
            .map(|(name, n)| NetworkInterface {
                name: name.to_string_lossy().into(),
                mac: n.mac_address().to_string(),
                ips: n.ip_networks().iter().map(|ip| format!("{}/{}", ip.addr, ip.prefix)).collect(),
            })
            .collect();
        // shortcut: no GPUs or serial number (they need WMI); add them when the Dashboard shows them.
        Ok(Inventory {
            os: sysinfo::System::name().unwrap_or_else(|_| "Windows".into()),
            os_version: sysinfo::System::long_os_version().unwrap_or_default(),
            hostname: sysinfo::System::host_name().unwrap_or_default(),
            cpu_model: s.system.cpus().first().map(|c| c.brand().trim().to_owned()).unwrap_or_default(),
            cpu_cores: s.system.cpus().len() as u32,
            ram_bytes: s.system.total_memory(),
            disks: s
                .disks
                .iter()
                .map(|d| Disk {
                    name: d.name().to_string_lossy().into(),
                    mount: d.mount_point().to_string_lossy().into(),
                    total_bytes: d.total_space(),
                })
                .collect(),
            network_interfaces: interfaces,
            displays: xcap::Monitor::all()?
                .iter()
                .map(|m| Display { name: m.name().unwrap_or_default(), width: m.width().unwrap_or(0), height: m.height().unwrap_or(0) })
                .collect(),
            ..Default::default()
        })
    }

    fn metrics(&self) -> anyhow::Result<Resources> {
        let mut guard = self.system.lock().unwrap();
        let s = &mut *guard;
        if s.last.elapsed() < sysinfo::MINIMUM_CPU_UPDATE_INTERVAL {
            std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
        }
        let secs = s.last.elapsed().as_secs_f64();
        s.last = Instant::now();
        s.system.refresh_cpu_usage();
        s.system.refresh_memory();
        s.disks.refresh(true);
        s.networks.refresh(true);
        let per_sec = |bytes: u64| Some((bytes as f64 / secs) as u64);
        let (user, _) = console_session().unwrap_or_default();
        let mut power = SYSTEM_POWER_STATUS::default();
        // BatteryFlag 128 = no battery; 255 = unknown percentage.
        let battery =
            unsafe { GetSystemPowerStatus(&mut power) }.is_ok() && power.BatteryFlag & 128 == 0 && power.BatteryLifePercent <= 100;
        Ok(Resources {
            cpu_percent: Some(s.system.global_cpu_usage().into()),
            ram_used_bytes: Some(s.system.used_memory()),
            disk_used_percent: s
                .disks
                .iter()
                .filter(|d| d.total_space() > 0)
                .map(|d| {
                    let used = d.total_space() - d.available_space();
                    (d.mount_point().to_string_lossy().into(), used as f64 * 100.0 / d.total_space() as f64)
                })
                .collect(),
            disk_read_bytes_per_sec: per_sec(s.disks.iter().map(|d| d.usage().read_bytes).sum()),
            disk_write_bytes_per_sec: per_sec(s.disks.iter().map(|d| d.usage().written_bytes).sum()),
            net_up_bytes_per_sec: per_sec(s.networks.values().map(|n| n.transmitted()).sum()),
            net_down_bytes_per_sec: per_sec(s.networks.values().map(|n| n.received()).sum()),
            uptime_secs: sysinfo::System::uptime().ok(),
            logged_in_user: Some(user),
            battery_percent: battery.then_some(power.BatteryLifePercent.into()),
            battery_charging: battery.then_some(power.ACLineStatus == 1),
            ..Default::default()
        })
    }

    fn processes(&self) -> anyhow::Result<Vec<Process>> {
        let mut guard = self.system.lock().unwrap();
        let s = &mut *guard;
        s.system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
        Ok(s.system
            .processes()
            .values()
            .map(|p| Process {
                pid: p.pid().as_u32(),
                name: p.name().to_string_lossy().into(),
                user: p.user_id().and_then(|id| s.users.get_user_by_id(id)).map(|u| u.name().to_owned()).unwrap_or_default(),
                cpu_percent: p.cpu_usage().into(),
                memory_bytes: p.memory(),
            })
            .collect())
    }

    // shortcut: runs as the account the Agent runs as, whatever `account` says (a user-session Agent cannot switch
    // accounts); ticket 22's SYSTEM service starts it as the Terminal User or SYSTEM.
    fn shell(&self, command: &str, _run_as: Account, timeout: Duration) -> anyhow::Result<ShellOutput> {
        let mut child = Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-EncodedCommand", &encode(&format!("{UTF8}{command}\n{EXIT_CODE}"))])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()?;
        let read = |mut r: Box<dyn Read + Send>| {
            std::thread::spawn(move || {
                let mut out = vec![];
                let _ = r.read_to_end(&mut out);
                String::from_utf8_lossy(&out).into_owned()
            })
        };
        let stdout = read(Box::new(child.stdout.take().unwrap()));
        let stderr = read(Box::new(child.stderr.take().unwrap()));
        let start = Instant::now();
        let (code, timed_out) = loop {
            if let Some(status) = child.try_wait()? {
                break (status.code().unwrap_or(-1), false);
            }
            if start.elapsed() >= timeout {
                kill_tree(child.id());
                break (child.wait()?.code().unwrap_or(-1), true);
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        let join = |t: std::thread::JoinHandle<String>| t.join().unwrap_or_default();
        Ok(ShellOutput { exit_code: code, stdout: join(stdout), stderr: join(stderr), timed_out })
    }

    // shortcut: same account rule as `shell`.
    fn pty(&self, _run_as: Account, cols: u16, rows: u16) -> anyhow::Result<Pty> {
        let pair = portable_pty::native_pty_system().openpty(size(cols, rows))?;
        let mut cmd = CommandBuilder::new("powershell.exe");
        cmd.arg("-NoLogo");
        if let Some(home) = std::env::var_os("USERPROFILE") {
            cmd.cwd(home);
        }
        let mut child = pair.slave.spawn_command(cmd)?;
        drop(pair.slave);
        let output = pair.master.try_clone_reader()?;
        let input = pair.master.take_writer()?;
        let pid = child.process_id();
        let master = Arc::new(Mutex::new(Some(pair.master)));
        let exit = Arc::new((Mutex::new(None), Condvar::new()));
        let (m, e) = (master.clone(), exit.clone());
        // ConPTY never ends the output on its own: closing the pseudo console once the shell exits makes the EOF.
        std::thread::spawn(move || {
            let code = child.wait().map(|s| s.exit_code() as i32).unwrap_or(-1);
            m.lock().unwrap().take();
            *e.0.lock().unwrap() = Some(code);
            e.1.notify_all();
        });
        Ok(Pty { output, input, control: Box::new(ConPty { master, exit, pid }) })
    }

    fn power(&self, action: PowerAction) -> anyhow::Result<()> {
        let (program, args): (&str, &[&str]) = match action {
            PowerAction::Restart => ("shutdown.exe", &["/r", "/t", "0"]),
            PowerAction::Shutdown => ("shutdown.exe", &["/s", "/t", "0"]),
            // .NET enables the shutdown privilege that SetSuspendState needs.
            PowerAction::Sleep => (
                "powershell.exe",
                &["-NoProfile", "-Command", "Add-Type -AssemblyName System.Windows.Forms;[void][System.Windows.Forms.Application]::SetSuspendState('Suspend',$false,$false)"],
            ),
            PowerAction::Lock => ("rundll32.exe", &["user32.dll,LockWorkStation"]),
        };
        let out = Command::new(program).args(args).creation_flags(CREATE_NO_WINDOW).output()?;
        anyhow::ensure!(out.status.success(), "{action:?} failed: {}", String::from_utf8_lossy(&out.stderr).trim());
        Ok(())
    }

    fn send_wake(&self, mac: [u8; 6]) -> anyhow::Result<()> {
        let socket = std::net::UdpSocket::bind("0.0.0.0:0")?;
        socket.set_broadcast(true)?;
        socket.send_to(&magic_packet(mac), "255.255.255.255:9")?;
        Ok(())
    }

    /// A running .exe cannot be overwritten but can be renamed: move it aside, write the new one, start it with the
    /// same arguments and exit.
    // shortcut: restarts as a plain child process; ticket 22's service restarts through the service manager instead.
    fn install_update(&self, _version: &str, build: &std::path::Path) -> anyhow::Result<()> {
        let exe = std::env::current_exe()?;
        let old = exe.with_extension("old.exe");
        let _ = std::fs::remove_file(&old);
        std::fs::rename(&exe, &old)?;
        if let Err(e) = std::fs::copy(build, &exe) {
            let _ = std::fs::rename(&old, &exe);
            return Err(e.into());
        }
        // This process exits below, so nobody else is left to delete the download.
        let _ = std::fs::remove_file(build);
        Command::new(&exe).args(std::env::args_os().skip(1)).spawn()?;
        std::process::exit(0);
    }

    // shortcut: no indicator of its own; in the desktop build the GUI window is the indicator (gui::WindowPlatform).
    // Ticket 22 adds one that also shows at the lock and login screens.
    fn set_control_indicator(&self, _on: bool) -> anyhow::Result<()> {
        Ok(())
    }

    /// Without the GUI there is nobody to ask: reject (the desktop build's window asks instead).
    fn ask_root(&self, _requester: &str, _timeout: Duration) -> anyhow::Result<bool> {
        Ok(false)
    }

    fn ask_accept(&self, _requester: &str, _timeout: Duration) -> anyhow::Result<bool> {
        Ok(false)
    }
}

struct ConPty {
    master: Arc<Mutex<Option<Box<dyn MasterPty + Send>>>>,
    exit: Arc<(Mutex<Option<i32>>, Condvar)>,
    pid: Option<u32>,
}

impl PtyControl for ConPty {
    fn resize(&mut self, cols: u16, rows: u16) -> anyhow::Result<()> {
        match self.master.lock().unwrap().as_ref() {
            Some(m) => m.resize(size(cols, rows)),
            None => Ok(()),
        }
    }

    fn wait(&mut self) -> anyhow::Result<i32> {
        let (lock, cvar) = &*self.exit;
        Ok(cvar.wait_while(lock.lock().unwrap(), |code| code.is_none()).unwrap().unwrap())
    }

    fn kill(&mut self) -> anyhow::Result<()> {
        if let Some(pid) = self.pid {
            kill_tree(pid);
        }
        Ok(())
    }
}

/// Moves the pointer to virtual-screen pixel (x, y). enigo's absolute move only spans the primary display.
fn move_pointer(x: i32, y: i32) -> anyhow::Result<()> {
    unsafe {
        let (vx, vy) = (GetSystemMetrics(SM_XVIRTUALSCREEN), GetSystemMetrics(SM_YVIRTUALSCREEN));
        let (vw, vh) = (GetSystemMetrics(SM_CXVIRTUALSCREEN), GetSystemMetrics(SM_CYVIRTUALSCREEN));
        let mi = MOUSEINPUT {
            dx: normalize(x - vx, vw),
            dy: normalize(y - vy, vh),
            dwFlags: MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
            ..Default::default()
        };
        let input = INPUT { r#type: INPUT_MOUSE, Anonymous: INPUT_0 { mi } };
        anyhow::ensure!(SendInput(&[input], size_of::<INPUT>() as i32) == 1, "SendInput refused the pointer move");
    }
    Ok(())
}

/// Pixel `p` of a `size`-pixel span as SendInput's 0..65535 absolute coordinate. Windows maps n back to
/// (n * size) >> 16, so the smallest n with n * size >= p * 65536 lands exactly on `p`.
fn normalize(p: i32, size: i32) -> i32 {
    ((p as i64 * 65536 + size as i64 - 1) / size.max(1) as i64) as i32
}

/// Wake-on-LAN: six 0xFF bytes, then the MAC sixteen times.
fn magic_packet(mac: [u8; 6]) -> Vec<u8> {
    [vec![0xFF; 6], mac.repeat(16)].concat()
}

fn size(cols: u16, rows: u16) -> PtySize {
    PtySize { rows, cols, pixel_width: 0, pixel_height: 0 }
}

/// A script for `powershell -EncodedCommand` (base64 of UTF-16LE), so quotes in it survive the command line.
fn encode(script: &str) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(script.encode_utf16().flat_map(u16::to_le_bytes).collect::<Vec<u8>>())
}

/// Ends `pid` and everything it started.
fn kill_tree(pid: u32) {
    let _ = Command::new("taskkill.exe").args(["/T", "/F", "/PID", &pid.to_string()]).creation_flags(CREATE_NO_WINDOW).output();
}

/// COM on this thread (UI Automation, WinRT OCR); calls come from arbitrary blocking threads.
fn com_init() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
}

thread_local! {
    /// The desktop this thread was last put on, kept open: a thread whose desktop handle is closed points at a
    /// desktop nobody can use, so the handle lives as long as the helper thread does.
    static CURRENT_DESKTOP: RefCell<Option<(HDESK, String)>> = const { RefCell::new(None) };
}

/// Puts the calling thread on the input desktop. The desktop changes on lock (`Winlogon`), login and a UAC prompt
/// (`Secure Desktop`); capture, input and UI Automation only see the desktop the thread is on (ADR 0001, ticket 22).
fn switch_to_input_desktop() -> anyhow::Result<()> {
    unsafe {
        let access = DESKTOP_ACCESS_FLAGS(DESKTOP_READOBJECTS.0 | DESKTOP_WRITEOBJECTS.0 | DESKTOP_ENUMERATE.0 | DESKTOP_SWITCHDESKTOP.0);
        let desktop = OpenInputDesktop(DF_ALLOWOTHERACCOUNTHOOK, false, access)?;
        let mut name = [0u16; 128];
        let mut needed = 0u32;
        if let Err(e) = GetUserObjectInformationW(
            HANDLE(desktop.0),
            UOI_NAME,
            Some(name.as_mut_ptr() as *mut core::ffi::c_void),
            (name.len() * 2) as u32,
            Some(&mut needed),
        ) {
            let _ = CloseDesktop(desktop);
            return Err(e.into());
        }
        let name = String::from_utf16_lossy(&name[..name.iter().position(|&c| c == 0).unwrap_or(name.len())]);
        let same = CURRENT_DESKTOP.with(|slot| slot.borrow().as_ref().is_some_and(|(_, current)| *current == name));
        if same {
            let _ = CloseDesktop(desktop);
            return Ok(());
        }
        SetThreadDesktop(desktop)?;
        let previous = CURRENT_DESKTOP.with(|slot| slot.borrow_mut().replace((desktop, name)));
        if let Some((old, _)) = previous {
            let _ = CloseDesktop(old);
        }
    }
    Ok(())
}

/// Whether `exe` is the installed service binary (`%ProgramFiles%\rcli\rcli.exe`), which restarts through the SCM
/// rather than as a child of itself.
pub fn installed_service(exe: &std::path::Path) -> bool {
    let name = |p: &std::path::Path| p.file_name().map(|n| n.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    name(exe) == "rcli.exe" && exe.parent().is_some_and(|dir| name(dir) == "rcli")
}

/// The user logged in on the console, if any: the Terminal User an install records.
pub fn active_user() -> Option<String> {
    console_session().ok().and_then(|(user, _)| (!user.is_empty()).then_some(user))
}

/// The console session's user ("" = nobody logged in) and whether it is locked.
fn console_session() -> anyhow::Result<(String, bool)> {
    unsafe {
        let mut info = windows::core::PWSTR::null();
        let mut len = 0;
        WTSQuerySessionInformationW(
            Some(WTS_CURRENT_SERVER_HANDLE),
            WTSGetActiveConsoleSessionId(),
            WTSSessionInfoEx,
            &mut info,
            &mut len,
        )?;
        let ex = &*(info.0 as *const WTSINFOEXW);
        let level1 = &ex.Data.WTSInfoExLevel1;
        let user = String::from_utf16_lossy(&level1.UserName).trim_end_matches('\0').to_owned();
        // WTS_SESSIONSTATE_LOCK = 0.
        let locked = level1.SessionFlags == 0;
        WTSFreeMemory(info.0 as _);
        Ok((user, locked))
    }
}

fn window_rect(window: HWND) -> RECT {
    let mut r = RECT::default();
    unsafe {
        let _ = GetWindowRect(window, &mut r);
    }
    r
}

fn overlaps(r: RECT, d: &DisplayInfo) -> bool {
    r.left < d.x + d.width as i32 && r.right > d.x && r.top < d.y + d.height as i32 && r.bottom > d.y
}

/// Part of the screen rect `r` on display `d`, in that display's real pixels; `None` if none of it is.
fn clip(r: RECT, d: &DisplayInfo) -> Option<(u32, u32, u32, u32)> {
    let (right, bottom) = (d.x + d.width as i32, d.y + d.height as i32);
    let (x1, y1, x2, y2) = (r.left.max(d.x), r.top.max(d.y), r.right.min(right), r.bottom.min(bottom));
    (x2 > x1 && y2 > y1).then(|| ((x1 - d.x) as u32, (y1 - d.y) as u32, (x2 - x1) as u32, (y2 - y1) as u32))
}

/// Targetable UI Automation control types; everything else (panes, groups, images...) makes no Mark.
#[allow(non_upper_case_globals)]
fn role(control_type: UIA_CONTROLTYPE_ID) -> Option<&'static str> {
    Some(match control_type {
        UIA_ButtonControlTypeId | UIA_SplitButtonControlTypeId => "button",
        UIA_EditControlTypeId | UIA_DocumentControlTypeId => "field",
        UIA_HyperlinkControlTypeId => "link",
        UIA_TextControlTypeId => "text",
        UIA_CheckBoxControlTypeId => "checkbox",
        UIA_RadioButtonControlTypeId => "radio",
        UIA_ComboBoxControlTypeId => "combobox",
        UIA_ListItemControlTypeId | UIA_TreeItemControlTypeId => "item",
        UIA_MenuItemControlTypeId => "menuitem",
        UIA_TabItemControlTypeId => "tab",
        _ => return None,
    })
}

/// Key names the Executor sends (`"a"`, `"enter"`, `"ctrl"` from `ctrl+shift+t`) as enigo keys.
fn key(name: &str) -> anyhow::Result<enigo::Key> {
    use enigo::Key::*;
    let lower = name.to_lowercase();
    Ok(match lower.as_str() {
        "enter" | "return" => Return,
        "tab" => Tab,
        "esc" | "escape" => Escape,
        "backspace" => Backspace,
        "delete" | "del" => Delete,
        "insert" => Insert,
        "space" => Space,
        "home" => Home,
        "end" => End,
        "pageup" => PageUp,
        "pagedown" => PageDown,
        "up" => UpArrow,
        "down" => DownArrow,
        "left" => LeftArrow,
        "right" => RightArrow,
        "ctrl" | "control" => Control,
        "shift" => Shift,
        "alt" | "option" => Alt,
        "win" | "meta" | "cmd" | "super" => Meta,
        "capslock" => CapsLock,
        f if f.len() > 1 && f.starts_with('f') && f[1..].parse::<u8>().is_ok_and(|n| (1..=12).contains(&n)) => {
            [F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12][f[1..].parse::<usize>().unwrap() - 1]
        }
        _ if name.chars().count() == 1 => Unicode(name.chars().next().unwrap()),
        _ => anyhow::bail!("unknown key {name:?}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn executor_key_names_map_to_keys() {
        assert_eq!(key("Enter").unwrap(), enigo::Key::Return);
        assert_eq!(key("ctrl").unwrap(), enigo::Key::Control);
        assert_eq!(key("f5").unwrap(), enigo::Key::F5);
        assert_eq!(key("a").unwrap(), enigo::Key::Unicode('a'));
        assert!(key("nosuchkey").is_err());
    }

    #[test]
    fn every_pixel_survives_absolute_pointer_coordinates() {
        for size in [1024, 1920, 3840, 1920 + 2560 + 3840] {
            for p in 0..size {
                assert_eq!((normalize(p, size) as i64 * size as i64) >> 16, p as i64, "pixel {p} of {size}");
            }
        }
    }

    #[test]
    fn scripts_are_encoded_as_utf16le_base64() {
        // powershell -EncodedCommand "ZQBjAGgAbwAgACIAaQAiAA==" runs: echo "i"
        assert_eq!(encode("echo \"i\""), "ZQBjAGgAbwAgACIAaQAiAA==");
    }

    #[test]
    fn magic_packet_is_ff_then_mac_sixteen_times() {
        let p = magic_packet([1, 2, 3, 4, 5, 6]);
        assert_eq!(p.len(), 102);
        assert_eq!(&p[..6], &[0xFF; 6]);
        assert_eq!(&p[96..], &[1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn element_boxes_are_clipped_to_their_display() {
        let d = DisplayInfo { index: 1, x: 1920, y: 0, width: 1280, height: 1024, scale: 1.0, primary: false };
        assert_eq!(clip(RECT { left: 1900, top: 10, right: 2000, bottom: 30 }, &d), Some((0, 10, 80, 20)));
        assert_eq!(clip(RECT { left: 0, top: 0, right: 100, bottom: 100 }, &d), None);
    }
}
