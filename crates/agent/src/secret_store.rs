//! Secret Store port: where the Device keeps its Device Password at rest.
//! The encrypted-file fallback, and the System Keychain on macOS (ticket 24); DPAPI and systemd-creds plug in here
//! (tickets 16/22) without touching callers.

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
        let plain = self
            .key()?
            .open_in_place(nonce, Aad::from(name.as_bytes()), &mut data)
            .map_err(|_| anyhow::anyhow!("cannot open a stored secret"))?;
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

/// Linux: Device secrets encrypted by `systemd-creds`, in files under the Agent's state directory.
/// `--with-key=auto` uses the host key and the TPM when the Device has one, so a copied disk or backup does not
/// reveal the secret; the credential name is bound into the key, and system-scope credentials are readable only by
/// root (the root service's user binding). `systemd-creds` is not a link-time dependency, so a system without it
/// falls back to `FileSecretStore` (chosen by `rcli agent install`).
#[cfg(target_os = "linux")]
pub struct SystemdCredsSecretStore {
    dir: PathBuf,
    run: std::sync::Arc<CredsRunner>,
}

#[cfg(target_os = "linux")]
type CredsRunner = dyn Fn(&[&str], Option<&[u8]>) -> anyhow::Result<Vec<u8>> + Send + Sync;

/// Whether this Device can encrypt with `systemd-creds` (systemd built with the credential support).
pub fn systemd_creds_available() -> bool {
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("systemd-creds")
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

#[cfg(target_os = "linux")]
impl SystemdCredsSecretStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self::with_runner(dir, run_systemd_creds)
    }

    /// `new` with the command runner given (tests: no systemd, no TPM).
    pub fn with_runner(
        dir: impl Into<PathBuf>,
        run: impl Fn(&[&str], Option<&[u8]>) -> anyhow::Result<Vec<u8>> + Send + Sync + 'static,
    ) -> Self {
        Self { dir: dir.into(), run: std::sync::Arc::new(run) }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.join(format!(".{name}.cred"))
    }
}

#[cfg(target_os = "linux")]
impl SecretStore for SystemdCredsSecretStore {
    // The secret is already encrypted by systemd-creds; the file only needs to be root-only.
    fn get(&self, name: &str) -> anyhow::Result<Option<String>> {
        let path = self.path(name);
        if !path.exists() {
            return Ok(None);
        }
        let plain = (self.run)(&["decrypt", "--name", name, &path.display().to_string(), "-"], None)?;
        Ok(Some(String::from_utf8(plain)?))
    }

    fn set(&self, name: &str, secret: &str) -> anyhow::Result<()> {
        use std::os::unix::fs::PermissionsExt;
        std::fs::create_dir_all(&self.dir)?;
        let path = self.path(name);
        let temp = self.dir.join(format!(".{name}.new"));
        let _ = std::fs::remove_file(&temp);
        let args = ["encrypt", "--name", name, "--with-key", "auto", "-", &temp.display().to_string()];
        (self.run)(&args, Some(secret.as_bytes()))?;
        std::fs::set_permissions(&temp, std::fs::Permissions::from_mode(0o600))?;
        std::fs::rename(&temp, &path)?;
        Ok(())
    }

    fn remove(&self, name: &str) -> anyhow::Result<()> {
        for path in [self.path(name), self.dir.join(format!(".{name}.new"))] {
            match std::fs::remove_file(path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
        Ok(())
    }
}

/// Runs `systemd-creds` (the real runner): the ciphertext goes to the last argument, the plaintext comes
/// from stdin when `input` is given, else stdout carries the plaintext.
#[cfg(target_os = "linux")]
fn run_systemd_creds(args: &[&str], input: Option<&[u8]>) -> anyhow::Result<Vec<u8>> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut command = Command::new("systemd-creds");
    command.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
    command.stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() });
    let mut child = command
        .spawn()
        .map_err(|e| anyhow::anyhow!("systemd-creds is not available: {e}"))?;
    if let Some(input) = input {
        child.stdin.take().expect("piped stdin").write_all(input)?;
    }
    let out = child.wait_with_output()?;
    anyhow::ensure!(
        out.status.success(),
        "systemd-creds {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&out.stderr).trim()
    );
    Ok(out.stdout)
}

/// macOS: generic passwords in a keychain file, by default the System Keychain, which only root can write.
/// Never prompts: a call that would show a dialog fails instead (a root service has nobody to ask).
// shortcut: items trust the binary that stored them, and an unsigned build is trusted by its hash, so after an
// upgrade reading fails until the secret is set again; signed builds (out of scope) fix this.
#[cfg(target_os = "macos")]
pub struct KeychainSecretStore {
    path: PathBuf,
}

#[cfg(target_os = "macos")]
impl KeychainSecretStore {
    const SERVICE: &str = "dev.remotecli.agent";

    pub fn system() -> Self {
        Self::new("/Library/Keychains/System.keychain")
    }

    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    fn open(&self) -> anyhow::Result<security_framework::os::macos::keychain::SecKeychain> {
        security_framework::os::macos::keychain::SecKeychain::open(&self.path)
            .map_err(|e| anyhow::anyhow!("cannot open the keychain {}: {e}", self.path.display()))
    }
}

#[cfg(target_os = "macos")]
impl SecretStore for KeychainSecretStore {
    fn get(&self, name: &str) -> anyhow::Result<Option<String>> {
        let _quiet = security_framework::os::macos::keychain::SecKeychain::disable_user_interaction()?;
        match self.open()?.find_generic_password(Self::SERVICE, name) {
            Ok((password, _)) => Ok(Some(String::from_utf8(password.to_vec())?)),
            Err(e) if e.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(None),
            Err(e) => anyhow::bail!("cannot read {name} from the keychain: {e}"),
        }
    }

    fn set(&self, name: &str, secret: &str) -> anyhow::Result<()> {
        let _quiet = security_framework::os::macos::keychain::SecKeychain::disable_user_interaction()?;
        self.open()?
            .set_generic_password(Self::SERVICE, name, secret.as_bytes())
            .map_err(|e| anyhow::anyhow!("cannot store {name} in the keychain: {e}"))
    }

    fn remove(&self, name: &str) -> anyhow::Result<()> {
        let _quiet = security_framework::os::macos::keychain::SecKeychain::disable_user_interaction()?;
        match self.open()?.find_generic_password(Self::SERVICE, name) {
            Ok((_, item)) => item.delete(),
            Err(e) if e.code() == ERR_SEC_ITEM_NOT_FOUND => {}
            Err(e) => anyhow::bail!("cannot remove {name} from the keychain: {e}"),
        }
        Ok(())
    }
}

#[cfg(target_os = "macos")]
const ERR_SEC_ITEM_NOT_FOUND: i32 = -25300;

/// Windows: DPAPI machine scope (`CRYPTPROTECT_LOCAL_MACHINE`), one blob per secret in `<dir>`. Only this Device
/// can decrypt it, so a copied disk or backup does not reveal the Device Password (ticket 22). The installer ACLs
/// `%ProgramData%\rcli` to SYSTEM and Administrators.
// shortcut: DPAPI machine scope means any account on this Device can decrypt; the directory ACL, not the account, is
// the gate. Per-user scope would break the SYSTEM service reading what an elevated install wrote.
#[cfg(windows)]
pub struct DpapiSecretStore {
    dir: PathBuf,
}

#[cfg(windows)]
impl DpapiSecretStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }
}

/// Encrypts `plain` for this Device. The optional entropy is the secret's name, so a blob cannot be moved to
/// another name.
#[cfg(windows)]
fn protect(name: &str, plain: &[u8]) -> anyhow::Result<Vec<u8>> {
    use windows::{
        core::PCWSTR,
        Win32::{
            Foundation::{LocalFree, HLOCAL},
            Security::Cryptography::{CryptProtectData, CRYPTPROTECT_LOCAL_MACHINE, CRYPT_INTEGER_BLOB},
        },
    };
    let mut plain = plain.to_vec();
    let mut entropy = name.as_bytes().to_vec();
    let input = CRYPT_INTEGER_BLOB { cbData: plain.len() as u32, pbData: plain.as_mut_ptr() };
    let entropy = CRYPT_INTEGER_BLOB { cbData: entropy.len() as u32, pbData: entropy.as_mut_ptr() };
    let mut out = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptProtectData(&input, PCWSTR::null(), Some(&entropy), None, None, CRYPTPROTECT_LOCAL_MACHINE, &mut out)?;
        let sealed = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        let _ = LocalFree(Some(HLOCAL(out.pbData as *mut core::ffi::c_void)));
        Ok(sealed)
    }
}

#[cfg(windows)]
fn unprotect(name: &str, sealed: &[u8]) -> anyhow::Result<Vec<u8>> {
    use windows::Win32::{
        Foundation::{LocalFree, HLOCAL},
        Security::Cryptography::{CryptUnprotectData, CRYPTPROTECT_LOCAL_MACHINE, CRYPT_INTEGER_BLOB},
    };
    let mut sealed = sealed.to_vec();
    let mut entropy = name.as_bytes().to_vec();
    let input = CRYPT_INTEGER_BLOB { cbData: sealed.len() as u32, pbData: sealed.as_mut_ptr() };
    let entropy = CRYPT_INTEGER_BLOB { cbData: entropy.len() as u32, pbData: entropy.as_mut_ptr() };
    let mut out = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptUnprotectData(&input, None, Some(&entropy), None, None, CRYPTPROTECT_LOCAL_MACHINE, &mut out)?;
        let plain = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        let _ = LocalFree(Some(HLOCAL(out.pbData as *mut core::ffi::c_void)));
        Ok(plain)
    }
}

#[cfg(windows)]
impl SecretStore for DpapiSecretStore {
    fn get(&self, name: &str) -> anyhow::Result<Option<String>> {
        let sealed = match std::fs::read(self.path(name)) {
            Ok(sealed) => sealed,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        Ok(Some(String::from_utf8(unprotect(name, &sealed)?)?))
    }

    fn set(&self, name: &str, secret: &str) -> anyhow::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        write_private(&self.path(name), &protect(name, secret.as_bytes())?)
    }

    fn remove(&self, name: &str) -> anyhow::Result<()> {
        match std::fs::remove_file(self.path(name)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

#[cfg(windows)]
#[cfg(test)]
mod dpapi_tests {
    use super::*;

    #[test]
    fn dpapi_secrets_are_device_bound_and_round_trip() {
        let dir = std::env::temp_dir().join(format!("rcli-dpapi-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = DpapiSecretStore::new(&dir);
        assert!(store.get("device_password").unwrap().is_none());
        store.set("device_password", "hunter2xyz").unwrap();
        assert_eq!(store.get("device_password").unwrap().as_deref(), Some("hunter2xyz"));
        store.set("second_secret", "s3cret pässword").unwrap();
        assert_eq!(store.get("second_secret").unwrap().as_deref(), Some("s3cret pässword"));
        let raw = std::fs::read(dir.join("device_password")).unwrap();
        assert!(!raw.windows(10).any(|w| w == b"hunter2xyz"), "the password is not stored in plaintext");
        // A blob moved to another name does not decrypt: the name is the DPAPI entropy.
        std::fs::copy(dir.join("device_password"), dir.join("other")).unwrap();
        assert!(store.get("other").is_err());
        store.remove("device_password").unwrap();
        store.remove("device_password").unwrap();
        assert!(store.get("device_password").unwrap().is_none());
        let _ = std::fs::remove_dir_all(&dir);
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

    /// The systemd-creds store, with a runner that plays the CLI's role: it writes the ciphertext file named by
    /// the last argument and reads it back for decrypt. The real `systemd-creds` is exercised on a real Device.
    #[cfg(target_os = "linux")]
    #[test]
    fn systemd_creds_store_round_trips_with_the_name_bound_and_the_cli_never_leaks_the_secret() {
        use std::sync::{Arc, Mutex};
        let dir = std::env::temp_dir().join(format!("rcli-creds-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let calls: Arc<Mutex<Vec<String>>> = Default::default();
        let seen = calls.clone();
        let store = SystemdCredsSecretStore::with_runner(&dir, move |args, input| {
            seen.lock().unwrap().push(args.join(" "));
            match args[0] {
                "encrypt" => {
                    std::fs::write(args.last().unwrap(), input.unwrap_or_default())?;
                    Ok(vec![])
                }
                "decrypt" => Ok(std::fs::read(args[args.len() - 2])?),
                other => anyhow::bail!("unexpected systemd-creds {other}"),
            }
        });
        assert!(store.get("device_password").unwrap().is_none());
        store.set("device_password", "hunter2xyz").unwrap();
        assert_eq!(store.get("device_password").unwrap().as_deref(), Some("hunter2xyz"));
        store.remove("device_password").unwrap();
        store.remove("device_password").unwrap();
        assert!(store.get("device_password").unwrap().is_none());
        let calls = calls.lock().unwrap();
        assert!(calls.iter().any(|c| c.starts_with("encrypt --name device_password --with-key auto - ")), "{calls:?}");
        assert!(calls.iter().any(|c| c.starts_with("decrypt --name device_password ")), "{calls:?}");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Creates a throwaway keychain file (not the System Keychain). Ignored by default: creating a keychain adds it
    /// to the user's keychain search list until it is deleted again. Run with `--ignored`.
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore]
    fn keychain_store_round_trips_in_a_throwaway_keychain() {
        let path = std::env::temp_dir().join(format!("rcli-test-{}.keychain-db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        security_framework::os::macos::keychain::CreateOptions::new().password("test").create(&path).unwrap();
        let store = KeychainSecretStore::new(&path);
        let result = std::panic::catch_unwind(|| {
            assert!(store.get("device_password").unwrap().is_none());
            store.set("device_password", "hunter2xyz").unwrap();
            store.set("device_password", "s3cret pässword").unwrap();
            assert_eq!(store.get("device_password").unwrap().as_deref(), Some("s3cret pässword"));
            store.remove("device_password").unwrap();
            store.remove("device_password").unwrap();
            assert!(store.get("device_password").unwrap().is_none());
        });
        let _ = std::process::Command::new("security").arg("delete-keychain").arg(&path).status();
        result.unwrap();
    }
}
