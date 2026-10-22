// Read Claude credentials from the user's home directory.
//
// Path: `~/.claude/.credentials.json`, `%USERPROFILE%\.claude\.credentials.json`
// on Windows (matches what the official Claude CLI writes). Schema:
// `{ "claudeAiOauth": { "accessToken", "expiresAt": <ms-since-epoch> } }`.

use std::path::PathBuf;
use std::time::UNIX_EPOCH;

pub struct LocalClaudeCreds {
    path: PathBuf,
    id: String,
}

impl LocalClaudeCreds {
    pub fn detect() -> Option<Self> {
        let home = dirs::home_dir()?;
        let path = home.join(".claude").join(".credentials.json");
        let id = format!("local:{}", path.display());
        Some(Self { path, id })
    }

    pub fn path(&self) -> &std::path::Path {
        &self.path
    }
}

impl super::CredentialSource for LocalClaudeCreds {
    fn id(&self) -> &str {
        &self.id
    }

    fn read(&self) -> Result<super::Token, super::Error> {
        let content = std::fs::read_to_string(&self.path)?;
        parse_claude_json(&content)
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
        super::RefreshHint::LocalClaudeCli
    }
}

/// Parse the Claude credential JSON shape written by the official CLI.
pub(crate) fn parse_claude_json(content: &str) -> Result<super::Token, super::Error> {
    let value: serde_json::Value = serde_json::from_str(content)?;
    let oauth = value
        .get("claudeAiOauth")
        .ok_or(super::Error::MissingField("claudeAiOauth"))?;
    let access_token = oauth
        .get("accessToken")
        .and_then(|v| v.as_str())
        .ok_or(super::Error::MissingField("accessToken"))?
        .to_string();
    // The CLI writes integer milliseconds; a float is truncated rather than
    // dropped, so a client-side expiry check still runs.
    let expires_at_unix_ms = oauth
        .get("expiresAt")
        .and_then(|v| v.as_i64().or_else(|| v.as_f64().map(|f| f as i64)));
    Ok(super::Token {
        access_token,
        expires_at_unix_ms,
        account_id: None,
    })
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
    fn full_credentials_read_token_and_expiry() {
        let token = parse_claude_json(&fixture("claude-credentials-full.json")).expect("token");
        assert_eq!(token.access_token, "sk-ant-oat01-fixture");
        assert_eq!(token.expires_at_unix_ms, Some(1_761_040_800_000));
        assert_eq!(token.account_id, None);
    }

    #[test]
    fn float_expiry_is_truncated_to_milliseconds() {
        let token =
            parse_claude_json(&fixture("claude-credentials-float-expiry.json")).expect("token");
        assert_eq!(token.expires_at_unix_ms, Some(1_761_040_800_000));
    }

    #[test]
    fn missing_expiry_is_none() {
        let token = parse_claude_json(r#"{"claudeAiOauth":{"accessToken":"t","expiresAt":null}}"#)
            .expect("token");
        assert_eq!(token.expires_at_unix_ms, None);
    }

    #[test]
    fn signature_changes_when_a_rewrite_changes_the_file_size() {
        use crate::creds::CredentialSource;

        let path = std::env::temp_dir().join(format!(
            "ccub-local-creds-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        ));
        let creds = LocalClaudeCreds {
            id: format!("local:{}", path.display()),
            path: path.clone(),
        };
        assert_eq!(creds.signature(), None, "missing file has no signature");

        std::fs::write(&path, r#"{"claudeAiOauth":{"accessToken":"a"}}"#).expect("write");
        let before = creds.signature().expect("file exists");
        assert_eq!(
            Some(before.clone()),
            creds.signature(),
            "stable while unchanged"
        );

        std::fs::write(&path, r#"{"claudeAiOauth":{"accessToken":"rotated"}}"#).expect("rewrite");
        let after = creds.signature().expect("file exists");
        let _ = std::fs::remove_file(&path);
        assert_ne!(before, after);
    }

    #[test]
    fn missing_fields_are_reported() {
        let err = parse_claude_json(&fixture("claude-credentials-missing-token.json")).unwrap_err();
        assert!(matches!(err, Error::MissingField("accessToken")), "{err}");
        let err = parse_claude_json(&fixture("claude-credentials-missing-oauth.json")).unwrap_err();
        assert!(matches!(err, Error::MissingField("claudeAiOauth")), "{err}");
        let err = parse_claude_json("not json").unwrap_err();
        assert!(matches!(err, Error::Json(_)), "{err}");
    }
}
