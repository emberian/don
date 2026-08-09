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

YOUR LANE: map-terrain
CHECKSUM CHANNEL(S) YOU SERVE: world
WRITE YOUR REPORT TO: /Users/ember/dev/don/docs/mechanics/map-terrain.md
YOUR RUST MODULE: /Users/ember/dev/don/crates/don-sim/src/systems/map_terrain.rs
(create docs/mechanics/ and crates/don-sim/src/systems/ if needed)

THE MAP. We currently have NO map at all — this is the largest single hole in the project.
Implement the terrain and world representation: the tile grid and its coordinate systems
(Coord/WCoord/TCoord each wrap one int — establish the relationships and the 48-world-unit
quarter-tile movement grid), heightmap, terrain types and their movement and combat modifiers,
cliffs, rivers and fords, and resource node placement.
Then MAP GENERATION: ron-data has mapstyles, Cliffs.xml, rivers.xml, roads.xml,
terrainunderlay.xml, tilesets.xml. Find the generator by symbol and determine whether it is
seeded reproducibly — if map generation is deterministic from the seed, we can generate the
same maps the engine does, which matters enormously for replay validation.
Note worldgen uses float freely; establish whether generated terrain is then quantised to
integers for the sim.


--- RESUMING AN INTERRUPTED RUN ---
A previous run of this exact lane was cut short (You've hit your session limit · resets 5:30pm (America/New_York)) after 67 tool calls. Its work is
still on disk. Do NOT start over — verify what is listed below, then continue from there.

Files it already wrote or edited (1):
  /Users/ember/dev/don/crates/don-sim/src/systems/map_terrain.rs

Commands it ran (34 distinct, first 15):
  $ ls -la /Users/ember/dev/don/ && ls /Users/ember/dev/don/docs/derivatio
  $ ls -la /Users/ember/dev/don/ron-data/ | head -60; echo "---"; ls /User
  $ find ron-data -iname '*cliff*' -o -iname '*river*' -o -iname '*road*' 
  $ grep -n "world" docs/derivation/checksum.md | head -60
  $ ls -R /Users/ember/dev/don/ron-bin | head -80; echo "=== search wide =
  $ python3 tools/pdb/lookup.py 6b5cf0 2>&1 | head -20; echo "=== World:: 
  $ ls schema/ && ls docs/mechanics 2>/dev/null; ls crates/don-sim/src/sys
  $ python3 -c "
  $ python3 - <<'EOF'
  $ ls re/decomp-all/006b5cf0.c 2>/dev/null && cat re/decomp-all/006b5cf0.
  $ for a in 47c660 478990 473120 681fa0 6b76f0 46d220; do echo "== $a"; p
  $ mkdir -p /private/tmp/claude-501/-Users-ember-dev-don/62b78482-846c-4f
  $ S=/private/tmp/claude-501/-Users-ember-dev-don/62b78482-846c-4ffd-a44c
  $ grep -rl "cae5fc\|00cae5fc\|DAT_00cae5fc" re/decomp-all/ 2>/dev/null |
  $ grep -rn "cae5fc" re/decomp-all/ 2>/dev/null | grep -iv "\[.*cae5fc.*\

--- END RESUME CONTEXT ---