// Read Codex (ChatGPT) credentials from the user profile.
//
// The Codex CLI writes `auth.json` to `$CODEX_HOME` or `~/.codex/`. Schema:
// `{ "tokens": { "access_token", "account_id" } }`. There is no expiry
// timestamp in the file; we discover expiry only when the server returns
// 401 or 403 to a poll.

use std::ffi::OsString;
use std::path::PathBuf;
use std::time::UNIX_EPOCH;

use serde::Deserialize;

pub struct LocalCodexCreds {
    path: PathBuf,
    id: String,
}

impl LocalCodexCreds {
    pub fn detect() -> Option<Self> {
        let path = auth_path(std::env::var_os("CODEX_HOME"), dirs::home_dir())?;
        let id = format!("codex:{}", path.display());
        Some(Self { path, id })
    }
}

/// `$CODEX_HOME/auth.json`, else `~/.codex/auth.json`. An empty `CODEX_HOME`
/// counts as unset (as in the Codex CLI), so it never resolves to a path
/// relative to the working directory.
fn auth_path(codex_home: Option<OsString>, home: Option<PathBuf>) -> Option<PathBuf> {
    match codex_home.filter(|h| !h.is_empty()) {
        Some(dir) => Some(PathBuf::from(dir).join("auth.json")),
        None => Some(home?.join(".codex").join("auth.json")),
    }
}

/// Parse the `auth.json` shape written by the Codex CLI.
fn parse_codex_json(content: &str) -> Result<super::Token, super::Error> {
    let parsed: Envelope = serde_json::from_str(content)?;
    let tokens = parsed.tokens.ok_or(super::Error::MissingField("tokens"))?;
    if tokens.access_token.is_empty() {
        return Err(super::Error::MissingField("access_token"));
    }
    Ok(super::Token {
        access_token: tokens.access_token,
        expires_at_unix_ms: None,
        account_id: tokens.account_id,
    })
}

impl super::CredentialSource for LocalCodexCreds {
    fn id(&self) -> &str {
        &self.id
    }

    fn read(&self) -> Result<super::Token, super::Error> {
        let content = std::fs::read_to_string(&self.path)?;
        parse_codex_json(&content)
    }

    fn signature(&self) -> Option<String> {
        let meta = std::fs::metadata(&self.path).ok()?;
        let modified = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        Some(format!("{}|{}|{}", self.id, meta.len(), modified))
    }

    fn refresh_hint(&self) -> super::RefreshHint {
        super::RefreshHint::LocalCodexCli
    }
}

#[derive(Deserialize)]
struct Envelope {
    tokens: Option<Tokens>,
}

#[derive(Deserialize)]
struct Tokens {
    access_token: String,
    account_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creds::Error;

    fn fixture(name: &str) -> String {
        let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"))
    }

    #[test]
    fn full_auth_reads_token_and_account() {
        let token = parse_codex_json(&fixture("codex-auth-full.json")).expect("token");
        assert_eq!(token.access_token, "fixture-access-token");
        assert_eq!(token.account_id.as_deref(), Some("fixture-account"));
        assert_eq!(token.expires_at_unix_ms, None);
    }

    #[test]
    fn null_tokens_is_a_missing_field() {
        let err = parse_codex_json(&fixture("codex-auth-null-tokens.json")).unwrap_err();
        assert!(matches!(err, Error::MissingField("tokens")), "{err}");
        let err = parse_codex_json("{}").unwrap_err();
        assert!(matches!(err, Error::MissingField("tokens")), "{err}");
    }

    #[test]
    fn empty_or_missing_access_token_is_rejected() {
        let err = parse_codex_json(&fixture("codex-auth-empty-access-token.json")).unwrap_err();
        assert!(matches!(err, Error::MissingField("access_token")), "{err}");
        let err = parse_codex_json(&fixture("codex-auth-missing-access-token.json")).unwrap_err();
        assert!(matches!(err, Error::Json(_)), "{err}");
    }

    #[test]
    fn codex_home_wins_unless_empty() {
        let home = Some(PathBuf::from("/home/u"));
        assert_eq!(
            auth_path(Some(OsString::from("/opt/codex")), home.clone()),
            Some(PathBuf::from("/opt/codex/auth.json"))
        );
        assert_eq!(
            auth_path(Some(OsString::new()), home.clone()),
            Some(PathBuf::from("/home/u/.codex/auth.json"))
        );
        assert_eq!(
            auth_path(None, home),
            Some(PathBuf::from("/home/u/.codex/auth.json"))
        );
        assert_eq!(auth_path(Some(OsString::new()), None), None);
    }
}
