# 2024 golden replay frames 2--31 chronology audit

Status: **static retail schedule recovered; replay closure remains capture-gated**. This document
freezes the vertical audit from the exact post-command frame-one tick return (`Game::frame == 2`)
through the end of the frame-31 tick, immediately before frame 32's wildlife block. It is not a
checksum, corpus-survival, or full-tick implementation claim.

The supported recording is
`Playback___2024.02.23_20_49_35__Fri_.rcx`, SHA-256
`1690431a5ef19b38a3425d3dd7311e8e83ca0d27c56fabe49d776a9f1421b251`, with setup RNG seed
`0x00bb97d3`. Function addresses below refer to the supported retail executable, SHA-256
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`.

## Authority limit

`setup_2024_golden_capture::GoldenCaptureManifest` schema 2 is a capture contract, not a source
of native bytes. Its eleven mandatory canonical DoNSave images are:

1. completed `place_all` / post-Dutch-Market `Setup::build_units` entry;
2. seven adjacent `Objects::init_unit` receiver after-images;
3. frame-one command entry after the frame-zero scheduler;
4. frame-one return after both serial-1 LeaderOptions commands;
5. the post-frame-one-tick image whose live frame is 2.

No production bundle is checked into the repository. The last image would bind complete orders,
paths, Unit and Guy state, generated World, object bands, persistent cursors and RNG at frame 2;
the schema does not manufacture any of those values. A frame-2 digest is also only provenance for
a later frame. It cannot authorize a frame-26 Unit position or WData owner after 24 intervening
ticks.

Therefore this audit separates:

- schedule facts derived from the executable and replay packages;
- reaches conditional on the missing canonical image; and
- state, RNG and external-child outcomes which must remain red.

Recorded replay checksums are never accepted as state inputs.

## Fixed owner-zero inventory

The source-bound setup order fixes this owner-zero cohort:

| object | `o` | type | Guys |
|---|---:|---:|---:|
| Scout | 0 | 69 | squad 1 + crew 1 = 2 |
| Dutch Armed Merchant | 1, 2 | 62 | squad 1 + crew 2 = 3 each |
| Citizen | 3, 4, 5, 6 | 50 | squad 1 = 1 each |
| Village | 2000 | 414 | Build |
| Dutch Market | 2001 | 436 | Build |

The two Builds are active, owner zero and linked to the same starting City slot. The seven Units
contain twelve Guys in total. These identities do not prove live frame-2 orders, positions,
animation records or masks. The owner-one and nature populations are likewise not replaced with
an empty synthetic inventory.

## Tick chronology

`Game::do_frame` `0x00591EF0` uses the pre-increment frame through object processing. The frame
counter changes at step 20, `0x005924BF`.

### Step 8: Leaders

`Leaders::process_all` `0x006ED2A0` visits fixed Leader slots. Every processed row clears its
hostile-seen bit, zeros the two per-frame counters, scans hostility, calls `Leader::gather`
`0x006CE280`, runs edge-triggered wall/unit stat passes, elimination, grace timers and taunts,
then clears the pending bit at `0x006ED407`.

`Leader::gather` always clears expenses, calculates caps and performs six-resource payout. Its
gross-income child, `Leader::calc_gather` `0x006CEEE0`, is due on:

```text
dirty: frame != 0 && (slot + frame) % 8 == 0
clean: frame >= last_calc_frame + 300 && (frame + slot*8) % 256 == 0
```

The source-derived Dutch Market Leader transaction dirties owner zero's economy, so the first
owner-zero recomposition is frame 8. That transaction exists on a detached implementation line;
until it and the native capture are composed, the actual frame-2 dirty/last-frame image remains an
authority input. An owner-one frame-7 recomposition must not be inferred without its captured
dirty bit.

Grace-timer cadence is frames 5, 10, 15, 20, 25 and 30. A timer changes only when its captured
value is negative and not frozen.

### Step 11: strategy

`Leaders::strategy_all` `0x006ED430` calls, per active row and in this order:

1. `Leader::check_explore` `0x006BC860`;
2. `Leader::plan_strategy` `0x006B9620`;
3. `Leader::compute_score(0)` `0x006EC560`;
4. `Leader::diplomacy` `0x006BC950`.

No exploration recount is due in frames 2--31. Human diplomacy returns at its entry gate. With
shipped `ai_speed == 1` and captured `production_step == 0`, the MakeList fast lane is due for
owner one at frame 5 and owner zero at frame 30. It reads the raw MakeList head; a head in
`0x220..0x274` reaches `Leader::can_pay` `0x006C9B90`. The head and production step are live state,
not replay constants.

Score mutation is due for owner one at frames 9, 19 and 29, and owner zero at frames 10, 20 and
30. `Leader::compute_explore_score` `0x006BC5E0` and `Leader::compute_pop_score` `0x006BC190`
are themselves retail stubs. The Sim's zero values are exact and are not unresolved score
children.

### Step 12: GameDaemon

`GameDaemon::process_all` `0x00732700` performs its local counters, victory pass, market pass,
64-region flag maintenance, border scan, collision-block reaper and `Groups::process`
`0x006FA210` tail in that order.

- No `calc_danger` (`frame % 200 == 0`) is due.
- No `update_all_seen` (`frame % 100 == 33`) is due.
- Region flags, border progress, the collision cursor and Group process cursor remain persistent
  simulation state even when their live populations are empty.

`GameDaemon::calc_markets` `0x00732180` runs every frame with shipped market rate 1. If the
required frame-2 image confirms cycle 2, the resource service phases are the table below. Cycle
zero rolls each trend for a shipped duration of 8--23 services; no resource can exhaust that
countdown by frame 31, so the market performs no main-RNG trend roll in this interval. This
no-draw conclusion is conditional on the lawful initial/cycle image rather than an invented
frame-2 market record.

### Steps 13 and 14: Armies and Objects

`Armies::process_all` `0x006F3B00` has an exact dispatcher. The ordinary opening is expected to
have no valid Army, but the frame-2 registry must establish that fact.

`Objects::process_all` `0x0065DCE0` rotates only the Unit bands in `(frame + i) % 10` owner order.
Within owner zero it visits Scout, both Merchants and the four Citizens in `o` order. All Unit
owners complete before the fixed Build bands, where the Village precedes the Market.

Nature owner 9 precedes owner zero on frames 2--9, 11--19, 21--29 and 31. Owner one additionally
precedes owner zero on frames 11, 21 and 31. Owner zero is first only on frames 10, 20 and 30.
Consequently foreign/nature Unit work can move the shared RNG before any known actor on 27 of the
30 audited frames. A driver seeded with owner-zero objects alone cannot reproduce this interval.

For a normal live on-map Unit, `Unit::process` `0x00610BC0` can reach, in order:

- four conditional timer decrements at `0x00610BE6..0x00610C2C`;
- Scout `Caster::process_spells` `0x00739AD0`;
- `Unit::process_healing` `0x005E0670`;
- on-map mask stores;
- the cloak and attrition phase gates;
- `Unit::work` `0x0060D180`;
- `Guy::process` `0x005E0230` / `Guy::move` `0x005D9240` for every live Guy.

The setup-empty Scout spell queue is derivable, but the complete order/work path is not. Frame
zero can alter Scout and Merchant orders, paths, masks and object-search scratch.

Idle is stateful. `Unit::do_idle` `0x0060DCD0` temporarily changes the Unit mask, normally calls
`Unit::set_anim(0,0,1)` `0x00616F40`, stores its animation word, calls `check_idle`
`0x006032C0` and `think` `0x005F6E40`, then restores the temporary bit. Nested
`Guy::set_anim` `0x005DA300` depends on installed graphics data and may consume main RNG. The
frame-one idle/SetAnim modules expose exact capture boundaries; they do not execute the general
frame-2--31 Guy body.

Citizen think's type-50 path clears instance masks and ORs the Leader pending flag. The dual
mirror primitive exists, but general idle continuation is not mounted in the tick. Retail order
is therefore step-8 pending clear followed by up to four later idempotent Citizen ORs.

### Build-band Wall prefix

Retail `Build::process` `0x0061EDF0` begins with a call at `0x0061EE11` to `Wall::process`
`0x00640450`.

The original audit found an interleaving defect, not a missing helper reset: the old tick called
`BuildData::begin_frame_construction`, which already reproduced the unconditional helper
latch/reset, but it skipped the native Wall root around those writes. Current `origin/dev` mounts
the bounded Build-owned Wall prefix in native order and charges exactly one `Gap::BuildProcess`
per Build call:

1. periodic targeted decay and the `Wall::check_ever_seen(0)` boundary;
2. slow-slot seen toggle and active/inactive decision;
3. unconditional helper latch/reset;
4. territory boundary;
5. only after a complete Wall return, the existing bounded construction/queue adapter.

A reached child stops before all later stores, so the current fail-closed execution differs from
the full retail return chronology by design. At frame 16 the Village's periodic seen boundary
precedes and therefore blocks its slow/territory continuation. The active Market at frame 15
applies the slow seen toggle and helper writes before stopping at territory; at frame 31 it
applies helper writes before the same territory boundary. The remaining Build body after the
Wall return is still charged.

Retail phase facts for the two owner-zero Builds are:

- periodic targeted/seen: both Builds at frames 8, 16 and 24;
- Market `o=2001` slow32 + territory: frame 15;
- Village `o=2000` slow32 + territory: frame 16;
- Market territory only: frame 31.

`setup_2024_frame1_village_process` separately proves the detached frame-one Village prefix
through `BuildTypeData::is_gather_type` `0x00472BB0`; its next exact boundary is `0x0061F451`.
It does not supply frame-2 bytes or close the later Build tail.

### Step 15 and the tick tail

`Objects::inc_time` `0x0065DB70` uses fixed owner order 0--9, Units then Builds per owner:

1. unconditional `Nuke::do_damage` `0x0092BC80`;
2. Unit virtual `Unit::inc_time` `0x00610B40` and `Unit::execute_events` `0x0060EDC0`;
3. Build virtual `Wall::inc_time` `0x0063FB60`;
4. Ammo, Death, Farms and presentation-only Doober/Surf tails.

The twelve known Guys can reach `Guy::inc_time` `0x005D9E10` and nested
`Guy::set_anim` on every frame. Animation selection can consume 0--1 main-RNG draws per
activation and can recurse. Simulation graphics events can also create Ammo. Both active Builds
reach the unported 2,273-byte `Wall::inc_time` body every frame. This is an every-frame state/RNG
barrier even after step 14 is closed.

`Leader::process_event_frame` `0x006EC180` is called at step 19, but none of its signed
`frame % 50 == 0` bodies is due here. Step 20 increments the frame. Step 22 runs
`Roads::scan_and_kill_stray_roads` `0x008956A0`; live road tiles require renderer-owned
`RoadElementCandidate` facts. The seconds increment and `TurnControl::check_cannon_time` call at
`0x005924E7` occur only after pre-increment frames 14 and 29. No wildlife (`frame % 32`) or herd
(`frame % 64`) block is due before the excluded frame 32.

## Exact cadence table

`rN` denotes the serviced market resource index. “C/A/W32” denotes the same actor's cloak,
attrition and Unit-work phase-32 gates; the latter two are reached only along their live captured
paths.

| frame | additional scheduled work |
|---:|---|
| 2 | market cycle only |
| 3 | market r5 |
| 4 | market r4 |
| 5 | market r3; owner-one MakeList fast lane; grace timers |
| 6 | market r2 |
| 7 | market r1; replay serial 2; owner-one dirty gather only if captured dirty |
| 8 | market r0; owner-zero dirty gather; both Builds periodic seen boundaries |
| 9 | market cycle only; owner-one score |
| 10 | market cycle only; owner-zero score; grace; owner zero first; cloak Citizen `o6` |
| 11 | market r5; cloak Citizen `o5` |
| 12 | market r4; cloak Citizen `o4` |
| 13 | market r3; replay serial 3; cloak Citizen `o3` |
| 14 | market r2; cloak Merchant `o2`; post-increment seconds/cannon |
| 15 | market r1; grace; cloak Merchant `o1`; Market slow32/helper/territory boundary |
| 16 | market r0; both Builds periodic; cloak Scout `o0`; Village slow32/territory lies after its periodic boundary |
| 17 | market cycle only |
| 18 | market cycle only |
| 19 | market r5; replay serial 4; owner-one score |
| 20 | market r4; owner-zero score; grace; owner zero first |
| 21 | market r3 |
| 22 | market r2 |
| 23 | market r1 |
| 24 | market r0; both Builds periodic seen boundaries |
| 25 | market cycle only; replay serial 5; grace |
| 26 | market cycle only; C/A/W32 Citizen `o6` |
| 27 | market r5; C/A/W32 Citizen `o5` |
| 28 | market r4; C/A/W32 Citizen `o4` |
| 29 | market r3; owner-one score; C/A/W32 Citizen `o3`; post-increment seconds/cannon |
| 30 | market r2; owner-zero MakeList fast lane + score; grace; owner zero first; C/A/W32 Merchant `o2` |
| 31 | market r1; replay serial 6; C/A/W32 Merchant `o1`; Market helper/territory boundary |

The next phase-32 actor is Scout `o0` at frame 32, coincident with the first wildlife cadence and
outside this audit.

The serial-2--6 packages at frames 7, 13, 19, 25 and 31 contain lockstep/checksum/session traffic,
not simulation commands. The next classified simulation command after serial 1 is serial 64 at
frame 379.

## Attrition owner and residual

The detached phase-32 transaction owns the exact caller/child/caller stores for frames 26--32:

```text
0x006115EA  test signed (frame + o) % 32
0x00611609  clear Unit unit_masks2 bit 0x00040000
0x005E11A6  clear Unit masks 0x00400080
0x005E11B2  attrition = 0
0x005E12A5  neutral/zero-neutral-attrition return
0x005E12C5  same-owner WData return
0x00611617..0x00611626  clear caller mask bits 0..1
```

It is deliberately not tick-mounted. It accepts only same-owner terrain or signed neutral
terrain with matching zero `neutral_attrition` mirrors, and requires an immediate pre-actor
whole-Sim, terrain, WData-cell, Handle and Unit capture. Foreign terrain, Scenario
attrition-free registries, supply, damage and `Leader::meet` remain red. Earlier Unit prelude,
cloak, actors and frames still have to reach the transaction lawfully.

## Earliest ordered blockers

Before any per-frame analysis, the absent eleven-image bundle is the global blocker. Given a
lawful frame-2 image, the ordered residual is:

1. frame 5 may first stop in step 11 at owner one's captured MakeList/can-pay child; frame 30 has
   the corresponding owner-zero boundary;
2. frame 8 requires the exact owner-zero gather census at step 8;
3. on ordinary frames the first systematic missing execution is step-14 Unit prelude,
   order/work and Guy chronology, with nature/foreign actors possibly earlier than owner zero;
4. the Build Wall prefix now fails closed at its first external child, while the later Build body
   remains charged;
5. step-15 Guy animation/events and Build animation form the next unconditional barrier;
6. road renderer candidates are the late-tick World barrier.

The detached attrition, Scout-caster, idle/SetAnim and frame-one Village authorities are useful
atomic seams. None bridges over an earlier red actor or authorizes the state between frame 2 and
its local entry.

## Minimal exact continuation

1. Produce and admit the full eleven-image supported-retail capture bundle.
2. Mount its frame-2 image as the sole entry to a golden frames-2--31 driver, retaining the full
   owner-one/nature object bands and generated World.
3. Complete one atomic Unit/Guy execution owner: timer prelude, healing, cloak, captured order,
   idle/think, `Guy::process`, full animation resources and shared RNG.
4. Reuse that Guy/animation owner for step 15 and bind simulation GraphicEvents.
5. Continue the Build body from the bounded Wall return, preserving the current fail-closed
   child ordering.
6. Bind road candidates and any remaining collision/World cursor inputs.
7. Use immediate native captures at cadence boundaries as validation artifacts only, never as
   substitutes for replay state or recorded-checksum fitting.

Until those inputs exist, “frames 2--31 replay compatible” would overstate both state ownership
and RNG chronology. The exact achievement here is the schedule, the ordered authority graph and
the identification of the first lawful continuation points.
