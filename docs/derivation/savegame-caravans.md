# Retail save boundary: Caravans

Status: **complete concrete eight-owner container with dynamic path stacks**.
This lane begins at `Caravans::walk_data`, consumes its tag, all eight
independent `PtrArray<Caravan>` histories and presence planes, every exact
Caravan save body, and each embedded `Stack<PathData>` history and live row.
It stops before the caller's Lands tag. It is an exclusive parser/test/doc
tranche and does not touch shared Rust or normalize retail history.

## Fresh-SVX splice

The fresh SVX compressed and decompressed SHA-256 values are
`161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7` and
`fa31f89f9bf435e28847e9e54d62cfe712e6bafc6cedb9ab98c8ab6730adb7c8`.
The exact Caravans stage is:

| range | owner | fresh value |
|---|---|---:|
| `0x27139..0x2713a` | Caravans tag, StringTable[417] | 0 |
| `0x2713a..0x2715a` | eight `PtrArray<Caravan>.length` words | all 0 |
| next at `0x2715a` | caller Lands tag, StringTable[4606] | excluded |

The exact 33-byte section SHA-256 is
`7f9c9e31ac8256ca2f258583df262dbc7d6f68f2a03043d5c99a4ae5a7396ce9`.
The test chains every landed helper through OilWells and Supplies to reach
`0x27139`, then proves that mutating `0x2715a` cannot affect Caravans. SVX
seed `0x014810ac` and RCX seed `0x007f93e0` remain distinct; RCX SHA-256 is
`558e0cd53dbed4f820e8757c0327a58384d433e5beb8e9eeef5d64df4d67bd54`.

## Exact container and row grammar

`Caravans::walk_data` `0x0073e3f0` emits its tag at StringTable byte offset
`0x2094`, exact index 417, then loops over eight 28-byte pointer arrays in the
PDB global block `0x00e3a290..0x00e3a36f`:

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
        u8  Caravan tag                 # StringTable[416]
        i16 city2
        i16 whom
        i16 city3
        i16 whose
        i16 cara
        i16 o
        u8  caravan_flags
        i8  who
        i32 making_road
        i32 reset_road
        i32 road.capacity
        i32 road.length
        i8  road.increment
        repeat road.length times:
            i32 to_x
            i32 to_y
            i32 tolerance
            i32 flags
```

Both pointer-array history images must agree but remain independently
represented. The path stack's capacity, live length, and signed increment are
also retained exactly. The parser rejects negative or oversized dimensions,
`road.length > road.capacity`, nonboolean presence bytes, writer-cleared bit
`0x40`, and mismatched repeated pointer-array history.

`Caravan::walk_data` `0x0073d2b0` proves the row order: the tag uses
StringTable byte offset `0x2080`, exact index 416; `[Caravan+0,Caravan+0xe)`
is emitted directly; `[Caravan+0x20,Caravan+0x28)` follows; then
`Stack<PathData>::walk_data` is called on `Caravan+0x10`. Thus the save stream
places `making_road` and `reset_road` before the road history even though the
PDB object places the stack earlier in memory.

`Stack<PathData>::walk_data` `0x0046d8b0` emits `[stack+4,stack+0xd)`—capacity,
length, and increment—then exactly `length` 16-byte `PathData` rows. The list
pointer is not serialized. PDB `PathData` confirms four 32-bit fields in the
walked order.

## Walked and excluded object state

PDB `CaravanData` is 68 bytes and `Caravan` is 80 bytes. Only these object
ranges participate in the save body:

- `+0x00..+0x0e`: six signed city/object shorts, unsigned
  `caravan_flags`, and signed `who`;
- `+0x20..+0x28`: `making_road` and `reset_road`;
- the dynamic projection of `road` at `+0x10` described above.

The stack list pointer, `openlist`, `openlistrefs`, `closedlist`, object
`offset`, `endx`, `endy`, `traversed`, the virtual-base pointer, PDB
`last_draw_frame`, padding, and object tail are not walked. A present row is
therefore exactly 32 bytes with an empty path stack, plus 16 bytes per live
`PathData` row. The parser preserves row tags without inventing a numeric
constant from the rowless fresh specimen.

## Next owner and frozen evidence

After Caravans and profiling, the main caller emits the Lands tag at
StringTable byte offset `0x167d8`, exact index 4606, then calls
`ObjectArray<Land>::walk_data` `0x004786f0`. This defines the exact boundary
`0x2715a`; the Lands tag is not part of Caravans.

The matching PDB and schema-export SHA-256 values are
`334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`
and `399aeac3bfb7a201a907e49e51388bb4e010420a54ccb781b335828315f28f14`.
The six-class layout receipt is
`45d962ca2e00858bbc31ce43e9065e0ab67f70d7591df41cba071b45f8ebe405`.
The matched PE SHA-256 is
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`.
Tests freeze Caravans, Caravan, `Stack<PathData>`, the caller handoff, and the
next `ObjectArray<Land>` body.

The synthetic fixture covers all eight owners, sparse presence `[1,0,1]`,
nondefault duplicated pointer history, arbitrary row tags, an empty path
stack, and a second stack with capacity five and two exact PathData rows.
Every owned-byte mutation fails or changes the receipt, every truncation is
rejected, and dedicated mutations cover all container/stack invariants and
the exact next-owner exclusion.

## Reproduction

```sh
python3 re/scripts/test_savegame_caravans.py

python3 re/scripts/savegame_caravans.py \
  "ron-data/savegames/new save game 2026.08.11 15'42'57 (Tue).SVX" \
  --offset 0x27139
```

The returned `end` is the caller's exact Lands-tag address.
