//! The `state-contract` command line: `status`, `dashboard` and `task`.
//!
//! `cargo run -p state-contract -- task S14` prints everything the plan says about one task; the
//! flags are parsed by hand — the crate stays zero-dependency, so there is no argument-parsing
//! library here either.

#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use state_contract::{DashboardOptions, TaskOptions, dashboard, status_with_detail, task};

/// The command a host points at for the full task; this binary is its own host.
const DETAIL: &str = "cargo run -p state-contract -- task";

fn main() {
    let code = match run(std::env::args().skip(1)) {
        Ok(()) => 0,
        Err(message) => {
            eprintln!("state-contract: {message}");
            2
        }
    };
    std::process::exit(code);
}

fn run(args: impl Iterator<Item = String>) -> Result<(), String> {
    let words: Vec<String> = args.collect();
    match words.first().map(String::as_str) {
        Some("task") => print_task(&words[1..]),
        Some("dashboard") => print_dashboard(&words[1..]),
        Some("status") => print_status(&words[1..]),
        Some("help") | Some("--help") | None => {
            print_usage();
            Ok(())
        }
        Some(other) => Err(format!("unknown command `{other}`")),
    }
}

fn print_usage() {
    println!("usage: cargo run -p state-contract -- <status|dashboard|task [<id>]> [options]");
    println!("  status         print the session-start block for the current task");
    println!("  dashboard      print a styled overview of the plan; each section names its source file");
    println!("  task [<id>]    print one task in full: its fields, files, map rows, notes, debt and questions");
    println!("options:");
    println!("  --root <path>   the project root that holds plan/ (default: .)");
    println!("  --width <n>     the box width in characters (default: 80)");
    println!("  --no-color      never emit ANSI codes (default: colored when stdout is a terminal)");
    println!("  --task <id>     dashboard: expand this task in a TASK block");
    println!("  --all           task: show every debt entry and question, not only the matching ones");
}

/// `task [<id>] [--root <path>] [--width <n>] [--no-color] [--all]`
fn print_task(args: &[String]) -> Result<(), String> {
    let flags = Flags::parse(args)?;
    let id = flags.id.clone().ok_or("task needs a task id, for example `task S14`")?;
    let opts = TaskOptions { width: flags.width, color: flags.color(), all: flags.all };
    match task(&flags.root, &id, &opts) {
        Ok(text) => {
            println!("{text}");
            Ok(())
        }
        Err(violations) => Err(report(&violations)),
    }
}

/// `dashboard [--root <path>] [--width <n>] [--no-color] [--task <id>]`
fn print_dashboard(args: &[String]) -> Result<(), String> {
    let flags = Flags::parse(args)?;
    let opts = DashboardOptions { width: flags.width, color: flags.color(), task: flags.dashboard_task.clone() };
    match dashboard(&flags.root, &opts) {
        Ok(text) => {
            println!("{text}");
            Ok(())
        }
        Err(violations) => Err(report(&violations)),
    }
}

/// `status [--root <path>]`
fn print_status(args: &[String]) -> Result<(), String> {
    let flags = Flags::parse(args)?;
    match status_with_detail(&flags.root, &state_contract::ProcessGit::new(&flags.root), Some(DETAIL)) {
        Ok(text) => {
            print!("{text}");
            Ok(())
        }
        Err(violations) => Err(report(&violations)),
    }
}

/// Prints every violation on its own line and returns the one-line summary as the error.
fn report(violations: &[String]) -> String {
    for violation in violations {
        eprintln!("error: {violation}");
    }
    format!("the plan has {} violation(s)", violations.len())
}

/// The parsed flags: everything the three commands accept.
struct Flags {
    root: PathBuf,
    width: usize,
    no_color: bool,
    all: bool,
    /// The positional task id of `task`.
    id: Option<String>,
    /// `--task`, the id the dashboard expands.
    dashboard_task: Option<String>,
}

impl Flags {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut flags = Self {
            root: Path::new(".").to_path_buf(),
            width: 80,
            no_color: false,
            all: false,
            id: None,
            dashboard_task: None,
        };
        let mut index = 0;
        while index < args.len() {
            let arg = args[index].as_str();
            index += 1;
            match arg {
                "--root" => {
                    let value = Self::value(args, &mut index, arg)?;
                    flags.root = PathBuf::from(value);
                }
                "--width" => {
                    let value = Self::value(args, &mut index, arg)?;
                    flags.width =
                        value.parse::<usize>().map_err(|_| format!("--width needs a number, got `{value}`"))?;
                }
                "--task" => {
                    let value = Self::value(args, &mut index, arg)?;
                    flags.dashboard_task = Some(value.to_owned());
                }
                "--no-color" => flags.no_color = true,
                "--all" => flags.all = true,
                other if other.starts_with("--") => return Err(format!("unknown option `{other}`")),
                other => {
                    if flags.id.is_some() {
                        return Err(format!("`task` takes one task id, got `{other}` too"));
                    }
                    flags.id = Some(other.to_owned());
                }
            }
        }
        if flags.width < 24 {
            return Err(format!("--width must be at least 24, got {}", flags.width));
        }
        Ok(flags)
    }

    /// Whether ANSI codes go to stdout: a pipe or a log file gets plain text unless `--no-color`.
    fn color(&self) -> bool {
        !self.no_color && std::io::stdout().is_terminal()
    }

    /// Reads the value that follows `flag`, or fails with a message naming the flag.
    fn value<'a>(args: &'a [String], index: &mut usize, flag: &str) -> Result<&'a str, String> {
        if *index >= args.len() {
            return Err(format!("{flag} needs a value"));
        }
        let value = args[*index].as_str();
        *index += 1;
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn flags_default_to_the_current_directory_and_no_width_or_color_override() {
        let flags = Flags::parse(&[]).expect("no arguments are legal");
        assert_eq!(flags.root, PathBuf::from("."));
        assert_eq!(flags.width, 80);
        assert!(!flags.no_color);
        assert!(!flags.all);
        assert_eq!(flags.id, None);
    }

    #[test]
    fn flags_read_root_width_and_no_color() {
        let flags = Flags::parse(&[
            "--root".to_owned(),
            "/tmp/project".to_owned(),
            "--width".to_owned(),
            "60".to_owned(),
            "--no-color".to_owned(),
        ])
        .expect("the full flag set parses");
        assert_eq!(flags.root, PathBuf::from("/tmp/project"));
        assert_eq!(flags.width, 60);
        assert!(flags.no_color);
    }

    #[test]
    fn a_bare_word_is_the_task_id_and_task_is_the_dashboard_target() {
        let flags = Flags::parse(&["S14".to_owned(), "--task".to_owned(), "P4".to_owned()])
            .expect("an id and a dashboard target parse");
        assert_eq!(flags.id.as_deref(), Some("S14"));
        assert_eq!(flags.dashboard_task.as_deref(), Some("P4"));
        assert!(Flags::parse(&["S14".to_owned(), "P4".to_owned()]).is_err(), "one id is enough");
    }

    #[test]
    fn flags_reject_a_missing_value_a_bad_width_and_an_unknown_option() {
        assert!(Flags::parse(&["--root".to_owned()]).is_err(), "--root without a value fails");
        assert!(Flags::parse(&["--width".to_owned(), "wide".to_owned()]).is_err(), "a non-number fails");
        assert!(Flags::parse(&["--width".to_owned(), "10".to_owned()]).is_err(), "a too-small width fails");
        assert!(Flags::parse(&["--colors".to_owned()]).is_err(), "a misspelled option fails");
    }

    #[test]
    fn task_without_an_id_says_what_it_needs() {
        let error = print_task(&[]).expect_err("an id is required");
        assert!(error.contains("task needs a task id"), "{error}");
    }
}
