//! Agent build signatures: Ed25519 over the version, the target and the build bytes, so a
//! signed build cannot be relabeled as another version or OS. The private key stays with the
//! admin (`rcli rollout keygen`); Agents are configured with the public key; the Server only stores.

use ring::{
    rand::SystemRandom,
    signature::{Ed25519KeyPair, KeyPair, UnparsedPublicKey, ED25519},
};

/// Largest Agent build the Server accepts.
pub const MAX_BUILD_BYTES: usize = 200 << 20;
/// Frame limit for authenticated Device sessions: a build plus room for the rest of the message.
pub const MAX_SESSION_FRAME: usize = MAX_BUILD_BYTES + (1 << 20);

fn message(version: &str, target: &str, build: &[u8]) -> Vec<u8> {
    let mut m = format!("rcli-agent-build\n{version}\n{target}\n").into_bytes();
    m.extend_from_slice(build);
    m
}

/// A new signing key: `(pkcs8 private key, raw 32-byte public key)`.
pub fn generate_key() -> anyhow::Result<(Vec<u8>, Vec<u8>)> {
    let pkcs8 = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).map_err(|_| anyhow::anyhow!("key generation failed"))?;
    let public = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap().public_key().as_ref().to_vec();
    Ok((pkcs8.as_ref().to_vec(), public))
}

pub fn sign(pkcs8: &[u8], version: &str, target: &str, build: &[u8]) -> anyhow::Result<Vec<u8>> {
    let key = Ed25519KeyPair::from_pkcs8(pkcs8).map_err(|_| anyhow::anyhow!("not an Ed25519 PKCS#8 signing key"))?;
    Ok(key.sign(&message(version, target, build)).as_ref().to_vec())
}

pub fn verify(public_key: &[u8], version: &str, target: &str, build: &[u8], signature: &[u8]) -> bool {
    UnparsedPublicKey::new(&ED25519, public_key).verify(&message(version, target, build), signature).is_ok()
}

/// HMAC-SHA256 of the hex update key under the Server Key's secret, served with `/api/v1/update-key`. A Controller checking it
/// holds the secret already, so an on-path attacker who rewrites the plain-HTTP reply cannot produce it.
pub fn pin_mac(server_key_secret: &str, update_key_hex: &str) -> Vec<u8> {
    let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, server_key_secret.as_bytes());
    ring::hmac::sign(&key, update_key_hex.as_bytes()).as_ref().to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_binds_key_version_target_and_bytes() {
        let (private, public) = generate_key().unwrap();
        let (_, other) = generate_key().unwrap();
        let sig = sign(&private, "1.2.0", "linux-x86_64", b"bin").unwrap();
        assert!(verify(&public, "1.2.0", "linux-x86_64", b"bin", &sig));
        assert!(!verify(&other, "1.2.0", "linux-x86_64", b"bin", &sig));
        assert!(!verify(&public, "1.3.0", "linux-x86_64", b"bin", &sig));
        assert!(!verify(&public, "1.2.0", "windows-x86_64", b"bin", &sig));
        assert!(!verify(&public, "1.2.0", "linux-x86_64", b"bin!", &sig));
        assert!(!verify(&public, "1.2.0", "linux-x86_64", b"bin", b""));
    }
}
