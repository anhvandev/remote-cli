use rcli_testkit::TestSystem;
use serde_json::Value;

fn samples(v: &Value) -> Vec<Value> {
    v["samples"].as_array().cloned().unwrap_or_default()
}

fn cpus(v: &Value) -> Vec<f64> {
    samples(v).iter().filter_map(|s| s["resources"]["cpu_percent"].as_f64()).collect()
}

#[tokio::test]
async fn rcli_metrics_history_lists_raw_samples_in_a_time_range() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent("lab-pc").await;
    agent.platform.update(|s| s.resources.cpu_percent = Some(10.0));
    let id = agent.device_id().to_owned();
    sys.wait_history(&id, "resolution=5s", |h| cpus(h).contains(&10.0)).await;

    sys.advance(60);
    agent.platform.update(|s| s.resources.cpu_percent = Some(20.0));
    sys.wait_history(&id, "resolution=5s", |h| cpus(h).contains(&20.0)).await;

    let all: Value =
        serde_json::from_str(&sys.rcli(&["metrics", "history", "lab-pc", "--json"]).await.unwrap()).unwrap();
    assert_eq!(all["resolution"], "5s");
    let first = samples(&all).into_iter().find(|s| s["resources"]["cpu_percent"] == 10.0).unwrap();
    // Stamped `Server now - age`, and a live sample is a few ms old when it arrives.
    let at = first["at"].as_str().unwrap();
    assert!(at > "2025-12-31T23:59:59" && at <= "2026-01-01T00:00:00Z", "{first}");
    // Only cpu changed on the wire; the stored sample is still complete.
    assert_eq!(first["resources"]["ram_used_bytes"], 4u64 << 30, "{first}");
    assert_eq!(first["resources"]["session"], "Active", "{first}");

    let later: Value = serde_json::from_str(
        &sys.rcli(&["metrics", "history", &id, "--from", "2026-01-01T00:00:30Z", "--json"]).await.unwrap(),
    )
    .unwrap();
    assert!(!cpus(&later).is_empty() && cpus(&later).iter().all(|c| *c == 20.0), "{later}");
    assert!(samples(&later).iter().all(|s| s["at"].as_str().unwrap() > "2026-01-01T00:00:59"), "{later}");

    let text = sys.rcli(&["metrics", "history", "lab-pc"]).await.unwrap();
    assert!(text.lines().any(|l| l.starts_with("2026-01-01T00:0") && l.contains("20.0")), "{text}");
}

#[tokio::test]
async fn samples_roll_up_per_minute_and_expire_after_7_and_90_days() {
    let sys = TestSystem::start().await;
    let mut agent = sys.start_agent("lab-pc").await;
    let id = agent.device_id().to_owned();
    // Away from the minute boundary: samples land a few ms before the (frozen) Server time.
    sys.advance(10);
    agent.platform.update(|s| s.resources.cpu_percent = Some(10.0));
    sys.wait_history(&id, "resolution=5s", |h| cpus(h).contains(&10.0)).await;
    sys.advance(30);
    agent.platform.update(|s| s.resources.cpu_percent = Some(30.0));
    sys.wait_history(&id, "resolution=5s", |h| cpus(h).contains(&30.0)).await;
    agent.stop();
    sys.wait_device(&id, |d| !d["last_seen_at"].is_null()).await;

    // 00:00:40 -> 00:01:40: minute 00:00 is complete.
    sys.advance(60);
    let range = "from=2026-01-01T00:00:00Z";
    let minute = sys.history(&id, &format!("{range}&resolution=1m")).await;
    assert_eq!(minute["resolution"], "1m");
    let rollups = samples(&minute);
    assert_eq!(rollups.len(), 1, "{minute}");
    assert_eq!(rollups[0]["at"], "2026-01-01T00:00:00Z");
    let avg = rollups[0]["resources"]["cpu_percent"].as_f64().unwrap();
    assert!(avg > 10.0 && avg < 30.0, "rollup averages the minute: {avg}");
    assert_eq!(rollups[0]["resources"]["session"], "Active");

    // Now 7 days after 00:00:40 minus one second: the 00:00:10 samples are gone, the 00:00:40 ones are not.
    sys.advance(7 * 86400 - 61);
    let raw = sys.history(&id, &format!("{range}&resolution=5s")).await;
    assert!(!cpus(&raw).is_empty() && cpus(&raw).iter().all(|c| *c == 30.0), "{raw}");
    sys.advance(1);
    assert!(samples(&sys.history(&id, &format!("{range}&resolution=5s")).await).is_empty());
    assert_eq!(samples(&sys.history(&id, &format!("{range}&resolution=1m")).await).len(), 1);
    // Defaults to rollups when the range reaches past the raw retention.
    assert_eq!(sys.history(&id, range).await["resolution"], "1m");

    sys.advance(83 * 86400);
    assert!(samples(&sys.history(&id, &format!("{range}&resolution=1m")).await).is_empty());
}

#[tokio::test]
async fn samples_taken_while_disconnected_are_backfilled_with_their_original_times() {
    let sys = TestSystem::start().await;
    let agent = sys.start_agent("lab-pc").await;
    let id = agent.device_id().to_owned();
    agent.platform.update(|s| s.resources.cpu_percent = Some(10.0));
    sys.wait_history(&id, "resolution=5s", |h| cpus(h).contains(&10.0)).await;

    agent.cut_network();
    sys.wait_device(&id, |d| !d["last_seen_at"].is_null()).await;
    agent.platform.update(|s| s.resources.cpu_percent = Some(77.0));
    // At least one 5 s sample is taken during the outage, and it is over a second old when it arrives.
    tokio::time::sleep(std::time::Duration::from_millis(6500)).await;
    agent.restore_network();

    // The Server clock stands still at 00:00:00, so a sample taken N s ago is stored at 23:59:(60-N);
    // over 1 s old = taken during the outage, not live after reconnect.
    let h = sys
        .wait_history(&id, "from=2025-12-31T23:00:00Z&resolution=5s", |h| {
            samples(h)
                .iter()
                .any(|s| s["resources"]["cpu_percent"] == 77.0 && s["at"].as_str().unwrap() < "2025-12-31T23:59:59")
        })
        .await;
    let backfilled: Vec<_> = samples(&h).into_iter().filter(|s| s["resources"]["cpu_percent"] == 77.0).collect();
    let earliest = backfilled[0]["at"].as_str().unwrap();
    assert!(earliest >= "2025-12-31T23:59:50Z", "{h}");
    let ats: Vec<_> = samples(&h).iter().map(|s| s["at"].as_str().unwrap().to_owned()).collect();
    assert!(ats.windows(2).all(|w| w[0] <= w[1]), "history is ordered by time: {ats:?}");
}
