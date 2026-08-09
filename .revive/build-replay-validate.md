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

YOUR LANE: replay-validate
WRITE YOUR REPORT TO: /Users/ember/dev/don/docs/tracks/replay-validation.md

BUILD THE REPLAY-DRIVEN VALIDATION HARNESS. This is the highest-leverage lane in the wave:
it turns every other lane work into something measurable.

We can decode real command streams byte-exactly (crates/don-net round-trips 1,296,192 of
1,296,194 packages across 61 recordings), and multiplayer replays carry a 16-channel
CheckSumsCommand (opcode 0x39, 65 bytes) EVERY TURN, per player. So:

BUILD a harness that feeds a real .rcx command stream into crates/don-sim, steps the sim
turn by turn, computes the same 16 channels, and compares against the recorded checksums —
reporting the FIRST DIVERGING TURN and WHICH CHANNEL diverged.

1. Implement the checksum channels over our state, mirroring CheckSums::check_all
   (0x00936560) and the DataWalk byte ranges in schema/state-schema.json. adler-32
   (primitive at 0x00a46830) is already differentially tested in the oracle regression suite.
2. Map decoded commands onto our sim order application. Use schema/command-wire.json.
3. Report a DIVERGENCE PROFILE: turn number, channel, and how far we got. Early failure is
   expected and fine — the number that matters is "we survive N turns on channel X", and
   that number becomes the project headline metric from here on.
4. Make it a repeatable command (a binary or script) plus a summary JSON under schema/, so
   progress is trackable run over run.

Note real retail desync exists: 21 disagreements in 265,931 cross-player comparisons, and
only mutable-sim channels ever differ. Do not treat a late-game mismatch as automatically
our bug.

Do NOT re-derive the replay format or the checksum layout; both are done. Build the loop.


--- RESUMING AN INTERRUPTED RUN ---
A previous run of this exact lane was cut short (You've hit your session limit · resets 5:30pm (America/New_York)) after 63 tool calls. Its work is
still on disk. Do NOT start over — verify what is listed below, then continue from there.

Files it already wrote or edited (13):
  /Users/ember/dev/don/crates/don-replay/gen/gen_wire.py
  /Users/ember/dev/don/crates/don-replay/Cargo.toml
  /Users/ember/dev/don/crates/don-replay/src/checksum.rs
  /Users/ember/dev/don/crates/don-replay/gen/gen_walk.py
  /Users/ember/dev/don/crates/don-replay/src/walk.rs
  /Users/ember/dev/don/crates/don-replay/src/state.rs
  /Users/ember/dev/don/crates/don-replay/src/wire.rs
  /Users/ember/dev/don/crates/don-replay/src/replay.rs
  /Users/ember/dev/don/crates/don-replay/src/harness.rs
  /Users/ember/dev/don/crates/don-replay/src/lib.rs
  /Users/ember/dev/don/crates/don-replay/src/report.rs
  /Users/ember/dev/don/crates/don-replay/src/bin/don-replay.rs
  /Users/ember/dev/don/Cargo.toml

Commands it ran (25 distinct, first 15):
  $ ls -la /Users/ember/dev/don/ && echo "---CRATES---" && ls /Users/ember
  $ find crates/don-net crates/don-sim -type f -name '*.rs' | head -50 && 
  $ python3 -c "
  $ wc -l crates/don-net/src/*.rs crates/don-net/tests/*.rs crates/don-sim
  $ cat crates/don-net/src/lib.rs
  $ cat /Users/ember/dev/don/crates/don-net/src/stream.rs; echo "=== OBF =
  $ ls -la /Users/ember/dev/don/ron-data/ | head -60; echo "---REPLAYS---"
  $ cat /Users/ember/dev/don/crates/don-sim/src/lib.rs; echo "=== WORLD (h
  $ grep -rn "265,931\|1,296,192\|265931\|1296192\|21 disagree" docs/ crat
  $ sed -n '1,120p' crates/don-net/tests/roundtrip.rs
  $ sed -n '120,330p' /Users/ember/dev/don/crates/don-net/tests/roundtrip.
  $ mkdir -p /Users/ember/dev/don/crates/don-replay/src/bin /Users/ember/d
  $ python3 crates/don-replay/gen/gen_wire.py && head -25 crates/don-repla
  $ python3 crates/don-replay/gen/gen_walk.py && wc -l crates/don-replay/s
  $ cargo build -p don-replay 2>&1 | tail -60

--- END RESUME CONTEXT ---