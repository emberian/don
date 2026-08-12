# Retail save boundary: Goods

Status: **complete concrete pointer-array owner**.  This lane starts at
`PtrArray<Good>.length` immediately after Forms, consumes both copies of the
container's allocation history, the pointer-presence plane, and every complete
`Good`/`SubObject` body, then stops at the next native owner,
`PtrArray<Item>::walk_data`.  It does not edit the shared parser, normalize
retail container history, or infer SVX state from the independently captured
RCX.

The exclusive helper is `re/scripts/savegame_goods.py`; exhaustive tests are
in `re/scripts/test_savegame_goods.py`.

## Fresh-SVX splice

The user-created SVX has compressed SHA-256
`161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7` and
decompressed SHA-256
`fa31f89f9bf435e28847e9e54d62cfe712e6bafc6cedb9ab98c8ab6730adb7c8`.
Chaining the exclusive parsers from Leaders through Forms reaches:

| stream range | exact owner | fresh value |
|---|---|---:|
| `0x27045..0x27049` | `PtrArray<Good>.length` | 0 |
| next owner at `0x27049` | `PtrArray<Item>::walk_data` `0x0045d020` | excluded |

The exact four-byte fresh Goods image is all zero and has SHA-256
`df3f619804a92fdb4057192dc43dd748ea778adc52bc498ce80524c014b81119`.
The zero-length branch owns no capacity, increment, flags, presence bytes,
repeated history, or Good bodies.

The separate RCX has SHA-256
`558e0cd53dbed4f820e8757c0327a58384d433e5beb8e9eeef5d64df4d67bd54`.
The test independently confirms the SVX seed `0x014810ac` and RCX seed
`0x007f93e0`; no state or identity is joined across them.

## Caller order and exact pointer-array grammar

`WalkDataGame::walk_data` pushes its `DataWalk`, selects the global
`PtrArray<Good>` at `0x00c0a0e0`, and calls the concrete specialization at
`0x005a2b0b`.  `PtrArray<Good>::walk_data` is `0x0045cce0` (830 bytes).  Its
save grammar is:

```text
i32 length
if length != 0:
    i32 capacity
    i16 increment
    u8  flags                       # writer persistently clears bit 0x40
    u8  pointer_present[length]     # exact booleans, one per logical slot
    i32 repeated_capacity
    i16 repeated_increment
    for each present slot, in slot order:
        Good body
```

The second capacity/increment pair is a real direct
`[PtrArray+8,PtrArray+0xe)` walk after the presence plane.  Both copies must
agree in a valid writer image, but both remain physically present.  The helper
therefore rejects disagreement without replacing either copy with a preferred
host value.  It also rejects negative or unreasonable lengths,
`capacity < length`, nonboolean presence markers, and the writer-cleared flags
bit.  A synthetic `length=2, capacity=99, increment=-17` image remains exactly
99 and -17 in both history passes.

On load, the retail function resizes the pointer list, reconstructs a 48-byte
`Good` for every true presence marker, and only then walks bodies.  Holes retain
their logical slot indices.  `cur_index` at array offset `+0x18` is not in this
walk.

After the function returns, profiling owns no stream bytes.  The caller next
invokes `PtrArray<Item>::walk_data` `0x0045d020`, defining the exact end.

## Complete Good body

Every present pointer dispatches virtual `Good::walk_data` `0x0066e5d0`:

```text
u8  Good tag                        # walk_test(StringTable[3549])
u8  ever_seen                       # Good +0x20
u8  SubObject tag                   # walk_test(StringTable[6262])
u8  flags                           # Good +0x08
u8  must_walk
if must_walk != 0:
    u8  who                         # [+0x09,+0x0a)
    i16 o                           # [+0x0a,+0x0c)
    i32 z_internal                  # [+0x0c,+0x10)
    i32 x_internal                  # [+0x10,+0x14)
    i32 y_internal                  # [+0x14,+0x18)
    i32 TypeIndex                   # resolved from ptype, not its address
```

Thus a present dormant body occupies five bytes and a full body occupies 24.
The two tags are SaveGame/LoadGame walk-test bytes.  The fresh specimen has no
rows, so the helper preserves arbitrary values at their PE-proven locations
instead of guessing numeric constants.

`SubObject::must_walk` `0x006623a0` is virtual but resolves through the Good
vtable to the inherited implementation.  For output walkers it derives true
when `ptype != nullptr` or `flags & 1`; for input walkers it reads the stream
value.  In both cases it calls the walker's virtual `walk_function` on the
boolean itself.  The helper consequently requires an exact 0 or 1 and uses it
to decide whether the 19-byte tail follows.  It does not invent an unwalked
`ptype` pointer for dormant rows, nor require a nonzero TypeIndex for a full
row: `flags & 1` alone is sufficient to make the native writer take the full
branch.

`GoodData::cur_time` at `+0x24` and `SubObjectOut::on_screen` at `+0x1c` are
not walked.  No bytes from the compiler/vtable prefixes are copied.

## PDB and executable receipts

The matching PDB has SHA-256
`334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`,
GUID `51D4F219-61C6-4F84-9D5B-C3361B0D291F`, age 1.  Its JSON export has
SHA-256
`399aeac3bfb7a201a907e49e51388bb4e010420a54ccb781b335828315f28f14`.
The helper loads that export at runtime and requires:

- `sizeof(PtrArray<Good>) == 28`, including length `+4`, capacity `+8`,
  signed-short increment `+12`, list `+16`, flags `+20`, and cursor `+24`;
- `sizeof(Good) == sizeof(GoodData) == 48`;
- `sizeof(SubObject) == 40` and `sizeof(SubObjectData) == 28`;
- flags/who/o/Z/X/Y/ptype at `+8/+9/+10/+12/+16/+20/+24`;
- `ever_seen` at `+32` and `cur_time` at `+36`.

The canonical PDB-layout receipt has SHA-256
`597622152a52d474951aa920abf388281366561adc263933ec79e3a89a12d3a2`.

The matched PE SHA-256 is
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`.
Tests freeze complete spans for the Good pointer-array specialization,
`Good::walk_data`, `SubObject::walk_data`, `SubObject::must_walk`, the native
Goods checksum scanner, `CheckSum::walk_function`, the CheckSum constructor,
the main caller sequence, and the following Items walker.

## Independent Goods checksum cross-check

The landed Sim Goods authority correctly supplies the named Good scalars,
sparse slot order, capacity, increment, flags, cursor, `good_mark`, and
`cur_time`.  Those facts are useful independent checks; none is used to
normalize the two retail save-history passes.

The comparison also finds one exact discrepancy in the landed checksum image.
`CheckSums::check_goods` `0x00937710` scans full logical array length, skips
slots whose `flags & 1` is clear, then executes the base-Good walk directly.
It calls `Good::walk_data`'s walk test and `ever_seen` range, followed by
`SubObject::walk_data`.  `CheckSum::walk_test` is a no-op, but the CheckSum
constructor `0x0045dec0` leaves `DataWalk::input == 0`; therefore
`SubObject::must_walk` derives true for every already-filtered active Good and
passes that byte to `CheckSum::walk_function` `0x00936ff0`.

The exact active checksum row is therefore 22 bytes:

```text
ever_seen : u8
flags     : u8
must_walk : u8                 # always 1 on check_goods' active path
who       : u8
o         : i16 LE
z         : i32 LE
x         : i32 LE
y         : i32 LE
TypeIndex : i32 LE
```

The currently landed `GoodNode::walked_bytes -> [u8; 21]` and
`GOOD_WALKED_BYTES = 21` omit only `must_walk`.  The exclusive test constructs
an independently parsed retail Good row, confirms that deleting byte 2 yields
the landed 21-byte image exactly, and flips only byte 2 to prove that the
retail Adler checksum changes while the landed projection cannot observe the
mutation.  This save-parser tranche records that discrepancy but does not edit
the shared Sim authority; its correction is a separate owner.

## Mutation and boundary proof

The nonempty fixture uses `length=3`, presence `[1,0,1]`, arbitrary valid
allocation history, one five-byte dormant body, and one complete 24-byte body.
Every owned-byte one-bit mutation either violates a structural invariant or
changes the parsed receipt and digest.  Every truncation is rejected.
Dedicated tests cover both history passes, pointer booleans, `must_walk`, exact
history/tag preservation, the 22-vs-21 checksum mutation, and exclusion of a
mutation at the first Items byte.

## Reproduction

```sh
python3 re/scripts/test_savegame_goods.py

python3 re/scripts/savegame_goods.py \
  "ron-data/savegames/new save game 2026.08.11 15'42'57 (Tue).SVX" \
  --offset 0x27045
```

The returned `end` is the exact start of `PtrArray<Item>::walk_data`.
