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

YOUR LANE: combat
CHECKSUM CHANNEL(S) YOU SERVE: units, deaths
WRITE YOUR REPORT TO: /Users/ember/dev/don/docs/mechanics/combat.md
YOUR RUST MODULE: /Users/ember/dev/don/crates/don-sim/src/systems/combat.rs
(create docs/mechanics/ and crates/don-sim/src/systems/ if needed)

THE COMBAT RESOLUTION LOOP — not the damage formula, which is already ported.
Implement everything AROUND ObjectData::get_damage: target acquisition and selection, the
attack cycle and RECHARGE timing, range checks and ATTENUATE accuracy, the overkill timestamp
mechanism (x1/3 damage inside a frame window — find the real constant and the real window),
splash damage and SPLASH_AREA/SPLASH_PERCENT, flanking level computation in the real world
(flank_level at 0x0092CFE0 is Tier B at 500,017 inputs; wire it up), height and terrain
modifiers, entrenchment, death and corpse handling, and experience or veterancy if present.
Load the real balance matrix from schema/live/balance-real.bin (493x493 int16) — do not embed.
Also finish the UNVERIFIED steps in the existing damage port where the symbols now let you.


--- RESUMING AN INTERRUPTED RUN ---
A previous run of this exact lane was cut short (You've hit your session limit · resets 5:30pm (America/New_York)) after 64 tool calls. Its work is
still on disk. Do NOT start over — verify what is listed below, then continue from there.

Commands it ran (35 distinct, first 15):
  $ grep -n "^## \|^### " docs/derivation/architecture.md | sed -n '1,200p
  $ ls && echo "--- crates ---" && ls crates && echo "--- don-sim src ---"
  $ sed -n '988,1075p' docs/derivation/architecture.md
  $ wc -l crates/don-sim/src/*.rs && ls schema/ && ls schema/live/ 2>/dev/
  $ grep -n "UNVERIFIED\|fn \|pub struct\|pub const" crates/don-sim/src/me
  $ python3 tools/pdb/lookup.py --name 'Object::' 2>/dev/null | head -100
  $ ls docs/derivation/ && head -30 docs/derivation/combat.md 2>/dev/null
  $ grep -n "^#\|^##\|^###" docs/derivation/combat.md
  $ wc -l docs/derivation/combat.md docs/derivation/damage-port.md
  $ sed -n '73,110p;216,235p;434,470p' docs/derivation/combat.md
  $ python3 - <<'EOF'
  $ python3 tools/pdb/lookup.py 678060 92bc80 64a480 64e5c0 652020 2>/dev/
  $ cd /Users/ember/dev/don/re/decomp-all && for f in 0064a480 00652020 00
  $ python3 tools/pdb/lookup.py --name 'CheckSums::' 2>/dev/null | head -7
  $ cd /Users/ember/dev/don/re/decomp-all && cat 009371d0.c 2>/dev/null; e

--- END RESUME CONTEXT ---