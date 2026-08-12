# Retail save boundary: Forts

Status: **complete concrete eight-owner container**. This lane begins at
`Forts::walk_data`, consumes its tag, all eight independent `PtrArray<Fort>`
histories and presence planes, and every exact Fort row, then stops before
`Docks::walk_data`. It does not edit the shared parser, normalize history, or
join SVX and RCX state.

The exclusive helper is `re/scripts/savegame_forts.py`; exhaustive tests are
in `re/scripts/test_savegame_forts.py`.

## Fresh-SVX splice

The user-created SVX has compressed SHA-256
`161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7` and
decompressed SHA-256
`fa31f89f9bf435e28847e9e54d62cfe712e6bafc6cedb9ab98c8ab6730adb7c8`.
Chaining the exclusive parsers from Leaders through Wonders reaches:

| stream range | exact owner | fresh value |
|---|---|---:|
| `0x270b5..0x270b6` | Forts `walk_test(StringTable[2697])` | 0 |
| `0x270b6..0x270ba` | `lists[0].length` | 0 |
| `0x270ba..0x270be` | `lists[1].length` | 0 |
| `0x270be..0x270c2` | `lists[2].length` | 0 |
| `0x270c2..0x270c6` | `lists[3].length` | 0 |
| `0x270c6..0x270ca` | `lists[4].length` | 0 |
| `0x270ca..0x270ce` | `lists[5].length` | 0 |
| `0x270ce..0x270d2` | `lists[6].length` | 0 |
| `0x270d2..0x270d6` | `lists[7].length` | 0 |
| next owner at `0x270d6` | `Docks::walk_data` `0x00741100` | excluded |

The exact 33-byte Forts image is all zero and has SHA-256
`7f9c9e31ac8256ca2f258583df262dbc7d6f68f2a03043d5c99a4ae5a7396ce9`.
The separate RCX SHA-256 is
`558e0cd53dbed4f820e8757c0327a58384d433e5beb8e9eeef5d64df4d67bd54`.
The test independently retains SVX seed `0x014810ac` and RCX seed
`0x007f93e0`; no identity is joined.

## Tag and eight pointer owners

`Forts::walk_data` `0x0073ef30` emits the tag at StringTable byte offset
`0xd2b4`, exact index 2697, then loops over eight 28-byte
`PtrArray<Fort>` globals at `0x00c0a480..0x00c0a55f`. PDB `Forts::lists` is
exactly `PtrArray<Fort>[8]`, size 224.

Each outer owner is:

```text
i32 length
if length != 0:
    i32 capacity
    i16 increment
    u8  flags                       # writer persistently clears bit 0x40
    u8  pointer_present[length]     # exact booleans, logical-slot order
    i32 repeated_capacity
    i16 repeated_increment
    for each present pointer, in slot order:
        Fort body
```

Both history images must agree but are retained separately. Negative or
unreasonable lengths, `capacity < length`, the cleared flags bit, nonboolean
presence, and history disagreement fail closed. Holes are never compacted;
histories are never defaulted or merged. `cur_index` at `+24` is not walked.

## Exact eight-byte Fort body

Every present pointer is constructed as a 16-byte `Fort`, but the outer
walker directly emits only `[Fort+0,Fort+8)`:

```text
i16 fort
i16 o
i16 reg
i8  fort_flags
i8  who
```

There is no row tag. `Fort::walk_data` `0x0073ebc0` independently makes the
same direct eight-byte call. PDB `FortData` is exactly eight bytes. PDB `Fort`
is 16 bytes because its output/access class adds a virtual-base pointer at
`+8`; that object tail is not serialized. Both final PDB `char` fields are
exposed as signed `i8` while preserving their exact bits.

## Exact next owner

After Forts returns, profiling owns no stream bytes and the main caller invokes
`Docks::walk_data` `0x00741100`. Docks starts with its own tag at StringTable
byte offset `0xccd8`, exact index 2622. Thus Forts ends immediately before the
Docks tag at fresh `0x270d6`.

## Receipts and tests

The matching PDB SHA-256 is
`334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`;
its JSON export SHA-256 is
`399aeac3bfb7a201a907e49e51388bb4e010420a54ccb781b335828315f28f14`.
The helper requires exact sizes and layouts for 232-byte Forts, 28-byte
`PtrArray<Fort>`, 16-byte Fort, and eight-byte FortData. The PDB layout receipt
is `6bdfd9d96f526fb925bb14718bb3255447c96a187cf025dd6b707705f02119d6`.

The matched PE SHA-256 is
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`.
Tests freeze `Forts::walk_data`, `Fort::walk_data`, the caller handoff, and
the complete following `Docks::walk_data` owner.

The nonempty fixture has all eight owners: owner 0 uses length 3, presence
`[1,0,1]`, duplicated nondefault history, and two full rows; owners 1 through
7 are empty. Every owned-byte mutation fails an invariant or changes the
receipt and digest. Every truncation is rejected. Tests additionally prove
signed field preservation, sparse identity, full eight-byte rows, exclusion
of the Fort object's virtual-base tail, the exact next-owner boundary, and
SVX/RCX independence.

## Reproduction

```sh
python3 re/scripts/test_savegame_forts.py

python3 re/scripts/savegame_forts.py \
  "ron-data/savegames/new save game 2026.08.11 15'42'57 (Tue).SVX" \
  --offset 0x270b5
```

The returned `end` is the exact start of `Docks::walk_data`.
