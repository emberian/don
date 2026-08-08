// Run the browser `.rcx` decoder over the whole shipped corpus, from node.
//
// The point of this file is that the page and this checker import the *same* module
// (`web/public/js/rcx.js`). A viewer whose decoder is only ever exercised by clicking
// around in a browser has no measurement behind it; this gives one, over 60-odd real
// recordings, in a few seconds.
//
//   node web/tools/rcx-check.mjs                     # table + totals
//   node web/tools/rcx-check.mjs --json out.json     # machine-readable
//   node web/tools/rcx-check.mjs ron-data/replays/today.rcx   # one file, verbose
//
// Reported per file:
//   pkgs      framed 18-byte records found by the structural chain search
//   dec       packages whose command list tiled the payload exactly
//   resid     bytes left over between the last record and EOF (must be 0)
//   cs        CheckSumsCommand (0x39) packets, and how many had a consistent total

import { readFile, readdir, writeFile } from 'node:fs/promises';
import { join, dirname, basename } from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = join(HERE, '..', '..');
const { decodeReplay, NUM_OPCODES } = await import(join(HERE, '..', 'public', 'js', 'rcx.js'));
const { COMMANDS } = await import(join(HERE, '..', 'public', 'js', 'wire.gen.js'));

async function corpus() {
  const out = [];
  for (const dir of [join(REPO, 'ron-data/replays'), join(REPO, 'ron-data/replays/multi')]) {
    let ents;
    try { ents = await readdir(dir); } catch { continue; }
    for (const e of ents) if (e.toLowerCase().endsWith('.rcx')) out.push(join(dir, e));
  }
  return out.sort();
}

const args = process.argv.slice(2);
const jsonAt = args.indexOf('--json');
const jsonOut = jsonAt >= 0 ? args[jsonAt + 1] : null;
const maxAt = args.indexOf('--max-ms');
const maxMs = maxAt >= 0 ? Number(args[maxAt + 1]) : 30000;
const optionValues = new Set([jsonAt + 1, maxAt + 1].filter((i) => i > 0));
const explicit = args.filter((a, i) => !a.startsWith('--') && !optionValues.has(i));
const defaultCorpus = explicit.length === 0;

const files = explicit.length ? explicit.map((f) => (f.startsWith('/') ? f : join(REPO, f))) : await corpus();
if (!files.length) {
  console.error('no .rcx found under ron-data/replays/ — that directory is gitignored game data');
  process.exit(2);
}

const rows = [];
const totals = {
  files: 0, failed: 0, skippedKnownNoStream: 0, packages: 0, decoded: 0, residue: 0,
  commands: 0, checksumPackets: 0, checksumTotalOk: 0, turns: 0,
  checksumShapeOk: 0, crossplayComparisons: 0, crossplayIdentical: 0,
  metadataOk: 0, commandStreams: 0, headerOnly: 0,
};
const opHist = new Int32Array(NUM_OPCODES);
const t0 = Date.now();
const KNOWN_NO_STREAM = new Set(['playback___2014.08.12_19_59_21__tue_.rcx']);

for (const f of files) {
  const raw = new Uint8Array(await readFile(f));
  let rep;
  const fileT0 = performance.now();
  try {
    rep = await decodeReplay(raw, { name: basename(f) });
  } catch (e) {
    const message = String(e.message || e);
    const known = defaultCorpus && KNOWN_NO_STREAM.has(basename(f))
      && message === 'no command-package chain found';
    rows.push({ file: basename(f), error: message, knownNoStream: known });
    totals.files++;
    if (known) totals.skippedKnownNoStream++;
    else totals.failed++;
    continue;
  }
  for (let i = 0; i < NUM_OPCODES; i++) opHist[i] += rep.opcodeHistogram[i];
  const row = {
    file: basename(f),
    version: rep.version,
    bytes: rep.compressedLen,
    payload: rep.payloadLen,
    streamStart: rep.streamStart,
    packages: rep.packages,
    decoded: rep.packagesDecoded,
    residue: rep.framingResidue,
    turns: rep.turns.length,
    commands: rep.commandCount,
    players: rep.players,
    xorKey: rep.xorKey,
    padSeed: rep.padSeed,
    obfuscationSource: rep.obfuscationSource,
    headerSeedKey: rep.headerSeedKey,
    seed: rep.header.seed,
    mapSize: rep.header.settings?.map_size,
    mapStyle: rep.header.settings?.map_style,
    gameSpeed: rep.header.settings?.game_speed,
    framesPerTurn: rep.framesPerTurn,
    checksumPackets: rep.checksumPackets,
    checksumTotalOk: rep.checksumTotalOk,
    checksumShapeOk: rep.checksumShapeOk,
    crossplay: { comparisons: rep.crossplay.comparisons, identical: rep.crossplay.identical },
    names: rep.header.players?.filter((p) => p.used).map((p) => `${p.name}#${p.tribe}`),
    warnings: rep.header.warnings,
    anomalies: rep.anomalies.length,
    metadataOk: Boolean(rep.header.ok && rep.version && rep.header.players.length === 8
      && rep.header.players.some((p) => p.used) && rep.header.headerEnd < rep.streamStart),
    decodeMs: Number((performance.now() - fileT0).toFixed(3)),
  };
  rows.push(row);
  totals.files++;
  if (row.metadataOk) totals.metadataOk++;
  if (rep.commandCount > 0) {
    totals.commandStreams++;
    totals.packages += rep.packages;
    totals.decoded += rep.packagesDecoded;
    totals.residue += rep.framingResidue;
    totals.commands += rep.commandCount;
    totals.turns += rep.turns.length;
    totals.checksumPackets += rep.checksumPackets;
    totals.checksumTotalOk += rep.checksumTotalOk;
    totals.checksumShapeOk += rep.checksumShapeOk;
    totals.crossplayComparisons += rep.crossplay.comparisons;
    totals.crossplayIdentical += rep.crossplay.identical;
  } else {
    // The 801-byte aborted 2014 capture happens to contain a two-record chain of empty
    // payloads. Keep the row visible, but do not let that structural false positive alter
    // command-stream totals or the lockstep distribution.
    totals.headerOnly++;
  }
}
const ms = Date.now() - t0;

const pad = (s, n) => String(s).padEnd(n);
const num = (s, n) => String(s).padStart(n);
console.log(pad('file', 42), num('pkgs', 7), num('tiled', 7), num('resid', 6),
  num('turns', 7), num('cmds', 8), num('cs', 7), num('key', 6), pad('source', 20));
for (const r of rows) {
  if (r.error) {
    console.log(pad(r.file.slice(0, 42), 42), ` ${r.knownNoStream ? 'KNOWN NO-STREAM' : 'ERROR'} ${r.error}`);
    continue;
  }
  console.log(
    pad(r.file.slice(0, 42), 42), num(r.packages, 7), num(r.decoded, 7),
    num(r.residue, 6), num(r.turns, 7), num(r.commands, 8), num(r.checksumPackets, 7),
    num('0x' + r.xorKey.toString(16), 6), pad(r.obfuscationSource, 20));
}
console.log();
console.log(`files ${totals.files}  failed ${totals.failed}  known no-stream ${totals.skippedKnownNoStream}  in ${ms} ms`);
console.log(`packages   ${totals.packages}   decoded ${totals.decoded} ` +
  `(${(100 * totals.decoded / Math.max(1, totals.packages)).toFixed(4)}%)`);
console.log(`command streams ${totals.commandStreams}; header-only/aborted ${totals.headerOnly}; metadata ${totals.metadataOk}/${totals.files - totals.skippedKnownNoStream} structurally sound`);
console.log(`framing residue total ${totals.residue} bytes`);
console.log(`turns ${totals.turns}  commands ${totals.commands}`);
console.log(`checksum packets ${totals.checksumPackets}, total-consistent ${totals.checksumTotalOk}, adler-shaped ${totals.checksumShapeOk}`);
console.log(`crossplay comparisons ${totals.crossplayComparisons}, identical ${totals.crossplayIdentical}`);
const frameTurns = {};
for (const r of rows) if (!r.error && r.framesPerTurn != null) {
  frameTurns[r.framesPerTurn] = (frameTurns[r.framesPerTurn] || 0) + 1;
}
console.log(`frames/turn distribution ${Object.entries(frameTurns).map(([k, v]) => `${k}:${v}`).join('  ')}`);
console.log();
console.log('opcode histogram (whole corpus):');
const hist = [...opHist].map((n, op) => [op, n]).filter((e) => e[1] > 0).sort((a, b) => b[1] - a[1]);
for (const [op, n] of hist.slice(0, 30)) {
  const s = COMMANDS[op];
  console.log(`  0x${op.toString(16).padStart(2, '0')} ${pad(s ? s.struct : '?', 28)} ${num(n, 10)}`);
}

if (jsonOut) {
  await writeFile(jsonOut, JSON.stringify({
    generated_by: 'web/tools/rcx-check.mjs',
    what: 'The browser .rcx decoder (web/public/js/rcx.js) run over ron-data/replays/.',
    elapsed_ms: ms, totals,
    opcodes: hist.map(([op, n]) => ({ op, struct: COMMANDS[op]?.struct ?? null, count: n })),
    files: rows,
  }, null, 1));
  console.log(`\nwrote ${jsonOut}`);
}

const decodedRate = totals.decoded / Math.max(1, totals.packages);
const gatesOk = !totals.failed && !totals.residue && decodedRate >= 0.99999
  && totals.checksumTotalOk === totals.checksumPackets
  && totals.checksumShapeOk === totals.checksumPackets
  && totals.crossplayIdentical === totals.crossplayComparisons
  && totals.metadataOk === totals.files - totals.skippedKnownNoStream
  && ms <= maxMs;
if (ms > maxMs) console.error(`performance gate failed: ${ms} ms > ${maxMs} ms`);
process.exit(gatesOk ? 0 : 1);
