// A `.rcx` recorded-game decoder that runs in the browser.
//
// This is a port of the decode path that already exists in Rust — `don-net`
// (`stream.rs`, `obfuscate.rs`, `lib.rs`) and `don-replay` (`replay.rs`) — into a
// dependency-free ES module, so a page can open a real recording without a server-side
// step. Every structural constant here has a named source in the derivation and is cited
// at its use site; nothing is guessed.
//
//   container            docs/derivation/replay-io.md §1   (one gzip member from offset 0)
//   GameInfo header      GameInfo::walk_data 0x005d6570, layout from schema/pdb-types.json
//   record framing       CommandPackage 0x00952fb0 / 0x00952d90, 18 bytes + payload
//   payload obfuscation  CommandPackage::process_all 0x0094c500
//   opcode wire lengths  CommandPackage::process 0x0094a700, 82-entry switch
//
// The one place this deviates from the Rust is in `recoverObfuscation`: it tries the
// identity key (0, no padding) *first* rather than only the frequency-ranked candidates.
// A solo recording is unobfuscated, and its modal payload word is a camera coordinate, not
// zero — so frequency ranking can miss the correct key on exactly the files where the
// correct key is trivially known. Testing it first is both faster and strictly safer.
//
// Fidelity: Tier C. The measurement that backs it is `web/tools/rcx-check.mjs`, which runs
// this module over the whole `ron-data/replays/` corpus and reports structural tiling,
// header-derived obfuscation, and checksum invariants. It does not call a command stream
// "round-tripped": copying slices back out of the plaintext cannot establish that claim.

// ---------------------------------------------------------------------------
// container
// ---------------------------------------------------------------------------

/// Decompress a `.rcx`. Most are a single gzip member from offset 0; a few in the shipped
/// corpus are stored raw, which is why the magic is checked instead of assumed.
export async function gunzip(bytes) {
  if (bytes.length >= 2 && bytes[0] === 0x1f && bytes[1] === 0x8b) {
    const ds = new DecompressionStream('gzip');
    const stream = new Blob([bytes]).stream().pipeThrough(ds);
    const parts = [];
    let total = 0;
    const reader = stream.getReader();
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      parts.push(value);
      total += value.length;
    }
    const out = new Uint8Array(total);
    let o = 0;
    for (const p of parts) { out.set(p, o); o += p.length; }
    return out;
  }
  return bytes;
}

// ---------------------------------------------------------------------------
// opcode table — CommandPackage::process 0x0094a700
// ---------------------------------------------------------------------------

/// Fixed wire size per opcode, or 0 for the three variable-length forms.
/// Values are `sizeof(<X>Command)` from `rise.pdb`, mirrored from
/// `crates/don-net/src/opcodes.rs` (itself generated from the PDB).
export const COMMAND_SIZES = [
  0, 1, 5, 13, 17, 13, 17, 22, 26, 10, 10, 25, 1, 1, 5, 9,
  13, 21, 9, 9, 13, 5, 17, 21, 9, 25, 17, 1, 25, 1, 13, 13,
  9, 9, 25, 1, 1, 13, 13, 9, 9, 9, 9, 17, 17, 17, 13, 13,
  15, 11, 9, 0, 5, 1, 1, 1, 5, 65, 6, 5, 5, 5, 1, 1,
  1, 5, 5, 17, 0, 9, 5, 7, 10, 33, 11, 53, 2, 2, 521, 9,
  3, 2,
];

export const NUM_OPCODES = 82;

/// Byte length of the command at `buf[off]`, mirroring the value the engine's handler
/// returns. The three variable-length formulas are read off the `lea` that produces each
/// handler's return value (see `Command::wire_len` in `don-net` for the instruction
/// addresses); in particular `process_chat` is `19 + 2*len`, because the wide string on
/// the wire is NUL-terminated.
export function commandWireLen(buf, off) {
  const have = buf.length - off;
  if (have < 1) return -1;
  const op = buf[off];
  if (op >= NUM_OPCODES) return -1;
  if (op === 0x00) {                                  // GroupCommand: 3 + 2*num
    if (have < 2) return -1;
    return 3 + 2 * buf[off + 1];
  }
  if (op === 0x33) {                                  // SplineCommand: 6 + 8*len
    if (have < 6) return -1;
    return 6 + 8 * (buf[off + 4] | (buf[off + 5] << 8));
  }
  if (op === 0x44) {                                  // ChatCommand: 19 + 2*len
    if (have < 17) return -1;
    const dv = new DataView(buf.buffer, buf.byteOffset + off + 13, 4);
    const n = dv.getInt32(0, true);
    if (n < 0 || n > 512) return -1;                  // the send buffer is 512 bytes
    return 19 + 2 * n;
  }
  return COMMAND_SIZES[op];
}

// ---------------------------------------------------------------------------
// obfuscation — CommandPackage::process_all 0x0094c500
// ---------------------------------------------------------------------------

export const LCG_A = 0x0019660d;
export const LCG_C = 0x3c6ef35f;

/// The engine's `Random`, which is exactly one u32. `in_range(0,2)` is the inter-command
/// pad draw; only the low 16 bits of the seed can ever change a draw, which is why
/// recovering the pad stream is a 256-way search once the XOR key is known.
export class PadRandom {
  constructor(seed) { this.seed = seed >>> 0; }
  nextPad() {
    // s = s*A + C  (mod 2^32), then (low16 * 2) >> 16
    this.seed = (Math.imul(this.seed, LCG_A) + LCG_C) >>> 0;
    return ((this.seed & 0xffff) * 2) >>> 16;
  }
}

/// XOR a payload in place over whole u16 words. A trailing odd byte is left alone:
/// `process_all` computes the word count as `size / 2`.
export function xorPayload(buf, key) {
  const lo = key & 0xff, hi = (key >> 8) & 0xff;
  const n = buf.length & ~1;
  for (let i = 0; i < n; i += 2) { buf[i] ^= lo; buf[i + 1] ^= hi; }
  return buf;
}

/// Rank candidate XOR keys by ciphertext word frequency, most likely first. Command
/// payloads carry long runs of zero, so `0 ^ key == key` is usually the modal word —
/// "usually" is not "always", which is why the caller validates each candidate by whether
/// the stream actually decodes.
export function rankXorKeys(payloads, n) {
  const counts = new Map();
  for (const p of payloads) {
    const m = p.length & ~1;
    for (let i = 0; i < m; i += 2) {
      const w = p[i] | (p[i + 1] << 8);
      counts.set(w, (counts.get(w) || 0) + 1);
    }
  }
  return [...counts.entries()]
    .sort((a, b) => (b[1] - a[1]) || (a[0] - b[0]))
    .slice(0, n)
    .map((e) => e[0]);
}

/// Split one package payload into commands, honouring inter-command padding.
/// Returns `null` when the command list does not tile the payload exactly — which is the
/// signal the obfuscation search scores on.
export function decodeCommands(payload, padSeed) {
  const rng = padSeed === null ? null : new PadRandom(padSeed);
  const out = [];
  let i = 0;
  while (i < payload.length) {
    const l = commandWireLen(payload, i);
    if (l <= 0 || i + l > payload.length) return null;
    out.push({ op: payload[i], off: i, len: l });
    i += l + (rng ? rng.nextPad() : 0);
  }
  return i === payload.length ? out : null;
}

/// Recover `(xorKey, padSeed)` for a whole recording.
///
/// `padSeed === null` means the unobfuscated branch of `process_all`. Multiplayer is not
/// inferred from ciphertext frequency: `GameInfo::seed` is independently present in the
/// replay header and `CommandPackage::process_all` derives both the XOR key and the local
/// per-package padding generator from exactly that value. The two engine branches are
/// evaluated and labelled first. A separately labelled payload-fit fallback keeps older
/// 2014 recordings readable where that relationship does not hold; it is structural
/// recovery, not independent confirmation of a key.
export function recoverObfuscation(records, { gameSeed = null, probe = 48, keys = 12 } = {}) {
  const payloads = records.map((r) => r.payload);
  const candidates = [{ key: 0, padSeed: null, source: 'plain' }];
  if (gameSeed !== null) {
    candidates.push({
      key: (gameSeed >>> 8) & 0xffff,
      padSeed: gameSeed >>> 0,
      source: 'GameInfo::seed',
    });
  }
  let best = { key: 0, padSeed: null, source: 'plain', plains: [], decoded: -1 };
  for (const candidate of candidates) {
    const { key, padSeed, source } = candidate;
    const plains = payloads.map((p) => (key === 0 ? p.slice() : xorPayload(p.slice(), key)));
    let decoded = 0;
    for (const plain of plains) if (decodeCommands(plain, padSeed)) decoded++;
    if (decoded > best.decoded) best = { key, padSeed, source, plains, decoded };
  }
  // Several 2014-era recordings do not relate their header seed to the command transform
  // in the way `0x0094c500` in the current executable does. Keep those readable, but label
  // the weaker evidence: rank ciphertext words, fit the low-byte padding state, then
  // require the chosen pair to tile the entire file. Never present this as header-derived.
  if (best.decoded === records.length || best.decoded >= records.length * 0.99) return best;
  const ranked = rankXorKeys(payloads, keys)
    .filter((key) => !candidates.some((c) => c.key === key));
  for (const key of ranked) {
    const plains = payloads.map((p) => xorPayload(p.slice(), key));
    const p = Math.min(probe, plains.length);
    let shortlist = [];
    let probeBest = -1;
    for (let lo = 0; lo < 256; lo++) {
      const padSeed = (((key & 0xff) << 8) | lo) >>> 0;
      let decoded = 0;
      for (let i = 0; i < p; i++) if (decodeCommands(plains[i], padSeed)) decoded++;
      if (decoded > probeBest) { probeBest = decoded; shortlist = [padSeed]; }
      else if (decoded === probeBest) shortlist.push(padSeed);
    }
    for (const padSeed of shortlist) {
      let decoded = 0;
      for (const plain of plains) if (decodeCommands(plain, padSeed)) decoded++;
      if (decoded > best.decoded) {
        best = { key, padSeed, source: 'payload-fit fallback', plains, decoded };
      }
    }
    if (best.decoded === records.length) break;
  }
  return best;
}

// ---------------------------------------------------------------------------
// record framing and stream location
// ---------------------------------------------------------------------------

export const PACKAGE_HEADER_LEN = 18;

/// On-disk fields from the writer at `0x00952fb0`. This is deliberately not named as the
/// in-memory `CommandPackage` prefix: the first word is copied from global `Game::frame`,
/// while the fourth is `CommandPackage::stamp`.
export function readPackageHeader(dv, o) {
  return {
    frame: dv.getUint32(o, true),
    play: dv.getInt32(o + 4, true),
    valid: dv.getInt32(o + 8, true),
    stamp: dv.getUint32(o + 12, true),
    size: dv.getUint16(o + 16, true),
  };
}

/// Find the command stream by backward dynamic programming: a record at offset `o` is
/// good iff `o + 18 + size` is good, with `play < 8` and `frame` non-decreasing across the
/// link. Nothing in the file points at the stream, so it is located structurally, as the
/// unique longest chain of records that tiles exactly to EOF.
///
/// Deliberately not constrained on `valid == 0`: that silently swallows the first package
/// of a solo stream, which is the only solo package with `valid == 1`.
export function findStream(buf) {
  const n = buf.length;
  if (n < PACKAGE_HEADER_LEN) return null;
  const dv = new DataView(buf.buffer, buf.byteOffset, n);
  const count = new Int32Array(n + 1);
  let bestC = 0, bestO = -1;
  for (let o = n - PACKAGE_HEADER_LEN; o >= 0; o--) {
    if (dv.getUint32(o + 4, true) >= 8) continue;
    const ln = dv.getUint16(o + 16, true);
    if (ln > 4000) continue;
    const next = o + PACKAGE_HEADER_LEN + ln;
    if (next > n) continue;
    let c;
    if (next === n) c = 1;
    else {
      if (count[next] === 0) continue;
      const a = dv.getUint32(o, true), b = dv.getUint32(next, true);
      if (b < a || b - a > 200) continue;
      c = count[next] + 1;
    }
    count[o] = c;
    if (c > bestC) { bestC = c; bestO = o; }
  }
  return bestO < 0 ? null : { start: bestO, records: bestC };
}

/// Walk the 18-byte framing from `start` to EOF.
export function readPackages(buf, start) {
  const dv = new DataView(buf.buffer, buf.byteOffset, buf.length);
  const out = [];
  let pos = start;
  while (pos + PACKAGE_HEADER_LEN <= buf.length) {
    const h = readPackageHeader(dv, pos);
    const end = pos + PACKAGE_HEADER_LEN + h.size;
    if (end > buf.length) break;
    out.push({ header: h, payload: buf.subarray(pos + PACKAGE_HEADER_LEN, end), offset: pos });
    pos = end;
  }
  return { records: out, end: pos };
}

// ---------------------------------------------------------------------------
// the GameInfo header
// ---------------------------------------------------------------------------

/// `GameInfo::data[30]` — the game-setup byte block, named from `rise.pdb`'s own TPI
/// (`schema/pdb-types.json`, `GameInfo` +0x18). The names are the engine's; each one
/// indexes a `<CATEGORIES>` list in `ron-data/rules.xml`, which `data/replaymeta.json`
/// carries when it has been packed.
export const SETTING_NAMES = [
  'team_style', 'map_style', 'map_size', 'players', 'max_observers', 'game_speed',
  'game_rules', 'difficulty', 'starting_town', 'starting_resources', 'starting_resources2',
  'tech_cost', 'reveal_map', 'pop_limit', 'rush_rules', 'cannon_times',
  'starting_technology', 'starting_technology2', 'ending_technology', 'elimination',
  'victory', 'wonderwin', 'score_goal', 'popwin', 'time_limit', 'chairs', 'econwin',
  'scenario_type', 'script_type', 'mods',
];

/// Which `<CATEGORIES id=...>` in rules.xml labels each setting. `null` where the engine
/// stores a raw number rather than a category index.
export const SETTING_CATEGORY = {
  team_style: 'gamestyles', map_style: 'mapstyles', map_size: 'mapsizes',
  players: null, max_observers: null, game_speed: 'gamespeeds', game_rules: 'gamerules',
  difficulty: 'difficulties', starting_town: 'startingtowns',
  starting_resources: 'startingresources', starting_resources2: 'defenderresources',
  tech_cost: 'techcosts', reveal_map: 'revealmaps', pop_limit: 'poplimits',
  rush_rules: 'rushrules', cannon_times: 'cannontimes',
  starting_technology: 'startingtechs', starting_technology2: 'defendertechs',
  ending_technology: 'endingtechs', elimination: 'eliminations', victory: 'victories',
  wonderwin: 'wonderwins', score_goal: 'scores', popwin: 'popwins',
  time_limit: 'timelimits', chairs: 'chairs', econwin: 'econwins',
  scenario_type: null, script_type: null, mods: 'mods',
};

function readWideString(dv, buf, o) {
  if (o + 4 > buf.length) return null;
  const n = dv.getUint32(o, true);
  if (n > 4096 || o + 4 + 2 * n > buf.length) return null;
  let s = '';
  for (let i = 0; i < n; i++) s += String.fromCharCode(dv.getUint16(o + 4 + 2 * i, true));
  return { value: s, end: o + 4 + 2 * n };
}

/// Parse the `SaveGame`-serialised header at the front of the payload.
///
/// Written by `Game::walk_data` 0x00589600 → `GameInfo::walk_data` 0x005d6570. The walk
/// order is: tag `0x16`, tag `0x42`, the version `String`, `GameInfo` +0x00..+0x18
/// (version word, seed, three checksum settings, flags), the 30 setting bytes one at a
/// time, then eight player slots each tagged `0x50`.
///
/// Per slot: a `u16` copy of `Player::flags`, and when `flags & 1` the 57-byte
/// `Player` head (`+0x00..+0x39`) followed by the player's name `String`. The flags word
/// therefore appears twice on the wire — that is the engine's own redundancy, not a
/// misread.
export function parseHeader(buf) {
  const dv = new DataView(buf.buffer, buf.byteOffset, buf.length);
  const h = { ok: false, warnings: [] };
  let o = 0;
  h.tagGame = buf[o++];
  h.tagInfo = buf[o++];
  if (h.tagGame !== 0x16 || h.tagInfo !== 0x42) {
    h.warnings.push(`unexpected section tags ${h.tagGame}/${h.tagInfo} (expected 0x16/0x42)`);
  }
  const ver = readWideString(dv, buf, o);
  if (!ver) return h;
  h.version = ver.value;
  o = ver.end;

  h.versionWord = dv.getUint32(o, true); o += 4;      // GameInfo::version — per engine build
  h.seed = dv.getUint32(o, true); o += 4;             // GameInfo::seed
  h.checksumDeep = dv.getInt32(o, true); o += 4;
  h.checksumWindowSize = dv.getInt32(o, true); o += 4;
  h.checksumFailureThreshold = dv.getInt32(o, true); o += 4;
  h.flags = dv.getUint32(o, true); o += 4;            // GameInfo::flags (save/load only)

  h.settings = {};
  h.settingBytes = Array.from(buf.subarray(o, o + 30));
  for (let i = 0; i < 30; i++) h.settings[SETTING_NAMES[i]] = buf[o + i];
  o += 30;

  h.players = [];
  for (let i = 0; i < 8; i++) {
    if (o >= buf.length) { h.warnings.push(`ran out of payload at slot ${i}`); break; }
    const tag = buf[o++];
    if (tag !== 0x50) { h.warnings.push(`slot ${i} tag ${tag} (expected 0x50)`); break; }
    const flags = dv.getUint16(o, true); o += 2;
    const p = { slot: i, flags, used: (flags & 1) === 1 };
    if (p.used) {
      if (o + 0x39 > buf.length) { h.warnings.push(`slot ${i} blob truncated`); break; }
      p.synced = {
        used_zoom_control: dv.getInt32(o + 0x00, true),
        frames_zoomed_in: dv.getInt32(o + 0x04, true),
        frames_zoomed_out: dv.getInt32(o + 0x08, true),
        clicks: dv.getInt32(o + 0x0c, true),
        hotkeys: dv.getInt32(o + 0x10, true),
        minimap_clicks: dv.getInt32(o + 0x14, true),
        mainmap_clicks: dv.getInt32(o + 0x18, true),
        cheated: dv.getInt32(o + 0x1c, true),
        control_groups_formed: dv.getInt32(o + 0x20, true),
        control_groups_activated: dv.getInt32(o + 0x24, true),
      };
      p.caravanFrame = dv.getInt32(o + 0x28, true);
      p.popCapFrame = dv.getInt32(o + 0x2c, true);
      p.tribe = buf[o + 0x32];
      p.who = buf[o + 0x33];
      p.team = dv.getInt8(o + 0x34);
      p.handicap = buf[o + 0x35];
      p.play = buf[o + 0x36];
      p.pauses = buf[o + 0x37];
      p.diff = buf[o + 0x38];
      o += 0x39;
      const name = readWideString(dv, buf, o);
      if (!name) { h.warnings.push(`slot ${i} name unreadable`); break; }
      p.name = name.value;
      o = name.end;
    }
    h.players.push(p);
  }
  h.headerEnd = o;

  // `Game::walk_data` continues: 404 bytes from `game+0x550` (whose first dword is
  // `Game::frame`), then a length-prefixed variable block, then 4 bytes, then the
  // map/scenario name `String` written by the header writer 0x00952a50. Everything after
  // that is the ~1 MB `Rules::walk_data` state blob, which is not decoded here.
  try {
    let q = o;
    h.frameAtSave = dv.getUint32(q, true);
    q += 404;
    q += 4;
    const varLen = dv.getUint32(q, true); q += 4;
    if (varLen >= 0 && varLen < 1 << 20 && q + varLen + 4 < buf.length) {
      q += varLen + 4;
      const mn = readWideString(dv, buf, q);
      // Accept it only if it looks like a name rather than a coincidence. An *empty*
      // string is the normal case: a random-map game has no scenario name, and the
      // engine still writes the four-byte zero count.
      if (mn && mn.value.length < 128 && /^[\x20-\x7e]*$/.test(mn.value)) {
        h.mapName = mn.value;
        h.mapNameAt = q;
      }
    }
  } catch { /* the trailing blob is best-effort; the header proper is what matters */ }

  h.ok = true;
  return h;
}

// ---------------------------------------------------------------------------
// the whole thing
// ---------------------------------------------------------------------------

/// Decode an entire recording from its raw (possibly gzipped) bytes.
export async function decodeReplay(raw, { name = '', onProgress = null } = {}) {
  const payload = await gunzip(raw instanceof Uint8Array ? raw : new Uint8Array(raw));
  const header = parseHeader(payload);
  onProgress?.('located');
  const loc = findStream(payload);
  if (!loc) throw new Error('no command-package chain found');
  const { records, end } = readPackages(payload, loc.start);
  const residue = payload.length - end;

  onProgress?.('deobfuscating');
  const obf = recoverObfuscation(records, { gameSeed: header.ok ? header.seed : null });
  if (!obf) throw new Error('no viable obfuscation key');

  const rep = {
    name,
    compressedLen: raw.length,
    payloadLen: payload.length,
    header,
    version: header.version || null,
    streamStart: loc.start,
    stateBlobLen: loc.start - (header.headerEnd || 0),
    framingResidue: residue,
    packages: records.length,
    packagesDecoded: obf.decoded,
    xorKey: obf.key,
    padSeed: obf.padSeed,
    obfuscationSource: obf.source,
    headerSeedKey: header.ok ? (header.seed >>> 8) & 0xffff : null,
    players: [],
    turns: [],
    turnIndex: new Map(),
    commandCount: 0,
    opcodeHistogram: new Int32Array(NUM_OPCODES),
    checksumPackets: 0,
    checksumTotalOk: 0,
    checksumShapeOk: 0,
    anomalies: [],
    frameFirst: Infinity,
    frameLast: -Infinity,
  };

  for (let i = 0; i < records.length; i++) {
    const r = records[i];
    const plain = obf.plains[i];
    const cmds = decodeCommands(plain, obf.padSeed);
    if (!cmds) {
      if (rep.anomalies.length < 16) {
        rep.anomalies.push(`stamp ${r.header.stamp} play ${r.header.play}: payload did not tile`);
      }
      continue;
    }
    const pt = {
      play: r.header.play,
      frame: r.header.frame,
      stamp: r.header.stamp,
      valid: r.header.valid,
      bytes: plain,
      commands: cmds,
      checksums: null,
    };
    for (const c of cmds) {
      rep.commandCount++;
      rep.opcodeHistogram[c.op]++;
      if (c.op === 0x39 && c.len === 65) {
        const dv = new DataView(plain.buffer, plain.byteOffset + c.off, 65);
        const v = new Uint32Array(16);
        for (let k = 0; k < 16; k++) v[k] = dv.getUint32(1 + 4 * k, true);
        pt.checksums = v;
        rep.checksumPackets++;
        let sum = 0;
        for (let k = 0; k < 15; k++) sum = (sum + v[k]) >>> 0;
        if (sum === v[15]) rep.checksumTotalOk++;
        let shaped = true;
        for (let k = 0; k < 15; k++) {
          if ((v[k] & 0xffff) >= 65521 || (v[k] >>> 16) >= 65521) shaped = false;
        }
        if (shaped) rep.checksumShapeOk++;
      }
    }
    if (!rep.players.includes(pt.play)) rep.players.push(pt.play);
    if (r.header.frame < rep.frameFirst) rep.frameFirst = r.header.frame;
    if (r.header.frame > rep.frameLast) rep.frameLast = r.header.frame;

    const g = r.header.stamp;
    let idx = rep.turnIndex.get(g);
    if (idx === undefined) {
      idx = rep.turns.length;
      rep.turnIndex.set(g, idx);
      rep.turns.push({ turn: g, players: [] });
    }
    rep.turns[idx].players.push(pt);
  }

  rep.turns.sort((a, b) => a.turn - b.turn);
  rep.turnIndex = new Map(rep.turns.map((t, i) => [t.turn, i]));
  rep.players.sort((a, b) => a - b);
  if (!isFinite(rep.frameFirst)) { rep.frameFirst = 0; rep.frameLast = 0; }

  // Cross-player checksum agreement, joined on the fourth on-disk word — the monotone
  // `CommandPackage::stamp`. Two retail clients disagreeing is a real desync in the
  // recording and is worth showing; it also puts a floor under what our simulation can be
  // asked to match.
  rep.crossplay = { comparisons: 0, identical: 0, perChannel: new Int32Array(16) };
  for (const t of rep.turns) {
    const tuples = t.players.filter((p) => p.checksums).map((p) => p.checksums);
    for (let i = 1; i < tuples.length; i++) {
      rep.crossplay.comparisons++;
      let same = true;
      for (let c = 0; c < 16; c++) {
        if (tuples[i][c] !== tuples[0][c]) { same = false; rep.crossplay.perChannel[c]++; }
      }
      if (same) rep.crossplay.identical++;
    }
  }

  // Median simulation frames per lockstep turn, measured from `Game::frame` deltas over
  // `CommandPackage::stamp` deltas rather than assumed: turn length adapts to latency.
  const p0 = rep.players[0];
  const samples = [];
  let prev = null;
  for (const t of rep.turns) {
    const pt = t.players.find((x) => x.play === p0);
    if (!pt) continue;
    if (prev) {
      const ds = t.turn - prev.stamp, df = pt.frame - prev.frame;
      if (ds > 0 && df >= 0) samples.push(df / ds);
    }
    prev = { stamp: t.turn, frame: pt.frame };
  }
  samples.sort((a, b) => a - b);
  rep.framesPerTurn = samples.length ? samples[samples.length >> 1] : null;

  return rep;
}
