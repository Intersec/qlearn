//! The command line.

use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "qlearn", version = qlearn::version::LONG, about, long_about = None)]
pub struct Cli {}
