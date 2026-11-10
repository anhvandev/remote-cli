//! Auth: users with a role (admin, operator) and their sessions.
//! The bootstrap token from config acts as an admin only until an enabled admin has a live session (first run).
//! A session is an API token with an expiry, from Dashboard sign-in or `rcli login`.

use chrono::{DateTime, Utc};
use serde::Serialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::AppState;

pub const ROLES: [&str; 2] = ["admin", "operator"];

#[derive(Serialize, Debug, Clone, sqlx::FromRow)]
pub struct User {
    pub id: Uuid,
    pub name: String,
    /// "admin", "operator", or "controller" for a password connection.
    pub role: String,
    pub disabled_at: Option<DateTime<Utc>>,
    /// Set for a password connection: the only Device it may reach.
    #[sqlx(default)]
    #[serde(skip)]
    pub device_scope: Option<Uuid>,
}

impl User {
    /// Server administration only: a password connection is never an admin.
    pub fn is_admin(&self) -> bool {
        self.role == "admin" && self.device_scope.is_none()
    }

    /// How the person at the Device sees this Controller: `<host> (<IP>)` for a password connection, `<name>
    /// (Dashboard)` for a Dashboard User. `rcli agent info` lists and `disconnect` matches this name.
    pub fn controller_display(&self) -> String {
        if self.device_scope.is_some() {
            self.name.clone()
        } else {
            format!("{} (Dashboard)", self.name)
        }
    }
}

/// Wrong Device Passwords in a row that lock a (Device ID, IP) pair out for a minute.
pub const MAX_FAILED_DEVICE_PASSWORDS: i32 = 5;
pub const PASSWORD_LOCKOUT: chrono::Duration = chrono::Duration::minutes(1);
/// The most a lockout grows to, however many times it repeats.
pub const PASSWORD_LOCKOUT_MAX: chrono::Duration = chrono::Duration::hours(1);
/// Failures of a pair not seen for this long start over.
// shortcut: the window is fixed, not sliding; a slower scanner can try a few per window indefinitely. Move to a
// sliding window if that shows up.
pub const PASSWORD_FAILURE_WINDOW: chrono::Duration = chrono::Duration::hours(24);

pub enum ControllerError {
    /// Wrong Server Key, Device ID or Device Password (no hint which).
    Unauthorized,
    /// Too many wrong passwords for this (Device ID, IP); nothing is checked until then.
    Locked(chrono::DateTime<chrono::Utc>),
}

/// A real check's cost for an unknown Device, so response time does not reveal whether the Device ID exists.
fn dummy_password_check(password: &str) {
    static DUMMY: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    let hash = DUMMY.get_or_init(|| rcli_protocol::password::hash("no such Device Password"));
    let _ = rcli_protocol::password::verify(password, hash);
}

/// The principal a password connection authenticates as: `<controller hostname> (<IP>)`, with a stable id per
/// (name, IP) so the Control Lock holder survives reconnects.
fn controller_user(controller: &str, ip: &str) -> User {
    let name = format!("{} ({ip})", controller.trim());
    let digest = Sha256::digest(name.as_bytes());
    let id_bytes = <[u8; 16]>::try_from(&digest[..16]).expect("16 bytes");
    User { id: Uuid::from_bytes(id_bytes), name, role: "controller".into(), disabled_at: None, device_scope: None }
}

/// A password connection: Server Key secret + Device ID + Device Password. It reaches only its own Device and has
/// no admin rights. Its display name is `<controller hostname> (<IP>)` in audit entries and as Control Lock holder.
/// Wrong Device Passwords are counted per (Device ID, IP): the fifth locks the pair out for a minute, each later
/// lockout doubles, and a correct password after that resets the count.
// shortcut: a wrong Server Key is not counted (it reveals nothing about a password); a leaked key still has to pass
// the limiter on the Device Password.
pub async fn authenticate_controller(
    state: &AppState,
    server_key: &str,
    device: &str,
    password: &str,
    controller: &str,
    ip: &str,
) -> anyhow::Result<Result<User, ControllerError>> {
    if !crate::server_key::admits(state, server_key).await {
        return Ok(Err(ControllerError::Unauthorized));
    }
    // A Device that exists is keyed by its UUID, so every way of naming it (Device ID, UUID, name) shares one
    // bucket; an unknown Device ID is keyed as presented, so it is counted too and cannot dodge the limiter.
    // An ambiguous name is treated as unknown: it gets no password check and no Device scope.
    let resolved = crate::registry::resolve(state, device).await?.ok().flatten();
    let bucket = resolved.map_or_else(|| device.to_owned(), |id| id.to_string());
    let now = state.clock.now();
    let locked_until: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT locked_until FROM device_password_failures WHERE device = $1 AND ip = $2")
            .bind(&bucket)
            .bind(ip)
            .fetch_optional(&state.db)
            .await?
            .flatten();
    if let Some(until) = locked_until.filter(|u| *u > now) {
        return Ok(Err(ControllerError::Locked(until)));
    }
    let Some(id) = resolved else {
        dummy_password_check(password);
        return password_failed(state, &bucket, ip, controller, None, now).await;
    };
    let stored: Option<String> =
        sqlx::query_scalar("SELECT device_password_hash FROM devices WHERE id = $1 AND revoked_at IS NULL")
            .bind(id)
            .fetch_optional(&state.db)
            .await?;
    let Some(stored) = stored.filter(|h| !h.is_empty()) else {
        dummy_password_check(password);
        return password_failed(state, &bucket, ip, controller, Some(id), now).await;
    };
    if !rcli_protocol::password::verify(password, &stored) {
        return password_failed(state, &bucket, ip, controller, Some(id), now).await;
    }
    sqlx::query("DELETE FROM device_password_failures WHERE device = $1 AND ip = $2")
        .bind(&bucket)
        .bind(ip)
        .execute(&state.db)
        .await?;
    let mut user = controller_user(controller, ip);
    user.device_scope = Some(id);
    Ok(Ok(user))
}

/// Counts one wrong Device Password (or unknown Device ID) for the pair; the one reaching
/// `MAX_FAILED_DEVICE_PASSWORDS` starts a lockout, doubling each time up to `PASSWORD_LOCKOUT_MAX`.
async fn password_failed(
    state: &AppState,
    bucket: &str,
    ip: &str,
    controller: &str,
    device: Option<Uuid>,
    now: chrono::DateTime<chrono::Utc>,
) -> anyhow::Result<Result<User, ControllerError>> {
    // One atomic upsert: counting with a read-then-write would lose failures under parallel requests, so a
    // parallel guesser would never reach the lockout. The lockout length is 1, 2, 4 ... minutes, capped at an hour.
    let locked_until: Option<chrono::DateTime<chrono::Utc>> = sqlx::query_scalar(
        "INSERT INTO device_password_failures AS f (device, ip, failures, lockouts, locked_until, seen_at)
         VALUES ($1, $2, 1, 0, NULL, $3)
         ON CONFLICT (device, ip) DO UPDATE SET
            failures = CASE
                WHEN f.seen_at < $3 - $4::interval THEN 1
                WHEN f.failures + 1 >= $5 THEN 0
                ELSE f.failures + 1 END,
            lockouts = CASE
                WHEN f.seen_at < $3 - $4::interval THEN 0
                WHEN f.failures + 1 >= $5 THEN f.lockouts + 1
                ELSE f.lockouts END,
            locked_until = CASE
                WHEN f.seen_at < $3 - $4::interval THEN NULL
                WHEN f.failures + 1 >= $5 THEN LEAST(
                    $3 + $6::interval * power(2, LEAST(f.lockouts, 6))::int, $3 + $7::interval)
                ELSE NULL END,
            seen_at = $3
         RETURNING locked_until",
    )
    .bind(bucket)
    .bind(ip)
    .bind(now)
    .bind(PASSWORD_FAILURE_WINDOW)
    .bind(MAX_FAILED_DEVICE_PASSWORDS)
    .bind(PASSWORD_LOCKOUT)
    .bind(PASSWORD_LOCKOUT_MAX)
    .fetch_one(&state.db)
    .await?;
    let user = controller_user(controller, ip);
    let device = device.unwrap_or(Uuid::nil());
    let recorded = async {
        crate::audit::record(state, &user, device, "connect.failed", serde_json::json!({}), serde_json::json!({ "error": "unauthorized" })).await?;
        if let Some(until) = locked_until {
            crate::audit::record(
                state,
                &user,
                device,
                "connect.locked",
                serde_json::json!({}),
                serde_json::json!({ "locked_until": until }),
            )
            .await?;
        }
        Ok::<_, anyhow::Error>(())
    }
    .await;
    if let Err(e) = recorded {
        tracing::error!("cannot audit a wrong Device Password: {e:#}");
    }
    Ok(Err(ControllerError::Unauthorized))
}

#[derive(Debug)]
pub enum Error {
    BadRequest(String),
    NameTaken,
    NotFound,
    Internal(anyhow::Error),
}

impl<E: Into<anyhow::Error>> From<E> for Error {
    fn from(e: E) -> Self {
        Self::Internal(e.into())
    }
}

fn hash(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

const USER_COLS: &str = "id, name, role, disabled_at";

/// The user a bearer token belongs to, if the token is live and the user enabled.
pub async fn authenticate(state: &AppState, token: &str) -> anyhow::Result<Option<User>> {
    let user = sqlx::query_as::<_, User>(&format!(
        "SELECT u.{} FROM api_tokens t JOIN users u ON u.id = t.user_id
         WHERE t.token_hash = $1 AND t.revoked_at IS NULL AND u.disabled_at IS NULL
           AND (t.expires_at IS NULL OR t.expires_at > $2)",
        USER_COLS.replace(", ", ", u.")
    ))
    .bind(hash(token))
    .bind(state.clock.now())
    .fetch_optional(&state.db)
    .await?;
    if user.is_some() || token != state.config.bootstrap_admin_token {
        return Ok(user);
    }
    let admin_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM api_tokens t JOIN users u ON u.id = t.user_id
         WHERE u.role = 'admin' AND u.disabled_at IS NULL AND t.revoked_at IS NULL
           AND (t.expires_at IS NULL OR t.expires_at > $1))",
    )
    .bind(state.clock.now())
    .fetch_one(&state.db)
    .await?;
    Ok((!admin_exists).then(|| User { id: Uuid::nil(), name: "bootstrap".into(), role: "admin".into(), disabled_at: None, device_scope: None }))
}

pub async fn create_user(state: &AppState, name: &str, role: &str) -> Result<User, Error> {
    // Names go into URL paths (`/users/{name}/...`), so keep them URL-safe.
    if name.is_empty() || name.len() > 64 || !name.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c)) {
        return Err(Error::BadRequest("name must be 1-64 characters of A-Z a-z 0-9 . _ -".into()));
    }
    if !ROLES.contains(&role) {
        return Err(Error::BadRequest(format!("role must be one of {ROLES:?}")));
    }
    sqlx::query_as::<_, User>(&format!(
        "INSERT INTO users (id, name, role, created_at) VALUES ($1, $2, $3, $4)
         ON CONFLICT (name) DO NOTHING RETURNING {USER_COLS}"
    ))
    .bind(Uuid::new_v4())
    .bind(name)
    .bind(role)
    .bind(state.clock.now())
    .fetch_optional(&state.db)
    .await?
    .ok_or(Error::NameTaken)
}

pub async fn list_users(state: &AppState) -> anyhow::Result<Vec<User>> {
    Ok(sqlx::query_as(&format!("SELECT {USER_COLS} FROM users ORDER BY name")).fetch_all(&state.db).await?)
}

/// Disables a user; all of their tokens stop working.
pub async fn disable_user(state: &AppState, name: &str) -> Result<User, Error> {
    sqlx::query_as::<_, User>(&format!(
        "UPDATE users SET disabled_at = COALESCE(disabled_at, $2) WHERE name = $1 RETURNING {USER_COLS}"
    ))
    .bind(name)
    .bind(state.clock.now())
    .fetch_optional(&state.db)
    .await?
    .ok_or(Error::NotFound)
}

/// Revokes the token itself (sign-out from `rcli logout` or the Dashboard).
pub async fn revoke_by_token(state: &AppState, token: &str) -> anyhow::Result<()> {
    sqlx::query("UPDATE api_tokens SET revoked_at = COALESCE(revoked_at, $2) WHERE token_hash = $1")
        .bind(hash(token))
        .bind(state.clock.now())
        .execute(&state.db)
        .await?;
    Ok(())
}

/// How long a Dashboard session lasts.
pub const SESSION_TTL: chrono::Duration = chrono::Duration::hours(12);
const PBKDF2_ITERATIONS: u32 = 600_000;
const TOTP_STEP_SECS: i64 = 30;

fn pbkdf2(password: &str, salt: &[u8], iterations: u32) -> [u8; 32] {
    let mut out = [0u8; 32];
    let n = std::num::NonZeroU32::new(iterations).unwrap();
    ring::pbkdf2::derive(ring::pbkdf2::PBKDF2_HMAC_SHA256, n, salt, password.as_bytes(), &mut out);
    out
}

fn random<const N: usize>() -> [u8; N] {
    ring::rand::generate(&ring::rand::SystemRandom::new()).expect("system RNG").expose()
}

/// Stored as `pbkdf2-sha256$<iterations>$<salt b64>$<hash b64>`.
fn hash_password(password: &str) -> String {
    use data_encoding::BASE64;
    let salt = random::<16>();
    let h = pbkdf2(password, &salt, PBKDF2_ITERATIONS);
    format!("pbkdf2-sha256${PBKDF2_ITERATIONS}${}${}", BASE64.encode(&salt), BASE64.encode(&h))
}

fn verify_password(password: &str, stored: &str) -> bool {
    use data_encoding::BASE64;
    let parts: Vec<&str> = stored.split('$').collect();
    let [_, iterations, salt, h] = parts[..] else { return false };
    let (Ok(iterations), Ok(salt), Ok(h)) = (iterations.parse(), BASE64.decode(salt.as_bytes()), BASE64.decode(h.as_bytes())) else {
        return false;
    };
    let n = std::num::NonZeroU32::new(iterations).unwrap_or(std::num::NonZeroU32::MIN);
    ring::pbkdf2::verify(ring::pbkdf2::PBKDF2_HMAC_SHA256, n, &salt, password.as_bytes(), &h).is_ok()
}

/// RFC 6238 TOTP (HMAC-SHA1, 30 s steps, 6 digits) for time `unix_secs`.
pub fn totp(secret: &[u8], unix_secs: i64) -> u32 {
    totp_at_step(secret, unix_secs.div_euclid(TOTP_STEP_SECS))
}

fn totp_at_step(secret: &[u8], step: i64) -> u32 {
    let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY, secret);
    let mac = ring::hmac::sign(&key, &step.to_be_bytes());
    let m = mac.as_ref();
    let i = (m[19] & 0x0f) as usize;
    (u32::from_be_bytes([m[i], m[i + 1], m[i + 2], m[i + 3]]) & 0x7fff_ffff) % 1_000_000
}

pub struct TotpEnrollment {
    /// Base32, for typing into an authenticator app.
    pub secret: String,
    /// `otpauth://` URL, for a QR code.
    pub url: String,
}

/// Sets a user's Dashboard password and enrolls a new TOTP secret (any previous one stops working).
pub async fn set_credentials(state: &AppState, name: &str, password: &str) -> Result<TotpEnrollment, Error> {
    if password.chars().count() < 8 {
        return Err(Error::BadRequest("password must be at least 8 characters".into()));
    }
    let secret = random::<20>();
    let id: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE name = $1")
        .bind(name)
        .fetch_optional(&state.db)
        .await?
        .ok_or(Error::NotFound)?;
    let (stored, sealed) = match &state.config.master_key {
        Some(key) => (key.seal_bytes(id.as_bytes(), &secret)?, true),
        None => (secret.to_vec(), false),
    };
    sqlx::query(
        "UPDATE users SET password_hash = $2, totp_secret = $3, totp_sealed = $4, totp_last_step = NULL WHERE id = $1",
    )
    .bind(id)
    .bind(hash_password(password))
    .bind(stored)
    .bind(sealed)
    .execute(&state.db)
    .await?;
    let secret = data_encoding::BASE32_NOPAD.encode(&secret);
    // `name` is URL-safe (validated in create_user).
    let url = format!("otpauth://totp/Remote%20CLI:{name}?secret={secret}&issuer=Remote%20CLI");
    Ok(TotpEnrollment { secret, url })
}

/// Failed sign-ins in a row (wrong password or TOTP code) that lock a user out for `LOCKOUT`.
pub const MAX_FAILED_SIGN_INS: i32 = 5;
pub const LOCKOUT: chrono::Duration = chrono::Duration::minutes(15);

pub enum SignInError {
    /// Wrong name, password or TOTP code (no hint which).
    Invalid,
    /// Too many failures; no credentials are checked until then.
    Locked(chrono::DateTime<chrono::Utc>),
}

/// Checks password + TOTP and opens a session (no hint which part failed).
/// A TOTP code is accepted for the current and the previous 30 s step, and each step only once.
/// `MAX_FAILED_SIGN_INS` failures in a row lock the user out for `LOCKOUT` (by the Server clock).
// shortcut: counted per user only, unknown names are not tracked; add a per-IP limit if name spraying becomes a problem.
pub async fn sign_in(
    state: &AppState,
    name: &str,
    password: &str,
    code: &str,
) -> anyhow::Result<Result<(User, String), SignInError>> {
    type Row = (Uuid, Option<String>, Option<Vec<u8>>, bool, Option<chrono::DateTime<chrono::Utc>>);
    let row: Option<Row> = sqlx::query_as(
        "SELECT id, password_hash, totp_secret, totp_sealed, sign_in_locked_until FROM users WHERE name = $1 AND disabled_at IS NULL",
    )
    .bind(name)
    .fetch_optional(&state.db)
    .await?;
    let (id, Some(stored), Some(secret), sealed, locked_until) = row.unwrap_or_default() else {
        // Same cost as a real check, so timing does not reveal which names exist.
        let _ = pbkdf2(password, b"no-such-user-salt", PBKDF2_ITERATIONS);
        return Ok(Err(SignInError::Invalid));
    };
    let now = state.clock.now();
    if let Some(until) = locked_until.filter(|u| *u > now) {
        return Ok(Err(SignInError::Locked(until)));
    }
    if !verify_password(password, &stored) {
        return failed(state, id).await;
    }
    let secret = match (sealed, &state.config.master_key) {
        (false, _) => secret,
        (true, Some(key)) => key.open_bytes(id.as_bytes(), &secret)?,
        (true, None) => anyhow::bail!("TOTP secrets are sealed: RCLI_MASTER_KEY is required"),
    };
    let step = now.timestamp().div_euclid(TOTP_STEP_SECS);
    let Some(step) = [step, step - 1].into_iter().find(|s| code == format!("{:06}", totp_at_step(&secret, *s))) else {
        return failed(state, id).await;
    };
    let fresh = sqlx::query(
        "UPDATE users SET totp_last_step = $2, failed_sign_ins = 0
         WHERE id = $1 AND (totp_last_step IS NULL OR totp_last_step < $2)",
    )
    .bind(id)
    .bind(step)
    .execute(&state.db)
    .await?;
    if fresh.rows_affected() == 0 {
        return failed(state, id).await; // replayed code
    }
    let token = format!("rcli_{}", Uuid::new_v4().simple());
    sqlx::query(
        "INSERT INTO api_tokens (id, user_id, token_hash, created_at, expires_at) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(Uuid::new_v4())
    .bind(id)
    .bind(hash(&token))
    .bind(now)
    .bind(now + SESSION_TTL)
    .execute(&state.db)
    .await?;
    let user = sqlx::query_as::<_, User>(&format!("SELECT {USER_COLS} FROM users WHERE id = $1"))
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    Ok(Ok((user, token)))
}

/// Seals TOTP secrets still stored in plaintext (from before the master key was configured).
pub async fn seal_totp_secrets(db: &sqlx::PgPool, key: &crate::master_key::MasterKey) -> anyhow::Result<()> {
    let plain: Vec<(Uuid, Vec<u8>)> =
        sqlx::query_as("SELECT id, totp_secret FROM users WHERE totp_secret IS NOT NULL AND NOT totp_sealed").fetch_all(db).await?;
    for (id, secret) in plain {
        sqlx::query("UPDATE users SET totp_secret = $2, totp_sealed = TRUE WHERE id = $1 AND NOT totp_sealed")
            .bind(id)
            .bind(key.seal_bytes(id.as_bytes(), &secret)?)
            .execute(db)
            .await?;
    }
    Ok(())
}

/// Counts a failed sign-in; the one that reaches `MAX_FAILED_SIGN_INS` starts the lockout (and resets the count).
async fn failed(state: &AppState, id: Uuid) -> anyhow::Result<Result<(User, String), SignInError>> {
    sqlx::query(
        "UPDATE users SET
            sign_in_locked_until = CASE WHEN failed_sign_ins + 1 >= $2 THEN $3 END,
            failed_sign_ins = CASE WHEN failed_sign_ins + 1 >= $2 THEN 0 ELSE failed_sign_ins + 1 END
         WHERE id = $1",
    )
    .bind(id)
    .bind(MAX_FAILED_SIGN_INS)
    .bind(state.clock.now() + LOCKOUT)
    .execute(&state.db)
    .await?;
    Ok(Err(SignInError::Invalid))
}

#[cfg(test)]
mod tests {
    #[test]
    fn totp_matches_rfc_6238_vectors() {
        // RFC 6238 appendix B, SHA-1, last 6 of the 8 digits.
        let secret = b"12345678901234567890";
        assert_eq!(super::totp(secret, 59), 287082);
        assert_eq!(super::totp(secret, 1111111109), 81804);
        assert_eq!(super::totp(secret, 2000000000), 279037);
    }

    #[test]
    fn password_hash_round_trips() {
        let h = super::hash_password("correct horse");
        assert!(super::verify_password("correct horse", &h));
        assert!(!super::verify_password("wrong", &h));
    }
}
