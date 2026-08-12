# Retail save boundary: Armies

Status: **complete PE/PDB-derived owner**.  This lane begins at the exact
`0x26ffe` end returned by the direct-scalars parser, consumes the complete
`Armies::walk_data` save image, and stops at the first byte owned by
`Cities::walk_data`.  It neither searches for values nor edits the shared save
parser.

The exclusive helper is `re/scripts/savegame_armies.py`; exhaustive tests are
in `re/scripts/test_savegame_armies.py`.

## Fresh-SVX splice

The user-created SVX has compressed SHA-256
`161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7` and
decompressed SHA-256
`fa31f89f9bf435e28847e9e54d62cfe712e6bafc6cedb9ab98c8ab6730adb7c8`.
Chaining the exclusive Leaders, Types, TileSet, Mountains, Constants, and
direct-scalars parsers reaches Armies without a byte search:

| stream range | exact owner | fresh image |
|---|---|---|
| `0x26ffe..0x26fff` | `Armies::walk_test`, `StringTable[133]` | tag `0x00` |
| `0x26fff..0x27003` | `lists[0].length` | 0 |
| `0x27003..0x27007` | `lists[1].length` | 0 |
| `0x27007..0x2700b` | `lists[2].length` | 0 |
| `0x2700b..0x2700f` | `lists[3].length` | 0 |
| `0x2700f..0x27013` | `lists[4].length` | 0 |
| `0x27013..0x27017` | `lists[5].length` | 0 |
| `0x27017..0x2701b` | `lists[6].length` | 0 |
| `0x2701b..0x2701f` | `lists[7].length` | 0 |
| next owner at `0x2701f` | `Cities::walk_data` `0x00735410` | excluded |

The exact 33-byte Armies image is all zero and has SHA-256
`7f9c9e31ac8256ca2f258583df262dbc7d6f68f2a03043d5c99a4ae5a7396ce9`.
The empty-list branch is important: it emits only the four-byte length, so the
fresh save is not evidence for the initialized 16-Army container history.

The separate RCX fixture has SHA-256
`558e0cd53dbed4f820e8757c0327a58384d433e5beb8e9eeef5d64df4d67bd54`.
The test keeps the artifacts independent and confirms the SVX seed
`0x014810ac` differs from the RCX seed `0x007f93e0`; no identity, boundary, or
state is joined across them.

## Complete retail grammar

`Armies::walk_data` is at `0x006f3700` (PDB length 1013).  It first calls
`walk_test` with StringTable byte offset `0xa64`; `sizeof(String) == 20`, so
the tag identity is index 133.  It then walks the global `Armies` object at
`0x00c09700` in eight `0x1c`-byte steps, from `0x00c09700` through but not
including `0x00c097e0`.  The PDB identifies those steps as
`PtrArray<Army> lists[8]`.

For each owner, the stream grammar is:

```text
i32 length
if length != 0:
    i32 capacity
    i16 increment
    u8  flags                    # writer persistently clears bit 0x40
    u8  presence[length]         # every value is exactly 0 or 1
    i32 repeated_capacity
    i16 repeated_increment
    for each slot whose presence byte is 1:
        u8  Army walk-test tag   # StringTable[134]
        i16 valid
        if valid != 0:
            bytes Army[+0x02..+0x98)  # exactly 150 bytes
```

The repeated capacity/increment range is the direct walk of
`[PtrArray+0x08, PtrArray+0x0e)` at `0x006f3a6f`; it must equal the earlier
values in a writer-produced image.  The helper also rejects negative or
unreasonably large lengths, `capacity < length`, the writer-cleared flag bit,
and nonboolean presence bytes.

`Army::walk_data` at `0x006f9850` independently proves each present body:
its walk test uses StringTable byte offset `0xa78`, hence index 134; it always
walks the two-byte `valid`, and only when that value is nonzero walks the
150-byte `[Army+2, Army+0x98)` tail.  The current retail specimen contains no
present Army pointer, so the numeric value of the per-Army save tag is not
guessed.  The helper preserves arbitrary values at the proven tag position;
the synthetic fixture demonstrates that structure with distinct tags.

After `Armies::walk_data` returns, `WalkDataGame::walk_data` calls
`Cities::walk_data` at caller site `0x005a2acf`.  That native caller order, not
a following-byte pattern, defines the exclusive end.

## PDB field image

The matched PDB is SHA-256
`334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`,
GUID `51D4F219-61C6-4F84-9D5B-C3361B0D291F`, age 1.  Its checked JSON export is
SHA-256
`399aeac3bfb7a201a907e49e51388bb4e010420a54ccb781b335828315f28f14`.
The helper loads that export at runtime and refuses any disagreement with
these compiler layouts:

- `sizeof(Armies) == 232`; flattened `lists` is
  `PtrArray<Army>[8]` at `+0`, size 224.
- `sizeof(PtrArray<Army>) == 28`; `length +4`, `size +8`,
  `increment +12`, `list +16`, `flags +20`, and `cur_index +24`.
- `sizeof(Army) == 160`; its flattened `ArmyData` fields cover every byte in
  `[+0,+0x98)` with no gap.  The remaining bytes hold virtual-base machinery
  and are not walked.

The conditional tail is decoded, in PDB order, as:

| Army range | field | type |
|---|---|---|
| `+0x02..+0x04` | `army` | `short` |
| `+0x04..+0x38` | `status`, `reg`, `role`, `num_units`, `num_captains`, `num_standard`, `num_decoys`, `city`, `navy`, `human_frame`, `hurry`, `target_o`, `target_who` | 13 × `int` |
| `+0x38..+0x40` | `x`, `y` | 2 × `Coord` |
| `+0x40..+0x48` | `angle`, `rally_dist` | 2 × `int` |
| `+0x48..+0x50` | `muster_x`, `muster_y` | 2 × `WCoord` |
| `+0x50..+0x54` | `muster_angle` | `int` |
| `+0x54..+0x94` | `list` | `int[16]` |
| `+0x94..+0x98` | `who`, `num_groups` | 2 × `short` |

The canonical layout receipt produced from these three PDB records has
SHA-256
`cfeef26e4db7667fc842ac307239fdebfc12c4339f7c476a74430b9c1309c066`.

## Independent checksum cross-check

The landed Sim Armies implementation is a useful independent comparison, not
the source of this save grammar.  Retail `CheckSum::walk_test` at `0x0041bfe0`
is `ret 4`, so checksum walking emits neither the leading Armies tag nor the
128 per-Army tags.  SaveGame/LoadGame does emit them.

For the independently established post-`Armies::init` shape—eight owners,
sixteen present but invalid Armies per owner—the exact save size is:

```text
1 + 8 * (17 + 16 + 16 * (1 + 2)) = 649 bytes
```

Removing only the one Armies tag and 128 proven per-Army tag bytes produces:

```text
649 - 1 - 128 = 520 bytes
```

The test constructs both byte images independently and proves the projection
equals the landed `INITIAL_ARMIES_WALK_BYTES == 520` authority byte-for-byte.
This also explains why the 520-byte initialized checksum image must not be
used to infer the fresh SVX's 33-byte empty-list image.

## Evidence and mutation coverage

The matched PE is SHA-256
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`.
Tests freeze complete executable spans for `Armies::walk_data` (1013 bytes),
`Army::walk_data` (70 bytes), the checksum walk-test no-op, the main caller's
Armies call, and the following Cities entry.

The synthetic nonempty image covers all dynamic branches: an empty owner; a
nonempty owner with invalid, absent, and live slots; duplicated history; and
the complete named 150-byte tail.  Every owned-byte single-bit mutation either
fails a structural invariant or changes the exact parsed receipt and digest.
Every truncation fails.  A mutation at the first Cities byte leaves the Armies
receipt unchanged.

## Reproduction

```sh
python3 re/scripts/test_savegame_armies.py

python3 re/scripts/savegame_armies.py \
  "ron-data/savegames/new save game 2026.08.11 15'42'57 (Tue).SVX" \
  --offset 0x26ffe

cargo test -p don-replay --test armies_runtime
```

The returned `end` is the exact start of the next native owner,
`Cities::walk_data`.
