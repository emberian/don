// Fail closed when the generated playable Wasm no longer matches its JavaScript ABI.
//
// This is a static module inspection: it does not instantiate the game, launch a browser,
// or mutate the artefact.  `web/build.sh` runs it after copying the freshly built module,
// and the browser smoke runs it before opening Chrome.

import { readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  FORBIDDEN_PLAY_WASM_EXPORTS,
  REQUIRED_PLAY_WASM_EXPORTS,
} from '../public/js/play/wasm-contract.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = join(HERE, '..', '..');
const wasm = resolve(process.argv[2] ?? join(REPO, 'web', 'public', 'wasm', 'don_web.wasm'));

const module = new WebAssembly.Module(readFileSync(wasm));
const exportsByName = new Map(WebAssembly.Module.exports(module).map((entry) => [entry.name, entry.kind]));
const missing = REQUIRED_PLAY_WASM_EXPORTS.filter((name) => !exportsByName.has(name));
const forbidden = FORBIDDEN_PLAY_WASM_EXPORTS.filter((name) => exportsByName.has(name));
const wrongKind = REQUIRED_PLAY_WASM_EXPORTS.filter((name) => exportsByName.has(name) &&
  exportsByName.get(name) !== (name === 'memory' ? 'memory' : 'function'));

// Keep the explicit runtime contract complete when JavaScript starts using another raw ABI
// symbol.  This catches source drift even before a newly built module happens to export it.
const runtimeSources = [
  join(REPO, 'web', 'public', 'js', 'play', 'wasmgame.js'),
  join(REPO, 'web', 'public', 'js', 'play', 'client.js'),
];
const referenced = new Set();
for (const source of runtimeSources) {
  const text = readFileSync(source, 'utf8');
  for (const match of text.matchAll(/\bx\.(game_[A-Za-z0-9_]+|memory)\b/g)) referenced.add(match[1]);
}
const unlisted = [...referenced].filter((name) => !REQUIRED_PLAY_WASM_EXPORTS.includes(name)).sort();

if (missing.length || forbidden.length || wrongKind.length || unlisted.length) {
  if (missing.length) console.error(`missing required Wasm exports: ${missing.join(', ')}`);
  if (forbidden.length) console.error(`forbidden setup setters exported: ${forbidden.join(', ')}`);
  if (wrongKind.length) console.error(`Wasm exports have the wrong kind: ${wrongKind.join(', ')}`);
  if (unlisted.length) console.error(`runtime ABI references absent from contract: ${unlisted.join(', ')}`);
  process.exit(1);
}

console.log(`play Wasm contract current: ${REQUIRED_PLAY_WASM_EXPORTS.length} required exports, ` +
  `${FORBIDDEN_PLAY_WASM_EXPORTS.length} forbidden setup setters absent`);
