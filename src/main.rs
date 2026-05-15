#![windows_subsystem = "windows"]

mod app;
mod bubble;
mod diagnose;
mod localization;
mod models;
mod native_interop;
mod panel;
mod poller;
mod settings;
mod theme;
mod tray_icon;
mod updater;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let diagnose_enabled = args.iter().any(|a| a == "--diagnose");
    if diagnose_enabled {
        if let Ok(path) = diagnose::init() {
            diagnose::log(format!(
                "startup args={args:?} log_path={}",
                path.display()
            ));
        }
    }

    if let Some(exit_code) = updater::handle_cli_mode(&args) {
        if diagnose_enabled {
            diagnose::log(format!("cli mode exited with code {exit_code}"));
        }
        std::process::exit(exit_code);
    }

    if diagnose_enabled {
        diagnose::log("entering app::run");
    }
    app::run();
}
