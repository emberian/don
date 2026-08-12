# Retail save boundary: Herds

Status: **complete caller tag plus concrete pointer-array owner**. This lane
starts at the Herds tag emitted by `WalkDataGame::walk_data`, consumes both
copies of the `PtrArray<Herd>` allocation history, its logical-slot presence
plane, and every exact Herd body, then stops at `Specials::walk_data`. It does
not edit the shared parser, normalize container history, copy compiler padding,
or join the independent SVX and RCX identities.

The exclusive helper is `re/scripts/savegame_herds.py`; exhaustive tests are
in `re/scripts/test_savegame_herds.py`.

## Fresh-SVX splice

The user-created SVX has compressed SHA-256
`161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7` and
decompressed SHA-256
`fa31f89f9bf435e28847e9e54d62cfe712e6bafc6cedb9ab98c8ab6730adb7c8`.
Chaining the exclusive parsers from Leaders through Heroes reaches:

| stream range | exact owner | fresh value |
|---|---|---:|
| `0x2706e..0x2706f` | caller `walk_test(StringTable[3958])` | 0 |
| `0x2706f..0x27073` | `PtrArray<Herd>.length` | 0 |
| next owner at `0x27073` | `Specials::walk_data` `0x007403b0` | excluded |

The five-byte fresh Herds image is all zero and has SHA-256
`8855508aade16ec573d21e6a485dfd0a7624085c1a14b5ecdd6485de0c6839a4`.
The zero-length branch owns no capacity, increment, flags, presence bytes,
repeated history, or Herd bodies.

The separate RCX has SHA-256
`558e0cd53dbed4f820e8757c0327a58384d433e5beb8e9eeef5d64df4d67bd54`.
The test independently confirms SVX seed `0x014810ac` and RCX seed
`0x007f93e0`; no state or identity is joined across them.

## Caller order and exact pointer-array grammar

After `Heroes::walk_data` and profiling, `WalkDataGame::walk_data` emits the
tag whose StringTable byte offset is `0x13538`. With `sizeof(String)==20`, the
exact index is 3958. It then calls the global-specific
`PtrArray<Herd>::walk_data` specialization at `0x0048d610`, which directly
addresses the array at `0x00c0a240`.

The complete grammar is:

```text
u8  Herds tag                      # walk_test(StringTable[3958])
i32 length
if length != 0:
    i32 capacity
    i16 increment
    u8  flags                      # writer persistently clears bit 0x40
    u8  pointer_present[length]    # exact booleans, logical-slot order
    i32 repeated_capacity
    i16 repeated_increment
    for each present pointer, in slot order:
        Herd body
```

The second capacity/increment pair is the direct `[array+8,array+0xe)` walk
after the presence plane. Both physical images must agree in a valid writer
stream. The helper retains both values and offsets while rejecting
disagreement; it never substitutes a host-container default or compacts
holes. It also rejects negative or unreasonable lengths, `capacity < length`,
nonboolean pointer markers, and the writer-cleared flags bit. `cur_index` at
array `+24` is not walked.

`Herds::walk_data` `0x00741d50` independently emits the same StringTable tag
and calls the same pointer-array specialization. The main save caller inlines
that two-operation sequence rather than calling the wrapper.

## Complete 27-byte Herd body

For each present slot, the load branch allocates and constructs a 36-byte
`Herd` object. The pointer-array specialization then directly invokes the
walker on `[Herd+0,Herd+0x1b)`. There is no per-Herd tag and no virtual body
dispatch in this specialization. The exact 27 bytes are:

```text
i32 cx                         # WCoord
i32 cy                         # WCoord
i32 wx                         # WCoord
i32 wy                         # WCoord
i32 t                          # enum TypeIndex
i32 good_o
i16 herd
i8  herd_flags
```

`Herd::walk_data` `0x00741a40` independently makes the same direct
`[this,this+0x1b)` call. PDB `HerdData` is 28 bytes because the compiler adds
one tail-padding byte; PDB `Herd` is 36 bytes because of its virtual-base
pointer. Neither the tail-padding byte at `+27` nor the pointer and trailing
object bytes are serialized.

The PDB declares `herd_flags` as signed `char`, so the parser exposes the
exact byte as `i8`. Bit tests remain lossless through `value & 0xff`.

## Exact next-owner boundary

After the Herd pointer array returns, profiling owns no stream bytes. The main
caller immediately invokes `Specials::walk_data` `0x007403b0`. That next
function begins with its own tag at StringTable byte offset `0x1e370`, exact
index 6188. Consequently the Herds parser ends before the Specials tag at
fresh offset `0x27073`.

## PDB and executable receipts

The matching PDB has SHA-256
`334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`,
GUID `51D4F219-61C6-4F84-9D5B-C3361B0D291F`, age 1. Its JSON export has
SHA-256
`399aeac3bfb7a201a907e49e51388bb4e010420a54ccb781b335828315f28f14`.
The helper loads that export at runtime and requires:

- `sizeof(PtrArray<Herd>)==28`, with length/capacity/increment/list/flags/cursor
  at `+4/+8/+12/+16/+20/+24`;
- `sizeof(Herd)==36`, with exactly the eight HerdData fields through `+26`; and
- `sizeof(HerdData)==28`, with cx/cy/wx/wy/t/good_o/herd/herd_flags at
  `+0/+4/+8/+12/+16/+20/+24/+26`.

The canonical PDB-layout receipt has SHA-256
`9cd0aebae5906ab64e1b838fb2f251916e7b0b194cfb2cd72dcc9aca2923f6fc`.

The matched PE SHA-256 is
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`.
Tests freeze complete spans for the Herd pointer-array specialization,
`Herd::walk_data`, `Herds::walk_data`, the main caller handoff, and the
following Specials walker.

## Independent Sim comparison

`crates/don-sim/src/systems/casters_animals.rs` independently defines the PDB
size-28 `#[repr(C)] HerdData` with the same four coordinates, TypeIndex,
good-object index, i16 herd identifier, and final flags byte. It explicitly
warns that compiler tail padding is not a serializer claim. The test compares
the exact independently parsed 27-byte save row to the same little-endian
field image and proves the signed parser spelling retains the Sim authority's
unsigned flags bit pattern. This is a row-semantics cross-check only; the Sim
representation never replaces sparse slot identity or either save-history
pass.

## Mutation and boundary proof

The nonempty fixture uses length 3, presence `[1,0,1]`, nondefault duplicated
allocation history, and two complete 27-byte Herd rows with signed boundary
values. Every owned-byte one-bit mutation either violates a structural
invariant or changes the parsed receipt and digest. Every truncation is
rejected. Dedicated tests cover both history passes, pointer booleans, exact
history and signed-byte preservation, tail-padding exclusion, and exclusion
of a mutation at the first Specials byte.

## Reproduction

```sh
python3 re/scripts/test_savegame_herds.py

python3 re/scripts/savegame_herds.py \
  "ron-data/savegames/new save game 2026.08.11 15'42'57 (Tue).SVX" \
  --offset 0x2706e
```

The returned `end` is the exact start of `Specials::walk_data`.
