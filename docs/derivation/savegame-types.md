# Retail save boundary: `Types::walk_data`

Status: **measured, source-only parser proof**. This lane continues the fresh
2026-08-11 retail `.SVX` at the exact end returned by the Leaders parser and
consumes the complete `Types::walk_data` traversal. It does not join the save to
the unrelated-seed replay and does not embed retail bytes.

The reusable proof is `re/scripts/savegame_types.py`. Its exhaustive synthetic
mutation tests and optional user-owned fresh-artifact splice check are
`re/scripts/test_savegame_types.py`; no retail bytes are embedded.

## Exact splice

For the fresh save with compressed SHA-256
`161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7`
and decompressed SHA-256
`fa31f89f9bf435e28847e9e54d62cfe712e6bafc6cedb9ab98c8ab6730adb7c8`:

| owner | decompressed range | bytes | SHA-256 |
|---|---:|---:|---|
| `Types::walk_data` | `0x00026247..0x0002629c` | 85 | `faabcd32d202c89e15b2530cb87dae68370357360133e712eef2ded2f1d6532e` |
| next owner, excluded | begins `0x0002629c` | — | `TileSet::walk_data` `0x0087b290` |

There is no `walk_test` tag inside `Types::walk_data`. In particular, the first
byte happens to be `0xee`, but it is `techtypes[544]->leader_off`, not another
`LeaderData` tag. The boundary is established by the executable traversal, not
by scanning the specimen for a marker.

## Independent PE, PDB, and Capstone evidence

The shipped PE has SHA-256
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`.
Capstone decoding of `Types::walk_data` `0x00669780..0x006697c0` gives the
complete loop:

- `0x00669788`: initialize the byte offset to `0x880`;
- `0x00669790`: load the global at `0x00c061b0`;
- `0x00669797`: load its `+0x10` pointer;
- `0x0066979a`: load one pointer at that byte offset;
- `0x0066979d..0x006697ad`: call `DataWalk::walk_function` on
  `[pointer+0x1e2, pointer+0x1e3)`;
- `0x006697af`: add four to the byte offset;
- `0x006697b2..0x006697b8`: repeat while the offset is less than `0x9d4`.

The matching PDB has GUID `51D4F219-61C6-4F84-9D5B-C3361B0D291F`, age 1,
and SHA-256
`334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`.
It names `0x00c061b0` as the `GameAccess::techtypes` reference of type
`PtrArray<TechType>&` and describes `TechType+0x1e2` as the one-byte
`unsigned char leader_off`. The executable itself performs the `+0x10` list
load. Therefore the loop writes exactly `(0x9d4 - 0x880) / 4 = 85` bytes for
TechType indices `544..628`, inclusive.

Capstone decoding of the caller `WalkDataGame::walk_data` shows the preceding
call to `Leaders::walk_data` at `0x005a2998`, the Types call at `0x005a299e`,
and the next call that receives the same `DataWalk *` at `0x005a29fb`, targeting
`TileSet::walk_data`. The intervening load-only finalization calls receive no
`DataWalk *` and own no stream bytes.

## Fresh specimen facts

The 85 `leader_off` values contain 73 zeroes. The nonzero values, keyed by
TechType index, are:

| TechType index | `leader_off` |
|---:|---:|
| 544 | `0xee` |
| 545 | `0x07` |
| 548 | `0x02` |
| 553 | `0x08` |
| 561–568 | `0xff` |

No value whitelist is imposed: PDB says the field is an unsigned byte, so all
256 values are structurally legal. The synthetic suite instead mutation-checks
every one of the 85 consumed bytes, proves a mutation changes only its exact
indexed field and section digest, kills every truncation from 0 through 84
retained bytes, and proves a mutation at the first `TileSet` byte is excluded.
When the user-owned artifacts are installed, the same suite starts from the
Leaders parser's returned `0x26247` boundary and separately confirms that the
SVX seed `0x014810ac` is not the RCX seed `0x007f93e0`.

## Reproduction

The suite embeds no retail artifact and skips its fresh-artifact splice check
when the user-owned files are absent:

```sh
python3 re/scripts/test_savegame_types.py
```

Against the local fresh save:

```sh
python3 re/scripts/savegame_types.py \
  "ron-data/savegames/new save game 2026.08.11 15'42'57 (Tue).SVX" \
  --offset 0x26247
```

The returned `end` is the exact splice a future shared save parser hook can
adopt after the Leaders lane is integrated.
