# Batch simulation core: occupancy, kernels, layout, scheduling

Lane: `simd`. This is **engineering, not derivation** — no mechanic was added, removed or
changed. Nothing here is a claim about Rise of Nations. The placeholder systems compute
exactly what they computed before, and the proof of that is at the top of the next section.

Everything below is `[measured]` on the two machines named. Nothing is `[reported]`.

Machines:

- **M2 Max** — this Mac, aarch64, 8 performance + 4 efficiency cores, `available_parallelism` = 12.
  **Shared with other lanes while measuring**; load average was 7-8 throughout. All figures are
  **medians of 4 interleaved runs** (baseline, after, baseline, after, …) so drift hits both sides.
- **hbox** — Intel i9-12900, x86-64, run under `nice -n 15 taskset -c 0-3`, co-tenant with codex's
  HOL build. Medians of 3. Cores 0-3 are two physical P-cores plus their SMT siblings, which is
  visible in the scaling row below.

---

## Headline

**The per-unit cost no longer depends on how full a world is.** Before this lane, 4096 worlds
holding 64 units each ran at 87.8 M unit-steps/s while 64 worlds holding 4096 ran at 1117.1 — a
**12.7x spread** caused entirely by stepping empty slots. After: 17723.2 and 18307.6, a **1.03x
spread**. Same measurement harness, same machine, same session.

The other structural finding is that at these speeds **the thread pool costs more than the frame
does**: an empty `std::thread::scope` with 12 workers costs 126.6 µs on the M2, and a 256-world
frame is now ~14 µs of actual work. Advancing a batch one frame at a time and joining is now the
dominant cost of doing that, which is why `Batch::run_parallel` exists.

---

## Semantics preserved — checked, not assumed

The dense rewrite changes where a unit lives, so the first thing built was a check that it does not
change what a unit *is*. A dump of all live unit state (`pos_x, pos_y, vel_x, vel_y, hits,
cooldown, owner`, sorted, so row order cannot hide a difference) was taken from the **pre-lane crate
at git 178ba5e**, over two scenarios:

- A: 64 spawns, 500 steps.
- B: 300 spawns, 50 steps, despawn every third by handle, 50 steps, 120 more spawns, 200 steps
  (ends at 320 live, frame 300) — i.e. churn with hole reuse, which is exactly what compaction changes.

Both dumps are byte-identical before and after, `sha256
4fe1ad65278428b876bdae0bca904fd2dee4c621e90bc7b15191085b699e9e3c`, re-verified after every
subsequent change to the storage layout.

The one deliberate behavioural change is the **digest**, and it is a debug utility, not state. It
was documented as order-independent and was not: it folded rows in index order. It now hashes each
unit *with its handle* and combines commutatively, so compaction is invisible to it while two units
swapping attributes is not. `world::tests::digest_is_independent_of_row_order` builds two worlds
that kill the same units in opposite orders — asserting first that the row orders really do differ,
so the test cannot pass vacuously — and requires equal digests before and after 50 steps.

---

## 1. The occupancy scan

`World` stored a fixed 4096 slots with an `alive: Vec<bool>` mask, and every system scanned all of
them. Live units now occupy rows `0..live` of every column with no holes, and identity moved from
the row number to a generational `Handle`:

- `spawn` appends at `live`.
- `despawn` swap-removes: one row copy, one handle repair, O(1), no allocation.
- `row_of(handle)` validates with two checks that are both load-bearing — the generation (catches
  ABA: an id freed and handed out again) and a row/id round trip, the id claims a row and that row
  must claim the id back (catches a plain dead id without needing a liveness flag).

The old `despawn(slot_index)` API had a real ABA hazard: despawn a slot, spawn into it, and a stale
index would kill the new unit silently. `world::tests::stale_handles_are_rejected_after_reuse`
asserts that is now rejected.

Compaction as such was **not** needed — no radix sort, no periodic pass. Keeping rows dense at the
mutation site is strictly cheaper than restoring density later, because the placeholder systems are
per-unit independent and do not care about ordering. If a later system needs units grouped (by
owner, by tile) that is when a sort earns its place, and then the Madrona-style periodic-sort
question becomes live again.

The handle tables cost memory, and the free list does not: **the pool of unused ids lives in the
tail of the row→handle array**, which is a permutation of `0..capacity` at all times, so `despawn`
is a swap inside that one permutation. `world::tests::handle_ids_remain_a_permutation_under_churn`
drives 400 randomized spawn/despawn rounds, checks the permutation property every round, and then
refills the world to capacity — a leak there would silently shrink the world, which is the failure
this storage trick could plausibly cause.

**Measured, M2 Max, median of 4, Ms/s unit-steps.** Both columns use `step_parallel` — one frame,
one join — so the comparison is like for like against the pre-lane code:

| configuration | before | after | ratio |
|---|---:|---:|---:|
| 1 world x 512 units, 1t | 111.1 | 3383.9 | **30.5x** |
| 256w x 256u, 1t | 59.5 | 1107.8 | **18.6x** |
| 256w x 256u, 12t | 220.6 | 470.9 | 2.1x |
| 4096w x 64u, 12t | 87.8 | 1283.0 | **14.6x** |
| 64w x 4096u, 12t | 1117.1 | 1755.8 | 1.6x |

The two small ratios are the interesting ones. `64w x 4096u` was already dense in effect — the
worlds were full — so it gains only from the branchless kernel. And the 12-thread row gains least
of all because with the work 15x cheaper, the per-frame thread spawn now dominates it. See §4.

## 2. Explicit SIMD — written, measured, and **not** shipped as the default

Three forms of each kernel exist in `crates/don-sim/src/simd.rs`, all asserted bit-identical:

| form | what it is |
|---|---|
| `*_scalar` | branchy reference; *defines* the semantics |
| `*_portable` | branchless, no `unsafe`, shaped so an autovectoriser can see it |
| `hand::*` | explicit NEON and SSE2 intrinsics, four vectors per iteration |

Bit-identity is currently structural rather than lucky: every column a tick touches is an integer,
and integer SIMD is the scalar computation done four or eight at a time — `vaddq_s32` is
`wrapping_add` per lane, comparisons produce exact all-ones masks. The wrap is written as
`x + (span & lt) - (span & ge)`, which equals the scalar `if/else if` chain exactly because the two
conditions are mutually exclusive for `span > 0`. That is why the scalar reference's `+` was
changed to `wrapping_add`: not a semantic change in the world's value range, but it makes the paths
equal on *every* input rather than only on in-range ones, so the tests can hammer `i32::MIN`.

Tests, per the lane's hard constraint:

- `simd::tests::integrate_wrap_paths_agree_bit_for_bit` / `tick_down_paths_agree_bit_for_bit` —
  every length 0..40, so every vector-tail shape, all three forms compared.
- `simd::tests::integrate_wrap_agrees_on_adversarial_inputs` — `i32::MIN`, `i32::MAX`, adds that
  wrap, positions outside the map, at spans 49152, 1, 2 and `i32::MAX`.
- `world::tests::every_kernel_path_steps_a_world_identically` — three identical worlds stepped 97
  frames through the three paths at 16 population sizes (0,1,3,4,5,7,8,9,15,16,17,31,33,64,257,1000),
  comparing columns and digest, with cooldowns seeded negative/zero/positive.

**The measurement went against the hand-written code, and that is the finding.** Comparing the two
vector forms through two different `World` entry points is worthless: on hbox, two entry points into
*bit-identical* SSE2 code measured 3302.3 and 3712.7 Ms/s purely from code layout — a 12% artifact,
larger than the effect being measured, and it briefly made the hand path look like the x86 winner.
The valid method is two builds that differ only in the line `integrate_wrap` dispatches on,
measured through the same entry point:

| machine | row | portable | hand |
|---|---|---:|---:|
| i9-12900, 1t | 1 world x 512u | **3722.5** | 3335.9 |
| i9-12900, 4t | 256w x 256u, run | **7494.2** | 6722.8 |
| i9-12900, 4t | 4096w x 64u, run | **7324.2** | 6421.5 |
| M2 Max, 1t | 1 world x 512u | **3395.9** | 3303.3 |
| M2 Max, 12t | 256w x 256u, run | 18044.9 | **18748.1** |
| M2 Max, 12t | 4096w x 64u, run | 18324.5 | **18750.6** |

x86-64 favours the portable form by 11-15%, consistently across rounds. aarch64 is a wash: single
thread favours portable by 2.8%, the contended 12-thread rows favour hand by 2-4%, and both are
inside the run-to-run spread of a shared laptop. **So the shipped dispatch is portable everywhere**,
and `don-bench` carries a two-pass row (forward and reverse order, because the harness itself has a
3-4% position bias) that re-runs the comparison on any new machine.

What did pay was making the kernel **branchless** and the region **dense**. Against the branchy
scalar reference: **1.47x on M2 Max** (3383.9 vs 2301.5) and **2.58x on hbox** (3738.9 vs 1450.1).
Unrolling mattered more than the intrinsics did — before the four-vector unroll the hand path was
13% behind portable on aarch64 and 4.8% behind on x86-64.

`hand::*` is kept, exported and tested on every target anyway. When a derived system integrates in
`f32` — and it will, the binary is SSE single-precision throughout — the autovectoriser becomes
untrustworthy, because it may only vectorise float code by reassociating it. The float discipline
is written down in the module docs: no FMA contraction, no reassociation, lane count must not change
the arithmetic tree. The scaffolding built here (kernel triples, adversarial bit-identity tests, a
benchmark row that compares them honestly) is the part that carries over.

There is no AVX2 path. Adding one would need runtime detection and I could only have measured it on
a machine I do not develop on; an unmeasured fast path is exactly the kind of plausible-looking
work this project forbids.

## 3. Memory layout

**Column-per-allocation already is the hot/cold split.** A tick touches `pos_x, pos_y, vel_x, vel_y,
cooldown` and pulls in no cache line belonging to `hits, armor, attack, recharge, owner`. Splitting
those into a second struct would have been documentation with an API cost and no layout effect, so
it was not done; the grouping is a comment and `hits` is marked as moving to the hot set the moment
a derived combat system lands.

**Provisioning was the real memory finding.** `World::new` reserved `MAX_UNITS` = 4096 rows
regardless of population. Peak RSS for 4096 worlds holding 64 units each, `/usr/bin/time -l`,
identical digests from both:

| provisioning | reserved | peak RSS |
|---|---:|---:|
| full capacity (pre-lane behaviour) | 656.0 MiB | **693,764,096 B** |
| right-sized (`with_capacity(64, …)`) | 10.2 MiB | **14,057,472 B** |

49x. The pages are genuinely resident, not merely reserved, so this is not a virtual-address
accounting artifact. `Batch::with_capacity` makes it available and `capacity_does_not_change_the_simulation`
asserts a right-sized world and a full one produce the same digest. Throughput barely moves (17723.2
vs 17916.0 for 4096x64) — this is a footprint result, and footprint is what decides how many
environments fit on a training box.

Alignment was **not** pursued and that is a deliberate negative: NEON and SSE2 unaligned loads carry
no penalty here, a 64-byte-aligned arena would mean hand-rolled allocation and `unsafe` in the
storage layer, and the measured bottleneck is elsewhere. It stays available if a future kernel wants
`vld1q_s32_x4`-style wide loads.

The dense rewrite does cost memory per unit of capacity: **41 bytes/unit vs 34 before**, because
`handle_of_row`, `row_of_handle` and `generation` (12 B/unit) replace `alive` and `free_slots`
(5 B/unit). That is the price of O(1) stable identity, it is paid per unit of *capacity*, and
right-sizing repays it many times over.

## 4. Scheduling — the largest single win

Measured cost of the thread pool itself on the M2 Max (2000 iterations each):

| workers | `std::thread::scope` spawn+join | `std::sync::Barrier` round trip | spin barrier round trip |
|---:|---:|---:|---:|
| 2 | 28.1 µs | 8.05 µs | 0.41 µs |
| 4 | 43.4 µs | 22.04 µs | 0.77 µs |
| 8 | 86.2 µs | 57.26 µs | 5.02 µs |
| 12 | 126.6 µs | 83.90 µs | 23.96 µs |

`Batch::step_parallel` pays the first column *every frame*. `Batch::run_parallel(frames, threads)`
spawns once for the whole rollout, hands workers groups of worlds from a shared cursor, and runs one
world through all its frames before moving on — world-major, so the world's columns stay in cache
across its frames, and uneven populations balance themselves. Worlds are independent, so any
assignment gives the same answer; that is asserted, not assumed.

**M2 Max, median of 4, Ms/s unit-steps:**

| configuration | pre-lane (step/frame) | after, step/frame | after, run | after, run + right-sized |
|---|---:|---:|---:|---:|
| 256w x 256u, 12t | 220.6 | 470.9 | 17641.9 | 17811.5 |
| 4096w x 64u, 12t | 87.8 | 1283.0 | 17723.2 | 17916.0 |
| 64w x 4096u, 12t | 1117.1 | 1755.8 | 18307.6 | 18005.2 |

**hbox i9-12900, 4 threads (`taskset -c 0-3`), median of 3, Ms/s unit-steps:**

| configuration | step/frame | run | run + right-sized | lane-major |
|---|---:|---:|---:|---:|
| 256w x 256u | 1195.4 | 7173.7 | 7233.2 | 7287.6 |
| 4096w x 64u | 1516.7 | 7211.4 | 7287.0 | 7369.3 |
| 64w x 4096u | 3434.8 | 7314.4 | 7228.0 | 7250.4 |

Thread scaling of `run`, 256w x 256u, Ms/s unit-steps:

| threads | 1 | 2 | 4 | 8 | 12 |
|---|---:|---:|---:|---:|---:|
| M2 Max | 3257.8 | 6395.4 | 12269.2 | 17695.3 | 18175.8 |
| hbox (4 cores allowed) | 3809.7 | 7369.9 | 7231.8 | 7222.8 | — |

M2 scales 1.96x / 3.77x / 5.43x / 5.58x. The flattening past 8 is the four efficiency cores plus, on
a 1.3 MiB hot set at ~18 G unit-steps/s, roughly 360 GB/s of L2 traffic — a bandwidth ceiling
hypothesis I did not separately confirm. hbox flattens after 2 threads because `taskset -c 0-3`
grants two physical P-cores and their SMT siblings, so threads 3 and 4 add no cores.

`step_parallel` was kept, unchanged in behaviour, because an interactive RL stepper genuinely needs
one-frame-at-a-time. What it should get eventually is a **persistent pool with a spin barrier** —
24 µs at 12 threads instead of 126.6, and better below 8 — but that needs raw pointers and hand-rolled
worker lifetime management, and `run_parallel` gets the same win with zero `unsafe` for any
rollout longer than one frame. Noted as future work rather than done half-way.

## 5. World interleaving (one SIMD lane = one world)

Implemented as `crates/don-sim/src/interleave.rs`: worlds in groups of 4, columns stored
`row * LANES + lane`, so one 128-bit load holds the same unit row of four different worlds. Rows past
a world's live count are zero-padded, and the tick systems map zero to zero, so padding lanes are
processed unmasked and provably contribute nothing — no mask register, no branch. `write_back` copies
only live rows. It is a *compiled* view of the authoritative dense worlds, not a replacement, and
`lane_major_stepping_matches_per_world_stepping` plus `ragged_populations_match_per_world_stepping`
assert bit-identical results against per-world stepping, including ragged populations where the
padding actually exists.

**Verdict: not adopted.** Ms/s unit-steps, medians:

| configuration | dense per-world | lane-major | delta |
|---|---:|---:|---:|
| M2 Max 12t, 4096w x 64u | 17916.0 | 20340.1 | **+13.5%** |
| M2 Max 12t, 256w x 256u | 17811.5 | 17543.2 | −1.5% |
| M2 Max 12t, 64w x 4096u | 18005.2 | 15567.8 | **−13.5%** |
| M2 Max 4t, 64w x 4096u | 12706.5 | 9944.1 | **−21.7%** |
| hbox 4t, 4096w x 64u | 7287.0 | 7369.3 | +1.1% |
| hbox 4t, 64w x 4096u | 7228.0 | 7250.4 | +0.3% |

For an element-wise system, lane-major and dense-per-world do *the same arithmetic in the same
order*; only memory traffic and loop overhead can move. The M2's large-world regression survived
being re-measured at 4 threads, where 64 worlds and 16 groups both divide evenly, so it is not the
scheduling coarsening that grouping four worlds per work item also causes. The leading explanation
(**hypothesis, not measured**) is working-set size: a 4096-unit world's hot columns are ~80 KiB and
fit the M2 P-core's 128 KiB L1, while a four-world group's are ~320 KiB and do not; hbox's 48 KiB L1
holds neither, which is consistent with the effect vanishing there.

The honest boundary: this experiment cannot speak to the case the pattern actually exists for.
Lane-major pays when worlds diverge in control flow or need per-world scalars broadcast into lanes,
and the placeholder systems are branch-free and world-agnostic. Revisit when real systems have
per-world branching, or if the batch becomes dominated by very small worlds, or on GPU. The module
stays in the tree with its equivalence tests so the question can be re-measured rather than re-argued.

---

## Determinism

The mandated test still passes unchanged: `batch::tests::parallel_matches_serial_regardless_of_thread_count`,
2/3/8/16 threads, 120 frames, digest equality. Added around it:

- `run_parallel_matches_serial_regardless_of_thread_count` — same, for the new rollout path.
- `run_and_repeated_step_agree` — frame-major and world-major traversal agree.
- `uneven_populations_still_match_serial` — the case dynamic work-pulling exists for.
- `capacity_does_not_change_the_simulation` — provisioning is not semantics.
- `every_kernel_path_steps_a_world_identically` — the SIMD bit-identity requirement, 16 sizes.
- `digest_is_independent_of_row_order`, `handle_ids_remain_a_permutation_under_churn`,
  `rows_stay_dense_under_churn`, `stale_handles_are_rejected_after_reuse`.
- Both `interleave` equivalence tests.

`don-bench` also cross-checks at runtime: serial, `step/frame`, `run` and `lane-major` are run over
the same start state and their digests printed and compared (`ALL AGREE` on both machines, 6/6 runs).

**27 tests pass in don-sim on both aarch64 and x86-64** — the x86 run is what exercises the SSE2
kernels at all, since the dev machine cannot.

---

## What got slower, or did not pay

1. **`step_parallel` at 12 threads is now slower than at 1 thread** on the M2 (470.9 vs 1107.8 Ms/s
   unit-steps) for 256x256. Not a regression in absolute terms — both are far above the pre-lane
   220.6 — but the *shape* changed: with the work 15x cheaper, a 126.6 µs join dominates a ~14 µs
   frame. Use `run_parallel` for anything longer than a single frame.
2. **Hand-written SIMD did not earn the default.** 11-15% behind portable on x86-64, a wash on
   aarch64. Kept and tested for the float future, not shipped.
3. **Lane-major interleaving is 13.5-21.7% slower for large worlds on the M2.** Not adopted.
4. **+7 bytes per unit of capacity** for generational handles (41 vs 34).
5. **The naive microbenchmark lied.** A standalone loop-over-arrays harness reported the branchy
   kernel as the *fastest* of the three at 8.4 G elem/s, because its branch pattern is perfectly
   predictable and the arrays sit in L1. Every number in this report comes from the real stepping
   loop instead. Related: `--emit asm` on the library showed the portable kernel compiling to zero
   vector instructions, while `otool -tV` on the shipped binary showed 86 — the library-level
   assembly dump was not what runs. Disassemble the artifact, not an intermediate.

---

## Files

Written by this lane, all under `/Users/ember/dev/don/`:

- `crates/don-sim/src/world.rs` — dense rows, generational handles, id-permutation free pool,
  column accessors trimmed to the live region, order-independent digest, three step entry points.
- `crates/don-sim/src/simd.rs` — new. Kernel triples, dispatch, NEON and SSE2 modules, tests.
- `crates/don-sim/src/interleave.rs` — new. `LaneBatch`, the lane-major experiment and its
  equivalence tests.
- `crates/don-sim/src/batch.rs` — `with_capacity`, `run_serial`, `run_parallel`, `live_units`, tests.
- `crates/don-sim/src/lib.rs` — module wiring and the layout summary.
- `crates/don-sim/src/bin/bench.rs` — time-budgeted harness, runtime cross-check, provisioning
  report, two-pass kernel comparison, thread scaling.
- `docs/derivation/simd-batch.md` — this file.

**`cargo test` at the repo root does not currently pass, for a reason outside this lane**:
`crates/don-gpu` (added to the workspace by another lane while this one ran) fails to compile —
`error[E0433]: cannot find type SolveStats in this scope`, in its `flowbench` bin test.
`cargo test -p don-sim -p don-rules -p don-pe` passes: **43 tests, 0 failures**, and don-sim's 27
also pass natively on x86-64.

## Reproduction

```sh
# tests, this machine
cd /Users/ember/dev/don && cargo test -p don-sim -p don-rules -p don-pe

# benchmark (medians over repeats; DON_BENCH_BUDGET sets seconds per row, default 0.4)
cargo build --release -p don-sim && ./target/release/don-bench
DON_BENCH_BUDGET=2 ./target/release/don-bench

# the pre-lane baseline, rebuilt from git and given the same harness
git show 178ba5e:crates/don-sim/src/world.rs   # etc; see the scratch crate recipe below

# x86-64 validation (the only way to exercise the SSE2 kernels)
scp crates/don-sim/src/*.rs hbox:~/don-sim-simd-lane/src/
ssh hbox 'cd ~/don-sim-simd-lane && nice -n 15 taskset -c 0-3 cargo test --release -q'
ssh hbox 'cd ~/don-sim-simd-lane && nice -n 15 taskset -c 0-3 ./target/release/don-bench'

# kernel A/B done correctly: two builds differing only in what integrate_wrap dispatches to,
# measured through the same entry point. ~/don-sim-portable on hbox is the forced-portable copy.
```

Baseline crate, state dumps, RSS harness and the raw benchmark rounds are in this session's
scratchpad at
`<local-recovery-scratchpad>/`
(`baseline-crate/`, `dumpbase/`, `dumpnew/`, `rss/`, `ship-base-*.txt`, `ship-after-*.txt`,
`hbox-*.txt`).

## Not established

- Whether a **persistent spin-barrier worker pool** actually delivers its 5x lower per-frame sync in
  the batch (only the barrier primitives were measured in isolation, not integrated).
- Whether the **L1 working-set explanation** for the lane-major large-world regression is correct;
  it fits both machines but no cache counters were read.
- Whether the M2's **flattening past 8 threads** is bandwidth or the efficiency cores. Both are
  plausible and they were not separated.
- **AVX2 / AVX-512**: not written, not measured.
- Whether any of this survives contact with **real derived systems**. Every number here is an upper
  bound for placeholder work whose cost is memory traffic; a real combat or pathfinding system
  changes the ratio of compute to traffic and could reorder every conclusion in this report,
  including the one about hand-written SIMD.
