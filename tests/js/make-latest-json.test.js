// Release manifest script: the asset set every client relies on and the
// `latest.json` shape the Tauri updater reads.

import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, beforeEach, describe, it } from 'node:test';

import {
  assetUrl,
  checkAssetNames,
  expectedAssets,
  run,
  versionFromTag,
} from '../../scripts/make-latest-json.js';

const TAG = 'v0.6.0';
const VERSION = '0.6.0';
const APPIMAGE = `usage-monitor_${VERSION}_amd64.AppImage`;
const DEB = `usage-monitor_${VERSION}_amd64.deb`;
const NOW = new Date('2026-10-21T09:30:00.000Z');

/** @type {string} */
let dir;

beforeEach(() => {
  dir = mkdtempSync(join(tmpdir(), 'make-latest-json-'));
});

afterEach(() => {
  rmSync(dir, { recursive: true, force: true });
});

/**
 * Create release assets in the temp dir; a `.sig` holds a fake signature.
 * @param {string[]} names Asset names.
 */
const put = (names) => {
  for (const name of names) {
    writeFileSync(join(dir, name), name.endsWith('.sig') ? `sig-of-${name}\n` : 'bytes');
  }
};

const FULL = ['usage-monitor.exe', 'usage-monitor.exe.sig', APPIMAGE, `${APPIMAGE}.sig`, DEB];

describe('versionFromTag', () => {
  it('strips the v and keeps a pre-release suffix', () => {
    assert.equal(versionFromTag('v1.2.3'), '1.2.3');
    assert.equal(versionFromTag('v0.0.0-test'), '0.0.0-test');
  });

  it('rejects tags that are not vX.Y.Z', () => {
    for (const tag of ['1.2.3', 'v1.2', 'v1.2.3.4', 'v1.2.3-', 'release', '']) {
      assert.throws(() => versionFromTag(tag), /not of the form/, tag);
    }
  });
});

describe('run', () => {
  it('writes exactly the two platforms with the signature content', () => {
    put(FULL);
    const manifest = run([TAG, dir], NOW);
    const written = JSON.parse(readFileSync(join(dir, 'latest.json'), 'utf8'));
    assert.deepEqual(written, manifest);
    assert.deepEqual(Object.keys(written).sort(), ['notes', 'platforms', 'pub_date', 'version']);
    assert.equal(written.version, VERSION);
    assert.equal(written.pub_date, '2026-10-21T09:30:00.000Z');
    assert.deepEqual(Object.keys(written.platforms).sort(), ['linux-x86_64', 'windows-x86_64']);
    assert.equal(written.platforms['windows-x86_64'].signature, 'sig-of-usage-monitor.exe.sig');
    assert.equal(written.platforms['linux-x86_64'].signature, `sig-of-${APPIMAGE}.sig`);
  });

  it('points each URL at the renamed asset of the same name', () => {
    put(FULL);
    const { platforms } = run([TAG, dir], NOW);
    const base = 'https://github.com/tiennm99/usage-monitor/releases/download/v0.6.0/';
    assert.equal(platforms['windows-x86_64'].url, `${base}usage-monitor.exe`);
    assert.equal(platforms['linux-x86_64'].url, `${base}${APPIMAGE}`);
    for (const { url } of Object.values(platforms)) {
      assert.ok(FULL.includes(url.slice(base.length)), url);
    }
  });

  it('writes only the Windows entry with --windows-only', () => {
    put(['usage-monitor.exe', 'usage-monitor.exe.sig']);
    const manifest = run(['--windows-only', TAG, dir], NOW);
    assert.deepEqual(Object.keys(manifest.platforms), ['windows-x86_64']);
  });

  it('ignores a latest.json left by an earlier run', () => {
    put(FULL);
    run([TAG, dir], NOW);
    assert.doesNotThrow(() => run([TAG, dir], NOW));
  });

  it('fails on an asset name with a space', () => {
    put([...FULL.filter((n) => n !== APPIMAGE), 'Usage Monitor_0.6.0_amd64.AppImage']);
    assert.throws(() => run([TAG, dir], NOW), /whitespace/);
  });

  it('fails on a second .exe asset, whatever its case', () => {
    put([...FULL, 'usage-monitor-setup.exe']);
    assert.throws(() => run([TAG, dir], NOW), /only \.exe asset allowed/);
    rmSync(join(dir, 'usage-monitor-setup.exe'));
    put(['Other.EXE']);
    assert.throws(() => run([TAG, dir], NOW), /only \.exe asset allowed/);
  });

  it('fails when a platform asset or its signature is missing', () => {
    put(FULL.filter((n) => n !== `${APPIMAGE}.sig`));
    assert.throws(() => run([TAG, dir], NOW), /missing release assets: .*AppImage\.sig/);
  });

  it('fails when the AppImage carries another version', () => {
    put([...FULL.filter((n) => !n.startsWith(APPIMAGE)), 'usage-monitor_0.5.0_amd64.AppImage']);
    assert.throws(() => run([TAG, dir], NOW), /missing release assets/);
  });

  it('fails on an empty signature', () => {
    put(FULL);
    writeFileSync(join(dir, 'usage-monitor.exe.sig'), '\n');
    assert.throws(() => run([TAG, dir], NOW), /exe\.sig is empty/);
  });

  it('fails on bad arguments', () => {
    assert.throws(() => run([TAG], NOW), /usage/);
    assert.throws(() => run(['--linux-only', TAG, dir], NOW), /usage/);
    assert.throws(() => run(['0.6.0', dir], NOW), /not of the form/);
  });
});

describe('checkAssetNames', () => {
  it('rejects Linux assets on a Windows-only release', () => {
    assert.throws(
      () => checkAssetNames([...expectedAssets(VERSION, true), DEB], VERSION, true),
      /unexpected release assets/,
    );
  });

  it('lists the full asset set', () => {
    assert.deepEqual(expectedAssets(VERSION, false), [...FULL].sort());
  });
});

describe('assetUrl', () => {
  it('builds the release-download URL', () => {
    assert.equal(
      assetUrl('v0.0.0-test', 'usage-monitor.exe'),
      'https://github.com/tiennm99/usage-monitor/releases/download/v0.0.0-test/usage-monitor.exe',
    );
  });
});
