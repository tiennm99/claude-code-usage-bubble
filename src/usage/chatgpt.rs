// Codex (ChatGPT) usage provider.
//
// Single endpoint: `/backend-api/wham/usage`. Response shape includes
// `rate_limit.{primary_window,secondary_window}.{used_percent,reset_at}`.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Deserialize;

use crate::creds::Locator;
use crate::net::Client;
use crate::usage::{Error, ProviderId, UsageProvider, UsageWindows, Window};

const USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";

pub struct ChatGptProvider {
    locator: Locator,
}

impl ChatGptProvider {
    pub fn new(locator: Locator) -> Self {
        Self { locator }
    }

    pub fn locator(&self) -> &Locator {
        &self.locator
    }
}

impl UsageProvider for ChatGptProvider {
    fn id(&self) -> ProviderId {
        ProviderId::ChatGpt
    }

    fn poll(&mut self, http: &Client) -> Result<UsageWindows, Error> {
        let source = self.locator.first_available().ok_or(Error::NoCredentials)?;
        let token = source.read()?;
        let mut req = http
            .get(USAGE_URL)
            .header("Authorization", &format!("Bearer {}", token.access_token))
            .header("User-Agent", "codex-cli");
        if let Some(account_id) = token.account_id.as_deref().filter(|s| !s.is_empty()) {
            req = req.header("ChatGPT-Account-Id", account_id);
        }
        let resp = match req.send() {
            Ok(r) => r,
            Err(crate::net::Error::Status(code)) if code == 401 || code == 403 => {
                return Err(Error::AuthRequired);
            }
            Err(e) => return Err(Error::Network(e)),
        };
        if resp.status() == 401 || resp.status() == 403 {
            return Err(Error::AuthRequired);
        }
        if !(200..300).contains(&resp.status()) {
            return Err(Error::BadResponse(format!(
                "Codex usage endpoint returned {}",
                resp.status()
            )));
        }
        let body: Envelope = resp
            .json()
            .map_err(|e| Error::BadResponse(format!("JSON parse: {e}")))?;
        envelope_to_windows(body)
            .ok_or_else(|| Error::BadResponse("missing rate_limit section".into()))
    }
}

fn envelope_to_windows(envelope: Envelope) -> Option<UsageWindows> {
    let rl = envelope.rate_limit.flatten_box()?;
    Some(UsageWindows {
        primary: rl
            .primary_window
            .flatten_box()
            .map(window_from)
            .unwrap_or_default(),
        secondary: rl
            .secondary_window
            .flatten_box()
            .map(window_from)
            .unwrap_or_default(),
    })
}

fn window_from(w: ApiWindow) -> Window {
    Window {
        utilization: w.used_percent.clamp(0.0, 100.0),
        resets_at: unix_to_systemtime(Some(w.reset_at)),
    }
}

fn unix_to_systemtime(secs: Option<i64>) -> Option<SystemTime> {
    let s = secs?;
    if s < 0 {
        return None;
    }
    Some(UNIX_EPOCH + Duration::from_secs(s as u64))
}

#[derive(Deserialize)]
struct Envelope {
    rate_limit: Option<Option<Box<RateLimit>>>,
}

#[derive(Deserialize)]
struct RateLimit {
    primary_window: Option<Option<Box<ApiWindow>>>,
    secondary_window: Option<Option<Box<ApiWindow>>>,
}

#[derive(Deserialize)]
struct ApiWindow {
    used_percent: f64,
    reset_at: i64,
}

// Helpers used to make `Option<Option<Box<…>>>` flatten cleanly. We can't
// reuse the std `Option::flatten` name — the inherent method (which returns
// `Option<Box<T>>`) would shadow this trait method.
trait FlattenBoxed<T> {
    fn flatten_box(self) -> Option<T>;
}
impl<T> FlattenBoxed<T> for Option<Option<Box<T>>> {
    fn flatten_box(self) -> Option<T> {
        self.and_then(|inner| inner.map(|b| *b))
    }
}
