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

/// The static system proxy that applies when no environment proxy is set.
///
/// Environment proxies (`HTTPS_PROXY`, `ALL_PROXY`, `NO_PROXY`, …) are read by
/// `ureq` itself and win over the system proxy. On Windows the fallback is the
/// `Internet Settings` proxy; other platforms have none.
pub fn system_proxy() -> Option<SystemProxy> {
    if Proxy::try_from_env().is_some() {
        return None;
    }
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
        Self::build(user_agent, Proxy::try_from_env(), system_proxy())
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
                // Without ureq's `socks-proxy` feature a manually configured
                // SOCKS proxy panics at connect time, so only CONNECT proxies
                // are accepted here.
                Ok(p) if !matches!(p.protocol(), ProxyProtocol::Http | ProxyProtocol::Https) => {
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
