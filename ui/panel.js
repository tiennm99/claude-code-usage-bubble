// The details panel page: draws the payload Rust sends. Rust formats every
// displayed string and picks every colour; this file only places them.
// The panel takes no input, so the webview's own shortcuts are suppressed.

import { isBrowserAccelerator, panelFillWidth } from './panel-view.js';
import { forwardErrors, invoke, listen, logFailure } from './tauri-bridge.js';

/**
 * The `panel:update` payload and `get_panel_state` result.
 * @typedef {object} PanelPayload
 * @property {string} provider Provider slug.
 * @property {string} label Header text.
 * @property {boolean} isDark Dark palette.
 * @property {string} accent Stripe colour.
 * @property {number} sessionPct Session usage percent.
 * @property {number} weeklyPct Weekly usage percent.
 * @property {string} sessionText Session row text.
 * @property {string} weeklyText Weekly row text.
 * @property {string} sessionFill Session bar colour.
 * @property {string} weeklyFill Weekly bar colour.
 */

/**
 * The strings this page reads from `get_strings`.
 * @typedef {object} PanelStrings
 * @property {string} sessionWindow Session row label.
 * @property {string} weeklyWindow Weekly row label.
 */

/**
 * Look up a required element.
 * @param {string} id Element id.
 * @returns {HTMLElement} The element.
 */
function byId(id) {
  const element = document.getElementById(id);
  if (!element) {
    throw new Error(`panel.html is missing #${id}`);
  }
  return element;
}

const stripe = byId('stripe');
const header = byId('header');
const sessionLabel = byId('session-label');
const sessionFill = byId('session-fill');
const sessionText = byId('session-text');
const weeklyLabel = byId('weekly-label');
const weeklyFill = byId('weekly-fill');
const weeklyText = byId('weekly-text');

/** Set once a pushed payload arrived, so a late initial state cannot win. */
let updated = false;

/**
 * Switch the palette.
 * @param {boolean} isDark Dark palette.
 * @returns {void}
 */
function setTheme(isDark) {
  document.documentElement.dataset.theme = isDark ? 'dark' : 'light';
}

/**
 * Draw one bar.
 * @param {HTMLElement} fill Fill element.
 * @param {number} pct Usage percent.
 * @param {string} color Fill colour.
 * @returns {void}
 */
function drawBar(fill, pct, color) {
  fill.style.width = `${panelFillWidth(pct)}px`;
  fill.style.background = color;
}

/**
 * Draw a payload.
 * @param {PanelPayload} payload Panel state.
 * @returns {void}
 */
function render(payload) {
  setTheme(payload.isDark);
  stripe.style.background = payload.accent;
  header.textContent = payload.label;
  drawBar(sessionFill, payload.sessionPct, payload.sessionFill);
  drawBar(weeklyFill, payload.weeklyPct, payload.weeklyFill);
  sessionText.textContent = payload.sessionText;
  weeklyText.textContent = payload.weeklyText;
}

forwardErrors();

window.addEventListener('contextmenu', (event) => event.preventDefault());
window.addEventListener(
  'wheel',
  (event) => {
    if (event.ctrlKey) {
      event.preventDefault();
    }
  },
  { passive: false },
);
window.addEventListener('keydown', (event) => {
  if (isBrowserAccelerator(event)) {
    event.preventDefault();
  }
});

listen('panel:update', (/** @type {PanelPayload} */ payload) => {
  updated = true;
  render(payload);
}).catch(logFailure('listen panel:update'));

listen('theme', (/** @type {{isDark: boolean}} */ payload) => {
  setTheme(payload.isDark);
}).catch(logFailure('listen theme'));

invoke('get_strings')
  .then((strings) => {
    const { sessionWindow, weeklyWindow } = /** @type {PanelStrings} */ (strings);
    sessionLabel.textContent = sessionWindow;
    weeklyLabel.textContent = weeklyWindow;
  })
  .catch(logFailure('get_strings'));

invoke('get_panel_state')
  .then((state) => {
    if (state && !updated) {
      render(/** @type {PanelPayload} */ (state));
    }
  })
  .catch(logFailure('get_panel_state'));
