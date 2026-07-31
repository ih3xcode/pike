/// Приймає як голий hex, так і формат `sha256sum`: "<hex>  <filename>".
pub fn verify_asset(data: &[u8], expected: &str) -> Result<(), String> {
    let expected_hex = expected
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_lowercase();
    if expected_hex.len() != 64 || !expected_hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("malformed checksum: {expected:?}"));
    }

    use sha2::{Digest, Sha256};
    let actual = hex::encode(Sha256::digest(data));
    if actual == expected_hex {
        Ok(())
    } else {
        Err(format!(
            "checksum mismatch: expected {expected_hex}, got {actual}"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sha256_of(data: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(data);
        hex::encode(h.finalize())
    }

    #[test]
    fn verify_accepts_matching_checksum() {
        let data = b"binary contents";
        assert!(verify_asset(data, &sha256_of(data)).is_ok());
    }

    #[test]
    fn verify_accepts_sha256sum_file_format() {
        // GNU sha256sum пише "<hex>  <filename>"
        let data = b"binary contents";
        let line = format!("{}  pike-linux-amd64\n", sha256_of(data));
        assert!(verify_asset(data, &line).is_ok());
    }

    #[test]
    fn verify_rejects_mismatch() {
        assert!(verify_asset(b"actual", &"0".repeat(64)).is_err());
    }

    #[test]
    fn verify_rejects_garbage_checksum() {
        assert!(verify_asset(b"actual", "not a checksum").is_err());
    }
}
