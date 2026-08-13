# Retail save boundary: `SelectGroups::walk_data`

Status: **measured complete owner**. The isolated helper
`re/scripts/savegame_select_groups.py` consumes the outer tag and both complete
`Array<SelectGroup>` images, including every live Group's variable-width member
planes. It stops before the following `Options` owner. Exhaustive gates live in
`re/scripts/test_savegame_select_groups.py`; no shared parser is changed.

## Exact installed boundary

The fresh SVX compressed/decompressed SHA-256 values are
`161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7` and
`fa31f89f9bf435e28847e9e54d62cfe712e6bafc6cedb9ab98c8ab6730adb7c8`.

| owner | range | bytes | SHA-256 |
|---|---:|---:|---|
| SelectGroups tag, StringTable[6009] | `0x2c234..0x2c235` | 1 | `6e340b9cffb37a989ca544e6bb780a2c78901d3fb33738768511a30617afa01d` |
| `select_list` zero length | `0x2c235..0x2c239` | 4 | `df3f619804a92fdb4057192dc43dd748ea778adc52bc498ce80524c014b81119` |
| `select_list_2` zero length | `0x2c239..0x2c23d` | 4 | `df3f619804a92fdb4057192dc43dd748ea778adc52bc498ce80524c014b81119` |
| complete SelectGroups owner | `0x2c234..0x2c23d` | 9 | `3e7077fd2f66d689e0cee6a7cf5b37bf2dca7c979af356d0a31cbc5c85605c7d` |
| next Options `Array<Option>.length` | begins `0x2c23d` | 4 | excluded |

The next owner does **not** begin with a tag. `Options::walk_data` first walks
its inherited `Array<Option>`; only afterward does it emit its own tag and
direct images. The parser therefore stops on the exact first byte of that
array's signed length word.

## Complete grammar

The stream starts with `walk_test(StringTable[6009])`, then walks two
positional arrays in order: `select_list`, followed by `select_list_2`.
Each `Array<SelectGroup>` uses the contiguous Array grammar:

```text
i32 length
if length != 0:
    i32 capacity
    i16 increment
    u8  flags                       # writer clears bit 0x40
    SelectGroup row[length]         # no presence plane or repeated history
```

Each SelectGroup row is:

```text
Group direct bytes [Group+4, Group+76)       # 17 i32 + four u8
if Group.num != 0:
    i16 list[num]
    i32 off_x[num]
    i32 off_y[num]
    i32 curr_x[num]
    i32 curr_y[num]
    i8  angles[num]
walk_test(StringTable[6007])
i32 whose, flashing, named, item_ox, good_ox, flash_frame
```

`Group.num` is bounded by the PDB's fixed 128-member arrays. All signed member
IDs, offsets, coordinates, angles, history values, and SelectGroup fields are
preserved exactly.

## PE evidence and next owner

The exact PE bodies are:

| body | VA | bytes | SHA-256 |
|---|---:|---:|---|
| `SelectGroups::walk_data` | `0x00717230` | 57 | `3187e207ece6a4fdc580107bfdd4255e1b6e228295e89b817a40e23b4069d526` |
| `Array<SelectGroup>::walk_data` | `0x00480900` | 545 | `752388b8cbff6cbe86bbb73d62050fa43707d626b9262177e0e4f11dfdb95346` |
| `SelectGroup::walk_data` | `0x007171b0` | 63 | `986eebaf5f23b364dee9c059b1e181984141036bce495c465ef21ba2fbe17a84` |
| `Group::walk_data` | `0x00708400` | 181 | `4b69d8857b8c760dc33641a2b1fc344e76bbac3d03d6bb1bf804bbc74fcd794f` |
| inlined caller owner | `0x005a2f57` | 45 | `ebc2f58ea2e39d1cde84f72f69e1f937aa6b3318f8ce4d469733c3da799d40dd` |
| next `Options::walk_data` | `0x0072c240` | 80 | `5173806c509cd96f5f5b0a9f43a45e188c6d9974f23d6e8a5412bfb5a92d42ed` |
| next `Array<Option>::walk_data` | `0x00480cc0` | 487 | `8af883a3db8cb42c004123fec9e4baac0171f00cb12bb1358695320b4d0b6f29` |

The matched executable SHA-256 is
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`.

## PDB ownership and exclusions

The matched PDB/schema SHA-256 values are
`334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`
and
`399aeac3bfb7a201a907e49e51388bb4e010420a54ccb781b335828315f28f14`.
PDB layout fixes `SelectGroups` at 60 bytes, two 28-byte arrays at `+4` and
`+32`, `SelectGroup` at 2,544 bytes, and its `Group` base at 2,516 bytes.

The Group walker serializes only the first 72 logical bytes and the first
`num` elements from each fixed member plane. SelectGroup then serializes
`[+0x9d0,+0x9e8)`. It deliberately excludes `hilited` at `+0x9e8`, the final
four bytes of class tail/virtual-base storage, every unused member-array slot,
the Group virtual-base pointer, array pointers, and `cur_index`.

The deterministic six-class layout receipt is
`e5bbd4c96699cfabc50f372a7b8f1561b22639e0482a4e1e2ddd67187d124281`.

## Gates

The synthetic image exercises both arrays, zero- and nonzero-member rows,
nondefault allocation histories, signed member planes, both tag levels, and
all six SelectGroup tail values. Tests mutate every owned byte, reject every
truncation, malformed capacity/flags/length/member count, mutated PDB size, and
prove every following Options byte is excluded. The installed gate chains all
owners from World through Camera, lands at `0x2c234`, freezes the exact PE
bodies, and checks independent SVX/RCX seeds.

## Reproduction

```sh
python3 re/scripts/test_savegame_select_groups.py
python3 re/scripts/savegame_select_groups.py \
  "ron-data/savegames/new save game 2026.08.11 15'42'57 (Tue).SVX" \
  --offset 0x2c234
```

The returned end, `0x2c23d`, is the exact first byte owned by Options.
