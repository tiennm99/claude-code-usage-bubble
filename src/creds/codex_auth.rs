// Read Codex (ChatGPT) credentials from the user profile.
//
// The Codex CLI writes `auth.json` to `$CODEX_HOME` or `~/.codex/`. Schema:
// `{ "tokens": { "access_token", "account_id" } }`. There is no expiry
// timestamp in the file; we discover expiry only when the server returns
// 401 or 403 to a poll.

use std::path::PathBuf;
use std::time::UNIX_EPOCH;

use serde::Deserialize;

pub struct LocalCodexCreds {
    path: PathBuf,
    id: String,
}

impl LocalCodexCreds {
    pub fn detect() -> Option<Self> {
        let path = if let Some(home) = std::env::var_os("CODEX_HOME") {
            PathBuf::from(home).join("auth.json")
        } else {
            dirs::home_dir()?.join(".codex").join("auth.json")
        };
        let id = format!("codex:{}", path.display());
        Some(Self { path, id })
    }
}

impl super::CredentialSource for LocalCodexCreds {
    fn id(&self) -> &str {
        &self.id
    }

    fn read(&self) -> Result<super::Token, super::Error> {
        let content = std::fs::read_to_string(&self.path)?;
        let parsed: Envelope = serde_json::from_str(&content)?;
        let tokens = parsed
            .tokens
            .ok_or(super::Error::MissingField("tokens"))?;
        if tokens.access_token.is_empty() {
            return Err(super::Error::MissingField("access_token"));
        }
        Ok(super::Token {
            access_token: tokens.access_token,
            expires_at_unix_ms: None,
            account_id: tokens.account_id,
        })
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
