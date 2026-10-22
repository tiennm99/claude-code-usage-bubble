// Write the Tauri updater manifest (`latest.json`) for a release.
//
// Usage: node scripts/make-latest-json.js [--windows-only] <tag> <dir>
//
// `<dir>` holds exactly the release assets, already renamed to space-free
// names. The script checks that set before it writes `<dir>/latest.json`,
// because the same files are uploaded next to the manifest:
//
// - Legacy clients (v0.5.0 and earlier) read `/releases/latest` through the
//   GitHub API and download the asset with the exe name they were built
//   under, else the first asset ending in `.exe`. No release carries their
//   old name, so exactly one `.exe` asset may exist.
// - GitHub rewrites spaces in asset names, so a name with whitespace would no
//   longer match the URL written into the manifest.
// - Every platform entry the app looks up must be present; a missing one
//   would leave those installs without updates.

import { readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';

export const REPO = 'tiennm99/usage-monitor';
export const WINDOWS_EXE = 'usage-monitor.exe';
export const MANIFEST_NAME = 'latest.json';

const TAG_PATTERN = /^v(\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?)$/;

/**
 * @typedef {object} PlatformEntry
 * @property {string} url Release-download URL of the update asset.
 * @property {string} signature Content of the asset's `.sig` file.
 */

/**
 * @typedef {object} Manifest
 * @property {string} version Version without the leading `v`.
 * @property {string} notes Short release note.
 * @property {string} pub_date RFC 3339 publication time.
 * @property {Record<string, PlatformEntry>} platforms Entries by updater target.
 */

/**
 * The version a release tag announces.
 * @param {string} tag Tag such as `v1.2.3` or `v0.0.0-test`.
 * @returns {string} The version without the leading `v`.
 * @throws {Error} When the tag is not `v<major>.<minor>.<patch>[-suffix]`.
 */
export function versionFromTag(tag) {
  const match = TAG_PATTERN.exec(tag);
  if (!match) {
    throw new Error(`tag "${tag}" is not of the form vX.Y.Z or vX.Y.Z-suffix`);
  }
  return match[1];
}

/**
 * The updater platforms a release carries, with the asset each one installs.
 * @param {string} version Release version.
 * @param {boolean} windowsOnly True for a Windows-only (legacy rollback) release.
 * @returns {Record<string, string>} Asset name by updater target.
 */
export function platformAssets(version, windowsOnly) {
  /** @type {Record<string, string>} */
  const platforms = { 'windows-x86_64': WINDOWS_EXE };
  if (!windowsOnly) {
    platforms['linux-x86_64'] = `usage-monitor_${version}_amd64.AppImage`;
  }
  return platforms;
}

/**
 * Every asset a release must upload besides `latest.json`.
 * @param {string} version Release version.
 * @param {boolean} windowsOnly True for a Windows-only release.
 * @returns {string[]} Asset names, sorted.
 */
export function expectedAssets(version, windowsOnly) {
  const names = Object.values(platformAssets(version, windowsOnly)).flatMap((n) => [n, `${n}.sig`]);
  if (!windowsOnly) {
    names.push(`usage-monitor_${version}_amd64.deb`);
  }
  return names.sort();
}

/**
 * Check a release's asset names against what every client needs.
 * @param {string[]} names Asset names, `latest.json` excluded.
 * @param {string} version Release version.
 * @param {boolean} windowsOnly True for a Windows-only release.
 * @throws {Error} On a name with whitespace, a second `.exe`, a missing asset or an unexpected one.
 */
export function checkAssetNames(names, version, windowsOnly) {
  const spaced = names.filter((n) => /\s/.test(n));
  if (spaced.length > 0) {
    throw new Error(`asset names must not contain whitespace: ${spaced.join(', ')}`);
  }
  const strayExe = names.filter((n) => n.toLowerCase().endsWith('.exe') && n !== WINDOWS_EXE);
  if (strayExe.length > 0) {
    throw new Error(
      `the only .exe asset allowed is ${WINDOWS_EXE}; legacy clients could pick: ${strayExe.join(', ')}`,
    );
  }
  const expected = expectedAssets(version, windowsOnly);
  const missing = expected.filter((n) => !names.includes(n));
  if (missing.length > 0) {
    throw new Error(`missing release assets: ${missing.join(', ')}`);
  }
  const unexpected = names.filter((n) => !expected.includes(n));
  if (unexpected.length > 0) {
    throw new Error(`unexpected release assets: ${unexpected.join(', ')}`);
  }
}

/**
 * The download URL GitHub serves a release asset from.
 * @param {string} tag Release tag.
 * @param {string} name Asset name.
 * @returns {string} The URL.
 */
export function assetUrl(tag, name) {
  return `https://github.com/${REPO}/releases/download/${encodeURIComponent(tag)}/${encodeURIComponent(name)}`;
}

/**
 * Build the manifest for a release whose asset names were already checked.
 * @param {object} input Release facts.
 * @param {string} input.tag Release tag.
 * @param {boolean} input.windowsOnly True for a Windows-only release.
 * @param {(name: string) => string} input.readSignature Content of the named `.sig` asset.
 * @param {Date} input.now Publication time.
 * @returns {Manifest} The manifest.
 * @throws {Error} When a signature is empty.
 */
export function buildManifest({ tag, windowsOnly, readSignature, now }) {
  const version = versionFromTag(tag);
  /** @type {Record<string, PlatformEntry>} */
  const platforms = {};
  for (const [target, name] of Object.entries(platformAssets(version, windowsOnly))) {
    const signature = readSignature(`${name}.sig`).trim();
    if (signature === '') {
      throw new Error(`${name}.sig is empty`);
    }
    platforms[target] = { url: assetUrl(tag, name), signature };
  }
  return {
    version,
    notes: `See https://github.com/${REPO}/releases/tag/${tag}`,
    pub_date: now.toISOString(),
    platforms,
  };
}

/**
 * Check the assets in `dir` and write `dir/latest.json`.
 * @param {string[]} argv Arguments after the script path.
 * @param {Date} [now] Publication time.
 * @returns {Manifest} The manifest written.
 * @throws {Error} On bad arguments or any failed check.
 */
export function run(argv, now = new Date()) {
  const windowsOnly = argv.includes('--windows-only');
  const positional = argv.filter((a) => a !== '--windows-only');
  if (positional.length !== 2 || positional.some((a) => a.startsWith('--'))) {
    throw new Error('usage: make-latest-json.js [--windows-only] <tag> <dir>');
  }
  const [tag, dir] = positional;
  const version = versionFromTag(tag);
  const names = readdirSync(dir).filter((n) => n !== MANIFEST_NAME);
  const notFiles = names.filter((n) => !statSync(join(dir, n)).isFile());
  if (notFiles.length > 0) {
    throw new Error(`not a file: ${notFiles.join(', ')}`);
  }
  checkAssetNames(names, version, windowsOnly);
  const manifest = buildManifest({
    tag,
    windowsOnly,
    readSignature: (name) => readFileSync(join(dir, name), 'utf8'),
    now,
  });
  writeFileSync(join(dir, MANIFEST_NAME), `${JSON.stringify(manifest, null, 2)}\n`);
  return manifest;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    const manifest = run(process.argv.slice(2));
    console.log(`latest.json ${manifest.version}: ${Object.keys(manifest.platforms).join(', ')}`);
  } catch (err) {
    console.error(`make-latest-json: ${err instanceof Error ? err.message : String(err)}`);
    process.exitCode = 1;
  }
}
