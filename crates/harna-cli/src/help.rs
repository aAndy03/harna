//! The top-level help, summary first (0.9.5): `harna --help` lists every
//! command in one short clause; `harna <command> --help` keeps the whole
//! text. Agents read the list to find a command, then ask that command.

use clap::{CommandFactory, FromArgMatches};

use crate::cli::Cli;

/// Longest clause the command list shows.
const MAX_ABOUT: usize = 72;

/// The first clause of an about line: up to its first sentence, colon,
/// dash or version note, never past [`MAX_ABOUT`].
pub fn short_about(about: &str) -> String {
    let mut cut = about.len();
    for stop in [". ", ": ", " — ", " (0.", " ("] {
        if let Some(i) = about.find(stop) {
            cut = cut.min(i);
        }
    }
    let s = about[..cut].trim_end_matches('.').trim();
    if s.chars().count() <= MAX_ABOUT {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(MAX_ABOUT - 1).collect::<String>())
    }
}

/// The parser with each command's about shortened in the list; the full
/// text moves to that command's own `--help`.
pub fn command() -> clap::Command {
    Cli::command().mut_subcommands(|sub| {
        let Some(about) = sub.get_about().map(|a| a.to_string()) else {
            return sub;
        };
        let short = short_about(&about);
        if short == about {
            return sub;
        }
        let long = sub.get_long_about().map(|l| l.to_string()).unwrap_or(about);
        sub.about(short).long_about(long)
    })
}

pub fn parse() -> Cli {
    let mut matches = command().get_matches();
    Cli::from_arg_matches_mut(&mut matches).unwrap_or_else(|e| e.exit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_shows_the_first_clause() {
        assert_eq!(
            short_about("Who answers for paths (0.7.0): owners per path"),
            "Who answers for paths"
        );
        assert_eq!(
            short_about("Initialize .harna/ in the repository"),
            "Initialize .harna/ in the repository"
        );
        assert!(short_about(&"x".repeat(200)).chars().count() <= MAX_ABOUT);
    }

    #[test]
    fn every_command_keeps_its_whole_text() {
        let cmd = command();
        let owners = cmd.find_subcommand("owners").expect("owners");
        assert_eq!(
            owners.get_about().unwrap().to_string(),
            "Who answers for paths"
        );
        assert!(owners
            .get_long_about()
            .unwrap()
            .to_string()
            .contains("--changed routes"));
    }
}
