//! Self-update through `tauri-plugin-updater`.
//!
//! The plugin fetches `latest.json`, picks this platform's entry and
//! downloads it, checking the minisign signature against the configured
//! public key before handing the bytes back. Applying depends on the
//! install [`Channel`]: the Windows portable exe swaps itself through
//! `self_replace`, an AppImage goes through the plugin's install, and other
//! channels only report that an update exists. The caller relaunches with
//! `--updated-to` and exits.
//!
//! Every call here blocks; run it on a worker thread.

pub mod channel;
pub mod install;

use std::time::Duration;

use tauri::{AppHandle, Runtime, Url};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::core::version::{self, Version};

pub use channel::{current as current_channel, Channel};

/// Budget for the whole check request.
pub const CHECK_TIMEOUT: Duration = Duration::from_secs(30);

/// Budget for the whole download request. The plugin does not carry the
/// check builder's timeout over to the download (an `Update` starts with
/// none), so apply sets this one explicitly.
pub const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(600);

/// Hosts the update requests go to: the manifest endpoint and the release
/// asset downloads it redirects to. A system proxy bypassing either is not
/// applied, so the system's own routing stays in charge.
const UPDATE_HOSTS: [&str; 3] = [
    "github.com",
    "release-assets.githubusercontent.com",
    "objects.githubusercontent.com",
];

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("network: {0}")]
    Network(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("no matching release asset")]
    NoAsset,
    #[error("install location not writable: {0}")]
    NotWritable(String),
    #[error("malformed version: {0}")]
    BadVersion(String),
    #[error("signature check failed: {0}")]
    Signature(String),
    #[error("path rejected for safety: {0}")]
    UnsafePath(String),
    #[error("file swap failed: {0}")]
    SwapFailed(String),
    #[error("updates cannot be applied in place for this install")]
    Unsupported,
    #[error("relaunch after the update failed: {0}")]
    Relaunch(std::io::Error),
    #[error("updater: {0}")]
    Updater(String),
}

impl From<tauri_plugin_updater::Error> for Error {
    fn from(err: tauri_plugin_updater::Error) -> Self {
        use tauri_plugin_updater::Error as E;
        match err {
            E::Io(e) => Error::Io(e),
            E::Reqwest(_)
            | E::Network(_)
            | E::Http(_)
            | E::ReleaseNotFound
            | E::Serialization(_)
            | E::UrlParse(_)
            | E::EmptyEndpoints
            | E::InsecureTransportProtocol => Error::Network(err.to_string()),
            E::Semver(_) => Error::BadVersion(err.to_string()),
            E::TargetNotFound(_) | E::TargetsNotFound(_) => Error::NoAsset,
            E::Minisign(_)
            | E::Base64(_)
            | E::SignatureUtf8(_)
            | E::SignedVersionMismatch { .. }
            | E::MissingSignedVersion => Error::Signature(err.to_string()),
            E::InvalidUpdaterFormat
            | E::BinaryNotFoundInArchive
            | E::TempDirNotFound
            | E::TempDirNotOnSameMountPoint
            | E::PreviousAppNotRestored(_)
            | E::FailedToDetermineExtractPath => Error::SwapFailed(err.to_string()),
            E::UnsupportedOs | E::UnsupportedArch => Error::Unsupported,
            other => Error::Updater(other.to_string()),
        }
    }
}

/// Result of an update check.
#[derive(Debug)]
pub enum CheckOutcome<T = Update> {
    UpToDate,
    Available(T),
}

/// Ask the update endpoint whether a newer release exists for this
/// platform. `proxy` overrides the plugin's own proxy discovery.
pub fn check<R: Runtime>(app: &AppHandle<R>, proxy: Option<Url>) -> Result<CheckOutcome, Error> {
    let mut builder = app
        .updater_builder()
        .version_comparator(|current, release| {
            version::newer(&release.version.to_string(), &current.to_string())
        })
        .timeout(CHECK_TIMEOUT);
    if let Some(proxy) = proxy {
        builder = builder.proxy(proxy);
    }
    // The file to replace is the AppImage itself, not the executable
    // inside its read-only mount.
    #[cfg(target_os = "linux")]
    if let Some(appimage) = std::env::var_os("APPIMAGE").filter(|v| !v.is_empty()) {
        builder = builder.executable_path(appimage);
    }
    let updater = builder.build()?;
    outcome_of(tauri::async_runtime::block_on(updater.check()))
}

/// Map the plugin's check result. A manifest without an entry for this
/// platform (a deliberately Windows-only release, say) is not a failure:
/// there is simply nothing to install here.
pub fn outcome_of<T>(
    result: Result<Option<T>, tauri_plugin_updater::Error>,
) -> Result<CheckOutcome<T>, Error> {
    use tauri_plugin_updater::Error as E;
    match result {
        Ok(Some(update)) => Ok(CheckOutcome::Available(update)),
        Ok(None) | Err(E::TargetNotFound(_) | E::TargetsNotFound(_)) => Ok(CheckOutcome::UpToDate),
        Err(err) => Err(err.into()),
    }
}

/// Download `update`, which the plugin checks against the signing key, and
/// put it in place of the running binary. Returns the installed version for
/// `--updated-to`. The caller relaunches; nothing here exits.
pub fn apply(mut update: Update, channel: Channel) -> Result<Version, Error> {
    if !channel.can_apply() {
        return Err(Error::Unsupported);
    }
    update.timeout = Some(DOWNLOAD_TIMEOUT);
    log::info!(
        "downloading update {} from {}",
        update.version,
        update.download_url
    );
    let bytes = tauri::async_runtime::block_on(update.download(|_, _| {}, || {}))?;
    log::info!("update {} downloaded and verified", update.version);
    match channel {
        Channel::Portable => install::replace_portable(&bytes)?,
        // `install` checks the signature again before swapping the file.
        Channel::AppImage => update.install(&bytes)?,
        Channel::PackageManager | Channel::Unsupported => return Err(Error::Unsupported),
    }
    Ok(Version::parse(&update.version))
}

/// The proxy for the update requests: the system proxy, unless an update
/// host bypasses it. Pass `None` when an environment proxy is set, which the
/// plugin reads itself.
pub fn proxy_url(system: Option<&crate::core::proxy::SystemProxy>) -> Option<Url> {
    let system = system?;
    if UPDATE_HOSTS.iter().any(|host| system.bypasses(host)) {
        return None;
    }
    match Url::parse(&system.url) {
        Ok(url) => Some(url),
        Err(err) => {
            // The proxy URL is not logged: it may carry credentials.
            log::warn!("system proxy ignored for updates, not a valid URL: {err}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::proxy::SystemProxy;
    use tauri_plugin_updater::Error as E;

    #[test]
    fn missing_platform_entry_is_up_to_date() {
        assert!(matches!(
            outcome_of::<()>(Err(E::TargetNotFound("windows-x86_64".into()))),
            Ok(CheckOutcome::UpToDate)
        ));
        assert!(matches!(
            outcome_of::<()>(Err(E::TargetsNotFound(vec!["linux-x86_64".into()]))),
            Ok(CheckOutcome::UpToDate)
        ));
        assert!(matches!(
            outcome_of::<()>(Ok(None)),
            Ok(CheckOutcome::UpToDate)
        ));
        assert!(matches!(
            outcome_of(Ok(Some(7))),
            Ok(CheckOutcome::Available(7))
        ));
    }

    #[test]
    fn other_check_errors_fail() {
        assert!(matches!(
            outcome_of::<()>(Err(E::ReleaseNotFound)),
            Err(Error::Network(_))
        ));
        assert!(matches!(
            outcome_of::<()>(Err(E::Network("503".into()))),
            Err(Error::Network(_))
        ));
    }

    #[test]
    fn plugin_errors_map_onto_the_update_errors() {
        let io = std::io::Error::other("disk");
        assert!(matches!(Error::from(E::Io(io)), Error::Io(_)));
        assert!(matches!(
            Error::from(E::SignatureUtf8("x".into())),
            Error::Signature(_)
        ));
        assert!(matches!(
            Error::from(E::SignedVersionMismatch {
                signed: "0.6.0".into(),
                announced: "0.7.0".into()
            }),
            Error::Signature(_)
        ));
        assert!(matches!(
            Error::from(E::InvalidUpdaterFormat),
            Error::SwapFailed(_)
        ));
        assert!(matches!(Error::from(E::UnsupportedOs), Error::Unsupported));
        assert!(matches!(
            Error::from(E::TargetNotFound("t".into())),
            Error::NoAsset
        ));
        assert!(matches!(
            Error::from(E::AuthenticationFailed),
            Error::Updater(_)
        ));
    }

    #[test]
    fn error_text_keeps_the_log_prefixes() {
        assert_eq!(
            Error::from(E::Network("timed out".into())).to_string(),
            "network: `timed out`"
        );
        assert_eq!(
            Error::NotWritable("denied".into()).to_string(),
            "install location not writable: denied"
        );
    }

    fn proxy(bypass: &[&str]) -> SystemProxy {
        SystemProxy {
            url: "http://proxy.corp:8080".into(),
            bypass: bypass.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn update_proxy_honours_the_bypass_list() {
        assert_eq!(proxy_url(None), None);
        assert_eq!(
            proxy_url(Some(&proxy(&[]))).map(|u| u.to_string()),
            Some("http://proxy.corp:8080/".into())
        );
        assert!(proxy_url(Some(&proxy(&["*.example.com"]))).is_some());
        assert_eq!(proxy_url(Some(&proxy(&["github.com"]))), None);
        assert_eq!(proxy_url(Some(&proxy(&["*.githubusercontent.com"]))), None);
    }

    #[test]
    fn config_points_the_plugin_at_the_release_manifest() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../../tauri.conf.json")).expect("tauri.conf.json");
        assert_eq!(
            config.pointer("/plugins/updater/endpoints"),
            Some(&serde_json::json!([
                "https://github.com/tiennm99/claude-code-usage-bubble/releases/latest/download/latest.json"
            ]))
        );
        // The plugin parses this when it is registered; a parse error
        // would stop the app from starting.
        let plugin: tauri_plugin_updater::Config =
            serde_json::from_value(config["plugins"]["updater"].clone())
                .expect("updater plugin config");
        assert_eq!(plugin.endpoints.len(), 1);
        // Only the release workflow turns updater artifacts on, so local
        // bundle builds need no signing key.
        assert_eq!(
            config.pointer("/bundle/createUpdaterArtifacts"),
            Some(&serde_json::Value::Bool(false))
        );
    }
}
