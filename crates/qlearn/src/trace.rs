//! Timings, when the run asks for them.
//!
//! The cost of a slow course is never where a reader guesses. `--trace`
//! prints one line per piece of work, so the answer comes from a measure
//! and not from an opinion.
//!
//! The lines go to standard error. Standard output belongs to the URL and
//! to the commands of the agent.
//!
//! A quiet run pays one atomic read per call site. `start` gives `None`, and
//! `since` then builds no string at all: every message is behind a closure.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

static ON: AtomicBool = AtomicBool::new(false);
static ORIGIN: OnceLock<Instant> = OnceLock::new();

/// The name of the variable that switches the timings on.
pub const ENV: &str = "QLEARN_TRACE";

/// Switch the timings on, and fix the origin of the clock.
pub fn enable() {
    ORIGIN.get_or_init(Instant::now);
    ON.store(true, Ordering::Relaxed);
}

/// Switch them on when the environment asks for it.
///
/// Any value but the empty string and `0` counts as yes.
pub fn enable_from_env() {
    match std::env::var(ENV) {
        Ok(value) if !value.is_empty() && value != "0" => enable(),
        _ => {}
    }
}

/// Whether this run prints timings.
pub fn on() -> bool {
    ON.load(Ordering::Relaxed)
}

/// The clock that a block of work starts with. `None` when the run is
/// quiet.
pub fn start() -> Option<Instant> {
    on().then(Instant::now)
}

/// Print what a block of work cost. Does nothing when `start` gave `None`.
pub fn since(started: Option<Instant>, what: impl FnOnce() -> String) {
    let Some(started) = started else {
        return;
    };
    let ms = started.elapsed().as_secs_f64() * 1000.0;
    let at = ORIGIN
        .get()
        .map(|origin| origin.elapsed().as_secs_f64())
        .unwrap_or_default();

    eprintln!("trace {at:8.3}s {ms:8.1}ms  {}", what());
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A quiet run must not pay for the message that it does not print.
    #[test]
    fn a_quiet_run_never_builds_the_message() {
        let mut built = false;

        since(None, || {
            built = true;
            String::new()
        });
        assert!(!built);
    }
}
