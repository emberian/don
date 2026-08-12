# Retail save boundary: `UnbuiltCities::walk_data`

Status: **measured complete dynamic owner**. The isolated helper
`re/scripts/savegame_unbuilt_cities.py` begins at the exact boundary returned
by UnbuiltWonders, consumes all eight City player arrays, and stops before the
tag owned by `UnbuiltForts::walk_data`. Tests are in
`re/scripts/test_savegame_unbuilt_cities.py`; no shared parser is changed.

## Exact boundary and the absent tag

The installed fresh SVX has compressed SHA-256
`161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7` and
decompressed SHA-256
`fa31f89f9bf435e28847e9e54d62cfe712e6bafc6cedb9ab98c8ab6730adb7c8`.

| owner | decompressed range | bytes | SHA-256 |
|---|---:|---:|---|
| `UnbuiltCities::walk_data` | `0x2bcae..0x2bcce` | 32 | `66687aadf862bd776c8fc18b8e9f8e20089714856ee233b3902a591d0d5f2925` |
| `UnbuiltForts::walk_data` | begins `0x2bcce` | dynamic | excluded |

Unlike the adjacent Wonders and Forts owners, the City body makes no
`walk_test`/tag call. Its first byte is the low byte of `lists[0].length`, not a
tag. The 32 fresh zero bytes are exactly eight zero `i32` length words. Forts
begins with its own zero tag at `0x2bcce`; mutating it leaves Cities unchanged.

## Complete executable grammar

`UnbuiltCities::walk_data` is the complete 533-byte PE body at VA `0x00460dc0`,
SHA-256 `fcb7592505bc0864a54da846796768b6cbf2cae2ffb76413cee811b6248dcdf0`:

```text
repeat player = 0..7:
    i32 length
    if length != 0:
        i32 capacity
        i16 increment
        u8  flags                       # retail writer clears bit 0x40
        repeat length times:
            i16 object_id               # UnbuiltCity.o
            i8  who
```

The owner loops from global `unbuilt_cities+0` through `+224` in 28-byte array
steps. Its row loop walks `[row,row+3)` and advances the backing storage by four:
each serialized row is the three-byte named prefix of a four-byte class, with
the trailing alignment byte excluded. Allocation history occurs once per live
value array; there is no pointer presence plane or duplicated history.

The following body is tagged `UnbuiltForts::walk_data`, VA `0x0073bcc0`, 550
bytes, SHA-256
`2900e5550e6339347e9e9c2a63ccbe46dc5cbb87ce98287f4f08bbe38b17a7f8`.
Its tag source is StringTable index 7081. The caller's City → non-serializing
`NetDaemon::process_all` → Fort transition is the 22-byte span at `0x005a2e7b`,
SHA-256 `1a8dbfa8302875e5d01b86cacfc3b6a0e9250a4ac24eb7ed2fea6d44a49087f4`.
`process_all` is frozen at `0x00951300`, 269 bytes, SHA-256
`130612cbfdab81986a51b53395ecf0a1496904de784461472b5fbb9ede75c18c`.

The shipped PE SHA-256 is
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`.

## PDB and exhaustive gates

The matched PDB/schema SHA-256 values are
`334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5` and
`399aeac3bfb7a201a907e49e51388bb4e010420a54ccb781b335828315f28f14`.
The helper freezes the 224-byte `UnbuiltCities` eight-array block, the complete
28-byte `Array<UnbuiltCity>` layout, and the four-byte `UnbuiltCity` whose named
`o`/`who` fields end at `+3`. Its deterministic layout receipt is
`1213ce8631c86989a20b38565630c10b3ba669b5ee1640f0e3baf2ee726c60ed`.

The dynamic fixture exercises mixed empty/nonempty lists, distinct histories,
signed `who` bytes, and multiple rows. Every owned byte is independently
mutated; each mutation rejects or changes the exact result and digest. Every
truncation fails. Separate gates reject negative length, capacity below length,
retained writer flag `0x40`, and a PDB row-size mutation. The installed gate
chains World through Wonders using only returned boundaries, lands exactly at
`0x2bcce`, and independently checks save/replay seeds `0x014810ac` and
`0x007f93e0`; replay state is never used to infer this save owner.

## Reproduction

```sh
python3 re/scripts/test_savegame_unbuilt_cities.py
python3 re/scripts/savegame_unbuilt_cities.py \
  "ron-data/savegames/new save game 2026.08.11 15'42'57 (Tue).SVX" \
  --offset 0x2bcae
```

The returned end is the exact tag byte owned by `UnbuiltForts::walk_data`.
