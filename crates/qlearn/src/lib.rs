//! qlearn — a course on a code base, in the browser, taught by an agent.
//!
//! The binary is a thin shell over this library: it reads the arguments,
//! opens the course, and serves the interface. Everything the course needs
//! lives here, so it can be tested without a process.

pub mod agent;
pub mod api;
pub mod assets;
pub mod cheatsheet;
pub mod choice;
pub mod events;
pub mod export;
pub mod finding;
pub mod git;
pub mod highlight;
pub mod json;
pub mod lesson;
pub mod model;
pub mod quiz;
pub mod report;
pub mod status;
pub mod store;
pub mod syllabus;
pub mod thread;
pub mod trace;
pub mod version;

#[cfg(test)]
mod testutil;
