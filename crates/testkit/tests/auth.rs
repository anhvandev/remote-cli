use rcli_testkit::{StatusCode, TestSystem};
use serde_json::json;

#[tokio::test]
async fn operator_can_list_devices_but_is_refused_admin_operations() {
    let sys = TestSystem::start().await;
    let token = sys.create_user("op", "operator").await;
    let op = sys.api.with_token(Some(&token));

    let (status, _) = op.get("/api/v1/devices").await;
    assert_eq!(status, StatusCode::OK);

    for (status, body) in [
        op.post("/api/v1/enrollment-codes").await,
        op.post_json("/api/v1/users", &json!({"name": "x", "role": "admin"})).await,
        op.get("/api/v1/users").await,
        op.post("/api/v1/users/op/tokens").await,
    ] {
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
        assert_eq!(body["error"]["code"], "admin_required");
    }
}

#[tokio::test]
async fn revoked_tokens_and_disabled_users_are_rejected() {
    let sys = TestSystem::start().await;
    let (status, user) = sys.api.post_json("/api/v1/users", &json!({"name": "ai-1", "role": "operator"})).await;
    assert_eq!(status, StatusCode::CREATED, "{user}");
    let (_, first) = sys.api.post("/api/v1/users/ai-1/tokens").await;
    let (_, second) = sys.api.post("/api/v1/users/ai-1/tokens").await;
    let as_token = |t: &serde_json::Value| sys.api.with_token(t["token"].as_str());

    let (status, _) = sys.api.post(&format!("/api/v1/tokens/{}/revoke", first["id"].as_str().unwrap())).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, body) = as_token(&first).get("/api/v1/devices").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"]["code"], "unauthorized");
    assert_eq!(as_token(&second).get("/api/v1/devices").await.0, StatusCode::OK);

    let (status, user) = sys.api.post("/api/v1/users/ai-1/disable").await;
    assert_eq!(status, StatusCode::OK);
    assert!(!user["disabled_at"].is_null());
    assert_eq!(as_token(&second).get("/api/v1/devices").await.0, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn bootstrap_token_works_only_until_an_admin_has_a_token() {
    let sys = TestSystem::start().await;
    let bootstrap = sys.api.with_token(Some(rcli_testkit::BOOTSTRAP_TOKEN));
    // The harness already created its admin user and token through the bootstrap token.
    let (status, body) = bootstrap.get("/api/v1/devices").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");

    let (status, users) = sys.api.get("/api/v1/users").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(users[0]["role"], "admin");
}

#[tokio::test]
async fn admin_manages_users_and_tokens_with_rcli() {
    let sys = TestSystem::start().await;
    sys.rcli(&["user", "create", "alice", "--role", "operator"]).await.unwrap();
    let list = sys.rcli(&["user", "list"]).await.unwrap();
    let row = list.lines().find(|l| l.contains("alice")).expect(&list);
    assert!(row.contains("operator") && row.contains("enabled"), "{list}");

    let issued: serde_json::Value =
        serde_json::from_str(&sys.rcli(&["token", "create", "alice", "--json"]).await.unwrap()).unwrap();
    let token = issued["token"].as_str().unwrap();
    sys.rcli_as(token, &["devices"]).await.unwrap();
    let err = sys.rcli_as(token, &["enrollment-code", "create"]).await.unwrap_err();
    assert!(err.to_string().contains("admin_required"), "{err}");

    sys.rcli(&["token", "revoke", issued["id"].as_str().unwrap()]).await.unwrap();
    let err = sys.rcli_as(token, &["devices"]).await.unwrap_err();
    assert!(err.to_string().contains("unauthorized"), "{err}");

    let text = sys.rcli(&["token", "create", "alice"]).await.unwrap();
    let second = text.split_whitespace().last().unwrap().to_owned();
    sys.rcli_as(&second, &["devices"]).await.unwrap();
    sys.rcli(&["user", "disable", "alice"]).await.unwrap();
    assert!(sys.rcli_as(&second, &["devices"]).await.is_err());
    assert!(sys.rcli(&["user", "list"]).await.unwrap().lines().any(|l| l.contains("alice") && l.contains("disabled")));
}

#[tokio::test]
async fn admin_lists_live_api_tokens_without_secrets_or_dashboard_sessions() {
    let sys = TestSystem::start().await;
    sys.api.post_json("/api/v1/users", &json!({"name": "ai-1", "role": "operator"})).await;
    let (_, first) = sys.api.post("/api/v1/users/ai-1/tokens").await;
    let (_, second) = sys.api.post("/api/v1/users/ai-1/tokens").await;
    sys.api.post(&format!("/api/v1/tokens/{}/revoke", first["id"].as_str().unwrap())).await;
    let (_, creds) = sys.api.post_json("/api/v1/users/ai-1/credentials", &json!({"password": "correct horse"})).await;
    let totp = sys.totp(creds["totp_secret"].as_str().unwrap());
    let (status, _) =
        sys.api.post_json("/api/v1/sessions", &json!({"name": "ai-1", "password": "correct horse", "totp": totp})).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, tokens) = sys.api.get("/api/v1/tokens").await;
    assert_eq!(status, StatusCode::OK, "{tokens}");
    let mine: Vec<_> = tokens.as_array().unwrap().iter().filter(|t| t["user"] == "ai-1").collect();
    assert_eq!(mine.len(), 1, "{tokens}");
    assert_eq!(mine[0]["id"], second["id"]);
    assert!(mine[0]["created_at"].is_string());
    assert!(!tokens.to_string().contains("rcli_"), "token secrets must never be listed");

    let op = sys.api.with_token(second["token"].as_str());
    assert_eq!(op.get("/api/v1/tokens").await.0, StatusCode::FORBIDDEN);
}
