// Usage data shapes shared across providers.
//
// Every provider reports its quota as two named "windows" (short + long).
// For Claude: 5-hour and 7-day. For ChatGPT: primary and secondary. We
// normalise to `primary` + `secondary` so the UI layer doesn't care which
// provider produced the snapshot.

use std::time::SystemTime;

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum ProviderId {
    Claude,
    ChatGpt,
}

impl ProviderId {
    pub fn slug(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::ChatGpt => "chatgpt",
        }
    }
}

/// One usage window: how much you've consumed (0–100), and when it resets.
#[derive(Clone, Copy, Debug, Default)]
pub struct Window {
    pub utilization: f64,
    pub resets_at: Option<SystemTime>,
}

/// The pair of windows a provider reports per poll.
#[derive(Clone, Copy, Debug, Default)]
pub struct UsageWindows {
    pub primary: Window,
    pub secondary: Window,
}

/// One provider's most recent poll result, keyed by `id`.
#[derive(Clone, Debug)]
pub struct ProviderSnapshot {
    pub id: ProviderId,
    pub windows: UsageWindows,
}
