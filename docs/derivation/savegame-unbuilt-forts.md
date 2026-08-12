# Retail save boundary: `UnbuiltForts::walk_data`

Status: **measured complete dynamic owner**. The isolated helper
`re/scripts/savegame_unbuilt_forts.py` begins at the exact Cities end, decodes
the Forts tag and all eight player arrays, and stops before
`ConquestGame::walk_data`. Exhaustive gates live in
`re/scripts/test_savegame_unbuilt_forts.py`; no shared parser is changed.

## Exact installed boundary

The fresh SVX compressed/decompressed SHA-256 values are
`161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7` and
`fa31f89f9bf435e28847e9e54d62cfe712e6bafc6cedb9ab98c8ab6730adb7c8`.

| owner | range | bytes | SHA-256 |
|---|---:|---:|---|
| `UnbuiltForts::walk_data` | `0x2bcce..0x2bcef` | 33 | `7f9c9e31ac8256ca2f258583df262dbc7d6f68f2a03043d5c99a4ae5a7396ce9` |
| next `ConquestGame::walk_data` | begins `0x2bcef` | dynamic | excluded |

The 33 zero bytes are the Forts tag from StringTable index 7081 followed by
eight zero array-length words. Mutating the first Conquest byte leaves the
Forts result identical.

## Complete PE grammar

The full `UnbuiltForts::walk_data` body is VA `0x0073bcc0`, 550 bytes, SHA-256
`2900e5550e6339347e9e9c2a63ccbe46dc5cbb87ce98287f4f08bbe38b17a7f8`:

```text
u8 tag                                  # StringTable[7081], expected zero
repeat player = 0..7:
    i32 length
    if length != 0:
        i32 capacity
        i16 increment
        u8  flags                       # writer clears bit 0x40
        repeat length times:
            i16 object_id               # UnbuiltFort.o
            i8  who
```

The body visits eight adjacent 28-byte arrays. It submits only `[row,row+3)` to
DataWalk and advances memory by four, excluding the final alignment byte of
each `UnbuiltFort`. This value array records allocation history once, with no
pointer presence plane and no repeated header.

The next body is `ConquestGame::walk_data`, VA `0x00798410`, 957 bytes,
SHA-256 `c8a3801f2ef2c4affb5014b2e400bff4f7b5ede2c34daba15a1903dfe6569c54`.
The caller's Forts → `NetDaemon::process_all` → Conquest transition is the
22-byte span at `0x005a2e8b`, SHA-256
`ce1b4b205d00217f559f43e97106f9368cad60af1b8fc7d55753c6a4ce9f9f30`.
The intervening non-serializing process body is VA `0x00951300`, 269 bytes,
SHA-256 `130612cbfdab81986a51b53395ecf0a1496904de784461472b5fbb9ede75c18c`.

The matched PE SHA-256 is
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`.

## PDB and exhaustive gates

The matched PDB/schema SHA-256 values are
`334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5` and
`399aeac3bfb7a201a907e49e51388bb4e010420a54ccb781b335828315f28f14`.
The helper freezes `UnbuiltForts` as eight `Array<UnbuiltFort>` objects in 224
bytes, the complete 28-byte array layout, and the four-byte row class whose
named fields end at `+3`. Its deterministic layout receipt is
`2e5382f0a1d14cdedbd68652afd0fb04fd601590472c108babb2801be9511ea9`.

The dynamic fixture exercises mixed empty/nonempty arrays, distinct histories,
signed owners, and multiple rows. Every owned byte is independently mutated;
each mutation rejects or changes the immutable result and digest. Every
truncation fails. Dedicated gates reject the tag, negative length, capacity
below length, retained flag `0x40`, and a PDB row-size mutation. The installed
gate chains World through Cities by returned boundaries, lands at `0x2bcef`,
and independently verifies SVX/RCX seeds `0x014810ac` and `0x007f93e0`.

## Reproduction

```sh
python3 re/scripts/test_savegame_unbuilt_forts.py
python3 re/scripts/savegame_unbuilt_forts.py \
  "ron-data/savegames/new save game 2026.08.11 15'42'57 (Tue).SVX" \
  --offset 0x2bcce
```

The returned end is the exact first byte owned by `ConquestGame::walk_data`.
