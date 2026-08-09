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

YOUR LANE: production
CHECKSUM CHANNEL(S) YOU SERVE: builds
WRITE YOUR REPORT TO: /Users/ember/dev/don/docs/mechanics/production.md
YOUR RUST MODULE: /Users/ember/dev/don/crates/don-sim/src/systems/production.rs
(create docs/mechanics/ and crates/don-sim/src/systems/ if needed)

PRODUCTION: CONSTRUCTION, TRAINING, AND QUEUES.
Implement: building placement validity and footprint, construction progress and the build rate
reduction while under attack, unit training queues, the JOB_EXTRA_TIME build-time ramp and its
cap, the cost ramping system (SUPPORT and PROGRESSION per unit type, with PROGRESSION being a
mode selector 0-3), repair, upgrades and their cost formula, and building destruction including
the under-construction secondary damage rule.
Buildings are their own checksum channel and their own pool (601 slots) with MiningList embedded
at Build+0x98 and GatherPointList at Build+0xb8 — those embeddings are [measured] from address
geometry, so respect them in your layout.


--- RESUMING AN INTERRUPTED RUN ---
A previous run of this exact lane was cut short (You've hit your session limit · resets 5:30pm (America/New_York)) after 64 tool calls. Its work is
still on disk. Do NOT start over — verify what is listed below, then continue from there.

Commands it ran (45 distinct, first 15):
  $ grep -n -i "checksum\|check_all\|channel" /Users/ember/dev/don/docs/de
  $ ls -la /Users/ember/dev/don/crates/don-sim/src/ /Users/ember/dev/don/d
  $ python3 -c "
  $ python3 - <<'EOF'
  $ grep -rn -i "job_extra_time\|JOB_EXTRA" --include=*.json --include=*.r
  $ grep -rn -i "job_extra" . 2>/dev/null | head -20; echo "=== rules-cons
  $ sed -n '1180,1280p' /Users/ember/dev/don/crates/don-sim/src/mechanics.
  $ sed -n '1600,1760p' /Users/ember/dev/don/crates/don-rules/src/offsets.
  $ cd /Users/ember/dev/don; ls re/decomp-all | head -3; ls re/decomp-all 
  $ cd /Users/ember/dev/don; wc -l re/decomp-all/0062d5c0.c; cat re/decomp
  $ cd /Users/ember/dev/don; python3 - <<'EOF'
  $ cd /Users/ember/dev/don; cat re/decomp-all/006508c0.c
  $ cd /Users/ember/dev/don; cat re/decomp-all/00937290.c; echo "=== Build
  $ cd /Users/ember/dev/don; python3 tools/pdb/lookup.py 6305f0 471c30 470
  $ cd /Users/ember/dev/don; cat re/decomp-all/00642510.c

--- END RESUME CONTEXT ---