# Retail save boundary: Items

Status: **complete concrete pointer-array owner**.  This lane starts at
`PtrArray<Item>.length` immediately after Goods, consumes both copies of the
container allocation history, the pointer-presence plane, and every complete
`Item`/`SubObject` body, then stops at the next native owner,
`Heroes::walk_data`.  It does not edit the shared parser, normalize retail
container history, or join the independent SVX and RCX identities.

The exclusive helper is `re/scripts/savegame_items.py`; exhaustive tests are
in `re/scripts/test_savegame_items.py`.

## Fresh-SVX splice

The user-created SVX has compressed SHA-256
`161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7` and
decompressed SHA-256
`fa31f89f9bf435e28847e9e54d62cfe712e6bafc6cedb9ab98c8ab6730adb7c8`.
Chaining the exclusive parsers from Leaders through Goods reaches:

| stream range | exact owner | fresh value |
|---|---|---:|
| `0x27049..0x2704d` | `PtrArray<Item>.length` | 0 |
| next owner at `0x2704d` | `Heroes::walk_data` `0x0073a510` | excluded |

The four-byte fresh Items image is all zero and has SHA-256
`df3f619804a92fdb4057192dc43dd748ea778adc52bc498ce80524c014b81119`.
The zero-length branch owns no capacity, increment, flags, presence bytes,
repeated history, or Item bodies.

The separate RCX has SHA-256
`558e0cd53dbed4f820e8757c0327a58384d433e5beb8e9eeef5d64df4d67bd54`.
The test independently confirms the SVX seed `0x014810ac` and RCX seed
`0x007f93e0`; no state or identity is joined across them.

## Caller order and exact pointer-array grammar

`WalkDataGame::walk_data` calls the global-specific
`PtrArray<Item>::walk_data` specialization at `0x005a2b1c`.  The callee is
`0x0045d020` (797 bytes) and directly addresses the PDB-named global
`GameAccess::items` at `0x00c0a100`.  Its grammar is:

```text
i32 length
if length != 0:
    i32 capacity
    i16 increment
    u8  flags                       # writer persistently clears bit 0x40
    u8  pointer_present[length]     # exact booleans, logical-slot order
    i32 repeated_capacity
    i16 repeated_increment
    for each present slot, in slot order:
        Item body
```

The second capacity/increment pair is the direct `[items+8,items+0xe)` walk
after the presence plane.  Both physical copies must agree in a valid writer
image.  The helper rejects disagreement but retains both values and offsets;
it never substitutes a host-container default.  It also rejects negative or
unreasonable lengths, `capacity < length`, nonboolean pointer markers, and the
writer-cleared flags bit.  A synthetic `length=2, capacity=99,
increment=-17` image remains exactly 99 and -17 in both history passes.

On load, the native function clears/reallocates the global list, constructs a
44-byte `Item` for every true marker, and walks bodies only after rebuilding
all pointers.  Holes therefore retain their logical indices.  `cur_index` at
array offset `+0x18` is not walked.

After the function returns, profiling owns no stream bytes.  The caller next
invokes `Heroes::walk_data` `0x0073a510`, defining the exact end.

## Complete Item body

Every present pointer dispatches virtual `Item::walk_data` `0x00677150`:

```text
u8  Item tag                        # walk_test(StringTable[4580])
u8  ever_seen                       # Item +0x20
u8  SubObject tag                   # walk_test(StringTable[6262])
u8  flags                           # Item +0x08
u8  must_walk
if must_walk != 0:
    u8  who                         # [+0x09,+0x0a)
    i16 o                           # [+0x0a,+0x0c)
    i32 z_internal                  # [+0x0c,+0x10)
    i32 x_internal                  # [+0x10,+0x14)
    i32 y_internal                  # [+0x14,+0x18)
    i32 TypeIndex                   # resolved from ptype, not its address
```

The Item walk-test byte offset is `0x165d0`; `sizeof(String)==20`, so its
StringTable index is 4580.  The inherited SubObject offset `0x1e938` gives
index 6262.  The fresh specimen has no rows, so their numeric tag values are
not guessed; the helper preserves any byte at each PE-proven position.

A present dormant body occupies five bytes and a full body occupies 24.
`SubObject::must_walk` `0x006623a0` derives true on output when the type pointer
is nonnull or `flags & 1`, and reads the stream byte on input.  Either way, it
passes the boolean itself through the walker's virtual `walk_function`.  The
helper requires an exact 0 or 1 and uses it to decide whether the 19-byte tail
follows.  It does not require a nonzero TypeIndex on a full row because the
active flag alone can take that branch.

`SubObjectOut::on_screen` at `+0x1c` is not walked.  No compiler/vtable-prefix
or trailing bytes are copied.

## PDB and executable receipts

The matching PDB has SHA-256
`334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`,
GUID `51D4F219-61C6-4F84-9D5B-C3361B0D291F`, age 1.  Its JSON export has
SHA-256
`399aeac3bfb7a201a907e49e51388bb4e010420a54ccb781b335828315f28f14`.
The helper loads that export at runtime and requires:

- `sizeof(PtrArray<Item>) == 28`, including length `+4`, capacity `+8`,
  signed-short increment `+12`, list `+16`, flags `+20`, and cursor `+24`;
- `sizeof(Item) == sizeof(ItemData) == 44`;
- `sizeof(SubObject) == 40` and `sizeof(SubObjectData) == 28`;
- flags/who/o/Z/X/Y/ptype at `+8/+9/+10/+12/+16/+20/+24`; and
- `ever_seen` at `+32`.

The canonical PDB-layout receipt has SHA-256
`5d639f178400a9c069a8e623f2cc1d582648e35908d073ad69f0a75b7aa311da`.

The matched PE SHA-256 is
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`.
Tests freeze complete spans for the Item pointer-array specialization,
`Item::walk_data`, `SubObject::walk_data`, `SubObject::must_walk`, the native
Items checksum scanner, the main caller sequence, and the following Heroes
walker.

## Independent canonical Items comparison

The landed Items authority independently identifies Item as the sole goody-box
class, TypeIndex 543, retains sparse slot identity and array history, and emits
the exact checksum row:

```text
ever_seen : u8
flags     : u8
must_walk : u8
who       : u8
o         : i16 LE
z         : i32 LE
x         : i32 LE
y         : i32 LE
TypeIndex : i32 LE
```

`CheckSums::check_items` `0x00937790` scans full logical array length, skips
inactive slots, and walks active Items in stable slot order.  Its two
`walk_test` calls are no-ops, so each active row is 22 bytes.  The test strips
only the two save tags from an independently parsed full Item and gets exactly
that 22-byte projection, including `must_walk=1` and TypeIndex 543.  Mutating
either tag changes the save digest but not this checksum projection.

This is a cross-check, not an inference source.  The landed runtime does not
replace either save-history pass, compact holes, or supply tag bytes absent
from the fresh specimen.

## Mutation and boundary proof

The nonempty fixture uses `length=3`, presence `[1,0,1]`, arbitrary valid
allocation history, one five-byte dormant body, and one complete 24-byte body.
Every owned-byte one-bit mutation either violates a structural invariant or
changes the parsed receipt and digest.  Every truncation is rejected.
Dedicated tests cover both history passes, pointer booleans, `must_walk`, exact
history/tag preservation, checksum/save-tag separation, and exclusion of a
mutation at the first Heroes byte.

## Reproduction

```sh
python3 re/scripts/test_savegame_items.py

python3 re/scripts/savegame_items.py \
  "ron-data/savegames/new save game 2026.08.11 15'42'57 (Tue).SVX" \
  --offset 0x27049
```

The returned `end` is the exact start of `Heroes::walk_data`.
