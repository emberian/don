# Retail save boundary: `GraphicEvents::walk_data`

Status: **measured, complete dynamic owner**. This isolated lane begins at the
exact `0x28028` end of the direct game RNG owner and decodes every phase of
`GraphicEvents::walk_data`. It stops at `Scene::walk_data`; no shared parser or
host-state file is changed.

The helper is `re/scripts/savegame_graphic_events.py`; its dynamic, mutation,
truncation, PE/PDB, fresh-SVX, and RCX-independence gates are in
`re/scripts/test_savegame_graphic_events.py`.

## Exact fresh-SVX splice

The fresh save's compressed SHA-256 is
`161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7`;
its decompressed SHA-256 is
`fa31f89f9bf435e28847e9e54d62cfe712e6bafc6cedb9ab98c8ab6730adb7c8`.

| owner | decompressed range | bytes | SHA-256 |
|---|---:|---:|---|
| `GraphicEvents::walk_data` | `0x28028..0x2bc4d` | 15,397 | `0efb92616dbc71df8cdb506eeae773870929781508c8d341f4d1f965b2e87497` |
| next owner, `Scene::walk_data` | begins at `0x2bc4d` | dynamic | excluded |
| complete fresh empty Scene image | `0x2bc4d..0x2bc76` | 41 | `9e1736c43d19118e6ce4302118af337109491ecc52757dfb949bad6a7940b0c2` |

Every byte in this fresh GraphicEvents image is zero. That does **not** make it
padding: it consists of one zero tag, 15,368 absent event-slot markers, four
zero-length array words, and three zero missile-offset words.

The runtime slot count is explicit input to the parser because retail does not
write it. In this installed fresh game it is 15,368. Retail computes the loop
limit in `GraphicEvents::init` and `walk_data` as
`GraphicPieces.first_ammo_piece + ammo_names.length`; `first_ammo_piece` is the
PDB field at `GraphicPieces+0xdc0`, and `ammo_names` is the PDB global
`ObjectArray<String>` at `0x00c0a368`. The installed `effects_graphics.xml`
contains 233 `AMMO` rows. The independently decoded Scene grammar consumes the
next 41 bytes exactly and lands at the caller's following FarmStruct tag at
`0x2bc76`, rejecting a shorter or longer event presence plane.

The helper therefore requires `slot_count`; it never scans for a nonzero byte,
guesses from remaining length, or substitutes a replay count.

## Complete executable grammar

The shipped PE SHA-256 is
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`.
`GraphicEvents::walk_data` is the 802-byte body at `0x008e4d70`, SHA-256
`c319fcf5923d34b6c41d7dd0e0b4d11aa91f71c8fae342bebabf4e664f85b4bb`.
For save mode its complete order is:

```text
u8  GraphicEvents tag                # StringTable[3575], byte offset 0x1174c

for slot in 0 .. runtime_slot_count:
    u8 root_present
    if root_present:
        repeat:
            for kind in 0 .. 38:
                PtrArray<GraphicEvent> events[kind]
            i8 civ
            i8 age
            u8 next_present
        while next_present

SimpleArray<unsigned short> entrench_who
SimpleArray<int>            entrench_o
SimpleArray<float>          entrench_angle
Array<AmbienceStruct>       ambience_structs
i32 missile_offset_x
i32 missile_offset_y
i32 missile_offset_z
```

The `loaded` and `pre_load_gpieces` arrays at GraphicEvents `+32` and `+60` are
runtime graphics caches and are not called by this walker. The walker invokes
only the four arrays at `+88`, `+116`, `+144`, and `+172`, then directly walks
`[+200,+212)`. Thus no object padding or fields `+212..sizeof(GraphicEvents)`
enter the save.

The top-level caller invokes GraphicEvents at `0x005a2dec` and next invokes
`Scene::walk_data` at `0x005a2e06`; the 33-byte caller span beginning at
`0x005a2dec` has SHA-256
`79992f5d6c2fa17a0d3ddaab7c4b92b8532bfc9849111dd7df3bedb2b324d115`.
The next owner body is `Scene::walk_data`, VA `0x008c0f70`, size 255, SHA-256
`27df0d70180debfaf27edbb748d786c55979c8b2257832585da17929487714e4`.

## Pointer-array and GraphicEvent grammar

Each of the 38 EventGroup arrays is a retail
`PtrArray<GraphicEvent>::walk_data` image:

```text
i32 length
if length != 0:
    i32 capacity
    i16 increment
    u8  flags                        # writer clears bit 0x40
    u8  presence[length]
    i32 repeated_capacity
    i16 repeated_increment
    for each present row:
        i32 event_type               # GraphicEvent +4..+8
        if event_type == 8:
            bytes GraphicEvent +8..+27
        else:
            bytes GraphicEvent +8..+36
```

The pointer-array body at `0x004aa490` is 843 bytes, SHA-256
`28e28b6ff57648318ccd4a9da4d391024ed7a2ad3d31b9f1396f5b2503494aa3`.
It serializes allocation history and a boolean presence plane, repeats
capacity/increment after that plane, allocates present rows on load, and then
dispatches their bodies. It never serializes pointers.

`GraphicEvent::walk_data` at `0x00919780` is 54 bytes, SHA-256
`2482f3cc01863fb7b065bda8a4efb12baac8e6b76605aadb53da09ff839a3239`.
The PDB gives `sizeof(GraphicEvent)==36` and a vptr at `+0`; the executable
excludes that vptr. It first walks `event_type` at `+4..+8`, then selects
`+8..+27` for event type 8 or `+8..+36` for every other type. The shorter type-8
image is 23 bytes total; the ordinary image is 32 bytes total.

## PDB ownership

The matched PDB SHA-256 is
`334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`;
the extracted schema SHA-256 is
`399aeac3bfb7a201a907e49e51388bb4e010420a54ccb781b335828315f28f14`.

The helper freezes and validates the compiler layouts for:

- `GraphicEvents` (220 bytes), including the exact four saved arrays and
  `[+200,+212)` missile offsets;
- `EventGroup` (1,072 bytes): 38 `PtrArray<GraphicEvent>` objects at `+0`,
  signed-byte `civ`/`age` at `+1064/+1065`, and linked `next` at `+1068`;
- `PtrArray<GraphicEvent>` (28 bytes) and its six history/runtime fields;
- `GraphicEvent` (36 bytes, vptr-bearing) and all conditional-range fields;
- each trailing SimpleArray type, `Array<AmbienceStruct>`, and the 24-byte
  `AmbienceStruct` whose saved raw element includes its single padding byte.

The deterministic layout receipt is
`ec970c7071509946a605359595292b3c614c5faaca94ca0ff95f365da1ebbeb7`.
The supporting executable bodies are also frozen: SimpleArray<unsigned short>
`0x00476610`, SimpleArray<int> `0x00473120`, SimpleArray<float> `0x00490b10`,
and Array<AmbienceStruct> `0x004aa9a0`.

## Dynamic mutation and truncation proof

The synthetic fixture covers three runtime root slots: one two-node linked
EventGroup chain, one absent root, and one single-node root. The three live
groups activate pointer-array kinds 0, 7, and 37. Each active array has mixed
present/absent rows and includes both the short type-8 GraphicEvent branch and
the full non-8 branch. All four trailing arrays exercise nonempty and empty
histories, including two 24-byte AmbienceStruct rows.

Every owned byte is mutated independently. Each mutation either fails a strict
tag/count/history/presence rule or changes the exact section object and digest.
Every truncation fails. Dedicated gates reject non-boolean root, linked-next,
and row-presence markers, inconsistent repeated histories, writer-retained flag
bit `0x40`, bad runtime counts, and a mutated PDB size. Mutating the first Scene
byte leaves the GraphicEvents result identical.

The installed-artifact gate chains World → GameDaemon → game_random →
GraphicEvents using only returned boundaries. It separately checks SVX seed
`0x014810ac` and RCX seed `0x007f93e0`; no replay bytes or state are used to
decode the save.

## Reproduction

```sh
python3 re/scripts/test_savegame_graphic_events.py

python3 re/scripts/savegame_graphic_events.py \
  "ron-data/savegames/new save game 2026.08.11 15'42'57 (Tue).SVX" \
  --offset 0x28028 --slot-count 15368
```

The returned end is the exact first byte owned by `Scene::walk_data`.
