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

YOUR LANE: tech-cities
CHECKSUM CHANNEL(S) YOU SERVE: cities
WRITE YOUR REPORT TO: /Users/ember/dev/don/docs/mechanics/tech-cities.md
YOUR RUST MODULE: /Users/ember/dev/don/crates/don-sim/src/systems/tech_cities.rs
(create docs/mechanics/ and crates/don-sim/src/systems/ if needed)

THE TECH TREE AND CITIES.
TECH: research costs and durations from techrules.xml, prerequisite graph evaluation, age
advancement and its requirements, and crucially HOW TECH EFFECTS APPLY to units and buildings —
the military-level attack and armor upgrades are already known (get_attack adds 10*level,
get_armor adds 1*level), but the general effect-application mechanism is not. Find it by symbol.
CITIES: city placement rules and minimum spacing, city levels and how they advance, population
and housing, the city capture and recapture mechanism including the recapture damage modifier,
city plunder, and the auto-gather cities perform.
Cities are their own checksum channel with a 160-slot pool.


--- RESUMING AN INTERRUPTED RUN ---
A previous run of this exact lane was cut short (You've hit your session limit · resets 5:30pm (America/New_York)) after 71 tool calls. Its work is
still on disk. Do NOT start over — verify what is listed below, then continue from there.

Commands it ran (49 distinct, first 15):
  $ ls && ls schema/ && ls docs/mechanics/ 2>/dev/null; ls crates/don-sim/
  $ python3 tools/pdb/lookup.py --name 'City' 2>&1 | head -100
  $ python3 tools/pdb/lookup.py --name 'Cities::' 2>&1 | head -60; echo --
  $ python3 tools/pdb/lookup.py --name 'Tech' 2>&1 | head -120
  $ ls /Users/ember/dev/don/re/decomp-all/ | head -3; for f in 00937600 00
  $ python3 -c "
  $ cat re/decomp-all/00937600.c; echo "=== 00735410 Cities::walk_data ===
  $ python3 - <<'EOF'
  $ python3 tools/pdb/lookup.py 489040 a1b2d0 936560 937040 2>&1 | head -2
  $ ls /Users/ember/dev/don/ron-data/ | head -50; echo ---; ls /Users/embe
  $ cd /Users/ember/dev/don/ron-data && grep -n -i 'city\|cities' rules.xm
  $ cd /Users/ember/dev/don/ron-data && head -60 techrules.xml && echo "..
  $ cat re/decomp-all/0066cba0.c
  $ grep -c . re/decomp-all/MANIFEST.jsonl 2>/dev/null; python3 - <<'EOF'
  $ mkdir -p /private/tmp/claude-501/-Users-ember-dev-don/62b78482-846c-4f

--- END RESUME CONTEXT ---