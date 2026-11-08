use sha2::{Digest, Sha256};

/// Returns the lowercase SHA-256 hexadecimal digest of the bytes exactly as stored on disk.
pub fn version_of(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
