// Pack the derived game data the spectator needs into one binary the browser fetches once.
//
//     node web/tools/pack-gamedata.mjs [--out web/public/data]
//
// Inputs, all produced by *other* lanes and all [measured] there — this tool derives
// nothing, it only reshapes:
//
//   schema/live/live-tables-unit.tsv      364 UnitType records read out of the live process
//                                         (ids 50..413; `UnitType` list at 0x00C0A264)
//   schema/live/balance-real.bin          493x493 int16 `Balance::final_balance_table`
//                                         captured at 0x00C12BF4
//   schema/live/rules-block-pid14644.txt  4 KB of the live `Constants` singleton, base64;
//                                         the combat constants live at +0x44..+0xB98
//
// # Why a binary and not JSON
//
// The browser hands the bytes straight to `sim_load_gamedata` and Rust reads fixed-size
// records out of them. A JSON round trip would cost a parse of half a million numbers on
// the main thread for data that is already in exactly the shape the sim wants. The only
// JSON emitted is the *name* table, which the UI needs and the simulation does not.
//
// # Copyright
//
// `schema/live/` is gitignored game content and so is the output directory
// (`web/public/data/.gitignore`). Nothing this tool writes is committed; a checkout
// without it runs the spectator on a synthetic fallback table and says so on screen.

import { readFileSync, writeFileSync, mkdirSync, existsSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = join(HERE, '..', '..');
const OUT = process.argv.includes('--out')
  ? process.argv[process.argv.indexOf('--out') + 1]
  : join(REPO, 'web', 'public', 'data');

// ---------------------------------------------------------------------------------------
// Layout — must match `web/wasm/src/gamedata.rs`. One place changes, both break loudly:
// the magic carries a version and the Rust side rejects a mismatch.
// ---------------------------------------------------------------------------------------

const MAGIC = 'DONPACK3';
/** i32 fields per unit-type record, in this order. */
const UNIT_FIELDS = [
  'type_id', 'attack', 'armor', 'hits', 'moves', 'max_range', 'min_range', 'recharge',
  'to_hit', 'domain', 'military_level', 'splash_area', 'splash_percent', 'obj_masks',
  'target_size', 'age', 'unit_flags', 'los', 'role', 'unit_flags2', 'guy_spacing',
  'x_spacing', 'y_spacing', 'uber_size', 'roster',
];
/** i32 rules values, in the field order of `don_sim::CombatRules` plus `rules_0x8b8`. */
const RULES_OFFSETS = [
  ['height_increment', 0x44], ['height_bonus', 0x48], ['flank_bonus', 0x4c],
  ['cavalry_flank_bonus', 0x50], ['vehicle_flank_bonus', 0x54], ['rocky_modifier', 0x58],
  ['overkill_frames', 0x5c], ['overkill_damage', 0x60], ['entrenchment_modifier', 0x64],
  ['river_modifier', 0x68], ['recapture_city_modifier', 0x6c],
  ['red_fort_air_defense', 0x4c4], ['rule_0x558', 0x558], ['rule_0x76c', 0x76c],
  ['rule_0xb98', 0xb98], ['rules_0x8b8', 0x8b8],
];
const BALANCE_N = 493;
const BALANCE_BASE_ID = 50; // unit ids start at 50; the table's row 0 is type id 50.
const UNIT_COUNT = 364;
const AUTHORITY_SCHEMA = 1;

// ---------------------------------------------------------------------------------------

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

function readRules(path) {
  const txt = readFileSync(path, 'utf8');
  const b64 = txt.split('\n').find((l) => l.startsWith('BLK='))?.slice(4);
  if (!b64) throw new Error('no BLK= line in ' + path);
  const buf = Buffer.from(b64, 'base64');
  return RULES_OFFSETS.map(([name, off]) => {
    if (off + 4 > buf.length) throw new Error(`rules block too short for ${name} at ${off}`);
    return [name, buf.readInt32LE(off)];
  });
}

/**
 * Pick the roster the spectator spawns from.
 *
 * The 364 UnitType records include per-nation duplicates (seven identical `Tank` rows) and
 * a long tail of non-combat and non-land types. The roster is land units that can attack
 * and can move, deduplicated by display name, keeping the lowest type id — which is also
 * the one whose balance row the duplicates share, since duplicates have identical stats.
 *
 * Deduplication is a *presentation* decision, not a simulation one: a duplicate row would
 * fight exactly the same, it would just be a second "Tank" in the legend.
 */
function pickRoster(units) {
  const seen = new Map();
  for (const u of units) {
    if (+u.domain !== 0) continue;         // land only: sea and air have no terrain here
    if (+u.attack <= 0) continue;          // must be able to deal damage
    if (+u.hits <= 0) continue;
    if (+u.moves <= 0) continue;           // must be able to close
    const key = u.name_display;
    if (!seen.has(key)) seen.set(key, u);
  }
  // Ordered by age then by type id, so the roster index is a meaningful axis for the UI.
  return [...seen.values()].sort((a, b) => (+a.age - +b.age) || (+a.type_id - +b.type_id));
}

function main() {
  const liveDir = join(REPO, 'schema', 'live');
  const unitTsv = join(liveDir, 'live-tables-unit.tsv');
  const balBin = join(liveDir, 'balance-real.bin');
  const rulesTxt = join(liveDir, 'rules-block-pid14644.txt');
  for (const p of [unitTsv, balBin, rulesTxt]) {
    if (!existsSync(p)) {
      console.error(`missing input: ${p}`);
      console.error('These are gitignored game-derived artefacts. Without them the');
      console.error('spectator falls back to a synthetic table and labels itself as such.');
      process.exit(2);
    }
  }

  const units = readTsv(unitTsv);
  if (units.length !== UNIT_COUNT) {
    throw new Error(`unit table has ${units.length} rows, expected ${UNIT_COUNT}`);
  }
  const ids = new Set();
  for (const u of units) {
    const typeId = Number(u.type_id);
    if (!Number.isInteger(typeId) || typeId < BALANCE_BASE_ID || typeId >= BALANCE_BASE_ID + UNIT_COUNT) {
      throw new Error(`invalid authority type_id ${u.type_id}`);
    }
    if (ids.has(typeId)) throw new Error(`duplicate authority type_id ${typeId}`);
    ids.add(typeId);
    for (const field of ['guy_spacing', 'x_spacing', 'y_spacing', 'uber_size']) {
      const value = Number(u[field]);
      if (!Number.isInteger(value) || value <= 0) {
        throw new Error(`unit ${typeId} has invalid ${field}: ${u[field]}`);
      }
    }
  }
  const roster = pickRoster(units);
  const rosterIndex = new Map(roster.map((u, i) => [u.type_id, i]));
  const rules = readRules(rulesTxt);
  const balance = readFileSync(balBin);
  const wantBalance = BALANCE_N * BALANCE_N * 2;
  if (balance.length !== wantBalance) {
    throw new Error(`balance table is ${balance.length} bytes, expected ${wantBalance}`);
  }

  const headerBytes = 8 + 7 * 4;
  const unitBytes = units.length * UNIT_FIELDS.length * 4;
  const rulesBytes = rules.length * 4;
  const out = Buffer.alloc(headerBytes + unitBytes + rulesBytes + balance.length);
  let o = 0;
  out.write(MAGIC, o, 'latin1'); o += 8;
  out.writeUInt32LE(units.length, o); o += 4;
  out.writeUInt32LE(UNIT_FIELDS.length, o); o += 4;
  out.writeUInt32LE(rules.length, o); o += 4;
  out.writeUInt32LE(BALANCE_N, o); o += 4;
  out.writeUInt32LE(BALANCE_BASE_ID, o); o += 4;
  out.writeUInt32LE(roster.length, o); o += 4;
  out.writeUInt32LE(AUTHORITY_SCHEMA, o); o += 4;

  for (const u of units) {
    for (const f of UNIT_FIELDS) {
      let v;
      if (f === 'roster') v = rosterIndex.has(u.type_id) ? rosterIndex.get(u.type_id) : -1;
      else v = Number(u[f]);
      if (!Number.isFinite(v)) throw new Error(`unit ${u.type_id} field ${f} is ${u[f]}`);
      // obj_masks and unit_flags are bit sets that overflow i32 as decimals in the TSV
      // only if they were written unsigned; write them back as the same 32 bits either way.
      out.writeInt32LE(v | 0, o); o += 4;
    }
  }
  for (const [, v] of rules) { out.writeInt32LE(v, o); o += 4; }
  balance.copy(out, o); o += balance.length;
  if (o !== out.length) throw new Error(`wrote ${o} of ${out.length} bytes`);

  mkdirSync(OUT, { recursive: true });
  writeFileSync(join(OUT, 'gamedata.bin'), out);
  // The name table is UI-only. Rust never sees a string.
  writeFileSync(join(OUT, 'gamedata.json'), JSON.stringify({
    source: {
      units: 'schema/live/live-tables-unit.tsv',
      balance: 'schema/live/balance-real.bin (Balance::final_balance_table @ 0x00C12BF4)',
      rules: 'schema/live/rules-block-pid14644.txt (Constants singleton, live read)',
    },
    unitCount: units.length,
    rosterCount: roster.length,
    balanceN: BALANCE_N,
    balanceBaseId: BALANCE_BASE_ID,
    rules: Object.fromEntries(rules),
    // index-aligned with the roster the sim spawns from
    roster: roster.map((u) => ({
      id: +u.type_id, name: u.name_display, age: +u.age, attack: +u.attack,
      armor: +u.armor, hits: +u.hits, moves: +u.moves, range: +u.max_range,
      recharge: +u.recharge,
    })),
    names: Object.fromEntries(units.map((u) => [u.type_id, u.name_display])),
  }));

  // A gitignore that ignores its own directory, so game-derived bytes can never be
  // committed by a lane that runs this tool and then stages a directory.
  writeFileSync(join(OUT, '.gitignore'), '*\n!.gitignore\n');

  const kb = (n) => (n / 1024).toFixed(1) + ' KB';
  console.log(`gamedata.bin  ${kb(out.length)}  (${units.length} unit types, ` +
    `${roster.length} in roster, ${rules.length} rules, ${BALANCE_N}x${BALANCE_N} balance)`);
  console.log(`roster ages ${roster[0].age}..${roster[roster.length - 1].age}: ` +
    roster.slice(0, 8).map((u) => u.name_display).join(', ') + ', ...');
}

main();
