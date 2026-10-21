//! Release version parsing and comparison for the update check.
//!
//! Only `major.minor.patch` counts: a leading `v` and anything after the
//! first `-` are ignored, and a missing or non-numeric part reads as 0. So
//! `v1.2.3-rc1` equals `1.2.3`, and a pre-release of the running version is
//! never offered as an update.

use std::fmt;

/// A `major.minor.patch` triple, ordered numerically part by part.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl Version {
    /// The running build's version, from `Cargo.toml`.
    pub fn current() -> Self {
        Self::parse(env!("CARGO_PKG_VERSION"))
    }

    /// Parse a release tag or version string. Never fails: anything that
    /// is not a number becomes 0, so `abc` is `0.0.0`.
    pub fn parse(s: &str) -> Self {
        let core = s
            .trim()
            .trim_start_matches(['v', 'V'])
            .split('-')
            .next()
            .unwrap_or_default();
        let mut parts = core.split('.').map(|p| p.trim().parse::<u32>().ok());
        let mut next = || parts.next().flatten().unwrap_or(0);
        Version {
            major: next(),
            minor: next(),
            patch: next(),
        }
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// Whether `candidate` is strictly newer than `current`. Equal versions and
/// downgrades are not updates.
pub fn newer(candidate: &str, current: &str) -> bool {
    Version::parse(candidate) > Version::parse(current)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(major: u32, minor: u32, patch: u32) -> Version {
        Version {
            major,
            minor,
            patch,
        }
    }

    #[test]
    fn suffixes_and_prefix_are_ignored() {
        assert_eq!(Version::parse("v1.2.3-rc1"), v(1, 2, 3));
        assert_eq!(Version::parse("1.2.3"), v(1, 2, 3));
        assert_eq!(Version::parse(" V0.5.0 "), v(0, 5, 0));
    }

    #[test]
    fn missing_and_malformed_parts_read_as_zero() {
        assert_eq!(Version::parse("v1"), v(1, 0, 0));
        assert_eq!(Version::parse("1.2"), v(1, 2, 0));
        assert_eq!(Version::parse("abc"), v(0, 0, 0));
        assert_eq!(Version::parse(""), v(0, 0, 0));
        assert_eq!(Version::parse("1.x.3"), v(1, 0, 3));
    }

    #[test]
    fn only_a_strictly_newer_version_is_an_update() {
        assert!(newer("0.6.0", "0.5.0"));
        assert!(newer("v0.5.1", "0.5.0"));
        assert!(newer("1.0.0", "0.99.99"));
        assert!(newer("0.10.0", "0.9.0"));
        // Same version, with or without a suffix: no update.
        assert!(!newer("0.5.0", "0.5.0"));
        assert!(!newer("v0.5.0-rc1", "0.5.0"));
        assert!(!newer("0.5.0", "0.5.0-beta"));
        // No downgrade.
        assert!(!newer("0.4.9", "0.5.0"));
        assert!(!newer("abc", "0.5.0"));
    }

    #[test]
    fn displays_the_bare_triple() {
        assert_eq!(Version::parse("v1.2.3-rc1").to_string(), "1.2.3");
    }
}
