# Retail save boundary: `Mountains::walk_data`

Status: **measured, exclusive parser proof**. This lane starts at the exact
`0x262a1` end returned by the TileSet parser, consumes the complete Mountains
walker, and stops before the caller's direct Constants range. It does not edit
the shared parser, embed retail bytes, or infer any identity across the
different-seed SVX and RCX.

The helper is `re/scripts/savegame_mountains.py`; exhaustive synthetic tests
and the optional user-owned chained splice are in
`re/scripts/test_savegame_mountains.py`.

## Exact splice

For the fresh save with compressed SHA-256
`161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7`
and decompressed SHA-256
`fa31f89f9bf435e28847e9e54d62cfe712e6bafc6cedb9ab98c8ab6730adb7c8`:

| owner | decompressed range | bytes | SHA-256 |
|---|---:|---:|---|
| `Mountains::walk_data` | `0x000262a1..0x000262b2` | 17 | `0a88111852095cae045340ea1f0b279944b2a756a213d9b50107d7489771e159` |
| next owner, excluded | begins `0x000262b2` | — | direct `Constants[+0x000..+0xd40)` walk at caller `0x005a2a68` |

The 17 bytes are a zero tag and four zero signed lengths. An empty retail
array writes only its four-byte length; its capacity, increment, flags, and
payload are absent. This boundary follows the executable call graph and is not
a scan through the save's long zero run.

## Complete Mountains body

The shipped PE SHA-256 is
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`.
Capstone over the PDB-sized 117-byte body at `0x0089d320` gives five and only
five serializer calls:

| call | executable target | PDB-resolved MountainsData field |
|---|---|---|
| `walk_test` `0x0089d339` | `int_str_array->list + 0x18178` | StringTable index `0x18178 / 20 = 4934` |
| `0x0089d34c` | `SimpleArray<WCoord>::walk_data` `0x0047c660` | `mountain_loc_wcoords_x` `+0x68` |
| `0x0089d361` | `SimpleArray<WCoord>::walk_data` `0x0047c660` | `mountain_loc_wcoords_y` `+0x84` |
| `0x0089d376` | `Array<Vector<float>>::walk_data` `0x004a46d0` | `mountain_locs` `+0xa0` |
| `0x0089d38b` | `SimpleArray<int>::walk_data` `0x00473120` | `mountain_types` `+0xbc` |

The field offsets above are relative to the `MountainsData` virtual base. The
instructions load the vbtable displacement from `mountains+0x04` and add it to
the four corresponding global field anchors; this is why the executable uses
absolute addresses `0x00e85fcc`, `0x00e85fe8`, `0x00e86004`, and
`0x00e86020` rather than a simple `this+offset` sequence.

The matching PDB SHA-256 is
`334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`,
GUID `51D4F219-61C6-4F84-9D5B-C3361B0D291F`, age 1. It gives
`sizeof(MountainsData) = 228`, both WCoord elements as four-byte signed values,
`Vector<float>` as three consecutive four-byte floats, and `mountain_types`
elements as four-byte signed integers.

## Exact array grammar

Capstone and decompilation of all three concrete template walkers agree. Each
array emits:

```text
i32 length
if length != 0:
    i32 capacity
    i16 increment
    u8  flags_with_bit_0x40_cleared
    u8  payload[length * element_size]
```

The WCoord and integer arrays use `element_size = 4` and make one contiguous
payload walk. `Array<Vector<float>>` uses `element_size = 12` and makes one
12-byte walk per element, which is the same byte stream without padding. The
parser preserves every payload word as raw `u32` bits, so floating-point NaN
canonicalization or host conversion cannot erase evidence.

The fresh save has all four lengths zero, hence `1 + 4*4 = 17` bytes. After the
Mountains return at caller `0x005a2a3d`, profiling calls carry no DataWalk. The
next actual stream operation loads `GameAccess::constants` from `0x00c061f0`
and walks `[constants+0x000, constants+0xd40)` at `0x005a2a68`; that direct
range, not Armies later in the caller, owns byte `0x262b2`.

## Mutation and provenance proof

The synthetic fixture gives every array a nonempty history and payload. The
suite mutates every owned byte independently; each mutation either fails a
structural invariant or changes the returned typed image and digest. Every
truncation fails, while a mutation at the first Constants byte is excluded.
Dedicated cases reject the wrong tag, negative/absurd lengths, capacity below
length, and the writer-cleared flag bit.

With the user-owned artifacts installed, the suite chains Leaders, Types,
TileSet, then Mountains to derive `0x262b2`. It separately verifies the SVX
seed is `0x014810ac` and the RCX seed is `0x007f93e0`; no state or identity is
joined between them.

## Reproduction

```sh
python3 re/scripts/test_savegame_mountains.py

python3 re/scripts/savegame_mountains.py \
  "ron-data/savegames/new save game 2026.08.11 15'42'57 (Tue).SVX" \
  --offset 0x262a1
```

The returned end is the exact start for a future exclusive Constants-prefix
lane.
