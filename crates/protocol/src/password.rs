//! Device Password: 10 characters without look-alikes, hashed with argon2. The password itself lives only on the
//! Device (Secret Store); the Server keeps this hash.

use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use ring::rand::{SecureRandom, SystemRandom};

/// A-Z a-z 0-9 without 0/O/o and 1/l/I/L.
const ALPHABET: &[u8] = b"23456789abcdefghijkmnpqrstuvwxyzABCDEFGHJKMNPQRSTUVWXYZ";
pub const LENGTH: usize = 10;
/// Existing Device Passwords stay valid after a change; new ones must be at least this long.
pub const MIN_LENGTH: usize = 8;

/// A fresh random Device Password.
pub fn generate() -> String {
    let mut bytes = [0u8; LENGTH];
    SystemRandom::new().fill(&mut bytes).expect("system RNG");
    bytes.iter().map(|b| ALPHABET[*b as usize % ALPHABET.len()] as char).collect()
}

/// `argon2id$v=19$...` PHC string; the salt is random per call.
pub fn hash(password: &str) -> String {
    let mut salt = [0u8; 16];
    SystemRandom::new().fill(&mut salt).expect("system RNG");
    let salt = SaltString::encode_b64(&salt).expect("16-byte salt");
    Argon2::default().hash_password(password.as_bytes(), &salt).expect("hash a Device Password").to_string()
}

pub fn verify(password: &str, stored: &str) -> bool {
    PasswordHash::new(stored)
        .ok()
        .is_some_and(|hash| Argon2::default().verify_password(password.as_bytes(), &hash).is_ok())
}

#[cfg(test)]
mod tests {
    /// `0/O/o` and `1/l/I/L`: the pairs a person reads off one screen and types on another.
    const LOOK_ALIKES: &str = "0Oo1lIL";

    #[test]
    fn the_whole_alphabet_avoids_look_alikes() {
        let alphabet = std::str::from_utf8(super::ALPHABET).unwrap();
        assert!(
            !alphabet.chars().any(|c| LOOK_ALIKES.contains(c)),
            "the alphabet contains a look-alike: {alphabet}"
        );
        assert!(alphabet.chars().all(|c| c.is_ascii_alphanumeric()), "{alphabet}");
        let unique: std::collections::HashSet<char> = alphabet.chars().collect();
        assert_eq!(unique.len(), alphabet.chars().count(), "a character is listed twice: {alphabet}");
    }

    #[test]
    fn passwords_avoid_look_alikes_and_verify_against_their_hash() {
        let pw = super::generate();
        assert_eq!(pw.len(), 10);
        assert!(!pw.chars().any(|c| LOOK_ALIKES.contains(c)), "{pw}");
        let stored = super::hash(&pw);
        assert!(stored.starts_with("$argon2"), "{stored}");
        assert!(super::verify(&pw, &stored));
        assert!(!super::verify("wrong", &stored));
    }
}
