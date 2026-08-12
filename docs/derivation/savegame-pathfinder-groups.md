# Retail save boundary: PathFinder direct state and Groups array

Status: **complete exact direct block plus dynamic Array<Group>**. This lane
begins with the caller's 108-byte `PathFinder::walk_data` projection, then
consumes the complete `Array<Group>` allocation history and every dynamic
Group row. It stops before the following caller tag for the remaining Groups
state. It is an exclusive parser/test/doc tranche and does not touch shared
Rust.

## Fresh-SVX splice

The fresh SVX compressed and decompressed SHA-256 values are
`161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7` and
`fa31f89f9bf435e28847e9e54d62cfe712e6bafc6cedb9ab98c8ab6730adb7c8`.
The exact combined stage is:

| range | owner | fresh value | SHA-256 |
|---|---|---:|---|
| `0x27dfe..0x27e6a` | PathFinder direct block | 108 zero bytes | `77133f431d5e12dd850002c0d3d4e0fecbe3a7a699d604dc8c5eae9976e1d260` |
| `0x27e6a..0x27e6e` | `Array<Group>.length` | 0 | `df3f619804a92fdb4057192dc43dd748ea778adc52bc498ce80524c014b81119` |
| next at `0x27e6e` | caller tag, StringTable[2920] | excluded | — |

The exact 112-byte combined SHA-256 is
`b5fdab78d8947eacc864bfeecb4d2100780e5afe1cd8efafb124887913ac49fa`.
The test chains every landed helper through OptionInfo to reach `0x27dfe`,
then proves that mutating `0x27e6e` cannot affect this tranche. SVX seed
`0x014810ac` and RCX seed `0x007f93e0` remain distinct; RCX SHA-256 is
`558e0cd53dbed4f820e8757c0327a58384d433e5beb8e9eeef5d64df4d67bd54`.

## Exact PathFinder projection

Caller `0x005a2c17` directly walks PE globals
`0x00e85e98..0x00e85f04`. The PDB public symbol places the global
`PathFinder pathfinder` at `0x00e85e40`, so this is exactly object offsets
`+88..+196`. Independent `PathFinder::walk_data` `0x00689cb0` walks the same
addresses. PDB resolves the 108 bytes as 27 consecutive signed 32-bit fields:

```text
sx, sy, dbg_collisions, anti_unit, offx, offy, army, iroquois,
worker, no_danger, limit, saving, avoid_land, avoid_sea, valid_hit,
scouting, can_transport, dbg_view_failures, road_base_val,
road_avoid_sea, road_cross_coast, road_enemy, road_noone,
road_bad_path, road_river, road_z_max, road_diag_penalty
```

This deliberately excludes the six path-search pointers through +88,
BaseParamRegister state, the virtual-base pointer, and `show_debug` at +196.
PDB `PathFinderData` independently proves the same field sequence at its
relative offsets +24..+132.

## Exact Array<Group> grammar

The caller next invokes `Array<Group>::walk_data` `0x0047ea30` with no tag:

```text
i32 length
if length != 0:
    i32 capacity
    i16 increment
    u8  flags                         # writer clears bit 0x40
    repeat length times:
        # fixed Group projection, [Group+4,Group+76)
        i32 id
        i32 army
        i32 num
        i32 form
        i32 stamp
        i32 ox
        i32 oy
        i32 o_dist
        i32 o_angle
        i32 disband
        i32 order_num
        i32 priority
        i32 role
        i32 think_frame
        i32 new_speed
        i32 speed
        i32 form_num
        u8  facing
        u8  buildings
        u8  who
        u8  march
        if num != 0:
            i16 list[num]
            i32 off_x[num]
            i32 off_y[num]
            i32 curr_x[num]
            i32 curr_y[num]
            i8  angles[num]
```

The array has one allocation-history image and contiguous rows—no pointer
presence plane, repeated history, outer tag, or row tag. PDB
`Array<Group>` is 28 bytes. PDB `GroupData` is 2,508 bytes and has a vftable
at +0; retail omits it and starts the fixed walk at +4. PDB `Group` is 2,516
bytes because of its virtual-base pointer/tail.

`Group::walk_data` `0x00708400` proves the unusual dynamic order. When
`num != 0`, it emits the member `list` at PDB +2252 first, then the four
coordinate/offset arrays at +76, +588, +1100, and +1612, and finally the
angle bytes at +2124. Only each array's first `num` entries are saved. All
six arrays have PDB capacity 128, so the parser rejects `num` outside
0..128. Unused array tails and the Group virtual-base state are excluded.

## Next owner and frozen evidence

After the array, caller `0x005a2c38` emits a tag at StringTable byte offset
`0xe420`, exact index 2920, before walking the remaining Groups fields. That
tag defines the exact boundary `0x27e6e` and is excluded from this tranche.

The matching PDB and schema-export SHA-256 values are
`334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`
and `399aeac3bfb7a201a907e49e51388bb4e010420a54ccb781b335828315f28f14`.
The five-class walked-layout receipt is
`e6ba89e00795a435dbe4686fdc02d9202d6887c5814b61fde7b9e2db63cefe98`.
The matched PE SHA-256 is
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`.
Tests freeze PathFinder, Array<Group>, Group, and the caller's full handoff.

The synthetic fixture varies all 27 PathFinder fields, uses nondefault Array
history, and carries two Groups: one `num=0` fixed row and one `num=3` row
with signed values in every dynamic array. Every owned-byte mutation fails or
changes the receipt, every truncation is rejected, and dedicated tests cover
container history, the exact 128-member limit, all signed projections, and
next-tag exclusion.

## Reproduction

```sh
python3 re/scripts/test_savegame_pathfinder_groups.py

python3 re/scripts/savegame_pathfinder_groups.py \
  "ron-data/savegames/new save game 2026.08.11 15'42'57 (Tue).SVX" \
  --offset 0x27dfe
```

The returned `end` is the caller's exact Groups-tail tag address.
