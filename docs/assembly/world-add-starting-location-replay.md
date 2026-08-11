# `World::add_starting_location` replay continuation

This lane executes the first East Meets West start append selected by the
already-admitted `Map::place_start_in_region` body. Evidence is the supported
`riseofnations.exe` (SHA-256 `30478a44…625079`), its matching shipped PDB,
direct i386 disassembly, Ghidra decompilation, the existing forked retail
oracle, and both checksum-bearing style-19 headers. No VM or live process is
used.

The PDB symbol at `0x006b2de0` is
`?add_starting_location@World@@QAEHABVWCoord@@0@Z`: an `int __thiscall`
method taking X and Y by const `WCoord&`. The complete 570-byte body ends after
the `ret 8` at `0x006b3017..0x006b3019`; `0x006b301a` is the exclusive end.
It performs no RNG call.

The body appends X and Y to the two player-start arrays, returning the original
start-array length. It then appends the starting-city footprint in this exact
order:

| array | appended values |
|---|---|
| `start_city_x` | `x, x-1, x, x-1` |
| `start_city_y` | `y, y, y-1, y-1` |

Finally it sets four row-major, LSB-first occupancy bits for `(x,y)`,
`(x-1,y)`, `(x,y-1)`, `(x-1,y-1)`. The four `SimpleArray<WCoord>` values are
walked in World section 2; `start_city_locs` is native state but is not emitted
by `World::walk_data`. The receipt therefore records both the walked checksum
transition and each unwalked byte/mask mutation. Empty retail World arrays have
capacity 0 and increment -1, so this first call grows every array to capacity
4 exactly as the already-oracle-backed sim writer does.

East Meets West calls the body at `0x00697461`. It returns at `0x00697466`.
The following instructions store X and Y in caller stack arrays
(`0x0069746c`, `0x00697473`), increment stack-local counters, and jump at
`0x0069747f` to the player-loop head `0x00696d28`. The continuation now
executes that bookkeeping and the same exact writer for every remaining active
slot. It freezes before the post-loop `Map::check_player_land` call at
`0x00697492` / callee `0x0068ef00`.

## Frozen style-19 receipts

| replay | input / return | appended footprint | full World | section 2 |
|---|---:|---|---:|---:|
| 2018-12-01 | `(26,56) / 0` | X `[26,25,26,25]`, Y `[56,56,55,55]` | `0xb2eeff92 → 0x35db0949` | `0x00100001 → 0xbfa809a9` |
| 2019-03-24 | `(87,78) / 0` | X `[87,86,87,86]`, Y `[78,78,77,77]` | `0xeb4960c6 → 0xfb5e6c0d` | `0x00100001 → 0xfcbd0b48` |

For 2018 the ordered occupancy writes are flat indices
`5626, 5625, 5526, 5525`, producing masks `0x04, 0x02, 0x40, 0x20` in bytes
703 and 690. For 2019 they are `7887, 7886, 7787, 7786`, producing masks
`0x80, 0x40, 0x08, 0x04` in bytes 985 and 973. Both calls leave the selector's
final RNG word unchanged and change no walked section except StartArrays.

The adapter preflights the selector success edge, parallel-array lengths,
native growth metadata, coordinates, and occupancy storage against a clone.
Malformed state or a selector failure returns without changing World. The
integrated selector failure still stops at `0x00697003` and never invokes this
writer.

## Exact residual

The first append receipt remains independently frozen at caller return
`0x00697466`. Its owner continuation, documented in
`docs/assembly/east-meets-west-remaining-starts-replay.md`, now executes all
remaining active-player selectors and appends and freezes before
`Map::check_player_land`. No post-loop mutation is claimed.

## Validation

- local fresh-source append suite: 3/3, including both real style-19 headers;
- local selector/edge/continent/centroid integration suites: 15/15;
- local reconstruction/owner-transition suites: 6/6;
- local full-corpus localizer: coherent owner ledgers 21/21, same-group peer
  comparisons 265,619/265,619, and the exact measured census is 13,119,476
  walked / 7,917,024 owned / 5,202,452 unknown bytes;
- Persvati clean-HEAD overlay compile
  `replay-add-start-check-v2-20260811T204209Z-29161-25203-90ecb6faccea`;
- Persvati clean-HEAD overlay synthetic suite
  `replay-add-start-tests-v2-20260811T204221Z-29370-16928-90ecb6faccea`:
  both asset-independent tests pass 2/2. The ignored replay corpus is not a
  remote overlay asset, so the two real receipts are gated locally.
