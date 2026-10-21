//! Parsing of the Windows "Internet Settings" static proxy.
//!
//! The registry reader lives in `platform::windows`; everything here is pure
//! so it can be tested on any host. PAC/WPAD scripts are not evaluated.

/// A static system proxy and the hosts that bypass it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SystemProxy {
    /// Proxy URI such as `http://proxy.corp:8080`, ready for `ureq::Proxy::new`.
    pub url: String,
    /// `ProxyOverride` entries, trimmed and non-empty.
    pub bypass: Vec<String>,
}

impl SystemProxy {
    /// Whether a request to `host` must skip the proxy.
    pub fn bypasses(&self, host: &str) -> bool {
        bypass_matches(&self.bypass, host)
    }
}

/// Build the proxy from the raw `ProxyEnable`, `ProxyServer` and
/// `ProxyOverride` registry values. Returns `None` unless the proxy is
/// enabled and the server string names a usable entry.
pub fn from_registry_values(
    enable: Option<u32>,
    server: Option<&str>,
    proxy_override: Option<&str>,
) -> Option<SystemProxy> {
    if enable != Some(1) {
        return None;
    }
    let url = parse_proxy_server(server?)?;
    Some(SystemProxy {
        url,
        bypass: parse_override(proxy_override.unwrap_or_default()),
    })
}

/// Pick the proxy for HTTPS traffic out of a `ProxyServer` value.
///
/// A per-scheme value (`http=h:p;https=h:p;…`) uses the `https=` entry, else
/// the `http=` entry. A bare `h:p` applies to every scheme. The result always
/// carries a scheme, defaulting to `http://` (a CONNECT proxy). Only `http`
/// and `https` proxies are accepted; any other scheme yields `None`.
pub fn parse_proxy_server(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let target = if value.contains('=') {
        let entries: Vec<(String, &str)> = value
            .split(';')
            .filter_map(|entry| entry.split_once('='))
            .map(|(scheme, addr)| (scheme.trim().to_ascii_lowercase(), addr.trim()))
            .filter(|(_, addr)| !addr.is_empty())
            .collect();
        let pick = |scheme: &str| {
            entries
                .iter()
                .find(|(s, _)| s == scheme)
                .map(|(_, addr)| *addr)
        };
        pick("https").or_else(|| pick("http"))?
    } else {
        value
    };
    match target.split_once("://") {
        None => Some(format!("http://{target}")),
        Some((scheme, _))
            if scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https") =>
        {
            Some(target.to_string())
        }
        // SOCKS and any other scheme are not supported as a system proxy.
        Some(_) => None,
    }
}

/// Split a `ProxyOverride` value on `;` into trimmed, non-empty entries.
pub fn parse_override(value: &str) -> Vec<String> {
    value
        .split(';')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(str::to_string)
        .collect()
}

/// Whether `host` matches any bypass entry. Entries are case-insensitive
/// globs where `*` matches any run of characters; `<local>` matches every
/// host without a dot.
pub fn bypass_matches(entries: &[String], host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    entries.iter().any(|entry| {
        if entry.eq_ignore_ascii_case("<local>") {
            !host.contains('.')
        } else {
            glob_match(&entry.to_ascii_lowercase(), &host)
        }
    })
}

/// Iterative `*`-only glob match with single-star backtracking.
fn glob_match(pattern: &str, text: &str) -> bool {
    let p = pattern.as_bytes();
    let t = text.as_bytes();
    let (mut pi, mut ti) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while ti < t.len() {
        if pi < p.len() && p[pi] == b'*' {
            star = Some((pi, ti));
            pi += 1;
        } else if pi < p.len() && p[pi] == t[ti] {
            pi += 1;
            ti += 1;
        } else if let Some((sp, st)) = star {
            pi = sp + 1;
            ti = st + 1;
            star = Some((sp, st + 1));
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == b'*')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries(value: &str) -> Vec<String> {
        parse_override(value)
    }

    #[test]
    fn per_scheme_server_prefers_https_entry() {
        assert_eq!(
            parse_proxy_server("http=web:80;https=secure:8443;ftp=f:21").as_deref(),
            Some("http://secure:8443")
        );
    }

    #[test]
    fn per_scheme_server_without_https_uses_http_entry() {
        assert_eq!(
            parse_proxy_server("ftp=f:21; HTTP=web:3128").as_deref(),
            Some("http://web:3128")
        );
    }

    #[test]
    fn per_scheme_server_without_http_entries_is_none() {
        assert_eq!(parse_proxy_server("socks=s:1080"), None);
        assert_eq!(parse_proxy_server("https=;http="), None);
    }

    #[test]
    fn bare_server_applies_to_all_schemes() {
        assert_eq!(
            parse_proxy_server(" proxy.corp:8080 ").as_deref(),
            Some("http://proxy.corp:8080")
        );
        assert_eq!(
            parse_proxy_server("http://proxy.corp:8080").as_deref(),
            Some("http://proxy.corp:8080")
        );
        assert_eq!(parse_proxy_server("   "), None);
    }

    #[test]
    fn non_http_proxy_schemes_are_rejected() {
        assert_eq!(parse_proxy_server("socks5://h:1"), None);
        assert_eq!(parse_proxy_server("https=socks5://h:1"), None);
        assert_eq!(parse_proxy_server("SOCKS4://h:1"), None);
        assert_eq!(
            parse_proxy_server("HTTPS://h:1").as_deref(),
            Some("HTTPS://h:1")
        );
        assert_eq!(
            from_registry_values(Some(1), Some("socks5://h:1"), None),
            None
        );
    }

    #[test]
    fn registry_values_need_enable_flag_and_server() {
        assert_eq!(
            from_registry_values(Some(0), Some("p:1"), Some("<local>")),
            None
        );
        assert_eq!(from_registry_values(None, Some("p:1"), None), None);
        assert_eq!(from_registry_values(Some(1), None, None), None);
        assert_eq!(
            from_registry_values(Some(1), Some("p:1"), Some("*.corp; <local> ;")),
            Some(SystemProxy {
                url: "http://p:1".into(),
                bypass: vec!["*.corp".into(), "<local>".into()],
            })
        );
        assert_eq!(
            from_registry_values(Some(1), Some("p:1"), None).map(|p| p.bypass),
            Some(Vec::new())
        );
    }

    #[test]
    fn override_wildcard_matches_subdomains_case_insensitively() {
        let list = entries("*.anthropic.com");
        assert!(bypass_matches(&list, "api.anthropic.com"));
        assert!(bypass_matches(&list, "API.Anthropic.COM"));
        assert!(!bypass_matches(&list, "anthropic.com"));
        assert!(!bypass_matches(&list, "chatgpt.com"));
    }

    #[test]
    fn override_exact_host_matches_only_that_host() {
        let list = entries("chatgpt.com");
        assert!(bypass_matches(&list, "chatgpt.com"));
        assert!(!bypass_matches(&list, "api.chatgpt.com"));
        assert!(!bypass_matches(&list, "chatgpt.com.evil"));
    }

    #[test]
    fn override_local_matches_dotless_hosts() {
        let list = entries("<LOCAL>");
        assert!(bypass_matches(&list, "intranet"));
        assert!(bypass_matches(&list, "localhost"));
        assert!(!bypass_matches(&list, "api.anthropic.com"));
        assert!(!bypass_matches(&list, "127.0.0.1"));
    }

    #[test]
    fn override_globs_cover_prefixes_and_inner_stars() {
        let list = entries("10.*;api.*.com");
        assert!(bypass_matches(&list, "10.1.2.3"));
        assert!(bypass_matches(&list, "api.anthropic.com"));
        assert!(!bypass_matches(&list, "chatgpt.com"));
        assert!(bypass_matches(&entries("*"), "anything.example"));
        assert!(!bypass_matches(&[], "chatgpt.com"));
    }
}
