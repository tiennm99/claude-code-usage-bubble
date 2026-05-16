// Query the GitHub Releases API and pick the relevant asset.

use serde::Deserialize;

use crate::net::Client;

const ASSET_NAME: &str = "claude-code-usage-bubble.exe";
const REPO_OWNER: &str = "tiennm99";
const REPO_NAME: &str = "claude-code-usage-bubble";

#[derive(Clone, Debug)]
pub struct Release {
    pub version: Version,
    pub asset_url: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl Version {
    pub fn current() -> Self {
        Self::parse(env!("CARGO_PKG_VERSION")).unwrap_or(Version {
            major: 0,
            minor: 0,
            patch: 0,
        })
    }

    pub fn parse(s: &str) -> Option<Self> {
        let core = s.trim().trim_start_matches('v').split('-').next()?;
        let mut parts = core.split('.').map(|p| p.parse::<u32>().ok());
        Some(Version {
            major: parts.next().flatten().unwrap_or(0),
            minor: parts.next().flatten().unwrap_or(0),
            patch: parts.next().flatten().unwrap_or(0),
        })
    }
}

pub fn fetch_latest(http: &Client) -> Result<super::CheckOutcome, super::Error> {
    let url = format!("https://api.github.com/repos/{REPO_OWNER}/{REPO_NAME}/releases/latest");
    let resp = http
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .header("User-Agent", user_agent())
        .send()?;
    if !(200..300).contains(&resp.status()) {
        return Err(super::Error::Network(crate::net::Error::Status(resp.status())));
    }
    let body: GhRelease = resp.json()?;
    let candidate = Version::parse(&body.tag_name)
        .ok_or_else(|| super::Error::BadVersion(body.tag_name.clone()))?;
    if candidate <= Version::current() {
        return Ok(super::CheckOutcome::UpToDate);
    }
    let asset = body
        .assets
        .iter()
        .find(|a| a.name.eq_ignore_ascii_case(ASSET_NAME))
        .or_else(|| {
            body.assets
                .iter()
                .find(|a| a.name.to_ascii_lowercase().ends_with(".exe"))
        })
        .ok_or(super::Error::NoAsset)?;
    Ok(super::CheckOutcome::Available(Release {
        version: candidate,
        asset_url: asset.browser_download_url.clone(),
    }))
}

pub fn user_agent() -> &'static str {
    concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"))
}

#[derive(Deserialize)]
struct GhRelease {
    tag_name: String,
    assets: Vec<GhAsset>,
}

#[derive(Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
}
