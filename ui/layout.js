// Pure bubble geometry and colour helpers. Every length is in physical
// pixels, driven by the canvas size and DPI that Rust sends in the payload,
// never by `innerWidth * devicePixelRatio`. Integer steps truncate toward
// zero, like the Rust `i32` arithmetic they mirror.

/** The DPI the OS reports for 100% scaling. */
export const BASE_DPI = 96;

/** Width the head label may not exceed; wider labels fall back to this. */
export const HEAD_LABEL_FALLBACK = '5H';

/** Widest countdown text the tail reserves room for. */
const COUNTDOWN_TEMPLATE = '999d';

/** Widest percent text the tail reserves room for. */
const PCT_TEMPLATE = '100%';

/** Usage at or above this percent pulses. */
export const PULSE_THRESHOLD_PCT = 95;

/** Pulse period in ticks and the share of the way to white at the peak. */
const PULSE_PERIOD = 24;
const PULSE_STRENGTH = 0.3;

/**
 * @typedef {object} Canvas
 * @property {number} canvasW Window width in physical px.
 * @property {number} canvasH Window height in physical px.
 * @property {number} dpi Window DPI (96 at 100% scaling).
 */

/**
 * Rectangle with exclusive right and bottom edges, like Win32 `RECT`.
 * @typedef {object} Rect
 * @property {number} l Left edge.
 * @property {number} t Top edge.
 * @property {number} r Right edge.
 * @property {number} b Bottom edge.
 */

/**
 * @typedef {object} FontSizes
 * @property {number} big Head percent, bold.
 * @property {number} small Head label and tail percent, semibold.
 * @property {number} main Tail countdown, regular.
 */

/**
 * @typedef {object} HeadMetrics
 * @property {number} ringCx Ring centre x.
 * @property {number} ringCy Ring centre y.
 * @property {number} ringRadius Centre-line radius of the usage ring.
 * @property {number} ringStroke Usage ring stroke width.
 * @property {number} timeRingRadius Centre-line radius of the time ring.
 * @property {number} timeRingStroke Time ring stroke width.
 * @property {Rect} labelRect Head label (countdown or "5H").
 * @property {Rect} pctRect Head percent.
 * @property {FontSizes} fonts Font sizes in physical px.
 */

/**
 * @typedef {object} TailLayout
 * @property {Rect} usagePctRect Weekly percent text.
 * @property {Rect} usageBarRect Weekly usage bar.
 * @property {Rect} timeTextRect Weekly countdown text.
 * @property {Rect} timeBarRect Weekly remaining-time bar.
 */

/**
 * Measures `text` set at `fontPx` (regular weight) in physical px.
 * @callback MeasureText
 * @param {string} text Text to measure.
 * @param {number} fontPx Font size in physical px.
 * @returns {number} Width in physical px.
 */

/**
 * @typedef {object} RingDash
 * @property {string} dashArray Value for `stroke-dasharray`.
 * @property {number} dashOffset Value for `stroke-dashoffset`.
 */

/**
 * Integer division truncating toward zero, like Rust `i32 / i32`.
 * @param {number} a Dividend.
 * @param {number} b Divisor.
 * @returns {number} Truncated quotient.
 */
function idiv(a, b) {
  return Math.trunc(a / b);
}

/**
 * Clamp `v` into `[lo, hi]`.
 * @param {number} v Value.
 * @param {number} lo Lower bound.
 * @param {number} hi Upper bound.
 * @returns {number} Clamped value.
 */
function clamp(v, lo, hi) {
  return Math.min(Math.max(v, lo), hi);
}

/**
 * Scale a logical (96-DPI) length to `dpi`, truncating.
 * @param {number} logical Length at 96 DPI.
 * @param {number} dpi Target DPI.
 * @returns {number} Physical length.
 */
export function scale(logical, dpi) {
  return idiv(logical * dpi, BASE_DPI);
}

/**
 * Convert a physical length to CSS px for a window at `dpi`.
 * @param {number} physical Length in physical px.
 * @param {number} dpi Window DPI.
 * @returns {number} Length in CSS px.
 */
export function cssPx(physical, dpi) {
  return (physical * BASE_DPI) / dpi;
}

/**
 * Font sizes for a canvas.
 * @param {Canvas} canvas Window canvas.
 * @returns {FontSizes} Sizes in physical px.
 */
export function fontSizes(canvas) {
  const { canvasH: h, dpi } = canvas;
  const big = Math.max(idiv(h * 24, 100), scale(11, dpi));
  const small = Math.max(idiv(big * 55, 100), scale(9, dpi));
  return { big, small, main: small };
}

/**
 * Geometry of the head: the usage and time rings and the two head texts.
 * The head is the left `canvasH × canvasH` square of the stadium.
 * @param {Canvas} canvas Window canvas.
 * @returns {HeadMetrics} Head geometry in physical px.
 */
export function headMetrics(canvas) {
  const { canvasH: h, dpi } = canvas;
  const headD = h;
  const headPad = scale(5, dpi);
  const ringStroke = clamp(scale(3, dpi), 2, 4);
  const ringOuter = headD / 2 - headPad;
  const ringRadius = ringOuter - ringStroke / 2;
  const timeRingStroke = clamp(scale(2, dpi), 2, 3);
  const timeRingRadius = Math.max(ringRadius - ringStroke - scale(4, dpi), timeRingStroke);

  const fonts = fontSizes(canvas);
  const labelH = fonts.small + scale(2, dpi);
  const pctH = fonts.big + scale(2, dpi);
  const gap = Math.max(idiv(fonts.big * 15, 100), scale(2, dpi));
  const total = labelH + gap + pctH;
  const top = idiv(h - total, 2);
  const inset = scale(4, dpi);
  return {
    ringCx: headD / 2,
    ringCy: h / 2,
    ringRadius,
    ringStroke,
    timeRingRadius,
    timeRingStroke,
    labelRect: { l: inset, t: top, r: headD - inset, b: top + labelH },
    pctRect: { l: inset, t: top + labelH + gap, r: headD - inset, b: top + total },
    fonts,
  };
}

/**
 * Geometry of the tail: the weekly usage bar and remaining-time bar with
 * their right-aligned texts. When the tail is too narrow, the text column
 * shrinks (possibly to zero) before the bars drop below their minimum.
 * @param {Canvas} canvas Window canvas.
 * @param {MeasureText} measure Regular-weight text measurement.
 * @returns {TailLayout} Tail geometry in physical px.
 */
export function tailLayout(canvas, measure) {
  const { canvasW: w, canvasH: h, dpi } = canvas;
  const fonts = fontSizes(canvas);
  const tailLeft = h;
  const contentRight = w - scale(14, dpi);
  const contentLeft = tailLeft + scale(6, dpi);
  const barTextGap = scale(8, dpi);

  const countdownW = Math.ceil(measure(COUNTDOWN_TEMPLATE, fonts.main));
  const pctReserveW = Math.ceil(measure(PCT_TEMPLATE, fonts.small)) + scale(2, dpi);

  const usageBarH = clamp(idiv(h * 10, 100), scale(6, dpi), scale(12, dpi));
  const timeBarH = clamp(idiv(h * 4, 100), scale(3, dpi), scale(6, dpi));
  const laneGap = scale(6, dpi);
  const usageBarTop = idiv(h - (usageBarH + laneGap + timeBarH), 2);
  const timeBarTop = usageBarTop + usageBarH + laneGap;
  const timeTextH = fonts.main + scale(2, dpi);
  const usagePctH = fonts.small + scale(2, dpi);

  const contentW = Math.max(contentRight - contentLeft, 0);
  const barMin = scale(8, dpi);
  const desiredTextW = Math.max(countdownW, pctReserveW);
  const textW =
    contentW >= desiredTextW + barTextGap + barMin
      ? desiredTextW
      : Math.max(contentW - barTextGap - barMin, 0);
  const textLeft = contentRight - textW;
  const barLeft = contentLeft;
  const barRight = Math.max(textLeft - barTextGap, barLeft + barMin);

  const usagePctTop = usageBarTop + idiv(usageBarH - usagePctH, 2);
  const timeTextTop = timeBarTop + idiv(timeBarH - timeTextH, 2);
  return {
    usagePctRect: { l: textLeft, t: usagePctTop, r: contentRight, b: usagePctTop + usagePctH },
    usageBarRect: { l: barLeft, t: usageBarTop, r: barRight, b: usageBarTop + usageBarH },
    timeTextRect: { l: textLeft, t: timeTextTop, r: contentRight, b: timeTextTop + timeTextH },
    timeBarRect: { l: barLeft, t: timeBarTop, r: barRight, b: timeBarTop + timeBarH },
  };
}

/**
 * Dash pattern for an arc on an SVG circle rotated -90° (so the path
 * starts at 12 o'clock and runs clockwise). The usage arc starts at
 * 12 o'clock; the remaining-time arc ends there, so the consumed wedge
 * grows clockwise from 12 like a clock hand.
 * @param {number} radius Circle radius.
 * @param {number} fraction Share of the full turn, clamped to [0, 1].
 * @param {'from-top' | 'to-top'} anchor Whether the arc starts or ends at 12 o'clock.
 * @returns {RingDash | null} The dash values, or null when nothing is drawn.
 */
export function ringDash(radius, fraction, anchor) {
  const f = clamp(fraction, 0, 1);
  if (!(f > 0)) {
    return null;
  }
  const circumference = 2 * Math.PI * radius;
  const length = f * circumference;
  return {
    dashArray: `${length} ${circumference}`,
    dashOffset: anchor === 'to-top' ? -(circumference - length) : 0,
  };
}

/**
 * Share of a usage window still left before it resets.
 * @param {number | null} resetsAtMs Reset time in Unix ms, or null.
 * @param {number} durationSecs Window length in seconds.
 * @param {number} nowMs Current time in Unix ms.
 * @returns {number | null} Fraction in [0, 1], or null without a reset time.
 */
export function remainingFraction(resetsAtMs, durationSecs, nowMs) {
  if (resetsAtMs === null || resetsAtMs === undefined || !(durationSecs > 0)) {
    return null;
  }
  const remainingSecs = Math.max(resetsAtMs - nowMs, 0) / 1000;
  return clamp(remainingSecs / durationSecs, 0, 1);
}

/**
 * Pulse triangle wave: 0 at phase 0, 1 at phase 12, period 24.
 * @param {number} phase Tick counter (a non-negative integer).
 * @returns {number} Brightening amount in [0, 1].
 */
export function pulseT(phase) {
  return 1 - Math.abs((phase % PULSE_PERIOD) - PULSE_PERIOD / 2) / (PULSE_PERIOD / 2);
}

/**
 * Whether either percent is in the pulsing alarm band.
 * @param {number | null} sessionPct Session percent.
 * @param {number | null} weeklyPct Weekly percent.
 * @returns {boolean} True while the pulse timer should run.
 */
export function pulseActive(sessionPct, weeklyPct) {
  return (
    (sessionPct !== null && sessionPct >= PULSE_THRESHOLD_PCT) ||
    (weeklyPct !== null && weeklyPct >= PULSE_THRESHOLD_PCT)
  );
}

/**
 * Move a `#RRGGBB` colour toward white by up to 30%.
 * @param {string} hex Colour as `#RRGGBB`.
 * @param {number} t Pulse amount in [0, 1].
 * @returns {string} The brightened colour as upper-case `#RRGGBB`; the input
 *   unchanged when it is not a `#RRGGBB` colour.
 */
export function brighten(hex, t) {
  const match = /^#([0-9a-f]{6})$/i.exec(hex.trim());
  if (!match) {
    return hex;
  }
  const k = clamp(t, 0, 1) * PULSE_STRENGTH;
  const value = parseInt(match[1], 16);
  const channels = [value >> 16, (value >> 8) & 0xff, value & 0xff].map((c) =>
    Math.round(c + (255 - c) * k),
  );
  return `#${channels.map((c) => c.toString(16).padStart(2, '0')).join('').toUpperCase()}`;
}

/**
 * Width of the weekly usage fill. A small non-zero value is floored at one
 * bar height, so 1% still shows as a dot.
 * @param {number} barW Bar width.
 * @param {number} barH Bar height.
 * @param {number | null} pct Weekly percent, or null.
 * @returns {number} Fill width; 0 draws nothing.
 */
export function usageFillWidth(barW, barH, pct) {
  if (pct === null || !(barW > 0) || !(barH > 0)) {
    return 0;
  }
  const fill = barW * (clamp(pct, 0, 100) / 100);
  if (!(fill > 0)) {
    return 0;
  }
  const clipped = Math.min(fill, barW);
  return clipped < barH ? Math.min(barH, barW) : clipped;
}

/**
 * Width of the remaining-time fill, anchored on the bar's right edge.
 * @param {number} barW Bar width.
 * @param {number | null} fraction Remaining fraction, or null.
 * @returns {number} Fill width; 0 draws nothing.
 */
export function timeFillWidth(barW, fraction) {
  if (fraction === null || !(barW > 0)) {
    return 0;
  }
  return Math.max(Math.min(barW * fraction, barW), 0);
}

/**
 * The head label to draw: the countdown, or "5H" when it is empty or wider
 * than its rect.
 * @param {string} label Countdown from the payload.
 * @param {number} labelW Its measured width in physical px.
 * @param {number} rectW Width of the head label rect.
 * @returns {string} Text to draw.
 */
export function headLabelText(label, labelW, rectW) {
  return label === '' || labelW > rectW ? HEAD_LABEL_FALLBACK : label;
}
