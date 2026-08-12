# Retail save boundary: caller Farms tranche and `Array<FarmStruct>`

Status: **measured complete owner**. This isolated parser begins at the tag
immediately following `Scene::walk_data`, preserves both direct `Farms` ranges
and the complete dynamic `Array<FarmStruct>::walk_data` image, and stops at the
first byte consumed by `UnbuiltWonders::walk_data`. It changes no shared parser
or simulation source.

The helper is `re/scripts/savegame_farm_structs.py`; its dynamic-row,
byte-mutation, truncation, PE/PDB, fresh-SVX, and RCX-independence gates are in
`re/scripts/test_savegame_farm_structs.py`.

## Exact caller order and boundary

The matched caller span is VA `0x005a2e20..0x005a2e71`, 81 bytes, SHA-256
`635f31c3efb02c178fbdf8da27fe1303d9b3658e3cc35540a293631dea56e8fc`.
It proves this order without locating fields by their save values:

```text
walk_test StringTable[2672]             # byte offset 0xd0c0, fresh tag 0x04
DataWalk [0x00c0a8f0,0x00c0a8fa)        # farms +64..+74
DataWalk [0x00c0a8fc,0x00c0a904)        # farms +76..+84
Array<FarmStruct>::walk_data             # this = 0x00c0a904, farms +84
profile transition
UnbuiltWonders::walk_data                # this = 0x00c120d0, next owner
```

The PDB global `farms` is at preferred VA `0x00c0a8b0`. The caller therefore
selects exactly `Farms::start_color[+0,+10)`, skips the Color object's two
trailing padding bytes, writes the two adjacent wheat-height floats, then
dispatches `Farms::farm_data`.

The fresh save's compressed SHA-256 is
`161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7`;
its decompressed SHA-256 is
`fa31f89f9bf435e28847e9e54d62cfe712e6bafc6cedb9ab98c8ab6730adb7c8`.

| owner | decompressed range | bytes | SHA-256 |
|---|---:|---:|---|
| caller Farms tranche | `0x2bc76..0x2bc8d` | 23 | `dfc517291c32174169a0463154005f2924a9567b493f3af5e4672629b7ecedfb` |
| next `UnbuiltWonders::walk_data` prefix | `0x2bc8d..0x2bc91` | 4 | `df3f619804a92fdb4057192dc43dd748ea778adc52bc498ce80524c014b81119` |

The 23-byte fresh image is one tag byte, ten logical Color bytes, eight wheat
bytes, and a zero `farm_data.length` word. The zero word at `0x2bc8d` belongs
to `UnbuiltWonders`; mutating it does not change the Farms parse.

## Exact save grammar

```text
u8    Farms tag                         # StringTable[2672], expected 0x04
bytes start_color[0..10)                # Farms +64..+74
u32   wheat_max_height IEEE-754 bits    # Farms +76..+80
u32   wheat_min_height IEEE-754 bits    # Farms +80..+84

i32 farm_data.length
if length != 0:
    i32 capacity
    i16 increment
    u8  flags                           # retail writer clears bit 0x40
    repeat length times:
        bytes FarmStruct[0..190)
```

The helper retains floating-point fields as raw `u32` bit words. This preserves
NaN payloads and signed zero exactly and makes the mutation proof independent
of host floating-point equality.

There is no pointer-array presence plane and no repeated capacity/increment
header here. `Array<FarmStruct>` is a simple-copy value container: allocation
history appears once, immediately before the contiguous logical rows.

## PE proof for 190-byte rows

`Array<FarmStruct>::walk_data` is the complete 502-byte body at VA
`0x004a8db0`, SHA-256
`e6c75197c554488767f5ffc39f25d0180ab15594abdda12504ca65f1c3a6d7b7`.
Its row loop obtains each element at the 192-byte in-memory stride, but submits
the half-open range from the row pointer through `row+0xbe` to the DataWalk.
It then advances the backing-list pointer by `0xc0`. Thus each save row is
exactly 190 bytes, and the structure's final two alignment bytes never enter
the stream.

The actual following owner is `UnbuiltWonders::walk_data`, complete body VA
`0x0073c290`, size 550, SHA-256
`2f39ca65083926b449a36b5b40f06d086ec707e9b9e9436b06e18e0129d50234`.
Freezing both its body and the caller transition prevents the end boundary from
being inferred by searching the fresh save for a run of zeroes.

The matched PE SHA-256 is
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`.

## PDB ownership

The matched PDB SHA-256 is
`334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`;
the extracted schema SHA-256 is
`399aeac3bfb7a201a907e49e51388bb4e010420a54ccb781b335828315f28f14`.

The helper freezes these layouts:

- `Farms`, 480 bytes: `start_color` at `+64`, wheat heights at `+76/+80`,
  and `farm_data` at `+84`;
- `Color`, 12 bytes: aliased `red`/`rgb` at `+0`, `green`/`blue`/`alpha`,
  `w_555`, `w_565`, `flags`, and `index`, whose final logical byte is `+9`;
- `Array<FarmStruct>`, 28 bytes: length, capacity, increment, runtime list
  pointer, flags, and cursor fields;
- `FarmStruct`, 192 bytes: `who`/`o` through `+8`, 16 float words through
  `+72`, 25 terrain-height float words through `+172`, 16 status bytes,
  `valid` at `+188`, and `farm_type` at `+189`.

The field map ends at `+190`; the remaining two bytes are implicit alignment
padding. The deterministic layout receipt is
`6880c7d5db8782867943ace21c178e71fa2741062491a31618e900f7fd3645db`.

## Dynamic mutation and truncation proof

The synthetic fixture uses two live rows, nontrivial capacity/increment/flags,
distinct signed owners and object IDs, all 16+25 float bit words per row,
distinct status planes, and both trailing logical bytes. It also uses a NaN
payload for a wheat word and for row values so host float conversion cannot
silently normalize the stream.

Every owned byte is mutated independently. Each mutation is rejected by a
strict tag/count/history/range rule or changes the immutable row/section image
and SHA-256. Every truncation fails. Dedicated gates reject negative length,
capacity below length, retained writer flag `0x40`, the wrong tag, and a mutated
PDB row size. Mutating the first `UnbuiltWonders` byte leaves the result
identical.

The installed-artifact gate chains World → GameDaemon → game_random →
GraphicEvents → Scene → Farms using only returned owner boundaries. It checks
the exact fresh range and independently verifies SVX seed `0x014810ac` and RCX
seed `0x007f93e0`; replay bytes are never used to decode the save.

## Reproduction

```sh
python3 re/scripts/test_savegame_farm_structs.py

python3 re/scripts/savegame_farm_structs.py \
  "ron-data/savegames/new save game 2026.08.11 15'42'57 (Tue).SVX" \
  --offset 0x2bc76
```

The returned end is the exact first byte owned by
`UnbuiltWonders::walk_data`.
