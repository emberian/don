# GPU and SIMD execution architecture

**Lane: `gpu`. This is design and prototype, not binary derivation.** Nothing here
implements a Rise of Nations mechanic and nothing here should be read as a claim about how
the game computes anything. The charter rule about deriving mechanics from the binary is
not relaxed — it is *not engaged*, because this lane deliberately designs around the
**shapes** of the work (locality, convergence, branchiness, memory traffic) rather than
around any numbers. Where I needed a number to size a benchmark I took it from the shipped
data and said so; where the shipped data does not settle it, I bracketed the range and said
that instead.

Every claim is marked **[measured]** (I ran it on this machine or read it out of our own
files) or **[reported]** (I read it in a source and have not checked it). One item in the
lane brief turned out to be a misreading of its source; it is corrected in §3 and the
correction is itself measured, from the paper.

---

## 0. Headline

**On an Apple M2 Max, a wgpu compute port of the single best-fit kernel in this simulation
— batched integer flow-field relaxation — runs the kernel about 6× faster than a
NEON-vectorised CPU version of the same kernel, and still loses to the CPU overall until
roughly 1000–2000 concurrent fields, because relaxation does 300–600× more arithmetic than
the bucket-queue Dijkstra a CPU would actually run.** [measured] With host round-trips
included it does not win at any size I could allocate. The GPU is not the wrong answer, but
it is only the right answer under three conditions that the prototype makes concrete: batch
≳1000 fields, the result stays resident on the device, and convergence is tracked **per
field** rather than per batch — the prototype's single batch-wide convergence flag costs up
to 23× on realistic incremental updates, and that is the largest fixable defect it found.

The prototype is at `/Users/ember/dev/don/crates/don-gpu/`, builds and tests green on this
arm64 Mac, and its GPU output is asserted bit-identical to three independent CPU
implementations on every shape it runs.

---

## 1. Factoring the simulation into work classes

The useful axis is not "parallel vs serial" — everything is parallel across worlds. It is
**arithmetic intensity per unit of divergence**, and whether the work is *local and
convergent* or *global and exact*.

### 1a. Stencil / relaxation shaped — strong GPU fit

These are local, iterative, and converge to a fixed point that does not care about
evaluation order. This is exactly the class the user identified, and it is the only class
where I would put a GPU kernel today.

| system | shape | why it fits | status in this project |
|---|---|---|---|
| flow-field / potential-field pathfinding | min-plus relaxation over a tile grid | unique least fixed point, 8-neighbour stencil | **prototyped here** |
| influence / threat maps | max-plus or additive diffusion from sources | same stencil, same batching | not derived |
| territory and border fields | region growth from city seeds | RTTI says `BorderSpline`, so borders are **splines**, not a field — see caveat below | not derived |
| fog of war / visibility | boolean OR accumulation, then a per-player mask | idempotent, order-free, packs to bitmaps | not derived |
| terrain cost fields | pure map over terrain type, then a stencil for smoothing | trivially parallel | not derived |

**Caveat that matters:** `BorderSpline` is a measured RTTI class name
(`docs/binary-ground-truth.md`), which means the engine's borders are spline geometry, not
a rasterised field. A GPU territory-field kernel would be *our* approximation of something
the engine does differently. That is a fidelity decision, not a performance one, and it is
not this lane's to make.

### 1b. Gather / scatter over SoA columns — strong SIMD fit, uncertain GPU fit

Economy tick, attrition, cooldowns, projectile integration, combat resolution. These are
one or two passes over parallel columns with a handful of arithmetic ops per element. They
vectorise beautifully and they are **bandwidth-bound**, which is the worst possible profile
for shipping data across a bus — and on this machine the "bus" is unified memory, so the
GPU has no bandwidth advantage worth the launch overhead.

The sibling `simd` lane is measuring this class directly in `crates/don-sim`
(`docs/derivation/simd-batch.md`), and its numbers already dominate anything a GPU would do
at these batch sizes. I measured one instructive data point of my own inside this lane:
restructuring the flow-field Jacobi sweep from an index-loop with bounds checks and eight
branches into four shifted-slice `min` reductions made it **9.7× faster on the same 12
threads** (`19.4 ms → 2.0 ms` at 64×64 × 24 fields) [measured], and `otool -tv` on the
release binary shows 40 `umin.4s` NEON instructions in the hot loop [measured] — LLVM
auto-vectorised it once the branches were gone. That factor of ~10 is available on the CPU
for free, and it is a factor of 10 the GPU has to beat *before* it starts winning. Any
CPU-vs-GPU comparison that does not vectorise the CPU side is not a comparison.

### 1c. Branchy / serial / small — keep on the CPU

Order execution (the engine's own 27-class `Order` hierarchy, measured), build queues, tech
unlocks, AI script evaluation, city management. Divergent control flow, pointer chasing,
tiny working sets, and a per-world state machine. On a GPU these become warp-divergence
disasters; Madrona's own measurement of the naive design (§3) puts the naive backend at
**1.9% of peak compute and 1.9% of peak bandwidth** [measured, from the paper] for exactly
this reason.

There is a subtler reason to keep them on the CPU that is specific to *this* project: these
are the systems whose fidelity we are trying to prove. A Tier-A or Tier-B claim is made
against a Rust function that an oracle can call and Kani can reason about. A WGSL kernel is
neither callable from the oracle harness nor reachable by any verifier we have. **Moving a
mechanic to the GPU currently costs us the ability to make a tier claim about it.** That is
an architectural constraint the charter implies but does not spell out, and it should be
spelled out: GPU kernels belong to systems where we are willing to hold Tier C, or where
the kernel is a *helper* whose output the CPU re-derives cheaply.

### 1d. The class that decides everything: reductions

Anything that sums. Income aggregation, damage accumulation onto a shared target, score.
See §5 — with `f32` this class is where GPU determinism dies, and the fix is representation
(fixed point), not scheduling.

---

## 2. Grid sizes: what the shipped data actually says

I needed a realistic tile-grid size to benchmark against, so I went to the data rather than
guessing.

`ron-data/rules.xml` lines 1884–1905 [measured]:

```xml
<CATEGORIES id="mapsizes" title="Map Size:">
  <CATEGORY name="Tiny"   key="Tiny"><DATA>40</DATA></CATEGORY>
  <CATEGORY name="Arena"  key="Arena"><DATA>50</DATA></CATEGORY>
  ...
  <CATEGORY name="Big Huge" key="Big Huge"><DATA>100</DATA></CATEGORY>
</CATEGORIES>
```

so the seven map sizes carry the linear parameters 40, 50, 60, 70, 80, 90, 100.

**The unit is not settled.** The same file's header comment (line 14) states *"Map distances
and speeds are specified as multiples or fractions of a 'tile' (aka 'TCoord'). A farm is 4
TCoords wide, or 1 WCoord wide"* [measured] — so the engine has at least two coordinate
scales at a 4:1 ratio. If `DATA` is in TCoords the largest map is 100×100 tiles; if it is
in WCoords it is 400×400. I did not resolve which, so the benchmark sweeps **64², 128² and
256²**, which brackets both readings. Resolving it is a one-line job for whichever lane
touches the map loader, and it changes the GPU verdict materially: at 100² a flow field is
10,000 cells and the GPU is hopeless; at 400² it is 160,000 cells and the picture changes.

---

## 3. The batched execution model — and a correction to the brief

I read the Madrona paper (Shacklett et al., *An Extensible, Data-Oriented Architecture for
High-Performance, Many-World Simulation*, ACM TOG 42(4), 2023) rather than working from the
summary. All four claims below are **[measured] from the paper text**, and one of them
contradicts the lane brief.

**What the design is.** One table per archetype spanning *all* worlds, with an **implicit
`EnvID` component** in every archetype mapping each row back to its environment. Component
columns are contiguous in virtual address space, and Madrona *"allows the application to
expose ECS component data … as tensors that alias the same GPU memory used by the
archetype's column store"*, so *"the resulting tensor is automatically batched across all
environments as a consequence of the single-table-per-archetype design"*. Deletion sets
`EnvID` to a sentinel `-1` — *"requires no synchronization between deleting threads"* — and
reclamation is a periodic **parallel radix sort on the `EnvID` column** that moves the `-1`
rows to the end so the table can be truncated. Sorting also improves coherence, and they
sort at the start of every step conditioned on the table having changed.

**The 42× figure is right, but it is not about world count.** *"ECS-GPU's peak performance
is over 42× slower than BATCH-ECS-GPU (and over 3× slower than ECS-CPU)"*, where ECS-GPU is
the one-environment-per-CUDA-thread port.

**The "8000 worlds" figure in the brief is a misreading, and the real number is far
better.** The paper's 8K is the *memory ceiling of the naive backend*: *"memory capacity
limits ECS-GPU to approximately 8K concurrent environments"* — a statement about
fragmentation in the design they are criticising. The actual crossover for their design is
*"BATCH-ECS-GPU begins to outperform ECS-CPU with just 500 environments for HideSeek and
200 environments for Overcooked."* On an i9-13900K (32 threads) plus an RTX 4090.

**200–500 worlds, not 8000.** This matters because it changes the strategic read: batch
GPU simulation is viable at batch sizes RL already uses. Note the hardware, though — a
discrete 4090 against a desktop CPU is a very different balance from an M2 Max's integrated
GPU against its own P-cores, and my measurements (§4) land much less favourably.

### 3a. What we can copy, and the one thing we cannot

| Madrona element | portable to wgpu? |
|---|---|
| one column-store table spanning all worlds, implicit world id | **yes** — implemented in `FieldBatch` |
| columns *are* the export tensors, zero copy | **yes** in principle; a `wgpu::Buffer` can back a DLPack/torch tensor on CUDA, and on Metal via `MTLBuffer` |
| `EnvID = -1` deletion sentinel + periodic radix compaction | **yes** — a radix sort is a standard multi-pass compute kernel |
| **megakernel + persistent threads task graph** | **no** |

The last row is the load-bearing one and it deserves to be stated plainly. Madrona compiles
*all* systems into **one** CUDA kernel launched once per batch step, executed with a
*"persistent-threads style design, where warps of threads loop repeatedly, fetching work at
warp-level granularity"*. They do this precisely because *"the overhead of frequent CPU
synchronization would severely limit performance"* for graphs with many small dynamic
systems.

**WebGPU/wgpu cannot express that.** There is no forward-progress guarantee across
workgroups, no device-side launch, no cooperative-groups equivalent. A persistent-threads
scheduler written against wgpu is not merely slow, it is *unsound* — a workgroup spinning on
work produced by a workgroup that has not been scheduled can deadlock, and the spec permits
it.

My prototype pays this cost in a visible place: convergence detection needs a flag readback
to the host, and §4c measures exactly what that stall costs (up to **7× at the wrong poll
frequency**). Two real mitigations exist inside wgpu, and both belong in any future design:

1. **Amortise the readback** — run N rounds per submission, poll once. Measured below.
2. **`dispatch_workgroups_indirect`** — the workgroup count is read from a device buffer, so
   a kernel can size the *next* kernel's dispatch without a host round trip. This covers
   Madrona's "dynamic number of entities" case without a megakernel. It does not give
   device-side control flow, so it cannot replace the task graph, but it removes the
   readback from the common case.

If we ever want the full Madrona shape we need CUDA or Metal directly, and that trades away
portability and the ability to develop on this laptop. **Recommendation: stay on wgpu,
design for a small number of large kernels rather than a graph of small ones, and treat any
host round-trip inside a tick as a bug.**

---

## 4. The prototype and its measurements

`/Users/ember/dev/don/crates/don-gpu/` — added to the workspace members in
`/Users/ember/dev/don/Cargo.toml`.

```
crates/don-gpu/
  src/field.rs               FieldBatch: one flat column per quantity, world id implicit
  src/cpu.rs                 three independent CPU solvers + flow-direction extraction
  src/gpu.rs                 wgpu host: buffers, ping-pong bind groups, convergence loop
  src/shaders/flowfield.wgsl the relaxation kernel (integer, workgroup-tiled, temporally blocked)
  src/shaders/directions.wgsl argmin flow-direction extraction
  src/bin/flowbench.rs       the benchmark that produced every number below
  tests/parity.rs            GPU/CPU parity and schedule-independence
```

### 4a. What the kernel computes

The least fixed point of a **min-plus (tropical) relaxation** over `u32`:

```
d[c] = min( d[c],  min over the 8 neighbours n of ( d[n] + w(n,c) * cost[c] ) )
```

with `w = 2` cardinal and `w = 3` diagonal — the 2/3 chamfer metric, which approximates
Euclidean distance to within ~5.4% and is exactly representable in integers. `cost[c]` is
the entry cost of a cell; `COST_BLOCKED` is impassable. There is not one floating-point
value anywhere in the kernel. **These weights and this cost model are ours**, chosen to
exercise the hardware; the engine's `PathFinder` is not derived and this is not a
hypothesis about it.

The GPU kernel is a workgroup-tiled Jacobi: each 8×8 workgroup loads its tile plus a
one-cell halo (10×10 `u32`) into workgroup memory, runs `inner_steps` relaxations there
against a **stale halo**, and writes back. Temporal blocking trades halo staleness for a
factor of `inner_steps` less global traffic. Staleness is sound here because every step is
`d = min(d, neighbour + w)` — values only decrease and can never fall below the true
distance, so an under-informed step is a slower step, never a wrong one.

Three CPU implementations exist and are asserted equal:

- `Jacobi` — the scalar analogue of the WGSL kernel (the honest kernel-for-kernel baseline);
- `JacobiRows` — the same arithmetic factored into four-way shifted-slice `min`s so LLVM
  auto-vectorises it (confirmed: 40 `umin.4s` in the release binary);
- `Dial` — a bucket-queue Dijkstra, `O(cells)` instead of `O(sweeps × cells)`. **This is
  what anyone would actually write**, and it is the baseline that decides the question.

### 4b. Kernel for kernel — the comparison the GPU flatters itself with

Apple M2 Max, 12 CPU threads, 38-core integrated GPU, wgpu 27.0.1 / Metal, release build,
best of 3, `inner_steps = 8`, `rounds_per_poll = 8`. Times are milliseconds for the whole
batch. `sweeps` is how many Jacobi sweeps one field needs to converge. [measured]

| grid | fields | sweeps | CPU 1t scalar | CPU 12t scalar | CPU 12t vectorised | GPU | GPU vs vec CPU |
|---|---|---|---|---|---|---|---|
| 64×64 | 24 | 114 | 111.9 | 19.4 | **2.0** | 5.2 | 0.38× |
| 64×64 | 96 | 114 | 471.1 | 70.1 | **8.0** | 7.9 | 1.01× |
| 128×128 | 24 | 209 | 978.8 | 144.5 | 11.6 | **7.7** | 1.51× |
| 128×128 | 96 | 209 | 3776.4 | 820.0 | 44.0 | **19.0** | 2.32× |
| 256×256 | 24 | 450 | 7443.7 | 1069.1 | 64.2 | **26.1** | 2.46× |

Two readings of the same table, and the difference between them is the whole point:

- **against unvectorised multithreaded C-shaped Rust the GPU is up to 43× faster** (the
  128×128 × 96 row: 820.0 ms vs 19.0 ms). That number is real and it is also **meaningless**,
  because the CPU code it beats is code nobody should ship.
- **against the vectorised CPU the same GPU is 0.4–2.5×.**

Raw kernel throughput, which is the device-vs-device number free of algorithm choice:
**GPU ≈ 70 G cell-updates/s** (256×256 × 64 fields, 80 rounds × 8 inner steps in 38.3 ms)
against **CPU ≈ 11 G cell-updates/s** on 12 threads (256×256 × 24 fields, ~450 sweeps in
64.2 ms) [measured]. So the honest device-vs-device factor on this hardware is **~6×**, not
43×. The per-field sweep count is measured on one field while the batch runs to the slowest,
so the CPU figure is a lower bound and the true ratio is if anything smaller. Addressed
memory traffic at
that rate is ~125 GB/s, against a quoted 400 GB/s for M2 Max [reported] — about 31%, which
says the kernel is not yet bandwidth-saturated and that packing (§6) has room.

### 4c. GPU against the algorithm a CPU would actually run

Same hardware, `inner_steps = 8`, `rounds_per_poll = 8`. `dial` is the bucket-queue
Dijkstra. `gpu+io` includes upload and readback. **The GPU result is asserted bit-identical
to the CPU result before any timing is printed.** [measured]

| grid | fields | Mcells | dial 1t | dial 12t | GPU | GPU+io | GPU Mcell/s | GPU / dial-12t |
|---|---|---|---|---|---|---|---|---|
| 64×64 | 1 | 0.004 | 0.1 | 0.1 | 3.9 | 6.5 | 1.0 | **0.02×** |
| 64×64 | 64 | 0.26 | 5.9 | 1.2 | 5.2 | 8.1 | 50.8 | 0.24× |
| 64×64 | 256 | 1.05 | 24.3 | 4.5 | 10.2 | 14.3 | 103.3 | 0.44× |
| 64×64 | 1024 | 4.19 | 99.1 | 19.3 | 22.8 | 32.6 | 183.9 | 0.84× |
| 64×64 | 4096 | 16.78 | 403.0 | 77.4 | **53.2** | 88.8 | 315.1 | **1.45×** |
| 128×128 | 64 | 1.05 | 23.1 | 4.9 | 10.3 | 14.4 | 102.1 | 0.47× |
| 128×128 | 256 | 4.19 | 94.9 | 20.0 | 30.5 | 39.4 | 137.5 | 0.66× |
| 128×128 | 1024 | 16.78 | 389.7 | 78.6 | **78.2** | 107.7 | 214.6 | **1.00×** |
| 256×256 | 64 | 4.19 | 94.3 | 19.0 | 43.4 | 53.4 | 96.6 | 0.44× |
| 256×256 | 256 | 16.78 | 396.8 | 88.6 | 132.4 | 164.2 | 126.7 | 0.67× |

**The crossover, stated plainly.** [measured]

- **64×64: ~2000 fields.** 0.84× at 1024, 1.45× at 4096.
- **128×128: ~1000 fields.** Exactly 1.00× at 1024.
- **256×256: not reached** below my 24 M-cell allocation cap; still 0.67× at 256 fields, and
  the trend suggests somewhere past 512.
- **Including host I/O: never.** At the best point measured (64×64 × 4096) the GPU is 53.2 ms
  of compute but 88.8 ms with upload and readback, against 77.4 ms for the CPU. On unified
  memory the round trip costs as much as the kernel.

**Why the GPU loses despite winning the kernel by 6–9×.** Relaxation needs 320–640 sweeps
where Dijkstra needs one pass. The GPU is doing two to three orders of magnitude more
arithmetic and paying for it with a 6–9× device advantage plus however much batching
recovers. That is the entire story, and it is why "GPU-accelerate the pathfinder" is a
worse idea than it sounds.

### 4d. The two knobs, and what they cost

256×256 × 64 fields, best of 2. `rounds` is global relaxation rounds; `polls` is host
readbacks. [measured]

| inner_steps | rounds_per_poll | rounds | polls | ms |
|---|---|---|---|---|
| 1 | 1 | 490 | 490 | 726.6 |
| 1 | 64 | 576 | 9 | 111.3 |
| 4 | 8 | 144 | 18 | 80.6 |
| **8** | **8** | **80** | **10** | **38.3** |
| 8 | 64 | 192 | 3 | 72.6 |
| 16 | 8 | 72 | 9 | 56.4 |
| 32 | 64 | 128 | 2 | 182.3 |

- **Host synchronisation costs up to 7×** (726.6 ms at one poll per round vs 111.3 ms at one
  per 64, same `inner_steps`). This is the tax for not having Madrona's megakernel, measured.
- **Temporal blocking pays until it doesn't.** 8 inner steps is the optimum on both grids;
  16 and 32 are worse, because a one-cell halo only carries useful information about one
  cell inward per step and the extra work is wasted.
- Polling too rarely also loses, by overshooting convergence (192 rounds instead of 80).
- Every row reads `match = yes`. See §5.

I set the crate defaults to `inner_steps = 8, rounds_per_poll = 8` on this evidence, with a
comment saying they are device-specific and must be retuned.

### 4e. Warm restart — and the batch-wide convergence flag is a trap

A flow field is not recomputed from nothing every frame. What matters is updating a
converged field after the world changed a little. Min-plus relaxation warm-starts trivially;
Dijkstra does not, because a converged field makes every cell a source.

**The soundness boundary, stated once and clearly:** warm relaxation is valid **only for
changes that can lower distances** — a goal added, an obstacle removed, terrain made
cheaper. The old field is then an upper bound and relaxation descends to the new fixed
point. For changes that *raise* distances — a wall built, a goal removed — the stale field
sits *below* the new answer and relaxation can never climb back to it. That case needs a
D*-Lite style raise phase or a full recompute. I measured only the sound direction.

`sweeps av/max` is per-field sweeps to converge, averaged and maximised over the batch.
[measured]

**Perturbation 1 — a second goal opened three quarters of the way across the map** (large
affected region):

| grid | fields | cold dial 12t | warm CPU vec 12t | sweeps av/max | warm GPU | GPU rounds | warm GPU vs cold dial |
|---|---|---|---|---|---|---|---|
| 64×64 | 256 | 4.8 | 13.8 | 64 / 104 | 11.4 | 24 | 0.42× |
| 64×64 | 4096 | 72.9 | 194.3 | 63 / 109 | **50.6** | 32 | **1.44×** |
| 128×128 | 1024 | 78.0 | 282.3 | 125 / 200 | **64.3** | 40 | **1.21×** |
| 256×256 | 256 | 74.2 | 427.6 | 251 / 377 | 101.5 | 64 | 0.73× |

**Perturbation 2 — a 4×4 patch of terrain made free next to the existing goal** (small
affected region):

| grid | fields | cold dial 12t | warm CPU vec 12t | sweeps av/max | warm GPU | GPU rounds | warm GPU vs cold dial |
|---|---|---|---|---|---|---|---|
| 64×64 | 256 | 4.7 | 7.5 | 35 / 125 | 10.2 | 32 | 0.46× |
| 64×64 | 4096 | 72.5 | 106.4 | 34 / 126 | **52.8** | 32 | **1.37×** |
| 128×128 | 1024 | 70.7 | 68.7 | 22 / 244 | 75.6 | 48 | 0.94× |
| 256×256 | 256 | 68.7 | 50.5 | **21 / 488** | 115.0 | 72 | 0.60× |

Warm-starting does move the GPU from 0.67× to 1.2–1.4× at the larger batch sizes, so the
architecture is right. But look at the last row's `21 / 488`. The average field converges in
21 sweeps; the worst needs 488. The CPU pays the **average**, because each field stops on
its own. My GPU pays the **maximum for every field in the batch**, because it has one
convergence flag for the whole dispatch — and it gets *worse* as the perturbation gets more
local, exactly backwards. That is a **23× self-inflicted wound**, and it is why the small
local change (which should be the easy case) is the GPU's worst result.

**This is the most actionable finding in the lane.** The fix is the Madrona lesson applied
one level down: track convergence **per field** (or per tile), keep an **active list**, and
compact it with the same radix-sort-on-world-id machinery Madrona uses for deleted entities.
Then the batch runs at the average rather than the tail. I did not implement it; the
prototype deliberately shows the cost of not having it.

---

## 5. Determinism

### 5a. Why floating point on a GPU is a determinism problem

A single IEEE-754 `f32` add is exactly rounded on any conformant device, so the naive fear
("GPU floats are fuzzy") is wrong. The real problem is **order**. A parallel reduction sums
in whatever order the scheduler picks; `f32` addition is not associative; so the same input
gives different bits on different runs, let alone different vendors or driver versions. For
a lockstep-faithful RTS that is fatal, and it is *unfixable by care* — it is a property of
the operation, not of the code.

Our binary is measured to be scalar SSE single precision with no FMA
(`docs/binary-ground-truth.md`), which is the best possible starting point for bit-exact
CPU work. A GPU reduction throws that away.

### 5b. What this kernel does instead

It sidesteps the problem rather than mitigating it, by choosing an operator that has no
order dependence to begin with. The kernel computes a least fixed point over `u32` where

- `min` on `u32` is associative, commutative **and idempotent**;
- integer `+` is associative and exact, and the operand ranges are bounded so nothing wraps
  (`MAX_COST = 0x00ff_ffff`, `INF = 0x7fff_ffff`, largest sum `< 2^32`, argued in the code);
- every step is monotone decreasing and bounded below by the true distance.

Three consequences, and they are **structural properties of the operator, not observations
from a test run**:

1. **The fixed point is unique.** Any schedule that relaxes until nothing changes lands on
   the same array of bits — Jacobi, Gauss-Seidel, Dijkstra, one thread or forty thousand.
2. **Stale and partial information is safe.** The stale halo can only slow convergence.
3. **Convergence detection does not break reproducibility.** Overshooting the fixed point is
   a no-op, so the round count is not part of the answer.

**Fidelity tier: this is `structural`, not Tier A.** It is a mathematical argument about the
operator we chose, in a kernel that implements no game mechanic. It is not an SMT proof over
a lifted binary semantics, and it makes no claim about Rise of Nations. I am naming it
separately precisely so it cannot be mistaken for a Tier-A claim about the game.

The tests make the argument checkable rather than merely stated
(`crates/don-gpu/tests/parity.rs`, `crates/don-gpu/src/cpu.rs`) [measured, all green]:

- three independent CPU implementations agree bit-for-bit (scalar Jacobi, vectorised Jacobi,
  bucket-queue Dijkstra);
- GPU agrees with CPU bit-for-bit on shapes including 17×13 and 33×65, which are not
  multiples of the 8×8 workgroup tile;
- the GPU result is **invariant under `inner_steps` ∈ {1,3,8,17} × `rounds_per_poll` ∈
  {1,5,32}** — i.e. under changes to the parallel schedule;
- the GPU gives the same bits on four consecutive runs;
- GPU flow directions match CPU flow directions, including the tie-break;
- CPU results are invariant under thread count.

The benchmark re-checks GPU-vs-CPU equality at every configuration it times, so a fast wrong
answer cannot slip through.

### 5c. Where this generalises, and where it stops

**Carries over** — any idempotent-semiring or exact-integer accumulation:

| system | operator | order-free? |
|---|---|---|
| flow fields, distance fields | `min` + integer `+` | yes |
| influence / threat maps built by maximum | `max` | yes |
| fog of war, explored mask | boolean `or` | yes |
| territory ownership by nearest-city | argmin with a fixed tie-break | yes, if the tie-break is total |
| any counter, any fixed-point accumulator | integer `+` | yes (`+` on `u32`/`i64` is associative) |

**Does not carry over** — and no amount of care fixes it:

| system | why |
|---|---|
| income summed over gatherers | `f32` `+` is not associative |
| damage accumulated onto a shared target from many attackers | same, plus the *order of application* is itself semantically load-bearing if there is a death threshold |
| any average, variance, or normalisation in `f32` | same |

**Recommendation, concretely.** Where a sum must be parallel and deterministic, accumulate
in **fixed-point integers** — pick a scale, convert once, sum with `atomicAdd` on `u32`/`u64`
(integer atomic add *is* associative, so the order genuinely does not matter), convert back
once at the end. Where the *semantics* depend on order — a death threshold, a resource cap —
the parallel reduction is wrong regardless of numerics, and that system stays on the CPU.

And the harder constraint from §1c: a mechanic in WGSL is outside the oracle's reach and
outside Kani's. Until that changes, **the GPU is for systems we are content to hold at Tier
C**, and for helper computations whose result the CPU can validate cheaply.

---

## 6. Recommendations

1. **Do not put a GPU flow-field solver in the trainer yet.** [measured] Cold, it loses to a
   12-thread bucket-queue Dijkstra below ~1000–2000 concurrent fields; warm-started it wins
   by 1.2–1.4× at 1024–4096 fields, but never once host round-trips are counted. Revisit
   when (a) the batch dimension is genuinely worlds × active goals ≳ 1000, and (b) the field
   never leaves the device. Note that the batch dimension for flow fields is worlds × active
   goals, not worlds — 256 worlds with 8 active rally points each is already 2048 fields, so
   this threshold is reachable, which is the reason to keep the prototype rather than
   discard it.
2. **Fix the convergence flag before anything else.** Per-field flags plus a compacted active
   list turn a 23× tail penalty into nothing (§4e). This is cheap and it is the single
   largest win available in the prototype.
3. **Vectorise the CPU first, always.** 9.7× on this kernel for a structural rewrite, no new
   dependency, no new determinism risk, and it stays inside the oracle's and Kani's reach.
4. **Pack the columns.** `cost` fits in `u8` (4 per `u32`) and `dist` in `u16` for any map we
   would plausibly run. At ~31% of quoted peak bandwidth there is headroom, and 4× less
   traffic is the obvious next kernel change.
5. **Never read back inside a tick.** Use `dispatch_workgroups_indirect` for device-computed
   sizes, and keep observations resident as tensors aliasing the storage buffers.
6. **Adopt Madrona's storage design, not its scheduling design.** The single table with an
   implicit world id, the sentinel deletion, the radix compaction and the zero-copy tensor
   export all port to wgpu. The megakernel does not, and pretending otherwise produces
   unsound code.
7. **Copy the layout into `don-sim` when the time comes** — `FieldBatch` is deliberately the
   same shape as `don-sim`'s SoA `World`, so a future `don-sim` ↔ `don-gpu` bridge is a
   buffer-mapping exercise, not a rewrite.
8. **Settle the map-size unit** (§2). It is a one-line derivation and it moves the GPU
   verdict by a factor of 16 in cell count.

---

## 7. What I could not establish

- **Whether any of this matches the engine.** `PathFinder` is a measured RTTI class name and
  nothing more. The engine's pathfinding algorithm, its cost model, its connectivity and its
  grid resolution are all underived. The chamfer weights, the 8-neighbour connectivity and
  the synthetic terrain are mine and are benchmark fixtures only.
- **The map-size unit** — 40–100 in `rules.xml`, but TCoords or WCoords is unresolved (§2).
- **Whether a discrete GPU changes the verdict.** Every number here is an M2 Max with
  unified memory and a 38-core integrated GPU. A 4090 has roughly an order of magnitude more
  compute and a PCIe bus instead of unified memory, which pushes the kernel comparison one
  way and the I/O comparison the other. I cannot measure it; hbox has no usable GPU for this
  and the 2400-series-era question does not arise there.
- **Vulkan parity.** The crate enables the Vulkan backend and the kernel uses nothing
  Metal-specific, but I only ran it on Metal. The parity tests are the check when someone
  runs it on Linux.
- **Whether wgpu inserts the barriers I rely on between dispatches inside one compute pass.**
  Metal's `transition_buffers` in wgpu-hal 27.0.4 is a **no-op** [measured, source read], so
  ordering there comes from Metal's default serial dispatch type, not from wgpu. On Vulkan
  the ping-ponged bind groups flip each buffer's usage between read-only and read-write
  storage every dispatch, which should force a transition — but I have not confirmed it on a
  Vulkan device. The parity tests would catch it; run them before trusting a Linux port.
- **Zero-copy tensor export.** Argued from the Madrona paper and from wgpu's API surface, not
  implemented or measured here.
- **Any claim about the other work classes.** §1a and §1b are a design argument. Only the
  flow field is measured.

---

## 8. Reproducing everything

```sh
cd /Users/ember/dev/don

# correctness: 13 tests, 5 of which are GPU parity and skip cleanly with no adapter
cargo test -p don-gpu
cargo test                       # whole workspace, green

# the benchmark (about 20 minutes for all four sections)
cargo build --release -p don-gpu
./target/release/flowbench              # all sections
./target/release/flowbench --quick      # a couple of minutes
./target/release/flowbench --a          # kernel for kernel
./target/release/flowbench --b          # vs bucket-queue Dijkstra, the crossover sweep
./target/release/flowbench --c          # inner_steps / rounds_per_poll sweep
./target/release/flowbench --d          # warm restart

# the auto-vectorisation check
otool -tv target/release/flowbench | grep -c 'umin\.4s'    # 40

# the map-size data
sed -n '1884,1905p' ron-data/rules.xml
```

Environment for every number above: Apple M2 Max, 12 CPU threads, 38 GPU cores, 96 GB
unified memory, macOS 25.6.0, rustc 1.98.0-nightly (91fe22da8 2026-06-21), wgpu 27.0.1 on
the Metal backend, `--release` (`opt-level = 3`, thin LTO, one codegen unit).

Every table above is transcribed verbatim from `flowbench` output; the raw output is
reproduced in the appendix so the transcription can be checked without re-running.

---

## Appendix — raw `flowbench` output

Sections A and D, tuned defaults (`inner_steps = 8`, `rounds_per_poll = 8`):

```
== A. same kernel (Jacobi relaxation to fixed point) ==
grid          fields   sweeps   cpu-1t ms   cpu-Nt ms  cpu-row ms     gpu ms    speedup
64x64             24      114       111.9        19.4         2.0        5.2       3.76
64x64             96      114       471.1        70.1         8.0        7.9       8.92
128x128           24      209       978.8       144.5        11.6        7.7      18.80
128x128           96      209      3776.4       820.0        44.0       19.0      43.12
256x256           24      450      7443.7      1069.1        64.2       26.1      40.96

== D. warm restart after a distance-lowering change ==

-- perturbation: a second goal opened three quarters of the way across the map --
grid        fields cold dial ms  warm row ms sweeps av/max warm gpu ms gpu rounds    gpu/cpu
64x64          256          4.8         13.8        64/104        11.4         24       1.21
64x64         4096         72.9        194.3        63/109        50.6         32       3.84
128x128       1024         78.0        282.3       125/200        64.3         40       4.39
256x256        256         74.2        427.6       251/377       101.5         64       4.21

-- perturbation: a 4x4 patch of terrain made free, next to the existing goal --
grid        fields cold dial ms  warm row ms sweeps av/max warm gpu ms gpu rounds    gpu/cpu
64x64          256          4.7          7.5        35/125        10.2         32       0.73
64x64         4096         72.5        106.4        34/126        52.8         32       2.02
128x128       1024         70.7         68.7        22/244        75.6         48       0.91
256x256        256         68.7         50.5        21/488       115.0         72       0.44
```

(the `gpu/cpu` column there is warm GPU against **warm CPU relaxation**, not against cold
Dijkstra; the report's tables recompute it against cold Dial, which is the comparison that
matters.)

Section B, tuned defaults:

```
== B. GPU relaxation vs CPU bucket-queue Dijkstra (the real baseline) ==
grid        fields    Mcells  dial-1t ms  dial-Nt ms     gpu ms  gpu+io ms   gpu Mc/s   speedup
64x64            1      0.00         0.1         0.1        3.9        6.5        1.0      0.02
64x64            8      0.03         0.8         0.3        5.2        7.8        6.3      0.06
64x64           64      0.26         5.9         1.2        5.2        8.1       50.8      0.24
64x64          256      1.05        24.3         4.5       10.2       14.3      103.3      0.44
64x64         1024      4.19        99.1        19.3       22.8       32.6      183.9      0.84
64x64         4096     16.78       403.0        77.4       53.2       88.8      315.1      1.45
128x128          1      0.02         0.4         0.4        6.3        8.9        2.6      0.06
128x128          8      0.13         3.1         0.8        7.8       10.6       16.8      0.10
128x128         64      1.05        23.1         4.9       10.3       14.4      102.1      0.47
128x128        256      4.19        94.9        20.0       30.5       39.4      137.5      0.66
128x128       1024     16.78       389.7        78.6       78.2      107.7      214.6      1.00
256x256          1      0.07         1.5         1.5       11.7       14.5        5.6      0.13
256x256          8      0.52        12.5         2.8       11.6       15.0       45.3      0.24
256x256         64      4.19        94.3        19.0       43.4       53.4       96.6      0.44
256x256        256     16.78       396.8        88.6      132.4      164.2      126.7      0.67
```

Section C, the knob sweep (abridged — the full 36 rows are what `--c` prints, and every one
reads `match = yes`):

```
-- 64x64 x 1024 fields --            -- 256x256 x 64 fields --
inner  poll  rounds polls      ms    inner  poll  rounds polls      ms
    1     1     136   136   182.4        1     1     490   490   726.6
    1    64     256     4    50.7        1    64     576     9   111.3
    2     8      80    10    27.9        2    64     384     6    82.7
    4     8      48     6    20.0        4    64     256     4    65.7
    8     8      32     4    15.1        8     8      80    10    38.3
   16     8      32     4    24.9       16     8      72     9    56.4
   32    64     128     2   144.3       32    64     128     2   182.3
```
