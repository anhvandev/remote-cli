//! One-shot commands and PTY shells as the Terminal User or root.

use std::{
    fs::File,
    io::Read,
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::process::{CommandExt, ExitStatusExt},
    },
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use crate::platform::{Pty, PtyControl, RunAs, ShellOutput};

/// The account this Agent runs as.
pub fn current_user() -> String {
    let out = Command::new("id").arg("-un").output().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned());
    out.unwrap_or_default()
}

/// `program` as `run_as`, in that account's home: directly when the Agent already is that account, else through
/// `sudo -n` (passwordless for the root service; a user Agent needs a sudoers rule to switch accounts).
fn command(run_as: &RunAs, program: &[&str]) -> Command {
    let user = match run_as {
        RunAs::User(u) => u.clone(),
        RunAs::Root => "root".into(),
    };
    let mut cmd = if user == current_user() {
        Command::new(program[0])
    } else {
        let mut cmd = Command::new("sudo");
        cmd.args(["-n", "-H", "-u", &user, "--", program[0]]);
        cmd
    };
    cmd.args(&program[1..]);
    if let Some(home) = passwd(&user, 5) {
        cmd.current_dir(home);
    }
    cmd
}

/// Field `index` of the account's passwd entry (5 = home, 6 = login shell), through NSS so LDAP/sssd accounts work.
fn passwd(user: &str, index: usize) -> Option<String> {
    let out = Command::new("getent").args(["passwd", user]).output().ok()?;
    String::from_utf8_lossy(&out.stdout).trim().split(':').nth(index).filter(|f| !f.is_empty()).map(str::to_owned)
}

pub fn run(command_line: &str, run_as: &RunAs, timeout: Duration) -> anyhow::Result<ShellOutput> {
    let mut cmd = command(run_as, &["sh", "-c", command_line]);
    // Own process group, so a timeout kills everything the command started.
    let mut child = cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).process_group(0).spawn()?;
    let (stdout, out_done) = collect(child.stdout.take().unwrap());
    let (stderr, err_done) = collect(child.stderr.take().unwrap());
    let deadline = Instant::now() + timeout;
    let mut timed_out = false;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            timed_out = true;
            unsafe { libc::killpg(child.id() as i32, libc::SIGKILL) };
            break child.wait()?;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    // A background child may keep the pipes open: wait for the rest of the output only until the deadline.
    let grace = deadline.max(Instant::now() + Duration::from_millis(100));
    while !(out_done.is_finished() && err_done.is_finished()) && Instant::now() < grace {
        std::thread::sleep(Duration::from_millis(10));
    }
    let text = |b: &Arc<Mutex<Vec<u8>>>| String::from_utf8_lossy(&b.lock().unwrap()).into_owned();
    let exit_code = status.code().unwrap_or_else(|| 128 + status.signal().unwrap_or(0));
    Ok(ShellOutput { exit_code, stdout: text(&stdout), stderr: text(&stderr), timed_out })
}

/// Reads `r` to its end on a thread, into a buffer that can be read before the end.
fn collect(mut r: impl Read + Send + 'static) -> (Arc<Mutex<Vec<u8>>>, std::thread::JoinHandle<()>) {
    let buf = Arc::new(Mutex::new(vec![]));
    let out = buf.clone();
    let reader = std::thread::spawn(move || {
        let mut chunk = [0; 8192];
        while let Ok(n @ 1..) = r.read(&mut chunk) {
            out.lock().unwrap().extend_from_slice(&chunk[..n]);
        }
    });
    (buf, reader)
}

pub fn pty(run_as: &RunAs, cols: u16, rows: u16) -> anyhow::Result<Pty> {
    let (mut master, mut slave) = (0, 0);
    let size = libc::winsize { ws_row: rows, ws_col: cols, ws_xpixel: 0, ws_ypixel: 0 };
    if unsafe { libc::openpty(&mut master, &mut slave, std::ptr::null_mut(), std::ptr::null(), &size) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let (master, slave) = unsafe { (OwnedFd::from_raw_fd(master), OwnedFd::from_raw_fd(slave)) };
    let user = match run_as {
        RunAs::User(u) => u.as_str(),
        RunAs::Root => "root",
    };
    let shell = passwd(user, 6).unwrap_or_else(|| "/bin/sh".into());
    let mut cmd = command(run_as, &[&shell, "-l"]);
    cmd.env("TERM", "xterm-256color")
        .stdin(Stdio::from(slave.try_clone()?))
        .stdout(Stdio::from(slave.try_clone()?))
        .stderr(Stdio::from(slave));
    // A new session with the PTY as its controlling terminal, so Ctrl-C and job control work.
    unsafe {
        cmd.pre_exec(|| {
            if libc::setsid() < 0 || libc::ioctl(0, libc::TIOCSCTTY, 0) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let child = cmd.spawn()?;
    // Our copies of the PTY's other end must be closed, or reading never ends after the shell exits.
    drop(cmd);
    let output = PtyOutput(File::from(master.try_clone()?));
    let input = File::from(master.try_clone()?);
    Ok(Pty { output: Box::new(output), input: Box::new(input), control: Box::new(PtyChild { master, child }) })
}

/// Linux reports EIO once the shell has exited and the terminal closed; that is the end of the output.
struct PtyOutput(File);

impl Read for PtyOutput {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self.0.read(buf) {
            Err(e) if e.raw_os_error() == Some(libc::EIO) => Ok(0),
            r => r,
        }
    }
}

struct PtyChild {
    master: OwnedFd,
    child: Child,
}

impl PtyControl for PtyChild {
    fn resize(&mut self, cols: u16, rows: u16) -> anyhow::Result<()> {
        let size = libc::winsize { ws_row: rows, ws_col: cols, ws_xpixel: 0, ws_ypixel: 0 };
        if unsafe { libc::ioctl(self.master.as_raw_fd(), libc::TIOCSWINSZ, &size) } < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(())
    }

    fn wait(&mut self) -> anyhow::Result<i32> {
        let status = self.child.wait()?;
        Ok(status.code().unwrap_or_else(|| 128 + status.signal().unwrap_or(0)))
    }

    fn kill(&mut self) -> anyhow::Result<()> {
        // The shell leads its own session and process group.
        unsafe { libc::killpg(self.child.id() as i32, libc::SIGKILL) };
        Ok(())
    }
}
