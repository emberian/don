# Canonical simple-Group package host

Status: UNITMASK opcode 32, the ordinary-Unit arm of STOP_SPELL opcode 29, HALT opcode 12, and
the ordinary-Unit arm of SET_TRANSPORT opcode 14 are mounted through
`Sim::process_simple_group_package` and covered by DoNSave v13 resume tests. HALT and
SET_TRANSPORT also cross one canonical `Sim::do_frame`. No command-table, general packet router,
or closure status changes are made. STOP_SPELL's special type 61/62/400 graphics tail remains a
typed whole-package refusal.

## First executable row

`canonical_simple_group_host.rs` admits exactly `[GroupCommand]` followed by UNITMASK,
STOP_SPELL, HALT, or SET_TRANSPORT. It never
constructs `command::Bridge` and never reads or copies the Bridge-owned `command::Groups`.
Opcode 0 reuses the fixed 512-slot `groups_guys::Groups` selector, play-keyed receive cache,
UID/Handle revalidation, allocator, old-Group removal, and Unit backlinks already used by the
canonical Group+Move host.

The action then consumes `plan_action_unitmask`, the recovered whole body at
`0x006FCB90..0x006FCD24`. Its detached after-images preserve:

- retail's loop-carried set-to-clear decision;
- mask `0x40000`'s forced-clear behavior;
- mask `0x100`'s true-plane skip;
- the ordered Unit mask write, Object flag `0x10`, mask `0x04000000` clear, path-anchor clear,
  order close, partial-path clear, and action-endpoint update; and
- the unread second wire dword, retained verbatim in the request and planner.

The prepare image binds the player-to-owner map, Game frame and RNG state, command selection
cache, complete fixed Groups pool, revision/digest/member authority, generational Unit identity,
Unit flags/masks/position/facing/action endpoint, concrete `OrderList`, and `PathStack`. Commit
revalidates every surface before assigning any after-image. UNITMASK consumes zero RNG draws.

The reusable selector now names this admission `SimpleUnitState`. It requires a live
generational Unit and exact authority-member binding but deliberately does not require
`can_install_order`, which UNITMASK does not read. The existing `EconomyOrderInstall` and
`MoveNear` predicates remain unchanged.

## Second executable row

STOP_SPELL consumes the already-recovered complete `plan_stop_spell` body at
`Group::action_stop_spell` `0x006FD7A0`. For every selected, on-map Unit whose current order is
CAST_SPELL, the detached after-image clears mask `0x04000000`, the complete path, the complete
order list, the action endpoint, and `UnitData::spell_time` (`+0x98`). The unit type vector,
spell clock, order kind/payload, mask, path, position/facing, selection authority, player map,
frame, and RNG are all revalidated before the same assignment-only commit used by UNITMASK.
The CAST executor is not invoked or required: this receiver cancels and closes its current
order.

Retail types 61, 62, and 400 additionally dirty `Objects +0x22C` and call
`Unit::update_gpiece`. Those global/graphics owners are not present in canonical Sim, so a
currently casting member of one of those types returns
`MissingStopSpellGpieceAuthority` before any Group, cache, or Unit publication. Merely selecting
such a type is harmless when it is not currently casting. The canonical Sim currently models
the ordinary multiplayer/product `ScenarioData::ignore_orders == 0` state; scenario prune state
is not invented by this host.

## Third executable row

HALT consumes the complete `plan_action_halt` body at `Group::action_halt`
`0x0070D0C0..0x0070D36D`. Opcode 12 is a one-byte command whose handler always supplies action
flags zero. The `flag_4_veto`, `special`, and `spy` predicates are therefore unreachable rather
than defaulted. The existing selection authority supplies the reached on-map, plane, domain,
and Unit-flags facts; the canonical `Order` supplies the ENTER/EXIT discriminator. A malformed
SPECIAL_ANIM without its typed payload refuses before publication.

For every admitted ordinary member the detached transaction clears mask bits `0x04000000` and
`0x100`, path state, the entire order list, and the action endpoint. It also applies
`action_begin` (`disband = 0`) and resets Group form. Entering/exiting Units and airborne planes
without the `unit_flags & 0x20` exception remain selected but are not halted, exactly as retail.
No order payload is installed, no RNG draw is consumed, and scenario ignore-orders remains the
same ordinary-product no-op boundary documented for STOP_SPELL.

## Fourth executable row

SET_TRANSPORT consumes the complete ordinary-Unit body of `Group::action_set_transport`
`0x007024B0..0x00702615`. Its fixed five-byte action wire retains the signed `flag` dword. The
owner's canonical `LeaderData::leader_flags` supplies retail's exact priority ladder:
`0x100 => 3`, else `0x200 => 2`, else `0x400 => 1`, else zero. A selected member receives mask
bit `0x00800000` exactly when that level and the wire flag are both nonzero; flag zero clears it.
`action_begin` also clears the fixed Group's `disband` field.

The only additional type fact is the exact Handle-bound result of
`UnitData::can_ever_transport()`. It is installed in `SimpleGroupActionAuthority` with a revision,
composition digest, and complete member vector. The host does not infer the sea-Unit arm from
movement facts: retail also reads carry capacity and ability 0x15f there. Missing capability,
changed authority, changed Leader flags, or changed Unit state refuses before Group/cache/Unit
publication. The adapter is reinstalled after load and is not a second persistent gameplay
owner. The action changes no orders or paths and consumes zero RNG draws.

## Retail packet evidence

The artifact-backed replay test freezes the shipped recording
`Playback - 2026.08.11 11'44'38 (Tue).rcx`, SHA-256
`558e0cd53dbed4f820e8757c0327a58384d433e5beb8e9eeef5d64df4d67bd54`. It contains five
Group+UNITMASK pairs. Four packages are exactly the strict two-command shape; the executable
fixture used by the host is package index 52, turn 53, frame 50:

```text
0001000100                 Group(owner=0, object=1)
2000010000ffffffff         Unitmask(mask=0x100, set=-1)
```

The explicit full-corpus census currently sees 302 pairs. The dominant action wire is the
mask-`0x100` form above. Two cached-selection packages use:

```text
000002                     Group(owner=2, empty selection/cache reuse)
200000200001000000         Unitmask(mask=0x00200000, set=1)
```

The host tests execute both shapes. Prefix/suffix packages remain typed refusals; they are not
silently stripped to manufacture a strict pair.

STOP_SPELL is bound to
`multi/Playback___2017.07.20_20_46_23__Thu_.rcx`, SHA-256
`2c962b3607348784caec4ca0f95a1e6a43b28b2175c2db0dce8afb48ae425741`. At package index 9,162,
turn 9,163, play 1, frame 135,624, it carries one explicit packet followed by two persistent-cache
forms:

```text
00030207003800e300         Group(owner=2, objects=7,56,227)
1d                       StopSpell
000002 1d                Group(owner=2, cache reuse), StopSpell
000002 1d                Group(owner=2, cache reuse), StopSpell
```

The full-corpus census sees 113 strict Group+STOP_SPELL pairs.

HALT is bound to `multi/Playback___2024.03.23_21_16_13__Sat_.rcx`, SHA-256
`82bfb0898209d40afd887b1a487f565446cdb154d1192435e41a43137809355a`. The executable witness is
package index 8,864, turn 8,865, play 1, frame 35,465: Group owner 0 with 24 explicit Unit ids,
followed by the one-byte `0c` action. Package index 9,574, turn 9,575, frame 38,305 supplies the
persistent-cache form:

```text
001800550074004200440045005e006900710075007d007c00800040004b005d0001000300050006000a000c005c006e008700
0c                       Halt
000000 0c                Group(owner=0, cache reuse), Halt
```

The full-corpus census sees 69 strict Group+HALT pairs.

SET_TRANSPORT is bound to
`multi/Playback___2024.02.23_20_49_35__Fri_.rcx`, SHA-256
`1690431a5ef19b38a3425d3dd7311e8e83ca0d27c56fabe49d776a9f1421b251`. Package index 118,
turn 119, play 1, frame 709 carries the only explicit selection in the five-packet corpus:

```text
0001010000               Group(owner=1, object=0)
0e01000000               SetTransport(flag=1)
```

The other four strict pairs carry `flag=0`; one uses empty Group owner 1 (`000001`) and three use
empty Group owner 0 (`000000`), exercising the play-keyed persistent selection cache. The
full-corpus census proves exactly five strict Group+SET_TRANSPORT pairs across four retail
recordings.

## Integration and save boundary

The module export and `Sim` sibling now construct the same present-player map as Group+Move and
call prepare/commit synchronously against the fixed owners. The integration test executes the
exact retail packet above, saves the resulting Groups/cache/Unit/order/path image, reloads it,
reinstalls only the revision-bound authority, executes the observed empty-Group cached-selection
wire, and proves complete after-image, Groups checksum, serialized bytes, and RNG equality.
The HALT resume test then executes one full canonical frame on both the direct and reloaded Sims
and proves the stopped orders remain empty and the serialized states remain identical. The
SET_TRANSPORT resume test executes the explicit flag-one packet, saves/loads, reinstalls both
external authorities, executes the observed empty-Group flag-zero packet, advances a frame, and
proves the Unit mask, Groups checksum, RNG state, and serialized bytes remain identical.

The remaining production-routing tranche is bounded:

1. teach the replay/package router to invoke this strict host only for exact admitted pairs;
2. compare the first executable packet's Groups/Unit channels to the retail recording; and
3. only after that evidence update closure reporting for opcode 32.

The other five audited simple actions remain red at this host.
FOLLOW needs its typed payload; STANCE spans Unit and Build;
DISBAND reaches the nested Build production queue; BUILDMASK requires the canonical Build-band
selector and feedback receipt. BEGIN has no corpus occurrence and is not used to claim execution.

## Gates

```sh
cargo test -p don-sim --test canonical_simple_group_host
cargo test -p don-replay --test retail_simple_group_package_fixtures \
  finished_replay_binds_five_group_unitmask_packets_and_one_strict_fixture
cargo test -p don-replay --test retail_simple_group_package_fixtures \
  retail_replay_binds_stop_spell_explicit_and_cached_wires
cargo test -p don-replay --test retail_simple_group_package_fixtures \
  retail_replay_binds_halt_explicit_and_persistent_cache_wires
cargo test -p don-replay --test retail_simple_group_package_fixtures \
  retail_replay_binds_set_transport_explicit_wire
cargo test -p don-replay --test retail_simple_group_package_fixtures \
  census_strict_group_unitmask_packets -- --ignored --nocapture
cargo test -p don-replay --test retail_simple_group_package_fixtures \
  census_strict_group_stop_spell_packets -- --ignored --nocapture
cargo test -p don-replay --test retail_simple_group_package_fixtures \
  census_strict_group_halt_packets -- --ignored --nocapture
cargo test -p don-replay --test retail_simple_group_package_fixtures \
  census_strict_group_set_transport_packets -- --ignored --nocapture
```
