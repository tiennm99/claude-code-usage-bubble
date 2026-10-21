// Test view: renders the bubble payload as raw JSON until the real bubble
// UI replaces this page.

const tauri = window.__TAURI__;
const view = /** @type {HTMLElement} */ (document.getElementById('payload'));
const webview = tauri.webviewWindow.getCurrentWebviewWindow();
const provider = webview.label.replace(/^bubble-/, '');

/**
 * Forward a message to the diagnostic log.
 * @param {'error' | 'warn' | 'info' | 'debug'} level Log level.
 * @param {string} message Text to log.
 * @returns {void}
 */
function logJs(level, message) {
  tauri.core.invoke('log_js', { level, message }).catch(() => {});
}

/**
 * Show one payload.
 * @param {unknown} payload The bubble payload from Rust.
 * @returns {void}
 */
function render(payload) {
  view.textContent = JSON.stringify(payload, null, 2);
}

window.addEventListener('error', (event) => {
  logJs('error', `${event.message} at ${event.filename}:${event.lineno}`);
});
window.addEventListener('unhandledrejection', (event) => {
  logJs('error', `unhandled rejection: ${String(event.reason)}`);
});

window.addEventListener('contextmenu', (event) => {
  event.preventDefault();
  tauri.core
    .invoke('show_context_menu', { label: webview.label })
    .catch((err) => logJs('error', `show_context_menu failed: ${String(err)}`));
});

webview
  .listen('bubble:update', (event) => render(event.payload))
  .catch((err) => logJs('error', `listen failed: ${String(err)}`));

tauri.core
  .invoke('get_bubble_state', { provider })
  .then((payload) => {
    render(payload);
    return tauri.core.invoke('bubble_ready', { label: webview.label });
  })
  .catch((err) => {
    view.textContent = `get_bubble_state failed: ${String(err)}`;
    logJs('error', `startup failed: ${String(err)}`);
  });
