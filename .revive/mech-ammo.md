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

YOUR LANE: ammo
CHECKSUM CHANNEL(S) YOU SERVE: ammo
WRITE YOUR REPORT TO: /Users/ember/dev/don/docs/mechanics/ammo.md
YOUR RUST MODULE: /Users/ember/dev/don/crates/don-sim/src/systems/ammo.rs
(create docs/mechanics/ and crates/don-sim/src/systems/ if needed)

PROJECTILES AND THE AMMO SYSTEM. Ammo is its own checksum channel and its own pooled class
(400 slots, embedded nowhere — it is not inside Unit).
Implement: projectile spawn on attack, PROJ_SPEED flight integration, ballistic arcs (AmmoData
ballistics are float — establish exactly which fields and whether they are sim-critical),
impact and hit resolution, AMMO_PER_ATT damage splitting with armor applied per projectile,
and miss behaviour. Report how projectiles interact with the damage pipeline and with death
ordering, since both feed the units and deaths channels.


--- RESUMING AN INTERRUPTED RUN ---
A previous run of this exact lane was cut short (You've hit your session limit · resets 5:30pm (America/New_York)) after 57 tool calls. Its work is
still on disk. Do NOT start over — verify what is listed below, then continue from there.

Files it already wrote or edited (1):
  /Users/ember/dev/don/crates/don-sim/src/systems/ammo.rs

Commands it ran (37 distinct, first 15):
  $ grep -n -i "ammo\|projectile\|proj_speed\|launch" docs/derivation/arch
  $ ls docs/mechanics/ 2>/dev/null; ls crates/don-sim/src/; ls crates/don-
  $ python3 tools/pdb/lookup.py --name 'Ammo' 2>/dev/null | head -80
  $ for f in 9374e0 67ab50 67d380 64c8b0 658b10 678d90 678f90; do echo "==
  $ python3 -c "
  $ cat re/decomp-all/009374e0.c re/decomp-all/0067ab50.c
  $ python3 - <<'EOF'
  $ cat re/decomp-all/0064c8b0.c
  $ grep -n "c061f4\|c061f8\|c061fc\|c06214\|c0aec0\|c0ab84" schema/rise-s
  $ sed -n '1,200p' re/decomp-all/0067d380.c
  $ ls -la re/decomp-all/0067bbf0.c re/decomp-all/00678060.c 2>&1; grep -n
  $ sed -n '200,400p' re/decomp-all/0067d380.c
  $ cat /Users/ember/dev/don/re/decomp-all/00678f90.c
  $ python3 tools/pdb/lookup.py 46cff0 866560 43f360 9096e0 6b53a0 913960 
  $ cat /Users/ember/dev/don/re/decomp-all/00678d90.c

--- END RESUME CONTEXT ---