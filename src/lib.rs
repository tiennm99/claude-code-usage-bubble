//! Claude Code Usage Bubble on Tauri v2.
//!
//! The binary in `src/main.rs` only calls [`run`]. Every module is `pub` so
//! that items wired up incrementally do not trip `dead_code`.

pub mod core;
pub mod diag;
pub mod usage;

use tauri::{RunEvent, WebviewUrl, WebviewWindowBuilder, WindowEvent};

/// Label of the scaffold window that proves the webview pipeline works.
const TEST_WINDOW_LABEL: &str = "bubble-test";

/// Starts the application and blocks until it exits.
pub fn run() {
    let args: Vec<String> = std::env::args().collect();
    let diagnose_enabled = args.iter().any(|a| a == "--diagnose");
    match diag::init(diagnose_enabled) {
        Ok(Some(path)) => log::info!("startup args={args:?} log_path={}", path.display()),
        Ok(None) => {}
        Err(err) => eprintln!("diagnostic log could not be opened: {err}"),
    }

    let app = tauri::Builder::default()
        .setup(|app| {
            let window = WebviewWindowBuilder::new(
                app,
                TEST_WINDOW_LABEL,
                WebviewUrl::App("bubble.html".into()),
            )
            .title("Claude Code Usage Bubble")
            .inner_size(220.0, 66.0)
            .transparent(true)
            .decorations(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .resizable(false)
            .build()?;

            // The process otherwise outlives its windows (see the exit
            // handler below), so closing the only window must exit
            // explicitly.
            let handle = app.handle().clone();
            window.on_window_event(move |event| {
                if let WindowEvent::CloseRequested { .. } = event {
                    handle.exit(0);
                }
            });
            Ok(())
        })
        .build(tauri::generate_context!());

    let app = match app {
        Ok(app) => app,
        Err(err) => {
            log::error!("tauri builder failed: {err}");
            eprintln!("Claude Code Usage Bubble failed to start: {err}");
            std::process::exit(1);
        }
    };

    app.run(|_app, event| {
        // Destroying the last window must not end the process; only an
        // explicit `exit` (which carries an exit code) does.
        if let RunEvent::ExitRequested {
            code: None, api, ..
        } = event
        {
            api.prevent_exit();
        }
    });
}
