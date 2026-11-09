use rcli_testkit::{Clock, StatusCode, TestSystem};
use serde_json::json;

#[tokio::test]
async fn user_signs_in_with_password_and_totp_and_gets_a_session() {
    let sys = TestSystem::start().await;
    sys.create_user("vanna", "operator").await;
    let (status, enrolled) =
        sys.api.post_json("/api/v1/users/vanna/credentials", &json!({"password": "correct horse"})).await;
    assert_eq!(status, StatusCode::OK, "{enrolled}");
    let secret = enrolled["totp_secret"].as_str().unwrap();
    assert!(enrolled["otpauth_url"].as_str().unwrap().starts_with("otpauth://totp/"));

    let anon = sys.api.with_token(None);
    let code = sys.totp(secret);
    let (status, session) =
        anon.post_json("/api/v1/sessions", &json!({"name": "vanna", "password": "correct horse", "totp": code})).await;
    assert_eq!(status, StatusCode::CREATED, "{session}");
    let me = sys.api.with_token(session["token"].as_str());
    let (status, user) = me.get("/api/v1/me").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(user["name"], "vanna");
    assert_eq!(user["role"], "operator");

    // Signing out ends the session.
    assert_eq!(me.delete("/api/v1/session").await.0, StatusCode::NO_CONTENT);
    assert_eq!(me.get("/api/v1/me").await.0, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn sign_in_is_refused_without_the_right_password_and_a_fresh_totp() {
    let sys = TestSystem::start().await;
    sys.create_user("vanna", "operator").await;
    let (_, enrolled) = sys.api.post_json("/api/v1/users/vanna/credentials", &json!({"password": "pw-1234567"})).await;
    let code = sys.totp(enrolled["totp_secret"].as_str().unwrap());
    let wrong_code = format!("{:06}", (code.parse::<u32>().unwrap() + 1) % 1_000_000);
    let anon = &sys.api.with_token(None);
    let sign_in = |name: &str, password: &str, totp: &str| {
        let body = json!({"name": name, "password": password, "totp": totp});
        async move { anon.post_json("/api/v1/sessions", &body).await }
    };

    for (name, password, totp) in
        [("vanna", "wrong", code.as_str()), ("vanna", "pw-1234567", wrong_code.as_str()), ("nobody", "pw-1234567", code.as_str())]
    {
        let (status, body) = sign_in(name, password, totp).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{name}/{password}/{totp}: {body}");
        assert_eq!(body["error"]["code"], "invalid_credentials");
    }
    assert_eq!(sign_in("vanna", "pw-1234567", &code).await.0, StatusCode::CREATED);
    // The same code cannot be replayed.
    assert_eq!(sign_in("vanna", "pw-1234567", &code).await.0, StatusCode::UNAUTHORIZED);
    // The next 30 s step brings a fresh code.
    sys.advance(30);
    assert_eq!(sign_in("vanna", "pw-1234567", &sys.totp(enrolled["totp_secret"].as_str().unwrap())).await.0, StatusCode::CREATED);
}

#[tokio::test]
async fn sessions_expire_and_disabled_users_cannot_sign_in() {
    let sys = TestSystem::start().await;
    sys.create_user("vanna", "operator").await;
    let (_, enrolled) = sys.api.post_json("/api/v1/users/vanna/credentials", &json!({"password": "pw-1234567"})).await;
    let secret = enrolled["totp_secret"].as_str().unwrap();
    let anon = sys.api.with_token(None);
    let (_, session) = anon
        .post_json("/api/v1/sessions", &json!({"name": "vanna", "password": "pw-1234567", "totp": sys.totp(secret)}))
        .await;
    let me = sys.api.with_token(session["token"].as_str());
    assert_eq!(me.get("/api/v1/devices").await.0, StatusCode::OK);

    let mut sys = sys;
    sys.advance(12 * 3600);
    assert_eq!(me.get("/api/v1/devices").await.0, StatusCode::UNAUTHORIZED);
    sys.sign_in_admin().await; // the harness admin's session expired too

    sys.api.post("/api/v1/users/vanna/disable").await;
    let (status, _) = anon
        .post_json("/api/v1/sessions", &json!({"name": "vanna", "password": "pw-1234567", "totp": sys.totp(secret)}))
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn only_admins_set_credentials_and_passwords_must_be_8_chars() {
    let sys = TestSystem::start().await;
    let op = sys.api.with_token(Some(&sys.create_user("op", "operator").await));
    let (status, _) = op.post_json("/api/v1/users/op/credentials", &json!({"password": "longenough"})).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, body) = sys.api.post_json("/api/v1/users/op/credentials", &json!({"password": "short"})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let (status, _) = sys.api.post_json("/api/v1/users/ghost/credentials", &json!({"password": "longenough"})).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn admin_sets_credentials_with_rcli() {
    let sys = TestSystem::start().await;
    sys.create_user("vanna", "admin").await;
    let out = sys.rcli(&["user", "set-password", "vanna", "--password", "pw-1234567"]).await.unwrap();
    assert!(out.contains("otpauth://totp/"), "{out}");
    let secret = out.lines().find_map(|l| l.strip_prefix("totp_secret ")).unwrap();
    let (status, _) = sys
        .api
        .with_token(None)
        .post_json("/api/v1/sessions", &json!({"name": "vanna", "password": "pw-1234567", "totp": sys.totp(secret)}))
        .await;
    assert_eq!(status, StatusCode::CREATED);
}

#[tokio::test]
async fn five_failed_sign_ins_lock_the_user_out_for_15_minutes() {
    let sys = TestSystem::start().await;
    sys.create_user("vanna", "operator").await;
    let (_, enrolled) = sys.api.post_json("/api/v1/users/vanna/credentials", &json!({"password": "pw-1234567"})).await;
    let secret = enrolled["totp_secret"].as_str().unwrap();
    let anon = &sys.api.with_token(None);
    let sign_in = |password: &str, totp: String| {
        let body = json!({"name": "vanna", "password": password, "totp": totp});
        async move { anon.post_json("/api/v1/sessions", &body).await }
    };

    // A success resets the count.
    for _ in 0..4 {
        assert_eq!(sign_in("wrong", sys.totp(secret)).await.0, StatusCode::UNAUTHORIZED);
    }
    assert_eq!(sign_in("pw-1234567", sys.totp(secret)).await.0, StatusCode::CREATED);
    sys.advance(30); // fresh TOTP step
    for _ in 0..5 {
        assert_eq!(sign_in("wrong", sys.totp(secret)).await.0, StatusCode::UNAUTHORIZED);
    }

    // Locked: even the right password and code are refused, with a typed error saying until when.
    let (status, body) = sign_in("pw-1234567", sys.totp(secret)).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{body}");
    assert_eq!(body["error"]["code"], "too_many_attempts");
    let until: chrono::DateTime<chrono::Utc> = body["error"]["locked_until"].as_str().unwrap().parse().unwrap();
    assert_eq!(until, sys.clock.now() + chrono::Duration::minutes(15));

    sys.advance(14 * 60);
    assert_eq!(sign_in("pw-1234567", sys.totp(secret)).await.0, StatusCode::TOO_MANY_REQUESTS);
    sys.advance(60);
    assert_eq!(sign_in("pw-1234567", sys.totp(secret)).await.0, StatusCode::CREATED);
}
