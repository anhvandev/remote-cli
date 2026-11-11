//! The Server's master key (`RCLI_MASTER_KEY`): seals the CA private key and TOTP secrets at rest.

use ring::{
    aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM, NONCE_LEN},
    rand::{SecureRandom, SystemRandom},
};

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sealed_secret_opens_only_with_the_same_key_and_aad() {
        let key: MasterKey = "11".repeat(32).parse().unwrap();
        let (a, b) = (b"owner-a".as_slice(), b"owner-b".as_slice());
        let sealed = key.seal_bytes(a, b"hunter2").unwrap();
        assert!(!sealed.windows(7).any(|w| w == b"hunter2"));
        assert_ne!(sealed, key.seal_bytes(a, b"hunter2").unwrap(), "fresh nonce each time");
        assert_eq!(key.open_bytes(a, &sealed).unwrap(), b"hunter2");
        assert!(key.open_bytes(b, &sealed).is_err());
        let other: MasterKey = "22".repeat(32).parse().unwrap();
        assert!(other.open_bytes(a, &sealed).is_err());
        assert!("abcd".parse::<MasterKey>().is_err());
        assert_eq!(format!("{key:?}"), "MasterKey(***)");
    }
}
