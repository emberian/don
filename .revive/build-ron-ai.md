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

YOUR LANE: ron-ai
WRITE YOUR REPORT TO: /Users/ember/dev/don/docs/tracks/ron-ai-impl.md

REPLICATE THE SHIPPED RISE OF NATIONS AI. It is our baseline opponent and the cheapest
bootstrap for RL (behaviour-clone, then fine-tune).

With full symbols this is now tractable. Use schema/symbols.json and rise.pdb to find the
BHS interpreter, the AI scheduler, and every AI class — search names containing Ai, Script,
Compiler, Personality, Strategy. docs/tooling/bhs-bridge.md established that Game::do_frame
calls run_script once per frame, that print and print_line are unconditional
OutputDebugStringW, that 226 engine script functions are missing from the shipped catalogue,
and that object_health returns a percent.

1. ANSWER THE DIVISION-OF-LABOUR QUESTION with symbols, not guesswork: how much AI is in the
   three shipped .bhs scripts (2400 lines) versus compiled C++? Count and name the compiled
   AI functions and their source files. If the scripts are a thin build-order layer over a
   compiled tactical AI, say so plainly and size both parts.
2. Recover the AI decision loop: what it reads per tick, difficulty scaling, and QUANTIFY any
   handicaps or resource bonuses. Many RTS AIs of this era cheat; find out and measure.
3. BUILD crates/don-ai into a real player: a faithful transcription of the economic opening
   from economic.bhs plus aibestbuildlibrary.bhs against our derived economy, driving the sim
   through the same order interface an RL agent will use. Not an improvement on it — a
   transcription, with every deviation marked.
4. Make it playable in crates/don-sim so we can run AI-vs-AI matches headlessly, and report
   what happens when you do.


--- RESUMING AN INTERRUPTED RUN ---
A previous run of this exact lane was cut short (You've hit your session limit · resets 5:30pm (America/New_York)) after 96 tool calls. Its work is
still on disk. Do NOT start over — verify what is listed below, then continue from there.

Files it already wrote or edited (10):
  /Users/ember/dev/don/crates/don-ai/src/rules.rs
  /Users/ember/dev/don/crates/don-ai/src/orders.rs
  /Users/ember/dev/don/crates/don-ai/src/game.rs
  /Users/ember/dev/don/crates/don-ai/src/api.rs
  /Users/ember/dev/don/crates/don-ai/src/probe.rs
  /Users/ember/dev/don/crates/don-ai/src/library.rs
  /Users/ember/dev/don/crates/don-ai/src/scheduler.rs
  /Users/ember/dev/don/crates/don-ai/src/bin/ai_match.rs
  /Users/ember/dev/don/crates/don-ai/Cargo.toml
  /Users/ember/dev/don/crates/don-ai/src/lib.rs

Commands it ran (47 distinct, first 15):
  $ ls -la /Users/ember/dev/don/ && ls -la /Users/ember/dev/don/crates/ &&
  $ find /Users/ember/dev/don/crates/don-ai -type f | head -50; echo "---C
  $ find /Users/ember/dev/don/ron-data -iname '*.bhs' 2>/dev/null | head -
  $ wc -l docs/tracks/ron-ai.md docs/tooling/bhs-bridge.md crates/don-ai/s
  $ cat docs/tracks/ron-ai.md
  $ find crates/don-sim -type f -name '*.rs' | xargs wc -l | tail -30; ech
  $ python3 -c "
  $ python3 - <<'EOF'
  $ ls /Users/ember/dev/don/re/decomp-all/ | head -3; ls /Users/ember/dev/
  $ cat /Users/ember/dev/don/re/decomp-all/006cfd00.c
  $ sed -n 1,70p crates/don-sim/src/lib.rs; echo "=====WORLD====="; sed -n
  $ grep -n "pub fn\|pub struct\|pub const\|RES_" crates/don-sim/src/mecha
  $ sed -n 860,1140p crates/don-sim/src/mechanics.rs
  $ cd /Users/ember/dev/don/ron-data && head -c 2000 buildingrules.xml; ec
  $ cd /Users/ember/dev/don/ron-data && grep -n -i "COST" buildingrules.xm

Its own last note, which usually says where it had got to:
Step 10 deadlocks on `city_placement`, whose trigger semantics were never established. Let me model it explicitly.

--- END RESUME CONTEXT ---