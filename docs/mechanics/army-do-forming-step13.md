# Step 13 red recovery: `Army::do_forming`

This tranche recovers the complete deterministic body of `Army::do_forming(int)` at
`0x006F43C0` into the source-frozen module
`crates/don-sim/src/systems/army_do_forming.rs`.  It deliberately does not edit
`systems/mod.rs`, `systems/armies.rs`, `tick.rs`, or the save owner.  The current step-13
row therefore remains honestly red until a later integration tranche installs the host
and removes this one reached gap.

## Selection from the compiled 29-step inventory

`schema/simulation-closure.json` currently reports 20/29 tick rows complete.  The nine red
rows are 4, 8, 11, 12, 13, 15, 17, 19, and 22.  Rows 12 and 15 were excluded by lane
ownership.  Rows 17, 19, and 22 already contain full deterministic bodies; their red state
comes from stale static `schedule::DO_FRAME` statuses rather than another body to recover.
Step 8's bounded `Wall::update_construct_time` body already exists in `production.rs`; its
remaining work is canonical input population.  Its largest wholly absent direct child is
`Object::eject_contents` `0x0064CD20` (2,962 bytes), but the step-8 call reaches the still
unbounded 9,925-byte `Unit::come_out` transaction and its conditional RNG tail for every
immediate passenger.  It is therefore not a closed source-only body without first freezing
a chain/come-out/RNG receipt transaction.  Step 11's remaining bodies are the 11,108 byte
`Leader::plan_strategy` and 20,348 byte `Leader::diplomacy` policy monoliths.

Step 13 has five direct status bodies still named as gaps.  `do_forming` is the largest one
that does not enter `find_target`, `find_muster_spot`, or any unresolved RNG cone.  Its
Group action endpoints remain typed receipts and independently red; this tranche recovers
the complete Army-level orchestrator, not those downstream action bodies:

| body | VA | bytes | boundary |
|---|---:|---:|---|
| `Army::do_mustering` | `0x006F4260` | 337 | bounded |
| `Army::do_defending` | `0x006F4070` | 488 | bounded |
| `Army::do_marching` | `0x006F3DF0` | 633 | reaches `find_target` |
| **`Army::do_forming`** | **`0x006F43C0`** | **720** | **bounded, recovered here** |
| `Army::do_transporting` | `0x006F4690` | 770 | reaches `find_target` |

`Army::find_target` `0x006F69B0` is a separate 7,571-byte body and the only RNG consumer
in the whole Army/Armies cone.  Treating a transcription of its three known draws as a
port of its target policy would be false closure, so it remains red.

## Ground truth and exact control flow

The sources are the shipped `ron-bin/riseofnations.exe` (the repository-pinned retail
image), `ron-bin/sbl/rise.pdb`, `schema/types.json`, and the independent decompiler listing
`re/decomp-all/006f43c0.c`.  PDB says `army.cpp:3045-3141`, 720 bytes.  The next procedure
starts at `0x006F4690`, so the body boundary is exact.

1. `0x006F43C9`: call `Army::is_engaged` `0x006F56D0`.  That callee first runs
   `Army::normalize` `0x006F9B50`, so derived Army fields and Group-list ordering may change
   even when `is_engaged` returns zero.  A non-zero result then returns 0 before any later
   read or write in `do_forming`.
2. `0x006F43DB..0x006F4420`: form the center
   `(muster_x*0x300+0x180, muster_y*0x300+0x180)` and call `project` `0x0092CF40` with
   `muster_angle` and signed `num_groups*0xC0`.  This call occurs even for a non-positive
   live-prefix length.
3. `0x006F4437..0x006F4680`: walk `ArmyData::list[0..num_groups]`.  Negative ids and Groups
   whose `GroupData::num +0x0C` is exactly zero are skipped and consume no spacing.
4. `0x006F4467`: for each live Group, write the sign-extended `ArmyData::army +0x02` to
   `GroupData::army +0x08`.  This is the first mutation in the per-group loop and precedes
   every relation, building, and target query there; the normalize prefix has already run.
5. `0x006F446E..0x006F44D9`: the early-age land branch may call `set_stance(3)`.  Its
   short-circuit is exact: non-negative target owner, target differs from the Leader's
   `+0x08` identity, either Leader diplomacy or the mirrored setup relation is zero,
   `navy == 0`, decoded age below 4, and a fresh `Army::count(4,0) == 0`.  Because this is
   inside the group loop, the army-wide stance fan-out can repeat once per live Group.
6. `0x006F44DE..0x006F452C`: call
   `ObjectsData::find_building(x,y,3,who,0x300,0x200,9,0,0)`.  A found building blocks the
   move branch only when another fresh `count(4,0)` is non-zero.
7. `0x006F4534..0x006F45A5`: navies bypass land-target validation.  Land armies require
   the target owner to be self or `LeaderData::is_enemy`, a non-negative target object,
   a non-negative target ObjectData type index, and `(Type.flags & 3) == 3`.  Retail calls
   `is_enemy(-1)` before noticing a negative target object; the port retains that order.
8. Move branch: `set_stance(0)` then `Group::action_move_to` `0x0070FBA0` with exact
   arguments `(x,y,2,1,angle,2,1,-1,-1,0)`.  Other cases call
   `Group::action_siege_attack_to` `0x0070D830`.  At this site the middle two formal stack
   arguments contain stale values; disassembly of all 2,037 callee bytes shows it reads
   only `x`, `y`, and the final angle, so the typed seam exposes exactly those observable
   values instead of canonizing arbitrary stack residue.
9. `0x006F45E9..0x006F4662`: only after a live Group action, advance the cursor by
   `sinx(muster_angle,0x180)` and `-cosx(muster_angle,0x180)`.  Loop, then return 1.

All coordinate multiplication/addition in the source uses wrapping `i32`, matching x86.
No loop bound or invalid id is clamped.  An impossible `num_groups > 16` becomes a typed
fail-closed `ArmyList` boundary rather than an out-of-bounds read.

## Ownership, fail-closed seam, and mutation order

`FormingArmy` is a field-for-field slice of `ArmyData`: `army +0x02`, `navy +0x24`, target
pair `+0x30/+0x34`, muster tuple `+0x48..+0x50`, list `+0x54`, and owner/length
`+0x94/+0x96`.  The body has no direct ArmyData store, but its first `is_engaged` call
normalizes the canonical Army and may reorder `list`.  The host method receives a mutable
view and must refresh it before the formation walk continues; retaining a pre-normalize
snapshot would visit the wrong Group order.

`FormingHost` owns every external read or mutation.  Queries return `Option<T>` and writes
return success.  Absence maps to a contextual `MissingFact`; execution stops immediately,
retains earlier writes, and performs no later query.  This deliberately models retail
mutation order rather than preflighting all facts into an atomic transaction.  The focused
test `missing_fact_stops_after_the_group_army_write` pins the most important boundary.

The external owners are:

| seam | retail owner |
|---|---|
| engagement/count/status | existing `ArmyData` plus `ArmyWorld` |
| group num/army/action | canonical `groups_guys::Groups` |
| identity/diplomacy/age/enemy | canonical eight `LeaderData` rows |
| mirrored relation | setup/GameInfo player relation matrix |
| building search | `ObjectsData` plus spatial index |
| target type | canonical object registry and 806-row Type table |
| project/advance | existing exact integer `trig::{sinx,cosx}` |

## RNG and checksum consequences

The 720-byte body has no `Random::get` call.  The direct entry bodies used here
(`is_engaged`, `project`, `count`, `is_enemy`, `set_stance`, `find_building`,
`action_move_to`, and `action_siege_attack_to`) also have no direct `Random::get` call.
Their unrecovered downstream order paths are not claimed RNG-audited.  The Army-level
orchestrator itself therefore consumes zero RNG values and cannot repair the separate
stream hazard in `Army::find_target`.

The first per-group mutation, `GroupData::army +0x08`, lies in `Group::walk_data`'s header
and changes `CheckSums::check_groups` channel 6 immediately.  The earlier normalization
prefix also mutates save-owned Army aggregates and can normalize Group state.  `set_stance`
and the two action calls emit
Group/Unit orders, changing the groups, guys, and units channels as those canonical owners
materialize them.  `ArmyData` itself is save-game state but is not one of `check_all`'s 15
per-turn channels; this body makes Army decisions checksum-visible through Groups and
Units, not through an armies channel.

## Frozen minimal integration map and honest red boundary

The later integrator needs only these edits, none performed here:

1. export `army_do_forming` from `systems/mod.rs`;
2. adapt `armies::ArmyData` to `FormingArmy` without a second persistent owner;
3. implement `FormingHost` over the existing `ArmyWorld` plus the canonical Groups,
   Leaders, Objects, setup-relation, and Type owners;
4. replace only the `status & ST_FORMING` gap in `Army::process`, preserving the current
   status-dispatch order; and
5. remove `do_forming` from `ArmyGaps`/`RUNTIME_FIDELITY_BLOCKERS` only after focused and
   full remote tests pass.

This does **not** make tick step 13 complete.  The honest remaining boundary includes
`do_mustering`, `do_defending`, `do_marching`, `do_transporting`, `engagement`, the three
specialist users, `find_muster_spot`, the full `find_target` policy/RNG cone, and the red
`Group::action_siege_attack_to` endpoint used by this orchestrator.  The compiled row must
remain `Stub` until all reached bodies are integrated and evidenced.

No Cargo, rustc, formatter, or local/remote build was run in this source-recovery lane, by
design.  The path-based test file makes the isolated module compilable without touching a
shared export; build validation belongs to the integration owner.
