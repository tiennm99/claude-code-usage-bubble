//! OS-specific code. Each platform file is compiled only on its own target,
//! and portable callers reach it through `cfg`-gated calls.

#[cfg(windows)]
pub mod windows;
