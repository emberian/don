# Dynamic replay `Groups` frontier — checksum channel 5

Lane: `replay-groups-dynamic`. This is the first producer step after the frozen
`Groups::clear` image in [`groups-initial-state.md`](groups-initial-state.md). It does
not replace that derivation, the save-game work in
[`savegame-groups.md`](../derivation/savegame-groups.md), or any of the mounted
`Group::action_*` planners. It joins the already-derived pieces at the replay boundary.

## The authoritative path

Recorded opcode 0 bytes now enter `don_sim::command::Bridge::process_one`. The bridge's
`command::Groups` pool remains the sole mutation owner. The replay adapter borrows that
pool and projects each live `GroupData` through `groups_channel::groups_checksum` in
retail order:

1. all 512 unconditional 72-byte group headers in slot order;
2. for each nonempty group, `list`, `off_x`, `off_y`, `curr_x`, `curr_y`, then `angles`,
   each using the live `num` prefix and preserving member order;
3. the eight `last_group` entries.

There is no replay-side copy of `GroupData`. `GroupMemberFact` is instead an explicit
admission record for the object columns opcode 0 reads: owner, object index, UID,
liveness, unit/building class, and role. Missing or out-of-range facts reject the
command before either the bridge or object table changes.

That distinction matters for empty opcode-0 selections. Retail reuses its previous
`(object, UID)` list and drops a recycled object whose current UID no longer agrees.
The producer therefore preflights the prior objects and lets the bridge's UID cache make
the decision; it does not turn the cached list into a set.

## The 8-owner correction

`ObjectTable` and the broader command `Fleet` correctly expose ten owner bands. Retail
`Groups::clear`, however, creates exactly eight bands of 64 slots, with cursors
`{0, 64, 128, 192, 256, 320, 384, 448}`. The command-side group pool formerly inherited
the ten-owner width and created 640 slots. It now has a separate `GROUP_OWNER_SLOTS = 8`
constant, the retail clear values (`army = form = -1`), bounds-safe cursors, and rejects
opcode 0 owners 8 and 9. Tests pin both sides of the separation so widening object
ownership cannot silently widen checksum channel 5 again.

## Seven-recording boundary

The human-only checksum corpus has the following first dynamic packages. The first
changed channel value is always exactly two recorded turns after the first nonempty
opcode 0. That is measured scheduling evidence, not a hard-coded execution policy.

| recording | opcode-0 issue | owner / members in wire order | Sim tail | first changed channel |
|---|---:|---|---:|---|
| 2024.02.23 20:49 | 64 | 0 / `3, 4, 5, 6` | 7 `MoveTo` | turn 66 `0x22a5074d` |
| 2019.03.24 | 18 | 1 / `1, 2, 3, 4` | 25 `Build` | turn 20 `0x624df46d` |
| 2024.03.29 21:52 | 15 | 0 / `1, 2` | 7 `MoveTo` | turn 17 `0x1feaf88c` |
| 2018.11.17 | 14 | 0 / `4` | 25 `Build` | turn 16 `0x1118f44b` |
| 2020.02.21 | 14 | 1 / `1, 2, 3, 4` | 25 `Build` | turn 16 `0x11d9f454` |
| 2018.12.01 | 8 | 2 / `1, 2`; 0 / `0` | 25 `Build`; 32 `UnitMask` | turn 10 `0x123cf3cf` |
| 2020.02.08 | 7 | 0 / `2000` | 18 `CityGather` | turn 9 `0xdffdf4f4` |

Every one of those opcode-0 commands shares a package with another Sim opcode. The
Groups-only package API scans the whole package before mutation and returns a typed
`UnsupportedSimTail` for all seven. It deliberately does not apply the selection and
pretend that the resulting checksum is the retail post-action state. Non-Sim commands
are checksum-inert at this boundary and are reported as ignored; multiple opcode-0 rows
in one package are also rejected rather than reordered.

The integration test freezes that frontier against the live corpus: seven qualifying
recordings, a two-turn first-mutation distance in each, the presence of a Sim tail, and
unchanged producer state after refusal. Separate synthetic transactions prove exact
slot selection, member order sensitivity, byte count, missing-fact rollback, and
UID-guarded empty reselection.

## What remains before an exact recorded post-command match

This tranche produces exact `check_groups` for admitted opcode-0-only transactions. It
does **not** claim a post-command corpus match yet. The seven starter packages require
two further authoritative inputs:

- starter object snapshots with retail owner/index/UID/class/role values at the package's
  execution frame;
- the package tail's complete effect on the selected group, including any normalize,
  kill, formation, or movement writes made before/during the mounted action path.

Those values must come from the world/setup owners and the existing command/action
bridge, not from fitting the target checksum. Once they are available, the same producer
can admit the full package and compare its projected bridge pool directly with the
recorded channel value. Until then it fails closed and preserves the last exact state.
