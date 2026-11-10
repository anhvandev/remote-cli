//! Runs the Platform contract suite on this machine. Prints the plan first, since the operator cannot read
//! a terminal at the lock or login screen; each step then gets a fixed time before the checks continue.
//! Ends with a markdown results table to paste into the README of the adapter's ticket.

use std::time::Duration;

use rcli_agent::contract;

const STEP_TIME: Duration = Duration::from_secs(20);

fn main() {
    #[cfg(target_os = "linux")]
    let platform = rcli_agent::linux::LinuxPlatform::new("contract");
    // shortcut: no adapter for this OS yet (tickets 21, 23); swap it in here when it lands.
    #[cfg(not(target_os = "linux"))]
    let platform = rcli_agent::platform::FakePlatform::new("contract");
    eprintln!("Platform contract suite. Each step below gets {} s:", STEP_TIME.as_secs());
    use contract::Step::*;
    for step in [FocusTextField, Lock, Unlock, LogOut, LogIn, Unlock] {
        eprintln!("  - {}", step.instruction());
    }
    eprintln!("(The suite locks the screen itself before the last step.)\n");
    let report = contract::run(&platform, &mut |step| {
        eprintln!("NOW: {}", step.instruction());
        std::thread::sleep(STEP_TIME);
    });
    print!("{}", report.table());
    std::process::exit(if report.failures().is_empty() { 0 } else { 1 });
}
