//! Commands the webviews invoke. Every argument comes from a webview, so
//! each one is validated here before it reaches the runtime.

use std::sync::Arc;

use tauri::{AppHandle, Manager, State, WebviewWindow};

use crate::core::i18n::LocaleStrings;
use crate::core::payload::BubblePayload;
use crate::runtime::{Runtime, BUBBLE_LABEL_PREFIX};
use crate::usage::ProviderId;

/// Longest JS log message kept, in characters.
const MAX_JS_LOG_CHARS: usize = 2_000;

/// The current payload for `provider`'s bubble, sized for the calling
/// window's scale factor.
#[tauri::command]
pub fn get_bubble_state(
    window: WebviewWindow,
    runtime: State<'_, Arc<Runtime>>,
    provider: String,
) -> Result<BubblePayload, String> {
    let provider = live_provider(&provider)?;
    let scale = window.scale_factor().map_err(|err| err.to_string())?;
    Ok(runtime.bubble_payload(provider, scale))
}

/// Every UI string, camelCase keys.
#[tauri::command]
pub fn get_strings(runtime: State<'_, Arc<Runtime>>) -> LocaleStrings {
    runtime.with_state(|s| s.strings.clone())
}

/// Forward a webview error or message to the diagnostic log.
#[tauri::command]
pub fn log_js(window: WebviewWindow, level: String, message: String) {
    let message = sanitize_log_message(&message);
    let label = window.label();
    match level.as_str() {
        "error" => log::error!(target: "js", "[{label}] {message}"),
        "warn" => log::warn!(target: "js", "[{label}] {message}"),
        "debug" => log::debug!(target: "js", "[{label}] {message}"),
        _ => log::info!(target: "js", "[{label}] {message}"),
    }
}

/// The bubble rendered its first state and can be shown.
#[tauri::command]
pub fn bubble_ready(app: AppHandle, label: String) -> Result<(), String> {
    if !label.starts_with(BUBBLE_LABEL_PREFIX) {
        return Err("not a bubble window".into());
    }
    let window = app
        .get_webview_window(&label)
        .ok_or_else(|| "no such bubble window".to_string())?;
    window.show().map_err(|err| err.to_string())
}

fn live_provider(slug: &str) -> Result<ProviderId, String> {
    ProviderId::from_slug(slug)
        .filter(|p| p.metadata().live_usage)
        .ok_or_else(|| "unknown provider".to_string())
}

/// One log line: control characters become spaces, so a message cannot
/// forge extra log lines, and the length is capped.
fn sanitize_log_message(message: &str) -> String {
    message
        .chars()
        .take(MAX_JS_LOG_CHARS)
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_live_providers_are_accepted() {
        assert_eq!(live_provider("claude"), Ok(ProviderId::Claude));
        assert_eq!(live_provider("chatgpt"), Ok(ProviderId::ChatGpt));
        assert!(live_provider("opencode-go").is_err());
        assert!(live_provider("").is_err());
        assert!(live_provider("CLAUDE").is_err());
    }

    #[test]
    fn log_messages_stay_on_one_capped_line() {
        assert_eq!(sanitize_log_message("a\nb\r\tc"), "a b  c");
        let long = "x".repeat(MAX_JS_LOG_CHARS + 50);
        assert_eq!(
            sanitize_log_message(&long).chars().count(),
            MAX_JS_LOG_CHARS
        );
    }
}
