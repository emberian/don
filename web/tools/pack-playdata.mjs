// Pack the extra derived tables a *playable* client needs, on top of `gamedata.bin`.
//
//     node web/tools/pack-playdata.mjs [--out web/public/data]
//
// `pack-gamedata.mjs` packs what a spectator needs: unit combat stats, the balance table
// and the combat constants. A client you can actually play needs four more things, and
// every one of them exists as derived data already — none of it is invented here:
//
//   schema/live/live-tables-unit.tsv       COST[6], JOB_TIME, WHERE (the producing
//                                          building's type id), AGE, per unit type
//   schema/live/live-tables-building.tsv   COST[6], JOB_TIME, X_SIZE/Y_SIZE (the footprint,
//                                          in tiles), HITS, ARMOR, for 129 buildings
//   schema/live/live-tables-tech.tsv       the seven age techs, 544..550, with real costs
//   schema/live/rules-block-pid14644.txt   the whole live `Constants` value block, which
//                                          `don_sim::systems::economy::EconRules::from_block`
//                                          consumes by byte offset
//
// The producer -> product edge list is `unit.WHERE` joined against the building table:
// **312 edges over 13 producers**, which is the same join `crates/don-env`'s `typecaps`
// makes from the shipped XML. Two independent extractions of the same relation.
//
// # Copyright
//
// Everything under `schema/live/` and `ron-data/` is gitignored game content and so is the
// output directory. Nothing this tool writes is committed.

import { readFileSync, writeFileSync, mkdirSync, existsSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = join(HERE, '..', '..');
const OUT = process.argv.includes('--out')
  ? process.argv[process.argv.indexOf('--out') + 1]
  : join(REPO, 'web', 'public', 'data');

const MAGIC = 'DONPLAY1';

/** i32 fields per unit record — must match `PLAY_UNIT_FIELDS` in `wasm/src/game.rs`. */
const UNIT_FIELDS = [
  'type_id', 'cost0', 'cost1', 'cost2', 'cost3', 'cost4', 'cost5',
  'pop', 'job_time', 'where', 'age', 'obj_masks', 'los',
];
/** i32 fields per building record — must match `PLAY_BLD_FIELDS` in `wasm/src/game.rs`. */
const BLD_FIELDS = [
  'type_id', 'cost0', 'cost1', 'cost2', 'cost3', 'cost4', 'cost5',
  'job_time', 'x_size', 'y_size', 'hits', 'armor', 'attack', 'max_range',
  'recharge', 'age', 'obj_masks', 'los',
];
/** The seven age techs. `TypeIndex` 544..550; 543 is the last item type. */
const AGE_TECH_IDS = [544, 545, 546, 547, 548, 549, 550];
/** `don_sim::systems::economy::RULES_DWORDS`. */
const RULES_DWORDS = 848;

function readTsv(path) {
  const lines = readFileSync(path, 'utf8').split('\n').filter((l) => l.length);
  const head = lines[0].split('\t');
  return lines.slice(1).map((l) => {
    const c = l.split('\t');
    const o = {};
    for (let i = 0; i < head.length; i++) o[head[i]] = c[i];
    return o;
  });
}

/** The whole live `Constants` block as i32 dwords, zero-extended to `RULES_DWORDS`. */
function readRulesBlock(path) {
  const b64 = readFileSync(path, 'utf8').split('\n').find((l) => l.startsWith('BLK='))?.slice(4);
  if (!b64) throw new Error('no BLK= line in ' + path);
  const buf = Buffer.from(b64, 'base64');
  const out = new Int32Array(RULES_DWORDS);
  const n = Math.min(RULES_DWORDS, buf.length >> 2);
  for (let i = 0; i < n; i++) out[i] = buf.readInt32LE(i * 4);
  return { dwords: out, covered: n, bytes: buf.length };
}

/**
 * `POP` per unit type, from `ron-data/unitrules.xml`.
 *
 * The live TSV has no POP column, so this is the one field that comes from the shipped XML
 * rather than a live read. The index mapping is the one `crates/don-env/src/typecaps.rs`
 * establishes and cross-validates: `unitrules[i]` is `TypeIndex 50 + i`, 364 entries.
 * Returns a Map keyed by type id; a type absent from the XML is simply absent, and the
 * caller records how many were resolved rather than defaulting silently.
 */
function readUnitPop(path) {
  if (!existsSync(path)) return { pop: new Map(), source: null };
  const xml = readFileSync(path, 'utf8');
  const blocks = xml.split('<UNIT>').slice(1);
  const pop = new Map();
  blocks.forEach((b, i) => {
    const m = /<POP>\s*([-0-9]+)/.exec(b);
    if (m) pop.set(String(50 + i), Number(m[1]));
  });
  return { pop, source: 'ron-data/unitrules.xml (positional, unitrules[i] = TypeIndex 50+i)' };
}

function num(row, field, fallback) {
  const v = Number(row[field]);
  if (Number.isFinite(v)) return v | 0;
  if (fallback !== undefined) return fallback;
  throw new Error(`row ${row.type_id} field ${field} is ${JSON.stringify(row[field])}`);
}

function main() {
  const liveDir = join(REPO, 'schema', 'live');
  const unitTsv = join(liveDir, 'live-tables-unit.tsv');
  const bldTsv = join(liveDir, 'live-tables-building.tsv');
  const techTsv = join(liveDir, 'live-tables-tech.tsv');
  const rulesTxt = join(liveDir, 'rules-block-pid14644.txt');
  for (const p of [unitTsv, bldTsv, techTsv, rulesTxt]) {
    if (!existsSync(p)) {
      console.error(`missing input: ${p}`);
      console.error('These are gitignored game-derived artefacts. Without them the play');
      console.error('client runs with no build/train menus and says so on screen.');
      process.exit(2);
    }
  }

  const units = readTsv(unitTsv);
  const blds = readTsv(bldTsv);
  const techs = readTsv(techTsv);
  const rules = readRulesBlock(rulesTxt);
  const { pop: popById, source: popSource } = readUnitPop(join(REPO, 'ron-data', 'unitrules.xml'));

  const bldById = new Map(blds.map((b) => [b.type_id, b]));

  // ---- the producer -> product join -------------------------------------------------
  // `unit.WHERE` names the building type that trains it. Keep only edges whose producer
  // actually exists in the building table, so a stale id can never become a phantom menu.
  const edges = [];
  const unresolved = [];
  for (const u of units) {
    const w = num(u, 'where', -1);
    if (w < 0) continue;
    if (!bldById.has(String(w))) { unresolved.push([u.type_id, w]); continue; }
    edges.push([w, num(u, 'type_id')]);
  }
  const producers = new Map();
  for (const [w, t] of edges) {
    if (!producers.has(w)) producers.set(w, []);
    producers.get(w).push(t);
  }

  const ages = AGE_TECH_IDS.map((id) => {
    const t = techs.find((r) => Number(r.type_id) === id);
    if (!t) throw new Error(`age tech ${id} missing from ${techTsv}`);
    return {
      id,
      name: t.name_display,
      cost: [0, 1, 2, 3, 4, 5].map((k) => num(t, `cost${k}`, 0)),
    };
  });

  // ---- binary ------------------------------------------------------------------------
  const head = 8 + 8 * 4;
  const bytes =
    head +
    units.length * UNIT_FIELDS.length * 4 +
    blds.length * BLD_FIELDS.length * 4 +
    edges.length * 2 * 4 +
    ages.length * 8 * 4 +
    RULES_DWORDS * 4;
  const out = Buffer.alloc(bytes);
  let o = 0;
  out.write(MAGIC, o, 'latin1'); o += 8;
  for (const v of [
    units.length, UNIT_FIELDS.length, blds.length, BLD_FIELDS.length,
    edges.length, ages.length, RULES_DWORDS, 0,
  ]) { out.writeUInt32LE(v, o); o += 4; }

  let popResolved = 0;
  for (const u of units) {
    for (const f of UNIT_FIELDS) {
      let v;
      if (f === 'pop') {
        if (popById.has(u.type_id)) { v = popById.get(u.type_id); popResolved++; }
        else v = -1;  // "not resolved" — the sim treats it as 1 and the UI can say so
      } else v = num(u, f, -1);
      out.writeInt32LE(v | 0, o); o += 4;
    }
  }
  for (const b of blds) {
    for (const f of BLD_FIELDS) { out.writeInt32LE(num(b, f, -1) | 0, o); o += 4; }
  }
  for (const [w, t] of edges) { out.writeInt32LE(w, o); o += 4; out.writeInt32LE(t, o); o += 4; }
  for (const a of ages) {
    out.writeInt32LE(a.id, o); o += 4;
    for (const c of a.cost) { out.writeInt32LE(c, o); o += 4; }
    out.writeInt32LE(0, o); o += 4;
  }
  for (let i = 0; i < RULES_DWORDS; i++) { out.writeInt32LE(rules.dwords[i], o); o += 4; }
  if (o !== out.length) throw new Error(`wrote ${o} of ${out.length}`);

  mkdirSync(OUT, { recursive: true });
  writeFileSync(join(OUT, 'playdata.bin'), out);

  // The JSON half is UI-only: names, menus, provenance. Rust never sees a string.
  const unitById = Object.fromEntries(units.map((u) => [u.type_id, {
    name: u.name_display, age: num(u, 'age', 0),
    cost: [0, 1, 2, 3, 4, 5].map((k) => num(u, `cost${k}`, 0)),
    pop: popById.has(u.type_id) ? popById.get(u.type_id) : null,
    jobTime: num(u, 'job_time', 0), where: num(u, 'where', -1),
    hits: num(u, 'hits', 0), attack: num(u, 'attack', 0), armor: num(u, 'armor', 0),
    moves: num(u, 'moves', 0), range: num(u, 'max_range', 0),
    recharge: num(u, 'recharge', 0), los: num(u, 'los', 0), domain: num(u, 'domain', 0),
  }]));
  const bldById2 = Object.fromEntries(blds.map((b) => [b.type_id, {
    name: b.name_display, age: num(b, 'age', 0),
    cost: [0, 1, 2, 3, 4, 5].map((k) => num(b, `cost${k}`, 0)),
    jobTime: num(b, 'job_time', 0),
    xSize: num(b, 'x_size', 1), ySize: num(b, 'y_size', 1),
    hits: num(b, 'hits', 0), armor: num(b, 'armor', 0), attack: num(b, 'attack', 0),
    range: num(b, 'max_range', 0), los: num(b, 'los', 0),
  }]));

  writeFileSync(join(OUT, 'playdata.json'), JSON.stringify({
    source: {
      units: 'schema/live/live-tables-unit.tsv (live UnitType list @ 0x00C0A264)',
      buildings: 'schema/live/live-tables-building.tsv (live BuildType list)',
      techs: 'schema/live/live-tables-tech.tsv (age techs, TypeIndex 544..550)',
      rules: 'schema/live/rules-block-pid14644.txt (Constants singleton, live read)',
      pop: popSource,
    },
    counts: {
      units: units.length, buildings: blds.length, edges: edges.length,
      producers: producers.size, ages: ages.length,
      rulesDwordsCovered: rules.covered, rulesBlockBytes: rules.bytes,
      popResolved,
    },
    unresolvedProducers: unresolved,
    edges: Object.fromEntries([...producers].map(([w, ts]) => [w, ts])),
    ages,
    units: unitById,
    buildings: bldById2,
  }));
  writeFileSync(join(OUT, '.gitignore'), '*\n!.gitignore\n');

  const kb = (n) => (n / 1024).toFixed(1) + ' KB';
  console.log(`playdata.bin  ${kb(out.length)}`);
  console.log(`  ${units.length} units, ${blds.length} buildings, ` +
    `${edges.length} producer->product edges over ${producers.size} producers`);
  console.log(`  POP resolved for ${popResolved}/${units.length} unit types`);
  console.log(`  rules block: ${rules.covered} of ${RULES_DWORDS} dwords covered ` +
    `(${rules.bytes} bytes captured)`);
  if (unresolved.length) {
    console.log(`  ${unresolved.length} WHERE ids not in the building table: ` +
      unresolved.slice(0, 6).map(([t, w]) => `${t}->${w}`).join(' '));
  }
  for (const [w, ts] of [...producers].sort((a, b) => b[1].length - a[1].length).slice(0, 6)) {
    console.log(`  ${String(w).padStart(3)} ${(bldById.get(String(w))?.name_display ?? '?')
      .padEnd(14)} trains ${ts.length}`);
  }
}

main();
