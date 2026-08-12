# Retail save boundary: `TileSet::walk_data`

Status: **measured, exclusive parser proof**. This lane starts at the exact
`0x2629c` end returned by the Types parser, consumes the complete TileSet
walker, and stops before Mountains. It does not hook the shared save parser,
join the unrelated-seed replay, or embed retail bytes.

The reusable helper is `re/scripts/savegame_tileset.py`; its exhaustive
mutation/truncation suite and optional user-owned artifact splice check are in
`re/scripts/test_savegame_tileset.py`.

## Exact splice

For the fresh save with compressed SHA-256
`161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7`
and decompressed SHA-256
`fa31f89f9bf435e28847e9e54d62cfe712e6bafc6cedb9ab98c8ab6730adb7c8`:

| owner | decompressed range | bytes | SHA-256 |
|---|---:|---:|---|
| `TileSet::walk_data` | `0x0002629c..0x000262a1` | 5 | `8855508aade16ec573d21e6a485dfd0a7624085c1a14b5ecdd6485de0c6839a4` |
| next owner, excluded | begins `0x000262a1` | — | `Mountains::walk_data` `0x0089d320` |

The five-byte image is `00 00 00 00 00`: tag `0x00`, followed by the
little-endian `u32` zero length of an empty current-tileset String. This is a
walk-derived boundary, not a scan for zeroes.

## Complete PE and PDB body

The shipped PE SHA-256 is
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`.
Capstone over the PDB-sized 212-byte `TileSet::walk_data` body at
`0x0087b290` establishes the complete serialization:

1. `0x0087b2ab..0x0087b2c1` loads `int_str_array->list + 0x20c60` and calls
   `DataWalk::walk_test`. PDB says `String` is 20 bytes, so this is StringTable
   index `0x20c60 / 20 = 6712`; `walk_test` emits its one-byte module id.
2. `0x0087b2c4..0x0087b2eb` constructs an empty local String and copies the
   global `STR_MODULE_ID` at `0x00cc2311` into its `module_id` byte.
3. `0x0087b2ee..0x0087b2fb` copies the current tileset name into that local
   String when `tilesets.cur_tileset` is non-null.
4. `0x0087b300..0x0087b304` passes the local String and the same `DataWalk *`
   to `String::walk_data` `0x00a1b2d0`.
5. `0x0087b309..0x0087b345` is load-only reconciliation: on a read walk it
   compares the loaded name, flushes the old tileset if needed, and calls
   `TileSet::load_tileset`. It emits no additional stream bytes.
6. `0x0087b345..0x0087b361` destroys the local String and returns.

The matching PDB SHA-256 is
`334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`,
GUID `51D4F219-61C6-4F84-9D5B-C3361B0D291F`, age 1. It identifies global
`tilesets` at `0x00e885d0`, `TileSet::cur_tileset` at `+0x20`, and
`TileSetData::name` as a 20-byte String at `+0`. It also gives String's
`curr_len` as a `u16` at `+0x08` and `module_id` at `+0x0b`.

Capstone of `String::walk_data` shows that a save widens `curr_len` to a
little-endian `u32`, emits those four bytes, and, only when nonzero, emits
exactly `curr_len * 2` bytes from the selected UTF-16 buffer. Therefore the
full TileSet grammar is:

```text
u8 tag; u32 current_name_code_units; u16le[current_name_code_units]
```

Finally, the `WalkDataGame::walk_data` caller invokes TileSet at `0x005a29fb`.
Its intervening renderer/load reconciliation has no `DataWalk *`; the next
stream-owning call is Mountains at `0x005a2a3d`.

## Mutation and provenance proof

The synthetic suite changes every owned byte independently. A tag mutation
fails; length mutations either change the returned extent or fail bounds; each
payload mutation changes the decoded field and digest. Every truncation from
zero through one byte short fails, while changing the first Mountains byte is
excluded. Separate cases reject impossible writer lengths and malformed
UTF-16, and prove the empty String is exactly five bytes.

When the user-owned artifacts are installed, the suite derives `0x2629c` by
running Leaders and Types first, then proves the TileSet end is `0x262a1`. It
also separately confirms the SVX seed `0x014810ac` differs from the RCX seed
`0x007f93e0`; no identity or payload is joined across them.

## Reproduction

```sh
python3 re/scripts/test_savegame_tileset.py

python3 re/scripts/savegame_tileset.py \
  "ron-data/savegames/new save game 2026.08.11 15'42'57 (Tue).SVX" \
  --offset 0x2629c
```

The returned `end` is the exact splice for the next exclusive Mountains lane.
