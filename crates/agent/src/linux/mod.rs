//! Linux adapter for a logged-in X11 session (ticket 15): the Agent runs as the session's user with `DISPLAY` set.
//! The lock screen and the greeter come with the root service (ticket 16). The control indicator and the
//! root/accept prompts are the GUI window (ticket 14), which wraps this platform on a desktop build.

mod atspi;
mod shell;
mod x11;

use std::{
    process::Command,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

use sysinfo::{Components, Disks, Networks, ProcessesToUpdate, System, Users};

use crate::platform::{
    Disk, Display, DisplayInfo, Frame, Input, Inventory, NetworkInterface, Platform, PowerAction, Process, Pty,
    Resources, RunAs, SessionState, ShellOutput, UiElement,
};

pub struct LinuxPlatform {
    name: String,
    /// Reconnected on the next call after an error, so an X server restart does not need an Agent restart.
    x: Mutex<Option<x11::X>>,
    input: Mutex<Option<enigo::Enigo>>,
    sys: Mutex<Sampler>,
}

/// Counters kept between samples, for CPU % and the per-second rates.
struct Sampler {
    system: System,
    disks: Disks,
    networks: Networks,
    at: Instant,
}

impl LinuxPlatform {
    /// `name` is the Device name sent at registration.
    pub fn new(name: impl Into<String>) -> anyhow::Result<Self> {
        let mut system = System::new()?;
        system.refresh_cpu_all();
        let sampler = Sampler { system, disks: Disks::new_with_refreshed_list()?, networks: Networks::new_with_refreshed_list()?, at: Instant::now() };
        Ok(Self { name: name.into(), x: Mutex::new(None), input: Mutex::new(None), sys: Mutex::new(sampler) })
    }

    fn with_x<T>(&self, f: impl FnOnce(&x11::X) -> anyhow::Result<T>) -> anyhow::Result<T> {
        let mut x = self.x.lock().unwrap();
        if x.is_none() {
            *x = Some(x11::X::connect()?);
        }
        let out = f(x.as_ref().unwrap());
        if out.is_err() {
            *x = None;
        }
        out
    }
}

impl Platform for LinuxPlatform {
    fn hostname(&self) -> String {
        self.name.clone()
    }

    fn displays(&self) -> anyhow::Result<Vec<DisplayInfo>> {
        Ok(self.with_x(|x| x.monitors())?.into_iter().map(|(_, d)| d).collect())
    }

    fn capture(&self, display: u32) -> anyhow::Result<Frame> {
        self.with_x(|x| x.capture(display))
    }

    fn input(&self, input: Input) -> anyhow::Result<()> {
        let displays = match input {
            Input::Move { .. } => self.displays()?,
            _ => vec![],
        };
        let mut enigo = self.input.lock().unwrap();
        if enigo.is_none() {
            *enigo = Some(enigo::Enigo::new(&enigo::Settings::default())?);
        }
        let out = x11::input(enigo.as_mut().unwrap(), &displays, input);
        if out.is_err() {
            *enigo = None;
        }
        out
    }

    fn accessibility(&self, display: u32) -> anyhow::Result<Option<Vec<UiElement>>> {
        let displays = self.displays()?;
        let d = displays.iter().find(|d| d.index == display).ok_or_else(|| anyhow::anyhow!("no display {display}"))?;
        atspi::elements(d)
    }

    // shortcut: OCR through the tesseract CLI (`apt install tesseract-ocr`); link libtesseract if spawning shows up as slow.
    fn recognize_text(&self, frame: &Frame) -> anyhow::Result<Vec<UiElement>> {
        use std::io::Write;
        let mut child = Command::new("tesseract")
            .args(["stdin", "stdout", "tsv"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| anyhow::anyhow!("text recognition needs tesseract (apt install tesseract-ocr): {e}"))?;
        // PPM: the simplest image format tesseract reads.
        let mut ppm = format!("P6\n{} {}\n255\n", frame.width, frame.height).into_bytes();
        ppm.extend(frame.rgba.as_chunks::<4>().0.iter().flat_map(|p| [p[0], p[1], p[2]]));
        child.stdin.take().unwrap().write_all(&ppm)?;
        let out = child.wait_with_output()?;
        anyhow::ensure!(out.status.success(), "tesseract failed");
        Ok(ocr_lines(&String::from_utf8_lossy(&out.stdout)))
    }

    fn session_state(&self) -> anyhow::Result<SessionState> {
        Ok(match session_property("LockedHint").as_deref() {
            Some("yes") => SessionState::Locked,
            // shortcut: no logind session (or LockedHint unsupported) reads as logged in; this adapter only runs inside one.
            _ => SessionState::LoggedIn,
        })
    }

    fn inventory(&self) -> anyhow::Result<Inventory> {
        let mut s = self.sys.lock().unwrap();
        s.system.refresh_memory();
        let cpus = s.system.cpus();
        let networks = Networks::new_with_refreshed_list()?;
        let displays = self.with_x(|x| x.monitors()).unwrap_or_default();
        Ok(Inventory {
            os: System::name().unwrap_or_else(|_| "Linux".into()),
            os_version: System::os_version().unwrap_or_default(),
            hostname: System::host_name().unwrap_or_default(),
            cpu_model: cpus.first().map(|c| c.brand().trim().to_owned()).unwrap_or_default(),
            cpu_cores: System::physical_core_count().unwrap_or(cpus.len()) as u32,
            ram_bytes: s.system.total_memory(),
            disks: s
                .disks
                .iter()
                .map(|d| Disk { name: d.name().to_string_lossy().into(), mount: d.mount_point().display().to_string(), total_bytes: d.total_space() })
                .collect(),
            gpus: gpus().clone(),
            network_interfaces: networks
                .iter()
                .filter(|(name, _)| *name != "lo")
                .map(|(name, n)| NetworkInterface {
                    name: name.to_string_lossy().into(),
                    mac: n.mac_address().to_string(),
                    ips: n.ip_networks().iter().map(|ip| format!("{}/{}", ip.addr, ip.prefix)).collect(),
                })
                .collect(),
            // Readable by root only; empty for a user Agent.
            serial: std::fs::read_to_string("/sys/class/dmi/id/product_serial").unwrap_or_default().trim().into(),
            displays: displays.into_iter().map(|(name, d)| Display { name, width: d.width, height: d.height }).collect(),
        })
    }

    fn metrics(&self) -> anyhow::Result<Resources> {
        let mut s = self.sys.lock().unwrap();
        let secs = s.at.elapsed().as_secs_f64().max(0.001);
        s.at = Instant::now();
        s.system.refresh_cpu_usage();
        s.system.refresh_memory();
        s.disks.refresh(true);
        s.networks.refresh(true);
        let per_sec = |bytes: u64| Some((bytes as f64 / secs) as u64);
        let (read, written) = s.disks.iter().fold((0, 0), |(r, w), d| (r + d.usage().read_bytes, w + d.usage().written_bytes));
        let (up, down) = s.networks.iter().filter(|(name, _)| *name != "lo").fold((0, 0), |(u, d), (_, n)| (u + n.transmitted(), d + n.received()));
        let cpu_temp = Components::new_with_refreshed_list().ok().and_then(|components| {
            components
                .iter()
                .filter(|c| ["coretemp", "k10temp", "package", "tctl", "cpu"].iter().any(|k| c.label().to_lowercase().contains(k)))
                .filter_map(|c| c.temperature())
                .reduce(f32::max)
        });
        let battery = std::fs::read_dir("/sys/class/power_supply").ok().and_then(|dir| {
            dir.flatten().map(|e| e.path()).find(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("BAT")))
        });
        let read_battery = |file: &str| battery.as_ref().and_then(|b| std::fs::read_to_string(b.join(file)).ok()).map(|v| v.trim().to_owned());
        Ok(Resources {
            cpu_percent: Some(f64::from(s.system.global_cpu_usage()).clamp(0.0, 100.0)),
            ram_used_bytes: Some(s.system.used_memory()),
            disk_used_percent: s
                .disks
                .iter()
                .filter(|d| d.total_space() > 0)
                .map(|d| (d.mount_point().display().to_string(), 100.0 * (d.total_space() - d.available_space()) as f64 / d.total_space() as f64))
                .collect(),
            disk_read_bytes_per_sec: per_sec(read),
            disk_write_bytes_per_sec: per_sec(written),
            net_up_bytes_per_sec: per_sec(up),
            net_down_bytes_per_sec: per_sec(down),
            gpu_percent: None,
            uptime_secs: System::uptime().ok(),
            logged_in_user: Some(session_property("Name").unwrap_or_else(shell::current_user)),
            session: None,
            cpu_temp_celsius: cpu_temp.map(f64::from),
            battery_percent: read_battery("capacity").and_then(|v| v.parse().ok()),
            battery_charging: read_battery("status").map(|v| v == "Charging" || v == "Full"),
        })
    }

    fn processes(&self) -> anyhow::Result<Vec<Process>> {
        let mut s = self.sys.lock().unwrap();
        s.system.refresh_processes(ProcessesToUpdate::All, true);
        let users = Users::new_with_refreshed_list()?;
        Ok(s.system
            .processes()
            .values()
            .map(|p| Process {
                pid: p.pid().as_u32(),
                name: p.name().to_string_lossy().into(),
                user: p.user_id().and_then(|u| users.get_user_by_id(u)).map(|u| u.name().to_owned()).unwrap_or_default(),
                cpu_percent: p.cpu_usage().into(),
                memory_bytes: p.memory(),
            })
            .collect())
    }

    fn shell(&self, command: &str, run_as: RunAs, timeout: Duration) -> anyhow::Result<ShellOutput> {
        shell::run(command, &run_as, timeout)
    }

    fn pty(&self, run_as: RunAs, cols: u16, rows: u16) -> anyhow::Result<Pty> {
        shell::pty(&run_as, cols, rows)
    }

    fn power(&self, action: PowerAction) -> anyhow::Result<()> {
        let args: &[&str] = match action {
            PowerAction::Restart => &["systemctl", "reboot"],
            PowerAction::Shutdown => &["systemctl", "poweroff"],
            PowerAction::Sleep => &["systemctl", "suspend"],
            PowerAction::Lock => &["loginctl", "lock-session", "auto"],
        };
        let out = Command::new(args[0]).args(&args[1..]).output()?;
        anyhow::ensure!(out.status.success(), "{} failed: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
        Ok(())
    }

    fn send_wake(&self, mac: [u8; 6]) -> anyhow::Result<()> {
        let mut packet = vec![0xff; 6];
        packet.extend(mac.repeat(16));
        let socket = std::net::UdpSocket::bind("0.0.0.0:0")?;
        socket.set_broadcast(true)?;
        socket.send_to(&packet, "255.255.255.255:9")?;
        Ok(())
    }

    /// Swaps the binary atomically, then becomes the new one in place (same PID, same arguments).
    fn install_update(&self, _version: &str, build: &[u8]) -> anyhow::Result<()> {
        use std::os::unix::{fs::PermissionsExt, process::CommandExt};
        let exe = std::env::current_exe()?;
        let temp = exe.with_extension("new");
        std::fs::write(&temp, build)?;
        std::fs::set_permissions(&temp, std::fs::Permissions::from_mode(0o755))?;
        std::fs::rename(&temp, &exe)?;
        Err(Command::new(&exe).args(std::env::args_os().skip(1)).exec().into())
    }

    /// The GUI window is the indicator (ticket 14); a headless build has nobody to show it to.
    fn set_control_indicator(&self, _on: bool) -> anyhow::Result<()> {
        Ok(())
    }

    /// The GUI window asks (ticket 14); a headless build has nobody to ask, which rejects.
    fn ask_root(&self, _requester: &str, _timeout: Duration) -> anyhow::Result<bool> {
        Ok(false)
    }

    fn ask_accept(&self, _requester: &str, _timeout: Duration) -> anyhow::Result<bool> {
        Ok(false)
    }
}

/// A property of this user's graphical logind session (`auto`: the caller's, else the user's display session).
fn session_property(name: &str) -> Option<String> {
    let out = Command::new("loginctl").args(["show-session", "auto", "-p", name, "--value"]).output().ok()?;
    let value = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    (out.status.success() && !value.is_empty()).then_some(value)
}

/// GPU names from `lspci`, read once (they do not change while running).
fn gpus() -> &'static Vec<String> {
    static GPUS: OnceLock<Vec<String>> = OnceLock::new();
    GPUS.get_or_init(|| {
        let out = Command::new("lspci").output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
        out.lines()
            .filter(|l| l.contains("VGA compatible controller") || l.contains("3D controller"))
            .filter_map(|l| l.split(": ").nth(1).map(str::to_owned))
            .collect()
    })
}

/// Lines of tesseract's TSV output (word rows grouped by block, paragraph and line), boxes joined.
fn ocr_lines(tsv: &str) -> Vec<UiElement> {
    let mut lines: Vec<((&str, &str, &str), UiElement)> = vec![];
    for row in tsv.lines().skip(1) {
        let c: Vec<&str> = row.split('\t').collect();
        let [level, _, block, par, line, _, x, y, w, h, conf, text] = c[..] else { continue };
        let num = |s: &str| s.parse::<u32>().unwrap_or(0);
        if level != "5" || text.trim().is_empty() || conf.parse::<f32>().unwrap_or(-1.0) < 0.0 {
            continue;
        }
        let word = UiElement { role: "text".into(), text: text.trim().into(), x: num(x), y: num(y), width: num(w), height: num(h) };
        match lines.last_mut() {
            Some((key, l)) if *key == (block, par, line) => {
                let (x2, y2) = ((l.x + l.width).max(word.x + word.width), (l.y + l.height).max(word.y + word.height));
                (l.x, l.y) = (l.x.min(word.x), l.y.min(word.y));
                (l.width, l.height) = (x2 - l.x, y2 - l.y);
                l.text = format!("{} {}", l.text, word.text);
            }
            _ => lines.push(((block, par, line), word)),
        }
    }
    lines.into_iter().map(|(_, l)| l).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ocr_words_on_one_line_become_one_element() {
        let tsv = "level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext\n\
                   4\t1\t1\t1\t1\t0\t10\t20\t100\t15\t-1\t\n\
                   5\t1\t1\t1\t1\t1\t10\t22\t40\t12\t96.5\tSign\n\
                   5\t1\t1\t1\t1\t2\t60\t20\t50\t15\t91\tin\n\
                   5\t1\t2\t1\t1\t1\t5\t80\t30\t10\t88\tHelp\n";
        let el = |text: &str, x, y, width, height| UiElement { role: "text".into(), text: text.into(), x, y, width, height };
        assert_eq!(ocr_lines(tsv), vec![el("Sign in", 10, 20, 100, 15), el("Help", 5, 80, 30, 10)]);
    }
}
