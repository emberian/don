# Retail save boundary: LeaderOptions

Status: **complete fixed ten-row owner with dynamic BitMask projections**.
This lane begins at `LeaderOptions::walk_data`, consumes its outer tag and all
ten positional `LeaderOption` rows, and stops before `OptionInfo::walk_data`.
It is an exclusive parser/test/doc tranche and does not touch shared Rust.

## Fresh-SVX splice

The fresh SVX compressed and decompressed SHA-256 values are
`161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7` and
`fa31f89f9bf435e28847e9e54d62cfe712e6bafc6cedb9ab98c8ab6730adb7c8`.
The exact LeaderOptions stage is:

| range | owner | fresh value |
|---|---|---:|
| `0x2715f..0x27160` | outer tag, StringTable[4614] | 0 |
| `0x27160..0x2725a` | ten 25-byte zero-state rows | all 0 |
| next at `0x2725a` | `OptionInfo::walk_data` | excluded |

Each fresh row has a zero row tag, four zero scalar fields, `bits=0`,
`size=0`, and therefore no mask payload. The exact 251-byte section SHA-256
is `e258fc78e23908bdff0123123cb31e7a81008118ab1188ddcb740360727add4f`.
The test chains every landed helper through Lands to reach `0x2715f`, then
proves that mutating `0x2725a` cannot affect LeaderOptions. SVX seed
`0x014810ac` and RCX seed `0x007f93e0` remain distinct; RCX SHA-256 is
`558e0cd53dbed4f820e8757c0327a58384d433e5beb8e9eeef5d64df4d67bd54`.

## Exact grammar

`LeaderOptions::walk_data` `0x006f19a0` emits the outer `walk_test` at
StringTable byte offset `0x16878`, exact index 4614, then advances through ten
32-byte PDB rows with no saved count:

```text
u8 LeaderOptions tag                 # StringTable[4614]
repeat exactly 10 times:
    u8  LeaderOption tag             # StringTable[4613]
    i32 who
    i32 peasants
    i32 peasants_wait
    i32 buildings
    i32 flags.bits
    i32 flags.size
    u8  flags.ptr[flags.size]
```

The row `walk_test` uses StringTable byte offset `0x16864`, exact index 4613.
The executable issues four separate scalar walks, one eight-byte direct walk
for the `BitMask<32>` `bits` and `size`, and a final dynamic direct walk from
`ptr` to `ptr+size`. The in-memory BitMask `flags` word at row +24 is skipped;
load mode explicitly restores it to 2 after walking.

## BitMask relationship and PDB bounds

PDB `LeaderOptions` is exactly `LeaderOption[10]` at 320 bytes. Both
`LeaderOption` and `LeaderOptionData` are 32 bytes: the first sixteen bytes
are the four named integers and the final sixteen are `BitMask<32>`. PDB
BitMask fields are:

| offset | field | save behavior |
|---:|---|---|
| +0 | `bits` i32 | walked |
| +4 | `size` i32 | walked |
| +8 | `flags` i32 | excluded; restored on load |
| +12 | `ptr[4]` | exactly `size` bytes walked |

`LeaderOption::LeaderOption` `0x006f1db0` proves the template's full state is
`bits=32,size=4`; other initialization paths compute the byte count as
`ceil(bits/8)`. The fresh uninitialized rows prove that the zero state
`bits=0,size=0` also occurs in a retail save. The parser therefore accepts
all exact shapes `0 <= bits <= 32` with `size == ceil(bits/8)` and rejects
negative, oversized, or mismatched pairs. It retains every payload byte and
does not normalize a partial final byte.

## Next owner and frozen evidence

After profiling at caller `0x005a2bf7`, the next direct call is
`OptionInfo::walk_data` `0x0072c1e0`; this defines the exact boundary
`0x2725a` without an inferred delimiter.

The matching PDB and schema-export SHA-256 values are
`334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`
and `399aeac3bfb7a201a907e49e51388bb4e010420a54ccb781b335828315f28f14`.
The four-class layout receipt is
`48086cd9601a5ae00269549dbb931436fc6930a92f8d29d906302aebff47c54a`.
The matched PE SHA-256 is
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`.
Tests freeze LeaderOptions, the LeaderOption constructor, caller handoff, and
the next OptionInfo body.

The synthetic fixture covers exactly ten rows with bit counts
`0,1,7,8,9,16,17,24,31,32`, all corresponding payload widths, distinct
signed scalar fields, distinct row tags, and independently varying mask
bytes. Every owned-byte mutation fails or changes the receipt, every
truncation is rejected, and dedicated tests kill all BitMask relationship
violations and prove next-owner exclusion.

## Reproduction

```sh
python3 re/scripts/test_savegame_leader_options.py

python3 re/scripts/savegame_leader_options.py \
  "ron-data/savegames/new save game 2026.08.11 15'42'57 (Tue).SVX" \
  --offset 0x2715f
```

The returned `end` is the exact start of `OptionInfo::walk_data`.
