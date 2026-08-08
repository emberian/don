// Main thread. Owns the DOM, the pointer, and nothing else.
//
// The canvas is transferred to the render worker at boot, so after that this thread does no
// drawing and no simulation. That is the point of the split: a long layout pass or a React
// -style re-render here cannot stall a frame, because frames are not produced here.
//
// Everything the automated harness needs is on `window.don`, promise-based, so the CDP
// driver in `web/bench.mjs` can run a sweep without scraping the DOM.

import { WasmSim, ORDER } from './wasm.js';
import { CTRL, CTRL_I32, BANKS, planSab, bankOffset, bankBytes } from './proto.js';

const $ = (id) => document.getElementById(id);
const WASM_URL = new URL('../wasm/don_web.wasm', import.meta.url).href;

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
  cam: { x: 0, y: 0, zoom: 1 },
  pointScale: 1,
  running: false,
  statsTimer: 0,
  lastStats: null,
  results: [],
  booted: false,
  backend: null,
  timestamps: false,
};

/**
 * Surface a worker that dies before it can speak.
 *
 * A module worker whose source fails to parse never runs a line, so it never posts the
 * message the boot sequence is waiting for and the page simply hangs with no error
 * anywhere. (A stray backtick inside a WGSL template literal did exactly this, and cost
 * more time than it should have.) `error` fires for load and parse failures;
 * `messageerror` fires when a structured clone fails, which is the other silent one.
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

// ---------------------------------------------------------------------------------------
// Boot
// ---------------------------------------------------------------------------------------

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

  // A throwaway wasm instance on the main thread, used only to read constants that live in
  // Rust (MAP_SPAN, TICK_HZ, and the capacity clamp that decides `stride`). Duplicating
  // those numbers in JS is exactly the kind of drift that is invisible until it is not.
  state.probe = await WasmSim.load(WASM_URL);
  log(`wasm loaded — map span ${state.probe.mapSpan} subtiles, tick ${state.probe.tickHz} Hz`, 'ok');

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
  const ad = b.adapter || {};
  log(`backend ${b.backend} — ${[ad.vendor, ad.architecture, ad.device, ad.description].filter(Boolean).join(' / ') || 'adapter details unavailable'}`, 'ok');
}

function onRenderMessage(e) {
  const m = e.data;
  if (m.type === 'stats') { state.lastStats = m; drawHud(m); }
  else if (m.type === 'error') log(`render worker error in ${m.where}: ${m.message}`, 'err');
  else if (m.type === 'picked') issueOrder(m);
}

// ---------------------------------------------------------------------------------------
// Configure / restart
// ---------------------------------------------------------------------------------------

function strideFor(units, capacity) {
  // Mirrors `don_web::Sim::new`'s clamp. Verified against the shard's own report below, so
  // a divergence surfaces as a thrown error rather than as silent corruption.
  const probe = state.probe;
  probe.create(1, Math.min(units, 4096), Math.min(capacity, 4096), 1);
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
  const units = Math.max(1, cfg.units | 0);
  const capacity = Math.max(units, cfg.capacity | 0 || units);
  let path = cfg.path || 'inline';
  if (path === 'zerocopy') worlds = 1;
  if (path === 'sab' && !crossOriginIsolated) { log('sab path needs cross-origin isolation; using inline', 'err'); path = 'inline'; }

  const stride = strideFor(units, capacity);
  let shards = path === 'sab' ? Math.max(1, cfg.shards | 0) : 1;
  let worldsPerShard = Math.ceil(worlds / shards);
  if (path === 'sab') worlds = worldsPerShard * shards;

  state.cfg = { worlds, units, capacity, path, shards, worldsPerShard, stride, seed, simHz: cfg.simHz ?? 15 };
  // Keep the controls honest: a sweep or the automation surface can change any of these,
  // and a panel showing a configuration that is not running is worse than no panel.
  $('worlds').value = worlds; $('units').value = units;
  $('path').value = path; $('shards').value = shards;

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
      w.postMessage({
        cmd: 'init', wasmUrl: WASM_URL, sab: state.sab, shard: s, shards,
        worldsPerShard, unitsPerWorld: units, capacity,
        seed: seed ^ (s * 0x9e3779b9), bankOffsets, simHz: state.cfg.simHz,
      });
    }
    const rs = await Promise.all(readies);
    for (const r of rs) {
      if (r.stride !== stride) throw new Error(`stride disagreement: main computed ${stride}, shard ${r.shard} reports ${r.stride}`);
    }
    const ready = waitFor(state.render, 'ready');
    state.render.postMessage({
      cmd: 'setup', path: 'sab', sab: state.sab, plan: state.plan, shards,
      worldsPerShard, worlds, stride, mapSpan: rs[0].mapSpan, simHz: state.cfg.simHz,
    });
    await ready;
  } else {
    const ready = waitFor(state.render, 'ready');
    state.render.postMessage({
      cmd: 'setup', path, wasmUrl: WASM_URL, worlds, unitsPerWorld: units,
      capacity, seed, simHz: state.cfg.simHz,
    });
    const r = await ready;
    if (r.stride !== stride) throw new Error(`stride disagreement: main computed ${stride}, renderer reports ${r.stride}`);
  }

  applyView(); applyCamera(); startStatsPump();
  // `autoplay: false` leaves the world at frame 0. The digest cross-check needs an exact
  // frame count, and a render loop that has already run a few frames makes that impossible.
  play(cfg.autoplay !== false);
  log(`configured: ${worlds} worlds x ${units} units (stride ${stride}) via ${path}${path === 'sab' ? ` on ${shards} workers` : ''}`, 'ok');
  return state.cfg;
}

function onSimMessage(e) {
  const m = e.data;
  if (m.type === 'error') log(`sim worker ${m.shard}: ${m.message}`, 'err');
  else if (m.type === 'stats') {
    // The aggregate view is fed at UI rate, not frame rate: 16 bytes per world through
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
  // flags bit 0 selects the aggregate metric. "population" is live units over capacity;
  // "state fingerprint" is the low bits of that world's `World::digest()`, which makes two
  // worlds in identical states the same colour and a diverging one visibly change — the
  // metric a spectator of a *cluster* actually wants, and the one that is not degenerate
  // when every world happens to be full.
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
  $('hud').innerHTML =
    `<b>${f(m.fps, 1)} fps</b>   ${m.backend} · ${m.path} · ${m.view}\n` +
    `worlds ${m.worlds}  grid ${m.cols}x${m.rows}  instances ${m.instances.toLocaleString()}\n` +
    `live units ${m.live.toLocaleString()}  sim frame ${m.simFrame.toLocaleString()}\n` +
    `step ${f(m.stepMs)}  gather ${f(m.gatherMs)}  copy ${f(m.copyMs)}\n` +
    `upload ${f(m.uploadMs)}  encode ${f(m.encodeMs)}  gpu ${f(m.gpuMs)}  Σcpu ${f(total)} ms`;
}

// ---------------------------------------------------------------------------------------
// Pointer: pan, zoom, and the order path
// ---------------------------------------------------------------------------------------

function wirePointer() {
  const c = $('c');
  let dragging = false, moved = 0, lx = 0, ly = 0;
  c.addEventListener('pointerdown', (e) => { dragging = true; moved = 0; lx = e.clientX; ly = e.clientY; c.setPointerCapture(e.pointerId); });
  c.addEventListener('pointermove', (e) => {
    if (!dragging) return;
    const r = c.getBoundingClientRect();
    const dx = (e.clientX - lx) / r.width * 2, dy = (e.clientY - ly) / r.height * 2;
    moved += Math.abs(dx) + Math.abs(dy);
    state.cam.x -= dx / state.cam.zoom; state.cam.y += dy / state.cam.zoom;
    lx = e.clientX; ly = e.clientY;
    applyCamera();
  });
  c.addEventListener('pointerup', (e) => {
    dragging = false;
    if (moved > 0.02) return;
    const r = c.getBoundingClientRect();
    const ndc = [((e.clientX - r.left) / r.width) * 2 - 1, 1 - ((e.clientY - r.top) / r.height) * 2];
    // The renderer owns the camera, so it owns the inverse transform.
    state.render.postMessage({ cmd: 'pick', ndc });
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

/**
 * A click becomes an order. This is the whole drop-in-to-play path in one function: the
 * pointer produces a *command record*, the command record goes to whichever thread owns
 * that world, and it is drained at a tick boundary. Nothing about it is specific to a
 * human — an RL policy emits the same record.
 */
function issueOrder(p) {
  const span = state.probe.mapSpan;
  const order = {
    // Opcode 0x07 is `MoveToCommand` in the engine's own command table. The record below
    // carries its `to_x`/`to_y` and nothing else; the seven other fields of the real struct
    // (`set_angle`, `angle`, `orders`, `queued`, `form`, `width`, `disembark`) have no
    // meaning against placeholder mechanics, and inventing values for them would be worse
    // than leaving them out.
    cmd: 'order', kind: ORDER.MOVE_TO, world: p.world, owner: 0,
    sx: span >> 1, sy: span >> 1, tx: p.sx, ty: p.sy, radius: span,
  };
  if (state.cfg.path === 'sab') {
    const w = state.sims[p.shard];
    if (w) w.postMessage({ ...order, world: p.worldInShard });
  } else {
    state.render.postMessage(order);
  }
  log(`order 0x07 MoveToCommand — world ${p.world} to_x=${p.sx} to_y=${p.sy} subtiles`);
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
  state.results.push(result);
  renderResults();
  return result;
}

function renderResults() {
  const rows = state.results.slice(-14).map((r) => `<tr>
    <td>${r.kind}</td><td>${r.path}</td><td>${r.cfg.worlds}</td>
    <td>${r.instances.toLocaleString()}</td><td>${r.fps.toFixed(1)}</td>
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
    // 128-unit world with a 4096 capacity would move 32x the bytes and quietly poison
    // every number in the sweep.
    if (base.capacity === undefined) cfg.capacity = cfg.units;
    await configure(cfg);
    await new Promise((r) => setTimeout(r, 250));
    const r = await bench(kind, ms);
    out.push(r);
    log(`${axis}=${v}: ${r.fps.toFixed(1)} fps (${r.instances.toLocaleString()} instances)`, 'hi');
  }
  return out;
}

/** Advance the inline simulation by an exact number of frames. Only meaningful on the
 *  inline/zerocopy paths; the sab path's shards run free by design. */
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
  return {
    shards: state.cfg.shards, worlds: state.cfg.worlds, units: state.cfg.units,
    simFramesPerSec: ((b - a) * 1000) / dt / state.cfg.shards,
    worldStepsPerSec: worldFrames,
    unitStepsPerSec: worldFrames * state.cfg.units,
    ms: dt,
  };
}

async function digest() {
  const r = waitFor(state.render, 'digest');
  state.render.postMessage({ cmd: 'digest' });
  const d = await r;
  log(`digest after ${d.frames} frames: ${d.digest}`, 'ok');
  return d;
}

// ---------------------------------------------------------------------------------------
// Wiring
// ---------------------------------------------------------------------------------------

function wireUi() {
  $('apply').onclick = () => configure({
    worlds: +$('worlds').value, units: +$('units').value, capacity: +$('units').value,
    path: $('path').value, shards: +$('shards').value, simHz: +$('hz').value,
  }).catch((e) => log(String(e), 'err'));
  $('view').onchange = applyView;
  $('metric').onchange = applyView;
  $('prefer').onchange = () => log('backend preference applies on reload', 'hi');
  $('playpause').onclick = () => play(!state.running);
  $('recenter').onclick = () => { state.cam = { x: 0, y: 0, zoom: 1 }; $('zoom').value = 0; $('zoomv').textContent = '1.00'; applyCamera(); };
  $('zoom').oninput = () => {
    state.cam.zoom = 0.2 * Math.pow(20000, +$('zoom').value / 1000);
    $('zoomv').textContent = state.cam.zoom.toFixed(2); applyCamera();
  };
  $('ps').oninput = () => { state.pointScale = +$('ps').value / 100; $('psv').textContent = state.pointScale.toFixed(1); applyCamera(); };
  $('hz').oninput = applySpeed;
  $('b-raf').onclick = () => bench('raf');
  $('b-unc').onclick = () => bench('uncapped');
  $('b-draw').onclick = () => bench('draw-only');
  $('sweep-units').onclick = () => sweep('units', [128, 512, 1024, 2048, 4096], 'uncapped', 2500, { worlds: 1, path: 'zerocopy' });
  $('sweep-worlds').onclick = () => sweep('worlds', [16, 64, 256, 1024, 4096], 'uncapped', 2500, { units: 64, path: 'inline' });
  $('sweep-paths').onclick = async () => {
    for (const p of ['inline', 'sab']) await sweep('path', [p], 'uncapped', 2500, { worlds: 1024, units: 64 });
  };
  $('digest').onclick = () => digest();
  $('dump').onclick = () => navigator.clipboard.writeText(JSON.stringify(state.results, null, 2));

  addEventListener('resize', () => {
    const r = $('c').getBoundingClientRect();
    state.render.postMessage({ cmd: 'resize', cssW: r.width, cssH: r.height, dpr: devicePixelRatio });
  });
}

// ---------------------------------------------------------------------------------------

const ready = (async () => {
  await boot();
  wireUi();
  wirePointer();
  await configure({ worlds: 1, units: 2048, capacity: 2048, path: 'zerocopy', shards: 4, simHz: 15 });
})();

// Automation surface for web/bench.mjs. Deliberately promise-based and DOM-free.
window.don = {
  ready, configure, bench, sweep, digest, play, stepFrames, simRate,
  get state() { return { cfg: state.cfg, backend: state.backend, timestamps: state.timestamps, isolated: crossOriginIsolated }; },
  get results() { return state.results; },
  get stats() { return state.lastStats; },
  hardware: { cores: navigator.hardwareConcurrency, dpr: devicePixelRatio, ua: navigator.userAgent },
};
