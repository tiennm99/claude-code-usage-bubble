// Claude (Anthropic) usage provider.
//
// Two HTTP paths: the dedicated `/api/oauth/usage` endpoint (preferred,
// returns structured JSON with ISO 8601 reset times) and a fallback POST
// to `/v1/messages` that exposes rate-limit data via response headers.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Deserialize;

use crate::creds::Locator;
use crate::net::Client;
use crate::usage::{headers, Error, ProviderId, UsageProvider, UsageWindows, Window};

const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const MESSAGES_URL: &str = "https://api.anthropic.com/v1/messages";
const BETA_HEADER: &str = "oauth-2025-04-20";
const API_VERSION: &str = "2023-06-01";

const PROBE_MODELS: &[&str] = &["claude-3-haiku-20240307", "claude-haiku-4-5-20251001"];

pub struct ClaudeProvider {
    locator: Locator,
}

impl ClaudeProvider {
    pub fn new(locator: Locator) -> Self {
        Self { locator }
    }

    pub fn locator(&self) -> &Locator {
        &self.locator
    }
}

impl UsageProvider for ClaudeProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Claude
    }

    fn poll(&mut self, http: &Client) -> Result<UsageWindows, Error> {
        let source = self.locator.first_available().ok_or(Error::NoCredentials)?;
        let token = source.read()?;
        if token_is_expired(token.expires_at_unix_ms) {
            return Err(Error::AuthRequired);
        }
        fetch_with_fallback(http, &token.access_token, USAGE_URL, MESSAGES_URL)
    }
}

fn fetch_with_fallback(
    http: &Client,
    token: &str,
    usage_url: &str,
    messages_url: &str,
) -> Result<UsageWindows, Error> {
    // First try the dedicated endpoint.
    match try_usage_endpoint(http, token, usage_url)? {
        Some(windows) if has_reset_times(&windows) => return Ok(windows),
        Some(partial) => {
            // Got percentages but no reset times — fill them in from messages.
            if let Ok(fallback) = try_messages_endpoint(http, token, messages_url) {
                return Ok(merge_resets(partial, fallback));
            }
            return Ok(partial);
        }
        None => {}
    }
    try_messages_endpoint(http, token, messages_url)
}

fn try_usage_endpoint(
    http: &Client,
    token: &str,
    url: &str,
) -> Result<Option<UsageWindows>, Error> {
    let resp = match http
        .get(url)
        .header("Authorization", &format!("Bearer {token}"))
        .header("anthropic-beta", BETA_HEADER)
        .send()
    {
        Ok(r) => r,
        Err(crate::net::Error::Status(code)) if code == 401 || code == 403 => {
            return Err(Error::AuthRequired);
        }
        Err(_) => return Ok(None),
    };
    if resp.status() == 401 || resp.status() == 403 {
        return Err(Error::AuthRequired);
    }
    if !(200..300).contains(&resp.status()) {
        return Ok(None);
    }
    Ok(parse_usage_body(resp.body()))
}

/// Windows from an `/api/oauth/usage` body, or `None` when the body does not
/// match the expected shape (which sends the caller to the Messages fallback).
fn parse_usage_body(body: &[u8]) -> Option<UsageWindows> {
    let body: OauthUsage = serde_json::from_slice(body).ok()?;
    let primary = body.five_hour.map(bucket_to_window).unwrap_or_default();
    let secondary = body.seven_day.map(bucket_to_window).unwrap_or_default();
    Some(UsageWindows { primary, secondary })
}

fn try_messages_endpoint(http: &Client, token: &str, url: &str) -> Result<UsageWindows, Error> {
    for model in PROBE_MODELS {
        let body = serde_json::json!({
            "model": model,
            "max_tokens": 1,
            "messages": [{"role": "user", "content": "."}],
        });
        let resp = match http
            .post(url)
            .header("Authorization", &format!("Bearer {token}"))
            .header("anthropic-version", API_VERSION)
            .header("anthropic-beta", BETA_HEADER)
            .json_body(&body)
            .and_then(|rb| rb.send())
        {
            Ok(r) => r,
            Err(crate::net::Error::Status(code)) if code == 401 || code == 403 => {
                return Err(Error::AuthRequired);
            }
            Err(_) => continue,
        };
        if resp.status() == 401 || resp.status() == 403 {
            return Err(Error::AuthRequired);
        }
        // Even an error response from Messages can carry rate-limit headers.
        if resp
            .header("anthropic-ratelimit-unified-5h-utilization")
            .is_some()
            || resp
                .header("anthropic-ratelimit-unified-7d-utilization")
                .is_some()
        {
            return Ok(headers::parse_anthropic(&resp));
        }
    }
    Err(Error::BadResponse(
        "no rate-limit headers in messages response".into(),
    ))
}

fn bucket_to_window(bucket: Bucket) -> Window {
    Window {
        utilization: bucket.utilization.clamp(0.0, 100.0),
        resets_at: bucket.resets_at.as_deref().and_then(parse_iso8601),
    }
}

fn has_reset_times(w: &UsageWindows) -> bool {
    w.primary.resets_at.is_some() && w.secondary.resets_at.is_some()
}

fn merge_resets(mut primary: UsageWindows, fallback: UsageWindows) -> UsageWindows {
    if primary.primary.resets_at.is_none() {
        primary.primary.resets_at = fallback.primary.resets_at;
    }
    if primary.secondary.resets_at.is_none() {
        primary.secondary.resets_at = fallback.secondary.resets_at;
    }
    primary
}

fn token_is_expired(expires_at_unix_ms: Option<i64>) -> bool {
    let Some(exp_ms) = expires_at_unix_ms else {
        return false;
    };
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    now_ms >= exp_ms
}

// --- ISO 8601 parsing (minimal — handles "YYYY-MM-DDTHH:MM:SS[.frac][Z|±HH:MM]") ---

fn parse_iso8601(s: &str) -> Option<SystemTime> {
    let (date, time_with_offset) = s.split_once('T')?;

    // Split the time-and-offset on the first 'Z' / '+' / '-' marker.
    let offset_pos = time_with_offset
        .char_indices()
        .find(|(_, c)| matches!(c, 'Z' | '+' | '-'))
        .map(|(i, _)| i);
    let (time, offset_str) = match offset_pos {
        Some(p) => (&time_with_offset[..p], &time_with_offset[p..]),
        None => (time_with_offset, ""),
    };
    let time = time.split_once('.').map_or(time, |(t, _)| t);

    let date_parts: Vec<&str> = date.split('-').collect();
    if date_parts.len() != 3 {
        return None;
    }
    let y: u64 = date_parts[0].parse().ok()?;
    let mo: u64 = date_parts[1].parse().ok()?;
    let d: u64 = date_parts[2].parse().ok()?;
    if y < 1970 || mo == 0 || mo > 12 || d == 0 {
        return None;
    }
    const DAYS_IN_MONTH: [u64; 13] = [0, 31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let max_day = DAYS_IN_MONTH[mo as usize] + if mo == 2 && is_leap(y) { 1 } else { 0 };
    if d > max_day {
        return None;
    }

    let time_parts: Vec<&str> = time.split(':').collect();
    if time_parts.len() != 3 {
        return None;
    }
    let h: u64 = time_parts[0].parse().ok()?;
    let mi: u64 = time_parts[1].parse().ok()?;
    let se: u64 = time_parts[2].parse().ok()?;

    let offset_minutes: i64 = if offset_str.is_empty() || offset_str == "Z" {
        0
    } else {
        let sign: i64 = if offset_str.starts_with('+') {
            1
        } else if offset_str.starts_with('-') {
            -1
        } else {
            return None;
        };
        let (oh_str, om_str) = offset_str[1..].split_once(':')?;
        let oh: i64 = oh_str.parse().ok()?;
        let om: i64 = om_str.parse().ok()?;
        sign * (oh * 60 + om)
    };

    let mut days: u64 = 0;
    for year in 1970..y {
        days += if is_leap(year) { 366 } else { 365 };
    }
    for month in 1..mo {
        days += DAYS_IN_MONTH[month as usize];
        if month == 2 && is_leap(y) {
            days += 1;
        }
    }
    days += d - 1;

    let local_secs = days * 86_400 + h * 3_600 + mi * 60 + se;
    let utc_secs = (local_secs as i64) - offset_minutes * 60;
    if utc_secs < 0 {
        return None;
    }
    Some(UNIX_EPOCH + Duration::from_secs(utc_secs as u64))
}

fn is_leap(year: u64) -> bool {
    (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400)
}

// --- JSON shape ---

#[derive(Deserialize)]
struct OauthUsage {
    five_hour: Option<Bucket>,
    seven_day: Option<Bucket>,
}

#[derive(Deserialize)]
struct Bucket {
    utilization: f64,
    resets_at: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::test_server::{serve_once, Served};
    use crate::net::USER_AGENT;

    fn at(secs: u64) -> Option<SystemTime> {
        Some(UNIX_EPOCH + Duration::from_secs(secs))
    }

    fn fixture(name: &str) -> Vec<u8> {
        let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        std::fs::read(&path).unwrap_or_else(|e| panic!("read {path}: {e}"))
    }

    #[test]
    fn full_usage_body_maps_both_buckets() {
        let w = parse_usage_body(&fixture("anthropic-usage-full.json")).expect("parses");
        assert_eq!(w.primary.utilization, 42.5);
        assert_eq!(w.primary.resets_at, at(1_761_040_800));
        assert_eq!(w.secondary.utilization, 100.0, "clamped to 100");
        assert_eq!(w.secondary.resets_at, at(1_761_508_800));
        assert!(has_reset_times(&w));
    }

    #[test]
    fn partial_usage_body_keeps_percentages_and_merges_fallback_resets() {
        let w = parse_usage_body(&fixture("anthropic-usage-partial.json")).expect("parses");
        assert_eq!(w.primary.utilization, 7.0);
        assert_eq!(w.primary.resets_at, None);
        assert_eq!(w.secondary.resets_at, at(1_709_251_199));
        assert!(!has_reset_times(&w));

        let fallback = UsageWindows {
            primary: Window {
                utilization: 99.0,
                resets_at: at(10),
            },
            secondary: Window {
                utilization: 99.0,
                resets_at: at(20),
            },
        };
        let merged = merge_resets(w, fallback);
        assert_eq!(merged.primary.utilization, 7.0, "percent is not replaced");
        assert_eq!(merged.primary.resets_at, at(10));
        assert_eq!(merged.secondary.resets_at, at(1_709_251_199));
    }

    #[test]
    fn missing_bucket_defaults_to_empty_window() {
        let w = parse_usage_body(&fixture("anthropic-usage-five-hour-only.json")).expect("parses");
        assert_eq!(w.primary.utilization, 3.0);
        assert_eq!(w.secondary.utilization, 0.0);
        assert_eq!(w.secondary.resets_at, None);
    }

    #[test]
    fn null_buckets_parse_as_empty_windows() {
        let w = parse_usage_body(&fixture("anthropic-usage-null-buckets.json")).expect("parses");
        assert_eq!(w.primary.utilization, 0.0);
        assert_eq!(w.secondary.utilization, 0.0);
        assert!(!has_reset_times(&w));
    }

    #[test]
    fn null_utilization_rejects_body_so_fallback_runs() {
        assert!(parse_usage_body(&fixture("anthropic-usage-null-utilization.json")).is_none());
        assert!(parse_usage_body(b"not json").is_none());
    }

    #[test]
    fn iso8601_accepts_utc_offsets_and_fractions() {
        assert_eq!(parse_iso8601("2025-10-21T10:00:00Z"), at(1_761_040_800));
        assert_eq!(
            parse_iso8601("2025-10-21T10:00:00.999999+00:00"),
            at(1_761_040_800)
        );
        assert_eq!(
            parse_iso8601("2025-10-27T03:00:00+07:00"),
            at(1_761_508_800)
        );
        assert_eq!(
            parse_iso8601("2025-10-21T05:30:00-04:30"),
            at(1_761_040_800)
        );
        assert_eq!(parse_iso8601("2025-10-21T10:00:00"), at(1_761_040_800));
        assert_eq!(parse_iso8601("2024-02-29T23:59:59Z"), at(1_709_251_199));
        assert_eq!(parse_iso8601("1970-01-01T00:00:00Z"), at(0));
    }

    #[test]
    fn iso8601_rejects_malformed_values() {
        assert_eq!(
            parse_iso8601("2025-10-21T10:00:00+0000"),
            None,
            "colonless offset"
        );
        assert_eq!(parse_iso8601("2025-10-21t10:00:00Z"), None, "lowercase t");
        assert_eq!(parse_iso8601("1969-12-31T23:59:59Z"), None, "before 1970");
        assert_eq!(
            parse_iso8601("1970-01-01T00:30:00+01:00"),
            None,
            "negative epoch"
        );
        assert_eq!(
            parse_iso8601("2025-02-29T00:00:00Z"),
            None,
            "not a leap year"
        );
        assert_eq!(parse_iso8601("2025-13-01T00:00:00Z"), None);
        assert_eq!(parse_iso8601("2025-10-21 10:00:00Z"), None);
        assert_eq!(parse_iso8601("2025-10-21T10:00Z"), None);
        assert_eq!(parse_iso8601(""), None);
    }

    #[test]
    fn token_expiry_compares_milliseconds() {
        assert!(!token_is_expired(None));
        assert!(token_is_expired(Some(0)));
        assert!(!token_is_expired(Some(i64::MAX)));
    }

    // --- HTTP paths against a loopback server ---

    const RATE_LIMIT_HEADERS: &[(&str, &str)] = &[
        ("anthropic-ratelimit-unified-5h-utilization", "0.25"),
        ("anthropic-ratelimit-unified-5h-reset", "1000"),
        ("anthropic-ratelimit-unified-7d-utilization", "0.5"),
        ("anthropic-ratelimit-unified-7d-reset", "2000"),
    ];

    fn client() -> Client {
        Client::without_proxy(USER_AGENT)
    }

    fn fetch(usage: &Served, messages: &Served) -> Result<UsageWindows, Error> {
        fetch_with_fallback(&client(), "tok", &usage.base, &messages.base)
    }

    fn received(served: &Served) -> String {
        served
            .request
            .recv_timeout(Duration::from_secs(10))
            .expect("server saw the request")
    }

    fn assert_not_called(served: &Served) {
        assert!(
            served
                .request
                .recv_timeout(Duration::from_millis(300))
                .is_err(),
            "endpoint must not be called"
        );
    }

    fn messages_with_headers() -> Served {
        serve_once("200 OK", RATE_LIMIT_HEADERS, b"{}".to_vec())
    }

    #[test]
    fn usage_auth_failures_stop_before_the_messages_fallback() {
        for status in ["401 Unauthorized", "403 Forbidden"] {
            let usage = serve_once(status, &[], b"{}".to_vec());
            let messages = messages_with_headers();
            let result = fetch(&usage, &messages);
            assert!(
                matches!(result, Err(Error::AuthRequired)),
                "{status}: {result:?}"
            );
            let request = received(&usage);
            assert!(request.starts_with("GET / HTTP/1.1\r\n"), "{request}");
            let lower = request.to_ascii_lowercase();
            assert!(lower.contains("authorization: bearer tok\r\n"), "{request}");
            assert!(
                lower.contains("anthropic-beta: oauth-2025-04-20\r\n"),
                "{request}"
            );
            assert_not_called(&messages);
        }
    }

    #[test]
    fn usage_server_error_falls_back_to_messages_headers() {
        let usage = serve_once("500 Internal Server Error", &[], b"oops".to_vec());
        let messages = messages_with_headers();
        let w = fetch(&usage, &messages).expect("fallback windows");
        assert_eq!(w.primary.utilization, 25.0);
        assert_eq!(w.primary.resets_at, at(1000));
        assert_eq!(w.secondary.utilization, 50.0);
        assert_eq!(w.secondary.resets_at, at(2000));
        let request = received(&messages);
        assert!(request.starts_with("POST / HTTP/1.1\r\n"), "{request}");
        assert!(request
            .to_ascii_lowercase()
            .contains("anthropic-version: 2023-06-01\r\n"));
        assert!(request.contains("\"max_tokens\":1"), "{request}");
    }

    #[test]
    fn unparsable_usage_body_falls_back_to_messages_headers() {
        let usage = serve_once("200 OK", &[], b"not json".to_vec());
        let messages = messages_with_headers();
        let w = fetch(&usage, &messages).expect("fallback windows");
        assert_eq!(w.primary.utilization, 25.0);
        assert_eq!(w.secondary.resets_at, at(2000));
        received(&messages);
    }

    #[test]
    fn full_usage_body_skips_the_messages_call() {
        let usage = serve_once("200 OK", &[], fixture("anthropic-usage-full.json"));
        let messages = messages_with_headers();
        let w = fetch(&usage, &messages).expect("usage windows");
        assert_eq!(w.primary.utilization, 42.5);
        assert_eq!(w.primary.resets_at, at(1_761_040_800));
        assert_not_called(&messages);
    }

    #[test]
    fn partial_usage_body_merges_reset_times_from_messages() {
        let usage = serve_once("200 OK", &[], fixture("anthropic-usage-partial.json"));
        let messages = messages_with_headers();
        let w = fetch(&usage, &messages).expect("merged windows");
        assert_eq!(w.primary.utilization, 7.0, "usage percent is kept");
        assert_eq!(w.primary.resets_at, at(1000), "missing reset is filled in");
        assert_eq!(w.secondary.utilization, 61.0);
        assert_eq!(
            w.secondary.resets_at,
            at(1_709_251_199),
            "known reset is kept"
        );
        received(&messages);
    }

    #[test]
    fn messages_auth_failure_is_auth_required() {
        let usage = serve_once("503 Service Unavailable", &[], Vec::new());
        let messages = serve_once("401 Unauthorized", RATE_LIMIT_HEADERS, b"{}".to_vec());
        let result = fetch(&usage, &messages);
        assert!(matches!(result, Err(Error::AuthRequired)), "{result:?}");
    }

    #[test]
    fn messages_without_rate_limit_headers_is_a_bad_response() {
        // The second probe model finds the one-shot server gone, which
        // counts as a failed probe too.
        let usage = serve_once("500 Internal Server Error", &[], Vec::new());
        let messages = serve_once("200 OK", &[], b"{}".to_vec());
        let result = fetch(&usage, &messages);
        assert!(matches!(result, Err(Error::BadResponse(_))), "{result:?}");
    }
}
