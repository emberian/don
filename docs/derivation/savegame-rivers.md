# Retail save boundary: `PtrArray<River>::walk_data`

Status: **complete PE/PDB grammar; synthetic boundary only**. The only
installed SVX is already contradictory inside the preceding
`CommandManager::walk_data`, so no River stream offset is claimed. The helper
`re/scripts/savegame_rivers.py` instead freezes the complete owner grammar for
an explicit known-good offset. Exhaustive gates live in
`re/scripts/test_savegame_rivers.py`.

## Complete retail grammar

`PtrArray<River>::walk_data` emits the generic pointer-array allocation image,
a byte-wide pointer-presence plane, a repeated allocation image, and one River
body for each present slot:

```text
i32 length
if length != 0:
    i32 capacity
    i16 increment
    u8  flags                         # writer clears bit 0x40
    u8  present[length]               # each exactly 0 or 1
    i32 repeated_capacity
    i16 repeated_increment
    for slot where present[slot] != 0:
        River::walk_data

River::walk_data:
    i32 creation_spline_present       # writer emits exactly 0 or 1
    if creation_spline_present != 0:
        SplineData::walk_data
```

The second capacity/increment pair comes from the direct walk of object bytes
`+8..+14` after the presence/allocation pass. A retail writer reads both
images from the same members, so the parser requires equality. An empty owner
is exactly four bytes. A nonempty one-slot owner with a null River pointer is
18 bytes; a present River with a null creation spline adds four bytes.

The River marker is notably a four-byte `int`. It is not the byte marker used
by the surrounding `PtrArray` template.

## Complete `SplineData` body

A present creation spline begins with one `walk_test` byte selected by
StringTable index 6,209, followed by the direct PDB member range
`SplineData +64..+100`:

```text
u8  walk_test(StringTable[6209])
i32 type
i32 flags
u32 max_control_depth_ratio_bits      # float, preserved bit-exactly
u32 total_spline_length_bits           # float, preserved bit-exactly
i32 last_knot
u32 curr_dist_bits                     # float, preserved bit-exactly
u32 next_search_dist_bits              # float, preserved bit-exactly
i32 search_scan
u16 degree
u16 depth

Array<Vector<float>> control_verts     # each element is 12 bytes
SimpleArray<float>   knots             # each element is 4 bytes
SimpleArray<float>   spline_knots      # each element is 4 bytes
SimpleArray<float>   weights           # each element is 4 bytes
Array<Vector<float>> spline_verts      # each element is 12 bytes
Array<Vector<float>> spline_normals    # each element is 12 bytes
```

Each nested array independently uses this grammar:

```text
i32 length
if length != 0:
    i32 capacity
    i16 increment
    u8  flags                         # writer clears bit 0x40
    u8  elements[length * element_size]
```

These arrays do **not** repeat capacity/increment. PE order matters: although
PDB declaration order places `weights` at `+156` before `spline_knots` at
`+184`, `SplineData::walk_data` writes `spline_knots` first and `weights`
second. The parser follows the executable.

`River` is 696 bytes in the PDB, but its walker owns only the
`creation_spline` pointer marker and, conditionally, that spline's body.
Creation distance, width, sections, fractal state, render state, masks, and
the other River members consume no bytes here.

## Logical next owner

After the River call, `WalkDataGame::walk_data` performs only non-walking work:
it processes the network daemon and updates a splash-screen string. Its next
call with the same `DataWalk*` is `Terrain::walk_coord_data` at `0x00850ef0`.
Therefore `RiversSection.end` is the logical first Terrain byte for a valid
input image. It is not an installed offset until a valid predecessor boundary
is available.

## Installed specimen and offline corpus audit

The installed SVX SHA-256 is
`161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7`.
Its exact CommandManager prefix ends at `0x2cb7a`; that byte is `0xff`, where
the repeated package tag requires `0x00`. Parsing past the disagreement only
produces a negative package size later. Consequently this work does not
reinterpret `0x2cb7a` (or any later byte) as `Rivers.length`.

An offline search found exactly one SVX below `ron-data`. The 64-file RCX
corpus contains no length-prefixed save magic at stream offset zero. Eighteen
recordings contain one UTF-16 `RoNMultiSave` occurrence each, every occurrence
immediately preceded by the String length 12; none contains `RoNSave`,
`RoNCTWSave`, or `RonCTWMapSave`. These are recorder header strings, not save
containers: `RecordGame::write_header` at `0x00952a50` calls
`Game::walk_data` and then `String::walk_data(game.info.save_name)` and
returns. It never calls `WalkDataGame::walk_data`, so the replay corpus does
not provide an embedded River specimen.

## PE evidence

| body | VA | bytes | SHA-256 |
|---|---:|---:|---|
| `PtrArray<River>::walk_data` | `0x004a2f80` | 843 | `fe26891065951acd4cc4344cd2fda3a6110c16b5406461bb592b1f77d6530f1c` |
| `River::walk_data` | `0x00883950` | 181 | `b4183fb4eaaa413b887b6acd5ba0c3f0ea27b7f557fe99c9bdfef3e117b5707f` |
| `SplineData::walk_data` | `0x009132b0` | 127 | `f5eb1132763a792b60a0d5f8e32043d814bcd308afd44faf1f7b24ba39695d35` |
| `Array<Vector<float>>::walk_data` | `0x004a46d0` | 488 | `69a9c55e0a532f52a0d40a4df7a2f4e93241fefe83426f9a08fba833c06cf790` |
| `SimpleArray<float>::walk_data` | `0x00490b10` | 464 | `a75d884c0c4fbb8062305ef785364a612b6c4bf6538f9b49e0d7693b6d944394` |
| caller transition through next data owner | `0x005a2ff5` | 68 | `00970fee6129066661741cb61442a53c36fccff528eddb94e33a3ecae111326b` |
| next `Terrain::walk_coord_data` | `0x00850ef0` | 128 | `eb243a029738c82e65614da956f4788efc024dcee8d2855e70db36d590bedb22` |
| `RecordGame::write_header` | `0x00952a50` | 233 | `7925ad68827762228d3a2faedf8a892aaceaacd21446029a9680f3ae9a2565e0` |

The matched executable SHA-256 is
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`.

## PDB evidence and gates

The matched PDB/schema SHA-256 values are
`334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`
and
`399aeac3bfb7a201a907e49e51388bb4e010420a54ccb781b335828315f28f14`.
The frozen records cover `PtrArray<River>`, the full flattened 696-byte
`River`, the full flattened 268-byte `SplineData`, `Vert3Array`,
`Array<Vector<float>>`, and `SimpleArray<float>`, including their base-class
layouts. The deterministic layout receipt is
`727ee2df32d73d9dd279cf7e3ec8a365d4491f352fa2a0cf7fe87b8f3f0cc354`.

The synthetic gate exercises sparse outer pointers, null and present creation
splines, all six nested arrays, empty and nonempty arrays, signed history
values, float bit patterns, the PDB/PE authorities, and a following Terrain
sentinel. It mutates every owned byte, rejects every truncation, proves every
following byte is excluded, kills bad counts/capacities/flags/presence values
and mismatched repeated histories, preserves the installed predecessor
contradiction, and audits the full RCX corpus for false save specimens.

## Reproduction

```sh
python3 re/scripts/test_savegame_rivers.py
python3 re/scripts/savegame_rivers.py FILE --offset OFFSET
```

An isolated worktree without copied retail assets can run all artifact gates
with `DON_RETAIL_ROOT=/path/to/don`.
