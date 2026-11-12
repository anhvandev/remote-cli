//! The Server master key seals the CA private key and TOTP secrets at rest.

use rcli_testkit::{FakePlatform, StatusCode, TestSystem};
use serde_json::json;

/// What is stored at rest: (CA key in plaintext?, `vanna`'s raw TOTP secret column).
async fn at_rest(sys: &TestSystem) -> (bool, Vec<u8>) {
    let db = sqlx::PgPool::connect(&sys.config.database_url).await.unwrap();
    let plain_ca: bool = sqlx::query_scalar("SELECT key_pem IS NOT NULL FROM server_ca").fetch_one(&db).await.unwrap();
    let totp: Vec<u8> = sqlx::query_scalar("SELECT totp_secret FROM users WHERE name = 'vanna'").fetch_one(&db).await.unwrap();
    (plain_ca, totp)
}

#[tokio::test]
async fn the_master_key_encrypts_the_ca_key_and_totp_secrets_and_seals_old_plaintext_on_startup() {
    // Without a master key everything still works, in plaintext.
    let mut sys = TestSystem::start_without_master_key().await;
    sys.create_user("vanna").await;
    let (_, enrolled) = sys.api.post_json("/api/v1/users/vanna/credentials", &json!({"password": "pw-1234567"})).await;
    let secret = enrolled["totp_secret"].as_str().unwrap().to_owned();
    let raw = data_encoding::BASE32_NOPAD.decode(secret.as_bytes()).unwrap();
    assert_eq!(at_rest(&sys).await, (true, raw.clone()));
    let (_, ca) = sys.api.get("/api/v1/server-ca").await;

    sys.restart(Some(TestSystem::master_key())).await;
    let (plain_ca, totp) = at_rest(&sys).await;
    assert!(!plain_ca, "CA key sealed");
    assert!(!totp.windows(raw.len()).any(|w| w == raw), "TOTP secret sealed");
    // Same CA, and it still signs: Agents keep trusting the Server and new Devices enroll.
    assert_eq!(sys.api.get("/api/v1/server-ca").await.1, ca);
    sys.register(&FakePlatform::new("new-pc")).await.unwrap();
    // The sealed TOTP secret still signs the user in.
    let body = json!({"name": "vanna", "password": "pw-1234567", "totp": sys.totp(&secret)});
    let (status, body) = sys.api.with_token(None).post_json("/api/v1/sessions", &body).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    // New secrets are sealed right away.
    sys.advance(30);
    let (_, enrolled) = sys.api.post_json("/api/v1/users/vanna/credentials", &json!({"password": "pw-1234567"})).await;
    let raw = data_encoding::BASE32_NOPAD.decode(enrolled["totp_secret"].as_str().unwrap().as_bytes()).unwrap();
    assert!(!at_rest(&sys).await.1.windows(raw.len()).any(|w| w == raw));
}
