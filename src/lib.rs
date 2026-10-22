//! Usage Monitor on Tauri v2.
//!
//! The binary in `src/main.rs` only calls [`run`]. Every module is `pub` so
//! that items wired up incrementally do not trip `dead_code`.

pub mod badge;
pub mod bubble;
pub mod cli;
pub mod commands;
pub mod core;
pub mod creds;
pub mod diag;
pub mod fullscreen;
pub mod handoff;
pub mod menu;
pub mod net;
pub mod panel;
pub mod platform;
pub mod runtime;
pub mod scheduler;
pub mod tray;
pub mod update;
pub mod usage;

/// Starts the application and blocks until it exits.
pub fn run() {
    runtime::run();
}
