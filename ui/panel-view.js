// Pure helpers for the details panel page, kept free of `window.__TAURI__`
// so they run under `node --test`.

/** Width of a panel usage bar in CSS px. */
export const PANEL_BAR_WIDTH = 120;

/**
 * Width of a panel bar fill: the clamped percent of the bar, rounded to
 * whole px, as the GDI panel drew it. The bar is square, so 1% stays 1 px.
 * @param {number} pct Usage percent; anything not finite counts as 0.
 * @returns {number} Fill width in CSS px, 0..120.
 */
export function panelFillWidth(pct) {
  const value = Number.isFinite(pct) ? Math.min(Math.max(pct, 0), 100) : 0;
  return Math.round((value / 100) * PANEL_BAR_WIDTH);
}

/** Keys that act as browser commands with Ctrl (Cmd on macOS) held. */
const CTRL_ACCELERATORS = new Set(['r', 'p', 'f', 'g', 's', 'u', 'o', '+', '=', '-', '_', '0']);

/** Keys that act as browser commands on their own. */
const PLAIN_ACCELERATORS = new Set(['F5', 'F7', 'F12']);

/**
 * The parts of a `KeyboardEvent` the accelerator check reads.
 * @typedef {object} KeyLike
 * @property {string} key Key value.
 * @property {boolean} ctrlKey Ctrl held.
 * @property {boolean} metaKey Cmd (Meta) held.
 */

/**
 * Whether a key press would run a browser command (reload, print, find,
 * save, view source, open, zoom, caret browsing, dev tools). The panel
 * ignores all input, so these are suppressed.
 * @param {KeyLike} event Key event.
 * @returns {boolean} True for a browser accelerator.
 */
export function isBrowserAccelerator(event) {
  if (PLAIN_ACCELERATORS.has(event.key)) {
    return true;
  }
  return (event.ctrlKey || event.metaKey) && CTRL_ACCELERATORS.has(event.key.toLowerCase());
}
