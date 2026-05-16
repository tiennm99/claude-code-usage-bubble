#![windows_subsystem = "windows"]
// Several modules (creds, usage, update, os::dpi, …) expose API that the
// app surface doesn't fully call yet — they're scaffolding for in-progress
// port phases. Allow at the crate root rather than scattering attributes.
#![allow(dead_code)]

// Original infrastructure.
mod creds;
mod diag;
mod i18n;
mod net;
mod os;
mod tray;
mod update;
mod usage;

// Application surface.
mod app;
mod bubble;
mod panel;
mod settings;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let diagnose_enabled = args.iter().any(|a| a == "--diagnose");
    if diagnose_enabled {
        if let Ok(Some(path)) = diag::init(true) {
            log::info!("startup args={args:?} log_path={}", path.display());
        }
    }

    if let Some(exit_code) = update::run_cli(&args) {
        if diagnose_enabled {
            log::info!("cli mode exited with code {exit_code}");
        }
        std::process::exit(exit_code);
    }

    if diagnose_enabled {
        log::info!("entering app::run");
    }
    app::run();
}
