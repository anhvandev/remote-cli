//! Server Key: `rk_<CA fingerprint>_<secret>`. The fingerprint pins the Server's CA; the secret lets a
//! Device (or Controller) connect. Admins read it; replacing it changes only the secret.

use rcli_protocol::server_key::ServerKey;
use sqlx::PgPool;

use crate::{pki::Ca, AppState};

pub const SECRET_LEN: usize = 32;

/// Loads the Server Key, creating its secret on first start. The CA is never regenerated.
pub async fn load_or_create(db: &PgPool, ca: &Ca) -> anyhow::Result<String> {
    let stored: String = sqlx::query_scalar("SELECT server_key_secret FROM server_ca").fetch_one(db).await?;
    let secret = match stored {
        secret if !secret.is_empty() => secret,
        _ => {
            let secret = random_secret()?;
            sqlx::query("UPDATE server_ca SET server_key_secret = $1").bind(&secret).execute(db).await?;
            secret
        }
    };
    Ok(ServerKey { fingerprint: ca.fingerprint.clone(), secret }.to_string())
}

/// Replaces only the secret; the CA (and so the fingerprint) is unchanged, so registered Devices keep working.
pub async fn rotate(db: &PgPool, ca: &Ca) -> anyhow::Result<String> {
    let secret = random_secret()?;
    sqlx::query("UPDATE server_ca SET server_key_secret = $1").bind(&secret).execute(db).await?;
    Ok(ServerKey { fingerprint: ca.fingerprint.clone(), secret }.to_string())
}

/// The current Server Key.
pub async fn current(state: &AppState) -> ServerKey {
    ServerKey::parse(&state.settings.key().await).expect("the Server writes its own key well-formed")
}

/// Whether the key `presented` carries the current Server Key's secret.
pub async fn admits(state: &AppState, presented: &str) -> bool {
    current(state).await.admits(presented)
}

fn random_secret() -> anyhow::Result<String> {
    use ring::rand::SecureRandom;
    let mut bytes = [0u8; SECRET_LEN / 2];
    ring::rand::SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| anyhow::anyhow!("cannot generate a Server Key secret"))?;
    Ok(hex::encode(bytes))
}
