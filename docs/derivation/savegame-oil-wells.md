# Retail save boundary: OilWells

Status: **complete concrete eight-owner container**. This lane begins at
`OilWells::walk_data`, consumes its tag, all eight independent
`PtrArray<OilWell>` histories and presence planes, and every exact OilWell
row, then stops before `Supplies::walk_data`. It remains an exclusive
parser/test/doc tranche and does not normalize history or join SVX/RCX state.

## Fresh-SVX splice

The fresh SVX compressed SHA-256 is
`161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7` and
its decompressed SHA-256 is
`fa31f89f9bf435e28847e9e54d62cfe712e6bafc6cedb9ab98c8ab6730adb7c8`.
Chaining the exclusive parsers through Docks reaches:

| stream range | owner | fresh value |
|---|---|---:|
| `0x270f7..0x270f8` | OilWells tag, StringTable[5071] | 0 |
| `0x270f8..0x27118` | eight `PtrArray<OilWell>.length` words | all 0 |
| next at `0x27118` | `Supplies::walk_data` `0x0073ae90` | excluded |

The exact 33-byte image has SHA-256
`7f9c9e31ac8256ca2f258583df262dbc7d6f68f2a03043d5c99a4ae5a7396ce9`.
SVX seed `0x014810ac` remains independent from RCX seed `0x007f93e0` and RCX
SHA-256 `558e0cd53dbed4f820e8757c0327a58384d433e5beb8e9eeef5d64df4d67bd54`.

## Exact grammar

`OilWells::walk_data` `0x0073f890` emits the tag at StringTable byte offset
`0x18c2c`, exact index 5071, then loops over the eight 28-byte arrays at
`0x00c0a680..0x00c0a75f`:

```text
i32 length
if length != 0:
    i32 capacity
    i16 increment
    u8  flags
    u8  pointer_present[length]
    i32 repeated_capacity
    i16 repeated_increment
    for each present slot:
        i16 oil_well
        i16 o
        i16 reg
        i8  oil_well_flags
        i8  who
```

The two history images are separately retained and must agree. Invalid
length, capacity, cleared flags bit `0x40`, presence, or repeated history is
rejected. Sparse logical identity and all signed values are preserved.

The present row is a direct `[OilWell,OilWell+8)` walk with no tag.
`OilWell::walk_data` `0x0073f650` independently makes the same call. PDB
OilWellData is exactly eight bytes; PDB OilWell is 16 bytes because its
virtual-base pointer begins at +8, exactly where serialization stops.

## Next owner and receipts

After profiling, the caller invokes `Supplies::walk_data` `0x0073ae90`.
Supplies starts with its own tag at StringTable byte offset `0x1e94c`; the
exact index is 6263. The OilWells boundary is therefore exactly fresh
`0x27118`.

The matching PDB and PDB-export SHA-256 values are
`334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`
and `399aeac3bfb7a201a907e49e51388bb4e010420a54ccb781b335828315f28f14`.
The exact PDB-layout receipt is
`eded763dbf926c50691f545c4417abf41242c2d5e5f8029954adbb0304e6fe8e`.
The matched executable SHA-256 is
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`.
Tests freeze OilWells, OilWell, the caller handoff, and Supplies.

The nonempty fixture exercises all eight owners, a sparse `[1,0,1]` presence
plane, duplicated nondefault history, and two complete rows. Every owned-byte
mutation fails or changes the receipt; every truncation is rejected. Tests
also freeze signed row bits, virtual-base-tail exclusion, exact next-owner
exclusion, and independent SVX/RCX identities.

## Reproduction

```sh
python3 re/scripts/test_savegame_oil_wells.py

python3 re/scripts/savegame_oil_wells.py \
  "ron-data/savegames/new save game 2026.08.11 15'42'57 (Tue).SVX" \
  --offset 0x270f7
```

The returned `end` is the exact start of `Supplies::walk_data`.
