# Retail save boundary: Supplies

Status: **complete concrete eight-owner container**. This lane begins at
`Supplies::walk_data`, consumes its tag, all eight independent
`PtrArray<Supply>` histories and presence planes, and every exact Supply row,
then stops before `Caravans::walk_data`. It is an exclusive parser/test/doc
tranche and does not touch shared Rust or normalize retail history.

## Fresh-SVX splice

The fresh SVX compressed and decompressed SHA-256 values are
`161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7` and
`fa31f89f9bf435e28847e9e54d62cfe712e6bafc6cedb9ab98c8ab6730adb7c8`.
The exact Supplies stage is:

| range | owner | fresh value |
|---|---|---:|
| `0x27118..0x27119` | tag, StringTable[6263] | 0 |
| `0x27119..0x27139` | eight `PtrArray<Supply>.length` words | all 0 |
| next at `0x27139` | `Caravans::walk_data` `0x0073e3f0` | excluded |

The exact 33-byte section SHA-256 is
`7f9c9e31ac8256ca2f258583df262dbc7d6f68f2a03043d5c99a4ae5a7396ce9`.
The test chains every landed helper through Docks and freezes the intervening
independently recovered 33-byte OilWells receipt to reach `0x27118`; this
keeps Supplies independent while OilWells is forward-repaired. SVX seed
`0x014810ac` and RCX seed `0x007f93e0` remain distinct; RCX SHA-256 is
`558e0cd53dbed4f820e8757c0327a58384d433e5beb8e9eeef5d64df4d67bd54`.

## Exact grammar and row

`Supplies::walk_data` `0x0073ae90` emits the tag at StringTable byte offset
`0x1e94c`, exact index 6263, then loops over eight 28-byte pointer arrays at
`0x00e3a1b0..0x00e3a28f`:

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
        i16 supply
        i16 o
        i8  supply_flags
        i8  who
```

Both physical history images must agree but remain independently represented.
The parser rejects invalid length/capacity/flags/presence/history and retains
sparse indices and signed field values.

There is no row tag. The outer walker directly emits six bytes; independent
`Supply::walk_data` `0x0073b540` makes the same `[Supply,Supply+6)` call. PDB
SupplyData is exactly six bytes. PDB Supply is 16 bytes, with alignment at
6..8 and its virtual-base pointer beginning at +8; neither object tail nor
padding is walked. PDB Supplies itself is exactly the 224-byte array block,
not the 232-byte output-wrapper layout used by several neighboring families.

## Next owner and evidence

After Supplies and profiling, the main caller invokes `Caravans::walk_data`
`0x0073e3f0`, defining the exact boundary `0x27139`.

The matching PDB and export SHA-256 values are
`334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`
and `399aeac3bfb7a201a907e49e51388bb4e010420a54ccb781b335828315f28f14`.
The layout receipt is
`2df0e191c619386789247c14320377d25dda2f317f1a4c0bf19ecd3911024275`.
The matched PE SHA-256 is
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`.
Tests freeze Supplies, Supply, the caller handoff, and Caravans.

The synthetic fixture covers all eight owners, sparse presence `[1,0,1]`,
nondefault duplicated history, and two complete six-byte rows. Every owned
byte mutation fails or changes the receipt, every truncation is rejected, and
dedicated tests prove exact signed rows, padding/tail exclusion, next-owner
exclusion, fresh chaining, and SVX/RCX independence.

## Reproduction

```sh
python3 re/scripts/test_savegame_supplies.py

python3 re/scripts/savegame_supplies.py \
  "ron-data/savegames/new save game 2026.08.11 15'42'57 (Tue).SVX" \
  --offset 0x27118
```

The returned `end` is the exact start of `Caravans::walk_data`.
