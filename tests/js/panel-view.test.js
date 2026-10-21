// Details panel helpers: bar fill widths as the GDI panel computed them and
// the browser shortcuts the panel suppresses.

import assert from 'node:assert/strict';
import { describe, it } from 'node:test';

import { isBrowserAccelerator, panelFillWidth } from '../../ui/panel-view.js';

/**
 * A key event with no modifiers unless given.
 * @param {string} key Key value.
 * @param {{ctrlKey?: boolean, metaKey?: boolean}} [mods] Modifiers.
 * @returns {import('../../ui/panel-view.js').KeyLike} Key event stand-in.
 */
const key = (key, mods = {}) => ({ key, ctrlKey: false, metaKey: false, ...mods });

describe('panelFillWidth', () => {
  it('rounds the clamped percent of the 120 px bar', () => {
    assert.equal(panelFillWidth(0), 0);
    assert.equal(panelFillWidth(50), 60);
    assert.equal(panelFillWidth(100), 120);
    assert.equal(panelFillWidth(72.5), 87);
    assert.equal(panelFillWidth(0.4), 0);
    assert.equal(panelFillWidth(0.5), 1);
  });

  it('clamps out-of-range and non-finite values', () => {
    assert.equal(panelFillWidth(-5), 0);
    assert.equal(panelFillWidth(150), 120);
    assert.equal(panelFillWidth(Number.NaN), 0);
    assert.equal(panelFillWidth(Number.POSITIVE_INFINITY), 0);
  });
});

describe('isBrowserAccelerator', () => {
  it('catches the function keys on their own', () => {
    for (const k of ['F5', 'F7', 'F12']) {
      assert.equal(isBrowserAccelerator(key(k)), true, k);
    }
    assert.equal(isBrowserAccelerator(key('F1')), false);
  });

  it('catches Ctrl or Cmd with a browser command key, in either case', () => {
    for (const k of ['r', 'R', 'p', 'f', 'g', 's', 'u', 'o', '+', '=', '-', '0']) {
      assert.equal(isBrowserAccelerator(key(k, { ctrlKey: true })), true, `Ctrl+${k}`);
      assert.equal(isBrowserAccelerator(key(k, { metaKey: true })), true, `Cmd+${k}`);
    }
  });

  it('lets other keys through', () => {
    assert.equal(isBrowserAccelerator(key('r')), false);
    assert.equal(isBrowserAccelerator(key('a', { ctrlKey: true })), false);
    assert.equal(isBrowserAccelerator(key('c', { ctrlKey: true })), false);
    assert.equal(isBrowserAccelerator(key('Escape')), false);
  });
});
