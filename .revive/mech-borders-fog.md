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

YOUR LANE: borders-fog
CHECKSUM CHANNEL(S) YOU SERVE: world
WRITE YOUR REPORT TO: /Users/ember/dev/don/docs/mechanics/borders-fog.md
YOUR RUST MODULE: /Users/ember/dev/don/crates/don-sim/src/systems/borders_fog.rs
(create docs/mechanics/ and crates/don-sim/src/systems/ if needed)

BORDERS, TERRITORY, SUPPLY, ATTRITION, AND FOG OF WAR.
Borders are RoN defining mechanic and nobody has implemented them. RTTI shows BorderSpline, so
they are spline-based rather than a simple radius — which is why no closed-form formula was
ever published by the community. Find the real border computation by symbol and implement it:
city and fort border push, tech and building contributions, border growth over time, and the
territory ownership query that everything else depends on.
Then SUPPLY and ATTRITION, which are territory-derived: attrition damage rate and period, the
out-of-supply siege reload penalty, and which units are immune.
Then FOG OF WAR and LOS: visibility state per player, unit LOS radii, detection of stealthed
units, and explored-versus-visible. Fog is required for partial observability in the RL env,
so design it to be queryable per player cheaply.


--- RESUMING AN INTERRUPTED RUN ---
A previous run of this exact lane was cut short (You've hit your session limit · resets 5:30pm (America/New_York)) after 44 tool calls. Its work is
still on disk. Do NOT start over — verify what is listed below, then continue from there.

Files it already wrote or edited (1):
  /Users/ember/dev/don/crates/don-sim/src/systems/borders_fog.rs

Commands it ran (32 distinct, first 15):
  $ grep -n -i "checksum\|check_all\|channel\|fog\|border\|territory\|supp
  $ ls /Users/ember/dev/don/docs/mechanics/ 2>/dev/null; ls /Users/ember/d
  $ python3 tools/pdb/lookup.py --name 'World::' 2>&1 | head -120
  $ python3 tools/pdb/lookup.py --name 'WorldData::' 2>&1 | head -90
  $ python3 tools/pdb/lookup.py --name 'GameDaemon::' 2>&1 | head -60
  $ cd /Users/ember/dev/don/re/decomp-all && for f in 006b5cf0 006b0bb0 00
  $ python3 -c "
  $ python3 - <<'EOF'
  $ grep -n -i "border\|territor" crates/don-rules/src/offsets.rs | head -
  $ python3 tools/pdb/lookup.py cae5fc c09970 c0a490 e3b7ee e3a798 e3a7b8 
  $ grep -n "cae5fc\|0x00cae5fc" schema/rise-symbols.tsv | head; echo ---;
  $ cd /Users/ember/dev/don/re/decomp-all && grep -rl "DAT_00cae5fc" . | h
  $ cd /Users/ember/dev/don/re/decomp-all && for f in 0063eca0 006b4700 00
  $ cd /Users/ember/dev/don/re/decomp-all && sed -n '1,200p' 006b76f0.c
  $ python3 tools/pdb/lookup.py --name 'attrition' 2>&1|head -20; echo ===

--- END RESUME CONTEXT ---