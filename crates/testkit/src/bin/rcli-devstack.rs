//! The in-process system for the Dashboard's Playwright tests: Server on Postgres (testcontainers), a manual
//! clock, and FakePlatform Agents. Usage: `rcli-devstack <http-port> <info.json>`.
//! Writes `{api, admin_token, now}` to `info.json` once the fleet is ready; runs until SIGINT/SIGTERM.
//! Fleet: `alpha` Online (LockScreen, user "an", full inventory, a rolled-up minute of history), `bravo` Offline,
//! and never-connected `charlie`, `delta`, `echo` for the admin screen tests to rename/revoke/delete/merge.
//! `update` in info.json is a build signed with the key the Agents trust, for the update screen tests.
//! The clock starts at real time (the Dashboard picks history ranges by its own clock) and is frozen after seeding.

use rcli_testkit::{platform::SessionState, Clock, FakePlatform, TestAgent, TestSystem};
use rcli_agent::platform::{Disk, Display, Inventory, NetworkInterface};
use serde_json::json;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let [_, port, out] = &args[..] else { anyhow::bail!("usage: rcli-devstack <http-port> <info.json>") };
    let _ = std::fs::remove_file(out);
    let sys = TestSystem::start_on(format!("127.0.0.1:{port}").parse()?).await;
    sys.clock.advance(chrono::Utc::now() - sys.clock.now());

    let alpha = FakePlatform::new("alpha");
    alpha.set_session_state(SessionState::Locked);
    alpha.update(|s| {
        s.inventory = Inventory {
            os: "FakeOS".into(),
            os_version: "1.0".into(),
            hostname: "alpha".into(),
            cpu_model: "Fake CPU 9000".into(),
            cpu_cores: 8,
            ram_bytes: 16 << 30,
            disks: vec![Disk { name: "disk0".into(), mount: "/".into(), total_bytes: 512 << 30 }],
            gpus: vec!["Fake GPU".into()],
            network_interfaces: vec![NetworkInterface {
                name: "eth0".into(),
                mac: "02:00:00:00:00:01".into(),
                ips: vec!["10.0.0.11".into()],
            }],
            serial: "FAKE-0001".into(),
            displays: vec![Display { name: "Display 1".into(), width: 1920, height: 1080 }],
        };
        s.resources.logged_in_user = Some("an".into());
        s.resources.uptime_secs = Some(3 * 3600);
        s.resources.disk_used_percent.insert("/".into(), 40.0);
    });
    let mut agents = vec![];
    for platform in [alpha, FakePlatform::new("bravo")] {
        let creds = sys.enroll(&sys.create_enrollment_code().await, &platform).await?;
        let mut agent = TestAgent::new(sys.endpoint(), platform, creds);
        agent.start();
        sys.wait_device_details(agent.device_id(), |d| !d["inventory"].is_null() && !d["resources"].is_null()).await;
        agents.push(agent);
    }
    // bravo drops off and stays away past the Offline threshold.
    agents[1].stop();
    let bravo = agents[1].device_id().to_owned();
    sys.wait_device(&bravo, |d| !d["last_seen_at"].is_null()).await;
    sys.advance(31);
    sys.wait_device(&bravo, |d| d["status"] == "Offline").await;
    // Complete alpha's first minute of samples so it has a 1-minute rollup.
    sys.advance(60);
    let from = (sys.clock.now() - chrono::Duration::hours(1)).to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let query = format!("resolution=1m&from={from}");
    sys.wait_history(agents[0].device_id(), &query, |h| h["samples"].as_array().is_some_and(|s| !s.is_empty())).await;

    for name in ["charlie", "delta", "echo"] {
        sys.enroll(&sys.create_enrollment_code().await, &FakePlatform::new(name)).await?;
    }
    let (version, target, build) = ("9.9.9", "fake-x86_64", "fake agent build");
    let signature = rcli_protocol::update::sign(&sys.update_signing_key, version, target, build.as_bytes())?;

    let info = json!({
        "api": format!("http://{}", sys.server.http_addr),
        "admin_token": sys.admin_token,
        "now": sys.clock.now().timestamp(),
        "update": {"version": version, "target": target, "build": build,
                   "signature": data_encoding::HEXLOWER.encode(&signature)},
    });
    std::fs::write(out, info.to_string())?;
    println!("rcli-devstack ready: {info}");

    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = term.recv() => {} }
    let _ = std::fs::remove_file(out);
    Ok(()) // dropping `sys` removes the Postgres container
}
