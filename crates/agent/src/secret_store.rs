//! Secret Store port: where the Device keeps its Device Password at rest.
//! This ticket ships the encrypted-file fallback; DPAPI, the macOS System Keychain and systemd-creds plug in here
//! (tickets 16/22/24) without touching callers.

use std::path::{Path, PathBuf};

use ring::{
    aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM, NONCE_LEN},
    rand::{SecureRandom, SystemRandom},
};

/// A named secret kept encrypted on the Device.
pub trait SecretStore: Send + Sync {
    fn get(&self, name: &str) -> anyhow::Result<Option<String>>;
    fn set(&self, name: &str, secret: &str) -> anyhow::Result<()>;
    /// Forgets a secret; forgetting one that is not there is not an error.
    fn remove(&self, name: &str) -> anyhow::Result<()>;
}

/// `<dir>/.secrets.key` (32 random bytes, mode 0600) encrypts each secret in its own file with AES-256-GCM.
pub struct FileSecretStore {
    dir: PathBuf,
}

impl FileSecretStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    fn key(&self) -> anyhow::Result<LessSafeKey> {
        let path = self.dir.join(".secrets.key");
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(_) => {
                std::fs::create_dir_all(&self.dir)?;
                let mut bytes = vec![0u8; 32];
                SystemRandom::new().fill(&mut bytes).map_err(|_| anyhow::anyhow!("no randomness"))?;
                write_private(&path, &bytes)?;
                bytes
            }
        };
        let key = <[u8; 32]>::try_from(bytes.as_slice()).map_err(|_| anyhow::anyhow!("bad secret key file"))?;
        Ok(LessSafeKey::new(UnboundKey::new(&AES_256_GCM, &key).map_err(|_| anyhow::anyhow!("bad secret key file"))?))
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.join(format!(".{name}"))
    }
}

impl SecretStore for FileSecretStore {
    fn get(&self, name: &str) -> anyhow::Result<Option<String>> {
        let sealed = match std::fs::read(self.path(name)) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        anyhow::ensure!(sealed.len() > NONCE_LEN, "sealed secret too short");
        let (nonce, data) = sealed.split_at(NONCE_LEN);
        let mut data = data.to_vec();
        let nonce = Nonce::try_assume_unique_for_key(nonce).map_err(|_| anyhow::anyhow!("bad secret nonce"))?;
        let plain = self.key()?.open_in_place(nonce, Aad::from(name.as_bytes()), &mut data).map_err(|_| anyhow::anyhow!("cannot open a stored secret"))?;
        Ok(Some(String::from_utf8(plain.to_vec())?))
    }

    fn set(&self, name: &str, secret: &str) -> anyhow::Result<()> {
        let mut nonce = [0u8; NONCE_LEN];
        SystemRandom::new().fill(&mut nonce).map_err(|_| anyhow::anyhow!("no randomness"))?;
        let mut data = secret.as_bytes().to_vec();
        self.key()?
            .seal_in_place_append_tag(Nonce::assume_unique_for_key(nonce), Aad::from(name.as_bytes()), &mut data)
            .map_err(|_| anyhow::anyhow!("cannot seal a secret"))?;
        write_private(&self.path(name), &[nonce.as_slice(), &data].concat())
    }

    fn remove(&self, name: &str) -> anyhow::Result<()> {
        match std::fs::remove_file(self.path(name)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

fn write_private(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    use std::io::Write;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
    // The key file is the root-only fallback: only root (or the Agent's account) reads it.
    opts.open(path)?.write_all(bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_are_encrypted_at_rest_and_round_trip() {
        let dir = std::env::temp_dir().join(format!("rcli-secret-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = FileSecretStore::new(&dir);
        store.set("device_password", "hunter2xyz").unwrap();
        assert_eq!(store.get("device_password").unwrap().as_deref(), Some("hunter2xyz"));
        assert!(store.get("absent").unwrap().is_none());
        let raw = std::fs::read(dir.join(".device_password")).unwrap();
        assert!(!raw.windows(10).any(|w| w == b"hunter2xyz"), "the password is not stored in plaintext");
        store.remove("device_password").unwrap();
        assert!(store.get("device_password").unwrap().is_none());
        store.remove("device_password").unwrap(); // forgetting a missing secret is fine
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(dir.join(".secrets.key")).unwrap().permissions().mode() & 0o777, 0o600);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
