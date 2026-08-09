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

YOUR LANE: victory-score
CHECKSUM CHANNEL(S) YOU SERVE: all
WRITE YOUR REPORT TO: /Users/ember/dev/don/docs/mechanics/victory-score.md
YOUR RUST MODULE: /Users/ember/dev/don/crates/don-sim/src/systems/victory_score.rs
(create docs/mechanics/ and crates/don-sim/src/systems/ if needed)

VICTORY CONDITIONS, SCORING, AND MATCH LIFECYCLE.
The defeat screen shows a score (766 in a real observed match) and a game time, so scoring is
computed and exposed — find it by symbol and implement it. This becomes the RL reward signal, so
accuracy matters more than it looks.
Implement: all victory conditions (conquest, wonder, territory, score, timed), the defeat and
elimination check, diplomacy state (war, peace, alliance) and how it gates targeting, the
Armageddon clock if it is sim-critical, and match end handling.
Also determine the SCORE FORMULA exactly — what contributes and with what weight. Report it as a
formula with sources. An RL agent trained against a wrong reward learns the wrong game.


--- RESUMING AN INTERRUPTED RUN ---
A previous run of this exact lane was cut short (You've hit your session limit · resets 5:30pm (America/New_York)) after 70 tool calls. Its work is
still on disk. Do NOT start over — verify what is listed below, then continue from there.

Files it already wrote or edited (1):
  /private/tmp/claude-501/-Users-ember-dev-don/62b78482-846c-4ffd-a44c-2199d3744a8e/scratchpad/dis.py

Commands it ran (41 distinct, first 15):
  $ python3 tools/pdb/lookup.py --name 'check_victory' 2>&1 | head -40; ec
  $ ls re/decomp-all/ | head -3; for f in 006ec560 006bc190 006bc1a0 006bc
  $ python3 tools/pdb/lookup.py --name 'victory' 2>&1 | head -60
  $ cat re/decomp-all/006ec560.c
  $ cat re/decomp-all/006e37f0.c
  $ python3 -c "
  $ ls schema/ && python3 -c "
  $ python3 - <<'EOF'
  $ for f in 006bc1a0 006bc270 006bc360 006bc3f0 006bc500; do echo "===== 
  $ python3 tools/pdb/lookup.py 594020 6e0c80 6db510 47cd40 6e33a0 6673b0 
  $ head -3 /Users/ember/dev/don/schema/rise-procs.tsv
  $ cd /Users/ember/dev/don/ron-bin && sed -i '' 's/p\[-1\] if len(p) > 2 
  $ mkdir -p /private/tmp/claude-501/-Users-ember-dev-don/62b78482-846c-4f
  $ cd /private/tmp/claude-501/-Users-ember-dev-don/62b78482-846c-4ffd-a44
  $ ls /Users/ember/dev/don/ron-data/ 2>/dev/null | head -30; echo "---"; 

--- END RESUME CONTEXT ---