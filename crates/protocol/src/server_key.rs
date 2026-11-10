//! Server Key: `rk_<CA fingerprint>_<secret>`. The fingerprint (hex SHA-256 of the CA certificate) pins the Server's
//! CA; the secret lets a Device register and a password connection reach the Controller API.

use std::fmt;

#[derive(Clone, Debug, PartialEq)]
pub struct ServerKey {
    pub fingerprint: String,
    pub secret: String,
}

impl ServerKey {
    /// Parses `rk_<64 hex characters>_<secret>`; None for anything else.
    pub fn parse(key: &str) -> Option<Self> {
        let (fingerprint, secret) = key.trim().strip_prefix("rk_")?.split_once('_')?;
        let well_formed = fingerprint.len() == 64 && fingerprint.chars().all(|c| c.is_ascii_hexdigit()) && !secret.is_empty();
        well_formed.then(|| Self { fingerprint: fingerprint.to_lowercase(), secret: secret.to_owned() })
    }

    /// Whether the key `presented` carries this key's secret (compared in constant time).
    pub fn admits(&self, presented: &str) -> bool {
        let presented = Self::parse(presented).map(|k| k.secret).unwrap_or_default();
        let (a, b) = (self.secret.as_bytes(), presented.as_bytes());
        a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
    }
}

impl fmt::Display for ServerKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "rk_{}_{}", self.fingerprint, self.secret)
    }
}

#[cfg(test)]
mod tests {
    use super::ServerKey;

    #[test]
    fn parses_round_trips_and_admits_only_its_secret() {
        let fingerprint = "AB".repeat(32);
        let key = ServerKey::parse(&format!(" rk_{fingerprint}_s3cret ")).unwrap();
        assert_eq!(key.fingerprint, "ab".repeat(32));
        assert_eq!(ServerKey::parse(&key.to_string()), Some(key.clone()));
        assert!(key.admits(&format!("rk_{}_s3cret", "00".repeat(32))), "the secret, not the fingerprint, admits");
        assert!(!key.admits(&format!("rk_{fingerprint}_other")));
        assert!(!key.admits("garbage"));
        for bad in ["", "rk_", "rk_abc_s3cret", &format!("rk_{}_s", "zz".repeat(32)), &format!("rk_{fingerprint}_"), &format!("xx_{fingerprint}_s")] {
            assert_eq!(ServerKey::parse(bad), None, "{bad}");
        }
    }
}
