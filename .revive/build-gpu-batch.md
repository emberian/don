PROJECT: Descent of Nations — /Users/ember/dev/don. Absolute paths always.

READ /Users/ember/dev/don/README-LLM.md FIRST (agent orientation; recently corrected).

**THIS WAVE IS FOR BUILDING, NOT RE-DERIVING.** A full day of reverse engineering is done
and its output is machine-readable. Do NOT re-derive what is already in schema/. Do NOT
spend your budget re-auditing prior claims. Take the biggest step your lane allows and
SHIP WORKING CODE. If you find an error in an existing artifact, note it in two sentences
and keep building.

GROUND TRUTH NOW AVAILABLE (all [measured], use freely, do not re-derive):
  ron-bin/sbl/rise.pdb — the game own full PDB, GUID-verified. Plus rise_z.map (13.5 MB
    linker map) and 8 other PDBs in ron-bin/sbl/ and ron-bin/pdb/.
  schema/symbols.json — 22,750 functions: VA, name, mangled, signature, size, obj,
    source file + line (98.9% coverage). 20,811 named globals with types.
  schema/types.json + schema/pdb-types.json — 19,914 classes, 26,469 fields with
    offsets/types/sizes, 21,997 virtual methods with vtable slots, enums.
  schema/state-schema.json — 278 walk_data impls, 1,316 named fields, 38,275 bytes of
    sim-critical state, ordered ops per class.
  schema/command-wire.json — 82 command opcodes with exact layouts and sizes.
  schema/live/balance-real.bin — the REAL 493x493 int16 damage matrix from
    combat_table+4 = 0x00C12BF4 (367 distinct values, range 5..2574, zero negatives).
  docs/derivation/architecture.md — Game::do_frame 29 ordered subsystem calls, the
    per-object chain, the source tree map.
  docs/derivation/*.md, docs/provenance-ledger.md, docs/tracks/*.md, docs/tooling/*.md.
  crates/: don-sim (SoA World + Batch), don-rules, don-pe, don-gpu, don-net, oracle, donscan.
  tools/oracle-regress.sh — re-runs all 12 differential cases against retail on hbox.

LOAD-BEARING FACTS, several recently corrected:
  The sim is INTEGERS. Constants has 722 members, all int/int[N]; TypeData, ObjectTypeData,
    UnitTypeData, BuildTypeData, TechTypeData have zero floats; Coord/WCoord/TCoord each
    wrap one int. Floats live in presentation and worldgen. EXCEPTIONS inside walked state:
    LeaderData::anti_att, LeaderData::plunder_scale, and Unit::move_step / find_path use
    sin_table, cosx, find_angle.
  The tick is 67 ms at Normal (TurnControl::timings 0x00AFC4A4 = {200,125,67,50,1} ms),
    NOT 15 Hz. The 15 is a frames-to-seconds conversion elsewhere.
  Objects::process_all ROTATES owner-slot order every frame: (frame + i) % 10. A
    fixed-order scheduler diverges within one tick.
  The real rules loader is Constants::init 0x00569A90 (the 112 "loaders" are log_data).
  Damage is ObjectData::get_damage 0x00644130, pure integer, armor subtracted mid-chain,
    attack stored x10, floor of 1 conditional.
  Class-name suffix is the sim/presentation cut: XData = POD state, X = behaviour,
    XOut = presentation, XType = static rules.
  Heap class counts are POOL CAPACITIES not live counts (Unit=600, Build=601, City=160).
  Live process: donscan does a full typed heap scan in 0.45-0.68 s.

ENVIRONMENT: this Mac is arm64 (no x86-32 execution). hbox (ssh hbox) is x86_64 Linux and
CO-TENANT — always nice -n 15 taskset -c 0-3, never install packages. The Parallels VM
"Windows 11" runs the live game; prlctl exec runs as SYSTEM so user paths must be explicit
(C:\Users\ember\...). Transfer via the host-only HTTP route (guest 10.211.55.6, host
10.211.55.2, guest curl.exe) or the shared folder \\Mac\deos -> /Users/ember/dev/breadstuffs;
both beat certutil. cargo-xwin + lld-link cross-builds Windows binaries from this Mac.

HOUSE RULES: never git stash; do NOT git commit or git add; never git add -A; write ONLY
files your lane owns. cargo test --workspace --exclude don-net --exclude don-ai must pass
if you touch shared crates.

DELIVERABLE: working code plus a short report at your lane path. Lead with what now WORKS
and how it was measured. Honest numbers over flattering ones.

YOUR LANE: gpu-batch
WRITE YOUR REPORT TO: /Users/ember/dev/don/docs/tracks/gpu-batch.md

SCALE THE BATCH SIMULATION: SIMD, GPU, and the factoring that makes it possible.

Prior work: docs/derivation/gpu-architecture.md (wgpu flow-field prototype, about 6x faster
than NEON on the kernel but losing overall below roughly 1000-2000 concurrent fields, with a
batch-wide convergence flag costing up to 23x — the largest fixable defect it found) and
docs/derivation/simd-batch.md (occupancy scan fixed; thread-pool overhead now dominates a
14 microsecond frame).

The user pushed back correctly on an earlier claim that order execution must stay CPU-bound
because it is branchy: branchiness is a property of LAYOUT, not of the problem. Build the ECS
factoring that removes it.

1. ARCHETYPE PARTITIONING: bucket entities by active OrderIndex (the engine own 28-entry jump
   table) and run one dense homogeneous kernel per bucket. The branch becomes a partition
   computed once per tick via radix sort or stream compaction on (order_type, world_id) — the
   same primitive Madrona uses for world compaction.
2. DETERMINISTIC CONTENTION: never use atomics for accumulation. Resolve many-to-one conflicts
   (damage into shared HP, workers drawing on a pool) by segmented reduce over a sorted key,
   which is deterministic by construction. The sim is integers, so integer addition is a
   commutative monoid and reduction ORDER does not affect the result — exploit that, and say
   clearly where it does not hold.
3. Fix the per-field convergence tracking in crates/don-gpu and re-measure the crossover.
4. Interleave worlds so one SIMD lane equals one world where it wins; measure, do not assume.
5. Report throughput honestly against entity count AND world count, including anything that
   got slower.

HARD CONSTRAINT: the determinism test (parallel stepping reproduces serial, and any SIMD or
GPU path is bit-identical to scalar) must pass. Add tests asserting it for every new path.


--- RESUMING AN INTERRUPTED RUN ---
A previous run of this exact lane was cut short (You've hit your session limit · resets 5:30pm (America/New_York)) after 58 tool calls. Its work is
still on disk. Do NOT start over — verify what is listed below, then continue from there.

Files it already wrote or edited (8):
  /Users/ember/dev/don/crates/don-gpu/src/shaders/flowfield.wgsl
  /Users/ember/dev/don/crates/don-gpu/src/gpu.rs
  /Users/ember/dev/don/crates/don-gpu/src/bin/flowbench.rs
  /Users/ember/dev/don/crates/don-gpu/tests/parity.rs
  /Users/ember/dev/don/crates/don-sim/src/orders.rs
  /Users/ember/dev/don/crates/don-sim/src/partition.rs
  /Users/ember/dev/don/crates/don-sim/src/reduce.rs
  /Users/ember/dev/don/crates/don-sim/src/arena.rs

Commands it ran (16 distinct, first 15):
  $ ls -la /Users/ember/dev/don/ && ls -R /Users/ember/dev/don/crates/don-
  $ wc -l /Users/ember/dev/don/docs/derivation/gpu-architecture.md /Users/
  $ cat /Users/ember/dev/don/crates/don-gpu/Cargo.toml /Users/ember/dev/do
  $ grep -n "23x\|convergence\|per-field\|crossover" docs/derivation/gpu-a
  $ sed -n '1,120p' docs/derivation/simd-batch.md
  $ sed -n '280,420p' docs/derivation/gpu-architecture.md
  $ grep -o '"OrderIndex[^"]*"' schema/*.json 2>/dev/null | sort -u | head
  $ sed -n '1,80p' crates/don-gpu/src/bin/flowbench.rs
  $ python3 -c "
  $ sed -n '80,439p' /Users/ember/dev/don/crates/don-gpu/src/bin/flowbench
  $ grep -n "field_offset\|Params" /Users/ember/dev/don/crates/don-gpu/src
  $ cargo build -p don-gpu --release 2>&1 | tail -30
  $ cargo build -p don-gpu --release 2>&1 | tail -20
  $ cargo test -p don-gpu --release 2>&1 | tail -40
  $ python3 - <<'EOF'

Its own last note, which usually says where it had got to:
Now the arena — the flat multi-world entity store with three phase-A implementations.

--- END RESUME CONTEXT ---