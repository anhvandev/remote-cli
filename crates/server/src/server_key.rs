//! Server Key: `rk_<CA fingerprint>_<secret>`. The fingerprint pins the Server's CA; the secret lets a
//! Device (or Controller) connect. Admins read it; replacing it changes only the secret.

use sqlx::PgPool;

use crate::pki::Ca;

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
    Ok(format!("rk_{}_{secret}", ca.fingerprint))
}

/// Replaces only the secret; the CA (and so the fingerprint) is unchanged, so registered Devices keep working.
pub async fn rotate(db: &PgPool, ca: &Ca) -> anyhow::Result<String> {
    let secret = random_secret()?;
    sqlx::query("UPDATE server_ca SET server_key_secret = $1").bind(&secret).execute(db).await?;
    Ok(format!("rk_{}_{secret}", ca.fingerprint))
}

/// `rk_<fingerprint>_<secret>` as `(fingerprint, secret)`.
pub fn split(key: &str) -> Option<(&str, &str)> {
    let (fingerprint, secret) = key.trim().strip_prefix("rk_")?.split_once('_')?;
    (fingerprint.len() == 64 && !secret.is_empty()).then_some((fingerprint, secret))
}

/// Whether `presented` holds `expected`, in constant time.
pub fn secret_matches(expected: &str, presented: &str) -> bool {
    let (a, b) = (expected.as_bytes(), presented.as_bytes());
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn random_secret() -> anyhow::Result<String> {
    use ring::rand::SecureRandom;
    let mut bytes = [0u8; SECRET_LEN / 2];
    ring::rand::SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| anyhow::anyhow!("cannot generate a Server Key secret"))?;
    Ok(hex::encode(bytes))
}
