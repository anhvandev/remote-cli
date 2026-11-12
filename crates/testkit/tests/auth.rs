use rcli_testkit::{StatusCode, temp_dir, TestSystem};
use serde_json::json;

#[tokio::test]
async fn operator_can_list_devices_but_is_refused_admin_operations() {
    let sys = TestSystem::start().await;
    let token = sys.create_user("op", "operator").await;
    let op = sys.api.with_token(Some(&token));

    let (status, _) = op.get("/api/v1/devices").await;
    assert_eq!(status, StatusCode::OK);

    for (status, body) in [
        op.get("/api/v1/server-key").await,
        op.post_json("/api/v1/users", &json!({"name": "x", "role": "admin"})).await,
        op.get("/api/v1/users").await,
        op.post_json("/api/v1/users/op/credentials", &json!({"password": "longenough"})).await,
    ] {
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
        assert_eq!(body["error"]["code"], "admin_required");
    }
}

#[tokio::test]
async fn signing_out_and_disabling_a_user_end_their_session() {
    let sys = TestSystem::start().await;
    let token = sys.create_user("ai-1", "operator").await;
    let me = sys.api.with_token(Some(&token));
    assert_eq!(me.get("/api/v1/devices").await.0, StatusCode::OK);

    assert_eq!(me.delete("/api/v1/session").await.0, StatusCode::NO_CONTENT);
    let (status, body) = me.get("/api/v1/devices").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"]["code"], "unauthorized");

    // A new session works until the user is disabled.
    let session = sys.create_user("ai-2", "operator").await;
    let me = sys.api.with_token(Some(&session));
    assert_eq!(me.get("/api/v1/devices").await.0, StatusCode::OK);
    let (status, user) = sys.api.post("/api/v1/users/ai-2/disable").await;
    assert_eq!(status, StatusCode::OK, "{user}");
    assert!(!user["disabled_at"].is_null());
    assert_eq!(me.get("/api/v1/devices").await.0, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn bootstrap_token_works_only_until_an_admin_has_a_session() {
    let sys = TestSystem::start().await;
    let bootstrap = sys.api.with_token(Some(rcli_testkit::BOOTSTRAP_TOKEN));
    // The harness already signed its admin in through the bootstrap token.
    let (status, body) = bootstrap.get("/api/v1/devices").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");

    let (status, users) = sys.api.get("/api/v1/users").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(users[0]["role"], "admin");
}

#[tokio::test]
async fn admin_signs_in_with_rcli_and_operators_are_refused_admin_commands() {
    let sys = TestSystem::start().await;
    let dir = temp_dir("admin");
    sys.rcli(&["user", "create", "alice", "--role", "operator"]).await.unwrap();
    let list = sys.rcli(&["user", "list"]).await.unwrap();
    let row = list.lines().find(|l| l.contains("alice")).expect(&list);
    assert!(row.contains("operator") && row.contains("enabled"), "{list}");
    let enrolled = sys.rcli(&["user", "set-password", "alice", "--password", TestSystem::PASSWORD]).await.unwrap();
    let secret = enrolled.lines().find_map(|l| l.strip_prefix("totp_secret ")).expect(&enrolled);
    sys.controller_config(&dir, None).await;

    // The Operator's session works for Device commands but every Admin command is refused.
    let out = sys.rcli_in(&dir, &["login", "alice", "--password", TestSystem::PASSWORD, "--totp", &sys.totp(secret)]).await.unwrap();
    assert!(out.contains("alice") && out.contains("operator"), "{out}");
    let err = sys.rcli_in(&dir, &["user", "list"]).await.unwrap_err();
    assert!(err.to_string().contains("admin_required"), "{err}");
}

#[tokio::test]
async fn rcli_login_saves_a_session_that_admin_commands_and_logout_use() {
    let sys = TestSystem::start().await;
    let dir = temp_dir("login");
    sys.controller_config(&dir, None).await;
    let secret = sys.create_dashboard_user("bob", "admin").await;
    let out = sys.rcli_in(&dir, &["login", "bob", "--password", TestSystem::PASSWORD, "--totp", &sys.totp(&secret)]).await.unwrap();
    assert!(out.contains("bob") && out.contains("admin"), "{out}");

    let config: serde_json::Value = serde_json::from_slice(&std::fs::read(dir.join("config.json")).unwrap()).unwrap();
    let token = config["session"].as_str().filter(|t| t.starts_with("rcli_")).unwrap_or_else(|| panic!("{config}")).to_owned();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(dir.join("config.json")).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "the session file must not be readable by other local users");
    }

    // Admin commands use the saved session.
    let out = sys.rcli_in(&dir, &["user", "list"]).await.unwrap();
    assert!(out.contains("bob"), "{out}");
    assert!(sys.rcli_in(&dir, &["server-key", "show"]).await.unwrap().starts_with("rk_"));
    let audit = sys.rcli_in(&dir, &["audit"]).await.unwrap();
    assert!(audit.starts_with("AT"), "{audit}");

    // Wrong credentials save nothing.
    let err = sys.rcli_in(&dir, &["login", "bob", "--password", "wrong", "--totp", &sys.totp(&secret)]).await.unwrap_err();
    assert!(err.to_string().contains("invalid_credentials"), "{err}");

    let out = sys.rcli_in(&dir, &["logout"]).await.unwrap();
    assert!(out.contains("signed out"), "{out}");
    let config: serde_json::Value = serde_json::from_slice(&std::fs::read(dir.join("config.json")).unwrap()).unwrap();
    assert!(config["session"].is_null(), "{config}");
    // `logout` revokes it at the Server too, not just locally.
    let (status, _) = sys.api.with_token(Some(&token)).get("/api/v1/devices").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let err = sys.rcli_in(&dir, &["user", "list"]).await.unwrap_err();
    assert!(err.to_string().contains("rcli login"), "{err}");
}

#[tokio::test]
async fn user_created_api_token_endpoints_are_gone() {
    let sys = TestSystem::start().await;
    for (status, _) in [
        sys.api.post("/api/v1/users/admin/tokens").await,
        sys.api.get("/api/v1/tokens").await,
        sys.api.post("/api/v1/tokens/00000000-0000-0000-0000-000000000000/revoke").await,
    ] {
        assert_eq!(status, StatusCode::NOT_FOUND);
    }
}


/// A token made by the removed `rcli token` never expires; it no longer signs anyone in.
#[tokio::test]
async fn an_old_api_token_without_an_expiry_is_refused() {
    let sys = TestSystem::start().await;
    let db = sqlx::PgPool::connect(&sys.config.database_url).await.unwrap();
    sqlx::query(
        "INSERT INTO api_tokens (id, user_id, token_hash, created_at)
         SELECT gen_random_uuid(), id, sha256(convert_to('rcli_old', 'UTF8')), now() FROM users WHERE name = 'admin'",
    )
    .execute(&db)
    .await
    .unwrap();
    let (status, body) = sys.api.with_token(Some("rcli_old")).get("/api/v1/devices").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
}
