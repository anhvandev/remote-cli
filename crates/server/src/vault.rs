//! Secrets Vault: per-Device unlock passwords, encrypted with the server master key.
//! A password leaves the Server only inside an `ActRequest` to that Device's Agent; nothing here returns it to the API.

use chrono::{DateTime, Utc};
use ring::{
    aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM, NONCE_LEN},
    rand::{SecureRandom, SystemRandom},
};
use uuid::Uuid;

use crate::AppState;

/// 32-byte AES-256-GCM key (`RCLI_MASTER_KEY`, 64 hex chars). Debug never prints it.
#[derive(Clone)]
pub struct MasterKey([u8; 32]);

impl std::fmt::Debug for MasterKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MasterKey(***)")
    }
}

impl std::str::FromStr for MasterKey {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> anyhow::Result<Self> {
        let bytes = hex::decode(s.trim()).ok().and_then(|b| <[u8; 32]>::try_from(b).ok());
        bytes.map(Self).ok_or_else(|| anyhow::anyhow!("RCLI_MASTER_KEY must be 64 hex characters (32 bytes)"))
    }
}

impl MasterKey {
    fn key(&self) -> LessSafeKey {
        LessSafeKey::new(UnboundKey::new(&AES_256_GCM, &self.0).expect("32-byte key"))
    }

    fn seal(&self, device: Uuid, secret: &str) -> anyhow::Result<Vec<u8>> {
        self.seal_bytes(device.as_bytes(), secret.as_bytes())
    }

    fn open(&self, device: Uuid, sealed: &[u8]) -> anyhow::Result<String> {
        Ok(String::from_utf8(self.open_bytes(device.as_bytes(), sealed)?)?)
    }

    /// nonce || ciphertext || tag. `aad` binds the result to its owner (e.g. a row id): it only opens with the same `aad`.
    pub fn seal_bytes(&self, aad: &[u8], plain: &[u8]) -> anyhow::Result<Vec<u8>> {
        let mut nonce = [0u8; NONCE_LEN];
        SystemRandom::new().fill(&mut nonce).map_err(|_| anyhow::anyhow!("no randomness"))?;
        let mut data = plain.to_vec();
        self.key()
            .seal_in_place_append_tag(Nonce::assume_unique_for_key(nonce), Aad::from(aad), &mut data)
            .map_err(|_| anyhow::anyhow!("seal failed"))?;
        Ok([nonce.as_slice(), &data].concat())
    }

    pub fn open_bytes(&self, aad: &[u8], sealed: &[u8]) -> anyhow::Result<Vec<u8>> {
        anyhow::ensure!(sealed.len() > NONCE_LEN, "sealed secret too short");
        let (nonce, data) = sealed.split_at(NONCE_LEN);
        let mut data = data.to_vec();
        let nonce = Nonce::try_assume_unique_for_key(nonce).map_err(|_| anyhow::anyhow!("bad nonce"))?;
        let plain = self
            .key()
            .open_in_place(nonce, Aad::from(aad), &mut data)
            .map_err(|_| anyhow::anyhow!("cannot decrypt a sealed secret (wrong RCLI_MASTER_KEY?)"))?;
        Ok(plain.to_vec())
    }
}

#[derive(Debug)]
pub enum Error {
    /// The Server has no `RCLI_MASTER_KEY`.
    NoMasterKey,
    /// No password stored for the Device.
    NotSet,
    Internal(anyhow::Error),
}

impl<E: Into<anyhow::Error>> From<E> for Error {
    fn from(e: E) -> Self {
        Self::Internal(e.into())
    }
}

fn master_key(state: &AppState) -> Result<&MasterKey, Error> {
    state.config.master_key.as_ref().ok_or(Error::NoMasterKey)
}

/// Stores (or replaces) the Device's unlock password.
pub async fn set(state: &AppState, device: Uuid, password: &str) -> Result<(), Error> {
    let sealed = master_key(state)?.seal(device, password)?;
    sqlx::query(
        "INSERT INTO unlock_passwords (device_id, sealed, updated_at) VALUES ($1, $2, $3)
         ON CONFLICT (device_id) DO UPDATE SET sealed = $2, updated_at = $3",
    )
    .bind(device)
    .bind(sealed)
    .bind(state.clock.now())
    .execute(&state.db)
    .await?;
    Ok(())
}

pub async fn clear(state: &AppState, device: Uuid) -> Result<(), Error> {
    sqlx::query("DELETE FROM unlock_passwords WHERE device_id = $1").bind(device).execute(&state.db).await?;
    Ok(())
}

/// When the password was last set; None = no password stored.
pub async fn updated_at(state: &AppState, device: Uuid) -> Result<Option<DateTime<Utc>>, Error> {
    Ok(sqlx::query_scalar("SELECT updated_at FROM unlock_passwords WHERE device_id = $1")
        .bind(device)
        .fetch_optional(&state.db)
        .await?)
}

/// The plaintext password, for sending to the Device's own Agent only.
pub async fn password(state: &AppState, device: Uuid) -> Result<String, Error> {
    let key = master_key(state)?;
    let sealed: Vec<u8> = sqlx::query_scalar("SELECT sealed FROM unlock_passwords WHERE device_id = $1")
        .bind(device)
        .fetch_optional(&state.db)
        .await?
        .ok_or(Error::NotSet)?;
    Ok(key.open(device, &sealed)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sealed_secret_opens_only_with_the_same_key_and_device() {
        let key: MasterKey = "11".repeat(32).parse().unwrap();
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        let sealed = key.seal(a, "hunter2").unwrap();
        assert!(!sealed.windows(7).any(|w| w == b"hunter2"));
        assert_ne!(sealed, key.seal(a, "hunter2").unwrap(), "fresh nonce each time");
        assert_eq!(key.open(a, &sealed).unwrap(), "hunter2");
        assert!(key.open(b, &sealed).is_err());
        let other: MasterKey = "22".repeat(32).parse().unwrap();
        assert!(other.open(a, &sealed).is_err());
        assert!("abcd".parse::<MasterKey>().is_err());
        assert_eq!(format!("{key:?}"), "MasterKey(***)");
    }
}
