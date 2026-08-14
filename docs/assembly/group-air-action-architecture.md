# Air group-action transaction architecture

Status: architecture/executable contract only; no closure row changes.

This tranche asks whether one canonical host can close a useful group of the red air
actions/opcodes without copying more state into `command::Bridge`.  The answer is yes, but
the coherent first group is deliberately narrow: `launch_patrol` plus opcode 11 and
`scramble` plus opcode 36.  Those four rows share the same containment walk, candidate
predicates, and two installer branches.  The stateless contract is
`systems/air_group_action_transaction.rs`; a real `tick::Sim` adapter remains required.

## Retail image and symbols used

| input | SHA-256 |
|---|---|
| `ron-bin/riseofnations.exe` | `30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079` |
| `ron-bin/sbl/rise.pdb` | `334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5` |

The PDB identifies the retail executable with GUID
`51D4F219-61C6-4F84-9D5B-C3361B0D291F`, age 1.  Full-body Capstone decoding against that
image produced the following ownership map.

| receiver | VA | PDB bytes | decoded evidence / principal children |
|---|---:|---:|---|
| `Group::action_flight` | `0x006FB260` | 3,398 | exact 901 instructions; delegates flight, launch-flight, guard, attack and installs STRAFE |
| `Group::action_launch_flight` | `0x006FBFB0` | 2,544 | 2,541 decoded plus 3 trailing alignment bytes; delegates flight and reads containment/busy/mana/age |
| `Group::action_guard` | `0x006FCD30` | 2,012 | exact 561 instructions; `update_guard_order`, `add_guard_order`, `finish_insert`, halt/form/membership |
| `Group::action_air_patrol` | `0x007029D0` | 1,763 | exact 468 instructions; `add_air_patrol_order`, busy/mana/containment, move-to delegate |
| `Group::action_patrol` | `0x007030C0` | 1,215 | exact 367 instructions; patrol/move installers and air-patrol delegate |
| `Group::action_launch_patrol` | `0x00703580` | 2,043 | exact 581 instructions; AIR_PATROL or inlined MOVE_TO over `inside_down` chains |
| `Group::action_air_attack_ground` | `0x00703D80` | 1,939 | exact 534 instructions; air-attack-ground installer, validity/ocean/mana/diplomacy gates |
| `Group::action_attack_ground` | `0x00704520` | 1,133 | exact 318 instructions; ground installer and air delegate |
| `Group::action_scramble` | `0x007111C0` | 894 | exact 250 instructions; same AIR_PATROL/inlined MOVE_TO branches as launch-patrol |

The direct command-handler calls were decoded too, rather than inferred from opcode names:

| opcode | handler | packet bytes | direct action |
|---:|---|---:|---|
| 9 | `process_attack_ground` | 10 | `action_attack_ground` |
| 10 | `process_patrol` | 10 | `action_patrol` |
| 11 | `process_launch_patrol` | 25 | `action_launch_patrol` |
| 28 | `process_flight` | 25 | `action_flight` |
| 31 | `process_guard` | 13 | `action_guard` |
| 36 | `process_scramble` | 1 | `action_scramble` |

`schema/command-structs.txt` independently confirms the layouts.  Opcode 11 is the opcode
byte followed by six unaligned little-endian dwords: `x`, `y`, `QueuePos`, `force_all`,
`bombers_only`, `fighters_only`.  Opcode 36 has no payload.  The transaction decoder rejects
both truncation and trailing bytes.

## Finished retail replay evidence

The finished solo recording
`ron-data/replays/Playback - 2026.08.11 11'44'38 (Tue).rcx` is ignored game content and is
not committed.  Its SHA-256 is
`558e0cd53dbed4f820e8757c0327a58384d433e5beb8e9eeef5d64df4d67bd54` (359,893 compressed
bytes).  The independent replay decoder found the command stream at decompressed offset
1,025,163 and tiled all 60,402 packages into 68,811 commands with zero undecoded packages.

The air-adjacent census is:

| opcode | commands | same-package preceding Group | raw Group sizes |
|---:|---:|---:|---|
| `0x24` Scramble | 84 | 84 | empty 81; one 1; two 1; three 1 |
| `0x1C` Flight | 29 | 29 | empty 21; explicit 1/7/12/14/20/22/45 |
| `0x09` AttackGround | 4 | 4 | empty 3; one 1 |
| `0x04` Attack, comparison row | 56 | 56 | empty 21; explicit up to 83 |

There are 675 Group commands in the recording.  Across those four action kinds, all 173
actions have a Group earlier in the same package and none is unpaired.  In every case the
action immediately follows the nearest Group command; five packages contain two
Group/action pairs.  No opcode 11 LaunchPatrol or opcode 31 Guard occurs in this specimen, so
the replay adds strong opcode-36 evidence but cannot be used to promote opcode 11.

Scramble's three explicit Group packets select only Build-band addresses:

| game frame / package serial | Group packet | Scramble packet | requested objects |
|---|---|---|---|
| 47,706 / 48,065 | `000100df07` | `24` | 2015 |
| 51,868 / 52,254 | `0002002a082b08` | `24` | 2090, 2091 |
| 57,968 / 58,402 | `0003002a082b086308` | `24` | 2090, 2091, 2147 |

The common form is `000000 24`: opcode 0 with `num == 0`, followed by Scramble.  Retail's
empty Group packet means “re-select this player's cached `(o, uid)` pairs,” not “select zero
objects.”  Tracking the last non-empty raw Group packet gives an answered cache for all 84
scrambles: effective raw cache sizes 1 in 17 commands, 2 in 41, and 3 in 26.  All 177 cached
member references are Build-band addresses.  That offline tracking is useful census evidence,
but it cannot prove the UID/liveness-filtered canonical group because the replay packets do not
carry those object facts.

The transaction contract therefore now decodes opcode 0 exactly (`3 + 2*num`), binds the
nearest Group and air command to their package frame/serial/play and command indices, and
requires a `CanonicalGroupPacketReceipt` matching the resulting group key, revision and full
walk image.  Empty Group packets additionally require a selection-cache revision.  A bare
`000000 24` fixture without that canonical receipt fails closed rather than being treated as an
empty no-op or borrowing the bridge's shadow cache.

The fresh retail save
`ron-data/savegames/new save game 2026.08.11 15'42'57 (Tue).SVX` has SHA-256
`161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7`, is 337,744 gzip
bytes / 2,923,161 plain bytes, and parses as `RoNSave` version 16 with build string
`00.2024.06.2000`.  This establishes a contemporary retail save specimen, not an air-order
round trip: no AIR_PATROL payload was extracted from it, so the payload/save blocker below
remains unchanged.

## Why the first cohort is four rows

`systems/air_launch_receivers.rs` already owns complete, fail-closed plans for both action
bodies.  Each selected group member is a container candidate, not an aircraft candidate.
Retail follows that member's `ObjectData::inside_down/inside_down_who` chain and evaluates
each contained object in this exact order:

1. `is_unit`;
2. type `domain == 2`;
3. `UnitData::is_busy == 0`;
4. `ObjectTypeData::obj_masks & 0x08000000 == 0`;
5. the action-specific mana/type/scoring gates.

The accepted object receives either AIR_PATROL or, when `unit_flags & 0x20` is set, the
inlined MOVE_TO launch sequence.  Thus one host closes two group actions and their two wire
handlers without pretending that nearby but materially different actions are done.

The neighboring rows are sequenced later for concrete reasons:

- `flight` can install STRAFE, whose production executor is not wired, and has a much larger
  delegate/fact surface;
- `launch_flight` is not itself on the wire and depends on flight;
- guard needs QUEUE_FIRST/`finish_insert`, RNG and CAST/movement effects;
- ordinary patrol needs the canonical movement transaction plus GROUP_PATROL tick work;
- attack-ground needs the combat, terrain/world-validity and air-physics hosts.

## One owner, one transaction

The production transaction must be owned by `tick::Sim`, because only `Sim` can validate
the checksum-visible owners together:

- `groups_guys::Groups` for the addressed `(owner, slot, GroupData::id)`;
- `World::object_bands` for generational Unit/Build/Wall identities;
- the selected member's Unit or Build `inside_down` head and every subsequent link;
- `World` order queues and Unit masks/action/path columns;
- synchronized scenario and type authority revisions.

`command::Bridge` may decode the packet and submit the request.  It must not mutate its own
`command::Groups`, `ObjectTable`, or an air-order sidecar and later publish that shadow as if
it were canonical.

The request carries:

- the exact packet and decoded command;
- the group key, revision and complete `GroupData::walk` byte image;
- stable row-independent identities for the ordered live member list;
- the expected scenario, type and world-order authority revisions.

Preflight is immutable.  It captures the same group image again, resolves all containment
links through the three retail bands, records a stable identity for every link, captures all
planner facts, and snapshots every eventual install target's identity, order revision/digest,
unit mask, action revision and path revision.  Only then is the existing whole-body planner
run.  Its emitted targets must match the captured target before-images one-for-one in retail
order.

Commit takes one checkpoint covering every writable owner, revalidates all preflight images,
and installs the whole plan.  A stale group/member/type/scenario/order image, a host rejection,
or malformed commit evidence restores the checkpoint.  The receipt records the request,
preflight image, recomputed plan, before/after state digests and exact committed identities and
order kinds.  A receipt validates only if re-running the decoder and planner produces the same
result.

## Typed production blockers

The contract does not turn absent authority into ordinary zero values.  These remain named
blockers:

| blocker | current reason |
|---|---|
| `ignore_orders` unavailable/armed | canonical `ScenarioDataState` does not yet own the scalar and same-transaction prune receipt |
| air type authority unavailable | production has partial flags/masks, but no authoritative complete domain plus non-strict BIPLANE/BOMBER/HELICOPTER relation projection |
| packet-to-Sim route unavailable | opcode handling still terminates in the bridge's duplicate group/object state |
| AIR_PATROL order tag unavailable | `World::Order` cannot yet retain the executable patrol payload |
| dynamic patrol arrays unavailable | patrol waypoint arrays must be lossless and stream-bounded, never silently capped to an invented fixed length |
| AIR_PATROL `unit_work` unavailable | `Sim::unit_work` does not dispatch order 17 into the recovered executor |
| save/reload/resume unavailable | canonical payload, waypoint/walk state and resumed tick effect must round-trip |

Payload tags are owned by the single DoNSave v12 extension envelope, not this lane.  The
coordinated proposal is: 0 None, 1 Move, 2 Gather, 3 CastSpell, 4 TradeRoute, 5 Guard,
6 AirPatrol, 7 GroupPatrol, 8 Strafe, 9 AttackGround, 10 AirAttackGround, each with an
independent payload version.  No tag is serialized by this transaction contract.

## Required production proof before closure

The four rows remain red until one integration test demonstrates all of the following through
the real owners:

1. exact opcode 11 and 36 packets enter the production decoder and address a canonical group;
2. selected Build- and Unit-band members resolve their full containment chain by stable
   identity;
3. the transaction commits the expected AIR_PATROL and helicopter MOVE_TO branches atomically;
4. the installed order executes a real `Sim::unit_work` tick with an observable canonical
   waypoint/movement/action/path effect;
5. save, reload and resume produce the same queue/payload/group/world digest and next-tick
   result;
6. the command and tick receipts recompute;
7. mutations of packet bytes, group id/revision/walk bytes/member order, containment
   generation, scenario/type/world revision, target uid/order revision, payload tag/version or
   dynamic length all reject with zero canonical changes and zero RNG consumption.

The exclusive integration test currently proves the stateless portions: exact decoding,
group-walk and stable-identity binding, both planner variants, every typed capability refusal,
receipt recomputation, stale revalidation, partial-write rollback, malformed-evidence rollback,
and the answered empty-containment no-effect branch.  It does not stand in for the production
path, tick or save/resume proof above.

## Cycle 17: bounded Patrol + fresh Flight

The one Unit-to-Build Flight package excluded by the Cycle 16 shell census is now an exact
combined transaction, not a claim that ordinary Patrol or fresh Flight is complete. In
`Playback___2019.03.24_11_56_19__Sun_.rcx` (SHA-256
`dab1c282556642300a5bc153f1f432f417fa039b265b4d72cb5876dd643ec055`), turn index 10,869,
turn 10,870, play 2, frame 65,215 has opcodes `[0,10,0,28,57,74,72]`. The two action packets
are `0acc0c01004534000001` (Patrol to 68,812 / 13,381, QueueLast) and
`1c2c080000010000000000000000000000000000000a000000` (ATTACK Flight to owner 1 Build
2092, no modifiers).

The admitted Patrol cone follows `Group::action_patrol` `0x007030C0` into
`Group::action_air_patrol` `0x007029D0`: a cached nonempty all-Unit group, active movable
true planes in domain 2, no helicopter/busy/missile/fuel boundary, and a coherent current
STRAFE on every actor. It clamps the destination, clears Group form, replaces every queue
with the exact group AIR_PATROL payload while preserving the STRAFE AirOrder home pair, and
clears each partial path. The immediately adjacent Flight follows the fresh-install call at
`0x006FBE8D..0x006FBEB3` and replaces those exact AIR_PATROL after-images with mandatory,
group STRAFE payloads targeting the live generational Build. Its range decision is an
explicit Handle-bound, target-address authority fact; absence stays red.

Preparation runs both pairs against detached World/Groups/path/cache owners. Commit checks
the command image, map dimensions, all owner snapshots and authorities, then recomputes and
publishes both pairs or restores the full checkpoint. The real seven-command packet fixture,
save/load byte identity, changed-map stale rollback, standalone Patrol rejection, and absent
fresh-range authority rejection are executable gates. The Unit-to-Build wire/shell census is
therefore 942/942 shell-admissible. Standalone Patrol, ground/mixed Patrol, QueueNew, and
general AIR_PATROL-to-Flight remain deliberately fail-closed.

## Cycle 18: Build-to-Unit Flight no-action target

The all-Airbase no-action Flight cone is target-class independent after the target's live
identity has been resolved. Cycle 18 exercises its Unit target rather than projecting it
through the earlier Build fixture. The shipped solo replay (SHA-256
`558e0cd53dbed4f820e8757c0327a58384d433e5beb8e9eeef5d64df4d67bd54`) supplies the exact
witness at turn index 39,549, turn 39,550, play 0, frame 39,338:
`0001001008` explicitly selects owner 0 Build 2064, and
`1c9f000000030000000000000000000000000000000a000000` issues unmodified ATTACK Flight
against owner 3 Unit 159.

The canonical host resolves that target through the Unit band and binds its Handle,
generation, UID, active flag, and position. It then walks the selected Airbase's complete
containment chain. With no type-315 Nuclear Missile child, retail reaches no order-changing
Flight tail: only the opcode-0 Group/cache selection publishes. A changed Unit UID between
prepare and commit rejects before that selection, and the applied no-action result round-trips
through save/load byte-identically with unchanged target orders, Builds, RNG, and Flight order
receipts.

The full replay corpus contains 301 Build-to-Unit ATTACK/no-modifier Flight pairs across 17
files: 29 explicit and 272 cached selections, with effective group sizes
`{1:41, 2:21, 3:48, 4:147, 6:37, 7:6, 8:1}`. All 301 packages use only the bounded AIR shell
grammar. This closes the measured wire/shell and exact empty-containment execution cone; it
does not authorize non-Airbase selections, incomplete containment, Nuclear Missile children,
modifiers, or any general Flight mutation.
