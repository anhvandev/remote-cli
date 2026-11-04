//! Contract suite for the Platform port, in the style of `prototype/lockscreen-probe`.
//! It runs against any adapter: the FakePlatform in CI, a real OS adapter on a real machine with an operator
//! following prompts. Each check yields one cell of a results table (rows = checks, columns = environments).

use std::time::{Duration, Instant};

use crate::platform::{Input, Platform, PowerAction, SessionState};

/// Locally administered MAC that belongs to no real NIC, so the Wake check wakes nothing.
pub const WAKE_TEST_MAC: [u8; 6] = [0x02, 0, 0, 0, 0, 0x27];
/// Like the probe: how long a key may take to show up on screen.
const SCREEN_CHANGE_TIMEOUT: Duration = Duration::from_secs(1);
const LOCK_TIMEOUT: Duration = Duration::from_secs(3);
const CHECKS: [&str; 8] = ["displays", "capture", "key shows on screen", "session state", "metrics", "shell", "wake send", "power lock"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Env {
    LoggedIn,
    LockScreen,
    LoginScreen,
}

const ENVS: [Env; 3] = [Env::LoggedIn, Env::LockScreen, Env::LoginScreen];

/// What the suite asks the person at the machine to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    FocusTextField,
    Lock,
    Unlock,
    LogOut,
    LogIn,
}

impl Step {
    pub fn instruction(self) -> &'static str {
        match self {
            Step::FocusTextField => "Click into an empty text field (the suite types the key `a`).",
            Step::Lock => "Lock the screen now.",
            Step::Unlock => "Delete the typed dots from the password field, then unlock.",
            Step::LogOut => "Log out now and stay at the login screen.",
            Step::LogIn => "Delete the typed dots from the password field, then log in.",
        }
    }
}

#[derive(Debug, Default)]
pub struct Report {
    rows: Vec<(&'static str, Env, Result<(), String>)>,
}

impl Report {
    fn check(&mut self, name: &'static str, env: Env, f: impl FnOnce() -> anyhow::Result<()>) {
        self.rows.push((name, env, f().map_err(|e| e.to_string())));
    }

    /// Failed checks in the order they ran.
    pub fn failures(&self) -> Vec<(&'static str, Env)> {
        self.rows.iter().filter(|r| r.2.is_err()).map(|r| (r.0, r.1)).collect()
    }

    /// Markdown table like the lockscreen probe README's.
    pub fn table(&self) -> String {
        let mut out = String::from("| Check | Logged in | Lock screen | Login screen |\n|---|---|---|---|\n");
        for check in CHECKS {
            out += &format!("| {check} |");
            for env in ENVS {
                match self.rows.iter().find(|r| r.0 == check && r.1 == env) {
                    None => out += " |",
                    Some((_, _, Ok(()))) => out += " ✅ |",
                    Some((_, _, Err(e))) => out += &format!(" ❌ {} |", e.replace('|', "/")),
                }
            }
            out += "\n";
        }
        out
    }
}

/// Runs every check; `operator` must have done each Step before it returns.
pub fn run(p: &dyn Platform, operator: &mut dyn FnMut(Step)) -> Report {
    let mut r = Report::default();
    r.check("displays", Env::LoggedIn, || {
        let displays = p.displays()?;
        anyhow::ensure!(displays.iter().filter(|d| d.primary).count() == 1, "need exactly one primary display: {displays:?}");
        for d in displays {
            let f = p.capture(d.index)?;
            anyhow::ensure!((f.width, f.height) == (d.width, d.height), "display {} captured as {}x{}", d.index, f.width, f.height);
        }
        Ok(())
    });
    operator(Step::FocusTextField);
    screen_checks(p, Env::LoggedIn, SessionState::LoggedIn, &mut r);
    r.check("metrics", Env::LoggedIn, || {
        let m = p.metrics()?;
        anyhow::ensure!((0.0..=100.0).contains(&m.cpu_percent), "cpu {}% out of range", m.cpu_percent);
        anyhow::ensure!(m.ram_used_bytes > 0, "ram used is 0");
        Ok(())
    });
    r.check("shell", Env::LoggedIn, || {
        let out = p.shell("echo rcli-contract")?;
        anyhow::ensure!(out.exit_code == 0 && out.stdout.trim() == "rcli-contract", "unexpected {out:?}");
        Ok(())
    });
    r.check("wake send", Env::LoggedIn, || p.send_wake(WAKE_TEST_MAC));

    operator(Step::Lock);
    screen_checks(p, Env::LockScreen, SessionState::Locked, &mut r);
    operator(Step::Unlock);

    operator(Step::LogOut);
    screen_checks(p, Env::LoginScreen, SessionState::LoginScreen, &mut r);
    operator(Step::LogIn);

    r.check("power lock", Env::LoggedIn, || {
        p.power(PowerAction::Lock)?;
        wait_for_state(p, SessionState::Locked, LOCK_TIMEOUT)
    });
    operator(Step::Unlock);
    r
}

fn screen_checks(p: &dyn Platform, env: Env, expected: SessionState, r: &mut Report) {
    r.check("capture", env, || {
        let f = p.capture(primary(p)?)?;
        anyhow::ensure!(f.width > 0 && f.height > 0, "empty frame");
        anyhow::ensure!(f.rgba.len() == (f.width * f.height * 4) as usize, "frame size mismatch");
        Ok(())
    });
    r.check("key shows on screen", env, || {
        let primary = primary(p)?;
        let before = p.capture(primary)?;
        p.input(Input::Key("a".into()))?;
        let start = Instant::now();
        while p.capture(primary)? == before {
            anyhow::ensure!(start.elapsed() < SCREEN_CHANGE_TIMEOUT, "screen did not change after the key");
            std::thread::sleep(Duration::from_millis(100));
        }
        Ok(())
    });
    r.check("session state", env, || wait_for_state(p, expected, Duration::ZERO));
}

fn primary(p: &dyn Platform) -> anyhow::Result<u32> {
    p.displays()?.iter().find(|d| d.primary).map(|d| d.index).ok_or_else(|| anyhow::anyhow!("no primary display"))
}

fn wait_for_state(p: &dyn Platform, expected: SessionState, timeout: Duration) -> anyhow::Result<()> {
    let start = Instant::now();
    loop {
        let got = p.session_state()?;
        if got == expected {
            return Ok(());
        }
        anyhow::ensure!(start.elapsed() < timeout, "reported {got:?}, expected {expected:?}");
        std::thread::sleep(Duration::from_millis(100));
    }
}
