# Retail save boundary: Docks

Status: **complete save-mode eight-owner container**. This lane begins at
`Docks::walk_data`, consumes its tag, all eight independent `PtrArray<Dock>`
histories and presence planes, and every complete generic-save Dock row, then
stops before `OilWells::walk_data`. It does not edit the shared parser,
normalize history, use the checksum projection as the save image, or join SVX
and RCX state.

The exclusive helper is `re/scripts/savegame_docks.py`; exhaustive tests are
in `re/scripts/test_savegame_docks.py`.

## Fresh-SVX splice

The user-created SVX has compressed SHA-256
`161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7` and
decompressed SHA-256
`fa31f89f9bf435e28847e9e54d62cfe712e6bafc6cedb9ab98c8ab6730adb7c8`.
Chaining the exclusive parsers from Leaders through Forts reaches:

| stream range | exact owner | fresh value |
|---|---|---:|
| `0x270d6..0x270d7` | Docks `walk_test(StringTable[2622])` | 0 |
| `0x270d7..0x270db` | `lists[0].length` | 0 |
| `0x270db..0x270df` | `lists[1].length` | 0 |
| `0x270df..0x270e3` | `lists[2].length` | 0 |
| `0x270e3..0x270e7` | `lists[3].length` | 0 |
| `0x270e7..0x270eb` | `lists[4].length` | 0 |
| `0x270eb..0x270ef` | `lists[5].length` | 0 |
| `0x270ef..0x270f3` | `lists[6].length` | 0 |
| `0x270f3..0x270f7` | `lists[7].length` | 0 |
| next owner at `0x270f7` | `OilWells::walk_data` `0x0073f890` | excluded |

The exact 33-byte Docks image is all zero and has SHA-256
`7f9c9e31ac8256ca2f258583df262dbc7d6f68f2a03043d5c99a4ae5a7396ce9`.
The separate RCX SHA-256 is
`558e0cd53dbed4f820e8757c0327a58384d433e5beb8e9eeef5d64df4d67bd54`.
The test retains independent SVX seed `0x014810ac` and RCX seed `0x007f93e0`.

## Tag and eight sparse owners

`Docks::walk_data` `0x00741100` emits the tag at StringTable byte offset
`0xccd8`, exact index 2622, then loops over eight `PtrArray<Dock>` globals at
`0x00c0a580..0x00c0a65f`. PDB `Docks::lists` is exactly
`PtrArray<Dock>[8]`, size 224.

Each owner has the usual exact pointer-array grammar:

```text
i32 length
if length != 0:
    i32 capacity
    i16 increment
    u8  flags                       # writer clears bit 0x40
    u8  pointer_present[length]
    i32 repeated_capacity
    i16 repeated_increment
    for each present pointer, in slot order:
        save-mode Dock body
```

Both history images must agree but remain separately represented. The helper
rejects invalid length/capacity/flags/presence/history and never compacts holes
or supplies host defaults. `cur_index` is not walked.

## Save-mode Dock body and non-memory order

`Dock::walk_data` `0x00740c40` does not walk its ten data bytes as one range.
Its exact generic-save order is:

```text
i16 dock                         # memory +0
i16 o                            # memory +2
i16 reg                          # memory +4
i8  dock_flags                   # memory +8
i8  who                          # memory +9
i16 gull_o                       # memory +6; walked last
```

The first call walks `[Dock+0,Dock+6)`, and the second walks
`[Dock+8,Dock+10)`. It then tests PDB `DataWalk::checksum` at visitor `+8`.
SaveGame and LoadGame use zero, so a third call walks `[Dock+6,Dock+8)` and
puts `gull_o` last in the stream. On input, retail subsequently resets the
in-memory `gull_o` to `-1`; that post-walk mutation does not remove its bytes
from the load image. A checksum visitor has nonzero checksum and omits this
last field, but that distinct projection is not used to infer save bytes.

PDB `DockData` is ten bytes in memory order with `gull_o` before the two chars.
PDB `Dock` is 20 bytes: bytes 10..11 are alignment padding and its virtual-base
pointer begins at +12. Neither padding nor the object tail is serialized.

## Exact next owner

After Docks and profiling, the main caller invokes `OilWells::walk_data`
`0x0073f890`. OilWells begins with its own tag at StringTable byte offset
`0x18c2c`, exact index 5071. Docks therefore ends at fresh `0x270f7`.

## Receipts and mutation proof

The matching PDB SHA-256 is
`334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`;
its JSON export SHA-256 is
`399aeac3bfb7a201a907e49e51388bb4e010420a54ccb781b335828315f28f14`.
The helper requires the exact 232/28/20/10-byte Docks, pointer-array, Dock,
and DockData layouts plus DataWalk's checksum member at +8. The PDB layout
receipt is `f8a25e54511a849b51d99a8ff9d5e4cd3316b3a4fc33e5cead47e3efe72b1a74`.

The matched PE SHA-256 is
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`.
Tests freeze Docks, Dock, the main caller handoff, and the next OilWells owner.

The nonempty fixture contains all eight owners; owner 0 has length 3,
presence `[1,0,1]`, nondefault repeated history, and two complete rows.
Every owned-byte mutation fails an invariant or changes the receipt and
digest, every truncation is rejected, and tests explicitly prove the
non-memory field order, `gull_o` inclusion, object-tail exclusion, exact next
boundary, and SVX/RCX independence.

## Reproduction

```sh
python3 re/scripts/test_savegame_docks.py

python3 re/scripts/savegame_docks.py \
  "ron-data/savegames/new save game 2026.08.11 15'42'57 (Tue).SVX" \
  --offset 0x270d6
```

The returned `end` is the exact start of `OilWells::walk_data`.
