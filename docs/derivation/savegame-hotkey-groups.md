# Retail save boundary: HotKeyGroup array

Status: **complete caller tag plus `Array<HotKeyGroup>`**. This isolated
tranche starts at StringTable[3963] immediately after the landed Objects
section, preserves the array history and the full count-dependent Group row
grammar, and stops before `World::walk_data`. It does not touch shared parser
or Rust files.

## Fresh-SVX splice

The fresh SVX compressed and decompressed SHA-256 values are
`161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7` and
`fa31f89f9bf435e28847e9e54d62cfe712e6bafc6cedb9ab98c8ab6730adb7c8`.
The landed Objects section ends at `0x27f4e`. The exact next stage is:

| range | owner | fresh value |
|---|---|---:|
| `0x27f4e..0x27f4f` | caller tag, StringTable[3963] | 0 |
| `0x27f4f..0x27f53` | `Array<HotKeyGroup>.length` | 0 |
| next at `0x27f53` | `World::walk_data` | excluded |

The exact five-byte section SHA-256 is
`8855508aade16ec573d21e6a485dfd0a7624085c1a14b5ecdd6485de0c6839a4`.
The installed-artifact test chains every landed save helper from Leaders at
`0x9a5a` through Objects to reach `0x27f4e`; no byte search is used. Mutating
the World byte at `0x27f53` cannot affect this section.

The caller at `0x005a2d88` emits a tag from StringTable byte offset `0x1359c`,
exact index 3963, and invokes `Array<HotKeyGroup>::walk_data` `0x00480290` at
`0x005a2d9f`. After the array returns, load-only pauses and reconstruction do
not consume stream bytes. The next visitor call is `World::walk_data`
`0x006b5cf0` with selector -1 at `0x005a2db1`. This proves the exact next owner.

## Array history

PDB `Array<HotKeyGroup>` is 28 bytes with the ordinary `ArrayBaseMaster`
history. The PE grammar is:

```text
u8  HotKeyGroup array tag             # StringTable[3963]
i32 length
if length != 0:
    i32 capacity
    i16 increment
    u8  flags                         # save path clears bit 0x40
    HotKeyGroup row[length]
```

Capacity must cover length. This Array specialization does not emit a
presence plane, a type plane, or a repeated capacity/increment tail; every
element from zero through `length - 1` is walked contiguously. The in-memory
row stride is exactly `0x9fc` (2556 bytes), matching PDB `HotKeyGroup` size.

## `Group::walk_data` base row

Each array element first calls `Group::walk_data` `0x00708400`. It directly
walks PDB `GroupData` +4..+76, which is 17 signed 32-bit scalars followed by
four unsigned bytes:

```text
i32 id, army, num, form, stamp
i32 ox, oy, o_dist, o_angle, disband, order_num, priority
i32 role, think_frame, new_speed, speed, form_num
u8  facing, buildings, who, march
```

When `num != 0`, the PE walks only the active prefix of each fixed 128-entry
PDB array, in this executable order:

```text
i16 list[num]                         # PDB +2252
i32 off_x[num]                        # PDB +76
i32 off_y[num]                        # PDB +588
i32 curr_x[num]                       # PDB +1100, Coord
i32 curr_y[num]                       # PDB +1612, Coord
i8  angles[num]                       # PDB +2124, char
```

The list-first order is not PDB address order. It follows the exact call
sequence at `0x00708424..0x007084ac`. The parser validates `0 <= num <= 128`
against all six fixed PDB arrays and never normalizes the current active
length to capacity 128.

PDB sizes distinguish compiler/runtime layout from the walked image:
`GroupData` is 2508 bytes, `GroupOut` 2512, and `Group` 2516. Only the direct
base range and the six `num`-sized prefixes above are serialized; virtual-base
state and padding are excluded.

## HotKeyGroup tail

After the Group base, the array walker emits a per-row tag using StringTable
byte offset `0x13588`, exact index 3962, then walks the exact PDB
`HotKeyGroupData` range +2512..+2524:

```text
u8  HotKeyGroup row tag               # StringTable[3962]
u32 loc_x IEEE-754 bits
u32 loc_y IEEE-754 bits
i32 valid
```

The parser retains the two float fields as raw IEEE-754 bit patterns. This is
lossless for NaNs and signed zero and avoids host float normalization in the
section receipt. PDB output-only `zoom_level`, `String name`, and
`statwin_icon` at +2524 and above are not walked.

## Frozen evidence and gates

The matching executable, PDB, and schema SHA-256 values are
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`,
`334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`,
and `399aeac3bfb7a201a907e49e51388bb4e010420a54ccb781b335828315f28f14`.
The exact seven-class PDB layout receipt is
`7f6450dae5436ae33a9e86b599ce6fdd3c688017c6c7d966034662b4211b90ca`.
Tests freeze the full Array and Group walker bodies, the caller tag/call span,
and the complete next World walker.

The nonempty synthetic fixture contains one zero-member row and one
three-member row with signed list, coordinate, angle, increment, and validity
values, plus NaN-payload and negative-zero float bit patterns. Every owned
byte mutation is rejected or changes the section receipt, every truncation is
rejected, and independent gates cover both tag levels, capacity below length,
writer-cleared flag bit, `num > 128`, PDB mutation, and World-byte exclusion.

SVX seed `0x014810ac` and RCX seed `0x007f93e0` remain distinct. RCX SHA-256 is
`558e0cd53dbed4f820e8757c0327a58384d433e5beb8e9eeef5d64df4d67bd54`;
the replay is checked independently rather than joined to the save stream.

## Reproduction

```sh
python3 re/scripts/test_savegame_hotkey_groups.py

python3 re/scripts/savegame_hotkey_groups.py \
  "ron-data/savegames/new save game 2026.08.11 15'42'57 (Tue).SVX" \
  --offset 0x27f4e
```

The returned `end` is the exact start of `World::walk_data`.
