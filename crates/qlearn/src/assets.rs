//! The interface, embedded in the binary.
//!
//! A release build carries `web/dist` inside the executable. A debug build
//! reads the directory from disk, so a changed component needs no rebuild
//! of the binary. The tests embed it as a release does.

use rust_embed::Embed;

// The crate compiles before the interface is built, so clippy and an editor
// work on a fresh clone. A release build without it fails in build.rs, and
// the tests fail with a word to build it.
#[derive(Embed)]
#[folder = "../../web/dist"]
#[allow_missing = true]
pub struct Assets;

/// The asset for a request path, with the type the browser needs.
///
/// None for a path that names no asset. The place of the learner rides in
/// the fragment, which never reaches the server, so the interface needs no
/// fallback, and a missing chunk is a 404 rather than a page.
pub fn get(path: &str) -> Option<(Vec<u8>, String)> {
    let path = path.trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    let file = Assets::get(path)?;
    let mime = mime_guess::from_path(path).first_or_octet_stream();

    Some((file.data.into_owned(), mime.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_is_embedded() {
        let (body, mime) = get("/").expect("the interface must be built before the binary");
        assert!(!body.is_empty());
        assert_eq!(mime, "text/html");
        assert_eq!(get("/index.html").unwrap().0, body);
    }

    #[test]
    fn a_path_that_names_no_asset_is_not_found() {
        for path in [
            "/anything",
            "/assets/index-gone.js",
            "/api/session",
            "/../../Cargo.toml",
        ] {
            assert!(get(path).is_none(), "{path}");
        }
    }
}
