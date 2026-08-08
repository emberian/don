// One shard of the cluster, running in its own worker with its own wasm instance.
//
// `wasm32-unknown-unknown` has no threads, so batch parallelism moves up one level: N
// workers, N wasm instances, N disjoint slices of the world set. Worlds share nothing,
// which is precisely what makes that substitution sound.
//
// Each shard publishes into its own triple buffer and never synchronises with any other
// shard. See `proto.js` for why, and for what that costs.
//
// The shard also owns *its* worlds' commands and queries: a click that lands in world 900
// is answered by whichever worker holds world 900, and the order that follows is submitted
// there. That is the same routing a networked client needs, done locally.

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
  // The one cross-thread copy in this mode: wasm linear memory -> shared memory. It exists
  // because a non-shared wasm `Memory` is a plain ArrayBuffer owned by this worker, which
  // no other thread can view. Making it disappear needs a *shared* wasm memory, which needs
  // `+atomics` and a rebuilt `std` — measured and discussed in the track report.
  new Int32Array(sab, off + layout.xOff, layout.cells).set(sim.mirrorX());
  new Int32Array(sab, off + layout.yOff, layout.cells).set(sim.mirrorY());
  // The tag column carries hit points now, so it changes every frame and rides the same
  // triple buffer as the positions rather than a dirty flag.
  new Uint32Array(sab, off + layout.tagOff, layout.cells).set(sim.mirrorTag());
  const t3 = performance.now();

  Atomics.store(ctrl, base + CTRL.SIM_FRAME, sim.frame | 0);
  Atomics.store(ctrl, base + CTRL.LIVE, sim.liveTotal);
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
  try {
    switch (m.cmd) {
      case 'init': {
        sim = await WasmSim.load(m.wasmUrl);
        if (m.gameData) sim.loadGameData(new Uint8Array(m.gameData));
        sim.create(m.worldsPerShard, m.owners, m.perOwner, m.capacity, m.seed);
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
        self.postMessage({ type: 'ready', shard, stride: sim.stride, worlds: sim.worlds,
          mapSpan: sim.mapSpan, tickMs: sim.tickMs, real: sim.isReal,
          statsPerWorld: sim.statsPerWorld, balanceDistinct: sim.balanceDistinct });
        break;
      }
      case 'run': running = true; lastT = performance.now(); loop(); break;
      case 'pause': running = false; break;
      case 'speed': simHz = m.simHz; framesPerIter = m.framesPerIter ?? framesPerIter; acc = 0; lastT = performance.now(); break;
      case 'command': sim.submit(m.world, m.who, new Uint8Array(m.bytes)); break;
      case 'query': {
        const out = { type: 'query-result', qid: m.qid, shard, world: m.world };
        if (m.what === 'pick-box') out.ids = sim.pickBox(m.world, m.who, m.x, m.y, m.radius);
        else if (m.what === 'nearest') out.unit = sim.pickNearest(m.world, m.x, m.y);
        self.postMessage(out, out.ids ? [out.ids.buffer] : []);
        break;
      }
      case 'stats': {
        sim.gatherStats();
        // A copy, on purpose: the stats table is 32 B per world and is consumed by the
        // aggregate view at UI rates, not frame rates, so postMessage is the right cost.
        self.postMessage({ type: 'stats', shard, stats: sim.stats().slice() });
        break;
      }
      case 'digest':
        self.postMessage({ type: 'digest', shard, digest: sim.digestHex(), frames: sim.frame,
          live: sim.liveTotal, kills: sim.kills });
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
          worlds: sim.worlds, live: sim.liveTotal, kills: sim.kills,
        });
        break;
      }
    }
  } catch (err) {
    self.postMessage({ type: 'error', shard, where: m.cmd, message: String(err && err.stack || err) });
  }
};
