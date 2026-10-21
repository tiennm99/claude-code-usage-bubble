//! Minimal blocking HTTP client on `ureq`.
//!
//! One [`Client`] is shared across poll workers. 4xx and 5xx responses come
//! back as ordinary [`Response`] values so providers can read the status and
//! the rate-limit headers of an error reply. Tokens and request headers are
//! never logged here.

use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::Serialize;
use ureq::http::{self, Uri};
use ureq::tls::{RootCerts, TlsConfig};
use ureq::{Agent, Proxy, ProxyProtocol};

use crate::core::proxy::SystemProxy;

/// Default User-Agent; a per-request `User-Agent` header replaces it.
pub const USER_AGENT: &str = concat!("claude-code-usage-bubble/", env!("CARGO_PKG_VERSION"));

const CONNECT_TIMEOUT: Duration = Duration::from_secs(60);
const RECEIVE_TIMEOUT: Duration = Duration::from_secs(30);
const GLOBAL_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("HTTP transport: {0}")]
    Transport(String),
    #[error("response was not valid UTF-8")]
    Utf8,
    #[error("JSON parse: {0}")]
    Json(#[from] serde_json::Error),
    /// Kept for the providers' 401/403 match arms. The client reports every
    /// status as a `Response`, so it never builds this variant itself.
    #[error("response status {0}")]
    Status(u32),
}

impl From<ureq::Error> for Error {
    fn from(err: ureq::Error) -> Self {
        Error::Transport(err.to_string())
    }
}

impl From<http::Error> for Error {
    fn from(err: http::Error) -> Self {
        Error::Transport(err.to_string())
    }
}

/// Proxy variables in the order `ureq` reads them; the first usable one wins.
const ENV_PROXY_VARS: &[&str] = &[
    "ALL_PROXY",
    "all_proxy",
    "HTTPS_PROXY",
    "https_proxy",
    "HTTP_PROXY",
    "http_proxy",
];

/// The environment proxy, if one is set.
///
/// Walks the variables `ureq` reads and keeps the first valid one, whatever its
/// scheme: HTTP and HTTPS proxies are reached with CONNECT, SOCKS4/4a/5/5h
/// proxies (`ALL_PROXY=socks5h://…`, as Clash and v2ray set) through ureq's
/// `socks-proxy` support. `NO_PROXY` applies to the chosen proxy.
pub fn env_proxy() -> Option<Proxy> {
    env_proxy_from(|name| std::env::var(name).ok())
}

fn env_proxy_from(var: impl Fn(&str) -> Option<String>) -> Option<Proxy> {
    let no_proxy = var("NO_PROXY").or_else(|| var("no_proxy"));
    for name in ENV_PROXY_VARS {
        let Some(value) = var(name) else { continue };
        // Invalid values are skipped, as `ureq` does. Values are not logged:
        // they may carry credentials.
        let Ok(parsed) = Proxy::new(&value) else {
            continue;
        };
        // Rebuild so the proxy carries the `NO_PROXY` bypass list, which only
        // the builder accepts.
        let mut builder = Proxy::builder(parsed.protocol())
            .host(parsed.host())
            .port(parsed.port());
        if let Some(user) = parsed.username() {
            builder = builder.username(user);
        }
        if let Some(password) = parsed.password() {
            builder = builder.password(password);
        }
        for expr in no_proxy.iter().flat_map(|v| v.split(',')) {
            let expr = expr.trim();
            if !expr.is_empty() {
                builder = builder.no_proxy(expr);
            }
        }
        match builder.build() {
            Ok(proxy) => return Some(proxy),
            Err(err) => log::warn!("{name} ignored, it is not a valid proxy URI: {err}"),
        }
    }
    None
}

/// Whether `proxy` is an HTTP CONNECT proxy, the only kind the system proxy
/// may be.
fn is_connect_proxy(proxy: &Proxy) -> bool {
    matches!(proxy.protocol(), ProxyProtocol::Http | ProxyProtocol::Https)
}

/// The static system proxy: the `Internet Settings` proxy on Windows, none
/// elsewhere. It applies only when [`env_proxy`] finds nothing.
pub fn system_proxy() -> Option<SystemProxy> {
    #[cfg(windows)]
    {
        crate::platform::windows::system_proxy()
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// A system-proxied agent plus the bypass rules that route around it.
struct Proxied {
    agent: Agent,
    proxy: SystemProxy,
}

pub struct Client {
    /// Direct agent, or the environment-proxied one when an env proxy is set.
    direct: Agent,
    proxied: Option<Proxied>,
}

impl Client {
    /// Create a client whose default User-Agent is `user_agent`.
    pub fn new(user_agent: &str) -> Result<Self, Error> {
        let env = env_proxy();
        let system = if env.is_some() { None } else { system_proxy() };
        Self::build(user_agent, env, system)
    }

    /// A client that ignores every proxy, for loopback tests.
    #[cfg(test)]
    pub(crate) fn without_proxy(user_agent: &str) -> Self {
        Self::build(user_agent, None, None).expect("client without proxy")
    }

    fn build(
        user_agent: &str,
        env_proxy: Option<Proxy>,
        system: Option<SystemProxy>,
    ) -> Result<Self, Error> {
        let direct = make_agent(user_agent, env_proxy);
        let proxied = match system {
            Some(proxy) => match Proxy::new(&proxy.url) {
                // The system proxy keeps the old WinHTTP client's scope:
                // CONNECT proxies only, as `core::proxy` already parses it.
                Ok(p) if !is_connect_proxy(&p) => {
                    log::warn!("system proxy ignored, only http and https proxies are supported");
                    None
                }
                Ok(p) => Some(Proxied {
                    agent: make_agent(user_agent, Some(p)),
                    proxy,
                }),
                Err(err) => {
                    // The proxy URL is not logged: it may carry credentials.
                    log::warn!("system proxy ignored, it is not a valid proxy URI: {err}");
                    None
                }
            },
            None => None,
        };
        Ok(Self { direct, proxied })
    }

    /// The system proxy this client routes through, if any. The updater
    /// reuses it so both paths reach the network the same way.
    pub fn system_proxy(&self) -> Option<&SystemProxy> {
        self.proxied.as_ref().map(|p| &p.proxy)
    }

    pub fn get<'a>(&'a self, url: &str) -> RequestBuilder<'a> {
        RequestBuilder::new(self, http::Method::GET, url)
    }

    pub fn post<'a>(&'a self, url: &str) -> RequestBuilder<'a> {
        RequestBuilder::new(self, http::Method::POST, url)
    }

    /// The agent for `uri`: direct when the host matches a bypass entry or no
    /// system proxy is configured, proxied otherwise.
    fn agent_for(&self, uri: &Uri) -> &Agent {
        match &self.proxied {
            Some(p) if !uri.host().is_some_and(|h| p.proxy.bypasses(h)) => &p.agent,
            _ => &self.direct,
        }
    }
}

fn make_agent(user_agent: &str, proxy: Option<Proxy>) -> Agent {
    let config = Agent::config_builder()
        .http_status_as_error(false)
        .timeout_connect(Some(CONNECT_TIMEOUT))
        .timeout_recv_response(Some(RECEIVE_TIMEOUT))
        .timeout_recv_body(Some(RECEIVE_TIMEOUT))
        .timeout_global(Some(GLOBAL_TIMEOUT))
        .user_agent(user_agent)
        .proxy(proxy)
        .tls_config(
            TlsConfig::builder()
                .root_certs(RootCerts::PlatformVerifier)
                .build(),
        )
        .build();
    Agent::new_with_config(config)
}

pub struct RequestBuilder<'a> {
    client: &'a Client,
    method: http::Method,
    url: String,
    headers: Vec<(String, String)>,
    body: Option<Vec<u8>>,
}

impl<'a> RequestBuilder<'a> {
    fn new(client: &'a Client, method: http::Method, url: &str) -> Self {
        Self {
            client,
            method,
            url: url.to_string(),
            headers: Vec::new(),
            body: None,
        }
    }

    /// Add a header. A `User-Agent` header replaces the client default.
    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_string(), value.to_string()));
        self
    }

    pub fn json_body<T: Serialize>(mut self, body: &T) -> Result<Self, Error> {
        self.body = Some(serde_json::to_vec(body)?);
        self.headers
            .push(("Content-Type".into(), "application/json".into()));
        Ok(self)
    }

    pub fn send(self) -> Result<Response, Error> {
        let uri: Uri = self
            .url
            .parse()
            .map_err(|err| Error::Transport(format!("invalid URL: {err}")))?;
        let agent = self.client.agent_for(&uri);
        let mut builder = http::Request::builder().method(self.method).uri(uri);
        for (name, value) in &self.headers {
            builder = builder.header(name.as_str(), value.as_str());
        }
        let response = match self.body {
            Some(body) => agent.run(builder.body(body)?)?,
            None => agent.run(builder.body(())?)?,
        };
        let status = u32::from(response.status().as_u16());
        let headers = response
            .headers()
            .iter()
            .map(|(name, value)| {
                (
                    name.as_str().to_string(),
                    String::from_utf8_lossy(value.as_bytes()).into_owned(),
                )
            })
            .collect();
        let body = response.into_body().read_to_vec()?;
        Ok(Response {
            status,
            headers,
            body,
        })
    }
}

pub struct Response {
    status: u32,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Response {
    /// Build a response by hand, for tests of code that inspects responses.
    #[cfg(test)]
    pub(crate) fn from_parts(status: u32, headers: &[(&str, &str)], body: &[u8]) -> Self {
        Self {
            status,
            headers: headers
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            body: body.to_vec(),
        }
    }

    pub fn status(&self) -> u32 {
        self.status
    }

    /// First header named `name`, compared case-insensitively.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    pub fn body(&self) -> &[u8] {
        &self.body
    }

    pub fn text(&self) -> Result<&str, Error> {
        std::str::from_utf8(&self.body).map_err(|_| Error::Utf8)
    }

    pub fn json<T: DeserializeOwned>(&self) -> Result<T, Error> {
        Ok(serde_json::from_slice(&self.body)?)
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::time::Duration;

    use super::test_server::serve_once;
    use super::*;

    fn direct_client() -> Client {
        Client::without_proxy(USER_AGENT)
    }

    fn received(served: &test_server::Served) -> String {
        served
            .request
            .recv_timeout(Duration::from_secs(10))
            .expect("server saw the request")
    }

    #[test]
    fn error_status_is_a_response_with_headers() {
        let served = serve_once(
            "429 Too Many Requests",
            &[("anthropic-ratelimit-unified-5h-utilization", "0.5")],
            b"{\"error\":\"rate_limited\"}".to_vec(),
        );
        let resp = direct_client()
            .get(&format!("{}/v1/messages", served.base))
            .send()
            .expect("429 is not a transport error");
        assert_eq!(resp.status(), 429);
        assert_eq!(
            resp.header("Anthropic-Ratelimit-Unified-5h-Utilization"),
            Some("0.5")
        );
        assert_eq!(resp.text().expect("utf8"), "{\"error\":\"rate_limited\"}");
        received(&served);
    }

    #[test]
    fn sends_default_user_agent() {
        let served = serve_once("200 OK", &[], Vec::new());
        direct_client().get(&served.base).send().expect("send");
        let request = received(&served).to_ascii_lowercase();
        let expected = format!("user-agent: {}\r\n", USER_AGENT.to_ascii_lowercase());
        assert!(request.contains(&expected), "{request}");
        assert!(USER_AGENT.starts_with("claude-code-usage-bubble/"));
    }

    #[test]
    fn per_request_user_agent_replaces_default() {
        let served = serve_once("200 OK", &[], Vec::new());
        direct_client()
            .get(&served.base)
            .header("User-Agent", "codex-cli")
            .send()
            .expect("send");
        let request = received(&served).to_ascii_lowercase();
        assert!(request.contains("user-agent: codex-cli\r\n"), "{request}");
        assert_eq!(request.matches("user-agent:").count(), 1, "{request}");
    }

    #[test]
    fn json_body_is_posted_with_content_type() {
        let served = serve_once("200 OK", &[], b"{\"ok\":true}".to_vec());
        let resp = direct_client()
            .post(&served.base)
            .json_body(&serde_json::json!({"max_tokens": 1}))
            .and_then(RequestBuilder::send)
            .expect("send");
        let value: serde_json::Value = resp.json().expect("json");
        assert_eq!(value["ok"], true);
        let request = received(&served);
        assert!(request.starts_with("POST / HTTP/1.1\r\n"), "{request}");
        assert!(request
            .to_ascii_lowercase()
            .contains("content-type: application/json\r\n"));
        assert!(request.ends_with("{\"max_tokens\":1}"), "{request}");
    }

    #[test]
    fn gzip_body_is_decompressed() {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(b"plain text").expect("compress");
        let gz = encoder.finish().expect("finish");
        let served = serve_once("200 OK", &[("Content-Encoding", "gzip")], gz);
        let resp = direct_client().get(&served.base).send().expect("send");
        assert_eq!(resp.body(), b"plain text");
        let request = received(&served).to_ascii_lowercase();
        assert!(request.contains("accept-encoding: gzip"), "{request}");
    }

    #[test]
    fn transport_failure_is_an_error() {
        let served = serve_once("200 OK", &[], Vec::new());
        let base = served.base.clone();
        // Free the port by completing the one request the server accepts.
        direct_client().get(&base).send().expect("send");
        received(&served);
        let err = direct_client()
            .get(&base)
            .send()
            .err()
            .expect("closed port fails");
        assert!(matches!(err, Error::Transport(_)), "{err}");
        assert!(matches!(
            direct_client().get("not a url").send(),
            Err(Error::Transport(_))
        ));
    }

    #[test]
    fn system_proxy_is_used_unless_the_host_is_bypassed() {
        let proxy = serve_once("200 OK", &[], Vec::new());
        let origin = serve_once("200 OK", &[], b"direct".to_vec());
        let system = SystemProxy {
            url: proxy.base.clone(),
            bypass: Vec::new(),
        };
        let client = Client::build(USER_AGENT, None, Some(system.clone())).expect("client");
        assert_eq!(client.system_proxy(), Some(&system));
        // The fake proxy does not tunnel, so the request itself fails; what
        // matters is that it went to the proxy as a CONNECT for the origin.
        let _ = client.get(&format!("{}/x", origin.base)).send();
        let request = received(&proxy);
        let authority = origin.base.trim_start_matches("http://");
        assert!(
            request.starts_with(&format!("CONNECT {authority} HTTP/1.1")),
            "{request}"
        );

        let bypassing = Client::build(
            USER_AGENT,
            None,
            Some(SystemProxy {
                url: proxy.base.clone(),
                bypass: vec!["127.0.0.*".into()],
            }),
        )
        .expect("client");
        let resp = bypassing.get(&origin.base).send().expect("send");
        assert_eq!(resp.body(), b"direct");
        received(&origin);
    }

    #[test]
    fn invalid_system_proxy_falls_back_to_direct() {
        let client = Client::build(
            USER_AGENT,
            None,
            Some(SystemProxy {
                url: "http://bad host:notaport".into(),
                bypass: Vec::new(),
            }),
        )
        .expect("client");
        assert_eq!(client.system_proxy(), None);
    }

    fn env(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let vars: Vec<(String, String)> = vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |name| vars.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone())
    }

    /// A one-shot SOCKS5 proxy that accepts the handshake, reports the CONNECT
    /// target it was asked for, then answers the tunnelled request itself with
    /// `200 OK` and `body`.
    fn serve_socks5_once(body: &'static [u8]) -> (u16, std::sync::mpsc::Receiver<String>) {
        use std::io::Read;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let port = listener.local_addr().expect("local addr").port();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            // A broken handshake must fail the test, not hang it.
            let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(10)));
            let byte = |stream: &mut std::net::TcpStream, n: usize| {
                let mut buf = vec![0u8; n];
                stream.read_exact(&mut buf).map(|_| buf)
            };
            // Greeting: version 5, then the offered auth methods.
            let Ok(head) = byte(&mut stream, 2) else {
                return;
            };
            let _ = byte(&mut stream, usize::from(head[1]));
            if stream.write_all(&[5, 0]).is_err() {
                return;
            }
            // Request: version, CONNECT, reserved, address type.
            let Ok(req) = byte(&mut stream, 4) else {
                return;
            };
            let target = match req[3] {
                3 => {
                    let len = byte(&mut stream, 1).map(|b| usize::from(b[0])).unwrap_or(0);
                    String::from_utf8_lossy(&byte(&mut stream, len).unwrap_or_default())
                        .into_owned()
                }
                1 => byte(&mut stream, 4)
                    .map(|a| std::net::Ipv4Addr::new(a[0], a[1], a[2], a[3]).to_string())
                    .unwrap_or_default(),
                other => format!("address type {other}"),
            };
            let port = byte(&mut stream, 2)
                .map(|p| u16::from_be_bytes([p[0], p[1]]))
                .unwrap_or(0);
            let _ = stream.write_all(&[5, 0, 0, 1, 0, 0, 0, 0, 0, 0]);
            // Read the tunnelled request head, then answer it.
            let mut seen = Vec::new();
            let mut buf = [0u8; 1024];
            while !seen.windows(4).any(|w| w == b"\r\n\r\n") {
                match stream.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => seen.extend_from_slice(&buf[..n]),
                }
            }
            let reply = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(reply.as_bytes());
            let _ = stream.write_all(body);
            let _ = tx.send(format!(
                "{} {target}:{port}\n{}",
                req[0],
                String::from_utf8_lossy(&seen)
            ));
        });
        (port, rx)
    }

    #[test]
    fn socks5h_env_proxy_tunnels_with_remote_resolution() {
        let (socks_port, seen) = serve_socks5_once(b"via socks");
        let http_proxy = serve_once("200 OK", &[], Vec::new());
        // Clash and v2ray set both; ureq's order puts ALL_PROXY first.
        let chosen = env_proxy_from(env(&[
            ("all_proxy", &format!("socks5h://127.0.0.1:{socks_port}")),
            ("https_proxy", &http_proxy.base),
        ]))
        .expect("socks env proxy");
        assert_eq!(chosen.protocol(), ProxyProtocol::Socks5h);

        let client = Client::build(USER_AGENT, Some(chosen), None).expect("client");
        // `socks5h` hands the host name to the proxy instead of resolving it,
        // so the name need not resolve to the fake proxy's own address.
        let resp = client
            .get("http://example.invalid:8080/x")
            .send()
            .expect("send through socks");
        assert_eq!(resp.body(), b"via socks");
        let request = seen
            .recv_timeout(Duration::from_secs(10))
            .expect("socks proxy saw the request");
        assert!(
            request.starts_with("5 example.invalid:8080\nGET /x HTTP/1.1\r\n"),
            "{request}"
        );
        assert!(http_proxy
            .request
            .recv_timeout(Duration::from_millis(200))
            .is_err());
    }

    #[test]
    fn env_proxy_follows_ureq_order_and_skips_invalid_values() {
        let socks = env_proxy_from(env(&[("ALL_PROXY", "socks5://127.0.0.1:1080")]))
            .expect("socks env proxy");
        assert_eq!(socks.protocol(), ProxyProtocol::Socks5);
        let http = env_proxy_from(env(&[
            ("ALL_PROXY", "bad host:x"),
            ("http_proxy", "http://proxy.corp:3128"),
        ]))
        .expect("http env proxy");
        assert_eq!(http.protocol(), ProxyProtocol::Http);
        assert_eq!(http.host(), "proxy.corp");
        assert!(env_proxy_from(env(&[("HTTPS_PROXY", "bad host:x")])).is_none());
        assert!(env_proxy_from(env(&[])).is_none());
    }

    #[test]
    fn env_proxy_keeps_credentials_and_no_proxy() {
        let chosen = env_proxy_from(env(&[
            ("HTTPS_PROXY", "http://user:pa%40ss@proxy.corp:3128"),
            ("NO_PROXY", "localhost, .internal"),
        ]))
        .expect("env proxy");
        assert_eq!(chosen.host(), "proxy.corp");
        assert_eq!(chosen.port(), 3128);
        assert_eq!(chosen.username(), Some("user"));
        assert_eq!(chosen.password(), Some("pa%40ss"));
        let uri = |s: &str| s.parse::<Uri>().expect("uri");
        assert!(chosen.is_no_proxy(&uri("http://localhost/")));
        assert!(chosen.is_no_proxy(&uri("https://api.internal/")));
        assert!(!chosen.is_no_proxy(&uri("https://api.anthropic.com/")));
    }

    #[test]
    fn socks_system_proxy_falls_back_to_direct() {
        let client = Client::build(
            USER_AGENT,
            None,
            Some(SystemProxy {
                url: "socks5://127.0.0.1:1080".into(),
                bypass: Vec::new(),
            }),
        )
        .expect("client");
        assert_eq!(client.system_proxy(), None);
    }
}

#[cfg(test)]
pub(crate) mod test_server {
    //! A one-shot HTTP/1.1 server on loopback that returns a canned reply and
    //! hands the raw request back to the test.

    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::thread;

    pub struct Served {
        /// `http://127.0.0.1:<port>`.
        pub base: String,
        /// Raw request head (request line plus headers), then the body.
        pub request: mpsc::Receiver<String>,
    }

    /// Serve one request with `status`, `headers` and `body`.
    pub fn serve_once(status: &str, headers: &[(&str, &str)], body: Vec<u8>) -> Served {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let port = listener.local_addr().expect("local addr").port();
        let mut head = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\n", body.len());
        for (k, v) in headers {
            head.push_str(&format!("{k}: {v}\r\n"));
        }
        head.push_str("Connection: close\r\n\r\n");
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let Ok((stream, _)) = listener.accept() else {
                return;
            };
            let mut reader = BufReader::new(stream);
            let mut request = String::new();
            let mut content_length = 0usize;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 {
                    break;
                }
                if let Some((k, v)) = line.split_once(':') {
                    if k.eq_ignore_ascii_case("content-length") {
                        content_length = v.trim().parse().unwrap_or(0);
                    }
                }
                request.push_str(&line);
                if line == "\r\n" {
                    break;
                }
            }
            let mut request_body = vec![0u8; content_length];
            let _ = reader.read_exact(&mut request_body);
            request.push_str(&String::from_utf8_lossy(&request_body));
            let mut stream = reader.into_inner();
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&body);
            let _ = tx.send(request);
        });
        Served {
            base: format!("http://127.0.0.1:{port}"),
            request: rx,
        }
    }
}
