//! Identifier rules shared by validation and later operation layers.

/// The reserved JavaScript property names which are never valid document ids.
pub const RESERVED_IDS: [&str; 3] = ["__proto__", "constructor", "prototype"];

/// Returns whether `id` is a portable storyboard identifier.
pub fn is_valid_id(id: &str) -> bool {
    (1..=160).contains(&id.len())
        && !RESERVED_IDS.contains(&id)
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
}
