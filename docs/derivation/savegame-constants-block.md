# Retail save boundary: direct `Constants[+0x000..+0xd40)`

Status: **measured, exclusive PDB-decoded block**. This lane begins at the
exact `0x262b2` end returned by the Mountains parser and decodes the caller's
complete direct Constants prefix. It stops before the next direct stream walk,
does not edit the shared parser, and makes no cross-match inference.

The helper is `re/scripts/savegame_constants_block.py`; tests are in
`re/scripts/test_savegame_constants_block.py`. The helper consumes the existing
compiler-emitted `schema/pdb-types.json` rather than hand-maintaining 721 field
names or flattening PDB arrays by guesswork.

## Exact splice

For the fresh save with compressed SHA-256
`161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7`
and decompressed SHA-256
`fa31f89f9bf435e28847e9e54d62cfe712e6bafc6cedb9ab98c8ab6730adb7c8`:

| owner | decompressed range | bytes | SHA-256 |
|---|---:|---:|---|
| direct `Constants[+0x000..+0xd40)` | `0x000262b2..0x00026ff2` | 3,392 | `91dd398f2278dc983e361803fe262142b261d1a985d5e9c4d1c1a7065b4bfbf4` |
| next owner, excluded | `0x00026ff2..0x00026ff6` | 4 | duplicate direct walk of `Constants::mongol_three_mil_cavalry` at `+0x804` |

The fresh block contains 848 signed 32-bit words: 813 zero and 35 nonzero,
distributed over 22 named PDB fields. Those values are reported as specimen
facts only; this lane does not infer that the unrelated replay shared them.

## Exact caller body

The shipped PE SHA-256 is
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`.
Capstone of `WalkDataGame::walk_data` shows:

```text
0x005a2a56  esi = *(Constants **)0x00c061f0
0x005a2a60  end = esi + 0xd40
0x005a2a67  begin = esi
0x005a2a68  DataWalk::walk_function(begin, end)
```

There is no loop, tag, or nested `Constants::walk_data`. This is one raw,
unconditional 3,392-byte call. The next stream call reloads the same `esi` and
submits `[esi+0x804, esi+0x808)` at `0x005a2a7c`; profiling between the calls
receives no DataWalk pointer. Therefore byte `0x26ff2` belongs to that separate
duplicate field, not to this prefix or to the later `ai_speed`/Armies owners.

PDB symbols name `0x00c061f0` as `GameAccess::constants` of type `Constants&`.

## Complete PDB representation

The matching PDB SHA-256 is
`334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`,
GUID `51D4F219-61C6-4F84-9D5B-C3361B0D291F`, age 1. The complete
compiler-extracted `schema/pdb-types.json` used to read its Constants layout
has SHA-256
`399aeac3bfb7a201a907e49e51388bb4e010420a54ccb781b335828315f28f14`.

The PDB says `sizeof(Constants) = 0xd68`. Its first 721 fields cover
`[+0x000,+0xd40)` exactly, monotonically and without one byte of gap, padding,
or overlap. Their types are:

| PDB type | fields | words per field |
|---|---:|---:|
| `int` | 690 | 1 |
| `int[3]` | 1 | 3 |
| `int[4]` | 11 | 4 |
| `int[5]` | 11 | 5 |
| `int[6]` | 4 | 6 |
| `int[8]` | 4 | 8 |

That totals 721 named fields and 848 four-byte signed words. The first field is
`unit_formation_spacing` at `+0x000`; the final included field is `attrition`
at `+0xd3c`. The first excluded PDB field is the 40-byte `XMLElement
curr_element` at `+0xd40`, exactly where the executable ends its direct range.

The helper validates all of these facts before decoding. It retains every PDB
array as one named field with its exact tuple of signed values, while also
offering the fully flattened 848-word view. Thus `int[8] pop_cap`, for example,
cannot silently become eight anonymously guessed integers.

The next direct caller range starts at Constants `+0x804`; the PDB resolves
that exact four-byte field as `mongol_three_mil_cavalry`.

## Mutation, truncation, and provenance proof

The synthetic suite mutates all 3,392 owned bytes independently and proves each
mutation changes exactly one PDB field plus the block digest. Every truncation
from zero through 3,391 retained bytes fails. Mutating the first byte of the
following duplicate field is excluded from the result.

The user-owned test chains Leaders, Types, TileSet, Mountains, then Constants
to derive the `0x26ff2` boundary and verify the fresh block digest. It separately
checks the SVX seed `0x014810ac` and RCX seed `0x007f93e0`; no bytes, object
identity, or state are joined between those artifacts.

## Reproduction

```sh
python3 re/scripts/test_savegame_constants_block.py

python3 re/scripts/savegame_constants_block.py \
  "ron-data/savegames/new save game 2026.08.11 15'42'57 (Tue).SVX" \
  --offset 0x262b2
```

Use `--json` to emit every one of the 721 named PDB fields. The returned end is
the exact start of the separate `Constants[+0x804..+0x808)` owner.
