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

YOUR LANE: rl-env
WRITE YOUR REPORT TO: /Users/ember/dev/don/docs/tracks/rl-env.md

BUILD THE RL ENVIRONMENT SURFACE. This is the project actual goal and nothing exists yet.

1. ACTION SPACE, derived not invented: the engine enumerates its own action space twice over
   — 82 command opcodes in schema/command-wire.json and 32 Order classes rooted at UnitOrder.
   Design a factored MultiDiscrete action space over them. Do NOT flatten; microRTS ablations
   show full parameter-level masking is the difference between a 0.82 win rate and 0.00, so
   masking is load-bearing, not an optimisation. Provide exact per-parameter masks from state.
2. OBSERVATION SPACE: spatial feature planes plus an entity list, drawn from the SoA columns
   with ZERO COPY. schema/state-schema.json says what is sim-critical; use it to decide what
   is observable rather than guessing. Support partial observability (fog) as a flag even if
   it is not enforced yet.
3. PyO3 bindings exposing a vectorised env: Gymnasium VectorEnv semantics plus PettingZoo
   parallel for multi-agent. Batched step over the crates/don-sim Batch. Observations as numpy
   arrays viewing Rust memory directly, no serialisation.
4. Reward: expose the engine own score (the defeat screen shows one) plus win/loss, and make
   reward shaping pluggable rather than baked in.
5. SHIP A WORKING SMOKE TEST: a Python script that creates N envs, steps them with random
   masked actions, and reports steps per second. Report the real number.

Design so it does not foreclose the endgame: full-game action space, self-play, league
training. Put Rust under crates/don-env/ and Python under python/.


--- RESUMING AN INTERRUPTED RUN ---
A previous run of this exact lane was cut short (You've hit your session limit · resets 5:30pm (America/New_York)) after 71 tool calls. Its work is
still on disk. Do NOT start over — verify what is listed below, then continue from there.

Files it already wrote or edited (12):
  /Users/ember/dev/don/crates/don-env/gen/gen_spec.py
  /Users/ember/dev/don/crates/don-env/Cargo.toml
  /Users/ember/dev/don/crates/don-env/src/typecaps.rs
  /Users/ember/dev/don/crates/don-env/src/spec.rs
  /Users/ember/dev/don/crates/don-env/src/state.rs
  /Users/ember/dev/don/crates/don-env/src/action.rs
  /Users/ember/dev/don/crates/don-env/src/mask.rs
  /Users/ember/dev/don/crates/don-env/src/obs.rs
  /Users/ember/dev/don/crates/don-env/src/reward.rs
  /Users/ember/dev/don/crates/don-env/src/env.rs
  /Users/ember/dev/don/crates/don-env/src/lib.rs
  /Users/ember/dev/don/crates/don-env/src/py.rs

Commands it ran (23 distinct, first 15):
  $ ls -la /Users/ember/dev/don/ && echo "--- crates ---" && ls -la /Users
  $ cat Cargo.toml && echo "=== don-sim ===" && find crates/don-sim -type 
  $ python3 -c "
  $ python3 - <<'EOF'
  $ python3 tools/pdb/lookup.py --name 'Order::' 2>/dev/null | head -60; e
  $ cat ~/.cargo/config.toml 2>/dev/null; echo "---"; ls ~/.cargo/registry
  $ ls /Users/ember/dev/don/ron-data/ | head -60; echo "---"; ls /Users/em
  $ cd /Users/ember/dev/don/ron-data && wc -c unitrules.xml buildingrules.
  $ cd /Users/ember/dev/don/ron-data && python3 - <<'EOF'
  $ cat .gitignore && python3 - <<'EOF'
  $ mkdir -p /Users/ember/dev/don/crates/don-env/src /Users/ember/dev/don/
  $ python3 crates/don-env/gen/gen_spec.py 2>&1 | tail -20
  $ python3 crates/don-env/gen/gen_spec.py && head -60 crates/don-env/src/
  $ grep -n 'pub struct DamageInput' -A 40 crates/don-sim/src/mechanics.rs
  $ grep -n 'impl Default for\|pub struct DamagePredicates\|pub struct Unr

--- END RESUME CONTEXT ---