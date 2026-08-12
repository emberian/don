# Retail save boundary: `UnbuiltWonders::walk_data`

Status: **measured complete dynamic owner**. The isolated helper
`re/scripts/savegame_unbuilt_wonders.py` begins at the exact end returned by the
Farms parser, consumes the owner tag and all eight player arrays, and stops at
`UnbuiltCities::walk_data`. Its exhaustive gates are in
`re/scripts/test_savegame_unbuilt_wonders.py`; no shared parser is changed.

## Exact fresh boundary

The fresh SVX compressed/decompressed SHA-256 pair is
`161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7` /
`fa31f89f9bf435e28847e9e54d62cfe712e6bafc6cedb9ab98c8ab6730adb7c8`.

| owner | decompressed range | bytes | SHA-256 |
|---|---:|---:|---|
| `UnbuiltWonders::walk_data` | `0x2bc8d..0x2bcae` | 33 | `7f9c9e31ac8256ca2f258583df262dbc7d6f68f2a03043d5c99a4ae5a7396ce9` |
| next `UnbuiltCities::walk_data` | `0x2bcae..0x2bcce` | 32 | `66687aadf862bd776c8fc18b8e9f8e20089714856ee233b3902a591d0d5f2925` |

The 33 zero bytes are structured: the zero tag for StringTable index 7082 and
eight zero array-length words. UnbuiltCities has no tag and begins directly
with its first length word. Mutating that next byte leaves this parse unchanged.

## PE-derived grammar

The full 550-byte owner body is VA `0x0073c290`, SHA-256
`2f39ca65083926b449a36b5b40f06d086ec707e9b9e9436b06e18e0129d50234`:

```text
u8 tag                                  # walk_test StringTable[7082]
repeat player = 0..7:
    i32 length
    if length != 0:
        i32 capacity
        i16 increment
        u8  flags                       # retail writer clears bit 0x40
        repeat length times:
            i16 object_id               # UnbuiltWonder.o
            i8  who
```

The owner iterates eight adjacent 28-byte arrays from global
`unbuilt_wonders+0` through `+224`. For each row the PE submits `[row,row+3)`
to DataWalk and advances the backing pointer by four. Thus the logical save row
is three bytes from a four-byte in-memory class; its final alignment byte is
excluded. This is an `Array<T>` value container: history appears once and there
is no pointer-presence plane or repeated header.

The next owner body is `UnbuiltCities::walk_data`, VA `0x00460dc0`, 533 bytes,
SHA-256 `fcb7592505bc0864a54da846796768b6cbf2cae2ffb76413cee811b6248dcdf0`.
The caller invokes Wonder, the non-serializing `NetDaemon::process_all`, then
City in the 22-byte span at `0x005a2e6b`, SHA-256
`b1aaad6a61d08d9481307df99772f9a0d8ff683b9d0c9b4741cc54f748f9bf94`.
`process_all` itself is frozen at VA `0x00951300`, 269 bytes, SHA-256
`130612cbfdab81986a51b53395ecf0a1496904de784461472b5fbb9ede75c18c`.

The matched PE SHA-256 is
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`.

## PDB proof and mutation gates

The matched PDB/schema SHA-256 values are
`334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5` and
`399aeac3bfb7a201a907e49e51388bb4e010420a54ccb781b335828315f28f14`.
The helper freezes `UnbuiltWonders` as eight `Array<UnbuiltWonder>` objects in
224 bytes, the complete 28-byte array history layout, and `UnbuiltWonder` as a
four-byte class whose named fields end at `+3`. Its deterministic layout receipt
is `9d51184a7cd1e152ec9d78e19070f71efa723815809a8ed8bf73ee22ca424f73`.

The synthetic image has mixed empty/nonempty lists, distinct histories, nine
rows, signed `who` values, and distinct object IDs. Every owned byte is mutated;
each mutation is rejected or changes the immutable result and section digest.
Every truncation fails. Dedicated gates reject tag, negative length, capacity
below length, retained flag `0x40`, and a PDB row-size mutation. The installed
gate chains every predecessor from World through Farms by returned boundaries,
lands at `0x2bcae`, and independently checks SVX/RCX seeds `0x014810ac` and
`0x007f93e0` without using replay data to decode the save.

## Reproduction

```sh
python3 re/scripts/test_savegame_unbuilt_wonders.py
python3 re/scripts/savegame_unbuilt_wonders.py \
  "ron-data/savegames/new save game 2026.08.11 15'42'57 (Tue).SVX" \
  --offset 0x2bc8d
```

The returned end is the exact first word consumed by `UnbuiltCities`.
