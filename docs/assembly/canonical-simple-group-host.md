# Canonical simple-Group package host

Status: UNITMASK opcode 32 is mounted through `Sim::process_simple_group_package` and covered by
DoNSave v13 resume tests. No command-table, general packet router, or closure status changes are
made.

## First executable row

`canonical_simple_group_host.rs` admits exactly `[GroupCommand][UnitmaskCommand]`. It never
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

## Integration and save boundary

The module export and `Sim` sibling now construct the same present-player map as Group+Move and
call prepare/commit synchronously against the fixed owners. The integration test executes the
exact retail packet above, saves the resulting Groups/cache/Unit/order/path image, reloads it,
reinstalls only the revision-bound authority, executes the observed empty-Group cached-selection
wire, and proves complete after-image, Groups checksum, serialized bytes, and RNG equality.

The remaining production-routing tranche is bounded:

1. teach the replay/package router to invoke this strict host only for an exact `[0,32]` pair;
2. compare the first executable packet's Groups/Unit channels to the retail recording; and
3. only after that evidence update closure reporting for opcode 32.

The other eight audited simple actions remain red at this host. HALT and STOP_SPELL can reuse the
same Unit image once scenario ignore-orders and their additional action/type fields are bound.
SET_TRANSPORT needs Leader flags; FOLLOW needs its typed payload; STANCE spans Unit and Build;
DISBAND reaches the nested Build production queue; BUILDMASK requires the canonical Build-band
selector and feedback receipt. BEGIN has no corpus occurrence and is not used to claim execution.

## Gates

```sh
cargo test -p don-sim --test canonical_simple_group_host
cargo test -p don-replay --test retail_simple_group_package_fixtures \
  finished_replay_binds_five_group_unitmask_packets_and_one_strict_fixture
cargo test -p don-replay --test retail_simple_group_package_fixtures \
  census_strict_group_unitmask_packets -- --ignored --nocapture
```
