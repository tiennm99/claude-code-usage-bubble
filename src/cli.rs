//! Command-line flags. Parsing is pure so every accepted form is unit-tested.
//!
//! Accepted flags:
//! - `--diagnose` turns on the diagnostic log.
//! - `--wait-pid <pid>` waits for an exiting instance before starting.
//! - `--updated-to <version>` announces a finished update.
//!
//! Anything else, including a flag whose value is missing or malformed, is
//! ignored so an old or newer caller can never stop the app from starting.

use std::ffi::OsString;

/// Longest version string accepted for `--updated-to`.
const MAX_VERSION_LEN: usize = 64;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CliArgs {
    pub diagnose: bool,
    pub wait_pid: Option<u32>,
    pub updated_to: Option<String>,
}

/// The process arguments, parsed. Non-UTF-8 arguments are read lossily
/// instead of panicking, as `std::env::args` would.
pub fn from_env() -> CliArgs {
    parse(std::env::args_os().skip(1))
}

/// Parse the arguments after the program name.
pub fn parse<I, S>(args: I) -> CliArgs
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    let args: Vec<String> = args
        .into_iter()
        .map(|a| a.into().to_string_lossy().into_owned())
        .collect();
    let mut out = CliArgs::default();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--diagnose" => out.diagnose = true,
            "--wait-pid" => {
                if let Some(pid) = args.get(i + 1).and_then(|v| parse_pid(v)) {
                    out.wait_pid = Some(pid);
                    i += 1;
                }
            }
            "--updated-to" => {
                if let Some(version) = args.get(i + 1).filter(|v| is_version_like(v)) {
                    out.updated_to = Some(version.clone());
                    i += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    out
}

/// A positive process id. Zero is rejected: on Unix it names the whole
/// process group, never a single parent.
fn parse_pid(value: &str) -> Option<u32> {
    value.parse::<u32>().ok().filter(|pid| *pid != 0)
}

/// A version as the updater passes it (`0.6.0`, `1.2.3-rc.1`). The text ends
/// up in a notification, so anything else is dropped.
fn is_version_like(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_VERSION_LEN
        && value.starts_with(|c: char| c.is_ascii_digit())
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(args: &[&str]) -> CliArgs {
        parse(args.iter().copied())
    }

    #[test]
    fn no_flags_is_the_default() {
        assert_eq!(p(&[]), CliArgs::default());
    }

    #[test]
    fn diagnose_flag() {
        assert!(p(&["--diagnose"]).diagnose);
    }

    #[test]
    fn wait_pid_takes_the_next_value() {
        assert_eq!(p(&["--wait-pid", "4242"]).wait_pid, Some(4242));
    }

    #[test]
    fn wait_pid_rejects_zero_garbage_and_a_missing_value() {
        assert_eq!(p(&["--wait-pid", "0"]).wait_pid, None);
        assert_eq!(p(&["--wait-pid", "-5"]).wait_pid, None);
        assert_eq!(p(&["--wait-pid"]).wait_pid, None);
        // A following flag is not swallowed as the value.
        let args = p(&["--wait-pid", "--diagnose"]);
        assert_eq!(args.wait_pid, None);
        assert!(args.diagnose);
    }

    #[test]
    fn updated_to_alone() {
        // v0.3.13 to v0.4.0 relaunch with only this flag.
        assert_eq!(
            p(&["--updated-to", "0.6.0"]),
            CliArgs {
                updated_to: Some("0.6.0".into()),
                ..CliArgs::default()
            }
        );
    }

    #[test]
    fn updated_to_combined_with_wait_pid() {
        assert_eq!(
            p(&[
                "--wait-pid",
                "17",
                "--updated-to",
                "1.2.3-rc.1",
                "--diagnose"
            ]),
            CliArgs {
                diagnose: true,
                wait_pid: Some(17),
                updated_to: Some("1.2.3-rc.1".into()),
            }
        );
    }

    #[test]
    fn updated_to_rejects_text_that_is_not_a_version() {
        assert_eq!(p(&["--updated-to", "--diagnose"]).updated_to, None);
        assert_eq!(p(&["--updated-to", "v0.6.0"]).updated_to, None);
        assert_eq!(p(&["--updated-to", "0.6.0 <b>"]).updated_to, None);
        let long = "1".repeat(MAX_VERSION_LEN + 1);
        assert_eq!(p(&["--updated-to", &long]).updated_to, None);
        assert_eq!(p(&["--updated-to"]).updated_to, None);
    }

    #[test]
    fn unknown_flags_are_ignored() {
        assert_eq!(
            p(&["--apply-update", "x", "--wait-pid", "9", "--frobnicate"]),
            CliArgs {
                wait_pid: Some(9),
                ..CliArgs::default()
            }
        );
    }
}
