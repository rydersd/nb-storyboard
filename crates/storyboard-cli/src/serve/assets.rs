//! The page and its modules, embedded from `web/` by build.rs.

include!(concat!(env!("OUT_DIR"), "/web_assets.rs"));

/// The embedded file for a request path; `/` is `index.html`.
pub(super) fn lookup(path: &str) -> Option<(&'static [u8], &'static str)> {
    let relative = path.trim_start_matches('/');
    let relative = if relative.is_empty() {
        "index.html"
    } else {
        relative
    };
    ASSETS
        .iter()
        .find(|(name, _, _)| *name == relative)
        .map(|(_, bytes, kind)| (*bytes, *kind))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_and_entry_module_are_embedded() {
        assert!(lookup("/").is_some_and(|(_, kind)| kind.starts_with("text/html")));
        assert!(lookup("/../Cargo.toml").is_none());
        assert!(
            ASSETS
                .iter()
                .all(|(name, bytes, _)| !name.contains("..") && !bytes.is_empty())
        );
    }
}
