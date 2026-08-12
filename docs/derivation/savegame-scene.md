# Retail save boundary: `Scene::walk_data`

Status: **measured, complete dynamic owner**. The isolated helper
`re/scripts/savegame_scene.py` starts at GraphicEvents' exact returned end and
stops at the following caller-owned FarmStruct tag. Tests are in
`re/scripts/test_savegame_scene.py`; no shared parser is changed.

## Fresh splice

For the fresh SVX (compressed SHA-256
`161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7`,
decompressed SHA-256
`fa31f89f9bf435e28847e9e54d62cfe712e6bafc6cedb9ab98c8ab6730adb7c8`):

| owner | range | bytes | SHA-256 |
|---|---:|---:|---|
| `Scene::walk_data` | `0x2bc4d..0x2bc76` | 41 | `9e1736c43d19118e6ce4302118af337109491ecc52757dfb949bad6a7940b0c2` |
| next FarmStruct caller tag | begins `0x2bc76` | 1 | excluded; fresh value `0x04` |

The fresh Scene is structurally empty: tag zero, two zero-bit/zero-size masks,
five zero-length arrays, and `last_time == 0`. All 41 bytes are nevertheless
owned by the executable grammar.

## Complete grammar and PE proof

`Scene::walk_data`, VA `0x008c0f70`, is 255 bytes with SHA-256
`27df0d70180debfaf27edbb748d786c55979c8b2257832585da17929487714e4`:

```text
u8 Scene tag                         # StringTable[5508], byte offset 0x1ae50
for flags, draw_flags:
    i32 bits
    i32 payload_size
    u8  payload[payload_size]        # BitMask<32> embedded storage
SimpleArray<Coord>          ping_x
SimpleArray<Coord>          ping_y
SimpleArray<unsigned char>  ping_who
SimpleArray<unsigned char>  ping_timer
SimpleArray<unsigned long>  ping_finish_frame
i32 last_time                       # Scene +0x304..+0x308
```

Each nonempty SimpleArray uses the exact history
`length:i32, capacity:i32, increment:i16, flags:u8`, followed by
`length * sizeof(element)` bytes. Writer bit `0x40` is cleared before the flags
byte is saved. The two BitMask walkers serialize `bits,size`, then exactly
`size` bytes from embedded `ptr+0`; their runtime `flags` field is excluded.
The parser enforces `0 <= bits <= 32`, `0 <= size <= 4`, and `bits <= 8*size`.

The top-level caller's Scene-to-next-owner span at `0x005a2e06` is frozen for
31 bytes, SHA-256
`41b5dfde1c33d872dce4cb5c07faff7eda21bdfb4abe5a974fa97f6a55f836dd`.
After Scene and two profiling calls, it emits the next tag from StringTable,
then begins the separate Farms/FarmStruct owner. No Scene bytes occur after
`last_time`.

Supporting array bodies are frozen at exact PDB lengths:

- `SimpleArray<Coord>::walk_data`, `0x00483cc0`, 448 bytes, SHA-256
  `8bbc954c4648e00ac6ea44442c0c06aacaa173b418ff0fabdc29a4d0a9b59ff0`;
- `SimpleArray<unsigned char>::walk_data`, `0x0049a090`, 463 bytes, SHA-256
  `3e5c4a5c4ae86dc1056d1b34f25333812f2b7379d06907b214a8f18bd919be56`;
- `SimpleArray<unsigned long>::walk_data`, `0x004a7f80`, 463 bytes, SHA-256
  `4b10c0597f4be29fa3d75973fc31befb0f832b26196a720248dc59e294457b72`.

## PDB ownership

The matched PDB SHA-256 is
`334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`;
the extracted schema SHA-256 is
`399aeac3bfb7a201a907e49e51388bb4e010420a54ccb781b335828315f28f14`.
It proves `sizeof(Scene)==824`, `flags`/`draw_flags` at `+484/+500`, the five
arrays at `+560,+588,+616,+644,+672`, and `last_time` at `+772`. It also proves
the 16-byte BitMask and all three 28-byte array representations. The helper's
deterministic layout receipt is
`e0f695fdb0a0d7c2c29702d9bfa42848ae031152e018edeb89a8d6af3a50a8c2`.

## Falsification gates

The synthetic fixture exercises both nonempty masks (including a full 32-bit
mask), nonempty and empty arrays of every element width, signed histories, and
a nonzero signed `last_time`. Every owned byte mutation either fails a strict
rule or changes the parsed object and digest; every truncation fails. Dedicated
tests reject oversized masks, retained flag bit `0x40`, a bad tag, and a
mutated PDB size. Mutation of the next FarmStruct tag is excluded.

The installed gate chains World → GameDaemon → game_random → GraphicEvents →
Scene solely through returned boundaries and separately verifies the SVX and
RCX seeds (`0x014810ac` versus `0x007f93e0`). No replay state is used to decode
the save.

## Reproduction

```sh
python3 re/scripts/test_savegame_scene.py
python3 re/scripts/savegame_scene.py \
  "ron-data/savegames/new save game 2026.08.11 15'42'57 (Tue).SVX" \
  --offset 0x2bc4d
```

The returned end is the exact first byte of the following FarmStruct owner.
