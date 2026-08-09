// Main thread. Owns the DOM, the pointer, and nothing else.
//
// The canvas is transferred to the render worker at boot, so after that this thread does no
// drawing and no simulation. That is the point of the split: a long layout pass here cannot
// stall a frame, because frames are not produced here.
//
// # The play path
//
// A click is answered in three hops, and the shape is deliberate:
//
//   1. `pick`  -> the renderer inverts the camera and returns (world, subtile x, subtile y).
//   2. `query` -> the thread that *owns* that world answers a read-only question about it
//                 (which of my units are near this point? what unit is under the cursor?).
//   3. `command` -> that same thread receives **engine wire bytes** encoded here by
//                 `wire.gen.js`, which is generated from `schema/command-wire.json`.
//
// Step 3 is the one that matters. What crosses the boundary is a `GroupCommand` (0x00) or a
// `MoveToCommand` (0x07) laid out byte-for-byte as the engine's own `CommandPackage::process`
// dispatch expects, drained at a tick boundary in arrival order. Nothing about it is
// specific to a human: a policy emits the same bytes, and over a network it is the same
// bytes again.

import { WasmSim } from './wasm.js';
import { CTRL, CTRL_I32, BANKS, planSab, bankOffset, bankBytes } from './proto.js';
import { encode, encodeGroup, decode, COMMANDS } from './wire.gen.js';

const $ = (id) => document.getElementById(id);
const WASM_URL = new URL('../wasm/don_web.wasm', import.meta.url).href;
const DATA_URL = new URL('../data/gamedata.bin', import.meta.url).href;
const NAMES_URL = new URL('../data/gamedata.json', import.meta.url).href;

const log = (msg, cls = '') => {
  const el = $('log');
  el.insertAdjacentHTML('beforeend', `<div class="${cls}">${msg}</div>`);
  el.scrollTop = el.scrollHeight;
};

// ---------------------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------------------

const state = {
  render: null,
  sims: [],
  sab: null,
  plan: null,
  cfg: null,
  probe: null,
  // The default battle occupies the middle third of a world, so start a little zoomed in
  // rather than framing three quarters of empty map.
  cam: { x: 0, y: 0, zoom: 1.8 },
  pointScale: 1,
  running: false,
  statsTimer: 0,
  lastStats: null,
  results: [],
  booted: false,
  backend: null,
  timestamps: false,
  /** The packed unit/balance/rules blob, or null when this checkout has no game data. */
  gameData: null,
  /** Names and roster from the pack, for the inspector. UI only; the sim never sees a string. */
  meta: null,
  /** Selection state, mirrored here only so the panel can report it. */
  selection: { world: 0, count: 0 },
  history: [],
  showAgg: true,
  qid: 1,
  queries: new Map(),
};

/**
 * Surface a worker that dies before it can speak.
 *
 * A module worker whose source fails to parse never runs a line, so it never posts the
 * message the boot sequence is waiting for and the page simply hangs with no error
 * anywhere. `error` fires for load and parse failures; `messageerror` fires when a
 * structured clone fails, which is the other silent one.
 */
function wireWorkerErrors(w, label) {
  w.addEventListener('error', (e) => {
    log(`${label} failed to start: ${e.message || 'load error'} (${e.filename}:${e.lineno})`, 'err');
  });
  w.addEventListener('messageerror', () => log(`${label}: message could not be deserialised`, 'err'));
}

/** Resolve on the next message of a given type from a worker, then unsubscribe. */
function waitFor(worker, type) {
  return new Promise((resolve) => {
    const h = (e) => {
      if (e.data && e.data.type === type) { worker.removeEventListener('message', h); resolve(e.data); }
    };
    worker.addEventListener('message', h);
  });
}

/** Ask the thread that owns `world` a read-only question. */
function query(target, what, args) {
  const qid = state.qid++;
  return new Promise((resolve) => {
    state.queries.set(qid, resolve);
    target.postMessage({ cmd: 'query', qid, what, ...args });
  });
}

/** The worker that owns a world, plus that world's index inside it. */
function ownerOf(world) {
  if (state.cfg.path === 'sab') {
    const s = Math.floor(world / state.cfg.worldsPerShard);
    return { target: state.sims[s], world: world % state.cfg.worldsPerShard };
  }
  return { target: state.render, world };
}

// ---------------------------------------------------------------------------------------
// Boot
// ---------------------------------------------------------------------------------------

async function loadGameData() {
  try {
    const r = await fetch(DATA_URL);
    if (!r.ok) throw new Error(`HTTP ${r.status}`);
    state.gameData = new Uint8Array(await r.arrayBuffer());
    try { state.meta = await (await fetch(NAMES_URL)).json(); } catch { state.meta = null; }
    log(`game data ${(state.gameData.length / 1024).toFixed(0)} KB — ` +
      `${state.meta ? state.meta.unitCount : '?'} unit types, ` +
      `${state.meta ? state.meta.rosterCount : '?'} in roster, 493x493 balance table`, 'ok');
  } catch (e) {
    state.gameData = null;
    log(`no packed game data (${e.message}) — running the SYNTHETIC table.`, 'err');
    log('Build it with: node web/tools/pack-gamedata.mjs (needs schema/live/).', 'err');
  }
}

async function boot() {
  // `?backend=webgl2` forces the fallback. The backend is chosen once, at device creation,
  // so it cannot be switched live -- and a fallback that is never actually run is a
  // fallback that does not work. This is how it gets measured.
  const q = new URLSearchParams(location.search);
  if (q.get('backend')) $('prefer').value = q.get('backend');
  $('iso').textContent = crossOriginIsolated ? 'crossOriginIsolated' : 'NOT isolated (no SAB)';
  $('iso').className = 'badge ' + (crossOriginIsolated ? 'on' : 'off');
  if (!crossOriginIsolated) {
    log('SharedArrayBuffer unavailable: the page is not cross-origin isolated.', 'err');
    log('Serve with COOP: same-origin and COEP: require-corp (web/serve.mjs does).', 'err');
    $('path').querySelector('option[value=sab]').disabled = true;
  }

  await loadGameData();

  // A throwaway wasm instance on the main thread, used only to read constants that live in
  // Rust (MAP_SPAN, the tick period, the capacity clamp that decides `stride`). Duplicating
  // those numbers in JS is exactly the kind of drift that is invisible until it is not.
  state.probe = await WasmSim.load(WASM_URL);
  if (state.gameData) state.probe.loadGameData(state.gameData);
  log(`wasm loaded — map span ${state.probe.mapSpan} subtiles, tick ${state.probe.tickMs} ms ` +
    `(TurnControl::timings, Normal)`, 'ok');

  const canvas = $('c');
  const rect = canvas.getBoundingClientRect();
  canvas.width = Math.round(rect.width * devicePixelRatio);
  canvas.height = Math.round(rect.height * devicePixelRatio);
  const off = canvas.transferControlToOffscreen();

  state.render = new Worker(new URL('./render.worker.js', import.meta.url), { type: 'module' });
  wireWorkerErrors(state.render, 'render worker');
  state.render.addEventListener('message', onRenderMessage);
  const booted = waitFor(state.render, 'booted');
  state.render.postMessage({
    cmd: 'boot', canvas: off, dpr: devicePixelRatio,
    cssW: rect.width, cssH: rect.height, prefer: $('prefer').value,
  }, [off]);
  const b = await booted;
  state.booted = true;
  state.backend = b.backend;
  state.timestamps = b.timestamps;
  $('be').textContent = b.backend;
  $('be').className = 'badge on';
  $('ts').textContent = b.timestamps ? 'gpu timestamps' : 'no gpu timestamps';
  $('ts').className = 'badge ' + (b.timestamps ? 'on' : 'off');
  if (b.shaderMessages && b.shaderMessages.length) {
    for (const m of b.shaderMessages) log(`SHADER ERROR ${m}`, 'err');
  }
  const ad = b.adapter || {};
  log(`backend ${b.backend} — ${[ad.vendor, ad.architecture, ad.device, ad.description].filter(Boolean).join(' / ') || 'adapter details unavailable'}`, 'ok');
}

function onRenderMessage(e) {
  const m = e.data;
  if (m.type === 'stats') { state.lastStats = m; drawHud(m); pushHistory(m); drawAgg(); }
  else if (m.type === 'error') log(`render worker error in ${m.where}: ${m.message}`, 'err');
  else if (m.type === 'picked') onPicked(m);
  else if (m.type === 'query-result') {
    const r = state.queries.get(m.qid);
    if (r) { state.queries.delete(m.qid); r(m); }
  }
}

// ---------------------------------------------------------------------------------------
// Configure / restart
// ---------------------------------------------------------------------------------------

function strideFor(owners, perOwner, capacity) {
  // Mirrors `don_web::Sim::new`'s clamp. Verified against the shard's own report below, so
  // a divergence surfaces as a thrown error rather than as silent corruption.
  const probe = state.probe;
  probe.create(1, owners, perOwner, capacity, 1);
  return probe.stride;
}

async function teardown() {
  state.running = false;
  if (state.render) state.render.postMessage({ cmd: 'pause' });
  for (const w of state.sims) { try { w.terminate(); } catch {} }
  state.sims = [];
  if (state.render) {
    state.render.postMessage({ cmd: 'teardown' });
    await new Promise((r) => setTimeout(r, 10));
  }
}

async function configure(cfg) {
  await teardown();
  const seed = cfg.seed ?? 0xC0FFEE;
  let worlds = Math.max(1, cfg.worlds | 0);
  const owners = Math.min(10, Math.max(2, cfg.owners | 0 || 2));
  const perOwner = Math.max(1, cfg.units | 0);
  const capacity = Math.max(owners * perOwner, cfg.capacity | 0 || owners * perOwner);
  let path = cfg.path || 'inline';
  if (path === 'zerocopy') worlds = 1;
  if (path === 'sab' && !crossOriginIsolated) { log('sab path needs cross-origin isolation; using inline', 'err'); path = 'inline'; }

  const stride = strideFor(owners, perOwner, capacity);
  const shards = path === 'sab' ? Math.max(1, cfg.shards | 0) : 1;
  const worldsPerShard = Math.ceil(worlds / shards);
  if (path === 'sab') worlds = worldsPerShard * shards;

  state.cfg = { worlds, owners, units: perOwner, capacity, path, shards, worldsPerShard,
    stride, seed, simHz: cfg.simHz ?? 15 };
  state.history = [];
  // Keep the controls honest: a sweep or the automation surface can change any of these,
  // and a panel showing a configuration that is not running is worse than no panel.
  $('worlds').value = worlds; $('units').value = perOwner; $('owners').value = owners;
  $('path').value = path; $('shards').value = shards;

  // Each worker gets its own copy of the pack: it is 500 KB, it is read once at startup,
  // and a transfer would leave the other workers with a detached buffer.
  const dataFor = () => (state.gameData ? state.gameData.slice().buffer : null);

  if (path === 'sab') {
    state.plan = planSab(shards, worldsPerShard, stride);
    state.sab = new SharedArrayBuffer(state.plan.total);
    const ctrl = new Int32Array(state.sab, 0, shards * CTRL_I32);
    for (let s = 0; s < shards; s++) {
      ctrl[s * CTRL_I32 + CTRL.PUBLISHED] = -1;
      ctrl[s * CTRL_I32 + CTRL.READER_HELD] = -1;
    }
    log(`SAB ${(state.plan.total / 1048576).toFixed(2)} MiB — ${shards} shards x ${BANKS} banks x ${(bankBytes(worldsPerShard, stride) / 1048576).toFixed(2)} MiB`);

    const readies = [];
    for (let s = 0; s < shards; s++) {
      const w = new Worker(new URL('./sim.worker.js', import.meta.url), { type: 'module' });
      wireWorkerErrors(w, `sim worker ${s}`);
      w.addEventListener('message', onSimMessage);
      state.sims.push(w);
      readies.push(waitFor(w, 'ready'));
      const bankOffsets = [];
      for (let b = 0; b < BANKS; b++) bankOffsets.push(bankOffset(state.plan, s, b, worldsPerShard, stride));
      const buf = dataFor();
      w.postMessage({
        cmd: 'init', wasmUrl: WASM_URL, sab: state.sab, shard: s, shards,
        worldsPerShard, owners, perOwner, capacity, gameData: buf,
        seed: seed ^ (s * 0x9e3779b9), bankOffsets, simHz: state.cfg.simHz,
      }, buf ? [buf] : []);
    }
    const rs = await Promise.all(readies);
    for (const r of rs) {
      if (r.stride !== stride) throw new Error(`stride disagreement: main computed ${stride}, shard ${r.shard} reports ${r.stride}`);
    }
    showDataBadge(rs[0].real, rs[0].balanceDistinct);
    const ready = waitFor(state.render, 'ready');
    state.render.postMessage({
      cmd: 'setup', path: 'sab', sab: state.sab, plan: state.plan, shards,
      worldsPerShard, worlds, stride, mapSpan: rs[0].mapSpan, simHz: state.cfg.simHz,
      statsPerWorld: rs[0].statsPerWorld,
    });
    await ready;
  } else {
    const ready = waitFor(state.render, 'ready');
    const buf = dataFor();
    state.render.postMessage({
      cmd: 'setup', path, wasmUrl: WASM_URL, worlds, owners, perOwner,
      capacity, seed, simHz: state.cfg.simHz, gameData: buf,
    }, buf ? [buf] : []);
    const r = await ready;
    if (r.stride !== stride) throw new Error(`stride disagreement: main computed ${stride}, renderer reports ${r.stride}`);
    showDataBadge(r.real, r.balanceDistinct);
  }

  // Frame the thing that was asked for: one world wants to be filled by its battle, a grid
  // of worlds wants to be entirely on screen. Only overridden when the user has not touched
  // the zoom for this configuration.
  state.cam = { x: 0, y: 0, zoom: worlds > 1 ? 1 : 1.8 };
  $('zoom').value = Math.round(Math.log(state.cam.zoom / 0.2) / Math.log(20000) * 1000);
  $('zoomv').textContent = state.cam.zoom.toFixed(2);
  applyView(); applyCamera(); startStatsPump();
  // `autoplay: false` leaves the world at frame 0. The digest cross-check needs an exact
  // frame count, and a render loop that has already run a few frames makes that impossible.
  play(cfg.autoplay !== false);
  log(`configured: ${worlds} worlds x ${owners} sides x ${perOwner} units ` +
    `(stride ${stride}) via ${path}${path === 'sab' ? ` on ${shards} workers` : ''}`, 'ok');
  return state.cfg;
}

/** Say, on screen, whether this is the derived data or the stand-in. Non-negotiable. */
function showDataBadge(real, distinct) {
  const el = $('data');
  if (real) {
    el.textContent = `real tables (${distinct} balance values)`;
    el.className = 'badge on';
  } else {
    el.textContent = 'SYNTHETIC table — not game data';
    el.className = 'badge bad';
  }
}

function onSimMessage(e) {
  const m = e.data;
  if (m.type === 'error') log(`sim worker ${m.shard}: ${m.message}`, 'err');
  else if (m.type === 'query-result') {
    const r = state.queries.get(m.qid);
    if (r) { state.queries.delete(m.qid); r(m); }
  } else if (m.type === 'stats') {
    // The aggregate view is fed at UI rate, not frame rate: 32 bytes per world through
    // postMessage is nothing at 2 Hz, and putting it in the SAB would have added a fourth
    // synchronisation surface for data nobody reads per frame.
    state.render.postMessage({ cmd: 'refresh-stats', stats: m.stats, worldOffset: m.shard * state.cfg.worldsPerShard });
  }
}

/** Refresh the per-world statistics that drive the aggregate view. Twice a second. */
function startStatsPump() {
  clearInterval(state.statsTimer);
  state.statsTimer = setInterval(() => {
    if (!state.running || !state.cfg) return;
    if (state.cfg.path === 'sab') for (const w of state.sims) w.postMessage({ cmd: 'stats' });
    else state.render.postMessage({ cmd: 'refresh-stats' });
  }, 500);
}

// ---------------------------------------------------------------------------------------
// Play / view / camera
// ---------------------------------------------------------------------------------------

function play(on) {
  state.running = on;
  state.render.postMessage({ cmd: on ? 'run' : 'pause' });
  for (const w of state.sims) w.postMessage({ cmd: on ? 'run' : 'pause' });
  $('playpause').textContent = on ? 'pause' : 'play';
}

function applyView() {
  state.render.postMessage({ cmd: 'view', view: $('view').value, flags: +$('metric').value });
}
function applyCamera() {
  state.render.postMessage({ cmd: 'camera', cam: state.cam, pointScale: state.pointScale });
}
function applySpeed() {
  const hz = +$('hz').value;
  $('hzv').textContent = hz === 0 ? 'max' : hz;
  state.render.postMessage({ cmd: 'speed', simHz: hz, framesPerIter: 1 });
  for (const w of state.sims) w.postMessage({ cmd: 'speed', simHz: hz, framesPerIter: 1 });
  if (state.cfg) state.cfg.simHz = hz;
}

function drawHud(m) {
  const f = (x, d = 2) => (x ?? 0).toFixed(d);
  const total = m.stepMs + m.gatherMs + m.copyMs + m.uploadMs + m.encodeMs;
  const s = m.summary;
  const tick = state.probe ? state.probe.tickMs : 67;
  $('hud').innerHTML =
    `<b>${f(m.fps, 1)} fps</b>   ${m.backend} · ${m.path} · ${m.view}\n` +
    `worlds ${m.worlds}  grid ${m.cols}x${m.rows}  instances ${m.instances.toLocaleString()}\n` +
    `live units ${m.live.toLocaleString()}  sim frame ${m.simFrame.toLocaleString()}  (tick ${tick} ms)\n` +
    (s ? `kills ${s.kills.toLocaleString()}  damage ${s.damage.toLocaleString()}  ` +
         `rounds ${s.rounds.toLocaleString()}  decided ${s.decided}/${s.worlds}\n` : '') +
    `step ${f(m.stepMs)}  gather ${f(m.gatherMs)}  copy ${f(m.copyMs)}\n` +
    `upload ${f(m.uploadMs)}  encode ${f(m.encodeMs)}  gpu ${f(m.gpuMs)}  Σcpu ${f(total)} ms`;
}

// ---------------------------------------------------------------------------------------
// Aggregate statistics panel
//
// The mosaic answers "which world is different"; this answers "what is the cluster doing".
// It is drawn on a 2D canvas on the main thread at the stats rate (2 Hz), never per frame,
// so it cannot compete with the renderer for anything.
// ---------------------------------------------------------------------------------------

function pushHistory(m) {
  if (!m.summary) return;
  state.history.push({ t: performance.now(), ...m.summary, fps: m.fps });
  if (state.history.length > 240) state.history.shift();
}

function drawAgg() {
  const box = $('agg');
  box.style.display = state.showAgg ? 'block' : 'none';
  if (!state.showAgg) return;
  const cv = $('aggc');
  const w = Math.round(box.clientWidth * devicePixelRatio);
  const h = Math.round(box.clientHeight * devicePixelRatio);
  if (!w || !h) return;
  if (cv.width !== w || cv.height !== h) { cv.width = w; cv.height = h; }
  const g = cv.getContext('2d');
  g.clearRect(0, 0, w, h);
  const hist = state.history;
  if (hist.length < 2) return;
  const pad = 8 * devicePixelRatio;

  const series = [
    { key: 'live', label: 'live units', color: '#5ab7ff' },
    { key: 'hits', label: 'army hit points', color: '#6ee7a8' },
    { key: 'kills', label: 'kills (cumulative)', color: '#ffb454' },
    { key: 'rounds', label: 'battles fought', color: '#c39bff' },
  ];
  const cw = (w - pad * 2) / series.length;
  g.font = `${10 * devicePixelRatio}px ui-monospace, Menlo, monospace`;
  series.forEach((s, i) => {
    const x0 = pad + i * cw, y0 = pad + 12 * devicePixelRatio, ch = h - y0 - pad;
    let max = 1;
    for (const p of hist) max = Math.max(max, p[s.key]);
    g.strokeStyle = s.color; g.lineWidth = 1.5 * devicePixelRatio;
    g.beginPath();
    hist.forEach((p, k) => {
      const x = x0 + (k / (hist.length - 1)) * (cw - 10 * devicePixelRatio);
      const y = y0 + ch - (p[s.key] / max) * ch;
      k ? g.lineTo(x, y) : g.moveTo(x, y);
    });
    g.stroke();
    g.fillStyle = '#8b93a6';
    g.fillText(s.label, x0, pad + 8 * devicePixelRatio);
    const last = hist[hist.length - 1][s.key].toLocaleString();
    g.fillStyle = s.color;
    g.fillText(last, x0 + cw - 10 * devicePixelRatio - g.measureText(last).width, pad + 8 * devicePixelRatio);
  });
}

// ---------------------------------------------------------------------------------------
// Pointer: pan, zoom, select, order
// ---------------------------------------------------------------------------------------

function wirePointer() {
  const c = $('c');
  let dragging = false, moved = 0, lx = 0, ly = 0, button = 0;
  c.addEventListener('contextmenu', (e) => e.preventDefault());
  c.addEventListener('pointerdown', (e) => {
    dragging = true; moved = 0; lx = e.clientX; ly = e.clientY; button = e.button;
    c.setPointerCapture(e.pointerId);
  });
  c.addEventListener('pointermove', (e) => {
    if (!dragging || button !== 0) return;
    const r = c.getBoundingClientRect();
    const dx = (e.clientX - lx) / r.width * 2, dy = (e.clientY - ly) / r.height * 2;
    moved += Math.abs(dx) + Math.abs(dy);
    if (moved > 0.02) {
      state.cam.x -= dx / state.cam.zoom; state.cam.y += dy / state.cam.zoom;
      lx = e.clientX; ly = e.clientY;
      applyCamera();
    }
  });
  c.addEventListener('pointerup', (e) => {
    dragging = false;
    if (e.button === 0 && moved > 0.02) return;
    const r = c.getBoundingClientRect();
    const ndc = [((e.clientX - r.left) / r.width) * 2 - 1, 1 - ((e.clientY - r.top) / r.height) * 2];
    const action = e.button === 0 ? 'select' : (e.shiftKey ? 'attack' : 'move');
    // The renderer owns the camera, so it owns the inverse transform.
    state.render.postMessage({ cmd: 'pick', ndc, action });
  });
  c.addEventListener('wheel', (e) => {
    e.preventDefault();
    const k = Math.exp(-e.deltaY * 0.0015);
    state.cam.zoom = Math.max(0.2, Math.min(4000, state.cam.zoom * k));
    $('zoom').value = Math.round(Math.log(state.cam.zoom / 0.2) / Math.log(20000) * 1000);
    $('zoomv').textContent = state.cam.zoom.toFixed(2);
    applyCamera();
  }, { passive: false });
}

const cmdLog = (line) => { $('cmdlog').textContent = line; log(line); };

/**
 * A `GroupCommand` names at most **255** objects, because `num` is an `unsigned char`.
 *
 * That is the packet's own limit, straight out of `schema/command-wire.json`, and it is a
 * fact about the action space rather than a limit of this page: whatever the engine does to
 * select a 400-unit army, it is not one `GroupCommand`. Truncating and saying so is the
 * honest handling; silently sending 400 ids into a `u8` count is not.
 */
function capSelection(ids) {
  if (ids.length <= 255) return ids;
  log(`selection truncated ${ids.length} -> 255: GroupCommand.num is an unsigned char`, 'hi');
  return ids.slice(0, 255);
}

/** Hex dump of a command, so the bytes on the wire are visible rather than asserted. */
const hex = (b) => Array.from(b, (v) => v.toString(16).padStart(2, '0')).join(' ');

/**
 * A click, resolved into engine commands.
 *
 * Selection is a command in this engine (`GroupCommand` 0x00 carries `num`, `who` and a
 * list of 2-byte object indices), so the action space is genuinely *selection then order*,
 * not per-unit orders. This function is the whole shape of that: query which units are
 * near the click, encode them into a real packet, send it, then send the order packet.
 */
async function onPicked(p) {
  const who = +$('who').value;
  const { target, world } = ownerOf(p.world);
  if (!target) return;

  if (p.action === 'select') {
    const radius = (+$('selr').value) * 192;   // tiles -> subtiles
    const [box, near] = await Promise.all([
      query(target, 'pick-box', { world, who, x: p.sx, y: p.sy, radius }),
      query(target, 'nearest', { world, x: p.sx, y: p.sy }),
    ]);
    const ids = capSelection(Array.from(box.ids || []));
    const bytes = encodeGroup(who, ids);
    // Read anything you want to report *before* the transfer: posting with a transfer list
    // detaches the buffer, and a detached Uint8Array reports length 0 — which printed
    // "GroupCommand ... 0 B" for a packet that was in fact 1,027 bytes.
    const note = `${bytes.length} B` + (ids.length <= 4 ? `  [${hex(bytes)}]` : '');
    target.postMessage({ cmd: 'command', world, who, bytes: bytes.buffer }, [bytes.buffer]);
    state.selection = { world: p.world, count: ids.length };
    cmdLog(`0x00 GroupCommand — ${ids.length} units, who=${who}, ${note}`);
    showInspect(near.unit, p);
    return;
  }

  if (p.action === 'move') {
    // `to_x`/`to_y` are honoured. The struct's other seven fields (`set_angle`, `angle`,
    // `orders`, `queued`, `form`, `width`, `disembark`) are written as zero and are *not*
    // modelled — inventing values for them against placeholder movement would be worse than
    // leaving them at zero and saying so.
    const bytes = encode(0x07, { to_x: p.sx, to_y: p.sy });
    target.postMessage({ cmd: 'command', world, who, bytes: bytes.buffer }, [bytes.buffer]);
    cmdLog(`0x07 MoveToCommand — world ${p.world} to_x=${p.sx} to_y=${p.sy} (${COMMANDS[0x07].size} B)`);
    return;
  }

  if (p.action === 'attack') {
    const near = await query(target, 'nearest', { world, x: p.sx, y: p.sy });
    if (!near.unit) return;
    const bytes = encode(0x04, { whom: near.unit.id, ox: -1, ignore: 0 });
    target.postMessage({ cmd: 'command', world, who, bytes: bytes.buffer }, [bytes.buffer]);
    cmdLog(`0x04 AttackCommand — whom=${near.unit.id} (${nameOf(near.unit.typeId)}, owner ${near.unit.owner})`);
    showInspect(near.unit, p);
  }
}

function nameOf(typeId) {
  return (state.meta && state.meta.names && state.meta.names[typeId]) || `type ${typeId}`;
}

function showInspect(u, p) {
  if (!u) { $('inspect').textContent = 'no unit there'; return; }
  const rules = state.meta ? state.meta.rules : null;
  $('inspect').textContent =
    `${nameOf(u.typeId)}   (type id ${u.typeId})\n` +
    `owner ${u.owner}   object id ${u.id}\n` +
    `hits ${u.hits} / ${u.maxHits}\n` +
    `attack ${(u.attackX10 / 10).toFixed(1)}  armor ${u.armor}\n` +
    `world ${p.world}  at ${p.sx},${p.sy} subtiles\n` +
    (rules ? `FLANK_BONUS ${rules.flank_bonus}%  RIVER ${rules.river_modifier}/256` : '');
}

// ---------------------------------------------------------------------------------------
// Benchmarks
// ---------------------------------------------------------------------------------------

async function bench(kind, ms = 3000) {
  const r = waitFor(state.render, 'bench');
  state.render.postMessage({ cmd: 'bench', kind, ms });
  const { result } = await r;
  result.cfg = { ...state.cfg };
  result.dpr = devicePixelRatio;
  result.timestamps = state.timestamps;
  result.realData = !!state.gameData;
  state.results.push(result);
  renderResults();
  return result;
}

function renderResults() {
  const rows = state.results.slice(-14).map((r) => `<tr>
    <td>${r.kind}</td><td>${r.path}</td><td>${r.cfg.worlds}</td>
    <td>${(r.instances ?? 0).toLocaleString()}</td><td>${(r.fps ?? 0).toFixed(1)}</td>
    <td>${(r.stepMs ?? 0).toFixed(2)}</td><td>${(r.uploadMs ?? 0).toFixed(2)}</td>
    <td>${(r.gpuMs ?? 0).toFixed(2)}</td></tr>`).join('');
  $('results').innerHTML = `<table><thead><tr>
    <th>kind</th><th>path</th><th>worlds</th><th>inst</th><th>fps</th>
    <th>step</th><th>upl</th><th>gpu</th></tr></thead><tbody>${rows}</tbody></table>
    <div class="note">step/upload/gpu are milliseconds per frame. gpu is a WebGPU timestamp
    query; 0 means unavailable.</div>`;
}

/** Sweep one axis, returning every measurement. Used by the automated harness. */
async function sweep(axis, values, kind = 'uncapped', ms = 2500, base = {}) {
  const out = [];
  for (const v of values) {
    const cfg = { ...state.cfg, ...base, [axis]: v };
    // Capacity tracks population unless a sweep asks otherwise. Leaving a stale capacity
    // behind would silently change `stride`, and `stride` is the mirror's row pitch — a
    // small world with a huge capacity would move many times the bytes and quietly poison
    // every number in the sweep.
    if (base.capacity === undefined) cfg.capacity = cfg.owners * cfg.units;
    await configure(cfg);
    await new Promise((r) => setTimeout(r, 250));
    const r = await bench(kind, ms);
    out.push(r);
    log(`${axis}=${v}: ${(r.fps ?? 0).toFixed(1)} fps (${(r.instances ?? 0).toLocaleString()} instances)`, 'hi');
  }
  return out;
}

/** Advance the inline simulation by an exact number of frames. */
async function stepFrames(n) {
  const r = waitFor(state.render, 'stepped');
  state.render.postMessage({ cmd: 'step-exact', frames: n });
  return await r;
}

/**
 * Simulation frames per second, summed across shards.
 *
 * This is the number worker-count actually moves, and it is *not* the frame rate: the
 * renderer consumes whatever has been published, so render fps is nearly independent of
 * how fast the sim runs. Measuring shard scaling with fps was measuring the wrong thing.
 */
async function simRate(ms = 2000) {
  if (state.cfg.path !== 'sab') return null;
  const ctrl = new Int32Array(state.sab, 0, state.cfg.shards * CTRL_I32);
  const read = () => { let t = 0; for (let s = 0; s < state.cfg.shards; s++) t += Atomics.load(ctrl, s * CTRL_I32 + CTRL.FRAMES); return t; };
  const a = read(), t0 = performance.now();
  await new Promise((r) => setTimeout(r, ms));
  const b = read(), dt = performance.now() - t0;
  const worldFrames = ((b - a) * state.cfg.worldsPerShard * 1000) / dt;
  const units = state.cfg.owners * state.cfg.units;
  return {
    shards: state.cfg.shards, worlds: state.cfg.worlds, units,
    simFramesPerSec: ((b - a) * 1000) / dt / state.cfg.shards,
    worldStepsPerSec: worldFrames,
    unitStepsPerSec: worldFrames * units,
    // The engine's Normal tick is 67 ms, so a world stepping 14.93 times a second is 1x.
    timesRealTime: (worldFrames / state.cfg.worlds) / (1000 / 67),
    ms: dt,
  };
}

/** PNG of exactly what the renderer drew, as a base64 string. See the worker for why. */
async function snapshot() {
  const r = waitFor(state.render, 'snapshot');
  state.render.postMessage({ cmd: 'snapshot' });
  const { bytes, width, height, nonZero, error } = await r;
  log(`snapshot ${width}x${height}, ${nonZero.toLocaleString()} non-black pixels` +
    (error ? ` — GPU validation: ${error}` : ''), nonZero ? 'ok' : 'err');
  const b = new Uint8Array(bytes);
  let s = '';
  for (let i = 0; i < b.length; i += 0x8000) s += String.fromCharCode.apply(null, b.subarray(i, i + 0x8000));
  return btoa(s);
}

async function digest() {
  const r = waitFor(state.render, 'digest');
  state.render.postMessage({ cmd: 'digest' });
  const d = await r;
  log(`digest after ${d.frames} frames: ${d.digest} (kills ${d.kills}, damage ${d.damage}, live ${d.live})`, 'ok');
  return d;
}

/** Round-trip proof that the codec both sides use is the same codec. */
function wireSelfTest() {
  const m = encode(0x07, { to_x: 12345, to_y: -42, queued: 1, form: 3 });
  const d = decode(m);
  const g = decode(encodeGroup(2, [7, 9, 4095]));
  const ok = d.to_x === 12345 && d.to_y === -42 && d.queued === 1 && d.form === 3 &&
    m.length === COMMANDS[0x07].size && g.num === 3 && g.who === 2 && g.list[2] === 4095;
  log(`wire codec self-test ${ok ? 'passed' : 'FAILED'} — 0x07 is ${m.length} B: ${hex(m)}`, ok ? 'ok' : 'err');
  return ok;
}

// ---------------------------------------------------------------------------------------
// Wiring
// ---------------------------------------------------------------------------------------

async function selectAll() {
  const who = +$('who').value;
  const world = state.selection.world || 0;
  const { target, world: w } = ownerOf(world);
  const span = state.probe.mapSpan;
  const box = await query(target, 'pick-box', { world: w, who, x: span >> 1, y: span >> 1, radius: span });
  const ids = capSelection(Array.from(box.ids || []));
  const bytes = encodeGroup(who, ids);
  const n = bytes.length;
  target.postMessage({ cmd: 'command', world: w, who, bytes: bytes.buffer }, [bytes.buffer]);
  state.selection = { world, count: ids.length };
  cmdLog(`0x00 GroupCommand — ${ids.length} units of player ${who} in world ${world} (${n} B)`);
}

function halt() {
  const who = +$('who').value;
  const { target, world } = ownerOf(state.selection.world || 0);
  const bytes = encode(0x0c);
  const dump = hex(bytes);
  target.postMessage({ cmd: 'command', world, who, bytes: bytes.buffer }, [bytes.buffer]);
  cmdLog(`0x0c HaltCommand — 1 B [${dump}]`);
}

function wireUi() {
  $('apply').onclick = () => configure({
    worlds: +$('worlds').value, owners: +$('owners').value, units: +$('units').value,
    path: $('path').value, shards: +$('shards').value, simHz: +$('hz').value,
  }).catch((e) => log(String(e), 'err'));
  $('view').onchange = applyView;
  $('metric').onchange = applyView;
  $('prefer').onchange = () => log('backend preference applies on reload', 'hi');
  $('playpause').onclick = () => play(!state.running);
  $('recenter').onclick = () => { state.cam = { x: 0, y: 0, zoom: 1 }; $('zoom').value = 0; $('zoomv').textContent = '1.00'; applyCamera(); };
  $('agg-toggle').onclick = () => { state.showAgg = !state.showAgg; drawAgg(); };
  $('zoom').oninput = () => {
    state.cam.zoom = 0.2 * Math.pow(20000, +$('zoom').value / 1000);
    $('zoomv').textContent = state.cam.zoom.toFixed(2); applyCamera();
  };
  $('ps').oninput = () => { state.pointScale = +$('ps').value / 100; $('psv').textContent = state.pointScale.toFixed(1); applyCamera(); };
  $('selr').oninput = () => { $('selrv').textContent = $('selr').value; };
  $('hz').oninput = applySpeed;
  $('sel-all').onclick = () => selectAll().catch((e) => log(String(e), 'err'));
  $('halt').onclick = halt;
  $('b-raf').onclick = () => bench('raf');
  $('b-unc').onclick = () => bench('uncapped');
  $('b-draw').onclick = () => bench('draw-only');
  $('b-sim').onclick = () => bench('sim-only');
  $('sweep-units').onclick = () => sweep('units', [64, 256, 512, 1024, 2048], 'uncapped', 2500, { worlds: 1, owners: 2, path: 'zerocopy' });
  $('sweep-worlds').onclick = () => sweep('worlds', [16, 64, 256, 1024, 4096], 'uncapped', 2500, { units: 32, owners: 2, path: 'inline' });
  $('sweep-paths').onclick = async () => {
    for (const p of ['inline', 'sab']) await sweep('path', [p], 'uncapped', 2500, { worlds: 1024, units: 32, owners: 2 });
  };
  $('digest').onclick = () => digest();
  $('dump').onclick = () => navigator.clipboard.writeText(JSON.stringify(state.results, null, 2));

  addEventListener('resize', () => {
    const r = $('c').getBoundingClientRect();
    state.render.postMessage({ cmd: 'resize', cssW: r.width, cssH: r.height, dpr: devicePixelRatio });
    drawAgg();
  });
}

// ---------------------------------------------------------------------------------------

const ready = (async () => {
  await boot();
  wireUi();
  wirePointer();
  $('zoom').value = Math.round(Math.log(state.cam.zoom / 0.2) / Math.log(20000) * 1000);
  $('zoomv').textContent = state.cam.zoom.toFixed(2);
  wireSelfTest();
  await configure({ worlds: 1, owners: 2, units: 512, path: 'zerocopy', shards: 4, simHz: +document.getElementById('hz').value });
})();

// Automation surface for web/bench.mjs. Deliberately promise-based and DOM-free.
window.don = {
  ready, configure, bench, sweep, digest, play, stepFrames, simRate, wireSelfTest, snapshot,
  get state() {
    return { cfg: state.cfg, backend: state.backend, timestamps: state.timestamps,
             isolated: crossOriginIsolated, realData: !!state.gameData,
             meta: state.meta ? { unitCount: state.meta.unitCount, rosterCount: state.meta.rosterCount, rules: state.meta.rules } : null };
  },
  get results() { return state.results; },
  get stats() { return state.lastStats; },
  hardware: { cores: navigator.hardwareConcurrency, dpr: devicePixelRatio, ua: navigator.userAgent },
};
