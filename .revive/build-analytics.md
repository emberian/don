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

YOUR LANE: analytics
WRITE YOUR REPORT TO: /Users/ember/dev/don/docs/tracks/analytics-v2.md

THE ANALYTIC TRACK: optimal build orders, and the first heuristic player built on the answer.

The economy is derived and live-validated (719/721 constants confirmed against the PDB
Constants layout, 686 exact offset/parser/scale agreements against the real loader
Constants::init, scale set 192/256/100 proven exhaustive). Prior work may exist at
docs/tracks/build-order-analytics.md — read it, keep what holds, GO FURTHER.

1. Build the opening-game optimisation model from DERIVED quantities only, citing sources.
   Anything not derived is an explicit assumption with a sensitivity number.
2. COMPUTE FALSIFIABLE RESULTS: minimum time to each age; optimal citizen allocation across
   food, timber and wealth over the first N minutes; the marginal value of the Nth citizen;
   the opportunity cost of an early military building. Numbers with units and assumptions.
3. CROSS-CHECK AGAINST THE DESIGNERS: ron-data/ai-scripts/economic.bhs is a human-authored
   boom order by Mark Sobota and Mike Engle. Where your optimum disagrees with what they
   wrote, that disagreement is the headline result. Report both and reason about why.
4. THEN BUILD A PLAYER: turn the optimum into a heuristic agent in crates/don-ai (coordinate
   with the ron-ai lane; it owns the transcription of the shipped AI, you own the
   optimiser-derived one). Run it against the transcribed shipped AI in our sim and report the
   result honestly, including if it loses.

Code at /Users/ember/dev/don/analysis/ (Python fine) plus the player in Rust. Write the report
as something a human would enjoy: tables, a clear argument, and a plot if it helps.


--- RESUMING AN INTERRUPTED RUN ---
A previous run of this exact lane was cut short (You've hit your session limit · resets 5:30pm (America/New_York)) after 71 tool calls. Its work is
still on disk. Do NOT start over — verify what is listed below, then continue from there.

Files it already wrote or edited (1):
  /Users/ember/dev/don/analysis/opening.py

Commands it ran (23 distinct, first 15):
  $ ls -la /Users/ember/dev/don/docs/tracks/ /Users/ember/dev/don/analysis
  $ ls crates/ && ls schema/ && ls -la crates/don-ai 2>/dev/null | head -2
  $ python3 -c "
  $ find crates/don-ai -type f | head -50 && echo "---" && cat crates/don-
  $ python3 - <<'EOF'
  $ mkdir -p /private/tmp/claude-501/-Users-ember-dev-don/62b78482-846c-4f
  $ SP=/private/tmp/claude-501/-Users-ember-dev-don/62b78482-846c-4ffd-a44
  $ ls -la /Users/ember/dev/don/schema/live/ && ls -la /Users/ember/dev/do
  $ head -60 /Users/ember/dev/don/schema/live/rules-block-pid14644.txt
  $ cd /Users/ember/dev/don/ron-data && ls ai-scripts/ | head -40; echo --
  $ cd /Users/ember/dev/don/ron-data && ls; echo "--- find bhs"; find . -n
  $ cd /Users/ember/dev/don/ron-data && grep -n -i -A4 -B2 "gather_rate\|c
  $ cd /Users/ember/dev/don/ron-data && python3 - <<'EOF'
  $ sed -n '1,140p' analysis/econ.py
  $ sed -n '140,420p' analysis/econ.py

--- END RESUME CONTEXT ---