# Retail save boundary: Lands

Status: **complete tagged ObjectArray with full Land POD and strings**. This
lane begins at the caller's Lands tag, consumes the exact
`ObjectArray<Land>` history and every complete Land row, and stops before
`LeaderOptions::walk_data`. It is an exclusive parser/test/doc tranche and
does not touch shared Rust or normalize retail allocation history.

## Fresh-SVX splice

The fresh SVX compressed and decompressed SHA-256 values are
`161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7` and
`fa31f89f9bf435e28847e9e54d62cfe712e6bafc6cedb9ab98c8ab6730adb7c8`.
The exact Lands stage is:

| range | owner | fresh value |
|---|---|---:|
| `0x2715a..0x2715b` | Lands tag, StringTable[4606] | 0 |
| `0x2715b..0x2715f` | `ObjectArray<Land>.length` | 0 |
| next at `0x2715f` | `LeaderOptions::walk_data` | excluded |

The exact five-byte section SHA-256 is
`8855508aade16ec573d21e6a485dfd0a7624085c1a14b5ecdd6485de0c6839a4`.
The test chains every landed helper through Caravans to reach `0x2715a`, then
proves that mutating `0x2715f` cannot affect Lands. SVX seed `0x014810ac` and
RCX seed `0x007f93e0` remain distinct; RCX SHA-256 is
`558e0cd53dbed4f820e8757c0327a58384d433e5beb8e9eeef5d64df4d67bd54`.

## Exact container and row grammar

The main caller at `0x005a2bd1` emits a `walk_test` at StringTable byte offset
`0x167d8`, exact index 4606, and then invokes
`ObjectArray<Land>::walk_data` `0x004786f0`. `Lands::walk_data` `0x0067e700`
is the independent class wrapper proving the identical tag and call order.

```text
u8  Lands tag                         # StringTable[4606]
i32 length
if length != 0:
    i32 capacity
    i16 increment
    u8  flags                         # writer clears bit 0x40
    repeat length times:
        u8  Land tag                  # StringTable[4604]
        i32 land
        i32 make[4]
        i32 num_make[4]
        i32 special[6]
        i32 special_sum
        i32 river_mask
        i32 river_bed
        i32 river_cost
        i32 num_rare
        i32 rare[44]
        i32 move_rate
        i32 combat_bonus
        String name
        String key

String:
    u32 code_unit_length
    u16 utf16le_code_units[code_unit_length]
```

Unlike the pointer arrays in neighboring owners, `ObjectArray<Land>` has no
presence plane and no repeated capacity/increment image. Rows are contiguous
and their identity is their array position. The parser retains the one
capacity/increment/flags history exactly, rejects invalid dimensions and the
writer-cleared flag bit, and preserves each row tag without inventing a
numeric byte value from the empty fresh array.

## PE/PDB field projection

Within each row the specialization emits the Land tag, directly walks
`[Land+0,Land+0x108)`, then calls `String::walk_data` on `Land+0x108` and
`Land+0x11c`. `Land::walk_data` `0x0067e680` independently proves the same
264-byte POD range and two calls. The matching PDB gives the exact split:

- `LandData +0..+264`: 66 signed 32-bit units grouped into the twelve named
  scalar/array fields above;
- `name +264` and `key +284`: two 20-byte in-memory `String` objects whose
  save projections are variable length;
- `LandData` ends at +304; PDB `Land` is 312 bytes because its virtual-base
  pointer begins at +304.

Only the String's current logical length and addressed UTF-16 payload are
serialized. Its pointer/const alias, offsets, flags, module id, and cached
hashes are not. The parser caps the stream length at the PDB `curr_len` u16
width and preserves arbitrary code units, including embedded zeroes and
unpaired surrogates, without text normalization. The Land virtual-base
pointer and object tail are excluded.

## Next owner and frozen evidence

After Lands and profiling, the caller directly invokes
`LeaderOptions::walk_data` `0x006f19a0`, defining the exact boundary
`0x2715f`. There is no inferred tag or alignment between them.

The matching PDB and schema-export SHA-256 values are
`334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`
and `399aeac3bfb7a201a907e49e51388bb4e010420a54ccb781b335828315f28f14`.
The six-class layout receipt is
`04016d308702adac2ef7228aa57022f7e14aea11a34b3820fd05f29b0b39aeb1`.
The matched PE SHA-256 is
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`.
Tests freeze Lands, ObjectArray, Land, String, the main caller handoff, and
the next LeaderOptions body.

The synthetic fixture carries a nondefault ObjectArray history and two exact
rows: all 66 POD integers vary, both tags vary, and the four strings cover
ordinary UTF-16, empty text, embedded zero, an unpaired surrogate, and a
non-ASCII code unit. Every owned-byte mutation fails or changes the receipt,
every truncation is rejected, and dedicated mutations freeze container and
String length constraints plus exact next-owner exclusion.

## Reproduction

```sh
python3 re/scripts/test_savegame_lands.py

python3 re/scripts/savegame_lands.py \
  "ron-data/savegames/new save game 2026.08.11 15'42'57 (Tue).SVX" \
  --offset 0x2715a
```

The returned `end` is the exact start of `LeaderOptions::walk_data`.
