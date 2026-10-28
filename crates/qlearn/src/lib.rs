//! qlearn — a course on a code base, in the browser, taught by an agent.
//!
//! The binary is a thin shell over this library: it reads the arguments,
//! opens the course, and serves the interface. Everything the course needs
//! lives here, so it can be tested without a process.

pub mod agent;
pub mod api;
pub mod assets;
pub mod git;
pub mod model;
pub mod report;
pub mod store;
pub mod trace;
pub mod version;

#[cfg(test)]
mod testutil;
