// Usage subsystem: trait + per-provider implementations + registry.
//
// Phase 2 stubs out only `types`. Phase 4 fills in `anthropic`, `chatgpt`,
// `refresh`, `registry`, and `headers`. The trait below is the contract
// every provider must satisfy.

pub mod anthropic;
pub mod chatgpt;
pub mod headers;
pub mod refresh;
pub mod registry;
pub mod types;

pub use registry::Registry;
pub use types::{ProviderId, ProviderSnapshot, UsageWindows, Window};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("authentication required")]
    AuthRequired,
    #[error("no credentials configured")]
    NoCredentials,
    #[error("token expired after refresh attempt")]
    TokenExpired,
    #[error("network: {0}")]
    Network(#[from] crate::net::Error),
    #[error("unexpected response shape: {0}")]
    BadResponse(String),
}

/// Every provider exposes a stable identity and a sync `poll` that performs
/// HTTP calls against the supplied client.
pub trait UsageProvider: Send {
    fn id(&self) -> ProviderId;
    fn poll(&mut self, http: &crate::net::Client) -> Result<UsageWindows, Error>;
}
