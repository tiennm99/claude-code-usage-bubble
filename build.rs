// `tauri-build` validates `tauri.conf.json` against the enabled crate
// features and generates the capability schemas under `gen/`. On Windows it
// also embeds the icon, a VERSIONINFO and the application manifest below.
//
// `tauri.conf.json` has no `version`, so `Cargo.toml` stays the only version
// source. `tauri-build` writes VERSIONINFO only from the config version, so
// the Cargo version is merged in through `TAURI_CONFIG`, which `try_build`
// reads after the config file. Any `TAURI_CONFIG` passed by the CLI
// (`--config`) is kept.
//
// The manifest keeps tauri's default Common-Controls v6 dependency and adds
// PerMonitorV2 DPI awareness, so the process is DPI aware before any window
// exists instead of relying on a runtime call.

fn main() {
    inject_cargo_version();

    let windows = tauri_build::WindowsAttributes::new()
        .app_manifest(include_str!("res/windows-app-manifest.xml"));
    tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows))
        .expect("failed to run the tauri build script");
}

fn inject_cargo_version() {
    let mut config = match std::env::var("TAURI_CONFIG") {
        Ok(raw) => serde_json::from_str::<serde_json::Value>(&raw)
            .expect("TAURI_CONFIG must be a JSON object"),
        Err(_) => serde_json::json!({}),
    };
    let object = config
        .as_object_mut()
        .expect("TAURI_CONFIG must be a JSON object");
    object
        .entry("version")
        .or_insert_with(|| env!("CARGO_PKG_VERSION").into());
    std::env::set_var("TAURI_CONFIG", config.to_string());
}
