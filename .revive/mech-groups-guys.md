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

YOUR LANE: groups-guys
CHECKSUM CHANNEL(S) YOU SERVE: groups, guys
WRITE YOUR REPORT TO: /Users/ember/dev/don/docs/mechanics/groups-guys.md
YOUR RUST MODULE: /Users/ember/dev/don/crates/don-sim/src/systems/groups_guys.rs
(create docs/mechanics/ and crates/don-sim/src/systems/ if needed)

GROUPS AND GUYS — two checksum channels nobody has touched, and unlike most pools these two
showed MOVING counts in live scans, so they are genuinely dynamic.
GUYS: a Unit in RoN is a squad of individual soldiers. Guy is a separate 481-object population
with its own Guy::process and Guy::move called from the per-object chain. Implement guy spawning
per unit, GUY_SPACING and formation within a unit, per-guy movement and animation-driven timing,
and how guy count relates to unit hit points and to the 3-sub-unit damage division.
GROUPS: player unit groups and control groups, group orders (GroupMoveOrder, GroupAttackOrder,
GroupPatrolOrder), formation shapes and FORM orders, and how a group order decomposes into
per-unit orders. This matters for the RL action space, which will issue group orders.


--- RESUMING AN INTERRUPTED RUN ---
A previous run of this exact lane was cut short (You've hit your session limit · resets 5:30pm (America/New_York)) after 54 tool calls. Its work is
still on disk. Do NOT start over — verify what is listed below, then continue from there.

Commands it ran (33 distinct, first 15):
  $ ls && ls crates/don-sim/src/ && ls docs/mechanics/ 2>/dev/null; ls cra
  $ python3 tools/pdb/lookup.py --name 'Guy::' 2>&1 | head -100
  $ python3 tools/pdb/lookup.py --name 'GuyData::' 2>&1 | head -60
  $ python3 tools/pdb/lookup.py --name 'CheckSums::check_g' 2>&1 | head -3
  $ ls /Users/ember/dev/don/re/decomp-all/ | head -5; for f in 00937430 00
  $ python3 -c "
  $ python3 tools/pdb/lookup.py 937530 937430 708400 5089d0 46df30 e85f14 
  $ python3 tools/pdb/lookup.py --name 'Groups::' 2>&1 | head -60
  $ cd /Users/ember/dev/don/ron-bin && uv run --quiet --with capstone --wi
  $ python3 tools/pdb/lookup.py --name 'guy' 2>&1 | grep -i 'PROC\|size=' 
  $ grep -in 'guy' schema/rise-procs.tsv | head -80
  $ cd /Users/ember/dev/don/ron-bin && uv run --quiet --with pefile python
  $ python3 tools/pdb/lookup.py --name 'Unit::set_type' --name 'Unit::init
  $ cat /Users/ember/dev/don/re/decomp-all/00612fa0.c 2>/dev/null | head -
  $ cd /Users/ember/dev/don; grep -inP '\t(Unit|UnitData|Object|ObjectData

--- END RESUME CONTEXT ---