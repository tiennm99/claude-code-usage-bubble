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

/// The proxy for the update requests, passed to the plugin explicitly so it
/// matches the one the usage requests use.
///
/// An environment proxy (`net::env_proxy`, SOCKS included) wins over the
/// system proxy. `None`, when an update host is on the `NO_PROXY` or system
/// bypass list or no proxy is set, leaves the plugin to its own discovery,
/// which honours those lists per host.
pub fn proxy_url(
    env: Option<&ureq::Proxy>,
    system: Option<&crate::core::proxy::SystemProxy>,
) -> Option<Url> {
    match env {
        Some(env) => env_proxy_url(env),
        None => system_proxy_url(system?),
    }
}

fn env_proxy_url(proxy: &ureq::Proxy) -> Option<Url> {
    use ureq::ProxyProtocol as P;
    let bypassed = UPDATE_HOSTS.iter().any(|host| {
        format!("https://{host}/")
            .parse::<ureq::http::Uri>()
            .is_ok_and(|uri| proxy.is_no_proxy(&uri))
    });
    if bypassed {
        return None;
    }
    let scheme = match proxy.protocol() {
        P::Http => "http",
        P::Https => "https",
        P::Socks4 => "socks4",
        P::Socks4A => "socks4a",
        P::Socks5 => "socks5",
        P::Socks5h => "socks5h",
        _ => {
            log::warn!("environment proxy ignored for updates, unsupported protocol");
            return None;
        }
    };
    // The credentials stay percent-encoded, as they were in the variable.
    let userinfo = match (proxy.username(), proxy.password()) {
        (Some(user), Some(password)) => format!("{user}:{password}@"),
        (Some(user), None) => format!("{user}@"),
        _ => String::new(),
    };
    let built = Url::parse(&format!(
        "{scheme}://{userinfo}{}:{}",
        proxy.host(),
        proxy.port()
    ));
    match built {
        Ok(url) => Some(url),
        Err(err) => {
            // The proxy URL is not logged: it may carry credentials.
            log::warn!("environment proxy ignored for updates, not a valid URL: {err}");
            None
        }
    }
}

fn system_proxy_url(system: &crate::core::proxy::SystemProxy) -> Option<Url> {
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
        assert_eq!(proxy_url(None, None), None);
        assert_eq!(
            proxy_url(None, Some(&proxy(&[]))).map(|u| u.to_string()),
            Some("http://proxy.corp:8080/".into())
        );
        assert!(proxy_url(None, Some(&proxy(&["*.example.com"]))).is_some());
        assert_eq!(proxy_url(None, Some(&proxy(&["github.com"]))), None);
        assert_eq!(
            proxy_url(None, Some(&proxy(&["*.githubusercontent.com"]))),
            None
        );
    }

    fn env(uri: &str) -> ureq::Proxy {
        ureq::Proxy::new(uri).expect("valid proxy")
    }

    #[test]
    fn update_proxy_passes_the_env_proxy_whatever_its_scheme() {
        let url = |env_uri: &str| proxy_url(Some(&env(env_uri)), None).map(|u| u.to_string());
        assert_eq!(
            url("http://proxy.corp:3128"),
            Some("http://proxy.corp:3128/".into())
        );
        assert_eq!(
            url("socks5h://127.0.0.1:7890"),
            Some("socks5h://127.0.0.1:7890".into())
        );
        assert_eq!(url("socks5://[::1]"), Some("socks5://[::1]:1080".into()));
        assert_eq!(
            url("socks4a://10.0.0.1:1081"),
            Some("socks4a://10.0.0.1:1081".into())
        );
        assert_eq!(
            url("socks5h://us%40r:p%3Ass@127.0.0.1:7890"),
            Some("socks5h://us%40r:p%3Ass@127.0.0.1:7890".into())
        );
    }

    #[test]
    fn update_proxy_prefers_the_env_proxy_over_the_system_proxy() {
        assert_eq!(
            proxy_url(Some(&env("socks5h://127.0.0.1:7890")), Some(&proxy(&[])))
                .map(|u| u.to_string()),
            Some("socks5h://127.0.0.1:7890".into())
        );
        assert_eq!(
            proxy_url(Some(&env("http://proxy.env:3128")), Some(&proxy(&[])))
                .map(|u| u.to_string()),
            Some("http://proxy.env:3128/".into())
        );
    }

    #[test]
    fn update_proxy_leaves_no_proxy_hosts_to_the_plugin() {
        let with_no_proxy = |expr: &str| {
            ureq::Proxy::builder(ureq::ProxyProtocol::Socks5h)
                .host("127.0.0.1")
                .port(7890)
                .no_proxy(expr)
                .build()
                .expect("valid proxy")
        };
        assert_eq!(proxy_url(Some(&with_no_proxy("github.com")), None), None);
        assert_eq!(
            proxy_url(Some(&with_no_proxy(".githubusercontent.com")), None),
            None
        );
        assert!(proxy_url(Some(&with_no_proxy("example.com")), None).is_some());
    }

    #[test]
    fn updater_client_reaches_the_network_through_a_socks_env_proxy() {
        // Guards the `socks` feature the direct reqwest dependency turns on
        // for the plugin: without it reqwest rejects the SOCKS proxy.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let (socks_port, seen) = crate::net::test_server::serve_socks5_once(b"via socks");
        let url = proxy_url(
            Some(&env(&format!("socks5h://127.0.0.1:{socks_port}"))),
            None,
        )
        .expect("socks proxy url");
        let client = reqwest::Client::builder()
            .proxy(reqwest::Proxy::all(url.as_str()).expect("reqwest proxy"))
            .build()
            .expect("reqwest client");
        let body = tauri::async_runtime::block_on(async {
            client
                .get("http://example.invalid:8080/latest.json")
                .send()
                .await?
                .text()
                .await
        })
        .expect("request through socks");
        assert_eq!(body, "via socks");
        let request = seen
            .recv_timeout(Duration::from_secs(10))
            .expect("socks proxy saw the request");
        assert!(
            request.starts_with("5 example.invalid:8080\nGET /latest.json HTTP/1.1\r\n"),
            "{request}"
        );
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
