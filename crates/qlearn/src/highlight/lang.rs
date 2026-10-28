//! Which language an extension names.
//!
//! The map is data, not code. Bundled grammars claim the common extensions
//! on their own, so this only carries the ones that they do not.

/// The extensions that no bundled grammar claims.
const DEFAULTS: &[(&str, &str)] = &[
    ("blk", "c"),
    ("blkk", "c"),
    ("pxc", "c"),
    // No Cython grammar is bundled, measured on the set we ship. Python is
    // the closest thing that exists, and it reads well enough.
    ("pyx", "python"),
    ("pxd", "python"),
    ("pxi", "python"),
    ("tpl", "jinja2"),
];

/// The language of an extension, or `None` when the map does not claim it.
///
/// `None` is not a failure. It asks the highlighter to decide, from the
/// names and the extensions that its grammars know.
pub fn of(extension: &str) -> Option<&'static str> {
    let extension = extension.trim_start_matches('.');

    DEFAULTS
        .iter()
        .find(|(ext, _)| ext.eq_ignore_ascii_case(extension))
        .map(|(_, lang)| *lang)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_built_in_extensions_resolve() {
        assert_eq!(of("blk"), Some("c"));
        assert_eq!(of("pxc"), Some("c"));
        assert_eq!(of("pyx"), Some("python"));
        assert_eq!(of(".tpl"), Some("jinja2"));
    }

    #[test]
    fn an_extension_nothing_claims_is_left_to_the_highlighter() {
        assert_eq!(of("rs"), None);
        assert_eq!(of(""), None);
        // The IOP grammar we ship claims it. A map entry would win over it.
        assert_eq!(of("iop"), None);
    }

    #[test]
    fn the_case_of_the_extension_does_not_matter() {
        assert_eq!(of("BLK"), Some("c"));
    }

    /// Every language the map names must exist in the grammar set we ship.
    /// A map entry that resolves to nothing is code shown as plain text.
    #[test]
    fn every_language_of_the_map_is_bundled() {
        let set = two_face::syntax::extra_newlines();

        for (ext, name) in DEFAULTS {
            assert!(
                set.find_syntax_by_token(name).is_some(),
                "no grammar for {name}, mapped from .{ext}"
            );
        }
    }
}
