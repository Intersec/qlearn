//! What this build is.
//!
//! Cargo knows the release. It does not know which of the hundred commits
//! between two releases this binary came from, so `build.rs` bakes that in.

/// The release, as `Cargo.toml` spells it.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The release and the commit under it, `0.5.4 (1a2b3c4de)`.
///
/// A build outside a checkout has no commit, and this is the release alone.
pub const LONG: &str = env!("QLEARN_VERSION_LONG");
