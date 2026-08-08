// The replay viewer: load a real `.rcx`, watch what every player ordered, turn by turn.
//
// What this page is honest about, stated once here and repeated on screen:
//
//   * The **command stream** is real and structurally decoded. Header, framing, payload
//     de-obfuscation and the 82-opcode split all come from `rcx.js`; the page identifies
//     whether the key came independently from GameInfo or from a weaker payload fit.
//     `node web/tools/rcx-check.mjs` runs the identical module over the whole corpus and
//     reports the two known short-package anomalies instead of hiding them.
//   * The **world is not**. The ~1 MB `Rules::walk_data` state blob between the header and
//     the command stream is not decoded (docs/derivation/replay-io.md §7.2), so there are
//     no starting units, no terrain and no object table. Nothing here simulates a unit.
//     Orders that carry a world coordinate are *plotted* at that coordinate; orders that
//     name an object id are *annotated*, because we have no object table to resolve them
//     against.
//   * The map extent is derived, not guessed: `mapsizes[map_size]/DATA * 768` fine units
//     square. See `worldExtent` below for the measurement that fixes the 768.
//
// Everything on screen is therefore either a decoded field of the recording or a count of
// decoded fields. Where the viewer cannot act on a command it says so and counts it.

import {
  decodeReplay, NUM_OPCODES, SETTING_NAMES, SETTING_CATEGORY,
} from './rcx.js';
import { COMMANDS, decode as decodeCmd } from './wire.gen.js';

// ---------------------------------------------------------------------------
// classification: what the viewer can do with each opcode
// ---------------------------------------------------------------------------

/// Commands that carry a world coordinate we can place on the map. The field pair is the
/// PDB field name from `schema/command-wire.json`, not a guess.
const PLOTTED = {
  0x07: ['to_x', 'to_y', 'move'],
  0x08: ['to_x', 'to_y', 'move'],
  0x09: ['to_x', 'to_y', 'attack'],
  0x0a: ['to_x', 'to_y', 'move'],
  0x0b: ['to_x', 'to_y', 'attack'],
  0x16: ['x', 'y', 'econ'],
  0x17: ['x', 'y', 'spell'],
  0x19: ['x', 'y', 'build'],
  0x32: ['x', 'y', 'ping'],
};

/// Commands for which this page actually changes model state. Anything whose switch arm
/// would only produce a line of text stays "annotated"; decoding a struct name is not
/// interpreting the command's effect.
const INTERPRETED = new Set([
  0x00, 0x34, 0x44, 0x46, 0x47, 0x48, 0x4c, 0x4f,
]);

/// Opcodes for which the separate WASM prototype has a handler, as implemented by
/// `web/wasm/src/real.rs` (`GroupCommand`, `AttackCommand`, `MoveToCommand`,
/// `HaltCommand`). This viewer never invokes that WASM module and does not execute a world.
const WASM_HANDLER = new Set([0x00, 0x04, 0x07, 0x0c]);

/// Per-frame bookkeeping the engine emits whether or not the player did anything:
/// the camera pose, the turn-timing report, the input-activity accumulators, the lockstep
/// checksum ledger, and the session begin/drop markers. They are 96–99% of every recording
/// by count, so leaving them in a coverage percentage would make the number meaningless.
/// They are still decoded, counted, and drawn — just not counted as *orders*.
///
///   0x01 begin · 0x38 check_random · 0x39 check_sums · 0x3a next_check_sum
///   0x48 camera · 0x4a turn_data · 0x4f player_speed · 0x50 ungraceful_player_drop
const BOOKKEEPING = new Set([0x01, 0x38, 0x39, 0x3a, 0x48, 0x4a, 0x4f, 0x50]);

/// Commands that arrive every frame and would flood a scrolling feed. Suppressed from the
/// per-player list only; every counter still sees them.
const FEED_SUPPRESS = new Set([0x48, 0x4a, 0x4f, 0x39, 0x3a, 0x38]);

/// `don-replay::wire::classify` — what a command does from the simulation's point of view.
/// Reproduced exactly rather than re-derived: `Presentation` is the conservative set, and
/// every opcode whose handler has not been read counts as `Sim`, so the worklist
/// over-reports rather than under-reports.
function classify(op) {
  if (op === 0x39 || op === 0x3a) return 'lockstep';
  if (op === 0x44 || op === 0x48) return 'presentation';
  return 'sim';
}

const CHANNELS = ['units', 'builds', 'walls', 'ammo', 'deaths', 'groups', 'guys', 'leaders',
  'cities', 'items', 'goods', 'world', 'rules', 'scenario_data', 'script_run_time'];

/// Eight owner slots, eight hues. Slot order is the engine's `play` field.
const PCOL = ['#5ab7ff', '#ff6b5b', '#6ee7a8', '#ffb454', '#c48bff', '#ff8fd0', '#7fe3e0', '#b9c34a'];

const KIND_COL = {
  move: '#5ab7ff', attack: '#ff6b5b', build: '#6ee7a8', econ: '#ffb454',
  spell: '#c48bff', ping: '#ffffff',
};

// ---------------------------------------------------------------------------
// dom
// ---------------------------------------------------------------------------

const $ = (id) => document.getElementById(id);
const el = {
  pick: $('pick'), file: $('file'), openLocal: $('openLocal'),
  bDecode: $('bDecode'), bMeta: $('bMeta'), bValid: $('bValid'),
  map: $('map'), overlay: $('overlay'), legend: $('legend'), drop: $('drop'),
  kvFile: $('kvFile'), kvSetup: $('kvSetup'), players: $('players'),
  covBar: $('covBar'), kvCov: $('kvCov'), covNote: $('covNote'),
  opHist: $('opHist'), chat: $('chat'), kvDecode: $('kvDecode'),
  btnPlay: $('btnPlay'), btnStepB: $('btnStepB'), btnStepF: $('btnStepF'), btnHome: $('btnHome'),
  speed: $('speed'), scrub: $('scrub'), clock: $('clock'),
  trails: $('trails'), cameraChk: $('camera'), strip: $('strip'),
};

let META = null;          // data/replaymeta.json
let VALID = null;         // the replay-validation entry matching the loaded file
let R = null;             // decoded replay
let EV = null;            // flat event arrays
let S = null;             // stream model state
let cursor = 0;           // simulation frame
let playing = false;
let lastT = 0;

// ---------------------------------------------------------------------------
// metadata
// ---------------------------------------------------------------------------

function catLabel(cat, v) {
  const c = META?.categories?.[cat];
  const it = c?.items?.[v];
  if (!it) return `${v}`;
  const name = (it.label || it.name || '').replace(/^#ICON\d+/, '').trim();
  return name ? `${name} (${v})` : `${v}`;
}
function catData(cat, v) {
  const d = META?.categories?.[cat]?.items?.[v]?.data;
  return d == null ? null : Number(d);
}
function tribeName(t) {
  const n = META?.tribes?.[t];
  return n ? `${n} (${t})` : `#${t} (not in rules.xml TRIBES)`;
}

/// World extent in fine units.
///
/// `<CATEGORIES id="mapsizes">` gives a `<DATA>` of 40/50/60/70/80/90/100 per size. On 59
/// corpus files the largest `CameraCommand` coordinate lands on **exactly** `DATA * 768`
/// for Standard (53760), Large (61440) and Big Huge (76800) — the camera clamps at the
/// world edge, so that product is the edge. 768 fine units is one `WCoord` cell (4 tiles ×
/// 192), so `DATA` counts WCoord cells and the map is `DATA * 4` tiles square. [measured,
/// web/tools/rcx-check.mjs over ron-data/replays/]
function worldExtent() {
  const d = catData('mapsizes', R?.header?.settings?.map_size ?? 0);
  return d ? d * 768 : 76800;
}

async function loadMeta() {
  try {
    META = await (await fetch('./data/replaymeta.json')).json();
    const nc = Object.keys(META.categories || {}).length;
    el.bMeta.textContent = `rules.xml ${nc} lists · ${META.tribes.length} nations`;
    el.bMeta.className = 'badge on';
  } catch {
    META = null;
    el.bMeta.textContent = 'no replaymeta.json — raw setting bytes';
    el.bMeta.className = 'badge off';
  }
  try {
    const idx = await (await fetch('./data/replays.json')).json();
    el.pick.replaceChildren();
    const prompt = document.createElement('option');
    prompt.value = '';
    prompt.textContent = '— pick a recording —';
    el.pick.append(prompt);
    for (const r of idx.replays) {
      const who = (r.slots || []).map((s) => s.name).filter(Boolean).join(' vs ') || '?';
      const tag = r.error ? ' [undecodable]'
        : r.commandStream === false ? ' [no command stream]'
          : `${r.multiplayer ? ' MP' : ''}${r.checksumPackets ? ' ✓cs' : ''}`;
      const option = document.createElement('option');
      option.value = r.file;
      option.textContent = `${r.file.replace(/\.rcx$/, '')} — ${who}${tag}`;
      el.pick.append(option);
    }
  } catch {
    el.pick.replaceChildren();
    const option = document.createElement('option');
    option.value = '';
    option.textContent = '— no data/replays.json: run web/tools/pack-replays.mjs —';
    el.pick.append(option);
  }
}

// ---------------------------------------------------------------------------
// loading a file
// ---------------------------------------------------------------------------

function status(msg, cls = '') {
  el.bDecode.textContent = msg;
  el.bDecode.className = 'badge' + (cls ? ' ' + cls : '');
}

async function loadBytes(bytes, name) {
  status('decoding…');
  await new Promise((r) => setTimeout(r, 0));
  const t0 = performance.now();
  try {
    R = await decodeReplay(bytes, { name });
  } catch (e) {
    status(`decode failed: ${e.message}`, 'bad');
    el.overlay.textContent = `${name}\n\n${e.message}`;
    return;
  }
  const ms = performance.now() - t0;
  buildEvents();
  matchValidation(name);
  resetModel();
  cursor = R.frameFirst;
  el.scrub.min = R.frameFirst; el.scrub.max = R.frameLast; el.scrub.value = cursor;
  el.scrub.disabled = false;
  for (const b of [el.btnPlay, el.btnStepB, el.btnStepF, el.btnHome]) b.disabled = false;
  const pct = (100 * R.packagesDecoded / Math.max(1, R.packages)).toFixed(3);
  status(`${R.packagesDecoded}/${R.packages} packages (${pct}%) · ${ms.toFixed(0)} ms`,
    R.packagesDecoded === R.packages && R.framingResidue === 0 ? 'on' : 'off');
  renderFile(ms);
  renderSetup();
  renderPlayers();
  renderLegend();
  applyTo(cursor);
  layout();
}

async function loadUrl(url, name) {
  status('fetching…');
  const res = await fetch(url);
  if (!res.ok) { status(`fetch ${res.status}`, 'bad'); return; }
  await loadBytes(new Uint8Array(await res.arrayBuffer()), name);
}

function matchValidation(name) {
  VALID = null;
  const files = META?.validation?.files;
  if (!files) { el.bValid.textContent = 'no divergence data'; el.bValid.className = 'badge off'; return; }
  const base = name.replace(/^multi__/, '');
  VALID = files.find((f) => f.file === base) || null;
  if (VALID) {
    // Report the best *non-trivial* survival. A channel whose agreements all came from a
    // walker that touched zero bytes "survives" the whole recording and means nothing;
    // putting that number in a badge would be the most flattering possible lie.
    let best = 0, nont = 0;
    for (const c of CHANNELS) {
      const ch = VALID.channels?.[c];
      if (!ch || !ch.compares) continue;
      if (ch.trivial >= ch.matches) continue;
      nont++;
      best = Math.max(best, ch.survived ?? 0);
    }
    el.bValid.textContent = nont
      ? `divergence: ${nont} non-trivial channels, best survived ${best} turns`
      : 'divergence: every channel agreement is trivial (walker touched 0 bytes)';
    el.bValid.className = 'badge ' + (best > 0 ? 'on' : 'off');
  } else {
    el.bValid.textContent = 'no divergence row for this file';
    el.bValid.className = 'badge off';
  }
}

// ---------------------------------------------------------------------------
// the flat event list
// ---------------------------------------------------------------------------

/// Flatten every command of every package into parallel typed arrays ordered by the
/// simulation frame the package was stamped with. Typed arrays rather than objects because
/// the largest recording in the corpus carries 352,868 commands.
function buildEvents() {
  let n = 0;
  for (const t of R.turns) for (const p of t.players) n += p.commands.length;
  const ev = {
    n,
    frame: new Int32Array(n), turn: new Int32Array(n), play: new Int8Array(n),
    op: new Uint8Array(n), pkg: new Int32Array(n), off: new Int32Array(n), len: new Int32Array(n),
    bufs: [],
  };
  let i = 0;
  for (const t of R.turns) {
    for (const p of t.players) {
      const b = ev.bufs.push(p.bytes) - 1;
      for (const c of p.commands) {
        ev.frame[i] = p.frame; ev.turn[i] = t.turn; ev.play[i] = p.play;
        ev.op[i] = c.op; ev.pkg[i] = b; ev.off[i] = c.off; ev.len[i] = c.len;
        i++;
      }
    }
  }
  // Records are appended in turn order and `find_stream` already required stamps to be
  // non-decreasing across the chain, so this is nearly sorted; the sort makes it exact
  // without disturbing within-package order.
  const order = Array.from({ length: n }, (_, k) => k)
    .sort((a, b) => (ev.frame[a] - ev.frame[b]) || (a - b));
  const re = (arr, C) => { const o = new C(n); for (let k = 0; k < n; k++) o[k] = arr[order[k]]; return o; };
  ev.frame = re(ev.frame, Int32Array); ev.turn = re(ev.turn, Int32Array);
  ev.play = re(ev.play, Int8Array); ev.op = re(ev.op, Uint8Array);
  ev.pkg = re(ev.pkg, Int32Array); ev.off = re(ev.off, Int32Array); ev.len = re(ev.len, Int32Array);
  EV = ev;
}

function evBytes(i) {
  const b = EV.bufs[EV.pkg[i]];
  return b.subarray(EV.off[i], EV.off[i] + EV.len[i]);
}

// ---------------------------------------------------------------------------
// the stream model — what the viewer can actually apply
// ---------------------------------------------------------------------------

const MAX_MARKERS = 60000;

function resetModel() {
  S = {
    idx: 0,
    frame: R.frameFirst,
    turn: R.turns.length ? R.turns[0].turn : 0,
    players: Array.from({ length: 8 }, () => ({
      seen: false, selection: [], selWho: -1, cam: null, trail: [],
      stance: null, resigned: false, quit: false, orders: 0,
      accum: { clicks: 0, hotkeys: 0, minimap: 0, mainmap: 0, groupsFormed: 0, groupsUsed: 0 },
      lastOp: null, counts: { interpreted: 0, plotted: 0, annotated: 0 },
      feed: [],
    })),
    markers: [], markerHead: 0,
    chat: [], diplo: [],
    gameSpeed: null, paused: false,
    counts: {
      interpreted: 0, plotted: 0, annotated: 0, wasmHandler: 0, total: 0,
      // orders = total minus per-frame bookkeeping; the denominator that means something
      orders: 0, oInterpreted: 0, oPlotted: 0, oAnnotated: 0, oWasmHandler: 0,
      // the engine's own three-way split, from don-replay::wire::classify
      sim: 0, lockstep: 0, presentation: 0,
    },
    opHist: new Int32Array(NUM_OPCODES),
    lastChecksum: null, checksumTurns: 0,
  };
}

function pushMarker(m) {
  if (S.markers.length < MAX_MARKERS) S.markers.push(m);
  else { S.markers[S.markerHead] = m; S.markerHead = (S.markerHead + 1) % MAX_MARKERS; }
}

function wideString(bytes, off, chars) {
  let s = '';
  for (let i = 0; i < chars && off + 2 * i + 1 < bytes.length; i++) {
    const c = bytes[off + 2 * i] | (bytes[off + 2 * i + 1] << 8);
    if (!c) break;
    s += String.fromCharCode(c);
  }
  return s;
}

/// Apply one event to the model. Returns the class the command fell into, which is what
/// the coverage counters are made of — there is no separate table of "supported" opcodes
/// that could drift away from the code that actually handles them.
function applyEvent(i) {
  const op = EV.op[i], play = EV.play[i], frame = EV.frame[i];
  const bytes = evBytes(i);
  const P = S.players[play & 7] ?? S.players[0];
  P.seen = true;
  S.opHist[op]++;
  S.counts.total++;
  if (WASM_HANDLER.has(op)) S.counts.wasmHandler++;
  let cls = 'annotated';
  let text = null;

  const plot = PLOTTED[op];
  if (plot) {
    const d = decodeCmd(bytes);
    const x = d[plot[0]], y = d[plot[1]];
    if (Number.isFinite(x) && Number.isFinite(y)) {
      pushMarker({ x, y, frame, play, kind: plot[2], op });
      P.orders++;
      cls = 'plotted';
      text = `${COMMANDS[op].struct.replace('Command', '')} → ${x},${y}`;
    }
  } else if (INTERPRETED.has(op)) {
    cls = 'interpreted';
    switch (op) {
      case 0x00: {                                   // GroupCommand — the selection
        const d = decodeCmd(bytes);
        // A 3-byte record with num == 0 means "same selection as last time"
        // (CommandPackage::add_group 0x0094bb60).
        if (d.num > 0) { P.selection = d.list; P.selWho = d.who; }
        text = d.num > 0 ? `select ${d.num} obj (who ${d.who})` : 'select — same as last';
        break;
      }
      case 0x02: { const d = decodeCmd(bytes); P.stance = d.stance; text = `stance ${d.stance}`; break; }
      case 0x0c: text = 'halt'; break;
      case 0x34: { const d = decodeCmd(bytes); S.gameSpeed = d.speed; text = `speed = ${d.speed}`; break; }
      case 0x35: text = 'speed up'; break;
      case 0x36: text = 'speed down'; break;
      case 0x4c: { const d = decodeCmd(bytes); S.paused = !!d.state; text = `pause ${d.state}`; break; }
      case 0x48: {                                   // CameraCommand
        const d = decodeCmd(bytes);
        P.cam = { x: d.x_loc, y: d.y_loc, zoom: d.zoom };
        P.trail.push(d.x_loc, d.y_loc);
        if (P.trail.length > 1200) P.trail.splice(0, P.trail.length - 1200);
        text = null;                                 // one per frame; would drown the feed
        break;
      }
      case 0x4f: {                                   // PlayerSpeedCommand — activity accumulators
        const d = decodeCmd(bytes);
        P.accum.clicks += d.accum_clicks | 0;
        P.accum.hotkeys += d.accum_hotkeys | 0;
        P.accum.minimap += d.accum_minimap_clicks | 0;
        P.accum.mainmap += d.accum_mainmap_clicks | 0;
        P.accum.groupsFormed += d.accum_control_groups_formed | 0;
        P.accum.groupsUsed += d.accum_control_groups_activated | 0;
        text = null;
        break;
      }
      case 0x44: {                                   // ChatCommand
        const d = decodeCmd(bytes);
        const msg = wideString(bytes, 17, d.len ?? 0);
        S.chat.push({ frame, play, msg, taunt: d.taunt });
        text = `chat: ${msg}`;
        break;
      }
      case 0x46: { const d = decodeCmd(bytes); P.resigned = true; text = `RESIGN (play ${d.play})`; break; }
      case 0x47: { P.quit = true; text = 'quit'; break; }
      case 0x25: case 0x26: {
        const d = decodeCmd(bytes);
        const what = op === 0x25 ? 'treaty' : 'declare';
        S.diplo.push({ frame, play, text: `${what} ${d.who}→${d.whom} = ${d.treaty}` });
        text = `${what} → ${d.whom} (${d.treaty})`;
        break;
      }
      case 0x2b: { const d = decodeCmd(bytes); text = `tribute ${d.amount} of good ${d.good} → ${d.whom}`; break; }
      case 0x39: {                                   // CheckSumsCommand
        S.checksumTurns++;
        text = null;
        break;
      }
      case 0x22: { const d = decodeCmd(bytes); text = `hotkey group ${d.group}${d.clear ? ' (set)' : ''}`; break; }
      default: text = COMMANDS[op]?.struct?.replace('Command', '') ?? `op 0x${op.toString(16)}`;
    }
  } else {
    // Annotated: decoded, listed, and explicitly not acted on.
    const d = decodeCmd(bytes);
    const parts = [];
    for (const f of COMMANDS[op]?.fields ?? []) {
      if (d[f.name] !== undefined && f.name !== 'queued') parts.push(`${f.name}=${d[f.name]}`);
    }
    text = `${COMMANDS[op]?.struct?.replace('Command', '') ?? '?'} ${parts.slice(0, 4).join(' ')}`;
  }

  P.counts[cls]++;
  S.counts[cls]++;
  S.counts[classify(op)]++;
  if (!BOOKKEEPING.has(op)) {
    S.counts.orders++;
    S.counts[cls === 'interpreted' ? 'oInterpreted' : cls === 'plotted' ? 'oPlotted' : 'oAnnotated']++;
    if (WASM_HANDLER.has(op)) S.counts.oWasmHandler++;
  }
  P.lastOp = op;
  if (text && !FEED_SUPPRESS.has(op)) {
    P.feed.push({ frame, turn: EV.turn[i], op, cls, text });
    if (P.feed.length > 220) P.feed.splice(0, P.feed.length - 220);
  }
  return cls;
}

/// Move the model to simulation frame `f`. Forward is incremental; backward rebuilds from
/// the start, which costs one pass over the event list (a few tens of milliseconds even on
/// the 352k-command recording) and keeps the model exactly reproducible.
function applyTo(f) {
  if (!EV) return;
  if (f < S.frame) resetModel();
  while (S.idx < EV.n && EV.frame[S.idx] <= f) { applyEvent(S.idx); S.idx++; }
  S.frame = f;
  if (S.idx > 0) S.turn = EV.turn[S.idx - 1];
  // The most recent recorded checksum tuple at or before the cursor.
  S.lastChecksum = null;
  for (let k = R.turns.length - 1; k >= 0; k--) {
    const t = R.turns[k];
    const withCs = t.players.filter((p) => p.checksums && p.frame <= f);
    if (withCs.length) {
      let agree = true;
      for (let j = 1; j < withCs.length; j++) {
        for (let c = 0; c < 16; c++) if (withCs[j].checksums[c] !== withCs[0].checksums[c]) agree = false;
      }
      S.lastChecksum = { turn: t.turn, values: withCs[0].checksums, reporters: withCs.length, agree };
      break;
    }
  }
}

// ---------------------------------------------------------------------------
// rendering — the map
// ---------------------------------------------------------------------------

const view = { zoom: 1, panX: 0, panY: 0, drag: null };
const mapCtx = el.map.getContext('2d');
const stripCtx = el.strip.getContext('2d');
let DPR = 1;

function layout() {
  DPR = Math.min(2, window.devicePixelRatio || 1);
  for (const [c, ctx] of [[el.map, mapCtx], [el.strip, stripCtx]]) {
    const r = c.getBoundingClientRect();
    c.width = Math.max(1, Math.round(r.width * DPR));
    c.height = Math.max(1, Math.round(r.height * DPR));
    ctx.setTransform(1, 0, 0, 1, 0, 0);
  }
  draw();
}

function mapTransform() {
  const w = el.map.width, h = el.map.height;
  const ext = worldExtent();
  const s = (Math.min(w, h) * 0.92 / ext) * view.zoom;
  return {
    s,
    ox: w / 2 - (ext / 2) * s + view.panX * DPR,
    oy: h / 2 - (ext / 2) * s + view.panY * DPR,
    ext,
  };
}

/// Decay length for an order marker, in simulation frames. 15 frames = 1 simulation second
/// by the engine's own definition (`Game::seconds++` every 15th frame, 0x005924cf), so
/// this is 40 seconds of game time.
const MARKER_LIFE = 600;

function draw() {
  const ctx = mapCtx, w = el.map.width, h = el.map.height;
  ctx.fillStyle = '#06070b';
  ctx.fillRect(0, 0, w, h);
  if (!R) return;
  const T = mapTransform();
  const X = (x) => T.ox + x * T.s, Y = (y) => T.oy + y * T.s;

  // world box + WCoord-cell grid (768 fine units = 4 tiles)
  ctx.strokeStyle = '#1a1f2b'; ctx.lineWidth = 1;
  const step = 768 * 10;
  ctx.beginPath();
  for (let g = 0; g <= T.ext; g += step) {
    ctx.moveTo(X(g), Y(0)); ctx.lineTo(X(g), Y(T.ext));
    ctx.moveTo(X(0), Y(g)); ctx.lineTo(X(T.ext), Y(g));
  }
  ctx.stroke();
  ctx.strokeStyle = '#39415a'; ctx.lineWidth = 1.5;
  ctx.strokeRect(X(0), Y(0), T.ext * T.s, T.ext * T.s);

  // Camera trail first, under the orders. A solo recording carries one CameraCommand per
  // simulation frame, so this is a literal record of where the player was looking. The
  // line is broken whenever two consecutive samples are more than a screen apart —
  // otherwise a minimap click draws a long straight streak that reads as a movement and is
  // not one.
  if (el.cameraChk.checked) {
    const JUMP = worldExtent() / 6;
    for (let p = 0; p < 8; p++) {
      const P = S.players[p];
      if (!P.seen || !P.cam) continue;
      ctx.strokeStyle = PCOL[p]; ctx.globalAlpha = 0.28; ctx.lineWidth = 1 * DPR;
      ctx.beginPath();
      const t = P.trail, from = Math.max(0, t.length - 1200) & ~1;
      let pen = false;
      for (let k = from; k + 1 < t.length; k += 2) {
        const jump = k > from && (Math.abs(t[k] - t[k - 2]) > JUMP || Math.abs(t[k + 1] - t[k - 1]) > JUMP);
        const x = X(t[k]), y = Y(t[k + 1]);
        if (!pen || jump) { ctx.moveTo(x, y); pen = true; } else ctx.lineTo(x, y);
      }
      ctx.stroke();
      ctx.globalAlpha = 1;
      const x = X(P.cam.x), y = Y(P.cam.y);
      ctx.fillStyle = PCOL[p];
      ctx.beginPath(); ctx.arc(x, y, 3.5 * DPR, 0, 6.2832); ctx.fill();
      ctx.strokeStyle = PCOL[p]; ctx.globalAlpha = 0.55; ctx.lineWidth = 1.4 * DPR;
      ctx.beginPath(); ctx.arc(x, y, 9 * DPR, 0, 6.2832); ctx.stroke();
      ctx.globalAlpha = 1;
    }
  }

  // Order markers. Fresh orders are bright and large; older ones stay on the map at low
  // alpha while `trails` is on, so a whole game's build-out is legible at once.
  const trails = el.trails.checked;
  for (const m of S.markers) {
    const age = S.frame - m.frame;
    if (age < 0) continue;
    const fresh = Math.max(0, 1 - age / MARKER_LIFE);
    let a = fresh;
    if (a <= 0.02) { if (!trails) continue; a = 0.22; }
    const x = X(m.x), y = Y(m.y);
    if (x < -20 || y < -20 || x > w + 20 || y > h + 20) continue;
    ctx.globalAlpha = Math.min(1, a);
    const col = KIND_COL[m.kind] || '#fff';
    const r = (2.5 + 6 * fresh) * DPR;
    if (m.kind === 'build') {
      ctx.fillStyle = col;
      ctx.fillRect(x - r * 0.7, y - r * 0.7, r * 1.4, r * 1.4);
      ctx.strokeStyle = PCOL[m.play & 7]; ctx.lineWidth = 1.4 * DPR;
      ctx.strokeRect(x - r * 1.1, y - r * 1.1, r * 2.2, r * 2.2);
    } else if (m.kind === 'attack') {
      ctx.strokeStyle = col; ctx.lineWidth = 2 * DPR;
      ctx.beginPath();
      ctx.moveTo(x - r, y - r); ctx.lineTo(x + r, y + r);
      ctx.moveTo(x + r, y - r); ctx.lineTo(x - r, y + r);
      ctx.stroke();
    } else {
      ctx.fillStyle = col;
      ctx.beginPath(); ctx.arc(x, y, r * 0.55, 0, 6.2832); ctx.fill();
      ctx.strokeStyle = PCOL[m.play & 7]; ctx.lineWidth = 1.4 * DPR;
      ctx.beginPath(); ctx.arc(x, y, r, 0, 6.2832); ctx.stroke();
    }
  }
  ctx.globalAlpha = 1;

  drawStrip();
  renderOverlay();
  renderSide();
}

function renderOverlay() {
  const secs = Math.floor(S.frame / 15);
  const mm = String(Math.floor(secs / 60)).padStart(2, '0');
  const ss = String(secs % 60).padStart(2, '0');
  const ext = worldExtent();
  const cov = S.counts;
  const pctP = cov.orders ? (100 * (cov.oInterpreted + cov.oPlotted) / cov.orders) : 0;
  el.overlay.innerHTML =
    `<b>frame</b> ${S.frame}   <b>turn</b> ${S.turn}   <b>clock</b> ${mm}:${ss}\n` +
    `<b>world</b> ${ext} fine units = ${ext / 192} tiles (${escapeHtml(catLabel('mapsizes', R.header.settings.map_size))})\n` +
    `<b>orders</b> ${cov.orders} of ${cov.total} commands   acted on ${pctP.toFixed(1)}%   ` +
    `plotted ${cov.oPlotted}   annotated ${cov.oAnnotated}\n` +
    `<b>recorded control</b> speed ${S.gameSpeed ?? 'default'}   paused ${S.paused ? 'yes' : 'no'}` +
    (S.lastChecksum
      ? `\n<b>checksum</b> turn ${S.lastChecksum.turn} · ${S.lastChecksum.reporters} reporter(s) · ` +
        `${S.lastChecksum.reporters > 1 ? (S.lastChecksum.agree ? 'agree' : 'DISAGREE') : 'single'}`
      : '');
  el.clock.innerHTML = `frame <b>${S.frame}</b> / ${R.frameLast}  <span>· turn ${S.turn} · ${mm}:${ss}</span>`;
}

function renderLegend() {
  const rows = [];
  for (const [k, c] of Object.entries(KIND_COL)) rows.push(`<div><i style="background:${c}"></i>${k}</div>`);
  rows.push('<div style="margin-top:4px;color:#8b93a6">ring colour = player</div>');
  for (let p = 0; p < 8; p++) {
    const P = R.header.players.find((x) => x.slot === p && x.used);
    if (P) rows.push(`<div><i style="background:${PCOL[p]}"></i>${escapeHtml(P.name || 'slot ' + p)}</div>`);
  }
  el.legend.innerHTML = rows.join('');
}

// ---------------------------------------------------------------------------
// rendering — the timeline strip
// ---------------------------------------------------------------------------

/// The timeline strip. One row per `CheckSums::check_all` channel, split into two bands:
///
///   upper band — what the **recording** says. Green where every reporting client's tuple
///                for that turn was identical, red where they disagreed (a real retail
///                desync), blue where only one client reported so there is nothing to
///                compare.
///   lower band — what **our simulation** did on this file, from
///                `schema/replay-validation.json`: solid green while our walked channel
///                still matched the recorded value, hatched where the agreement was
///                *trivial* (our walker touched zero bytes — agreeing on nothing is not
///                agreement), and a red stop at `first_divergence_turn`.
///
/// A solo recording carries no `CheckSumsCommand` at all, and the strip says so instead of
/// drawing an empty grid that could be mistaken for "all channels agree".
function drawStrip() {
  const ctx = stripCtx, w = el.strip.width, h = el.strip.height;
  ctx.fillStyle = '#0d0f16'; ctx.fillRect(0, 0, w, h);
  if (!R) return;
  const gut = 104 * DPR;
  const plotW = w - gut - 8 * DPR;
  const f0 = R.frameFirst, f1 = Math.max(R.frameLast, f0 + 1);
  const X = (f) => gut + ((f - f0) / (f1 - f0)) * plotW;
  const axisH = 15 * DPR;
  const densH = 14 * DPR;
  const rowH = Math.max(3 * DPR, (h - axisH - densH - 4 * DPR) / CHANNELS.length);

  ctx.font = `${9.5 * DPR}px ui-monospace, Menlo, monospace`;
  ctx.textBaseline = 'middle';

  // Command density, coloured by the player who issued the first command in that column.
  const cols = Math.max(1, Math.floor(plotW));
  const dens = new Float32Array(cols);
  const densP = new Int8Array(cols).fill(-1);
  for (let i = 0; i < EV.n; i++) {
    if (BOOKKEEPING.has(EV.op[i])) continue;                 // orders only — see BOOKKEEPING
    const c = Math.min(cols - 1, Math.max(0, Math.floor(((EV.frame[i] - f0) / (f1 - f0)) * (cols - 1))));
    dens[c]++;
    if (densP[c] < 0) densP[c] = EV.play[i];
  }
  let dmax = 1;
  for (const v of dens) if (v > dmax) dmax = v;
  ctx.fillStyle = '#8b93a6';
  ctx.fillText('orders/turn', 4 * DPR, densH / 2 + 1 * DPR);
  ctx.fillStyle = '#151925';
  ctx.fillRect(gut, 1 * DPR, plotW, densH - 3 * DPR);
  for (let c = 0; c < cols; c++) {
    if (!dens[c]) continue;
    const v = Math.min(1, Math.log1p(dens[c]) / Math.log1p(dmax));
    const bh = Math.max(1 * DPR, v * (densH - 4 * DPR));
    ctx.fillStyle = PCOL[densP[c] & 7];
    ctx.fillRect(gut + c, densH - 2 * DPR - bh, 1, bh);
  }

  // Recorded per-turn agreement, bucketed to pixel columns so a 30,000-turn file draws in
  // one pass instead of 30,000 fills.
  const agreeCol = new Int8Array(cols);        // 0 none, 1 single reporter, 2 agree, 3 disagree
  let csTurns = 0;
  const turnX = new Map();
  for (const t of R.turns) {
    const withCs = t.players.filter((p) => p.checksums);
    if (!withCs.length) continue;
    csTurns++;
    let agree = true;
    for (let j = 1; j < withCs.length; j++) {
      for (let c = 0; c < 16; c++) if (withCs[j].checksums[c] !== withCs[0].checksums[c]) agree = false;
    }
    const col = Math.min(cols - 1, Math.max(0, Math.floor(((withCs[0].frame - f0) / (f1 - f0)) * (cols - 1))));
    const v = withCs.length < 2 ? 1 : (agree ? 2 : 3);
    if (v > agreeCol[col]) agreeCol[col] = v;
    turnX.set(t.turn, X(withCs[0].frame));
  }
  const AGREE_COL = [null, '#274058', '#1f6b4b', '#ff6b5b'];

  for (let ci = 0; ci < CHANNELS.length; ci++) {
    const y = densH + 2 * DPR + rowH * ci;
    const hh = rowH - 1.5 * DPR;
    ctx.fillStyle = '#8b93a6';
    ctx.fillText(CHANNELS[ci], 4 * DPR, y + hh / 2);
    ctx.fillStyle = '#151925';
    ctx.fillRect(gut, y, plotW, hh);
    const half = hh / 2;
    for (let c = 0; c < cols; c++) {
      if (!agreeCol[c]) continue;
      ctx.fillStyle = AGREE_COL[agreeCol[c]];
      ctx.fillRect(gut + c, y, 1, half);
    }
    const ch = VALID?.channels?.[CHANNELS[ci]];
    if (ch && ch.compares > 0) {
      const dv = ch.first_divergence_turn;
      const stop = dv == null ? gut + plotW : (turnX.get(dv) ?? gut);
      const trivial = ch.trivial >= ch.matches && ch.matches > 0;
      ctx.fillStyle = trivial ? 'rgba(110,231,168,.16)' : 'rgba(110,231,168,.55)';
      ctx.fillRect(gut, y + half, Math.max(0, stop - gut), half);
      if (trivial) {                       // hatch: agreement on an empty channel
        ctx.strokeStyle = 'rgba(110,231,168,.45)'; ctx.lineWidth = 1;
        ctx.beginPath();
        for (let x = gut; x < stop; x += 6 * DPR) { ctx.moveTo(x, y + hh); ctx.lineTo(x + half, y + half); }
        ctx.stroke();
      }
      if (dv != null) { ctx.fillStyle = '#ff6b5b'; ctx.fillRect(stop, y + half, 2 * DPR, half); }
    } else if (VALID) {
      ctx.fillStyle = '#20242f';
      ctx.fillRect(gut, y + hh / 2, plotW, hh / 2);
    }
  }

  // cursor
  const cx = X(S.frame);
  ctx.strokeStyle = '#e7e9f0'; ctx.lineWidth = 1 * DPR;
  ctx.beginPath(); ctx.moveTo(cx, 0); ctx.lineTo(cx, h - axisH); ctx.stroke();

  // axis + legend
  ctx.fillStyle = '#8b93a6';
  const secs = Math.floor(f1 / 15);
  const note = csTurns
    ? `${csTurns.toLocaleString()} turns carry CheckSumsCommand · upper band = recorded clients (green agree / red desync / blue single) · lower = our sim (hatched = trivial)`
    : 'no CheckSumsCommand in this recording (single-player replays carry none) — nothing to compare';
  ctx.fillText(`frame ${f0}`, gut, h - axisH / 2);
  const right = `frame ${f1} · ${Math.floor(secs / 60)}m${String(secs % 60).padStart(2, '0')}s`;
  ctx.fillText(right, w - ctx.measureText(right).width - 4 * DPR, h - axisH / 2);
  const cw = ctx.measureText(note).width;
  ctx.fillText(note, Math.max(gut + 90 * DPR, (w - cw) / 2), h - axisH / 2);
}

// ---------------------------------------------------------------------------
// rendering — the side panels
// ---------------------------------------------------------------------------

const kv = (t, rows) => {
  t.replaceChildren();
  for (const [k, v] of rows.filter(Boolean)) {
    const tr = document.createElement('tr');
    const key = document.createElement('td');
    const value = document.createElement('td');
    key.textContent = String(k);
    value.textContent = String(v);
    tr.append(key, value);
    t.append(tr);
  }
};

function renderFile(ms) {
  const h = R.header;
  kv(el.kvFile, [
    ['file', R.name],
    ['version', h.version ?? '—'],
    ['build word', '0x' + (h.versionWord >>> 0).toString(16)],
    ['seed', `${h.seed} (GameInfo::seed)`],
    ['map/scenario', h.mapName ? h.mapName : '(none — random map)'],
    ['payload', `${R.payloadLen.toLocaleString()} B (gzip ${R.compressedLen.toLocaleString()} B)`],
    ['header ends', '0x' + h.headerEnd.toString(16)],
    ['state blob', `${R.stateBlobLen.toLocaleString()} B — not decoded`],
    ['stream at', '0x' + R.streamStart.toString(16)],
    ['turns', `${R.turns.length.toLocaleString()}  (${R.framesPerTurn ?? '?'} frames/turn median)`],
    ['commands', R.commandCount.toLocaleString()],
    ['decode time', `${ms.toFixed(0)} ms in this browser`],
  ]);
  kv(el.kvDecode, [
    ['packages', `${R.packages.toLocaleString()}`],
    ['decoded', `${R.packagesDecoded.toLocaleString()} (${(100 * R.packagesDecoded / Math.max(1, R.packages)).toFixed(4)}%)`],
    ['structural tiling', `${R.packagesDecoded.toLocaleString()} package payloads`],
    ['framing residue', `${R.framingResidue} bytes`],
    ['XOR key', '0x' + R.xorKey.toString(16).padStart(4, '0') + (R.xorKey ? '' : ' (solo, unobfuscated)')],
    ['key evidence', R.obfuscationSource],
    ['pad seed', R.padSeed === null ? 'none' : '0x' + R.padSeed.toString(16)],
    ['checksum packets', `${R.checksumPackets.toLocaleString()} (${R.checksumTotalOk.toLocaleString()} totals consistent; ${R.checksumShapeOk.toLocaleString()} adler-shaped)`],
    ['crossplay', R.crossplay.comparisons
      ? `${R.crossplay.identical}/${R.crossplay.comparisons} tuples identical`
      : 'single reporter — no comparison'],
    R.anomalies.length ? ['anomalies', R.anomalies.slice(0, 4).join(' · ')] : null,
  ]);
}

function renderSetup() {
  const s = R.header.settings;
  const rows = SETTING_NAMES.map((n) => {
    const cat = SETTING_CATEGORY[n];
    return [n, cat ? catLabel(cat, s[n]) : String(s[n])];
  });
  rows.push(['checksum window', String(R.header.checksumWindowSize)]);
  rows.push(['GameInfo::flags', '0x' + (R.header.flags >>> 0).toString(16)]);
  kv(el.kvSetup, rows);
}

function renderPlayers() {
  el.players.innerHTML = R.header.players.filter((p) => p.used).map((p) => `
    <div style="margin-bottom:8px">
      <div class="pname"><span style="color:${PCOL[p.slot & 7]}">■</span>
        <b>${escapeHtml(p.name || '(unnamed)')}</b> <span style="color:var(--dim)">slot ${p.slot}</span></div>
      <table class="kv">
        <tr><td>nation</td><td>${escapeHtml(tribeName(p.tribe))}</td></tr>
        <tr><td>team / handicap</td><td>${p.team} / ${p.handicap}</td></tr>
        <tr><td>who / play / flags</td><td>${p.who} / ${p.play} / 0x${p.flags.toString(16)}</td></tr>
        <tr><td>difficulty</td><td>${escapeHtml(catLabel('difficulties', p.diff))}</td></tr>
      </table>
      <div class="feed" id="feed${p.slot}"></div>
    </div>`).join('');
}

let lastSideT = 0;
function renderSide() {
  const now = performance.now();
  if (now - lastSideT < 120) return;
  lastSideT = now;

  for (const p of R.header.players) {
    if (!p.used) continue;
    const box = document.getElementById('feed' + p.slot);
    if (!box) continue;
    const P = S.players[p.slot];
    const rows = P.feed.slice(-24).reverse().map((f) =>
      `<div class="${f.cls === 'plotted' ? 'plotted' : f.cls === 'interpreted' ? 'interp' : 'anno'}">` +
      `<span class="op">t${f.turn} 0x${f.op.toString(16).padStart(2, '0')}</span> ${escapeHtml(f.text)}</div>`).join('');
    const sel = P.selection.length ? `<div class="op">selection: ${P.selection.length} objects</div>` : '';
    const st = [P.resigned ? 'RESIGNED' : null, P.quit ? 'quit' : null,
      `clicks ${P.accum.clicks}`, `hotkeys ${P.accum.hotkeys}`].filter(Boolean).join(' · ');
    box.innerHTML = `<div class="op">${st}</div>${sel}${rows || '<div class="op">—</div>'}`;
  }

  const c = S.counts;
  const ord = Math.max(1, c.orders);
  el.covBar.innerHTML =
    `<i style="width:${100 * c.oInterpreted / ord}%;background:#6ee7a8"></i>` +
    `<i style="width:${100 * c.oPlotted / ord}%;background:#5ab7ff"></i>` +
    `<i style="width:${100 * c.oAnnotated / ord}%;background:#3a3f4d"></i>`;
  kv(el.kvCov, [
    ['commands', c.total.toLocaleString()],
    ['— bookkeeping', (c.total - c.orders).toLocaleString() + ' (camera, turn_data, player_speed, checksums)'],
    ['orders', c.orders.toLocaleString()],
    ['interpreted', `${c.oInterpreted.toLocaleString()} (${(100 * c.oInterpreted / ord).toFixed(1)}%)`],
    ['plotted on map', `${c.oPlotted.toLocaleString()} (${(100 * c.oPlotted / ord).toFixed(1)}%)`],
    ['annotated only', `${c.oAnnotated.toLocaleString()} (${(100 * c.oAnnotated / ord).toFixed(1)}%)`],
    ['WASM handler present', `${c.oWasmHandler.toLocaleString()} (${(100 * c.oWasmHandler / ord).toFixed(1)}%) of orders — not run`],
    ['engine class split', `sim ${c.sim.toLocaleString()} · lockstep ${c.lockstep.toLocaleString()} · presentation ${c.presentation.toLocaleString()}`],
  ]);
  el.covNote.innerHTML =
    'The denominator is <b>orders</b>, not commands: 96–99% of a recording is per-frame ' +
    'bookkeeping the engine emits regardless of what the player did, and including it ' +
    'would make any percentage here meaningless. ' +
    '<b>interpreted</b> = the viewer changes state this page shows (selection, speed, ' +
    'pause, camera, activity counters, chat, resignation). <b>plotted</b> = the order carries a world ' +
    'coordinate and is drawn where it was issued. <b>annotated</b> = decoded and listed, ' +
    'nothing else — almost always because the order names an object id and we have no ' +
    'object table. None of these means the simulation ran the order: <code>don-sim</code> ' +
    'has handlers for 4 of 82 opcodes (0x00 0x04 0x07 0x0c, <code>web/wasm/src/real.rs</code>), ' +
    'but this viewer does not load or call that module. World state sits in the undecoded blob before the stream. ' +
    'The class split is <code>don-replay::wire::classify</code>, reproduced verbatim.';

  const hist = [...S.opHist].map((n, op) => [op, n]).filter((e) => e[1]).sort((a, b) => b[1] - a[1]);
  el.opHist.innerHTML = hist.slice(0, 22).map(([op, n]) =>
    `<div>0x${op.toString(16).padStart(2, '0')} ${(COMMANDS[op]?.struct ?? '?').padEnd(24)} ${n}</div>`).join('');

  el.chat.innerHTML = S.chat.slice(-14).reverse().map((m) =>
    `<div><span class="op">f${m.frame}</span> <span style="color:${PCOL[m.play & 7]}">■</span> ${escapeHtml(m.msg)}</div>`)
    .join('') || '<div class="op">—</div>';
}

function escapeHtml(s) {
  return String(s).replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));
}

// ---------------------------------------------------------------------------
// transport
// ---------------------------------------------------------------------------

/// One simulation frame is 67 ms of wall clock at Normal speed
/// (`TurnControl::timings` 0x00AFC4A4 index 2 — *not* 15 Hz, which is the simulation's own
/// definition of a second). 1× here means the recording plays at the speed it was played.
const MS_PER_FRAME = 67;

function tick(t) {
  requestAnimationFrame(tick);
  if (!R) return;
  if (playing) {
    const dt = Math.min(250, t - lastT);
    const adv = (dt / MS_PER_FRAME) * Number(el.speed.value);
    let f = cursor + adv;
    if (f >= R.frameLast) { f = R.frameLast; setPlaying(false); }
    cursor = f;
    el.scrub.value = Math.round(cursor);
    applyTo(Math.floor(cursor));
  }
  lastT = t;
  draw();
}

function setPlaying(v) {
  playing = v;
  el.btnPlay.textContent = v ? '⏸ pause' : '▶ play';
  lastT = performance.now();
}

function stepTurn(dir) {
  const cur = S.turn;
  const i = R.turnIndex.get(cur) ?? 0;
  const j = Math.max(0, Math.min(R.turns.length - 1, i + dir));
  const t = R.turns[j];
  const st = t.players.length ? t.players[0].frame : cursor;
  cursor = st;
  el.scrub.value = Math.round(cursor);
  applyTo(Math.floor(cursor));
}

el.btnPlay.onclick = () => setPlaying(!playing);
el.btnStepF.onclick = () => { setPlaying(false); stepTurn(1); };
el.btnStepB.onclick = () => { setPlaying(false); stepTurn(-1); };
el.btnHome.onclick = () => { setPlaying(false); cursor = R.frameFirst; el.scrub.value = cursor; applyTo(cursor); };
el.scrub.oninput = () => { cursor = Number(el.scrub.value); applyTo(Math.floor(cursor)); };
el.trails.onchange = el.cameraChk.onchange = draw;

document.addEventListener('keydown', (e) => {
  if (!R) return;
  if (e.code === 'Space') { e.preventDefault(); setPlaying(!playing); }
  if (e.code === 'ArrowRight') { setPlaying(false); stepTurn(e.shiftKey ? 10 : 1); }
  if (e.code === 'ArrowLeft') { setPlaying(false); stepTurn(e.shiftKey ? -10 : -1); }
  if (e.code === 'Home') { el.btnHome.click(); }
});

// map pan / zoom
el.map.addEventListener('pointerdown', (e) => {
  view.drag = { x: e.clientX, y: e.clientY, px: view.panX, py: view.panY };
  el.map.setPointerCapture(e.pointerId);
});
el.map.addEventListener('pointermove', (e) => {
  if (!view.drag) return;
  view.panX = view.drag.px + (e.clientX - view.drag.x);
  view.panY = view.drag.py + (e.clientY - view.drag.y);
});
el.map.addEventListener('pointerup', () => { view.drag = null; });
el.map.addEventListener('wheel', (e) => {
  e.preventDefault();
  const k = Math.exp(-e.deltaY * 0.0015);
  view.zoom = Math.max(0.4, Math.min(24, view.zoom * k));
}, { passive: false });
el.strip.addEventListener('pointerdown', (e) => {
  if (!R) return;
  const r = el.strip.getBoundingClientRect();
  const gut = 96, plotW = r.width - gut - 8;
  const f = R.frameFirst + Math.max(0, Math.min(1, (e.clientX - r.left - gut) / plotW)) * (R.frameLast - R.frameFirst);
  setPlaying(false);
  cursor = Math.round(f); el.scrub.value = cursor; applyTo(cursor);
});

// file input
el.openLocal.onclick = () => el.file.click();
el.file.onchange = async () => {
  const f = el.file.files[0];
  if (f) await loadBytes(new Uint8Array(await f.arrayBuffer()), f.name);
};
el.pick.onchange = () => {
  const v = el.pick.value;
  if (v) loadUrl('./data/replays/' + encodeURIComponent(v), v);
};
for (const ev of ['dragenter', 'dragover']) {
  document.addEventListener(ev, (e) => { e.preventDefault(); el.drop.classList.add('on'); });
}
for (const ev of ['dragleave', 'drop']) {
  document.addEventListener(ev, (e) => { e.preventDefault(); if (ev === 'dragleave' && e.relatedTarget) return; el.drop.classList.remove('on'); });
}
document.addEventListener('drop', async (e) => {
  const f = e.dataTransfer?.files?.[0];
  if (f) await loadBytes(new Uint8Array(await f.arrayBuffer()), f.name);
});

window.addEventListener('resize', layout);

// automation surface, mirroring `window.don` on the spectator page
window.donReplay = {
  get replay() { return R; },
  get model() { return S; },
  loadUrl, loadBytes,
  seek(f) { setPlaying(false); cursor = f; el.scrub.value = f; applyTo(f); draw(); },
  play: () => setPlaying(true),
  pause: () => setPlaying(false),
  stats() {
    return R && {
      name: R.name, packages: R.packages, decoded: R.packagesDecoded,
      residue: R.framingResidue, turns: R.turns.length,
      commands: R.commandCount, checksumPackets: R.checksumPackets,
      checksumTotalOk: R.checksumTotalOk, checksumShapeOk: R.checksumShapeOk,
      xorKey: R.xorKey, padSeed: R.padSeed, obfuscationSource: R.obfuscationSource,
      seed: R.header.seed,
      coverage: { ...S.counts },
    };
  },
};

await loadMeta();
layout();
requestAnimationFrame(tick);
const q = new URLSearchParams(location.search).get('file');
if (q) loadUrl('./data/replays/' + encodeURIComponent(q), q);
