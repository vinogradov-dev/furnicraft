//! The `state-contract` command line: `check`, `status` and `dashboard`.
//!
//! `cargo run -p state-contract -- dashboard` prints the styled overview of the plan files; every
//! section header names the file it was read from. The flags are parsed by hand — the crate stays
//! zero-dependency, so there is no argument-parsing library here either.

use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use state_contract::{DashboardOptions, dashboard, status};

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
    println!("usage: cargo run -p state-contract -- <dashboard|status> [options]");
    println!("  dashboard   print a styled overview of the plan; each section names its source file");
    println!("  status      print the session-start block for the current task");
    println!("options:");
    println!("  --root <path>   the project root that holds plan/ (default: .)");
    println!("  --width <n>     the dashboard box width in characters (default: 80)");
    println!("  --no-color      never emit ANSI codes (default: colored when stdout is a terminal)");
}

/// `dashboard [--root <path>] [--width <n>] [--no-color]`
fn print_dashboard(args: &[String]) -> Result<(), String> {
    let flags = Flags::parse(args)?;
    let opts = DashboardOptions {
        width: flags.width,
        // A pipe or a log file gets plain text unless the caller insists; `--no-color` always wins.
        color: !flags.no_color && std::io::stdout().is_terminal(),
    };
    match dashboard(&flags.root, &opts) {
        Ok(text) => {
            println!("{text}");
            Ok(())
        }
        Err(violations) => {
            for violation in &violations {
                eprintln!("error: {violation}");
            }
            Err(format!("the plan has {} violation(s)", violations.len()))
        }
    }
}

/// `status [--root <path>]`
fn print_status(args: &[String]) -> Result<(), String> {
    let flags = Flags::parse(args)?;
    match status(&flags.root) {
        Ok(text) => {
            print!("{text}");
            Ok(())
        }
        Err(violations) => {
            for violation in &violations {
                eprintln!("error: {violation}");
            }
            Err(format!("the plan has {} violation(s)", violations.len()))
        }
    }
}

/// The parsed flags: everything `dashboard` and `status` accept.
struct Flags {
    root: PathBuf,
    width: usize,
    no_color: bool,
}

impl Flags {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut root = Path::new(".").to_path_buf();
        let mut width = 80;
        let mut no_color = false;
        let mut index = 0;
        while index < args.len() {
            let arg = args[index].as_str();
            index += 1;
            match arg {
                "--root" => {
                    let value = Self::value(args, &mut index, arg)?;
                    root = PathBuf::from(value);
                }
                "--width" => {
                    let value = Self::value(args, &mut index, arg)?;
                    width = value.parse::<usize>().map_err(|_| format!("--width needs a number, got `{value}`"))?;
                }
                "--no-color" => no_color = true,
                other => return Err(format!("unknown option `{other}`")),
            }
        }
        if width < 24 {
            return Err(format!("--width must be at least 24, got {width}"));
        }
        Ok(Self { root, width, no_color })
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
    use super::*;

    #[test]
    fn flags_default_to_the_current_directory_and_no_width_or_color_override() {
        let flags = Flags::parse(&[]).expect("no arguments are legal");
        assert_eq!(flags.root, PathBuf::from("."));
        assert_eq!(flags.width, 80);
        assert!(!flags.no_color);
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
    fn flags_reject_a_missing_value_a_bad_width_and_an_unknown_option() {
        assert!(Flags::parse(&["--root".to_owned()]).is_err(), "--root without a value fails");
        assert!(Flags::parse(&["--width".to_owned(), "wide".to_owned()]).is_err(), "a non-number fails");
        assert!(Flags::parse(&["--width".to_owned(), "10".to_owned()]).is_err(), "a too-small width fails");
        assert!(Flags::parse(&["--colors".to_owned()]).is_err(), "a misspelled option fails");
    }
}
