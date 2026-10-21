// Parse Anthropic rate-limit headers into `UsageWindows`.
//
// The Messages API returns the user's remaining quota in response headers
// when the dedicated usage endpoint isn't available. Header names:
//   anthropic-ratelimit-unified-5h-utilization (0.0–1.0)
//   anthropic-ratelimit-unified-5h-reset       (Unix seconds)
//   anthropic-ratelimit-unified-7d-utilization
//   anthropic-ratelimit-unified-7d-reset

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::net::Response;
use crate::usage::{UsageWindows, Window};

pub fn parse_anthropic(response: &Response) -> UsageWindows {
    UsageWindows {
        primary: Window {
            utilization: (header_f64(response, "anthropic-ratelimit-unified-5h-utilization")
                * 100.0)
                .clamp(0.0, 100.0),
            resets_at: unix_to_systemtime(header_i64(
                response,
                "anthropic-ratelimit-unified-5h-reset",
            )),
        },
        secondary: Window {
            utilization: (header_f64(response, "anthropic-ratelimit-unified-7d-utilization")
                * 100.0)
                .clamp(0.0, 100.0),
            resets_at: unix_to_systemtime(header_i64(
                response,
                "anthropic-ratelimit-unified-7d-reset",
            )),
        },
    }
}

fn header_f64(response: &Response, name: &str) -> f64 {
    response
        .header(name)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0)
}

fn header_i64(response: &Response, name: &str) -> Option<i64> {
    response.header(name).and_then(|s| s.parse().ok())
}

fn unix_to_systemtime(secs: Option<i64>) -> Option<SystemTime> {
    let s = secs?;
    if s < 0 {
        return None;
    }
    Some(UNIX_EPOCH + Duration::from_secs(s as u64))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::test_server::serve_once;
    use crate::net::{Client, USER_AGENT};

    fn at(secs: u64) -> Option<SystemTime> {
        Some(UNIX_EPOCH + Duration::from_secs(secs))
    }

    #[test]
    fn fractions_scale_to_percent_and_resets_parse() {
        let resp = Response::from_parts(
            200,
            &[
                ("anthropic-ratelimit-unified-5h-utilization", "0.425"),
                ("anthropic-ratelimit-unified-5h-reset", "1761040800"),
                ("Anthropic-Ratelimit-Unified-7d-Utilization", "1.2"),
                ("anthropic-ratelimit-unified-7d-reset", "1761508800"),
            ],
            b"",
        );
        let w = parse_anthropic(&resp);
        assert!((w.primary.utilization - 42.5).abs() < 1e-9);
        assert_eq!(w.primary.resets_at, at(1_761_040_800));
        assert_eq!(w.secondary.utilization, 100.0, "clamped to 100");
        assert_eq!(w.secondary.resets_at, at(1_761_508_800));
    }

    #[test]
    fn missing_or_garbage_values_fall_back_to_empty() {
        let resp = Response::from_parts(
            200,
            &[
                ("anthropic-ratelimit-unified-5h-utilization", "abc"),
                ("anthropic-ratelimit-unified-5h-reset", "soon"),
            ],
            b"",
        );
        let w = parse_anthropic(&resp);
        assert_eq!(w.primary.utilization, 0.0);
        assert_eq!(w.primary.resets_at, None);
        assert_eq!(w.secondary.utilization, 0.0);
        assert_eq!(w.secondary.resets_at, None);
    }

    #[test]
    fn negative_values_clamp_and_drop_reset() {
        let resp = Response::from_parts(
            200,
            &[
                ("anthropic-ratelimit-unified-5h-utilization", "-0.3"),
                ("anthropic-ratelimit-unified-5h-reset", "-1"),
                ("anthropic-ratelimit-unified-7d-utilization", "0.5"),
                ("anthropic-ratelimit-unified-7d-reset", "0"),
            ],
            b"",
        );
        let w = parse_anthropic(&resp);
        assert_eq!(w.primary.utilization, 0.0);
        assert_eq!(w.primary.resets_at, None);
        assert_eq!(w.secondary.utilization, 50.0);
        assert_eq!(w.secondary.resets_at, at(0));
    }

    #[test]
    fn rate_limited_reply_still_yields_headers_over_http() {
        let served = serve_once(
            "429 Too Many Requests",
            &[
                ("anthropic-ratelimit-unified-5h-utilization", "1.0"),
                ("anthropic-ratelimit-unified-5h-reset", "1761040800"),
                ("anthropic-ratelimit-unified-7d-utilization", "0.25"),
                ("anthropic-ratelimit-unified-7d-reset", "1761508800"),
            ],
            br#"{"type":"error","error":{"type":"rate_limit_error"}}"#.to_vec(),
        );
        let client = Client::without_proxy(USER_AGENT);
        let resp = client
            .post(&served.base)
            .json_body(&serde_json::json!({"max_tokens": 1}))
            .and_then(crate::net::RequestBuilder::send)
            .expect("429 is a response, not an error");
        assert_eq!(resp.status(), 429);
        let w = parse_anthropic(&resp);
        assert_eq!(w.primary.utilization, 100.0);
        assert_eq!(w.primary.resets_at, at(1_761_040_800));
        assert_eq!(w.secondary.utilization, 25.0);
        assert_eq!(w.secondary.resets_at, at(1_761_508_800));
    }
}
