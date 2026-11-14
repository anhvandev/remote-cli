//! macOS adapter of the Platform port for a logged-in session (ticket 23). The lock screen and the login window
//! (root daemon, LoginWindow agent) come with ticket 24.
//! Needs the Screen Recording grant for `capture` and the Accessibility grant for `input` and `accessibility`.

use std::{
    ffi::c_void,
    io::Read,
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    os::unix::process::{CommandExt, ExitStatusExt},
    process::{Command, Stdio},
    ptr::NonNull,
    sync::Mutex,
    time::{Duration, Instant},
};

use enigo::{Axis, Coordinate, Direction, Keyboard, Mouse};
use objc2_application_services::{AXError, AXUIElement, AXValue, AXValueType};
use objc2_core_foundation::{CFArray, CFBoolean, CFData, CFNumber, CFRetained, CFString, CFType, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{
    CGBitmapInfo, CGColorRenderingIntent, CGColorSpace, CGDataProvider, CGDirectDisplayID, CGDisplayBounds,
    CGDisplayCopyDisplayMode, CGDisplayMode, CGError, CGGetActiveDisplayList, CGImage, CGImageAlphaInfo,
    CGSessionCopyCurrentDictionary,
};
use objc2::AnyThread;
use objc2_foundation::{NSArray, NSDictionary};
use objc2_vision::{VNImageRequestHandler, VNRecognizeTextRequest, VNRequest};

use crate::platform::{
    Button, DisplayInfo, Disk, Display, Frame, Input, Inventory, NetworkInterface, Platform, PowerAction, Process, Pty,
    PtyControl, Resources, Account, SessionState, ShellOutput, UiElement,
};

pub struct MacPlatform {
    hostname: String,
    /// Created on first input: creating it asks for the Accessibility grant.
    enigo: Mutex<Option<enigo::Enigo>>,
    sys: Mutex<Sys>,
}

struct Sys {
    system: sysinfo::System,
    networks: sysinfo::Networks,
    networks_at: Instant,
    users: sysinfo::Users,
}

impl MacPlatform {
    /// `hostname` is the Device name reported at registration.
    pub fn new(hostname: impl Into<String>) -> anyhow::Result<Self> {
        let sys = Sys {
            system: sysinfo::System::new()?,
            networks: sysinfo::Networks::new_with_refreshed_list()?,
            networks_at: Instant::now(),
            users: sysinfo::Users::new_with_refreshed_list()?,
        };
        Ok(Self { hostname: hostname.into(), enigo: Mutex::new(None), sys: Mutex::new(sys) })
    }

    fn with_enigo(&self, f: impl FnOnce(&mut enigo::Enigo) -> enigo::InputResult<()>) -> anyhow::Result<()> {
        let mut slot = self.enigo.lock().unwrap();
        if slot.is_none() {
            let enigo = enigo::Enigo::new(&enigo::Settings::default())
                .map_err(|e| anyhow::anyhow!("cannot send input ({e}): grant rcli Accessibility in System Settings"))?;
            *slot = Some(enigo);
        }
        f(slot.as_mut().unwrap()).map_err(|e| anyhow::anyhow!("input failed: {e}"))
    }
}

/// Active displays, the main one first (`CGGetActiveDisplayList` order), with each one's id.
fn screens() -> anyhow::Result<Vec<(CGDirectDisplayID, DisplayInfo)>> {
    let mut ids = [0 as CGDirectDisplayID; 32];
    let mut count = 0u32;
    let err = unsafe { CGGetActiveDisplayList(ids.len() as u32, ids.as_mut_ptr(), &mut count) };
    anyhow::ensure!(err == CGError::Success, "CGGetActiveDisplayList failed: {err:?}");
    Ok(ids[..count as usize]
        .iter()
        .enumerate()
        .map(|(index, &id)| {
            let bounds = CGDisplayBounds(id);
            let mode = CGDisplayCopyDisplayMode(id);
            let pixel_width = CGDisplayMode::pixel_width(mode.as_deref()) as u32;
            let pixel_height = CGDisplayMode::pixel_height(mode.as_deref()) as u32;
            let scale = if bounds.size.width > 0.0 { pixel_width as f64 / bounds.size.width } else { 1.0 };
            // x/y: origin in global points, the space CGEvent and AX positions use.
            let info = DisplayInfo {
                index: index as u32,
                x: bounds.origin.x as i32,
                y: bounds.origin.y as i32,
                width: pixel_width,
                height: pixel_height,
                scale,
                primary: index == 0,
            };
            (id, info)
        })
        .collect())
}

fn screen(display: u32) -> anyhow::Result<(CGDirectDisplayID, DisplayInfo)> {
    screens()?.into_iter().find(|(_, d)| d.index == display).ok_or_else(|| anyhow::anyhow!("no display {display}"))
}

/// Global point of real pixel (x, y) of `d`.
fn to_point(d: &DisplayInfo, x: u32, y: u32) -> (f64, f64) {
    (d.x as f64 + x as f64 / d.scale, d.y as f64 + y as f64 / d.scale)
}

/// Box in real pixels of `d` for a rect in global points, clipped to `d`; `None` if it is not on `d`.
fn to_pixels(d: &DisplayInfo, origin: CGPoint, size: CGSize) -> Option<(u32, u32, u32, u32)> {
    let px = |v: f64, o: i32, max: u32| ((v - o as f64) * d.scale).round().clamp(0.0, max as f64) as u32;
    let (x, y) = (px(origin.x, d.x, d.width), px(origin.y, d.y, d.height));
    let (x2, y2) = (px(origin.x + size.width, d.x, d.width), px(origin.y + size.height, d.y, d.height));
    (x2 > x && y2 > y).then(|| (x, y, x2 - x, y2 - y))
}

/// Key names the Executor sends (`"enter"`, `"ctrl"`, `"a"`). `ctrl` is Control; `cmd` is Command.
fn key(name: &str) -> anyhow::Result<enigo::Key> {
    use enigo::Key::*;
    let lower = name.to_lowercase();
    Ok(match lower.as_str() {
        "enter" | "return" => Return,
        "tab" => Tab,
        "esc" | "escape" => Escape,
        "backspace" => Backspace,
        "delete" | "del" => Delete,
        "space" => Space,
        "up" => UpArrow,
        "down" => DownArrow,
        "left" => LeftArrow,
        "right" => RightArrow,
        "home" => Home,
        "end" => End,
        "pageup" => PageUp,
        "pagedown" => PageDown,
        "ctrl" | "control" => Control,
        "shift" => Shift,
        "alt" | "option" => Alt,
        "cmd" | "command" | "meta" | "super" | "win" => Meta,
        "capslock" => CapsLock,
        "f1" => F1,
        "f2" => F2,
        "f3" => F3,
        "f4" => F4,
        "f5" => F5,
        "f6" => F6,
        "f7" => F7,
        "f8" => F8,
        "f9" => F9,
        "f10" => F10,
        "f11" => F11,
        "f12" => F12,
        k => match k.chars().collect::<Vec<_>>()[..] {
            [c] => Unicode(c),
            _ => anyhow::bail!("unknown key {name:?}"),
        },
    })
}

/// The Mark role for an AX role; `None` = not something to target.
fn role(ax_role: &str) -> Option<&'static str> {
    Some(match ax_role {
        "AXButton" | "AXMenuButton" | "AXPopUpButton" | "AXDisclosureTriangle" => "button",
        "AXTextField" | "AXTextArea" | "AXSearchField" | "AXComboBox" => "field",
        "AXLink" => "link",
        "AXCheckBox" => "checkbox",
        "AXRadioButton" => "radio",
        "AXMenuItem" | "AXMenuBarItem" => "menuitem",
        "AXSlider" => "slider",
        "AXStaticText" => "text",
        _ => return None,
    })
}

fn ax_attr(element: &AXUIElement, name: &str) -> Option<CFRetained<CFType>> {
    let mut value: *const CFType = std::ptr::null();
    let err = unsafe { element.copy_attribute_value(&CFString::from_str(name), NonNull::from(&mut value)) };
    (err == AXError::Success && !value.is_null()).then(|| unsafe { CFRetained::from_raw(NonNull::new_unchecked(value as *mut CFType)) })
}

fn ax_string(element: &AXUIElement, name: &str) -> Option<String> {
    ax_attr(element, name)?.downcast_ref::<CFString>().map(|s| s.to_string()).filter(|s| !s.trim().is_empty())
}

fn ax_value<T: Default>(element: &AXUIElement, name: &str, kind: AXValueType) -> Option<T> {
    let attr = ax_attr(element, name)?;
    let value = attr.downcast_ref::<AXValue>()?;
    let mut out = T::default();
    unsafe { value.value(kind, NonNull::from(&mut out).cast()) }.then_some(out)
}

// shortcut: only the focused app's tree (menu bar and windows); elements of windows behind it are left out, so a
// Mark is never hidden under another window. Walk all on-screen apps if Controllers need the Dock or other windows.
const AX_MAX_NODES: usize = 3000;

fn accessibility_tree(d: &DisplayInfo) -> Option<Vec<UiElement>> {
    if !unsafe { objc2_application_services::AXIsProcessTrusted() } {
        return None;
    }
    let system = unsafe { AXUIElement::new_system_wide() };
    let app = ax_attr(&system, "AXFocusedApplication")?.downcast::<AXUIElement>().ok()?;
    // A hung app must not stall the Observation.
    unsafe { app.set_messaging_timeout(0.5) };
    let mut out = vec![];
    let mut stack = vec![app];
    let mut visited = 0;
    while let Some(element) = stack.pop() {
        visited += 1;
        if visited > AX_MAX_NODES {
            break;
        }
        if let Some(role) = ax_string(&element, "AXRole").as_deref().and_then(role) {
            let text = ["AXTitle", "AXDescription", "AXValue", "AXPlaceholderValue"]
                .iter()
                .find_map(|a| ax_string(&element, a))
                .unwrap_or_default();
            let origin = ax_value::<CGPoint>(&element, "AXPosition", AXValueType::CGPoint);
            let size = ax_value::<CGSize>(&element, "AXSize", AXValueType::CGSize);
            if let (Some(origin), Some(size)) = (origin, size) {
                if let Some((x, y, width, height)) = to_pixels(d, origin, size) {
                    if role != "text" || !text.is_empty() {
                        out.push(UiElement { role: role.into(), text, x, y, width, height, ..Default::default() });
                    }
                }
            }
        }
        if let Some(children) = ax_attr(&element, "AXChildren").and_then(|c| c.downcast::<CFArray>().ok()) {
            let children: CFRetained<CFArray<AXUIElement>> = unsafe { CFRetained::cast_unchecked(children) };
            stack.extend(children.iter());
        }
    }
    Some(out)
}

/// The frame as a CGImage, for Vision.
fn cg_image(frame: &Frame) -> anyhow::Result<CFRetained<CGImage>> {
    let data = CFData::from_bytes(&frame.rgba);
    let provider = CGDataProvider::with_cf_data(Some(&data)).ok_or_else(|| anyhow::anyhow!("no data provider"))?;
    let space = CGColorSpace::new_device_rgb();
    unsafe {
        CGImage::new(
            frame.width as usize,
            frame.height as usize,
            8,
            32,
            frame.width as usize * 4,
            space.as_deref(),
            CGBitmapInfo(CGImageAlphaInfo::NoneSkipLast.0),
            Some(&provider),
            std::ptr::null(),
            false,
            CGColorRenderingIntent::RenderingIntentDefault,
        )
    }
    .ok_or_else(|| anyhow::anyhow!("CGImageCreate failed"))
}

/// Vision bounding box (normalized, origin bottom-left) as a pixel box of a `w` x `h` image.
fn vision_box(b: CGRect, w: u32, h: u32) -> (u32, u32, u32, u32) {
    let px = |v: f64, max: u32| (v * max as f64).round().clamp(0.0, max as f64) as u32;
    let (x, x2) = (px(b.origin.x, w), px(b.origin.x + b.size.width, w));
    let (y, y2) = (px(1.0 - b.origin.y - b.size.height, h), px(1.0 - b.origin.y, h));
    (x, y, x2 - x, y2 - y)
}

fn sh(program: &str, args: &[&str]) -> anyhow::Result<String> {
    let out = Command::new(program).args(args).output()?;
    anyhow::ensure!(out.status.success(), "{program} failed: {}", String::from_utf8_lossy(&out.stderr).trim());
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn is_root() -> bool {
    unsafe { libc::geteuid() == 0 }
}

/// argv for `account`: `command` = None is an interactive login shell.
// shortcut: zsh (the macOS default) rather than each account's login shell from the directory service.
fn shell_argv(account: &Account, command: Option<&str>) -> Vec<String> {
    let mut argv: Vec<String> = match (account, is_root()) {
        // As root, the Terminal User is one `sudo -u` away; `-i` gives that account's login environment.
        (Account::User(user), true) => vec!["sudo".into(), "-n".into(), "-u".into(), user.clone(), "-i".into()],
        (Account::Root, true) => vec![],
        // An Agent in a user session can only be itself; `sudo` works as over SSH.
        (Account::User(_), false) => vec![],
        (Account::Root, false) => vec!["sudo".into(), "-i".into()],
    };
    if argv.last().is_some_and(|a| a == "-i") {
        if let Some(c) = command {
            argv.extend(["/bin/zsh".into(), "-c".into(), c.into()]);
        }
    } else {
        argv.extend(["/bin/zsh".into(), "-l".into()]);
        if let Some(c) = command {
            argv.extend(["-c".into(), c.into()]);
        }
    }
    argv
}

fn command(argv: &[String]) -> Command {
    let mut cmd = Command::new(&argv[0]);
    cmd.args(&argv[1..]);
    cmd
}

fn exit_code(status: std::process::ExitStatus) -> i32 {
    status.code().unwrap_or_else(|| 128 + status.signal().unwrap_or(0))
}

/// Master side of a PTY; reading after the shell has gone gives EOF, not EIO.
struct Master(std::fs::File);

impl Read for Master {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self.0.read(buf) {
            Err(e) if e.raw_os_error() == Some(libc::EIO) => Ok(0),
            r => r,
        }
    }
}

struct MacPty {
    child: std::process::Child,
    master: std::fs::File,
}

impl PtyControl for MacPty {
    fn resize(&mut self, cols: u16, rows: u16) -> anyhow::Result<()> {
        let size = libc::winsize { ws_row: rows, ws_col: cols, ws_xpixel: 0, ws_ypixel: 0 };
        anyhow::ensure!(unsafe { libc::ioctl(self.master.as_raw_fd(), libc::TIOCSWINSZ, &size) } == 0, "resize failed");
        Ok(())
    }
    fn wait(&mut self) -> anyhow::Result<i32> {
        Ok(exit_code(self.child.wait()?))
    }
    fn kill(&mut self) -> anyhow::Result<()> {
        // The shell leads its own session and process group: end everything it started.
        unsafe { libc::killpg(self.child.id() as i32, libc::SIGKILL) };
        let _ = self.child.wait();
        Ok(())
    }
}

impl Platform for MacPlatform {
    fn hostname(&self) -> String {
        self.hostname.clone()
    }

    fn displays(&self) -> anyhow::Result<Vec<DisplayInfo>> {
        Ok(screens()?.into_iter().map(|(_, d)| d).collect())
    }

    // shortcut: CGDisplayCreateImage is deprecated in favour of ScreenCaptureKit but still works on macOS 15;
    // move to ScreenCaptureKit (or RustDesk's scrap, ADR 0002) when Apple removes it.
    #[allow(deprecated)]
    fn capture(&self, display: u32) -> anyhow::Result<Frame> {
        let (id, _) = screen(display)?;
        let image = objc2_core_graphics::CGDisplayCreateImage(id)
            .ok_or_else(|| anyhow::anyhow!("cannot capture display {display}: grant rcli Screen Recording in System Settings"))?;
        let (width, height) = (CGImage::width(Some(&image)), CGImage::height(Some(&image)));
        anyhow::ensure!(CGImage::bits_per_pixel(Some(&image)) == 32, "unsupported pixel format");
        let stride = CGImage::bytes_per_row(Some(&image));
        let data = CGDataProvider::data(CGImage::data_provider(Some(&image)).as_deref())
            .ok_or_else(|| anyhow::anyhow!("no pixels for display {display}"))?;
        let bytes = data.to_vec();
        let mut rgba = Vec::with_capacity(width * height * 4);
        for row in bytes.chunks(stride).take(height) {
            // Display images are BGRA (32-bit little-endian, alpha first).
            for p in row[..width * 4].as_chunks::<4>().0 {
                rgba.extend_from_slice(&[p[2], p[1], p[0], 255]);
            }
        }
        Ok(Frame { width: width as u32, height: height as u32, rgba })
    }

    fn input(&self, input: Input) -> anyhow::Result<()> {
        match input {
            Input::Move { display, x, y } => {
                let (_, d) = screen(display)?;
                // shortcut: enigo takes whole points, so on Retina an odd pixel lands 1 px off; post CGEvents directly if that matters.
                let (px, py) = to_point(&d, x, y);
                self.with_enigo(|e| e.move_mouse(px.round() as i32, py.round() as i32, Coordinate::Abs))
            }
            Input::Press { button, down } => {
                let button = match button {
                    Button::Left => enigo::Button::Left,
                    Button::Right => enigo::Button::Right,
                    Button::Middle => enigo::Button::Middle,
                };
                self.with_enigo(|e| e.button(button, if down { Direction::Press } else { Direction::Release }))
            }
            Input::Scroll { dx, dy } => self.with_enigo(|e| {
                if dy != 0 {
                    e.scroll(dy, Axis::Vertical)?;
                }
                if dx != 0 {
                    e.scroll(dx, Axis::Horizontal)?;
                }
                Ok(())
            }),
            Input::Text(text) => self.with_enigo(|e| e.text(&text)),
            Input::Key(name) => {
                let k = key(&name)?;
                self.with_enigo(|e| e.key(k, Direction::Click))
            }
            Input::KeyDown(name) => {
                let k = key(&name)?;
                self.with_enigo(|e| e.key(k, Direction::Press))
            }
            Input::KeyUp(name) => {
                let k = key(&name)?;
                self.with_enigo(|e| e.key(k, Direction::Release))
            }
            // Ctrl+Alt+Del is the secure attention sequence, which macOS has no equivalent of.
            Input::Sas => anyhow::bail!("Ctrl+Alt+Del is not available on macOS"),
        }
    }

    /// `None` without the Accessibility grant, so Marks fall back to OCR.
    fn accessibility(&self, display: u32) -> anyhow::Result<Option<Vec<UiElement>>> {
        let (_, d) = screen(display)?;
        Ok(objc2::rc::autoreleasepool(|_| accessibility_tree(&d)))
    }

    /// Apple Vision text recognition, one element per line.
    fn recognize_text(&self, frame: &Frame) -> anyhow::Result<Vec<UiElement>> {
        let image = cg_image(frame)?;
        objc2::rc::autoreleasepool(|_| {
            let request = VNRecognizeTextRequest::new();
            let handler = unsafe {
                VNImageRequestHandler::initWithCGImage_options(VNImageRequestHandler::alloc(), &image, &NSDictionary::new())
            };
            let requests: &VNRequest = &request;
            handler.performRequests_error(&NSArray::from_slice(&[requests])).map_err(|e| anyhow::anyhow!("OCR failed: {e}"))?;
            let mut out = vec![];
            for observation in request.results().unwrap_or_default().iter() {
                let Some(best) = observation.topCandidates(1).firstObject() else { continue };
                let (x, y, width, height) = vision_box(unsafe { observation.boundingBox() }, frame.width, frame.height);
                out.push(UiElement { role: "text".into(), text: best.string().to_string(), x, y, width, height, ..Default::default() });
            }
            Ok(out)
        })
    }

    /// From the session this process runs in: no GUI session, or one not logged in yet (the LoginWindow
    /// LaunchAgent's), = the login window.
    fn session_state(&self) -> anyhow::Result<SessionState> {
        Ok(match session_flags() {
            None => SessionState::LoginScreen,
            Some(flag) if !flag("kCGSessionLoginDoneKey") => SessionState::LoginScreen,
            Some(flag) if flag("CGSSessionScreenIsLocked") => SessionState::Locked,
            Some(_) => SessionState::LoggedIn,
        })
    }

    // shortcut: no GPU list (system_profiler takes seconds); add it if Controllers ask for GPUs.
    fn inventory(&self) -> anyhow::Result<Inventory> {
        let mut s = self.sys.lock().unwrap();
        s.system.refresh_cpu_all();
        s.system.refresh_memory();
        let disks = sysinfo::Disks::new_with_refreshed_list()?;
        let serial = sh("/usr/sbin/ioreg", &["-rd1", "-c", "IOPlatformExpertDevice"]).unwrap_or_default();
        let serial = serial
            .lines()
            .find(|l| l.contains("\"IOPlatformSerialNumber\""))
            .and_then(|l| l.rsplit('"').nth(1))
            .unwrap_or_default();
        Ok(Inventory {
            os: "macOS".into(),
            os_version: sysinfo::System::os_version().unwrap_or_default(),
            hostname: sysinfo::System::host_name().unwrap_or_default(),
            cpu_model: s.system.cpus().first().map(|c| c.brand().to_owned()).unwrap_or_default(),
            cpu_cores: s.system.cpus().len() as u32,
            ram_bytes: s.system.total_memory(),
            disks: disks
                .iter()
                .map(|d| Disk {
                    name: d.name().to_string_lossy().into(),
                    mount: d.mount_point().display().to_string(),
                    total_bytes: d.total_space(),
                })
                .collect(),
            gpus: vec![],
            network_interfaces: s
                .networks
                .iter()
                .map(|(name, n)| NetworkInterface {
                    name: name.to_string_lossy().into(),
                    mac: n.mac_address().to_string(),
                    ips: n.ip_networks().iter().map(|ip| format!("{}/{}", ip.addr, ip.prefix)).collect(),
                })
                .collect(),
            serial: serial.into(),
            displays: self
                .displays()?
                .iter()
                .map(|d| Display { name: format!("Display {}", d.index + 1), width: d.width, height: d.height })
                .collect(),
        })
    }

    // shortcut: no disk I/O rate, GPU load, temperature or battery yet; they stay unset (= unavailable).
    fn metrics(&self) -> anyhow::Result<Resources> {
        let mut s = self.sys.lock().unwrap();
        if s.system.cpus().is_empty() {
            // CPU usage is a difference between two samples.
            s.system.refresh_cpu_usage();
            std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
        }
        s.system.refresh_cpu_usage();
        s.system.refresh_memory();
        s.networks.refresh(true);
        let secs = s.networks_at.elapsed().as_secs_f64().max(0.001);
        s.networks_at = Instant::now();
        let (down, up) = s.networks.values().fold((0, 0), |(d, u), n| (d + n.received(), u + n.transmitted()));
        let disks = sysinfo::Disks::new_with_refreshed_list()?;
        // The console owner is the logged-in user; root owns it at the login window.
        let console = std::os::unix::fs::MetadataExt::uid(&std::fs::metadata("/dev/console")?);
        s.users.refresh();
        let user = s.users.iter().find(|u| **u.id() == console).map(|u| u.name().to_owned()).filter(|u| u != "root");
        Ok(Resources {
            cpu_percent: Some(s.system.global_cpu_usage() as f64),
            ram_used_bytes: Some(s.system.used_memory()),
            disk_used_percent: disks
                .iter()
                .filter(|d| d.total_space() > 0)
                .map(|d| {
                    let used = d.total_space() - d.available_space();
                    (d.mount_point().display().to_string(), used as f64 * 100.0 / d.total_space() as f64)
                })
                .collect(),
            net_down_bytes_per_sec: Some((down as f64 / secs) as u64),
            net_up_bytes_per_sec: Some((up as f64 / secs) as u64),
            uptime_secs: sysinfo::System::uptime().ok(),
            logged_in_user: Some(user.unwrap_or_default()),
            ..Default::default()
        })
    }

    fn processes(&self) -> anyhow::Result<Vec<Process>> {
        let mut s = self.sys.lock().unwrap();
        s.system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
        let s = &*s;
        Ok(s.system
            .processes()
            .values()
            .map(|p| Process {
                pid: p.pid().as_u32(),
                name: p.name().to_string_lossy().into(),
                user: p.user_id().and_then(|id| s.users.get_user_by_id(id)).map(|u| u.name().to_owned()).unwrap_or_default(),
                cpu_percent: p.cpu_usage() as f64,
                memory_bytes: p.memory(),
            })
            .collect())
    }

    fn shell(&self, command_line: &str, account: Account, timeout: Duration) -> anyhow::Result<ShellOutput> {
        let mut child = command(&shell_argv(&account, Some(command_line)))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0)
            .spawn()?;
        let reader = |mut pipe: Box<dyn Read + Send>| {
            std::thread::spawn(move || {
                let mut buf = vec![];
                let _ = pipe.read_to_end(&mut buf);
                String::from_utf8_lossy(&buf).into_owned()
            })
        };
        let stdout = reader(Box::new(child.stdout.take().unwrap()));
        let stderr = reader(Box::new(child.stderr.take().unwrap()));
        let start = Instant::now();
        let (status, timed_out) = loop {
            if let Some(status) = child.try_wait()? {
                break (status, false);
            }
            if start.elapsed() >= timeout {
                unsafe { libc::killpg(child.id() as i32, libc::SIGKILL) };
                break (child.wait()?, true);
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        Ok(ShellOutput {
            exit_code: exit_code(status),
            stdout: stdout.join().unwrap_or_default(),
            stderr: stderr.join().unwrap_or_default(),
            timed_out,
        })
    }

    fn pty(&self, account: Account, cols: u16, rows: u16) -> anyhow::Result<Pty> {
        let (mut master, mut slave) = (0, 0);
        let mut size = libc::winsize { ws_row: rows, ws_col: cols, ws_xpixel: 0, ws_ypixel: 0 };
        let ok = unsafe { libc::openpty(&mut master, &mut slave, std::ptr::null_mut(), std::ptr::null_mut(), &mut size) };
        anyhow::ensure!(ok == 0, "openpty failed: {}", std::io::Error::last_os_error());
        let (master, slave) = unsafe { (std::fs::File::from_raw_fd(master), OwnedFd::from_raw_fd(slave)) };
        let mut cmd = command(&shell_argv(&account, None));
        cmd.env("TERM", "xterm-256color")
            .stdin(slave.try_clone()?)
            .stdout(slave.try_clone()?)
            .stderr(slave);
        // New session with the PTY as its controlling terminal, so Ctrl-C reaches the foreground job.
        unsafe {
            cmd.pre_exec(|| {
                if libc::setsid() < 0 || libc::ioctl(0, libc::TIOCSCTTY as _, 0) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = cmd.spawn()?;
        drop(cmd); // closes our copies of the slave, so `output` hits EOF when the shell exits
        Ok(Pty {
            output: Box::new(Master(master.try_clone()?)),
            input: Box::new(master.try_clone()?),
            control: Box::new(MacPty { child, master }),
        })
    }

    fn power(&self, action: PowerAction) -> anyhow::Result<()> {
        let root = is_root();
        match action {
            PowerAction::Lock => lock_screen(),
            PowerAction::Sleep => sh("/usr/bin/pmset", &["sleepnow"]).map(drop),
            PowerAction::Restart if root => sh("/sbin/shutdown", &["-r", "now"]).map(drop),
            PowerAction::Shutdown if root => sh("/sbin/shutdown", &["-h", "now"]).map(drop),
            // As the user: ask loginwindow, which closes apps as the Apple menu does (no confirmation dialog).
            PowerAction::Restart => sh("/usr/bin/osascript", &["-e", "tell application \"loginwindow\" to «event aevtrest»"]).map(drop),
            PowerAction::Shutdown => sh("/usr/bin/osascript", &["-e", "tell application \"loginwindow\" to «event aevtshut»"]).map(drop),
        }
    }

    fn send_wake(&self, mac: [u8; 6]) -> anyhow::Result<()> {
        let socket = std::net::UdpSocket::bind("0.0.0.0:0")?;
        socket.set_broadcast(true)?;
        let packet: Vec<u8> = [0xff; 6].into_iter().chain(mac.repeat(16)).collect();
        socket.send_to(&packet, "255.255.255.255:9")?;
        Ok(())
    }

    /// Swaps the binary in place and re-executes it with the same arguments.
    fn install_update(&self, _version: &str, build: &std::path::Path) -> anyhow::Result<()> {
        let exe = std::env::current_exe()?;
        let temp = exe.with_extension("new");
        std::fs::copy(build, &temp)?;
        std::fs::set_permissions(&temp, std::os::unix::fs::PermissionsExt::from_mode(0o755))?;
        std::fs::rename(&temp, &exe)?;
        // This process is replaced below, so nobody else is left to delete the download.
        let _ = std::fs::remove_file(build);
        Err(Command::new(&exe).args(std::env::args_os().skip(1)).exec().into())
    }

    /// The GUI window is the indicator (`gui::wrap`); nothing else to show in a logged-in session.
    fn set_control_indicator(&self, _on: bool) -> anyhow::Result<()> {
        Ok(())
    }

    // shortcut: without the GUI window (`gui::wrap` answers prompts on desktop builds) nobody can be asked, so reject.
    fn ask_root(&self, _requester: &str, _timeout: Duration) -> anyhow::Result<bool> {
        Ok(false)
    }

    fn ask_accept(&self, _requester: &str, _timeout: Duration) -> anyhow::Result<bool> {
        Ok(false)
    }
}

/// The current GUI session's boolean flags (`None` = no GUI session); values may be CFBoolean or CFNumber.
fn session_flags() -> Option<impl Fn(&str) -> bool> {
    let dict = CGSessionCopyCurrentDictionary()?;
    let dict: CFRetained<objc2_core_foundation::CFDictionary<CFString, CFType>> = unsafe { CFRetained::cast_unchecked(dict) };
    Some(move |key: &str| {
        dict.get(&CFString::from_str(key)).is_some_and(|v| {
            v.downcast_ref::<CFBoolean>().map(|b| b.as_bool())
                .or_else(|| v.downcast_ref::<CFNumber>().and_then(|n| n.as_i64()).map(|n| n != 0))
                .unwrap_or(false)
        })
    })
}

/// Whether this process's GUI session is the one on the console (not a fast-user-switched one in the background).
/// The login window has no session dictionary and is always on the console.
pub fn on_console() -> bool {
    session_flags().is_none_or(|flag| flag("kCGSSessionOnConsoleKey"))
}

/// Shows the Screen Recording and Accessibility prompts for this app (the GUI's permission guide); grants already
/// given are kept. Returns whether both are granted now.
pub fn request_grants() -> bool {
    let screen = objc2_core_graphics::CGPreflightScreenCaptureAccess() || objc2_core_graphics::CGRequestScreenCaptureAccess();
    let prompt: &CFString = unsafe { objc2_application_services::kAXTrustedCheckOptionPrompt };
    let options = objc2_core_foundation::CFDictionary::from_slices(&[prompt], &[CFBoolean::new(true)]);
    let ax = unsafe { objc2_application_services::AXIsProcessTrustedWithOptions(Some(options.as_opaque())) };
    screen && ax
}

/// Whether Screen Recording and Accessibility are granted to this app, without prompting.
pub fn grants() -> (bool, bool) {
    (objc2_core_graphics::CGPreflightScreenCaptureAccess(), unsafe { objc2_application_services::AXIsProcessTrusted() })
}

/// Locks the screen as Control-Command-Q does, through the private login framework (no keystroke, no grant).
fn lock_screen() -> anyhow::Result<()> {
    let path = c"/System/Library/PrivateFrameworks/login.framework/Versions/Current/login";
    let lib = unsafe { libc::dlopen(path.as_ptr(), libc::RTLD_LAZY) };
    anyhow::ensure!(!lib.is_null(), "login framework not found");
    let f = unsafe { libc::dlsym(lib, c"SACLockScreenImmediate".as_ptr()) };
    anyhow::ensure!(!f.is_null(), "SACLockScreenImmediate not found");
    let lock: extern "C" fn() -> i32 = unsafe { std::mem::transmute::<*mut c_void, _>(f) };
    anyhow::ensure!(lock() == 0, "locking the screen failed");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn retina_right_of_main() -> DisplayInfo {
        DisplayInfo { index: 1, x: 1440, y: -100, width: 2880, height: 1800, scale: 2.0, primary: false }
    }

    #[test]
    fn real_pixels_map_to_global_points_on_a_retina_secondary_display() {
        assert_eq!(to_point(&retina_right_of_main(), 0, 0), (1440.0, -100.0));
        assert_eq!(to_point(&retina_right_of_main(), 101, 2880 / 2), (1490.5, 620.0));
    }

    #[test]
    fn ax_boxes_in_points_become_real_pixels_of_their_display_or_nothing() {
        let d = retina_right_of_main();
        let rect = |x, y, w, h| (CGPoint { x, y }, CGSize { width: w, height: h });
        let (o, s) = rect(1450.0, -90.0, 100.0, 20.0);
        assert_eq!(to_pixels(&d, o, s), Some((20, 20, 200, 40)));
        // Straddling the left edge: clipped to the display.
        let (o, s) = rect(1400.0, 0.0, 50.0, 10.0);
        assert_eq!(to_pixels(&d, o, s), Some((0, 200, 20, 20)));
        // On the main display, left of this one.
        let (o, s) = rect(100.0, 100.0, 50.0, 10.0);
        assert_eq!(to_pixels(&d, o, s), None);
    }

    #[test]
    fn vision_boxes_flip_to_top_left_pixels() {
        let b = CGRect { origin: CGPoint { x: 0.25, y: 0.5 }, size: CGSize { width: 0.5, height: 0.25 } };
        assert_eq!(vision_box(b, 200, 100), (50, 25, 100, 25));
    }

    #[test]
    fn executor_key_names_map_to_mac_keys() {
        assert_eq!(key("enter").unwrap(), enigo::Key::Return);
        assert_eq!(key("ctrl").unwrap(), enigo::Key::Control);
        assert_eq!(key("cmd").unwrap(), enigo::Key::Meta);
        assert_eq!(key("F5").unwrap(), enigo::Key::F5);
        assert_eq!(key("a").unwrap(), enigo::Key::Unicode('a'));
        assert!(key("hyper").is_err());
    }

    #[test]
    fn interactive_shells_and_commands_get_the_right_argv() {
        let user = Account::User("alice".into());
        if is_root() {
            assert_eq!(shell_argv(&user, Some("id")), ["sudo", "-n", "-u", "alice", "-i", "/bin/zsh", "-c", "id"]);
        } else {
            assert_eq!(shell_argv(&user, Some("id")), ["/bin/zsh", "-l", "-c", "id"]);
            assert_eq!(shell_argv(&user, None), ["/bin/zsh", "-l"]);
            assert_eq!(shell_argv(&Account::Root, None), ["sudo", "-i"]);
        }
    }

    // The non-interactive part of the contract on this Mac: nothing here moves the pointer, types, captures the
    // screen or needs a grant. The full suite is `rcli-contract` (see ticket 23).
    fn mac() -> MacPlatform {
        MacPlatform::new("test-mac").unwrap()
    }

    #[test]
    fn shell_runs_and_times_out() {
        let p = mac();
        let out = p.shell("echo hi; echo err >&2; exit 4", Account::User("x".into()), Duration::from_secs(5)).unwrap();
        assert_eq!((out.exit_code, out.stdout.as_str(), out.stderr.as_str(), out.timed_out), (4, "hi\n", "err\n", false));
        let start = Instant::now();
        let out = p.shell("echo started; sleep 30 & wait", Account::User("x".into()), Duration::from_millis(500)).unwrap();
        assert!(out.timed_out && out.stdout == "started\n", "{out:?}");
        assert!(start.elapsed() < Duration::from_secs(3), "child of the shell kept the pipes open");
    }

    #[test]
    fn pty_echoes_and_reports_the_exit_code() {
        let mut pty = mac().pty(Account::User("x".into()), 80, 24).unwrap();
        pty.control.resize(100, 30).unwrap();
        pty.input.write_all(b"stty size\rexit 3\r").unwrap();
        let mut text = String::new();
        pty.output.read_to_string(&mut text).unwrap();
        assert!(text.contains("30 100"), "{text:?}");
        assert_eq!(pty.control.wait().unwrap(), 3);
    }

    #[test]
    fn system_facts_are_complete() {
        let p = mac();
        let i = p.inventory().unwrap();
        assert!(i.os == "macOS" && !i.os_version.is_empty() && i.cpu_cores > 0 && i.ram_bytes > 0, "{i:?}");
        assert!(i.disks.iter().any(|d| d.mount == "/") && !i.network_interfaces.is_empty(), "{i:?}");
        let m = p.metrics().unwrap();
        assert!((0.0..=100.0).contains(&m.cpu_percent.unwrap()) && m.ram_used_bytes.unwrap() > 0, "{m:?}");
        assert!(m.disk_used_percent.contains_key("/"), "{m:?}");
        let me = std::env::var("USER").unwrap();
        assert_eq!(m.logged_in_user.as_deref(), Some(me.as_str()));
        let processes = p.processes().unwrap();
        assert!(processes.iter().any(|q| q.pid == std::process::id() && q.user == me), "this test process is listed");
    }

    #[test]
    fn displays_session_wake_and_text_recognition_work_without_grants() {
        let p = mac();
        let displays = p.displays().unwrap();
        assert_eq!(displays.iter().filter(|d| d.primary).count(), 1, "{displays:?}");
        assert!(displays.iter().all(|d| d.width > 0 && d.scale >= 1.0), "{displays:?}");
        assert_eq!(p.session_state().unwrap(), SessionState::LoggedIn);
        p.send_wake(crate::contract::WAKE_TEST_MAC).unwrap();
        let blank = Frame { width: 64, height: 32, rgba: vec![255; 64 * 32 * 4] };
        assert_eq!(p.recognize_text(&blank).unwrap(), vec![]);
        // Read-only; `None` when this terminal has no Accessibility grant.
        let _ = p.accessibility(0).unwrap();
    }
}
