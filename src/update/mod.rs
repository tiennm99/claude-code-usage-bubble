// Self-update subsystem.
//
// Two stages: `release::fetch_latest` checks GitHub releases for a newer
// build; `install::begin` downloads the .exe and hands off to a detached
// `cmd /c` script that swaps the binary and restarts.

pub mod channel;
pub mod install;
pub mod release;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("network: {0}")]
    Network(#[from] crate::net::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("no matching release asset")]
    NoAsset,
    #[error("install location not writable: {0}")]
    NotWritable(String),
    #[error("malformed version: {0}")]
    BadVersion(String),
    #[error("asset checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },
    #[error("path rejected for safety: {0}")]
    UnsafePath(String),
}

pub use channel::{current as current_channel, Channel};
pub use install::run_cli;
pub use release::Release;

/// Result of a release-check call.
#[derive(Debug)]
pub enum CheckOutcome {
    UpToDate,
    Available(Release),
}
