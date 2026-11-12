//! Agent build signatures: Ed25519 over `rcli-agent-build\n{version}\n{target}\n{sha256 lowercase hex}`, so a
//! signed build cannot be relabeled as another version or OS, and is checked without the build in memory. The private key stays with the
//! admin (`rcli rollout keygen`); Agents and the Server are configured with the public key.

use ring::{
    rand::SystemRandom,
    signature::{Ed25519KeyPair, KeyPair, UnparsedPublicKey, ED25519},
};

/// Largest Agent build the Server accepts.
pub const MAX_BUILD_BYTES: usize = 200 << 20;
/// Frame limit for authenticated Device sessions: a build plus room for the rest of the message.
pub const MAX_SESSION_FRAME: usize = MAX_BUILD_BYTES + (1 << 20);

fn message(version: &str, target: &str, sha256: &str) -> Vec<u8> {
    format!("rcli-agent-build\n{version}\n{target}\n{sha256}").into_bytes()
}

/// Lowercase hex sha256 of a build, the digest the signature covers.
pub fn sha256_hex(build: &[u8]) -> String {
    ring::digest::digest(&ring::digest::SHA256, build).as_ref().iter().map(|b| format!("{b:02x}")).collect()
}

/// A new signing key: `(pkcs8 private key, raw 32-byte public key)`.
pub fn generate_key() -> anyhow::Result<(Vec<u8>, Vec<u8>)> {
    let pkcs8 = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).map_err(|_| anyhow::anyhow!("key generation failed"))?;
    let public = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap().public_key().as_ref().to_vec();
    Ok((pkcs8.as_ref().to_vec(), public))
}

pub fn sign(pkcs8: &[u8], version: &str, target: &str, sha256: &str) -> anyhow::Result<Vec<u8>> {
    let key = Ed25519KeyPair::from_pkcs8(pkcs8).map_err(|_| anyhow::anyhow!("not an Ed25519 PKCS#8 signing key"))?;
    Ok(key.sign(&message(version, target, sha256)).as_ref().to_vec())
}

pub fn verify(public_key: &[u8], version: &str, target: &str, sha256: &str, signature: &[u8]) -> bool {
    UnparsedPublicKey::new(&ED25519, public_key).verify(&message(version, target, sha256), signature).is_ok()
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
    fn signature_binds_key_version_target_and_digest() {
        let (private, public) = generate_key().unwrap();
        let (_, other) = generate_key().unwrap();
        let digest = sha256_hex(b"bin");
        // Worked value from `printf bin | sha256sum`.
        assert_eq!(digest, "51a1f05af85e342e3c849b47d387086476282d5f50dc240c19216d6edfb1eb5a");
        let sig = sign(&private, "1.2.0", "linux-x86_64", &digest).unwrap();
        assert!(verify(&public, "1.2.0", "linux-x86_64", &digest, &sig));
        assert!(!verify(&other, "1.2.0", "linux-x86_64", &digest, &sig));
        assert!(!verify(&public, "1.3.0", "linux-x86_64", &digest, &sig));
        assert!(!verify(&public, "1.2.0", "windows-x86_64", &digest, &sig));
        assert!(!verify(&public, "1.2.0", "linux-x86_64", &sha256_hex(b"bin!"), &sig));
        assert!(!verify(&public, "1.2.0", "linux-x86_64", &digest, b""));
    }

    #[test]
    fn signed_message_is_the_documented_format() {
        let (private, public) = generate_key().unwrap();
        let key = Ed25519KeyPair::from_pkcs8(&private).unwrap();
        let sig = key.sign(b"rcli-agent-build\n1.2.0\nlinux-x86_64\nabc123");
        assert!(verify(&public, "1.2.0", "linux-x86_64", "abc123", sig.as_ref()));
    }
}
