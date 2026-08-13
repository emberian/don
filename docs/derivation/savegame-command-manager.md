# Retail save boundary: `CommandManager::walk_data`

Status: **complete PE/PDB grammar; maximal exact installed prefix**. The helper
`re/scripts/savegame_command_manager.py` parses a structurally valid complete
CommandManager owner and separately freezes the longest prefix the installed
SVX proves. That specimen contradicts the retail grammar at FIFO 5 / package
12, so this lane does not invent a complete installed boundary. Exhaustive
gates live in `re/scripts/test_savegame_command_manager.py`.

## Complete retail grammar

`CommandManager::walk_data` emits two tags, the local package, and all eight
20-slot per-player FIFOs:

```text
u8 walk_test(StringTable[574])
u8 walk_test(StringTable[623])
CommandPackage local_package             # no additional package tag

for player in 0..8:
    u8 walk_test(StringTable[5206])
    i32 front
    i32 front_local
    i32 length
    i32 length_local
    for slot in 0..20:
        u8 walk_test(StringTable[623])
        CommandPackage packages[slot]

CommandPackage:
    u32 stamp
    i32 play
    i32 valid
    i32 group
    i16 size
    u8  data[size]                       # 0 <= size <= 512
```

The minimum valid owner is 3,196 bytes. Its maximum is 85,628 bytes when the
local package and every FIFO slot contain all 512 payload bytes. The next
owner in `WalkDataGame::walk_data` is `PtrArray<River>::walk_data`; its stream
offset is variable because CommandManager payload sizes are variable.

## Exact installed prefix and contradiction

The fresh SVX compressed/decompressed SHA-256 values are
`161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7` and
`fa31f89f9bf435e28847e9e54d62cfe712e6bafc6cedb9ab98c8ab6730adb7c8`.

| proven tranche | stream range | bytes | SHA-256 |
|---|---:|---:|---|
| manager tag, StringTable[574] | `0x2c2b0..0x2c2b1` | 1 | `6e340b9cffb37a989ca544e6bb780a2c78901d3fb33738768511a30617afa01d` |
| local-package tag, StringTable[623] | `0x2c2b1..0x2c2b2` | 1 | `6e340b9cffb37a989ca544e6bb780a2c78901d3fb33738768511a30617afa01d` |
| local package, zero payload | `0x2c2b2..0x2c2c4` | 18 | `60daa3a5f7dbfa200f8c82840ecf5b42640b70f3b7218a4c6bbd67db542e75a4` |
| complete FIFOs 0..4 | `0x2c2c4..0x2ca85` | 1,985 | `67a94f093536915a7b7748f190352cad1118ff123cd9cedea3845392c2c97237` |
| FIFO 5 tag and header | `0x2ca85..0x2ca96` | 17 | `0a88111852095cae045340ea1f0b279944b2a756a213d9b50107d7489771e159` |
| FIFO 5 packages 0..11 | `0x2ca96..0x2cb7a` | 228 | `f727a034d8cf235962503b7e239b52168b232bb285eb2aa38f17acbc4d1b5529` |
| maximal exact prefix | `0x2c2b0..0x2cb7a` | 2,250 | `35a5a2477ca191574974e3bc44f3453f69d71136231a9c12c92532b6d06d2269` |
| FIFO 5 / package 12 tag | begins `0x2cb7a` | observed `0xff` | expected `0x00` |

The contradiction is not a guessed semantic invariant. Every package tag in
this body uses the same String object, StringTable[623]. Its first value and
all preceding repetitions are `0x00`; the byte at `0x2cb7a` is `0xff`.
Disabling tag checks only delays failure: FIFO 5 / package 16 then presents
signed `size = -1` at `0x2cbd7`. The retail body sign-extends that nonzero
value and submits `[data,data+size)` to the visitor. `SaveGame::walk_function`
passes the resulting byte count to `gzwrite`; `-1` is not a zero-payload
sentinel and cannot describe a valid saved package.

Therefore the installed specimen does not prove a complete CommandManager
owner or the following River offset. It may be a damaged/incomplete save, but
that cause is not asserted without a second specimen or successful retail
load evidence.

## PE evidence

| body | VA | bytes | SHA-256 |
|---|---:|---:|---|
| `CommandManager::walk_data` | `0x00942d30` | 222 | `bfe3ff7f3e2bc9468aeb7187bf71bc2ee15281b73dae6e189a672d2478d2cb96` |
| `PackageFifo::walk_data` | `0x00952500` | 129 | `606e9e4708c029224c4d74d82cfca60d56a4c33267fd3ccdb2a8aa80290db706` |
| `SaveGame::walk_function` | `0x0043d730` | 259 | `98f6ddc9eeab25a905f8c1dfaf39df1bcac09a310ea5e68f87da0afb8685d5f9` |
| `LoadGame::walk_function` | `0x0043d950` | 259 | `5b215f6e2e6f891fcea759c0741eb23e015e56e842312bbd65eb9d96b0829612` |
| `gzwrite` | `0x00509350` | 157 | `0ae50e79f5b3dfdc5e589ed51d86388fc3579286f03b0da3a73ef93ecde42e4a` |
| caller transition | `0x005a2fd5` | 44 | `7cd889c60efcd81abd32445f3893c1c6aab4fcbbef35850c9089900494525565` |
| next `PtrArray<River>::walk_data` | `0x004a2f80` | 843 | `fe26891065951acd4cc4344cd2fda3a6110c16b5406461bb592b1f77d6530f1c` |

The matched executable SHA-256 is
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`.

## PDB ownership and exclusions

The matched PDB/schema SHA-256 values are
`334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`
and
`399aeac3bfb7a201a907e49e51388bb4e010420a54ccb781b335828315f28f14`.
PDB layout fixes CommandManager at 86,632 bytes, each PackageFifo at
10,736 bytes, and each CommandPackage at 536 bytes. The eight FIFOs begin at
CommandManager `+576`; each contains a 16-byte header and 20 packages.

The walkers deliberately exclude CommandManager's stamps, wait state,
session state, playback state, vbptr, and virtual base. They exclude every
CommandPackage payload byte after `size`, the two bytes between payload and
`padding`, and the four-byte `Random padding` member. Nevertheless all 20
package headers are walked for every FIFO, regardless of FIFO length.

The deterministic layout receipt is
`6d1f90af707c8dc294103ff12c8a364c92d682122633bf5846818d4c4cabd74c`.

## Gates

The complete synthetic owner exercises all eight FIFOs and 160 packages,
zero and nonzero payloads, signed headers, nondefault FIFO indices, all three
tag sites, and a following River sentinel. Tests mutate every owned byte,
reject every truncation, negative and oversized payload, wrong tag, and
mutated PDB size, freeze the seven PE bodies above, and prove every following
byte is excluded. The installed gate chains every owner from World through
Options, freezes the 2,250-byte prefix, and requires the exact first tag
contradiction plus an independent RCX seed.

## Reproduction

```sh
python3 re/scripts/test_savegame_command_manager.py
python3 re/scripts/savegame_command_manager.py \
  "ron-data/savegames/new save game 2026.08.11 15'42'57 (Tue).SVX" \
  --offset 0x2c2b0 --installed-prefix
```

An isolated worktree without copied retail assets can point the installed
gate at a populated checkout with `DON_RETAIL_ROOT=/path/to/don`.
