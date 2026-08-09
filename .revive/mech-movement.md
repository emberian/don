PROJECT: Descent of Nations — /Users/ember/dev/don. Absolute paths always.
READ /Users/ember/dev/don/README-LLM.md FIRST. Then docs/derivation/architecture.md.

**BUILD, DO NOT RE-DERIVE.** The reverse engineering is done and machine-readable. Your job
is to read the NAMED functions for your subsystem, implement them in Rust, and make the
result measurable. Do not re-audit prior claims; if you find an error, note it in two
sentences and keep going.

GROUND TRUTH (all [measured], use freely):
  ron-bin/sbl/rise.pdb — the game own full PDB. Everything is NAMED now.
  schema/symbols.json — 22,750 functions with VA, name, signature, size, source file+line.
  schema/types.json, schema/pdb-types.json — 19,914 classes, 26,469 fields with offsets.
  schema/state-schema.json — 278 walk_data impls, 1,316 named fields, the sim-state schema.
  schema/command-wire.json — 82 command opcodes with exact layouts.
  schema/live/balance-real.bin — the REAL 493x493 int16 damage matrix (combat_table+4).
  docs/derivation/architecture.md — Game::do_frame 29 ordered subsystem calls, per-object chain.
  re/decomp-all/<EA>.c — bulk decompiled C. GREP THIS FIRST, it is far cheaper than decompiling.
  tools/pdb/{lookup,callers,xref}.py — symbol lookup and call graph.

LOAD-BEARING FACTS:
  The sim is INTEGERS. Coord/WCoord/TCoord each wrap one int. Floats live in presentation
  and worldgen. Known exceptions inside walked state: LeaderData::anti_att,
  LeaderData::plunder_scale, and Unit::move_step / find_path use sin_table, cosx, find_angle.
  Tick is 67 ms at Normal (TurnControl::timings 0x00AFC4A4 = {200,125,67,50,1} ms), NOT 15 Hz.
  Objects::process_all rotates owner-slot order every frame: (frame + i) % 10.
  Real rules loader is Constants::init 0x00569A90 (the 112 "loaders" are log_data).
  Damage is ObjectData::get_damage 0x00644130 — pure integer, armor subtracted mid-chain,
  attack stored x10, floor of 1 conditional. Ported in crates/don-sim/src/mechanics.rs with
  several steps still marked UNVERIFIED.
  Class-name suffix is the sim/presentation cut: XData = POD state, X = behaviour,
  XOut = presentation, XType = static rules. Ignore XOut entirely.
  Ghidra decompilation reorders integer vs FP ops on this profile — structure from decompiled
  C, VALUES from the instruction stream or from behaviour.

**FILE OWNERSHIP — CRITICAL, LANES ARE RUNNING IN PARALLEL.** A separate wave is rewriting
crates/don-sim/src/{world.rs,batch.rs,lib.rs,mechanics.rs} RIGHT NOW. Do NOT edit those.
Put your implementation in its OWN new module: crates/don-sim/src/systems/<yourlane>.rs, and
if you need it referenced, say so in your report rather than editing lib.rs. Write your report
only to your named path. Never git add -A, never commit, never git stash.

THE MEASURABLE GOAL: a replay-validation harness is being built that feeds a real command
stream into our sim and compares the engine 16 checksum channels per turn. Your subsystem maps
to one or more channels. Design so your state can be checksummed the way CheckSums::check_all
(0x00936560) does it, and say in your report which channel(s) you serve.

DELIVERABLE: working Rust in your own module, plus a report at your named path leading with
what now works and how it was measured. Honest partial coverage beats claimed completeness.

YOUR LANE: movement
CHECKSUM CHANNEL(S) YOU SERVE: units
WRITE YOUR REPORT TO: /Users/ember/dev/don/docs/mechanics/movement.md
YOUR RUST MODULE: /Users/ember/dev/don/crates/don-sim/src/systems/movement.rs
(create docs/mechanics/ and crates/don-sim/src/systems/ if needed)

MOVEMENT AND THE REAL UNIT PATHFINDER.
Prior work derived PathFinder::astar_caravan_road — the CARAVAN ROAD pathfinder, not the unit
one. Find the real unit pathfinding entry points by name and implement them.
Known: the unit grid is 48 world units = 1/4 tile, NOT the tile grid. PathFinder::calc_cost
makes ZERO RNG calls; the two draws in astar_path are in the FAILURE epilogue, setting a 6-8
tick retry delay when a third-party unit blocks. Unit::move_step and find_path DO use float
(sin_table, cosx, find_angle) so the integrator is not pure integer.
Implement: the A* itself (heuristic, tie-breaking, open/closed structure, path caching and
invalidation), Unit::do_move, Unit::move_step, unit collision and pushing, and formation
movement if it lives here. Report exactly where float enters and whether it is reproducible.


--- RESUMING AN INTERRUPTED RUN ---
A previous run of this exact lane was cut short (You've hit your session limit · resets 5:30pm (America/New_York)) after 30 tool calls. Its work is
still on disk. Do NOT start over — verify what is listed below, then continue from there.

Files it already wrote or edited (1):
  /private/tmp/claude-501/-Users-ember-dev-don/62b78482-846c-4ffd-a44c-2199d3744a8e/scratchpad/part2.rs

Commands it ran (13 distinct, first 15):
  $ ls -la /Users/ember/dev/don/crates/don-sim/src/ /Users/ember/dev/don/d
  $ python3 tools/pdb/lookup.py --name 'PathFinder::' 2>&1 | head -80
  $ cd /Users/ember/dev/don/re/decomp-all && for f in 00683770 00684e50 00
  $ sed -n '1,120p' docs/derivation/pathfinding.md
  $ python3 tools/pdb/lookup.py 46cff0 479540 4796f0 479920 479b90 479770 
  $ cd /Users/ember/dev/don/ron-bin && uv run --quiet --with capstone --wi
  $ python3 tools/pdb/lookup.py 92d130 a46a00 92d0c0 46f960 6b52e0 46d820 
  $ grep -rl "e32f40" re/decomp-all/ 2>/dev/null | head -20; echo "--- xre
  $ python3 tools/pdb/lookup.py --name 'sin_table' 2>&1|head; echo ---; py
  $ python3 - <<'EOF'
  $ grep -n -i "adler\|check_all\|CheckSum::" docs/derivation/checksum.md 
  $ mkdir -p /Users/ember/dev/don/crates/don-sim/src/systems /Users/ember/
  $ python3 - <<'PYEOF'

--- END RESUME CONTEXT ---