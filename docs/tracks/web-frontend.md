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

**A working WebGPU spectator holds this display's full 120 fps while simulating and drawing
4,096 independent worlds — 262,144 units — one simulation tick per rendered frame, on an
Apple M2 Max.** [measured] It costs 1.08 ms of GPU time and 3.05 ms of CPU on the render
thread, or **0.94 ms of render-thread CPU** when the simulation is moved onto four workers.
At the single-world scale a spectator actually watches (one world, up to `MAX_UNITS` = 4,096
units) the whole per-frame CPU cost is **0.062 ms** and the GPU cost **0.15 ms** — 0.7% and
1.8% of a 120 Hz frame. Above the readability threshold the aggregate view draws a
**1,048,576-unit, 16,384-world** cluster for 0.15 ms of GPU. The 60 fps bar in the lane brief
is nowhere near binding; the display refresh is.

Separately, and the number that matters for the RL endgame: **1,024 worlds simulate at about
1,000× real time in a browser tab** (16.1 M world-steps/s across 12 workers) while all of
them render at refresh rate.

The design claim underneath all of it, and the one worth arguing about:

> **`don-sim`'s SoA columns are already the layout the GPU's vertex stage wants.** Bind
> `pos_x` as one vertex buffer with `format: 'sint32'` and `stepMode: 'instance'`, `pos_y` as
> another, and the per-frame CPU work collapses to one `queue.writeBuffer` per column. There
> is no interleaved per-instance struct anywhere in this system, so there is no repack pass,
> no `f32` conversion pass, and no per-entity CPU loop at all.

Four qualifications, each measured, each of which changed what I would have said:

1. **The cluster path pays one CPU copy the single-world path does not** — `don_sim::Batch`
   gives every world its own `Vec` per column, and an instanced draw across thousands of
   worlds needs one contiguous range. It costs a fifth to a third of the upload it precedes
   (§4.3), which is *smaller* than I expected and reorders what is worth fixing.
2. **Moving the sim onto N workers does not raise the frame rate.** The renderer draws
   whatever has been published, so fps moves 1.08× from 1 to 12 workers. What moves 6.24× is
   simulation throughput (§4.5). Measuring worker scaling with fps would have concluded that
   worker parallelism does not work.
3. **The aggregate view is not a fallback**, it is the correct visualisation below ~18 device
   pixels per world, and it costs 0.11–0.15 ms of GPU regardless of cluster size because its
   instance count is the world count (§4.4).
4. **This machine sat at load average 50–174 throughout** — shared with other lanes and a
   running Parallels VM. Every CPU-side number is a pessimistic sample with real spread; the
   spread is reported, not averaged away, and the native reference varies 4× run to run,
   which is why §4.6 refuses to state a wasm-versus-native ratio.

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
  aggregate view is not a fallback, it is the correct visualisation — and it is also 4–10×
  cheaper on the GPU (§4.4), because its instance count is the *world* count rather than the
  unit count. Per-world statistics (live count, summed hit points, digest low bits, frame)
  are gathered in wasm into a 16-byte-per-world table and uploaded at UI rate, not frame
  rate.
- **Two metrics, because one of them is degenerate.** Colouring by population is the obvious
  choice, and it renders 16,384 identical full worlds as one flat sheet of colour — true,
  and useless. The alternative is the low bits of each world's `World::digest()`, a **state
  fingerprint**: two worlds in identical states get identical colours, and a world that
  diverges visibly changes. That is what a spectator of a *cluster* is actually looking for,
  and it is what makes a 16,384-cell mosaic worth having on screen. Selector in the panel;
  the shader picks between them on one uniform bit, so it costs nothing.

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

Every row runs **3 times** and the report keeps the median and the min/max. GPU time is
itself a median: `lastGpuNs` is whatever query pair resolved most recently, so reading it
once at the end of a run samples one arbitrary frame — two runs of identical work differed by
2× that way before this was fixed. It is now sampled every iteration and reduced. The load
average is recorded with every row. Simulation load is defined as **one simulation tick per rendered
frame** (`simHz: 0`); note that the engine's real tick rate is 15 Hz against a 120 Hz
display, so the realistic spectator workload is roughly **8× less simulation per frame** than
these rows carry.

Machine: Apple M2 Max (38-core GPU, 12 CPU cores, 96 GB), macOS 26.6, Chrome 153.0.7979.3
dev, WebGPU on Metal 3, `devicePixelRatio` 2, canvas ~2560×1826 device pixels.

**Contamination, stated plainly:** load average was 50–174 throughout, from a running
Parallels VM, Lean/`lake` builds, `rustc`, and other agents' work. CPU-side timings have
real spread (the native reference below varies by 4× on the same configuration). This makes
the CPU numbers *conservative* — the true costs are lower — and it makes any fine-grained
CPU comparison between two paths unreliable. GPU timestamps and the rAF cap are stable.

---

## 4. Numbers

All rows: WebGPU, Apple M2 Max, canvas 2560x1826 device pixels, **one simulation tick per
rendered frame**, median of 3 runs x 2.5 s each. Raw JSON: `web/results.json`. Load average
149 at the start of the run, 77 at the end. Zero page errors or exceptions across the whole
suite. All [measured].

"Render-thread CPU" is the per-frame cost *on the thread that draws*. On the `sab` path the
step/gather/copy columns happen on other cores and are shown in italics, because adding them
to the renderer's budget would be simply wrong.

### 4.1 One world: frames per second against entity count

`zerocopy` path — sim in the render worker, vertex buffers filled from `World::pos_x` itself,
**no CPU copy at all**.

| units | rAF fps | uncapped fps | step ms | upload ms | encode ms | render-thread CPU | GPU ms |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 128 | **120.4** | 1013.6 | 0.001 | 0.004 | 0.026 | **0.031** | 0.108 |
| 512 | **120.4** | 989.1 | 0.003 | 0.005 | 0.030 | **0.038** | 0.110 |
| 1,024 | **120.4** | 1004.7 | 0.005 | 0.006 | 0.026 | **0.038** | 0.128 |
| 2,048 | **120.4** | 957.2 | 0.008 | 0.010 | 0.030 | **0.048** | 0.135 |
| 4,096 | **120.4** | 894.2 | 0.015 | 0.017 | 0.030 | **0.062** | 0.150 |

120.4 fps is this display's refresh rate; the rAF loop is refresh-locked at every size. At
4,096 units — `don_sim::MAX_UNITS`, the most a single world can currently hold — the entire
per-frame CPU cost is **0.062 ms, or 0.7% of a 120 Hz frame budget**, and the GPU is busy
0.15 ms. Entity count is not the binding constraint on this path by any margin; the display
is.

Read the uncapped column as **per-frame latency including a full CPU-GPU fence**. At 128
units the work is 0.031 ms of CPU plus 0.108 ms of GPU, and a frame still takes 0.99 ms — so
about 0.85 ms of it is the round trip. It is a useful comparative number and a misleading
absolute one.

### 4.2 Cluster: frames per second against world count

64 units per world. `inline` = one wasm instance inside the render worker (one gather copy);
`sab` = 4 sim workers publishing through shared memory.

| worlds | instances = live units | rAF fps | uncapped, `inline` | uncapped, `sab` x4 | GPU ms |
|---:|---:|---:|---:|---:|---:|
| 16 | 1,024 | **120.4** | 984.6 | 1041.0 | 0.115 |
| 64 | 4,096 | **120.4** | 903.7 | 1028.5 | 0.126 |
| 256 | 16,384 | **120.4** | 756.9 | 896.6 | 0.180 |
| 1,024 | 65,536 | **120.4** | 454.6 | 655.4 | 0.345 |
| 4,096 | 262,144 | **120.4** | 167.5 | 299.0 | 1.076 |

**The display cap holds at every size measured**, up to 4,096 worlds and 262,144 units, on
both paths. Where the frame budget goes at the top row:

| 4,096 worlds x 64 units | step | gather | wasm→SAB | upload | encode | render-thread CPU | GPU |
|---|---:|---:|---:|---:|---:|---:|---:|
| `inline` (sim on the render thread) | 1.837 | 0.315 | — | 0.836 | 0.061 | **3.049 ms** — 37% of a 120 Hz frame | 1.076 |
| `sab` x4 (sim on four other cores) | *0.808* | *0.175* | *0.077* | 0.889 | 0.053 | **0.943 ms** — 11% | 1.075 |

So the `sab` path's real payoff is not frame rate, which is capped anyway: it is that **the
renderer's own per-frame cost drops 3.2x**, from 3.05 ms to 0.94 ms, because stepping and
gathering left the thread that draws. The same effect at 1,024 worlds is 0.70 ms → 0.26 ms.

Two things to hold onto about these rows. First, one sim tick per rendered frame at 120 Hz is
**8x the engine's real 15 Hz tick rate**, so a spectator watching at true speed pays roughly
an eighth of the `step` column. Second, at 4,096 worlds the automatic view switch has *not*
engaged (33 device pixels per cell against an 18 px threshold), so this is the genuinely
expensive drawing mode — see §4.4.

### 4.3 What each copy costs

Priced from the rows above, per frame:

| | 1,024 worlds (65,536 cells) | 4,096 worlds (262,144 cells) |
|---|---:|---:|
| `sim_gather`, the cluster mirror (`inline`/`sab`) | 0.047 ms → 1.4 G cells/s | 0.315 ms → 0.83 G cells/s |
| wasm → SharedArrayBuffer (`sab` only, per shard) | 0.024 ms | 0.077 ms |
| `queue.writeBuffer` — **the copy nobody can avoid** | 0.215 ms | 0.836 ms |

The copy the cluster path adds is **roughly a fifth to a third of the copy that cannot be
removed**, and the shared-memory hop is smaller again. Two consequences worth stating:

- At these sizes the extra copies are not what to optimise. The upload dominates, and the
  upload is the floor for any design that keeps the simulation in wasm.
- Shared wasm memory (`+atomics`, `-Zbuild-std`) would delete only the second row, the
  smallest one. Changing `Batch` to a cluster-native column pool would delete the first,
  which is 4x larger. If either is ever worth doing, it is the `don-sim` change — and that
  is the opposite of where I would have guessed before measuring.

### 4.4 Drawing alone, and what the aggregate view buys

Render only: no stepping, no uploads. This isolates the GPU cost of the two view modes.

| worlds | instances | units view: fps / GPU ms | aggregate view: fps / GPU ms |
|---:|---:|---:|---:|
| 1,024 | 65,536 | 817 / **0.342** | 975 / **0.122** |
| 4,096 | 262,144 | 622 / **0.617** | 998 / **0.114** |
| 16,384 | 1,048,576 | 361 / **1.472** | 952 / **0.149** |

The aggregate view costs **essentially nothing extra as the cluster grows** — 0.11–0.15 ms
whether it summarises 1,024 worlds or 16,384 — because its instance count is the *world*
count, not the unit count. That is what makes it the right answer above the readability
threshold rather than a consolation prize. At 16,384 worlds x 64 units the page is drawing a
**1,048,576-instance** unit view at 361 fps, or the aggregate of the same cluster at 952.

One honest wrinkle: draw-only at 4,096 worlds reports 0.617 ms of GPU while the full
pipeline at the same configuration reports 1.076 ms. Draw-only should be the cheaper of the
two and it is, but not by the amount that difference implies. The plausible cause is that
the full pipeline's vertex fetch reads 2 MB of *just-written* buffer each frame while
draw-only re-reads resident data — that is a **hypothesis, not a measurement**; I did not
isolate it.

### 4.5 Simulation throughput against worker count

**This is the axis frame rate cannot see.** The renderer draws whatever the shards have
published, so render fps barely moves with shard count (653 → 706 fps from 1 to 12 workers,
§4.2's `shards` sweep). What moves is how fast the cluster actually simulates, read from the
shards' own frame counters over a 2 s window at 1,024 worlds x 64 units:

| sim workers | M unit-steps/s | M world-steps/s | speed-up | x real time (15 Hz tick) |
|---:|---:|---:|---:|---:|
| 1 | 164.7 | 2.57 | 1.00x | 168x |
| 2 | 307.8 | 4.81 | 1.87x | 313x |
| 4 | 556.9 | 8.70 | 3.38x | 567x |
| 6 | 788.2 | 12.32 | 4.79x | 800x |
| 8 | 946.4 | 14.79 | 5.75x | 963x |
| 12 | 1028.5 | 16.07 | 6.24x | **1038x** |

**A browser tab simulates 1,024 worlds at about 1,000x real time while rendering all of them
at the display refresh rate.** Scaling is near-linear to 4 workers, still 5.75x at 8, and
flattens by 12 — on a 12-core machine already at load average 50–80 from other work, with a
render worker also running. Treat this as a floor on the scaling, not a ceiling.

Measuring shard scaling with fps would have shown a 1.08x "speed-up" and concluded that
worker parallelism does not work. It was the wrong instrument, and swapping it changed the
answer by 6x.

### 4.6 Native reference, and why it is only a sanity check

Same code, host build, single-threaded, median of 5 (`digest bench`):

| configuration | native step ms [min..max] | native M unit-steps/s | wasm in browser |
|---|---:|---:|---:|
| 1 world x 4,096 units | 0.017 [0.008..0.049] | 247 | 273 |
| 1,024 worlds x 64 units | 0.241 [0.165..0.701] | 272 | 165 (1 worker) |
| 4,096 worlds x 64 units | 3.134 [1.383..6.392] | 84 | 143 |

**The native spread is 4x on the same configuration**, so this table supports exactly one
conclusion: wasm and native are within the same order of magnitude on this workload. I am
deliberately not reporting a "wasm is X% of native" figure, because this data cannot carry
one.

The native numbers do show one thing that is not noise, because it is monotone across five
configurations: **single-threaded step throughput collapses as world count rises** — 276 M
unit-steps/s at 256 worlds, 272 M at 1,024, 84 M at 4,096, 41 M at 16,384 — while a single
4,096-unit world sustains 247 M. Identical total unit counts, 3–6x the cost. The arithmetic
is the same, so the difference is memory layout: 4,096 worlds means roughly 53,000 small
separate allocations instead of a handful of large ones. See §7.2.

### 4.7 Cross-target determinism — the check that could have failed

4 worlds x 64 units, capacity 64, seed `0xC0FFEE`, exactly 1,000 frames:

| build | `Batch::digest()` |
|---|---|
| aarch64-apple-darwin — `cargo run --release --bin digest -- digest 4 64 64 1000 0xC0FFEE` | `0x1b07da068f50ba66` |
| wasm32-unknown-unknown in Chrome — `window.don.stepFrames(1000)` then `digest()` | `1b07da068f50ba66` |

**Identical.** Two independently compiled targets — one where LLVM autovectorises `don-sim`'s
tick kernels to NEON, one where it does not — agree bit-for-bit on a 64-bit digest after
256,000 unit-frames. This proves nothing about the *game*; it is evidence that the browser is
running the same simulation the host runs, which is the precondition for a spectator being
worth looking at. It re-runs in one command and it would fail loudly.

### 4.8 WebGL2 fallback

Forced with `?backend=webgl2`. Same architecture: integer instance attributes, one draw call.

| configuration | instances | rAF fps |
|---|---:|---:|
| 1 world x 2,048 units | 2,048 | **120.6** |
| 256 worlds x 64 units | 16,384 | **120.4** |
| 1,024 worlds x 64 units | 65,536 | **120.5** |
| 4,096 worlds x 64 units | 262,144 | **120.5** |

The fallback holds the display cap at every size and its screenshot is indistinguishable
from the WebGPU one. **Its uncapped numbers are deliberately not reported**: WebGL2's only
barrier is `gl.finish()`, which in Chrome returns once commands reach the GPU process rather
than once the GPU has drained, so the uncapped loop there measured 18,000 fps — an artefact,
not a result. There are no timestamp queries on this path either, so its GPU time is simply
unmeasured.

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

Visually this is unambiguous: click inside one world of a four-world view and that world's
512 units converge on the point while the other three carry on undisturbed. The converging
swarm forms a cross rather than a disc, which is not a bug in the plumbing — it is the
Chebyshev overshoot of clamping each axis independently in the placeholder follow step, and
it is a good reminder of what is and is not derived here.

The order record is deliberately integer-only and fixed-size:

```rust
pub struct Order { kind: u32, world: u32, owner: u32,
                   sx: i32, sy: i32, tx: i32, ty: i32, radius: i32 }
```

Nothing about that is specific to a human. An RL policy emits the same record. The
*mechanism* — a command queue drained at a deterministic tick boundary — is the property
that makes spectating, playing, replaying, and training the same code path, and it is the
property lockstep requires.

### 5.2 The action space is derived, and this page already speaks its opcodes

While this lane was running, the `headless-client` lane derived the **whole command wire
format** from the shipped `rise.pdb` — `schema/command-wire.json`, 82 entries, each with its
struct name, byte size and complete field layout, cross-checked against
`CommandPackage::process` = `FUN_0094a700`. I read that file [measured: 82 entries, opcodes
`0x00`–`0x51`, no gaps]; the derivation itself is their lane's claim, not mine, and
`docs/tracks/headless-client.md` is where it is defended.

That file *is* the action space, and it is far more specific than the RTTI class list I had
started from. It settles several things this section previously had to speculate about:

| opcode | struct | size | fields |
|---|---|---:|---|
| `0x00` | `GroupCommand` | 5 | `num, who, list` |
| `0x07` | `MoveToCommand` | 22 | `to_x, to_y, set_angle, angle, orders, queued, form, width, disembark` |
| `0x04` | `AttackCommand` | 17 | `ox, whom, ignore, queued` |
| `0x0a` | `PatrolCommand` | 10 | `to_x, to_y, queued` |
| `0x0c` | `HaltCommand` | 1 | — |
| `0x13` | `GatherCommand` | 9 | `ox, queued` |
| `0x19` | `BuildCommand` | 25 | `x, y, x2, y2, type, queued` |
| `0x39` | `CheckSumsCommand` | 65 | sixteen named `*_checksum` channels |

- **Selection is its own command.** `GroupCommand` (`0x00`) carries `num, who, list`, so the
  engine's action space is genuinely *selection then command*, not per-unit commands. An RL
  action space that emits one command per unit is modelling a different game.
- **`queued` appears on nearly every order.** Shift-queueing is a first-class field of the
  wire format, so it is a per-action parameter the policy must emit, not a UI convenience.
- **`MoveToCommand` has nine fields, not two.** `form`, `width`, `orders`, `set_angle`,
  `angle`, `disembark` are all part of a move. This is exactly the "full-game action space,
  not a toy subset" the charter insists on, and it is now enumerable rather than guessed.
- **The 65-byte `CheckSumsCommand` names all sixteen channels** (`units_`, `builds_`,
  `walls_`, `ammo_`, `deaths_`, `groups_`, `guys_`, `leaders_`, `cities_`, `items_`,
  `goods_`, `world_`, `rules_`, `scenario_data_`, `script_run_time_`, `all_`). For a
  spectator that is a ready-made per-world divergence display (§7.5).

**This page now uses the real opcodes.** `don_web::order_kind` no longer holds invented
constants: a click emits `kind = 0x07`, the actual `MoveToCommand` opcode, and "stop" is
`0x0c` `HaltCommand`. The one non-engine action (adding a unit, a demo affordance) is
`0x1000`, deliberately outside the engine's `0x00`–`0x51` range so it can never be mistaken
for a command. What the shim honours is a strict subset — `to_x` and `to_y` of the nine
fields — and both the Rust and the JS say exactly that at the definition site. Placeholder
mechanics have nothing for `form` or `queued` to mean yet, and inventing values for them
would be worse than leaving them out.

The RTTI `Order` classes (32 of them in `schema/vtables.json`, including `MoveOrder`,
`AttackToOrder`, `GatherOrder`, `GuardOrder` and their `Group…` variants) are the *engine
side* of the same thing: a command arrives on the wire, and the unit ends up holding an
`Order`. Both halves are now named; the mapping between them is not derived.

### 5.3 The concrete path from here

1. **Replace the envelope with the real structs.** `don_web::Order` is a fixed-size integer
   superset with `kind` as the opcode. `schema/command-wire.json` is machine-readable, so the
   82 command structs and their encoders/decoders should be *generated* from it, not typed
   out. That is a `don-sim`/`don-net` job; the browser then carries the generated type.
2. **Selection before command.** The click path currently applies an order to every unit of
   an owner inside a radius. The engine says selection is `GroupCommand` (`0x00`), so the UI
   wants a real selection model whose output is that packet.
3. **Parameter-level masking, from the start.** The charter is explicit that unmasked RTS
   action spaces measurably train to zero, and the field lists above show how large the
   parameter space per action is. The web client is the natural place to *see* the mask —
   the predicate that greys out an illegal click is the same mask the policy gets. Not built.
4. **Join a running cluster.** Spectating is read-only today; a participant needs its orders
   to reach the shard owning its world, which the `sab` path already does locally
   (`postMessage` to the owning sim worker). Over a network the same record goes over a
   socket, and `CheckSumsCommand` (`0x39`) is what lets a joiner verify it is in sync rather
   than hope.

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
- **WebGL2 GPU time, and any WebGL2 throughput number.** The fallback is measured at the
  display cap up to 262,144 instances (§4.8), and that much is real. Beyond it: `gl.finish()`
  is not a GPU fence in Chrome, so the uncapped loop is meaningless there, and WebGL2 has no
  timestamp query on this path, so its GPU occupancy is simply unknown. I know the WebGL2
  path *keeps up*; I do not know by how much.
- **Anything above 4,096 worlds on the `sab` path.** 16,384 worlds × 64 units
  (1,048,576 units) is in the draw-only data and runs interactively at the display cap, but
  it was never swept with the simulation on separate workers.
- **Where the extra 0.46 ms of GPU time in the full pipeline at 4,096 worlds comes from**
  (§4.4). I have a plausible cause and no measurement of it.
- **Behaviour on any machine but this one.** One GPU, one browser, one OS. Nothing here
  generalises without re-measurement, and `bench.mjs` exists so that re-measurement is one
  command.
- **Whether the aggregate view's metrics are the right ones.** Population, summed hit points
  and a digest fingerprint are what `don-sim` exposes today. What a *spectator of a training
  run* actually needs to see — reward, divergence from a reference trajectory, a checksum
  mismatch against the engine's own 16-channel `check_all` tuple — is underived, because
  none of those quantities exist yet.

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
5. **`World::digest()` is already a usable per-world state fingerprint for visualisation**, and
   it is what makes a 16,384-world mosaic readable at all (§2.8). The aggregate view is a
   drop-in home for the engine's own checksum: `CheckSumsCommand` (`0x39`) carries sixteen
   named channels, so colouring cells by `all_checksum` — or by *which* of the sixteen
   disagrees — turns the cluster view into a live desync display with no new rendering work.
6. **Frame rate was the wrong instrument for worker scaling and it gave the wrong answer by
   6×** (§4.5). The renderer consumes published state, so it is decoupled from how fast the
   simulation runs; the shards' own frame counters are the only truthful source. Any future
   throughput lane measuring an asynchronous producer through a consumer's rate has the same
   trap waiting for it.
7. **Two silent-failure modes cost real time here and are cheap to pre-empt.** A module worker
   whose source fails to parse never posts a message, so the page hangs with no error
   anywhere unless you listen for `error` and `messageerror` on the `Worker` — the page now
   does. And a stale module-level variable read on a path that never writes it reported a
   *previous configuration's* step time as the current one; the fix was to make the timing
   source a function of the active path rather than a shared mutable.
8. **`schema/command-wire.json` is immediately usable by the client, and should be code-genned
   rather than transcribed.** This lane switched its order path onto the real opcodes
   (`0x07 MoveToCommand`, `0x0c HaltCommand`) in minutes just by reading the file. The 82
   command structs deserve a generated Rust encoder/decoder that both `don-net` and this page
   consume, so no hand-written copy of a field layout can drift from the derived one.
