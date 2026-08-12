# Retail save boundary: direct `GameAccess::game_random[+0x0..+0x4)`

Status: **measured, complete direct owner**. This isolated lane begins at the
exact `0x28024` end of the top-level GameDaemon range, decodes the complete
four-byte `Random` object, and stops at `GraphicEvents::walk_data`. It does not
edit the shared save parser.

The helper is `re/scripts/savegame_game_random.py`; its exact gates are in
`re/scripts/test_savegame_game_random.py`.

## Exact fresh-SVX splice

The fresh save has compressed SHA-256
`161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7`
and decompressed SHA-256
`fa31f89f9bf435e28847e9e54d62cfe712e6bafc6cedb9ab98c8ab6730adb7c8`.

| owner | decompressed range | bytes | SHA-256 |
|---|---:|---:|---|
| direct `GameAccess::game_random[+0,+4)` | `0x28024..0x28028` | 4 | `df3f619804a92fdb4057192dc43dd748ea778adc52bc498ce80524c014b81119` |
| next owner | begins at `0x28028` | dynamic | `GraphicEvents::walk_data` |

The fresh serialized seed is zero. This is an exact specimen fact; it is not
substituted with the top-level `GameInfo.seed`, and it is not inferred from a
replay or from current runtime state.

## Exact PE caller and owner transition

The shipped executable SHA-256 is
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`.
The top-level caller emits:

```text
005a2ddb  mov  ecx, dword ptr [0x00c06184]  ; GameAccess::game_random : Random&
005a2de1  mov  edx, dword ptr [ebx]         ; DataWalk vtable
005a2de3  lea  eax, [ecx + 4]               ; exclusive end
005a2de6  push eax
005a2de7  push ecx                           ; begin at +0
005a2de8  mov  ecx, ebx
005a2dea  call dword ptr [edx]               ; walk_function(begin,end)
005a2dec  push ebx
005a2ded  call 0x008e4d70                    ; GraphicEvents::walk_data
```

The exact 17-byte `game_random` body at `0x005a2ddb` has SHA-256
`5717106e2cf73f745b667649c2e2dacb542ca64c7c3c823b51dc186b2942753d`.
The immediately following six-byte GraphicEvents invocation has SHA-256
`2faf0e0afb3a8f0001e512e8773d314e07238018e61e74602fa7f0cd062b7adf`.
No profiling call, tag, padding, or other DataWalk owner lies between them.

The PDB identifies `0x00c06184` as the public static reference
`GameAccess::game_random : Random&`. The referenced concrete object is the PDB
global `game_random` at `0x00e37a8c`; this is the simulation RNG, distinct from
the unrelated `internal_random` global.

The complete next function, `GraphicEvents::walk_data` at `0x008e4d70`, is 802
bytes with SHA-256
`c319fcf5923d34b6c41d7dd0e0b4d11aa91f71c8fae342bebabf4e664f85b4bb`.
This freezes the next native owner without claiming any of its bytes here.

## Complete PDB representation

The matched PDB SHA-256 is
`334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`;
the extracted `schema/pdb-types.json` SHA-256 is
`399aeac3bfb7a201a907e49e51388bb4e010420a54ccb781b335828315f28f14`.

The PDB says `sizeof(Random) == 4` and provides exactly one field:

| PDB field | object range | stream representation |
|---|---:|---|
| `random_seed` | `+0x0..+0x4` | `unsigned long`, one little-endian `u32` |

Thus the executable's `[ecx,ecx+4)` range serializes the complete object—not a
guessed prefix—and there is no excluded tail or padding. The helper validates
the class size, field name, offset, size, and unsigned type before decoding.
Its deterministic layout-receipt SHA-256 is
`9c30aeac9604d442a2ad76d10e4a959d91e537402ced6403f6188fadc09ea346`.

## Mutation, truncation, and independence gates

The synthetic suite mutates every owned byte independently and proves the
unsigned seed and digest both change. All four truncations fail closed.
Mutating the first byte of GraphicEvents leaves the result identical. A PDB
mutation from `unsigned long` to `long` is rejected, and the direct caller,
next call site, and next full walker are PE-hash frozen.

The installed-artifact gate chains World, GameDaemon, then game_random using
each predecessor's returned boundary. It separately parses the SVX
`GameInfo.seed == 0x014810ac` and RCX `GameInfo.seed == 0x007f93e0`; neither is
confused with the serialized zero `Random.random_seed`, and no save/replay
state is joined.

## Reproduction

```sh
python3 re/scripts/test_savegame_game_random.py

python3 re/scripts/savegame_game_random.py \
  "ron-data/savegames/new save game 2026.08.11 15'42'57 (Tue).SVX" \
  --offset 0x28024
```

Use `--json` for the unsigned decimal and hexadecimal seed. The returned end is
the exact first byte consumed by `GraphicEvents::walk_data`.
