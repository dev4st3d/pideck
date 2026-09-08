#!/usr/bin/env node
// Source/package contract checks only; not a Rust compiler or rendered UI test.
import assert from 'node:assert/strict';
import { readFileSync, readdirSync, existsSync } from 'node:fs';
import { dirname, join, resolve, relative } from 'node:path';
import { fileURLToPath } from 'node:url';
import { PI_PACKAGE, PI_VERSION, MIN_NODE_VERSION } from '../bridge/pi-contract.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const read = path => readFileSync(join(root, path), 'utf8');
function files(directory) {
  return readdirSync(directory, { withFileTypes: true }).flatMap(entry =>
    entry.isDirectory() ? files(join(directory, entry.name)) : [join(directory, entry.name)]);
}
const rustFiles = [...files(join(root, 'src')), ...files(join(root, 'tests')), join(root, 'build.rs')]
  .filter(path => path.endsWith('.rs'));
let includes = 0;
for (const path of rustFiles) {
  for (const match of readFileSync(path, 'utf8').matchAll(/include_(?:str|bytes)!\(\s*"([^"\n]+)"\s*\)/g)) {
    assert(existsSync(resolve(dirname(path), match[1])), `${relative(root, path)}: missing ${match[1]}`);
    includes++;
  }
}
const runtimeModules = readdirSync(join(root, 'bridge')).filter(name => name.endsWith('.mjs') && !name.endsWith('.test.mjs')).sort();
const embedding = read('src/services/sdk_bridge.rs');
const embeddedModules = [...embedding.matchAll(/include_bytes!\("\.\.\/\.\.\/bridge\/([^"\n]+)"\)/g)].map(match => match[1]).sort();
assert.deepEqual(embeddedModules, runtimeModules, 'Every runtime bridge module must ship in the executable');
assert.equal(Number(embedding.match(/const EMBEDDED_BRIDGE_FILES: \[\(&str, &\[u8\]\); (\d+)\]/)?.[1]), runtimeModules.length);
for (const name of runtimeModules) {
  for (const match of read(`bridge/${name}`).matchAll(/(?:from\s+|import\s*\()\s*["'](\.\/[^"']+)["']/g)) {
    assert(existsSync(resolve(root, 'bridge', match[1])), `Missing local ESM dependency: ${name} -> ${match[1]}`);
  }
}
const discovery = read('src/services/pi_process/discovery.rs');
assert(discovery.includes(`SUPPORTED_PI_VERSION: &str = "${PI_VERSION}"`));
assert(discovery.includes(`PI_PACKAGE_NAME: &str = "${PI_PACKAGE}"`));
assert(discovery.includes(`MIN_NODE_VERSION: (u64, u64, u64) = (${MIN_NODE_VERSION.split('.').join(', ')})`));
assert.match(read('Cargo.lock'), /name = "gpui"\nversion = "0\.2\.2"/);
assert.match(read('Cargo.toml'), /serde\s*=.*features\s*=\s*\["derive", "rc"\]/);
for (const name of ['drafts', 'editor', 'workspace_layout']) {
  assert(read('src/state.rs').includes(`mod ${name};`));
  assert(existsSync(join(root, 'src', 'state', `${name}.rs`)));
}
assert(!existsSync(join(root, 'src/views/composer/buffer.rs')), 'The superseded editor must not remain as a second implementation');

const theme = read('src/theme.rs');
const colors = name => Object.fromEntries([...theme.match(new RegExp(`const ${name}: Palette = Palette \\{([\\s\\S]*?)\\n\\};`))[1]
  .matchAll(/(\w+):\s*0x([0-9a-f]{8})/g)].map(([, key, hex]) => [key, Number.parseInt(hex, 16)]));
function luminance(rgba) {
  return [24, 16, 8].map(shift => ((rgba >>> shift) & 255) / 255)
    .map(value => value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4)
    .reduce((sum, value, index) => sum + value * [0.2126, 0.7152, 0.0722][index], 0);
}
function contrast(foreground, background) {
  const a = luminance(foreground), b = luminance(background);
  return (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
}
const paletteChecks = [];
for (const name of ['ORIGINAL', 'LINEN', 'GRAPHITE', 'MIDNIGHT']) {
  const palette = colors(name);
  const results = [];
  let minimumFocusRatio = Infinity;
  for (const background of ['canvas', 'floor', 'panel', 'panel_lift', 'user_message']) {
    for (const foreground of ['bone', 'bone_dim', 'ash', 'smoke']) {
      assert.equal(palette[foreground] & 255, 255, `${name}.${foreground} is not opaque`);
      const ratio = contrast(palette[foreground], palette[background]);
      assert(ratio >= 4.5, `${name}: ${foreground} on ${background} = ${ratio}`);
      results.push({ foreground, background, ratio });
    }
    const focusRatio = contrast(palette.focus, palette[background]);
    assert(focusRatio >= 3, `${name}: insufficient focus contrast`);
    minimumFocusRatio = Math.min(minimumFocusRatio, focusRatio);
  }
  for (const [foreground, background] of [['bone', 'selection'], ['on_accent', 'signal'], ['rail_muted', 'rail']]) {
    const ratio = contrast(palette[foreground], palette[background]);
    assert(ratio >= 4.5, `${name}: ${foreground} on ${background} = ${ratio}`);
    results.push({ foreground, background, ratio });
  }
  results.sort((a, b) => a.ratio - b.ratio);
  paletteChecks.push({ name, text_pairs: results.length, minimum: results[0], minimum_focus_ratio: minimumFocusRatio,
    reference_muted_on_selection_ratio: contrast(palette.ash, palette.selection) });
}
const geometry = { TITLE_H: 40, TOOLBAR_H: 60, READING_W: 840, STREAM_PAD_X: 24 };
assert.match(read('src/state/workspace_layout.rs'), /RAIL_WIDTH: f32 = 64\.0;/);
assert.match(read('src/state/workspace_layout.rs'), /NAVIGATION_WIDTH: f32 = 240\.0;/);
assert.match(theme, /RAIL_W: f32 = crate::state::workspace_layout::RAIL_WIDTH;/);
assert.match(theme, /SIDE_W: f32 = crate::state::workspace_layout::NAVIGATION_WIDTH;/);
for (const [name, value] of Object.entries(geometry)) {
  assert.match(theme, new RegExp(`pub const ${name}: f32 = ${value}\\.0;`), `Reference geometry ${name}`);
}
const manifest = JSON.parse(read('assets/fonts/sources.json'));
assert.equal(manifest.length, 4);
for (const font of manifest) {
  assert.match(font.sha256, /^[a-f0-9]{64}$/);
  assert(read('src/fonts.rs').includes(`/fonts/${font.file}`), `Unregistered font ${font.file}`);
  assert(read('build.rs').includes(`"${font.file}"`), `Font omitted from build ${font.file}`);
}
assert(read('src/views/root/render.rs').includes('.child(native_titlebar(projection, cx))'));
assert(read('src/views/root/render.rs').includes('.child(titlebar('));
assert(!/ThemeId::Midnight/.test(read('src/views/root/shell.rs')), 'Midnight must share the same toolbar layout');
assert(read('src/state/reducer.rs').includes('RuntimeIntent::ClearQueuedInputs'));
assert(read('src/services/rpc/runtime_adapter.rs').includes('RuntimeRequest::ClearQueuedInputs'));
assert(read('src/views/root.rs').includes('matches.truncate(60)'));
console.log(JSON.stringify({ scope: 'Static source/package contract and opaque palette checks. No compilation, SDK certification, or rendered UI.',
  rust_files_inspected: rustFiles.length, literal_includes_verified: includes, embedded_runtime_modules: runtimeModules,
  contract: { package: PI_PACKAGE, version: PI_VERSION, minimum_node: MIN_NODE_VERSION }, geometry, fonts: manifest.map(f => f.file), palettes: paletteChecks,
  result: 'passed' }, null, 2));
