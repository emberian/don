// The renderer. Owns the OffscreenCanvas and the GPU device; the main thread never touches
// either, so a slow DOM update or a long main-thread task cannot drop a frame here.
//
// # Three data paths, deliberately kept side by side
//
//   `zerocopy` — one world, sim hosted in this worker. The three vertex buffers are filled
//                straight from that world's own `pos_x`/`pos_y`/`tag` allocations. CPU-side
//                copies before the upload: 0.
//   `inline`   — many worlds, sim hosted in this worker. Three `memcpy`s per world into the
//                cluster mirror (`sim_gather`), then upload. CPU-side copies: 1.
//   `sab`      — many worlds across N sim workers. Each shard also copies wasm memory into
//                shared memory. CPU-side copies: 2, and the sim runs on other cores.
//
// They exist together because the interesting question is not "is it fast" but "what does
// each copy actually cost, and at what scale does moving the sim off this thread start to
// pay for the extra copy". The bench answers that with numbers rather than intuition.
//
// # Why the tag column is uploaded every frame now
//
// It carries hit points. In the placeholder simulation the tag held only owner and
// occupancy, so it changed once per population change and rode a dirty flag; with real
// combat every hit changes a unit's colour. That is a third full-width upload per frame and
// it is measured (see the track report) rather than assumed away.

import { WasmSim } from './wasm.js';
import { WebGpuBackend } from './webgpu.js';
import { WebGl2Backend } from './webgl2.js';
import { CTRL, CTRL_I32, bankLayout } from './proto.js';

let backend = null;
let canvas = null;
let dpr = 1, cssW = 800, cssH = 600;

let path = 'inline';          // 'zerocopy' | 'inline' | 'sab'
let viewMode = 'auto';        // 'auto' | 'units' | 'aggregate'
let cam = { x: 0, y: 0, zoom: 1 };
let pointScale = 1;
let flags = 0;

let sim = null;               // inline/zerocopy: our own WasmSim
let simHz = 0, framesPerIter = 1, accum = 0;

let sab = null, ctrl = null, sabI32 = null, sabU32 = null;
let shards = 0, worldsPerShard = 0, stride = 0, mapSpan = 49152;
let plan = null;
let worldsTotal = 0, cells = 0, statsPerWorld = 8;

let running = false;
let frameCount = 0, lastFpsT = 0, fps = 0;
let uploadMs = 0, encodeMs = 0, stepMs = 0, gatherMs = 0;
/** Per-world stats mirror kept on this thread, so the aggregate ramps can be normalised
 *  against the cluster's own maxima instead of a magic constant. */
let statsCpu = null;
let statsMax = { hits: 1, kills: 1 };

function grid() {
  const aspect = (cssW * dpr) / Math.max(1, cssH * dpr);
  let cols = Math.max(1, Math.round(Math.sqrt(worldsTotal * aspect)));
  cols = Math.min(cols, worldsTotal);
  const rows = Math.ceil(worldsTotal / cols);
  return { cols, rows };
}

function resolveView(g) {
  const cellPx = (cssW * dpr * cam.zoom) / g.cols;
  if (viewMode === 'units') return 'units';
  if (viewMode === 'aggregate') return 'aggregate';
  // A world is 256 tiles across. Below ~18 device pixels a cell has under a fourteenth of a
  // pixel per tile, so individual units cannot be told apart no matter how they are drawn —
  // at that scale the aggregate view is not a fallback, it is the correct visualisation.
  return cellPx < 18 ? 'aggregate' : 'units';
}

function pointPx(g) {
  const cellPx = (cssW * dpr * cam.zoom) / g.cols;
  // A world is 256 tiles across; make a unit roughly two tiles, clamped so it never
  // disappears and never becomes a blob when zoomed in.
  return Math.max(1, Math.min(20, (cellPx / 256) * 2)) * pointScale;
}

// ---------------------------------------------------------------------------------------
// Uploads
// ---------------------------------------------------------------------------------------

function uploadInline() {
  const t0 = performance.now();
  if (path === 'zerocopy') {
    const live = sim.worldLive(0);
    // These three views alias the world's own columns. Nothing was copied to build them and
    // nothing is copied to read them; `writeBuffer` reads the simulation itself.
    backend.writeColumn('x', 0, sim.worldX(0), 0, live);
    backend.writeColumn('y', 0, sim.worldY(0), 0, live);
    backend.writeColumn('tag', 0, sim.worldTag(0), 0, stride);
  } else {
    backend.writeColumn('x', 0, sim.mirrorX(), 0, cells);
    backend.writeColumn('y', 0, sim.mirrorY(), 0, cells);
    backend.writeColumn('tag', 0, sim.mirrorTag(), 0, cells);
  }
  uploadMs = performance.now() - t0;
}

function uploadSab() {
  const t0 = performance.now();
  const lay = bankLayout(worldsPerShard, stride);
  for (let s = 0; s < shards; s++) {
    const base = s * CTRL_I32;
    const b = Atomics.load(ctrl, base + CTRL.PUBLISHED);
    if (b < 0) continue;
    // Claim the bank so the shard's writer will not choose it while this upload reads it.
    Atomics.store(ctrl, base + CTRL.READER_HELD, b);
    const off = plan.ctrlBytes + s * plan.shardBytes + b * (lay.cells * 12);
    const dst = s * worldsPerShard * stride;
    backend.writeColumn('x', dst, sabI32, (off + lay.xOff) >> 2, lay.cells);
    backend.writeColumn('y', dst, sabI32, (off + lay.yOff) >> 2, lay.cells);
    backend.writeColumn('tag', dst, sabU32, (off + lay.tagOff) >> 2, lay.cells);
    Atomics.store(ctrl, base + CTRL.READER_HELD, -1);
  }
  uploadMs = performance.now() - t0;
}

/** Sim-side timings for the current path. In `sab` mode the sim runs in other workers, so
 *  the only truthful source is their control blocks — reading the inline variables there
 *  reports whatever the *previous* inline configuration happened to leave behind, which is
 *  exactly the bug this function exists to make impossible. */
function simTimings() {
  if (path === 'sab') {
    const sh = aggregateShardCtrl();
    return sh ? { step: sh.stepMs, gather: sh.gatherMs, copy: sh.copyMs } : { step: 0, gather: 0, copy: 0 };
  }
  return { step: stepMs, gather: gatherMs, copy: 0 };
}

function stepInline(dtMs) {
  if (!sim) return;
  const t0 = performance.now();
  if (simHz > 0) {
    accum += (dtMs / 1000) * simHz;
    const n = Math.min(240, Math.floor(accum));
    if (n > 0) { accum -= n; sim.step(n); }
  } else {
    sim.step(framesPerIter);
  }
  const t1 = performance.now();
  if (path !== 'zerocopy') sim.gather();
  const t2 = performance.now();
  stepMs = t1 - t0; gatherMs = t2 - t1;
}

function renderOnce() {
  const g = grid();
  const view = resolveView(g);
  const base = {
    gridCols: g.cols, gridRows: g.rows, stride, mapSpan,
    panX: cam.x, panY: cam.y, zoom: cam.zoom, pointPx: pointPx(g),
    vw: canvas.width, vh: canvas.height, worlds: worldsTotal, flags,
    hpScale: statsMax.hits, killScale: statsMax.kills,
  };
  backend.setUniform({ ...base, plate: 1, inset: 0.04 }, 0);
  // Slot 1 is the per-world plate drawn beneath the units. Without it a grid of thousands
  // of worlds renders as one undifferentiated field of dots — technically correct, visually
  // useless, and the failure mode a screenshot catches and a frame counter never does.
  const many = worldsTotal > 1;
  if (many) backend.setUniform({ ...base, plate: 0.09, inset: 0.035 }, 1);
  const t0 = performance.now();
  if (view === 'aggregate') {
    backend.draw('aggregate', worldsTotal);
  } else {
    backend.draw(many ? 'cluster' : 'units', path === 'zerocopy' ? sim.worldLive(0) : cells);
  }
  encodeMs = performance.now() - t0;
  return view;
}

// ---------------------------------------------------------------------------------------
// Frame loop
// ---------------------------------------------------------------------------------------

let lastFrameT = 0;

function frame(t) {
  if (!running) return;
  const dt = lastFrameT ? t - lastFrameT : 16.7;
  lastFrameT = t;
  if (path === 'sab') { uploadSab(); } else { stepInline(dt); uploadInline(); }
  renderOnce();
  frameCount++;
  if (t - lastFpsT >= 500) {
    fps = (frameCount * 1000) / (t - lastFpsT);
    frameCount = 0; lastFpsT = t;
    postStats();
  }
  requestAnimationFrame(frame);
}

function aggregateShardCtrl() {
  if (!ctrl) return null;
  let live = 0, simFrame = 0, step = 0, gather = 0, copy = 0, frames = 0;
  for (let s = 0; s < shards; s++) {
    const b = s * CTRL_I32;
    live += Atomics.load(ctrl, b + CTRL.LIVE);
    simFrame = Math.max(simFrame, Atomics.load(ctrl, b + CTRL.SIM_FRAME));
    step = Math.max(step, Atomics.load(ctrl, b + CTRL.STEP_US));
    gather = Math.max(gather, Atomics.load(ctrl, b + CTRL.GATHER_US));
    copy = Math.max(copy, Atomics.load(ctrl, b + CTRL.COPY_US));
    frames += Atomics.load(ctrl, b + CTRL.FRAMES);
  }
  return { live, simFrame, stepMs: step / 1000, gatherMs: gather / 1000, copyMs: copy / 1000, frames };
}

/** Cluster-wide roll-up of the per-world stats table, for the statistics panel. */
function clusterSummary() {
  if (!statsCpu) return null;
  let live = 0, hits = 0, kills = 0, damage = 0, rounds = 0, decided = 0;
  let maxH = 1, maxK = 1;
  const n = Math.min(worldsTotal, (statsCpu.length / statsPerWorld) | 0);
  for (let w = 0; w < n; w++) {
    const o = w * statsPerWorld;
    live += statsCpu[o];
    hits += statsCpu[o + 1];
    kills += statsCpu[o + 4];
    damage += statsCpu[o + 5];
    rounds += statsCpu[o + 6];
    if (statsCpu[o + 7] < 2) decided++;
    if (statsCpu[o + 1] > maxH) maxH = statsCpu[o + 1];
    if (statsCpu[o + 4] > maxK) maxK = statsCpu[o + 4];
  }
  statsMax = { hits: maxH, kills: maxK };
  return { worlds: n, live, hits, kills, damage, rounds, decided };
}

function postStats() {
  const g = grid();
  const sh = aggregateShardCtrl();
  self.postMessage({
    type: 'stats',
    backend: backend.constructor.name,
    path, view: resolveView(g), fps,
    worlds: worldsTotal, stride, instances: path === 'zerocopy' ? sim.worldLive(0) : cells,
    live: sh ? sh.live : (sim ? sim.liveTotal : 0),
    simFrame: sh ? sh.simFrame : (sim ? sim.frame : 0),
    stepMs: sh ? sh.stepMs : stepMs,
    gatherMs: sh ? sh.gatherMs : gatherMs,
    copyMs: sh ? sh.copyMs : 0,
    uploadMs, encodeMs,
    gpuMs: backend.lastGpuNs / 1e6,
    cols: g.cols, rows: g.rows,
    zoom: cam.zoom,
    summary: clusterSummary(),
    kills: sim ? sim.kills : 0,
    damage: sim ? sim.damage : 0,
    real: sim ? sim.isReal : null,
  });
}

// ---------------------------------------------------------------------------------------
// Benchmark
// ---------------------------------------------------------------------------------------

/**
 * Median of the GPU-timestamp samples collected during a run.
 *
 * `backend.lastGpuNs` is whatever query pair resolved most recently — resolution is async,
 * so reading it once at the end of a run reports a single frame chosen arbitrarily, and two
 * runs of identical work can differ by 2x on clock state alone. Sampling every iteration
 * and taking the median is not free precision, but it is an actual distribution instead of
 * one draw from it.
 */
function medianOf(xs) {
  const s = xs.filter((x) => x > 0).sort((a, b) => a - b);
  if (!s.length) return 0;
  const h = s.length >> 1;
  return s.length % 2 ? s[h] : (s[h - 1] + s[h]) / 2;
}

/** Frames the display-synchronised loop actually achieves, i.e. what a spectator sees. */
async function benchRaf(ms) {
  return new Promise((resolve) => {
    let n = 0, t0 = 0, upl = 0, enc = 0, stp = 0, gat = 0, cpy = 0; const gpu = [];
    const tick = (t) => {
      if (!t0) t0 = t;
      if (path === 'sab') uploadSab(); else { stepInline(16.7); uploadInline(); }
      renderOnce();
      { const s = simTimings(); stp += s.step; gat += s.gather; cpy += s.copy; }
      upl += uploadMs; enc += encodeMs; n++; gpu.push(backend.lastGpuNs / 1e6);
      if (t - t0 >= ms) {
        resolve({ kind: 'raf', frames: n, ms: t - t0, fps: (n * 1000) / (t - t0),
          uploadMs: upl / n, encodeMs: enc / n, stepMs: stp / n, gatherMs: gat / n,
          copyMs: cpy / n, gpuMs: medianOf(gpu), gpuSamples: gpu.length });
      } else requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
  });
}

/**
 * Frames per second with the display out of the way: submit, then block on the GPU having
 * finished, and repeat. `requestAnimationFrame` can never report more than the refresh
 * rate, so it cannot distinguish "comfortably ahead" from "exactly keeping up" — this can.
 */
async function benchUncapped(ms) {
  const t0 = performance.now();
  let n = 0, upl = 0, enc = 0, stp = 0, gat = 0, cpy = 0; const gpu = [];
  // Warm up: first frames include pipeline/allocation costs that are not steady state.
  for (let i = 0; i < 5; i++) {
    if (path === 'sab') uploadSab(); else { stepInline(16.7); uploadInline(); }
    renderOnce(); await backend.waitIdle();
  }
  const s = performance.now();
  while (performance.now() - s < ms) {
    if (path === 'sab') uploadSab(); else { stepInline(16.7); uploadInline(); }
    renderOnce();
    await backend.waitIdle();
    const sT = simTimings();
    upl += uploadMs; enc += encodeMs; stp += sT.step; gat += sT.gather; cpy += sT.copy; n++;
    gpu.push(backend.lastGpuNs / 1e6);
  }
  const el = performance.now() - s;
  return { kind: 'uncapped', frames: n, ms: el, fps: (n * 1000) / el,
    uploadMs: upl / n, encodeMs: enc / n, stepMs: stp / n, gatherMs: gat / n,
    copyMs: cpy / n, gpuMs: medianOf(gpu), gpuSamples: gpu.length, warmupMs: s - t0 };
}

/** Render-only: no sim stepping, no uploads. Isolates the draw from everything else. */
async function benchDrawOnly(ms) {
  const s = performance.now();
  let n = 0; const gpu = [];
  for (let i = 0; i < 5; i++) { renderOnce(); await backend.waitIdle(); }
  const s2 = performance.now();
  while (performance.now() - s2 < ms) { renderOnce(); await backend.waitIdle(); n++; gpu.push(backend.lastGpuNs / 1e6); }
  const el = performance.now() - s2;
  return { kind: 'draw-only', frames: n, ms: el, fps: (n * 1000) / el,
    gpuMs: medianOf(gpu), gpuSamples: gpu.length, setupMs: s2 - s };
}

/** Simulation only: no uploads, no draws. The honest cost of a real tick in wasm. */
async function benchSimOnly(ms) {
  if (!sim) return { kind: 'sim-only', unsupported: 'sab path: ask the shards' };
  sim.step(20);
  const s = performance.now();
  let n = 0;
  while (performance.now() - s < ms) { sim.step(1); n++; }
  const el = performance.now() - s;
  return { kind: 'sim-only', frames: n, ms: el, fps: (n * 1000) / el,
    stepMs: el / n, unitStepsPerSec: (sim.liveTotal * n * 1000) / el,
    worldStepsPerSec: (worldsTotal * n * 1000) / el, live: sim.liveTotal };
}

// ---------------------------------------------------------------------------------------
// Setup
// ---------------------------------------------------------------------------------------

async function makeBackend(prefer) {
  const order = prefer === 'webgl2' ? [WebGl2Backend, WebGpuBackend] : [WebGpuBackend, WebGl2Backend];
  const errs = [];
  for (const B of order) {
    try { return await B.create(canvas); } catch (e) { errs.push(`${B.name}: ${e.message}`); }
  }
  throw new Error('no usable graphics backend — ' + errs.join('; '));
}

async function setup(m) {
  path = m.path;
  worldsTotal = m.worlds;
  stride = m.stride ?? 0;
  simHz = m.simHz ?? 0;
  framesPerIter = m.framesPerIter ?? 1;

  if (path === 'sab') {
    sab = m.sab; plan = m.plan; shards = m.shards; worldsPerShard = m.worldsPerShard;
    ctrl = new Int32Array(sab, 0, shards * CTRL_I32);
    sabI32 = new Int32Array(sab);
    sabU32 = new Uint32Array(sab);
    mapSpan = m.mapSpan; stride = m.stride; statsPerWorld = m.statsPerWorld ?? 8;
    sim?.destroy(); sim = null;
  } else {
    // Drop every trace of a previous `sab` configuration. Leaving `ctrl` populated made
    // `aggregateShardCtrl()` keep answering, so an inline run silently reported the
    // *previous* cluster's live count and step time — numbers that looked entirely
    // plausible and were from a different simulation.
    sab = null; ctrl = null; sabI32 = null; sabU32 = null; plan = null; shards = 0; worldsPerShard = 0;
    sim = await WasmSim.load(m.wasmUrl);
    if (m.gameData) sim.loadGameData(new Uint8Array(m.gameData));
    sim.create(m.worlds, m.owners, m.perOwner, m.capacity, m.seed);
    stride = sim.stride; mapSpan = sim.mapSpan; worldsTotal = sim.worlds;
    statsPerWorld = sim.statsPerWorld;
  }
  cells = worldsTotal * stride;
  stepMs = 0; gatherMs = 0; uploadMs = 0; encodeMs = 0;
  statsCpu = new Uint32Array(worldsTotal * statsPerWorld);
  statsMax = { hits: 1, kills: 1 };
  backend.ensureCapacity(cells, worldsTotal);
  refreshStats();
}

/** Refresh the per-world statistics that drive the aggregate view and the panel. */
function refreshStats(payload) {
  if (sim) {
    sim.gatherStats();
    const st = sim.stats();
    statsCpu.set(st.subarray(0, statsCpu.length));
    backend.writeColumn('stats', 0, st, 0, worldsTotal * statsPerWorld);
  } else if (payload && payload.stats) {
    const off = payload.worldOffset * statsPerWorld;
    statsCpu.set(payload.stats.subarray(0, Math.min(payload.stats.length, statsCpu.length - off)), off);
    backend.writeColumn('stats', off, payload.stats, 0, payload.stats.length);
  }
  clusterSummary();
}

self.onmessage = async (e) => {
  const m = e.data;
  try {
    switch (m.cmd) {
      case 'boot': {
        canvas = m.canvas; dpr = m.dpr; cssW = m.cssW; cssH = m.cssH;
        backend = await makeBackend(m.prefer);
        backend.resize(Math.round(cssW * dpr), Math.round(cssH * dpr));
        self.postMessage({ type: 'booted', backend: backend.constructor.name,
          adapter: backend.adapterInfo, timestamps: backend.timestampSupported });
        break;
      }
      case 'setup':
        await setup(m);
        self.postMessage({ type: 'ready', worlds: worldsTotal, stride, cells, mapSpan,
          statsPerWorld,
          real: sim ? sim.isReal : null,
          balanceDistinct: sim ? sim.balanceDistinct : 0,
          tickMs: sim ? sim.tickMs : 67,
          digest: sim ? sim.digestHex() : null });
        break;
      case 'run': running = true; lastFrameT = 0; lastFpsT = performance.now(); requestAnimationFrame(frame); break;
      case 'pause': running = false; break;
      case 'resize':
        cssW = m.cssW; cssH = m.cssH; dpr = m.dpr;
        backend.resize(Math.round(cssW * dpr), Math.round(cssH * dpr));
        break;
      case 'camera': cam = m.cam; pointScale = m.pointScale ?? pointScale; break;
      case 'view': viewMode = m.view; flags = m.flags ?? flags; break;
      case 'speed': simHz = m.simHz; framesPerIter = m.framesPerIter ?? framesPerIter; accum = 0; break;
      case 'refresh-stats': refreshStats(m); break;
      case 'pick': {
        // Canvas NDC -> (world, subtile x, subtile y). The renderer owns the camera, so it
        // owns the inverse transform; the main thread only forwards the raw pointer.
        const g = grid();
        const px = m.ndc[0] / cam.zoom + cam.x;
        const py = m.ndc[1] / cam.zoom + cam.y;
        const gx = (px + 1) * 0.5 * g.cols;
        const gy = (1 - py) * 0.5 * g.rows;
        const wc = Math.floor(gx), wr = Math.floor(gy);
        const world = wr * g.cols + wc;
        if (world < 0 || world >= worldsTotal || wc < 0 || wc >= g.cols) break;
        const sx = Math.round((gx - wc) * mapSpan), sy = Math.round((gy - wr) * mapSpan);
        self.postMessage({ type: 'picked', action: m.action, world, sx, sy,
          shard: shards ? Math.floor(world / worldsPerShard) : 0,
          worldInShard: shards ? world % worldsPerShard : world });
        break;
      }
      case 'query': {
        // Read-only questions about the world, answered where the world lives. Selection is
        // a *query*; applying it is a `GroupCommand`, which is a command.
        if (!sim) break;
        const out = { type: 'query-result', qid: m.qid, world: m.world };
        if (m.what === 'pick-box') out.ids = sim.pickBox(m.world, m.who, m.x, m.y, m.radius);
        else if (m.what === 'nearest') out.unit = sim.pickNearest(m.world, m.x, m.y);
        self.postMessage(out, out.ids ? [out.ids.buffer] : []);
        break;
      }
      case 'command':
        // Raw engine wire bytes straight through to the shim.
        if (sim) sim.submit(m.world, m.who, new Uint8Array(m.bytes));
        break;
      case 'bench': {
        const wasRunning = running; running = false;
        await new Promise((r) => setTimeout(r, 30));
        const out = m.kind === 'raf' ? await benchRaf(m.ms)
          : m.kind === 'draw-only' ? await benchDrawOnly(m.ms)
          : m.kind === 'sim-only' ? await benchSimOnly(m.ms)
          : await benchUncapped(m.ms);
        out.worlds = worldsTotal; out.stride = stride;
        out.instances = path === 'zerocopy' ? sim.worldLive(0) : cells;
        out.live = sim ? sim.liveTotal : aggregateShardCtrl()?.live ?? 0;
        out.path = path; out.backend = backend.constructor.name;
        out.view = resolveView(grid());
        out.canvas = [canvas.width, canvas.height];
        // Recorded because the camera decides `resolveView`, and a sweep that silently
        // crossed the units/aggregate threshold partway through would otherwise look like
        // a performance cliff instead of a change of what is being drawn.
        out.zoom = cam.zoom; out.grid = grid(); out.simHz = simHz;
        out.kills = sim ? sim.kills : 0;
        self.postMessage({ type: 'bench', result: out });
        running = wasRunning;
        if (wasRunning) { lastFrameT = 0; requestAnimationFrame(frame); }
        break;
      }
      case 'step-exact': {
        // Advance by an exact frame count, independent of the render loop, so a digest can
        // be compared against a native run of the same configuration.
        if (sim) sim.step(m.frames);
        self.postMessage({ type: 'stepped', frames: sim ? sim.frame : 0 });
        break;
      }
      case 'digest':
        self.postMessage({ type: 'digest', digest: sim ? sim.digestHex() : null,
          frames: sim ? sim.frame : 0, kills: sim ? sim.kills : 0,
          damage: sim ? sim.damage : 0, live: sim ? sim.liveTotal : 0 });
        break;
      case 'teardown':
        running = false; sim?.destroy(); sim = null;
        break;
    }
  } catch (err) {
    self.postMessage({ type: 'error', where: m.cmd, message: String(err && err.stack || err) });
  }
};
