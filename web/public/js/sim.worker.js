// One shard of the cluster, running in its own worker with its own wasm instance.
//
// `wasm32-unknown-unknown` has no threads, so `don_sim::Batch::run_parallel` is unusable
// in the browser. Batch parallelism therefore moves up one level: N workers, N wasm
// instances, N disjoint slices of the world set. Worlds share nothing in `don-sim`, which
// is precisely what makes that substitution sound — the shard boundary is the same
// boundary `run_parallel` chunks on natively.
//
// Each shard publishes into its own triple buffer and never synchronises with any other
// shard. See `proto.js` for why, and for what that costs.

import { WasmSim } from './wasm.js';
import { CTRL, CTRL_I32, bankLayout, chooseWriteBank } from './proto.js';

let sim = null;
let ctrl = null;       // Int32Array over the control region
let sab = null;
let base = 0;          // control-block base index for this shard
let shard = 0;
let bankByteOffsets = [];
let layout = null;
let running = false;
let lastLive = -1;
let simHz = 0;         // 0 means "as fast as possible"
let framesPerIter = 1;
let acc = 0, lastT = 0;
const chan = new MessageChannel();

function publish() {
  const t0 = performance.now();
  sim.step(framesPerIter);
  const t1 = performance.now();
  sim.gather();
  const t2 = performance.now();

  const b = chooseWriteBank(ctrl, base);
  const off = bankByteOffsets[b];
  const dstX = new Int32Array(sab, off + layout.xOff, layout.cells);
  const dstY = new Int32Array(sab, off + layout.yOff, layout.cells);
  // The one cross-thread copy in this mode: wasm linear memory -> shared memory. It exists
  // because a non-shared wasm `Memory` is a plain ArrayBuffer owned by this worker, which
  // no other thread can view. Making it disappear needs a *shared* wasm memory, which needs
  // `+atomics` and a rebuilt `std` — measured and discussed in the track report.
  dstX.set(sim.mirrorX());
  dstY.set(sim.mirrorY());
  const t3 = performance.now();

  const live = sim.liveTotal;
  if (live !== lastLive) {
    // Population changed, so the owner/occupancy column changed. Rare, so write it into
    // every bank at once rather than tracking per-bank staleness.
    const tags = sim.tags();
    for (const o of bankByteOffsets) new Uint32Array(sab, o + layout.tagOff, layout.cells).set(tags);
    lastLive = live;
    Atomics.store(ctrl, base + CTRL.TAG_DIRTY, 1);
  }

  Atomics.store(ctrl, base + CTRL.SIM_FRAME, sim.frame | 0);
  Atomics.store(ctrl, base + CTRL.LIVE, live);
  Atomics.store(ctrl, base + CTRL.STEP_US, Math.round((t1 - t0) * 1000));
  Atomics.store(ctrl, base + CTRL.GATHER_US, Math.round((t2 - t1) * 1000));
  Atomics.store(ctrl, base + CTRL.COPY_US, Math.round((t3 - t2) * 1000));
  Atomics.add(ctrl, base + CTRL.FRAMES, framesPerIter);
  // Publish last: the bank is complete before any reader can be pointed at it.
  Atomics.store(ctrl, base + CTRL.PUBLISHED, b);
  Atomics.add(ctrl, base + CTRL.SEQ, 1);
}

function loop() {
  if (!running) return;
  if (Atomics.load(ctrl, base + CTRL.QUIT)) { running = false; return; }
  if (simHz > 0) {
    const now = performance.now();
    acc += ((now - lastT) / 1000) * simHz;
    lastT = now;
    if (acc >= 1) { framesPerIter = Math.min(240, Math.floor(acc)); acc -= framesPerIter; publish(); }
  } else {
    publish();
  }
  // A MessageChannel round trip is the shortest macrotask yield a worker has; `setTimeout`
  // is clamped and would cap the loop far below what the sim can do.
  chan.port2.postMessage(0);
}
chan.port1.onmessage = loop;

self.onmessage = async (e) => {
  const m = e.data;
  switch (m.cmd) {
    case 'init': {
      sim = await WasmSim.load(m.wasmUrl);
      sim.create(m.worldsPerShard, m.unitsPerWorld, m.capacity, m.seed);
      sab = m.sab; shard = m.shard; base = shard * CTRL_I32;
      ctrl = new Int32Array(sab, 0, m.shards * CTRL_I32);
      layout = bankLayout(m.worldsPerShard, sim.stride);
      bankByteOffsets = m.bankOffsets;
      simHz = m.simHz ?? 0;
      framesPerIter = m.framesPerIter ?? 1;
      lastT = performance.now();
      sim.gatherStats();
      Atomics.store(ctrl, base + CTRL.READY, 1);
      publish();
      self.postMessage({ type: 'ready', shard, stride: sim.stride, worlds: sim.worlds, mapSpan: sim.mapSpan, tickHz: sim.tickHz });
      break;
    }
    case 'run': running = true; lastT = performance.now(); loop(); break;
    case 'pause': running = false; break;
    case 'speed': simHz = m.simHz; framesPerIter = m.framesPerIter ?? framesPerIter; acc = 0; lastT = performance.now(); break;
    case 'order': sim.order(m.kind, m.world, m.owner, m.sx, m.sy, m.tx, m.ty, m.radius); break;
    case 'stats': {
      sim.gatherStats();
      const st = sim.stats();
      // A copy, on purpose: the stats table is 16 B per world and is consumed by the
      // aggregate view at UI rates, not frame rates, so postMessage is the right cost.
      self.postMessage({ type: 'stats', shard, stats: st.slice() });
      break;
    }
    case 'digest':
      self.postMessage({ type: 'digest', shard, digest: sim.digestHex(), frames: sim.frame, live: sim.liveTotal });
      break;
    case 'bench-sim': {
      // Pure simulation throughput inside wasm, no rendering, no copies.
      const t0 = performance.now();
      sim.step(m.frames);
      const t1 = performance.now();
      const g0 = performance.now();
      for (let i = 0; i < m.gathers; i++) sim.gather();
      const g1 = performance.now();
      self.postMessage({
        type: 'bench-sim', shard,
        stepMs: t1 - t0, frames: m.frames,
        gatherMs: (g1 - g0) / Math.max(1, m.gathers),
        worlds: sim.worlds, live: sim.liveTotal,
      });
      break;
    }
  }
};
