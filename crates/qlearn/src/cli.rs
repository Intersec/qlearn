//! The command line.

use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "qlearn", version = qlearn::version::LONG, about, long_about = None)]
// A slug before a subcommand would be dropped without a word.
#[command(args_conflicts_with_subcommands = true)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// The course, `.learn/<slug>/`. It is created when it is new. The only
    /// course of `.learn/` by default.
    pub slug: Option<String>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// List the courses of `.learn/`.
    List,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn the_arguments_are_consistent() {
        Cli::command().debug_assert();
    }

    #[test]
    fn a_slug_and_the_list_command_parse() {
        let cli = Cli::try_parse_from(["qlearn", "raft"]).unwrap();
        assert_eq!(cli.slug.as_deref(), Some("raft"));
        assert!(cli.command.is_none());

        let cli = Cli::try_parse_from(["qlearn", "list"]).unwrap();
        assert!(matches!(cli.command, Some(Command::List)));

        assert!(Cli::try_parse_from(["qlearn", "raft", "list"]).is_err());
    }
}
