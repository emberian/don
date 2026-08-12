# Retail save boundary: Constants duplicate and AI scalars

Status: **measured, coherent direct-scalar tranche**. This lane begins at the
exact `0x26ff2` end returned by the Constants-block parser and follows caller
order through three direct four-byte walks. It stops before Armies, does not
edit the shared parser, and makes no cross-match inference.

The helper is `re/scripts/savegame_direct_scalars.py`; tests are in
`re/scripts/test_savegame_direct_scalars.py`.

## Exact splice

For the fresh save with compressed SHA-256
`161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7`:

| stream range | caller | exact PDB owner | fresh value |
|---|---:|---|---:|
| `0x26ff2..0x26ff6` | `0x005a2a7c` | `Constants::mongol_three_mil_cavalry` `+0x804` | 0 |
| `0x26ff6..0x26ffa` | `0x005a2a97` | `GameAccess::ai_speed` | 0 |
| `0x26ffa..0x26ffe` | `0x005a2ab2` | `GameAccess::ai_off` | 0 |
| next owner at `0x26ffe` | `0x005a2abf` | `Armies::walk_data` `0x006f3700` | excluded |

The combined 12-byte fresh image is all zero, SHA-256
`15ec7bf0b50732b49f8228e07d24365338f9e3ab994b00af08e5a3bffe55fd8b`.

## Why these three form one tranche

The shipped PE SHA-256 is
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`.
Capstone of `WalkDataGame::walk_data` proves this exact sequence:

1. `0x005a2a6c..0x005a2a7c` reuses the Constants base and calls
   `DataWalk::walk_function(constants+0x804, constants+0x808)`.
2. `0x005a2a88..0x005a2a97` dereferences the `int&` stored at `0x00c061c0`
   and walks exactly four bytes.
3. `0x005a2aa3..0x005a2ab2` dereferences the `int&` stored at `0x00c061c4`
   and walks exactly four bytes.
4. `0x005a2abe..0x005a2abf` passes the same DataWalk to
   `Armies::walk_data`, the next native owner.

The calls at `0x005a2a7e`, `0x005a2a99`, and `0x005a2ab4` are profiling
bookkeeping with no DataWalk argument and no stream bytes. There is no tag,
alignment, or hidden framing between the three scalar writes, so grouping them
preserves both their individual boundaries and exact caller order.

The matching PDB SHA-256 is
`334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`,
GUID `51D4F219-61C6-4F84-9D5B-C3361B0D291F`, age 1. It resolves Constants
`+0x804` as the four-byte `int mongol_three_mil_cavalry`, `0x00c061c0` as
`GameAccess::ai_speed` of type `int&`, `0x00c061c4` as
`GameAccess::ai_off` of type `int&`, and the following method as
`Armies::walk_data`.

## Mutation, truncation, and provenance proof

The suite mutates all 12 owned bytes and proves each changes exactly its one
four-byte typed scalar and the tranche digest. Every truncation fails, while a
mutation at the first Armies byte is excluded.

The user-owned test chains Leaders, Types, TileSet, Mountains, the direct
Constants block, then these scalars to derive `0x26ffe`. It separately checks
the SVX seed `0x014810ac` and RCX seed `0x007f93e0`; no state or identity is
joined across those different artifacts.

## Reproduction

```sh
python3 re/scripts/test_savegame_direct_scalars.py

python3 re/scripts/savegame_direct_scalars.py \
  "ron-data/savegames/new save game 2026.08.11 15'42'57 (Tue).SVX" \
  --offset 0x26ff2
```

The returned end is the exact start of the exclusive Armies lane.
