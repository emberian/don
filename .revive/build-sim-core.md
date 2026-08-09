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

YOUR LANE: sim-core
WRITE YOUR REPORT TO: /Users/ember/dev/don/docs/tracks/sim-core.md

BUILD THE REAL SIMULATION CORE, replacing the placeholders.

crates/don-sim currently has a fixed-capacity SoA World with PLACEHOLDER systems that
compute nothing faithful. Replace them with the real architecture, now that we have it.

1. GENERATE Rust state structs from schema/pdb-types.json + schema/state-schema.json for
   the sim-critical classes: ObjectData, UnitData, BuildData, CityData, LeaderData,
   AmmoData, plus the type tables (ObjectTypeData, UnitTypeData, BuildTypeData,
   TechTypeData). Write a generator under re/scripts/ so it regenerates; do not hand-type
   1,316 fields. Keep the SoA layout for batch throughput but let the generated schema
   drive field names, widths and semantics. All integers unless the PDB says float.
2. IMPLEMENT the Game::do_frame ordered subsystem calls from docs/derivation/architecture.md
   as a real scheduler. Implement the Objects::process_all (frame + i) % 10 owner rotation —
   this is not optional, a fixed order diverges immediately.
3. IMPLEMENT the per-object chain: Unit::process -> Unit::work -> Unit::do_job (28-entry
   jump table on OrderIndex) -> Unit::do_move. Implement the order types you can support
   now; leave the rest as explicit unimplemented arms that RECORD which OrderIndex was hit,
   so we can measure coverage rather than guess it.
4. WIRE IN the derived mechanics that already exist: the damage pipeline in
   crates/don-sim/src/mechanics.rs, the rules constants in crates/don-rules, and the real
   balance matrix from schema/live/balance-real.bin (493x493 int16 — load it, do not embed).
5. Keep don-bench working and report before/after throughput.

Preserve the determinism test (parallel stepping must reproduce serial). This is the
project spine; prioritise a correct, complete tick skeleton with honest coverage accounting
over a small number of perfectly finished systems.


--- RESUMING AN INTERRUPTED RUN ---
A previous run of this exact lane was cut short (You've hit your session limit · resets 5:30pm (America/New_York)) after 58 tool calls. Its work is
still on disk. Do NOT start over — verify what is listed below, then continue from there.

Files it already wrote or edited (8):
  /Users/ember/dev/don/re/scripts/gen_sim_state.py
  /Users/ember/dev/don/crates/don-sim/src/generated/mod.rs
  /Users/ember/dev/don/crates/don-sim/src/rng.rs
  /Users/ember/dev/don/crates/don-sim/src/trig.rs
  /Users/ember/dev/don/crates/don-sim/src/balance.rs
  /Users/ember/dev/don/crates/don-sim/src/order.rs
  /Users/ember/dev/don/crates/don-sim/src/objects.rs
  /Users/ember/dev/don/crates/don-sim/src/schedule.rs

Commands it ran (23 distinct, first 15):
  $ ls && echo "---CRATES---" && ls crates/ && echo "---SCHEMA---" && ls -
  $ find crates/don-sim -name '*.rs' -o -name 'Cargo.toml' | head -50 && e
  $ ls docs/derivation/ && echo "---TRACKS---" && ls docs/tracks/
  $ python3 -c "
  $ sed -n '1,120p' crates/don-sim/src/mechanics.rs
  $ grep -n '^pub fn\|^pub struct\|^pub enum\|^pub const\|^    pub fn\|^im
  $ cat crates/don-sim/Cargo.toml && echo "=== BENCH ===" && sed -n '1,80p
  $ sed -n '140,300p' crates/don-sim/src/mechanics.rs
  $ sed -n '420,520p' crates/don-sim/src/mechanics.rs; echo "=== rules ===
  $ grep -n 'pub struct Rules\|pub fn \|pub const \|pub static' crates/don
  $ python3 - <<'EOF'
  $ ls re/decomp-all/ | head -3; echo ---; sed -n '1,140p' re/decomp-all/0
  $ sed -n '1,120p' crates/don-sim/src/interleave.rs; echo "=== simd pub =
  $ cd /Users/ember/dev/don/ron-bin && uv run --quiet --with pefile python
  $ cd /Users/ember/dev/don/ron-bin && uv run --quiet --with capstone --wi

--- END RESUME CONTEXT ---