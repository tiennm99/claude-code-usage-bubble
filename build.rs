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
// A build that has the updater signing key in its environment is a release
// build, so it must also carry the matching public key: without one, the
// installed app could never verify, and so never install, a later update.
//
// The manifest keeps tauri's default Common-Controls v6 dependency and adds
// PerMonitorV2 DPI awareness, so the process is DPI aware before any window
// exists instead of relying on a runtime call.

fn main() {
    inject_cargo_version();
    require_updater_pubkey_when_signing();

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

fn require_updater_pubkey_when_signing() {
    println!("cargo:rerun-if-env-changed=TAURI_SIGNING_PRIVATE_KEY");
    let signing = std::env::var("TAURI_SIGNING_PRIVATE_KEY").is_ok_and(|v| !v.trim().is_empty());
    if !signing {
        return;
    }
    let raw = std::fs::read_to_string("tauri.conf.json").expect("tauri.conf.json must be readable");
    let file: serde_json::Value =
        serde_json::from_str(&raw).expect("tauri.conf.json must be valid JSON");
    // `--config` overrides arrive through TAURI_CONFIG and win over the file.
    let overrides: serde_json::Value = std::env::var("TAURI_CONFIG")
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default();
    const PUBKEY: &str = "/plugins/updater/pubkey";
    let pubkey = overrides
        .pointer(PUBKEY)
        .or_else(|| file.pointer(PUBKEY))
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    assert!(
        !pubkey.trim().is_empty(),
        "plugins.updater.pubkey in tauri.conf.json is empty, but TAURI_SIGNING_PRIVATE_KEY is set: \
         put the public key of that signing key there before building a release"
    );
}
