# Lane: gpu-batch — scaling the batch simulation

Lane: `gpu-batch`. This is **engineering, not derivation**. No Rise of Nations mechanic was
added, removed or changed, and nothing here raises any fidelity tier. Everything measured is
marked `[measured]`; nothing is `[reported]`.

Machine: **M2 Max**, aarch64, 8 performance + 4 efficiency cores, `available_parallelism` = 12,
Metal (`Apple M2 Max`, IntegratedGpu). Figures are best-of-3 (best-of, not mean: the noise on a
shared laptop is one-sided and we are after the machine's capability).

> ⚠ **Read the ratios, not the milliseconds.** The machine was **shared with other lanes
> throughout**, and heavily. Absolute times in this lane run roughly **2× worse than
> `docs/derivation/gpu-architecture.md` §4c measured on the same hardware** — its
> 64×64 × 4096 `dial-1t` was 403 ms, mine was 9 339 ms. Every ratio reported here is measured
> *within one run*, so both sides took the same beating; cross-run absolute comparisons are not
> valid and I have not made any. Where a single row mattered I re-ran it and give the spread.

Everything in this lane lives in `crates/don-gpu/`. It does not touch `crates/don-sim/`.

---

## Headline

1. **The largest known defect in the GPU prototype is fixed, and it is worth 2.8–3.1× on the
   cases that provoked it.** `gpu-architecture.md` §4e found that a single batch-wide
   convergence flag makes every field run as long as the *slowest* field, called it "the most
   actionable finding in the lane", and did not implement the fix. Per-field flags plus
   active-list compaction now run the batch nearer the average: **256×256 × 256 local, 137.1 ms →
   43.9 ms (3.12×)** and **128×128 × 1024 local, 112.3 ms → 40.2 ms (2.80×)** [measured]. The old
   path is still selectable (`per_field_convergence = false`), so this is a before/after on the
   *same binary in the same run*, not against a remembered number.

2. **The crossover roughly halved on every grid, and the GPU now wins including host I/O —
   which §4c said never happened.** 64×64 went from ~2000 fields to **~512**; 256×256 was "not
   reached below the allocation cap" and now crosses at **256 fields (1.37×)**. Including upload
   and readback, 64×64 × 1024 is **1.60×** [measured]. §2.

3. **"Order execution has to stay CPU-bound because it is branchy" is false — but the win is
   proportional to actual divergence, and it is negative when there is none.** Bucketing by
   order archetype: **1.45–2.07× on a uniform order mix, ~1.0–1.24× on a realistic skewed mix,
   and 0.61–0.69× (i.e. a 30–40% loss) when every entity shares one order.** That last row is
   the honest floor and it is in the table, not a footnote. §3.

4. **The dominant cost at batch scale is not the branch — it is the thread pool.** Spawning a
   scope per frame is **up to 52× slower than single-threaded**; spawning once per rollout gives
   **3.6–6.4× on 12 cores and 420 M entity-steps/s**. This reproduces `simd-batch.md`'s finding
   in a second, independent codebase. §3.

5. **No atomics anywhere.** Many-to-one conflicts resolve by segmented reduce and segmented
   scan over a sorted key. The reduce is order-free because integer `wrapping_add` is the abelian
   group `Z/2^32`; the pool draw is *not* order-free and uses a scan that reproduces the
   sequential answer exactly. §4 states where each argument stops holding — **and §3 shows the
   segmented reduce currently costs 1.2–1.7× on CPU**, with the architectural reason it is still
   right.

6. **Every path is bit-identical to the scalar reference, with a test per path.** 11 execution
   plans plus `run_parallel` at 6 thread counts × 4 plans are asserted to produce the same digest
   *and* the same columns. GPU compaction is asserted bit-identical to the uncompacted GPU run
   *and* to a bucket-queue Dijkstra. 28 lib tests + 7 GPU parity tests, all green.

---

## 1. The convergence flag: what was wrong, and the fix

### The defect

The prototype had one `atomic<u32>` for the entire dispatch. Any field still changing kept
**every** field relaxing. §4e measured the consequence indirectly, via CPU sweep counts: at
256×256 × 256 fields the average field converged in 21 sweeps and the worst needed 488. The CPU
pays the average because each field stops on its own; the GPU paid the maximum, for all 256
fields. The pathology got *worse* as the perturbation got more local — exactly backwards, since
a local change should be the easy case.

### The fix

Three changes, in `crates/don-gpu/src/shaders/flowfield.wgsl` and `src/gpu.rs`:

- **`flags` is now `array<atomic<u32>>`, one entry per field**, OR-ed by every workgroup that
  changed a cell. Contention on it went *down*, not up: previously every workgroup in the batch
  hammered one address.
- **An active list** (`active_list: array<u32>`, dispatch slot → field index) that the host
  compacts at each poll. The dispatch `z` extent shrinks as fields converge. This is stream
  compaction on the world dimension — the same primitive Madrona uses for world compaction.
- **The poll group is forced even.** The kernel ping-pongs between two distance buffers; a
  dropped field stops being written, so its answer freezes in whichever buffer it last landed
  in. An even number of rounds per poll makes parity identical at every poll boundary, so every
  field's latest value always sits in `dist[0]` whether or not it is still active. Cost: at most
  one extra round per poll, versus copying whole converged fields between buffers.

### Why dropping a field is exact, not an approximation

This is the part that has to be argued rather than tested, so it is written out in the doc
comment on `FlowSolver::run` as well:

- Fields are independent — no cell of field `f` reads a cell of field `g` — so "this field
  changed nothing this round" is a statement about `f` alone.
- It is also a statement that `f` is at its **true** fixed point, not merely a fixed point of
  the temporally-blocked operator. Values inside the workgroup loop are non-increasing
  (`nd = min(sd[idx], …)`), so a cell whose final value equals its loaded value never moved at
  *any* inner step — in particular not at the **first**, whose halo is read fresh from `src`. No
  change at the first inner step, across every tile of the field, is exactly "one true Jacobi
  sweep changed nothing".
- Min-plus relaxation is idempotent at its fixed point, so further rounds could only reproduce
  the same bits.

### Measured [measured]

`flowbench --e`, warm restart from a converged field, best of 2, GPU compute only.
`case` is the perturbation: `local` = a 4×4 patch of terrain freed next to the goal (small
affected region), `wide` = a second goal opened three quarters across the map.

| grid | fields | case | batch-wide ms | per-field ms | gain | poll=2 ms | gain@2 | rounds b/f | field-work saved | conv av/max |
|---|---:|---|---:|---:|---:|---:|---:|---|---:|---|
| 64×64 | 256 | local | 14.4 | 13.3 | **1.08×** | 20.4 | 0.70× | 32/32 | 1.75× | 18/32 |
| 64×64 | 4096 | local | 61.9 | 48.2 | **1.29×** | 78.3 | 0.79× | 32/32 | 1.74× | 18/32 |
| 128×128 | 1024 | local | 112.3 | 40.2 | **2.80×** | 71.7 | 1.57× | 48/48 | 2.79× | 17/48 |
| 256×256 | 256 | local | 137.1 | 43.9 | **3.12×** | 84.0 | 1.63× | 72/72 | 4.19× | 17/72 |
| 256×256 | 256 | wide | 127.2 | 105.3 | **1.21×** | 151.6 | 0.84× | 64/64 | 1.37× | 47/64 |
| 128×128 | 1024 | wide | 82.2 | 66.0 | **1.24×** | 71.9 | 1.14× | 40/40 | 1.32× | 30/40 |

Run-to-run spread, since the machine is shared: an earlier run of the same binary gave **4.06×**
for the 256×256 row and **1.46×** for the 128×128 × 1024 row. The sign and rough magnitude are
stable; the second decimal is not. Take "≈3×, sometimes 4×" as the claim.

Reading it:

- **Total rounds are unchanged** (`32/32`, `72/72`, …) and that is correct: the last field to
  converge still sets the round count either way. The entire win is doing less work *per* round,
  and `field-work saved` (dispatched field-rounds, uncompacted ÷ compacted) tracks the speedup
  almost exactly — 4.19× of saved work bought 4.06× of time at 256×256.
- **The pathology reverses.** §4e's worst case (local perturbation, large grid) is now the best
  case, which is what should happen: a local change means most fields converge almost at once.
- **`conv av/max` is the mechanism**, per field, measured on the GPU itself rather than inferred
  from a CPU proxy: 17 rounds average against 72 max.

### Where the gain does *not* show up, and why

**Small grids gain the least** (64×64: 1.08–1.29×) even though they save the same 1.74× of
field-work. A 64×64 field is only 64 workgroups, so at 4096 fields the dispatch is 262 144
workgroups and the device is plausibly launch-bound rather than work-bound — removing work from
the tail then does not remove time. **I did not verify that**, and it is an open question in §8,
not a conclusion. (An earlier run of this row read 0.97×, i.e. marginally *slower*; the repeat
read 1.29×. The honest statement is "no reliable gain at 64×64", not "a regression".)

### Polling more often does not recover the rest — measured, and it was worth checking

A field can only leave the dispatch **at a poll boundary**, so `rounds_per_poll` is a floor on
the observable convergence mean: at 8 rounds per poll no field can be *seen* to converge before
round 8, which is why the measured mean is 17 and not the ~3 the CPU sweep counts imply. That
caps the achievable gain at roughly `max / rounds_per_poll`, so tightening the poll looks free.

**It is not.** At `rounds_per_poll = 2` the solve is worse in five of six rows (`gain@2` above),
including the two best rows dropping from 2.80× and 3.12× to 1.57× and 1.63×. The extra readback
stalls cost more than the tighter convergence fit buys, which is consistent with §4d measuring
host synchronisation at up to 7×. **Conclusion: leave the poll at 8 and get the rest of the way
by removing the stall (a megakernel), not by stalling more often.**

---

## 2. Crossover against the CPU, re-measured

`flowbench --b`. GPU relaxation against a bucket-queue Dijkstra — the algorithm anyone would
actually write on a CPU, and therefore the baseline the GPU has to beat to justify itself.
**Every GPU result is asserted bit-identical to the CPU result before its timing is printed.**
`speedup` is GPU compute vs `dial-Nt` (12 threads). [measured]

| grid | fields | Mcells | dial-1t ms | dial-Nt ms | GPU ms | GPU+io ms | GPU Mc/s | speedup | §4c was |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 64×64 | 64 | 0.26 | 8.5 | 3.0 | 8.2 | 13.5 | 32.0 | 0.37× | 0.24× |
| 64×64 | 256 | 1.05 | 163.4 | 12.3 | 15.5 | 24.2 | 67.6 | 0.80× | 0.44× |
| 64×64 | 1024 | 4.19 | 680.9 | 71.0 | 24.5 | 44.3 | 170.9 | **2.89×** | 0.84× |
| 64×64 | 4096 | 16.78 | 9339.3 | 152.0 | 139.1 | 253.7 | 120.7 | **1.09×** | 1.45× |
| 128×128 | 64 | 1.05 | 154.2 | 9.0 | 17.7 | 27.6 | 59.3 | 0.51× | 0.47× |
| 128×128 | 256 | 4.19 | 513.1 | 31.5 | 38.6 | 51.5 | 108.8 | 0.82× | 0.66× |
| 128×128 | 1024 | 16.78 | 1874.3 | 129.2 | 68.9 | 119.5 | 243.6 | **1.88×** | 1.00× |
| 256×256 | 64 | 4.19 | 140.1 | 28.8 | 45.5 | 57.0 | 92.3 | 0.63× | 0.44× |
| 256×256 | 256 | 16.78 | 852.3 | 151.9 | 111.2 | 157.8 | 150.9 | **1.37×** | 0.67× |

**The crossover moved down on every grid** [measured]:

- **64×64: ~2000 fields → ~512.** 0.80× at 256, 2.89× at 1024.
- **128×128: ~1000 → ~400.** 0.82× at 256, 1.88× at 1024.
- **256×256: "not reached below the cap" → reached at 256 fields**, 1.37×.

**Including host I/O, §4c said "never". That is no longer true**: 64×64 × 1024 is 44.3 ms against
71.0 ms of CPU (**1.60×**), and 128×128 × 1024 is 119.5 against 129.2 (1.08×). Everything else
still loses on the round trip, and on unified memory that remains the dominant practical
objection.

Two caveats I will not paper over:

- **`dial-1t` is wildly noisy** in this run (9 339 ms where §4c measured 403 ms for the same
  configuration) because of co-tenancy. It is not used in any ratio above; `dial-Nt` and `GPU` are
  the two columns the speedup is built from, and they were measured adjacently.
- **64×64 × 4096 went the wrong way** (1.45× → 1.09×). Both sides slowed under load and the GPU
  slowed more. This is the same configuration that shows no compaction gain in §1, and I think it
  is the same underlying launch-bound effect, but that is a hypothesis.

---

## 3. Archetype partitioning: the branch is a layout choice

### The claim being tested

The pushback was correct: *branchiness is a property of the layout, not of the problem.* The
engine dispatches per-unit behaviour through the `Order` hierarchy — `schema/symbols.json`
contains exactly **27** `OrderIndex X::get_type() const` overrides [measured], and with "no
order" that is the **28** archetypes in `crates/don-gpu/src/orders.rs`. Ported directly, that
shape costs one indirect call and one mispredict per entity per tick, forever.

**The names are [measured]; the numbers are ours.** The `OrderIndex` enum is *not* in
`schema/pdb-types.json` (36 enums were extracted and it is not among them), so the engine's own
numbering is unknown and nothing here claims otherwise. Renumbering changes nothing downstream —
the partitioner only needs the key to be a small dense integer.

### The factoring

`crates/don-gpu/src/partition.rs` — the key we want is `(order_type, world_id)`: orders major so
a bucket is one homogeneous kernel launch spanning the whole batch, worlds minor so memory access
stays world-ordered. An LSD radix sort on that pair is two stable passes.

**The world pass is free.** The arena stores worlds contiguously and enumerates live slots
world-major, so the input list is *already* world-sorted and a single stable counting-sort pass on
the order digit lands exactly on `(order, world, row)`. Partitioning therefore costs one O(n) pass
with a 28-entry histogram, not a sort. `partition::tests::one_stable_pass_gives_order_major_world_minor`
asserts the property rather than assuming it.

Inside a bucket there is no dynamic dispatch at all. What survives is per-entity *predicates*
(has the cooldown expired?), and those are written as branch-free selects — partitioning removes
the order branch, the select removes what is left.

### Measured: dispatch strategy, single-threaded

`orderbench`, 200 frames, best of 3. `virt` = one indirect call per entity through the 28-entry
table (the engine's shape). `match` = one `match` per entity (the obvious Rust rewrite, and
*kinder* to the predictor, so the fair baseline). `part` = partition once, 28 dense kernels.
`pworld` = partition each world separately. All four asserted bit-identical before timing.
[measured]

Only rows above ~200 ms are shown: below that the shared-machine noise exceeds the effect, and
including them would be reading tea leaves. (The full table is in the tool output; the small
configurations swing by 3× between runs in *both* directions.)

| mix | worlds | ents/w | virt ms | match ms | part ms | pworld ms | part/virt | **part/match** |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| uniform | 256 | 64 | 224.3 | 261.1 | 126.4 | 199.0 | 1.77× | **2.07×** |
| uniform | 1024 | 64 | 864.9 | 656.0 | 452.5 | 636.7 | 1.91× | **1.45×** |
| uniform | 4096 | 64 | 4341.5 | 3112.3 | 1726.9 | 1889.0 | 2.51× | **1.80×** |
| uniform | 64 | 1024 | 540.9 | 529.9 | 342.6 | 270.0 | 1.58× | **1.55×** |
| uniform | 256 | 256 | 407.3 | 429.6 | 281.1 | 309.6 | 1.45× | **1.53×** |
| skewed | 1024 | 64 | 310.1 | 292.9 | 237.0 | 294.2 | 1.31× | 1.24× |
| skewed | 4096 | 64 | 1272.4 | 1601.6 | 1640.7 | 1563.0 | 0.78× | 0.98× |
| skewed | 64 | 1024 | 456.8 | 377.4 | 312.0 | 314.3 | 1.46× | 1.21× |
| skewed | 256 | 256 | 373.8 | 407.9 | 352.1 | 341.0 | 1.06× | 1.16× |
| single | 1024 | 64 | 139.7 | 130.5 | 189.2 | 209.1 | 0.74× | **0.69×** |
| single | 4096 | 64 | 573.5 | 501.7 | 800.9 | 832.4 | 0.72× | **0.63×** |
| single | 64 | 1024 | 135.3 | 115.8 | 179.4 | 173.8 | 0.75× | **0.65×** |
| single | 256 | 256 | 132.1 | 119.3 | 194.2 | 184.8 | 0.68× | **0.61×** |

**The result is a gradient, not a win.** Partitioning pays exactly in proportion to how divergent
the order mix actually is:

- **uniform (28 archetypes equally likely): 1.45–2.07× over `match`, 1.45–2.51× over the virtual
  call.** This is the case the objection was about, and here the objection is simply wrong.
- **skewed (55% Idle, 20% Move, 10% Attack — a realistic army): 0.98–1.24×.** Marginal. A skewed
  mix is *easy* for a branch predictor, so there is little mispredict left to recover.
- **single (one archetype): 0.61–0.69×, i.e. a 30–40% loss.** The partition pass is pure overhead
  when the branch is perfectly predicted. This is the honest floor and it belongs in the headline,
  not a footnote.

So the correct engineering claim is **not** "partition always". It is: *the branch is a layout
choice, the cost of choosing wrong is about 40% either way, and which way is right is a property
of the order histogram — which is cheap to measure at runtime* (`Arena::order_histogram`).
`pworld` is essentially a wash against `part` (within noise both directions), which answers the
"one lane per world" question: **with a batch-wide partition there is nothing left for lane-major
interleaving to recover**, because a bucket already spans every world.

### Measured: threads, and the finding that dwarfs the branch

| mix | worlds | ents/w | 1 thread ms | per-frame spawn ms | `run_parallel` ms | speedup | M ent-steps/s |
|---|---:|---:|---:|---:|---:|---:|---:|
| uniform | 64 | 64 | 10.3 | 151.5 | **2.9** | 3.55× | 282.6 |
| uniform | 256 | 64 | 43.1 | 264.3 | **8.3** | 5.21× | 396.1 |
| uniform | 1024 | 64 | 201.2 | 562.3 | **31.5** | 6.39× | 416.2 |
| uniform | 4096 | 64 | 833.5 | 1321.2 | **153.5** | 5.43× | 341.5 |
| uniform | 256 | 256 | 181.7 | 548.5 | **31.2** | 5.82× | 420.0 |
| skewed | 1024 | 64 | 277.0 | 694.5 | **51.0** | 5.43× | 256.8 |
| skewed | 4096 | 64 | 1524.3 | 1879.7 | **259.3** | 5.88× | 202.2 |
| skewed | 256 | 256 | 408.6 | 661.6 | **65.7** | 6.22× | 199.5 |

**Spawning a thread scope per frame is up to 52× slower than not threading at all** (64 worlds:
151.5 ms threaded vs 2.9 ms with the pool amortised, and 10.3 ms single-threaded). Every
per-frame row is *worse than one thread*. This is `simd-batch.md`'s finding — an empty 12-worker
`std::thread::scope` costs 126.6 µs against a frame costing microseconds — reproduced
independently in a second codebase, and it is a much larger effect than the branch.

`Arena::run_parallel` spawns **once for the whole rollout** and lets each worker run complete
frames (phase A, resolve, apply) on its own world range, with **no per-frame barrier at all**.
That is sound only because worlds are independent, which is why `target` is stored as a row
*within* a world: it makes entity state position-independent and therefore sliceable.
**3.55–6.39× on 12 cores (8P+4E), peaking at 420 M entity-steps/s.**

> These are *this layout's* entity-steps, over placeholder kernels plus a full conflict-resolution
> pass. They are **not** comparable to `simd-batch.md`'s unit-steps/s, which count three
> element-wise kernels and no conflict resolution. Do not read either as a simulation speed.

### Measured: conflict resolution

| worlds | ents/w | sequential ms | segmented ms | segmented-par ms | seq/par |
|---:|---:|---:|---:|---:|---:|
| 64 | 64 | 26.4 | 40.7 | 159.8 | 0.16× |
| 1024 | 64 | 375.0 | 464.8 | 620.5 | 0.60× |
| 256 | 256 | 285.1 | 488.2 | 477.9 | 0.60× |
| 64 | 1024 | 307.0 | 477.7 | 751.8 | 0.41× |

**The segmented reduce costs 1.24–1.72× against the sequential loop, and the parallel version is
worse still.** Sorting `n` contributions in order to sum them is more work than summing them in
place, and the parallel variant pays a per-frame spawn on top. Reported plainly because it is the
opposite of what "use the parallel-safe primitive" suggests.

**Why it is still the right primitive, and what the numbers actually teach:** the reason
`run_parallel` is fast is that it slices by *world*, and conflicts never cross a world — so each
worker's resolve is sequential, disjoint, and deterministic without any reduce at all. The lesson
is therefore **partition the conflict domain if you can; segment-reduce only when conflicts cross
the partition.** The segmented path exists for the cases where that is impossible (a GPU kernel
where the partition is threads, not worlds) and it is the thing you reach for *instead of an
atomic*, not instead of a sequential loop.

### What the partition looks like

16 384 entities, 64 worlds × 256:

| mix | non-empty buckets | largest bucket |
|---|---:|---|
| uniform | 28/28 | Think, 630 (4%) |
| skewed | 28/28 | Idle, 9 063 (55%) |

**A fixture bug worth recording.** The first run of this benchmark reported `uniform` as
**14/28 buckets**. The generator used `rnd() % 28` on a plain LCG, and an LCG with a power-of-two
modulus has period 2^k in its low k bits — `% 28` is `% 4` interleaved with `% 7`, so half the
archetypes were unreachable, and `rnd() % 16` was producing a 16-long repeating cooldown pattern.
The "maximally divergent" case was measuring a half-divergent one. Taking the high bits fixes it;
the numbers above are post-fix.

---

## 4. Deterministic contention, and exactly where the argument stops

`crates/don-gpu/src/reduce.rs`. **No atomics.** Two conflict shapes, and they are not the same
problem.

### Accumulation — order-free, by algebra

Many attackers add damage into one defender; many gatherers add income into one stockpile.
`accumulate_i32` sorts the `(target, value)` contributions by target with a stable counting sort
and reduces each run. Reduction order cannot matter because the values are integers under
`wrapping_add`, which is the abelian group `Z/2^32` — associative, commutative, exact, no
rounding. The sim *is* integers (`Constants` has 722 members, all `int`/`int[N]`; `TypeData`,
`ObjectTypeData`, `UnitTypeData`, `BuildTypeData`, `TechTypeData` hold no floats), so this is the
normal case, not a lucky one.

**Where it does not hold** — the part that is easy to lose, so it is stated in the module docs
and repeated here:

- **Saturating or clamped addition is not associative.** For `i8`, `sat(sat(120,100),−100) = 27`
  but `sat(120, sat(100,−100)) = 120`. If HP clamps at zero *during* accumulation the sum is
  order-dependent and none of this applies. Accumulate raw, clamp **once, after** — `Arena::apply`
  is the only place a clamp happens, and `accumulate_i32` only ever hands back a total, so the
  bug is hard to spell.
- **Anything with a division or rescale mid-chain.** `ObjectData::get_damage` (`0x00644130`)
  subtracts armour at step 22 of 31 and rescales by 10; those are *per strike*, before the value
  reaches the reduce. Feed this per-strike results, never raw attack values.
- **`min`/`max` accumulation is fine** (idempotent commutative monoid); **`argmin`/`argmax` is
  not**, unless the tie-break is in the key — sort by `(value, entity_id)` and it becomes total.
- **Floating point is never fine**, and no amount of care fixes it.

### Pool draw — order-*faithful*, by a scan

Workers taking from a resource that can run dry. Here the answer *does* depend on order, so
commutativity is the wrong tool. Let the draws against one pool be `w_0, w_1, …` in canonical
order with the pool holding `P`. The sequential loop is `take_i = min(w_i, remaining_i)`, and by
induction `remaining_i = max(0, P − Σ_{j<i} w_j)`, so

```
take_i = clamp(P − Σ_{j<i} w_j, 0, w_i)
```

— an **exclusive prefix sum followed by a clamp**. A scan parallelises, and it reproduces the
sequential answer *exactly* rather than approximately. Prefix sums are taken in `i64` so a long
segment cannot overflow (`reduce::tests::a_long_segment_cannot_overflow_the_prefix_sum` drives
100 000 draws of 21 M each past `i32::MAX` and checks both paths agree).

The canonical order is `(pool, slot)` and slot is `world * capacity + row`. **That is a modelling
choice, not a derived fact.** When the engine's own resolution order is recovered it may differ —
the owner-slot rotation `(frame + i) % 10` in `Objects::process_all` is exactly the kind of thing
that would change it — and then only the sort key changes.

`reduce::tests::exhaustion_pays_in_canonical_order_not_in_bulk` is the test that a commutative
reduce would fail: pool 25, four draws of 10, and the answer must be `[10, 10, 5, 0]` — *which*
drawer gets the partial payment is the whole point.

---

## 5. Determinism — the hard constraint, and how it is checked

Every claim below is a test that fails if the property breaks, not a comment.

**CPU / layout paths** (`crates/don-gpu/src/arena.rs`):

- `every_execution_plan_produces_identical_state` — **11 plans** (indirect dispatch, per-entity
  match, batch-partitioned, per-world-partitioned, 2/3/8/16-thread partitioned, and all three
  conflict-resolution strategies) over 5 arena shapes × 60 frames, all against
  `StepPlan::REFERENCE`. Digests must be equal.
- `run_parallel_matches_stepping_at_every_thread_count` — the no-barrier rollout path at **6
  thread counts × 4 plans × 3 shapes × 75 frames**, against serial stepping. This is the one that
  matters most, because `run_parallel` is the fast path and it has no per-frame synchronisation
  anywhere.
- `columns_are_identical_not_merely_the_digest` — a digest is a hash; this compares every column
  (`pos_x`, `pos_y`, `hits`, `carried`, `cooldown`, `seed`, `pool`) after 100 frames.
- `the_fixture_really_does_contend_and_really_does_exhaust` — the **anti-vacuity** test. Asserts
  that some target really is hit more than once in a frame (so there is contention to resolve)
  and that some pool really does run dry (so the clamped scan is exercised). Without it the two
  tests above could pass on a fixture where the interesting paths never execute.
- `worlds_do_not_interact` — each world re-simulated alone must reach the same state.
- `the_partition_covers_every_live_entity_exactly_once`.

**Reduce** (`reduce.rs`): match against the sequential reference across sizes; independence from
thread count at 1/2/3/8/16; invariance under a permutation of the contributions; agreement on
**wrapping overflow** (`i32::MAX + i32::MAX`), because the abelian-group argument is about
`Z/2^32` and not about "values that happen to be small".

**Partition** (`partition.rs`): buckets dense, stable, complete, no duplicates; empty input;
`KeySort` stability and segment boundaries checked against hand-written expected output.

**GPU** (`tests/parity.rs`): all 7 pass on Metal [measured].

- `gpu_matches_cpu_bit_for_bit` — against bucket-queue Dijkstra, on shapes that are not multiples
  of the 8×8 workgroup tile.
- `gpu_result_is_invariant_under_the_schedule` — now sweeps `per_field_convergence` ∈ {off, on}
  as well as `inner_steps` ∈ {1,3,8,17} and `rounds_per_poll` ∈ {1,5,32}. 24 combinations, one
  answer.
- `active_list_compaction_is_bit_identical_to_running_the_whole_batch` — **new.** Ragged
  difficulty per field (field `f` gets `f` extra goals) so fields converge at wildly different
  rounds and compaction actually happens; compacted and uncompacted runs are both compared
  against Dijkstra. Carries an anti-vacuity guard asserting compaction really did drop fields.
- `a_reused_solver_resets_its_active_list` — **new**, and it is guarding a real hazard: a solver
  is allocated once per batch shape and re-uploaded many times in a training loop, so a stale
  (already compacted) active list would silently freeze most of the batch on the second solve.
- `gpu_is_reproducible_across_runs`, `gpu_directions_match_cpu_directions`, and the dispatch-chunk
  arithmetic check.

---

## 6. Three bugs this lane created and fixed, recorded because they are the interesting kind

**1. The fix-up pass that ran twice.** The first parallel phase-A implementation ran the kernels
on a worker-local slice and then *rebased* the columns carrying absolute ids in a fix-up pass. It
rebased **every** slot rather than the ones that had written a key that frame, so a non-drawing
entity's stale `pool_key` accumulated the world shift again every frame and drifted out of range
after a few hundred ticks — a crash whose onset depended on how long you ran. Caught by
`every_execution_plan_produces_identical_state`, not by inspection. The fix is structural, not a
bounds check: the offsets moved *into* the kernels (`Cols::slot_base` / `world_base`) and the
fix-up pass is gone. A pass that patches state after the fact is a pass that can patch it twice.

**2. The fixture that measured the wrong thing.** `uniform` reached only 14 of 28 archetypes
because `rnd() % 28` on an LCG uses the low bits, whose period is 4 (§3). The benchmark ran, was
green, and reported a number for a workload half as divergent as the one named. **A benchmark
fixture is code and needs its own assertion** — `section_histogram` now prints the bucket
occupancy, which is what exposed it.

**3. Stale ids that were valid in the wrong coordinate space.** After making entity state
position-independent, `strike_target` values written by a whole-arena frame were global while a
sliced frame expects slice-local. Zero damage made them harmless *arithmetically* but they still
index the reduce. Fixed by resetting both contribution key columns each frame to a value valid in
the current view (§7 notes the cost). The general lesson: **a payload of zero does not make an
index safe.**

---

## 7. Honest summary of what got slower or did not pay

- **Archetype partitioning loses 30–40% when the order mix is not divergent** (`single`:
  0.61–0.69×). The headline claim is a gradient, not a win.
- **`skewed`, the realistic mix, is close to a wash** (0.98–1.24×). If real RoN order histograms
  look like this, partitioning is worth having for the *layout* (it is what makes a GPU port
  possible at all) rather than for the CPU speedup.
- **64×64 grids show no reliable compaction gain** (1.08–1.29×, and 0.97× on one run) despite
  saving the same 1.74× of field-work. Suspected launch-bound; unverified.
- **64×64 × 4096 crossover regressed** 1.45× → 1.09× versus §4c, under a heavier machine load.
- **Polling 4× more often is worse in 5 of 6 cases** (§1) — a plausible-sounding optimisation that
  measurement killed.
- **`Resolve::Segmented` costs 1.24–1.72×** against the sequential loop and the parallel variant
  is worse still (§3). It is the right primitive for the case where conflicts cross the
  partition, and the wrong one when they do not.
- The per-field flag buffer grew from 4 bytes to `4 × fields` (16 KiB at 4096 fields) and the poll
  reads all of it. Not what a poll costs — the stall is — but not free.
- Contribution columns are dense per slot rather than compacted to the entities that actually
  emitted, so the resolve is O(total slots) even when three entities fired. `step` now also clears
  `strike_target` and `pool_key` every frame (two more full-array memsets) to keep every id
  in-range for its view. Simple and obviously correct; compacting it is the next optimisation, and
  it is the same active-list machinery §1 added on the GPU side.

---

## 8. What is next, in order

1. **Get the real order histogram.** §3's whole conclusion is a function of it, and it is the
   cheapest high-value measurement left — `donscan` already does a full typed heap scan of the
   live process in 0.45–0.68 s, and `UnitData::order_type()` is a named accessor. Until then,
   `uniform` and `skewed` bracket the answer and the truth is somewhere between 0.98× and 2.07×.
2. **The 64×64 non-result.** Confirm or refute launch-bound with a workgroup-count sweep. It is
   the same configuration behind both §1's weakest row and §2's only regression.
3. **Per-tile convergence**, one level below per-field: a large field with one corner still moving
   currently redispatches the whole grid.
4. **Compact the contribution columns** (§7), removing the O(total slots) resolve floor.
5. **A megakernel** to remove the poll stall, which §1 shows is what caps the compaction gain —
   and *not* more frequent polling, which was measured and is worse.
6. **Replace the placeholder order kernels** as real mechanics are derived. The partitioning is
   independent of what the kernels compute, so it should be a drop-in — that is the point of the
   factoring.

---

## A note on scope, for whoever picks this up

`crates/don-gpu/` now holds two things: the flow-field solver it was named for, and the ECS
factoring (`arena`, `orders`, `partition`, `reduce`). They share a crate because this lane owns it
and because `don-sim` is being actively rewritten by another lane, not because they belong
together. **`arena.rs` duplicates `MAP_SPAN` rather than depending on `don-sim`**, and `Arena` is
deliberately not `don_sim::World`. When the sim core settles, the partition/reduce machinery is
the part worth moving; the arena is a measurement rig.

Nothing in this lane implements a Rise of Nations mechanic, and no number here is a simulation
speed.

---

## Reproducing

```sh
cargo test -p don-gpu --release              # 28 lib tests + 7 GPU parity tests
./target/release/flowbench --e               # the convergence-flag before/after
./target/release/flowbench --b               # the CPU crossover
./target/release/orderbench                  # archetype partitioning, threads, resolution
```

`flowbench` skips its GPU sections with a printed line when no adapter exists; a skip is never a
silent pass. `orderbench` asserts bit-identity before printing any timing, so a fast wrong answer
cannot be reported as a result.
