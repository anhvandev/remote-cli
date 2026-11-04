//! The Platform contract suite, run against the FakePlatform with a scripted operator.

use rcli_agent::{
    contract::{self, Env, Step},
    platform::{
        DisplayInfo, FakePlatform, Frame, Input, Inventory, Platform, PowerAction, Process, Resources, SessionState,
        ShellOutput,
    },
};

/// Does to the FakePlatform what a person at the machine would do.
fn scripted_operator(fake: &FakePlatform) -> impl FnMut(Step) + '_ {
    move |step| match step {
        Step::FocusTextField => {}
        Step::Lock => fake.set_session_state(SessionState::Locked),
        Step::Unlock | Step::LogIn => fake.set_session_state(SessionState::LoggedIn),
        Step::LogOut => fake.set_session_state(SessionState::LoginScreen),
    }
}

#[test]
fn fake_platform_passes_every_check_in_every_environment() {
    let fake = FakePlatform::new("lab-pc");
    let report = contract::run(&fake, &mut scripted_operator(&fake));
    assert_eq!(report.failures(), vec![]);
    assert_eq!(
        report.table(),
        "\
| Check | Logged in | Lock screen | Login screen |
|---|---|---|---|
| displays | ✅ | | |
| capture | ✅ | ✅ | ✅ |
| key shows on screen | ✅ | ✅ | ✅ |
| session state | ✅ | ✅ | ✅ |
| metrics | ✅ | | |
| inventory | ✅ | | |
| processes | ✅ | | |
| shell | ✅ | | |
| wake send | ✅ | | |
| power lock | ✅ | | |
"
    );
    assert_eq!(fake.wakes_sent(), vec![contract::WAKE_TEST_MAC]);
}

/// A FakePlatform with one broken capability, to prove the suite catches it.
struct Broken {
    fake: FakePlatform,
    keys_reach_screen: bool,
    detects_session: bool,
}

impl Platform for Broken {
    fn hostname(&self) -> String {
        self.fake.hostname()
    }
    fn displays(&self) -> anyhow::Result<Vec<DisplayInfo>> {
        self.fake.displays()
    }
    fn capture(&self, display: u32) -> anyhow::Result<Frame> {
        self.fake.capture(display)
    }
    fn input(&self, input: Input) -> anyhow::Result<()> {
        if self.keys_reach_screen {
            self.fake.input(input)
        } else {
            Ok(())
        }
    }
    fn session_state(&self) -> anyhow::Result<SessionState> {
        if self.detects_session {
            self.fake.session_state()
        } else {
            Ok(SessionState::LoggedIn)
        }
    }
    fn inventory(&self) -> anyhow::Result<Inventory> {
        self.fake.inventory()
    }
    fn metrics(&self) -> anyhow::Result<Resources> {
        self.fake.metrics()
    }
    fn processes(&self) -> anyhow::Result<Vec<Process>> {
        self.fake.processes()
    }
    fn shell(&self, command: &str) -> anyhow::Result<ShellOutput> {
        self.fake.shell(command)
    }
    fn power(&self, action: PowerAction) -> anyhow::Result<()> {
        self.fake.power(action)
    }
    fn send_wake(&self, mac: [u8; 6]) -> anyhow::Result<()> {
        self.fake.send_wake(mac)
    }
    fn install_update(&self, version: &str, build: &[u8]) -> anyhow::Result<()> {
        self.fake.install_update(version, build)
    }
}

#[test]
fn keys_that_never_reach_the_screen_fail_in_every_environment() {
    let fake = FakePlatform::new("lab-pc");
    let broken = Broken { fake: fake.clone(), keys_reach_screen: false, detects_session: true };
    let report = contract::run(&broken, &mut scripted_operator(&fake));
    assert_eq!(
        report.failures(),
        vec![
            ("key shows on screen", Env::LoggedIn),
            ("key shows on screen", Env::LockScreen),
            ("key shows on screen", Env::LoginScreen),
        ]
    );
    assert!(report.table().contains("| key shows on screen | ❌ screen did not change after the key |"));
}

#[test]
fn session_state_that_ignores_reality_fails_at_lock_and_login_screens() {
    let fake = FakePlatform::new("lab-pc");
    let broken = Broken { fake: fake.clone(), keys_reach_screen: true, detects_session: false };
    let report = contract::run(&broken, &mut scripted_operator(&fake));
    assert_eq!(
        report.failures(),
        vec![
            ("session state", Env::LockScreen),
            ("session state", Env::LoginScreen),
            ("power lock", Env::LoggedIn),
        ]
    );
    assert!(report.table().contains("❌ reported LoggedIn, expected Locked"));
}
