// Stage everything the replay viewer needs under `web/public/data/`, which is gitignored.
//
//   node web/tools/pack-replays.mjs [--limit N] [--no-copy]
//
// Three products, all optional at runtime — the page states in a badge which ones it got
// and never silently substitutes a made-up value for a missing one:
//
//   data/replays/*.rcx        copies of `ron-data/replays/**` so the page can fetch one
//                             without a bespoke server route (`serve.mjs` serves `public/`)
//   data/replays.json         the index, with the header fields decoded here so the picker
//                             can show version / players / map before fetching 1 MB
//   data/replaymeta.json      `<CATEGORIES>` label lists and the `<TRIBES>` nation order
//                             from `ron-data/rules.xml`, plus `schema/replay-validation.json`
//
// `ron-data/` and `schema/live/` are gitignored as copyrighted game content; so is
// everything this writes. Nothing here invents a label: if rules.xml does not name a
// category value, the page shows the raw byte.

import { readFile, readdir, writeFile, mkdir, copyFile, stat } from 'node:fs/promises';
import { join, dirname, basename } from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = join(HERE, '..', '..');
const OUT = join(HERE, '..', 'public', 'data');
const { decodeReplay } = await import(join(HERE, '..', 'public', 'js', 'rcx.js'));

const args = process.argv.slice(2);
const limit = args.includes('--limit') ? Number(args[args.indexOf('--limit') + 1]) : Infinity;
const noCopy = args.includes('--no-copy');

// ---------------------------------------------------------------------------
// rules.xml: category label lists and the nation order
// ---------------------------------------------------------------------------

function packRules(xml) {
  const cats = {};
  const re = /<CATEGORIES id="([^"]+)"([^>]*)>([\s\S]*?)<\/CATEGORIES>/g;
  let m;
  while ((m = re.exec(xml))) {
    const [, id, attrs, body] = m;
    const title = /title="([^"]*)"/.exec(attrs)?.[1] ?? null;
    const items = [];
    const cre = /<CATEGORY\b([^>]*?)(?:\/>|>([\s\S]*?)<\/CATEGORY>)/g;
    let c;
    while ((c = cre.exec(body))) {
      const a = c[1], inner = c[2] || '';
      const name = /name="([^"]*)"/.exec(a)?.[1] ?? null;
      const key = /key="([^"]*)"/.exec(a)?.[1] ?? null;
      const data = /<DATA>([\s\S]*?)<\/DATA>/.exec(inner)?.[1]?.trim() ?? null;
      // Names carry a `#ICONnn` display prefix in the map/style lists; strip it for
      // display but keep the raw string so nothing is silently lost.
      const label = (key ?? name ?? '').trim() || (name ?? '').replace(/^#ICON\d+/, '').trim();
      items.push({ label, name, key, data });
    }
    cats[id] = { title, items };
  }
  // <TRIBES> is a flat list of <TRIBE><FILE>..</FILE><KEY>..</KEY></TRIBE>; the index in
  // this list is the value of `Player::tribe` in the replay header.
  const tribes = [];
  const tb = /<TRIBES>([\s\S]*?)<\/TRIBES>/.exec(xml);
  if (tb) {
    const tre = /<TRIBE>[\s\S]*?<KEY>([\s\S]*?)<\/KEY>[\s\S]*?<\/TRIBE>/g;
    let t;
    while ((t = tre.exec(tb[1]))) tribes.push(t[1].trim());
  }
  return { categories: cats, tribes };
}

await mkdir(OUT, { recursive: true });

let meta = { source: 'ron-data/rules.xml', categories: {}, tribes: [], validation: null };
try {
  const xml = await readFile(join(REPO, 'ron-data', 'rules.xml'), 'utf8');
  Object.assign(meta, packRules(xml));
  console.log(`rules.xml: ${Object.keys(meta.categories).length} category lists, ${meta.tribes.length} tribes`);
} catch (e) {
  console.warn(`rules.xml unavailable (${e.code || e.message}) — the page will show raw setting bytes`);
}
try {
  meta.validation = JSON.parse(await readFile(join(REPO, 'schema', 'replay-validation.json'), 'utf8'));
  console.log(`replay-validation.json: ${meta.validation.files?.length ?? 0} files of divergence data`);
} catch (e) {
  console.warn(`schema/replay-validation.json unavailable (${e.code || e.message}) — no divergence strip`);
}
await writeFile(join(OUT, 'replaymeta.json'), JSON.stringify(meta));

// ---------------------------------------------------------------------------
// the replays themselves
// ---------------------------------------------------------------------------

const files = [];
for (const dir of ['ron-data/replays', 'ron-data/replays/multi']) {
  let ents;
  try { ents = await readdir(join(REPO, dir)); } catch { continue; }
  for (const e of ents) if (e.toLowerCase().endsWith('.rcx')) files.push(join(dir, e));
}
files.sort();

await mkdir(join(OUT, 'replays'), { recursive: true });
const index = [];
let n = 0;
for (const rel of files) {
  if (n >= limit) break;
  const src = join(REPO, rel);
  const flat = rel.replace(/^ron-data\/replays\//, '').replace(/[\/\\]/g, '__');
  const raw = new Uint8Array(await readFile(src));
  let entry = { file: flat, source: rel, bytes: raw.length };
  try {
    const r = await decodeReplay(raw, { name: flat });
    const used = r.header.players.filter((p) => p.used);
    entry = {
      ...entry,
      version: r.version,
      seed: r.header.seed,
      settings: r.header.settings,
      mapName: r.header.mapName ?? null,
      slots: used.map((p) => ({ slot: p.slot, name: p.name, tribe: p.tribe, team: p.team, diff: p.diff })),
      packages: r.packages,
      decoded: r.packagesDecoded,
      turns: r.turns.length,
      commands: r.commandCount,
      commandStream: r.commandCount > 0,
      checksumPackets: r.checksumPackets,
      players: r.players,
      framesPerTurn: r.framesPerTurn,
      frameLast: r.frameLast,
      obfuscationSource: r.obfuscationSource,
      multiplayer: r.obfuscationSource !== 'plain' || r.checksumPackets > 0,
    };
  } catch (e) {
    entry.error = String(e.message || e);
  }
  if (!noCopy) await copyFile(src, join(OUT, 'replays', flat));
  index.push(entry);
  n++;
  process.stdout.write(`\r${n}/${Math.min(files.length, limit)} ${flat.slice(0, 50)}`.padEnd(78));
}
process.stdout.write('\n');

await writeFile(join(OUT, 'replays.json'), JSON.stringify({
  generated_by: 'web/tools/pack-replays.mjs',
  count: index.length,
  replays: index,
}, null, 1));

const okc = index.filter((e) => !e.error).length;
const sz = (await stat(join(OUT, 'replays.json'))).size;
console.log(`indexed ${index.length} replays (${okc} decoded), replays.json ${sz} bytes`);
if (!noCopy) console.log(`copied .rcx into web/public/data/replays/ (gitignored)`);
