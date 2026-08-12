# Retail save boundary: direct `GameDaemon[+0x00..+0x28)`

Status: **measured, complete direct owner**. This isolated lane begins at the
exact `0x27ffc` end of `World::walk_data(-1)`, decodes the caller's complete
40-byte GameDaemon range, and stops before the separate direct
`GameAccess::game_random` walk. It does not edit the shared save parser.

The exclusive helper is `re/scripts/savegame_game_daemon.py`; its gates are in
`re/scripts/test_savegame_game_daemon.py`.

## Exact fresh-SVX splice

The specimen is the fresh save with compressed SHA-256
`161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7`
and decompressed SHA-256
`fa31f89f9bf435e28847e9e54d62cfe712e6bafc6cedb9ab98c8ab6730adb7c8`.

| owner | decompressed range | bytes | SHA-256 |
|---|---:|---:|---|
| direct `GameDaemon[+0x00..+0x28)` | `0x27ffc..0x28024` | 40 | `2c34ce1df23b838c5abf2a7f6437cca3d3067ed509ff25f11df6b11b582b51eb` |
| next owner, excluded: `game_random[+0x0..+0x4)` | `0x28024..0x28028` | 4 | `df3f619804a92fdb4057192dc43dd748ea778adc52bc498ce80524c014b81119` |

All ten included signed words are zero in this fresh specimen. That is a
specimen fact, not a reason to infer or normalize values in any replay.

## Exact PE caller proof

The shipped executable SHA-256 is
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`.
Its `WalkDataGame::walk_data` caller emits:

```text
005a2dc0  mov  ecx, dword ptr [0x00c061bc]  ; GameAccess::game_daemon : GameDaemon&
005a2dc6  mov  edx, dword ptr [ebx]         ; DataWalk vtable
005a2dc8  lea  eax, [ecx + 0x28]            ; exclusive end
005a2dcb  push eax
005a2dcc  push ecx                           ; begin at +0x00
005a2dcd  mov  ecx, ebx
005a2dcf  call dword ptr [edx]               ; walk_function(begin,end)
```

The exact 17-byte body at `0x005a2dc0` has SHA-256
`915eb356f30b313a78116fd4865dba1b53eb1fd2f0eb82434abd5c3081f0159e`.
There is no tag, loop, presence flag, or nested `GameDaemon::walk_data` call.
This is an unconditional raw range owned by the top-level caller.

After a profiling call that receives no DataWalk pointer, the caller separately
loads `GameAccess::game_random` from `0x00c06184` and submits `[+0,+4)` at
`0x005a2ddb`. That separate 17-byte caller body has SHA-256
`5717106e2cf73f745b667649c2e2dacb542ca64c7c3c823b51dc186b2942753d`.
Consequently the byte at stream offset `0x28024` cannot belong to GameDaemon.

## Complete PDB representation

The matched PDB SHA-256 is
`334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`;
the extracted `schema/pdb-types.json` SHA-256 is
`399aeac3bfb7a201a907e49e51388bb4e010420a54ccb781b335828315f28f14`.
The PDB says `sizeof(GameDaemon) == 0x2c`:

| PDB field | object range | stream representation |
|---|---:|---|
| `repaths` | `+0x00..+0x20` | `int[8]`, eight little-endian signed words |
| `empty_colls` | `+0x20..+0x24` | one little-endian signed word |
| `borders` | `+0x24..+0x28` | one little-endian signed word |
| `busy` | `+0x28..+0x2c` | **excluded by the caller's end pointer** |

The first three fields cover every byte of `[+0,+0x28)` monotonically, with no
gap, overlap, or padding. The first excluded field begins exactly at the caller
end. The helper validates all sizes, types, offsets, field names, and that
excluded-field identity before parsing. Its deterministic layout-receipt
SHA-256 is
`506b314ae1911e57fcd96ee6e14aeec6f4b8ff7a756799e94053780d6ba4b4c1`.

## Mutation, truncation, and independence gates

The synthetic suite mutates each of the 40 owned bytes independently and proves
that exactly one named PDB field changes, together with the section digest. All
40 truncations fail closed. Mutating the first byte of `game_random` leaves the
GameDaemon result identical. A deliberately overlapped PDB field layout is
rejected, and both exact PE caller bodies are hash-frozen.

The installed-artifact gate parses World at its independently recovered fresh
offset, obtains `World.end == 0x27ffc`, and splices GameDaemon from that returned
boundary. It separately parses the SVX seed `0x014810ac` and RCX seed
`0x007f93e0`; no save bytes or state are inferred from the replay.

## Reproduction

```sh
python3 re/scripts/test_savegame_game_daemon.py

python3 re/scripts/savegame_game_daemon.py \
  "ron-data/savegames/new save game 2026.08.11 15'42'57 (Tue).SVX" \
  --offset 0x27ffc
```

Use `--json` for the three named fields and all ten exact signed values. The
returned end is the first byte of the separate `game_random` owner.
