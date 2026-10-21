// Thin typed wrappers over `window.__TAURI__` (`withGlobalTauri`), shared by
// the bubble and panel pages.

/**
 * The slice of a Tauri webview window this UI uses.
 * @typedef {object} TauriWebviewWindow
 * @property {string} label Window label.
 * @property {(event: string, handler: (e: {payload: unknown}) => void) => Promise<() => void>} listen
 *   Listen for an event targeted at this window.
 * @property {() => Promise<void>} startDragging Start an OS window drag.
 */

/**
 * The slice of `window.__TAURI__` (`@tauri-apps/api`, which this
 * bundler-free UI does not import) that this UI uses.
 * @typedef {object} TauriGlobal
 * @property {{invoke: (command: string, args?: Record<string, unknown>) => Promise<unknown>}} core
 *   Command IPC.
 * @property {{getCurrentWebviewWindow: () => TauriWebviewWindow}} webviewWindow
 *   Window access.
 */

/** @type {TauriGlobal} */
const tauri = window.__TAURI__;

/** The webview window this page runs in. */
const webview = tauri.webviewWindow.getCurrentWebviewWindow();

/**
 * Label of this page's window, e.g. `bubble-claude`.
 * @type {string}
 */
export const windowLabel = webview.label;

/**
 * Call a Rust command.
 * @template T
 * @param {string} command Command name.
 * @param {Record<string, unknown>} [args] Command arguments (camelCase).
 * @returns {Promise<T>} The command's result.
 */
export function invoke(command, args = {}) {
  return /** @type {Promise<T>} */ (tauri.core.invoke(command, args));
}

/**
 * Listen for an event Rust sends to this window.
 * @template T
 * @param {string} event Event name.
 * @param {(payload: T) => void} handler Called with each event's payload.
 * @returns {Promise<() => void>} Resolves to a function that stops listening.
 */
export function listen(event, handler) {
  return webview.listen(event, (e) => handler(/** @type {T} */ (e.payload)));
}

/**
 * Start an OS window drag. Call it while the primary button is down.
 * @returns {Promise<void>} Resolves once the drag was requested.
 */
export function startDragging() {
  return webview.startDragging();
}

/**
 * Forward a message to the diagnostic log (written only with `--diagnose`).
 * @param {'error' | 'warn' | 'info' | 'debug'} level Log level.
 * @param {string} message Text to log.
 * @returns {void}
 */
export function logJs(level, message) {
  invoke('log_js', { level, message }).catch(() => {});
}

/**
 * Turn a rejected promise into an error log line.
 * @param {string} what What failed, for the log.
 * @returns {(err: unknown) => void} A `catch` handler.
 */
export function logFailure(what) {
  return (err) => logJs('error', `${what} failed: ${String(err)}`);
}

/**
 * Forward uncaught errors and unhandled rejections to the diagnostic log.
 * @returns {void}
 */
export function forwardErrors() {
  window.addEventListener('error', (event) => {
    logJs('error', `${event.message} at ${event.filename}:${event.lineno}`);
  });
  window.addEventListener('unhandledrejection', (event) => {
    logJs('error', `unhandled rejection: ${String(event.reason)}`);
  });
}
