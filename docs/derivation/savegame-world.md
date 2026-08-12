# Retail save boundary: World

Status: **complete `World::walk_data(..., -1)` section**. This isolated
tranche begins at the World tag after HotKeyGroups, follows every selector
phase activated by -1, and stops before the caller's direct GameDaemon range.

## Fresh-SVX splice

The fresh SVX compressed/decompressed SHA-256 values are
`161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7` and
`fa31f89f9bf435e28847e9e54d62cfe712e6bafc6cedb9ab98c8ab6730adb7c8`.
HotKeyGroups ends at `0x27f53`. World is exactly `0x27f53..0x27ffc` (169
bytes), SHA-256
`605d47a6802a6ba6675ce2970606011e1d53eebdd846effd6f47bd0903d7ed13`.
The fresh values and all six coordinate-array lengths, four Terrain-array
lengths, and all size-driven planes are zero. The next byte at `0x27ffc` is
owned by the caller's direct `GameDaemon[0..40)` walk and is excluded.

`World::walk_data` `0x006b5cf0` emits StringTable byte offset `0xe470`, index
2924. The caller invokes it with selector -1 at `0x005a2db1`; every guarded
phase therefore runs. On return, `0x005a2dc0..0x005a2dcf` performs the next
visitor call over the GameDaemon global, proving the boundary independently
of the all-zero specimen.

## Exact phase order

The PE traversal is:

```text
u8  World tag                          # StringTable[2924]
i32 xs, ys                             # World +0..+8

SimpleArray<WCoord> start_x
SimpleArray<WCoord> start_y
SimpleArray<WCoord> start_city_x
SimpleArray<WCoord> start_city_y
SimpleArray<WCoord> oil_x
SimpleArray<WCoord> oil_y

i32 WorldData[+8..+128)                # 30 scalars
bytes WData[0..size)[+0..+21)          # 21 bytes per world cell
bytes TData[0..tile_size)               # 2 bytes per tile
u8 seen[fog_size], seen2[], seen3[]
u8 wcoord_seen[size]
i32 danger[8][reg_size]
CollBlock branch[size]

Array<WCoordData> Terrain.halfland_locs
SimpleArray<int> Terrain.halfland_types
SimpleArray<int> Terrain.halfland_subtypes
SimpleArray<int> Terrain.nuke_hits
```

PDB `WorldData` is 364 bytes, `WorldOut` 368, and `World` 372. The direct
+8..+128 range supplies `size`, `fog_size`, `tile_size`, and `reg_size`; those
exact values control all following pointer-owned byte counts. Pointers,
`DynamicBitMask start_city_locs`, and virtual/output state are never serialized
as raw addresses.

All simple arrays use `i32 length`; when nonzero, this is followed by `i32
capacity`, `i16 increment`, writer-cleared `u8 flags`, and contiguous payload.
WCoord payload elements are 4 bytes, WCoordData 8, and int 4. Empty arrays
contain only the length, preserving retail history rather than normalizing it.

PDB `WData` is 28 bytes. World walks only +0..+21 for each cell, excluding
padding and the +24 `CollBlock*`. It later serializes that pointer as an
independent signed 32-bit boolean. When present, it walks `CollBlock.bits` and
`size` (+0..+8), then exactly `size` bytes from +12. The runtime +8 flags word
is recreated on load and is not in the stream. PDB storage bounds the payload
to 96 bytes and bits to 768.

The final four arrays live in PDB `Terrain` at +19328, +19356, +19384, and
+19412. Their native walkers are `Array<WCoordData>::walk_data` `0x00478990`
and `SimpleArray<int>::walk_data` `0x00473120`. The six World coordinate
arrays use `SimpleArray<WCoord>::walk_data` `0x0047c660`.

## Evidence and gates

The matching PE, PDB, and schema SHA-256 values are
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`,
`334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`,
and `399aeac3bfb7a201a907e49e51388bb4e010420a54ccb781b335828315f28f14`.
The PDB layout receipt is
`353dad0d87212ade4f9a9fd768eb7bd025a0dbefdbf8e65be05fc2b2c805134b`.
Tests freeze World, all three array walkers, and the next caller handoff.

The nonempty synthetic fixture exercises nonzero history, all size/fog/tile/
region planes, absent and present CollBlocks, and every Terrain array form.
Every owned-byte mutation is rejected or changes the exact receipt, every
truncation is rejected, and independent gates cover tag, capacity, flags,
dimensions, CollBlock bounds, PDB mutation, and next-byte exclusion.

The installed test chains every landed helper from Leaders at `0x9a5a` to
World. SVX seed `0x014810ac` and RCX seed `0x007f93e0` remain independent; RCX
SHA-256 is
`558e0cd53dbed4f820e8757c0327a58384d433e5beb8e9eeef5d64df4d67bd54`.

## Reproduction

```sh
python3 re/scripts/test_savegame_world.py

python3 re/scripts/savegame_world.py \
  "ron-data/savegames/new save game 2026.08.11 15'42'57 (Tue).SVX" \
  --offset 0x27f53
```
