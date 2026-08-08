# Track: web-frontend — a browser spectator for one simulation or a cluster of thousands

**Lane: `web-frontend`. This is engineering and measurement, not binary derivation.** No
Rise of Nations mechanic is implemented, claimed, or relied on here. The charter rule about
deriving mechanics from the binary is not relaxed, it is *not engaged*: everything this page
simulates is `don-sim`'s own **PLACEHOLDER** systems plus one more placeholder of this
lane's making (an order-follow pass), and the numbers below measure a **data path** — how
fast simulation state can get from a Rust structure-of-arrays column onto a screen — not a
simulation speed.

Every claim is **[measured]** (I ran it on this machine and the artefact is in-repo) or
**[reported]** (I read it and have not checked it). Fidelity tiers (A/B/C) do not apply to
this lane, because nothing here is compared against retail; where a claim *could* have
failed and did not, I say what the falsifier was.

Artefacts: `/Users/ember/dev/don/web/` (app), `/Users/ember/dev/don/web/results.json`
(raw measurement output), `/Users/ember/dev/don/web/README.md` (how to run).

---

## 0. Headline

**A working WebGPU spectator renders 4,096 independent simulated worlds — 262,144 units —
at the display's full 120 fps on an Apple M2 Max, with 1.08 ms of GPU time and ~2.8 ms of
total CPU per frame, while stepping every world one simulation tick per rendered frame.**
[measured] At the *single-world* scale a spectator actually watches (one world, up to 4,096
units) the whole per-frame CPU cost is **0.05 ms** and the GPU cost is **0.19 ms**, i.e.
about 1.5% of a 120 Hz frame budget. The 60 fps bar in the lane brief is not close to
binding; the display refresh is.

The design claim underneath that, and the one worth arguing about:

> **`don-sim`'s SoA columns are already the layout the GPU's vertex stage wants.** Bind
> `pos_x` as one vertex buffer with `format: 'sint32'` and `stepMode: 'instance'`, `pos_y`
> as another, and the per-frame CPU work collapses to one `queue.writeBuffer` per column.
> There is no interleaved per-instance struct anywhere in this system, so there is no
> repack pass, no `f32` conversion pass, and no per-entity CPU loop at all.

Three honest qualifications, each measured:

1. **The cluster path pays one CPU copy that the single-world path does not**, because
   `don_sim::Batch` allocates each world's columns separately and an instanced draw over
   thousands of worlds needs one contiguous range. Measured at §4.3.
2. **Moving the simulation onto N workers does not raise the frame rate** — the renderer
   consumes whatever has been published — **it raises simulation throughput**, and that
   scaling is sublinear and modest at these sizes. Measured at §4.5.
3. **This machine was at load average 65–145 during every measurement** (it is shared with
   other lanes and a running Parallels VM). Every CPU-side number below is therefore a
   *pessimistic* sample with real spread, and the spread is reported rather than averaged
   away. GPU-timestamp numbers are far less affected.

---

## 1. What was built

`web/` — no bundler, no `npm install`, no transpile, no framework. ES modules served as-is;
the only build artefact is a **23.8 KB** `.wasm` [measured].

```sh
web/build.sh                                 # cargo -> wasm32 -> public/wasm/don_web.wasm
node web/serve.mjs                           # http://127.0.0.1:8787 with COOP/COEP
node web/bench.mjs --out results.json        # launches Chrome, drives it over CDP
cd web/wasm && cargo run --release --bin digest -- digest 4 64 64 1000 0xC0FFEE
cd web/wasm && cargo run --release --bin digest -- bench 4096 64 64 100
```

| piece | what it is |
|---|---|
| `web/wasm/` | `don-web`, a raw C-ABI wasm shim over `don-sim`. **No `wasm-bindgen`.** Its own cargo workspace, so the root `cargo test` never builds it and no lane's build grows a cdylib link step. |
| `public/js/proto.js` | SharedArrayBuffer layout: per-shard control block plus triple-buffered column banks |
| `public/js/wasm.js` | module loader; typed-array views over wasm linear memory, with detach-safety |
| `public/js/sim.worker.js` | one cluster shard per worker, publishing into shared memory |
| `public/js/render.worker.js` | OffscreenCanvas owner, frame loop, the three data paths, the benchmark |
| `public/js/webgpu.js` | primary backend |
| `public/js/webgl2.js` | fallback with the *same* architecture (`vertexAttribIPointer` + `vertexAttribDivisor`) |
| `public/js/main.js` | DOM, pointer, and the `window.don` promise-based automation surface |

It is a live page, not a mock: it steps a real `don_sim::Batch` compiled to
`wasm32-unknown-unknown`, renders from that batch's own columns, and lets you click to
issue an order that the units obey.

---

## 2. The data path, and where every copy is

### 2.1 Rust → wasm linear memory: not a copy, a *view*

`don-web` exports byte offsets, not values:

```rust
pub unsafe extern "C" fn sim_world_x_ptr(s: *mut Sim, w: u32) -> *const i32
```

JavaScript builds `new Int32Array(memory.buffer, ptr, len)` at that offset. That array **is**
`World::pos_x`'s allocation. Stepping the simulation changes its contents with no call on
the JS side. Nothing is serialised; there is no JSON anywhere in this system, and no
`wasm-bindgen` glue in the per-frame path — the module has **zero imports** [measured, via
`WebAssembly.Module.imports`], so there is nothing for glue to bridge.

The hazard this creates is real and is handled: **any wasm allocation can grow linear memory
and detach every existing JS view.** All per-frame storage is allocated once inside
`sim_create`, so a running shard never grows; `wasm.js` additionally revalidates views
against the buffer identity on each accessor call, which is one identity comparison, not a
per-element cost.

### 2.2 wasm memory → GPU: one copy, and it cannot be removed

```js
device.queue.writeBuffer(bufX, 0, sim.worldX(0), 0, live);   // WebGPU
gl.bufferSubData(gl.ARRAY_BUFFER, 0, column, 0, live);       // WebGL2
```

No browser API hands the GPU a pointer to a page the CPU is still writing. `mapAsync` +
`getMappedRange` relocates the same `memcpy` into user code *and* adds a round trip; a
persistently mapped ring would require the simulation to write **into the mapping**, which
it cannot, because it writes into wasm linear memory. So one copy at upload is the floor for
any design that keeps the sim in wasm. This is the honest answer to "where is a copy
unavoidable, and why".

### 2.3 SoA columns *are* the instance buffer

The reflex design builds `Vec<{x, y, colour}>` and uploads it. That is a full CPU pass over
every entity, every frame, in JavaScript. It is unnecessary: a vertex buffer does not have
to be interleaved.

```js
const col = (fmt) => ({ arrayStride: 4, stepMode: 'instance',
                        attributes: [{ shaderLocation: 0, offset: 0, format: fmt }] });
buffers: [col('sint32'),   // pos_x  — the simulation's own column
          col('sint32'),   // pos_y
          col('uint32')]   // tag: bit 31 occupied, bits 0..7 owner
```

Consequences that fall out of that one decision:

- **Positions stay `i32` in engine subtile units all the way to the vertex shader.** The
  divide by `MAP_SPAN` happens once per instance, on the GPU. There is no CPU-side
  normalisation pass and no `f32` conversion pass.
- **Quad corners come from `@builtin(vertex_index)`**, so there is no geometry vertex buffer
  and no index buffer: 4 vertices, `triangle-strip`, `instanceCount = worlds * stride`.
- **The tag column is uploaded on a dirty flag, not per frame**, because owner and occupancy
  change only when a world's population changes.
- **The whole cluster is one draw call.** World index is `instance_index / stride` in the
  shader; the grid cell offset is computed there too.

The WebGL2 fallback is the *same* architecture — `vertexAttribIPointer` (integer path, no
normalisation) plus `vertexAttribDivisor(loc, 1)` and `drawArraysInstanced`. That matters:
if the fallback had to interleave, its numbers would say nothing about the primary path.

### 2.4 The three paths, and the copy count of each

| path | sim runs in | CPU copies before upload | why it exists |
|---|---|---|---|
| `zerocopy` | render worker | **0** | one world; the vertex buffer is filled from `World::pos_x` itself |
| `inline` | render worker | **1** (`sim_gather`) | many worlds; `Batch` has no contiguous range spanning worlds |
| `sab` | N sim workers | **2** (`sim_gather`, then wasm→SharedArrayBuffer) | many worlds, simulation on other cores |

The `inline` copy is a consequence of `Batch`'s allocation strategy, **not of the browser**:
`Batch::with_capacity` gives every world its own `Vec` per column so it can right-size
provisioning, and an instanced draw over thousands of worlds needs one contiguous byte
range. A cluster-native pool (one column per attribute for the whole batch, world-major at a
fixed stride) would delete this copy. That is a `don-sim` decision, not a web one, and it is
recorded here as a finding rather than acted on — this lane does not own `crates/don-sim`.

The `sab` copy exists because a non-shared wasm `Memory` is a plain `ArrayBuffer` owned by
one worker. Removing it needs a **shared** wasm memory, which needs
`-Ctarget-feature=+atomics,+bulk-memory,+mutable-globals` and a rebuilt `std`
(`-Zbuild-std`) on nightly. Not attempted; see §6.

### 2.5 Threading, and why the batch scheduler could not come along

`wasm32-unknown-unknown` has **no threads**, so `don_sim::Batch::run_parallel` is unusable in
the browser. Batch parallelism moves up one level: N workers, N wasm instances, N disjoint
slices of the world set. That substitution is sound for exactly the reason `run_parallel` is
sound — worlds share nothing in `don-sim` — and the shard boundary is the same boundary
`run_parallel` chunks on natively.

### 2.6 The shared-memory protocol

Each shard owns a control block and **three** column banks. Two banks are not enough: with
two, the writer publishes B and immediately starts refilling A — the bank the renderer is
still reading — and the upload tears. With three, the writer excludes both the published
bank and the bank the reader has claimed, so a free bank always exists. The reader claims by
storing its bank index into the control block before reading and clearing it after: one
atomic store on each side of an upload that itself costs hundreds of microseconds.

**Shards never synchronise with each other.** The renderer can be showing world 0 from sim
frame 9001 and world 900 from 9003. That is deliberate — this is a spectator, not a lockstep
client, and a cross-shard barrier would make the slowest shard set every other shard's
throughput, which is precisely the property the batch architecture exists to avoid.

### 2.7 COOP/COEP is a hard gate, not a tax

`SharedArrayBuffer` exists only on a **cross-origin isolated** page. The document must
arrive with:

```
Cross-Origin-Opener-Policy:   same-origin
Cross-Origin-Embedder-Policy: require-corp
```

Without them `crossOriginIsolated` is `false` and the `SharedArrayBuffer` constructor is not
even defined — the multi-worker path cannot exist at all. `web/serve.mjs` sets them (plus
`Cross-Origin-Resource-Policy: same-origin`); the page detects the state, shows it in the
panel, and disables the `sab` path with an explanation rather than failing obscurely. The
price of isolation is that every cross-origin subresource must opt in via CORP or CORS —
this app has **none**, which is itself an argument for the no-framework build.

### 2.8 Reading the cluster: the aggregate view

A grid of 4,096 worlds at 33 device pixels per cell renders, correctly, as one
undifferentiated field of dots. The frame counter is perfectly happy about this; a
screenshot is not. Two things fix it:

- **A per-world plate**: one extra instanced draw of `worlds` quads underneath the units,
  same pipeline as the aggregate view, one uniform apart (a second bind group over the same
  uniform buffer at 256-byte alignment, because a render pass cannot have a buffer rewritten
  between its draws). This is what makes a cluster read as a cluster.
- **Automatic switch to the aggregate view below ~18 device pixels per cell.** A world is
  256 tiles across; below that, a cell has under a fourteenth of a pixel per tile and
  individual units cannot be distinguished no matter how they are drawn. At that scale the
  aggregate view is not a fallback, it is the correct visualisation. Per-world statistics
  (live count, summed hit points, digest low bits, frame) are gathered in wasm into a
  16-byte-per-world table and uploaded at UI rate, not frame rate.

---

## 3. How the measurements were taken

`node web/bench.mjs` launches Chrome in a throwaway profile and drives the page over the
DevTools protocol; it has no npm dependencies (Node's global `WebSocket` and `fetch` speak
CDP directly). The browser is launched **headed** on purpose: headless Chrome on macOS falls
back to software rasterisation for much of this work, and a software number reported as a
GPU number is worse than no number.

Three measurements, because they answer different questions:

- **`raf`** — the display-synchronised loop, i.e. *what a spectator sees*. Cannot exceed the
  refresh rate; this display is 120 Hz.
- **`uncapped`** — submit a frame, then block on `queue.onSubmittedWorkDone()`, repeat.
  `requestAnimationFrame` can never distinguish "comfortably ahead" from "exactly keeping
  up"; this can. **Read it as a per-frame latency including a full CPU↔GPU round trip**, not
  as the GPU's throughput ceiling — the ~1.0 ms floor visible at small sizes is the fence
  round trip, not the work.
- **GPU timestamp queries** (`timestamp-query`, available on this machine) — the actual GPU
  occupancy of the render pass, and the only number that says how much headroom exists.

Every row runs **3 times** and the report keeps the median and the min/max. The load average
is recorded with every row. Simulation load is defined as **one simulation tick per rendered
frame** (`simHz: 0`); note that the engine's real tick rate is 15 Hz against a 120 Hz
display, so the realistic spectator workload is roughly **8× less simulation per frame** than
these rows carry.

Machine: Apple M2 Max (38-core GPU, 12 CPU cores, 96 GB), macOS 26.6, Chrome 153.0.7979.3
dev, WebGPU on Metal 3, `devicePixelRatio` 2, canvas ~2560×1826 device pixels.

**Contamination, stated plainly:** load average was 65–145 throughout, from a running
Parallels VM, Lean/`lake` builds, `rustc`, and other agents' work. CPU-side timings have
real spread (the native reference below varies by 4× on the same configuration). This makes
the CPU numbers *conservative* — the true costs are lower — and it makes any fine-grained
CPU comparison between two paths unreliable. GPU timestamps and the rAF cap are stable.

---

## 4. Numbers

All rows: WebGPU, Apple M2 Max, canvas 2560×1826 device pixels, one simulation tick per
rendered frame, median of 3 × 2.5 s. Raw JSON: `web/results.json`. All [measured].

### 4.1 One world: frames per second against entity count

The `zerocopy` path — sim in the render worker, vertex buffer filled from `World::pos_x`
itself, no CPU copy at all.

| units | rAF fps | uncapped fps | step ms | upload ms | encode ms | **CPU ms/frame** | **GPU ms/frame** |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 128 | **120.4** | 1030.7 | 0.002 | 0.004 | 0.026 | 0.032 | 0.095 |
| 512 | **120.4** | 1016.9 | 0.003 | 0.005 | 0.029 | 0.037 | 0.110 |
| 1,024 | **120.4** | 978.8 | 0.005 | 0.007 | 0.029 | 0.041 | 0.149 |
| 2,048 | **120.4** | 968.4 | 0.009 | 0.010 | 0.030 | 0.048 | 0.134 |
| 4,096 | **120.4** | 926.3 | 0.016 | 0.017 | 0.030 | 0.062 | 0.151 |

`120.4` is this display's refresh rate; the rAF loop is refresh-locked at every size. The
whole per-frame CPU cost at 4,096 units is **0.062 ms — 0.7% of a 120 Hz frame budget**, and
the GPU is busy 0.15 ms. `MAX_UNITS` in `don-sim` is 4,096, so this row is the ceiling of
what a single world can currently hold, and it is not remotely a limit for the renderer.

Read the uncapped column as **per-frame latency including a full CPU↔GPU fence**: at 128
units the work is 0.032 ms CPU + 0.095 ms GPU, yet a frame takes 0.97 ms, so ~0.85 ms of it
is the round trip. It is a useful *comparative* number and a misleading absolute one.

### 4.2 Cluster: frames per second against world count

64 units per world. `inline` = one wasm instance in the render worker (one gather copy);
`sab` = 4 sim workers publishing through shared memory.

| worlds | instances | live units | rAF fps (inline) | uncapped (inline) | uncapped (sab ×4) | GPU ms |
|---:|---:|---:|---:|---:|---:|---:|
| 16 | 1,024 | 1,024 | **120.4** | 959.4 | 1075.4 | 0.12 |
| 64 | 4,096 | 4,096 | **120.4** | 902.0 | 1036.0 | 0.13–0.15 |
| 256 | 16,384 | 16,384 | **120.4** | 778.8 | 907.0 | 0.18 |
| 1,024 | 65,536 | 65,536 | **120.4** | 463.5 | 651.5 | 0.34 |
| 4,096 | 262,144 | 262,144 | **120.4** | 170.9 | 295.5 | 1.07 |

**The display cap holds at every size measured, including 4,096 worlds and 262,144 units.**
Where the frame budget actually goes at the top row:

| 4,096 worlds × 64 units | step | gather | wasm→SAB | upload | encode | **render-thread CPU** | GPU |
|---|---:|---:|---:|---:|---:|---:|---:|
| `inline` (sim on render thread) | 1.808 | 0.322 | — | 0.862 | 0.062 | **3.054 ms** (37% of a 120 Hz frame) | 1.070 |
| `sab` ×4 (sim on 4 other cores) | *0.866* | *0.176* | *0.067* | 0.897 | 0.053 | **0.950 ms** (11%) | 1.075 |

*Italic* columns happen on the sim workers, not on the render thread. So the `sab` path's
real payoff is not frame rate — it is that **the renderer's own per-frame cost drops 3.2×**,
from 3.05 ms to 0.95 ms, because stepping and gathering left the thread that draws.

Two caveats on these rows. First, one sim tick per rendered frame at 120 Hz is **8× the
engine's real 15 Hz tick rate**, so a spectator watching at true speed pays roughly an eighth
of the `step` column. Second, at 4,096 worlds the auto view switch has *not* engaged (33
device pixels per cell, threshold 18), so this is the genuinely expensive rendering mode; the
aggregate view at the same world count costs 0.16 ms of GPU instead of 1.07 ms.

### 4.3 What each copy costs

Priced from the same rows, per frame:

| | 1,024 worlds (65,536 cells) | 4,096 worlds (262,144 cells) |
|---|---:|---:|
| `sim_gather` (wasm-internal, the cluster mirror) | 0.046 ms → 1.42 G cells/s | 0.322 ms → 814 M cells/s |
| wasm → SharedArrayBuffer (`sab` only) | 0.028 ms → 585 M cells/s* | 0.067 ms → 980 M cells/s* |
| `queue.writeBuffer` (unavoidable) | 0.217 ms | 0.862 ms |

\* per shard, over that shard's quarter of the cluster.

So the copy the cluster path adds is **roughly a fifth to a third of the copy nobody can
avoid**, and the shared-memory hop is smaller again. That is the quantitative answer to "is
the extra copy worth caring about": at these sizes, not very — the upload dominates, and the
upload is the floor. It also means shared wasm memory (which would delete only the second
row) would buy less than the `Batch` layout change (which would delete the first).

### 4.4 Draw alone

1,024 worlds × 64 units, no stepping and no uploads: **820 fps, 0.41 ms of GPU time**. The
full pipeline at the same configuration runs 463 fps with 0.34 ms of GPU. The two GPU
figures agreeing to within noise says the render pass is not affected by the uploads — the
difference between 820 and 463 fps is entirely CPU-side sim and upload work, which is what
the breakdown in §4.2 already shows.

### 4.5 Simulation throughput against worker count

**This is the axis frame rate cannot see.** The renderer draws whatever the shards have
published, so render fps barely moves with shard count (649 → 710 fps from 1 to 12 workers).
What moves is how fast the cluster actually simulates, read from the shards' own frame
counters over a 2 s window, 1,024 worlds × 64 units:

| sim workers | M unit-steps/s | M world-steps/s | speed-up | × real time (15 Hz tick) |
|---:|---:|---:|---:|---:|
| 1 | 160.6 | 2.51 | 1.00× | 163× |
| 2 | 303.5 | 4.74 | 1.89× | 309× |
| 4 | 517.1 | 8.08 | 3.22× | 526× |
| 6 | 744.3 | 11.63 | 4.63× | 756× |
| 8 | 918.0 | 14.34 | 5.71× | 934× |
| 12 | 991.0 | 15.48 | 6.17× | **1000×** |

**A browser tab simulates 1,024 worlds at about 1,000× real time while rendering all of them
at the display's refresh rate.** Scaling is near-linear to 4 workers, still good at 8
(5.71× on 8), and flattens at 12 — on a 12-core machine that was already at load average
105–127 from other work, with the render worker also running. This is a *floor* on the
scaling, not a ceiling.

### 4.6 Native reference, and why it is only a sanity check

Same code, host build, single-threaded, median of 5 (`digest bench`):

| configuration | native step ms [min..max] | native M unit-steps/s | wasm-in-browser M unit-steps/s |
|---|---:|---:|---:|
| 1 world × 4,096 units | 0.017 [0.008..0.049] | 247 | 256 |
| 1,024 worlds × 64 units | 0.241 [0.165..0.701] | 272 | 161 (1 worker) |
| 4,096 worlds × 64 units | 3.134 [1.383..6.392] | 84 | 145 |

**The native spread is 4× on the same configuration**, so this table supports exactly one
conclusion: wasm and native are within the same order of magnitude on this workload, and no
finer claim is defensible from data taken on a machine at load average 100+. I am
deliberately not reporting a "wasm is X% of native" figure.

The native numbers do show one structural thing that is not noise, because it is monotone
across five configurations: **single-threaded step throughput collapses as world count
rises** — 276 M unit-steps/s at 256 worlds, 272 M at 1,024, 84 M at 4,096, 41 M at 16,384 —
while a *single* 4,096-unit world sustains 247 M. Same total unit count, 3–6× the cost. The
work is identical, so the difference is memory layout: 4,096 worlds means ~53,000 small
separate allocations instead of a handful of large ones. See §7.2.

### 4.7 Cross-target determinism — the check that could have failed

4 worlds × 64 units, capacity 64, seed `0xC0FFEE`, exactly 1,000 frames:

| build | `Batch::digest()` |
|---|---|
| aarch64-apple-darwin, `cargo run --release --bin digest -- digest 4 64 64 1000 0xC0FFEE` | `0x1b07da068f50ba66` |
| wasm32-unknown-unknown, in Chrome, `window.don.stepFrames(1000)` then `digest()` | `1b07da068f50ba66` |

**Identical.** Two independently compiled targets — one where LLVM autovectorises `don-sim`'s
tick kernels to NEON, one where it does not — agree bit-for-bit on a 64-bit digest after
256,000 unit-frames. This is not proof of anything about the *game*; it is evidence that the
browser is running the same simulation the host runs, which is the precondition for a
spectator being worth looking at. It is one command to re-run, and it would fail loudly.

### 4.8 WebGL2 fallback

Forced with `?backend=webgl2`. Same architecture, integer instance attributes, one draw call.

| configuration | rAF fps |
|---|---:|
| 1 world × 2,048 units | **120.6** |
| 256 worlds × 16,384 instances | **120.4** |
| 1,024 worlds × 65,536 instances | **120.5** |
| 4,096 worlds × 262,144 instances | **120.5** |

The fallback holds the display cap at every size too, and a screenshot is
pixel-indistinguishable from the WebGPU one. **Its uncapped numbers are not reported**:
WebGL2's only barrier is `gl.finish()`, which on Chrome returns once commands reach the GPU
process rather than once the GPU has drained, so the uncapped loop there measured 18,000 fps
— an artefact, not a result. There are no timestamp queries on this path either, so its GPU
time is simply unmeasured.

---

## 5. Drop in and play

The page already does the first half: click the canvas and the blue team in that world moves
where you clicked. That path is deliberately shaped like the real one.

### 5.1 What a click becomes

```
pointerup  ->  render worker inverts the camera  ->  (world, subtile_x, subtile_y)
           ->  a fixed-size integer order record
           ->  posted to whichever thread owns that world
           ->  pushed onto that shard's pending queue
           ->  drained at the next tick boundary, in arrival order, before the tick
```

The order record is deliberately integer-only and fixed-size:

```rust
pub struct Order { kind: u32, world: u32, owner: u32,
                   sx: i32, sy: i32, tx: i32, ty: i32, radius: i32 }
```

Nothing about that is specific to a human. An RL policy emits the same record. The
*mechanism* — a command queue drained at a deterministic tick boundary — is the property
that makes spectating, playing, replaying, and training the same code path, and it is the
property lockstep requires.

### 5.2 What the action space must actually be, and what is already derived

**The action space is the engine's own `Order` hierarchy, and its class list is measured.**
`schema/vtables.json` (RTTI, 1,777 vtables) contains **32 `…Order` classes** [measured, this
lane, by reading that file]:

```
AirAttackGroundOrder  AirOrder            AirPatrolOrder      AttackGroundOrder
AttackOrder           AttackToOrder       AwaitBoardOrder     BoardOrder
BuildOrder            CastOrder           ExploreToOrder      FleeToOrder
FollowOrder           FormOrder           GarrisonOrder       GatherOrder
GroupAttackOrder      GroupAttackToOrder  GroupMoveOrder      GroupOrder
GroupPatrolOrder      GuardOrder          MoveOrder           OrderList
PatrolOrder           RepairOrder         SpecialAnimOrder    StrafeOrder
TargetOrder           ThinkOrder          TradeOrder          UnitOrder
```

Some of those are bases or containers (`UnitOrder`, `GroupOrder`, `TargetOrder`, `AirOrder`,
`OrderList`), so the *dispatchable* set is smaller — bounding it exactly is underived. The
`Group…` variants matter for the RL surface specifically: they say the engine already models
"this order applies to a selection", which is the difference between an action space of
per-unit commands and one of selection+command.

**The wire encoding is also partly derived, and it is the same wire.** Established ground
truth for this project: the `.rcx` command stream **is** the engine network protocol;
`CommandPackage::process` = `FUN_0094a700` is an 82-opcode switch (`0x00`–`0x51`) where each
handler returns the packet's byte length, so *the function is the wire format*; records are
framed `u32 frame, u32 play, u32 valid, u32 stamp, u16 size, u8 data[size]`. A viewer
becoming a participant therefore means **emitting one of those 82 packets into the frame's
command stream**, not inventing a new protocol.

`don_web::order_kind` in this lane is **three invented constants** (`MOVE`, `SPAWN`, `STOP`)
and is labelled as such in its own doc comment. It exists to demonstrate the shape. When the
opcode→`Order` mapping is derived from `FUN_0094a700`, `kind` becomes an index into that
table and this enum is deleted.

### 5.3 The concrete path from here

1. **Derive the opcode table.** Walk the 82 cases of `FUN_0094a700`, recover each handler's
   packet length and field layout, and cross-check against real `.rcx` records — the
   corpus already spans 2014/2020/2024/2026 builds, so a field that is stable across twelve
   years is structural and one that is not is build-specific. This is the single artefact
   that turns "our order struct" into "the engine's order struct".
2. **Make the browser order struct that struct.** The transport is already right: a
   fixed-size integer record queued at a tick boundary. Only the *contents* are ours.
3. **Parameter-level masking, from the start.** The charter is explicit that unmasked RTS
   action spaces measurably train to zero. The web client is the natural place to *see*
   the mask: the same predicate that greys out an illegal click is the mask the policy gets.
   Not built.
4. **Join a running cluster.** Spectating is read-only today; a participant needs its orders
   to reach the shard that owns its world, which the `sab` path already does for the local
   case (`postMessage` to the owning sim worker). Over a network the same record goes over a
   socket, and the determinism work (`docs/derivation/checksum.md`, the 16-channel
   `check_all` tuple at opcode `0x39`) is what would let a late joiner verify it is in sync
   rather than hope.

---

## 6. What I could not establish

- **Zero-copy across threads.** True zero-copy from a *worker's* simulation to the renderer
  needs shared wasm memory (`+atomics`, `-Zbuild-std`), which I did not attempt. The
  `zerocopy` path achieves zero CPU copies only by putting the sim in the render worker,
  which does not scale past one core. The cost of not having it is measured (§4.3) and is
  small; the claim that it *would* be faster is untested.
- **Any comparison of wasm against native at fine resolution.** The native reference varies
  by 4× run-to-run on this loaded machine. The two are in the same order of magnitude and I
  will not say more than that from this data.
- **WebGL2 numbers.** The fallback compiles and is architecturally identical, but every
  measurement here is WebGPU. The WebGL2 path is exercised only by construction, not by the
  benchmark, and it has no timestamp queries so its GPU time is unmeasurable by this harness.
- **Anything above 4,096 worlds × 64 units on the `sab` path.** Larger clusters were run
  interactively (16,384 worlds × 16 units renders at the display cap) but are not in the
  swept data.
- **Behaviour on any machine but this one.** One GPU, one browser, one OS. Nothing here
  generalises without re-measurement, and `bench.mjs` exists so that re-measurement is one
  command.
- **Whether the aggregate view's metrics are the right ones.** Live count and summed hit
  points are what `don-sim` exposes today; what a *spectator of a training run* actually
  needs to see (reward, divergence from a reference, checksum mismatch) is underived because
  those quantities do not exist yet.

---

## 7. Findings other lanes may care about

1. **`don-sim` compiles to `wasm32-unknown-unknown` unmodified** [measured] — no `cfg`, no
   feature gate, no threading shim. `Batch::run_parallel` is dead code there (wasm has no
   threads) but does not fail to build.
2. **`Batch`'s per-world column allocation is the cluster path's structural cost.** It forces
   a gather before an instanced draw, and — separately — native single-threaded step time
   degrades sharply with world count on this machine (§4.6). A cluster-native column pool
   would address both. Recorded as a finding; `crates/don-sim` is not this lane's to change.
3. **`World` exposes no velocity setter**, so the shim's order-follow pass writes positions
   through `set_pos` after the tick rather than steering. Fine for a demo; a real order
   system needs either a velocity/target column in `World` or the whole order system living
   in `don-sim`. The latter is obviously right.
4. **A 64-bit `Batch::digest()` agrees bit-for-bit between an aarch64 native build and a
   wasm32 build** over 1,000 frames of 4 worlds × 64 units (§4.7). That is a cross-target
   determinism check that could have failed, and it is cheap to run on every change:
   `cargo run --release --bin digest -- digest 4 64 64 1000 0xC0FFEE`.
