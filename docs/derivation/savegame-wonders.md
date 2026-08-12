# Retail save boundary: Wonders

Status: **complete concrete eight-owner container**. This lane begins at
`Wonders::walk_data`, consumes its tag, all eight independent
`PtrArray<Wonder>` histories and presence planes, and every exact Wonder row,
then stops before `Forts::walk_data`. It does not edit the shared parser,
normalize allocation history, copy compiler padding, or join SVX and RCX
state.

The exclusive helper is `re/scripts/savegame_wonders.py`; exhaustive tests are
in `re/scripts/test_savegame_wonders.py`.

## Fresh-SVX splice

The user-created SVX has compressed SHA-256
`161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7` and
decompressed SHA-256
`fa31f89f9bf435e28847e9e54d62cfe712e6bafc6cedb9ab98c8ab6730adb7c8`.
Chaining the exclusive parsers from Leaders through Specials reaches:

| stream range | exact owner | fresh value |
|---|---|---:|
| `0x27094..0x27095` | Wonders `walk_test(StringTable[7131])` | 0 |
| `0x27095..0x27099` | `lists[0].length` | 0 |
| `0x27099..0x2709d` | `lists[1].length` | 0 |
| `0x2709d..0x270a1` | `lists[2].length` | 0 |
| `0x270a1..0x270a5` | `lists[3].length` | 0 |
| `0x270a5..0x270a9` | `lists[4].length` | 0 |
| `0x270a9..0x270ad` | `lists[5].length` | 0 |
| `0x270ad..0x270b1` | `lists[6].length` | 0 |
| `0x270b1..0x270b5` | `lists[7].length` | 0 |
| next owner at `0x270b5` | `Forts::walk_data` `0x0073ef30` | excluded |

The exact 33-byte Wonders image is all zero and has SHA-256
`7f9c9e31ac8256ca2f258583df262dbc7d6f68f2a03043d5c99a4ae5a7396ce9`.
Each zero-length owner owns only its four-byte length.

The separate RCX has SHA-256
`558e0cd53dbed4f820e8757c0327a58384d433e5beb8e9eeef5d64df4d67bd54`.
The test independently confirms SVX seed `0x014810ac` and RCX seed
`0x007f93e0`; no state or identity is joined across them.

## Outer tag and eight pointer arrays

`Wonders::walk_data` `0x0073ca40` emits the tag whose StringTable byte offset
is `0x22d1c`. With `sizeof(String)==20`, the exact index is 7131. It then loops
over the eight 28-byte `PtrArray<Wonder>` globals at
`0x00c0a380..0x00c0a45f`. PDB `Wonders::lists` is exactly
`PtrArray<Wonder>[8]`, size 224.

Each owner has this grammar:

```text
i32 length
if length != 0:
    i32 capacity
    i16 increment
    u8  flags                       # writer persistently clears bit 0x40
    u8  pointer_present[length]     # exact booleans, logical-slot order
    i32 repeated_capacity
    i16 repeated_increment
    for each present pointer, in slot order:
        Wonder body
```

The repeated pair is a second physical walk of `[owner+8,owner+0xe)` after
the presence plane. The helper retains both images and requires them to agree.
It rejects negative or unreasonable lengths, `capacity < length`, the
writer-cleared flags bit, nonboolean presence, and history disagreement. It
never compacts holes, supplies host defaults, or merges histories among
owners. `cur_index` at `+24` is not walked.

## Exact 14-byte Wonder body

Every present pointer is constructed as a 24-byte `Wonder`, but the outer
walker directly emits only `[Wonder+0,Wonder+0xe)`:

```text
i16 wonder
i16 o
i32 stamp
i32 timer
i8  wonder_flags
i8  who
```

There is no per-row tag. `Wonder::walk_data` `0x0073c780` independently makes
the same direct 14-byte walker call. PDB `WonderData` is 16 bytes: offsets 14
and 15 are compiler tail padding. PDB `Wonder` is 24 bytes because its output
class adds a virtual-base pointer at `+16`. Neither padding nor object tail is
serialized.

The PDB declares both final fields as signed `char`, so the parser exposes
them as `i8`; bit patterns remain exact through `value & 0xff`.

## Next-owner boundary

After Wonders returns, the main caller's profiling call owns no stream bytes
and it invokes `Forts::walk_data` `0x0073ef30`. Forts starts with its own tag at
StringTable byte offset `0xd2b4`, exact index 2697. Thus Wonders ends before
the Forts tag at fresh `0x270b5`.

## PDB and executable receipts

The matching PDB SHA-256 is
`334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`,
GUID `51D4F219-61C6-4F84-9D5B-C3361B0D291F`, age 1. Its JSON export SHA-256
is `399aeac3bfb7a201a907e49e51388bb4e010420a54ccb781b335828315f28f14`.
The helper requires `sizeof(Wonders)==232`, `sizeof(PtrArray<Wonder>)==28`,
`sizeof(Wonder)==24`, `sizeof(WonderData)==16`, all exact field offsets and
pointer types, and a gap-free serialized prefix through `who+1 == 14`.

The canonical PDB-layout receipt has SHA-256
`3ad1ad289eeba9243999bc98249d78eaeaf83073accaf09470574e2b6a57aa9c`.

The matched PE SHA-256 is
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`.
Tests freeze complete spans for `Wonders::walk_data`, `Wonder::walk_data`, the
main caller handoff, and the following `Forts::walk_data` owner.

## Independent canonical Wonder comparison

`crates/don-sim/src/systems/wonders.rs` independently defines `WonderRecord`
with the same six fields. Its documentation identifies those fields as the
simulation data walked by `Wonder::walk_data`, distinguishes the 16-byte PDB
prefix from the 24-byte object, and retains unsigned flag semantics. The test
compares an independently parsed save row byte-for-byte and proves the signed
PDB spelling preserves the authority's unsigned flags image. This does not
replace sparse slot identity or either retail history pass.

## Mutation and boundary proof

The nonempty fixture contains all eight owners. Owner 0 has length 3,
presence `[1,0,1]`, nondefault duplicated history, and two full 14-byte rows;
owners 1 through 7 are empty. Every owned-byte mutation either fails an
invariant or changes the receipt and digest. Every truncation is rejected.
Dedicated tests cover exact history and signed field preservation, padding
exclusion, and exclusion of a mutation at the first Forts byte.

## Reproduction

```sh
python3 re/scripts/test_savegame_wonders.py

python3 re/scripts/savegame_wonders.py \
  "ron-data/savegames/new save game 2026.08.11 15'42'57 (Tue).SVX" \
  --offset 0x27094
```

The returned `end` is the exact start of `Forts::walk_data`.
