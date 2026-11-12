//! Agent Builds are not signed (ADR 0008): a download is checked against the sha256 the Server computed at upload.

/// Largest Agent build the Server accepts.
pub const MAX_BUILD_BYTES: usize = 200 << 20;

/// Lowercase hex sha256 of a build.
pub fn sha256_hex(build: &[u8]) -> String {
    ring::digest::digest(&ring::digest::SHA256, build).as_ref().iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use prost::Message;

    /// An old Server's `AgentUpdate` still carries a signature (field 4): it decodes, the signature skipped.
    #[test]
    fn an_agent_update_with_a_signature_still_decodes() {
        let mut wire = vec![0x0a, 5];
        wire.extend(b"0.2.0");
        wire.extend([0x22, 64]);
        wire.extend([7u8; 64]);
        wire.extend([0x28, 16, 0x32, 3]);
        wire.extend(b"abc");
        let update = crate::agent::AgentUpdate::decode(wire.as_slice()).unwrap();
        assert_eq!((update.version.as_str(), update.size, update.sha256.as_str()), ("0.2.0", 16, "abc"));
    }
}
