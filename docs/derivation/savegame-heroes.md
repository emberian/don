# Retail save boundary: Heroes

Status: **complete concrete eight-owner container**. This lane starts at the
tag emitted inside `Heroes::walk_data`, consumes all eight
`PtrArray<Hero>` histories and presence planes plus every complete Hero and
nested active-spell value array, and stops before the next caller-owned Herds
tag. It does not edit the shared parser, normalize either container layer, or
join the independent SVX and RCX identities.

The exclusive helper is `re/scripts/savegame_heroes.py`; exhaustive tests are
in `re/scripts/test_savegame_heroes.py`.

## Fresh-SVX splice

The user-created SVX has compressed SHA-256
`161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7` and
decompressed SHA-256
`fa31f89f9bf435e28847e9e54d62cfe712e6bafc6cedb9ab98c8ab6730adb7c8`.
Chaining the exclusive parsers from Leaders through Items reaches:

| stream range | exact owner | fresh value |
|---|---|---:|
| `0x2704d..0x2704e` | Heroes `walk_test(StringTable[3959])` | 0 |
| `0x2704e..0x27052` | `lists[0].length` | 0 |
| `0x27052..0x27056` | `lists[1].length` | 0 |
| `0x27056..0x2705a` | `lists[2].length` | 0 |
| `0x2705a..0x2705e` | `lists[3].length` | 0 |
| `0x2705e..0x27062` | `lists[4].length` | 0 |
| `0x27062..0x27066` | `lists[5].length` | 0 |
| `0x27066..0x2706a` | `lists[6].length` | 0 |
| `0x2706a..0x2706e` | `lists[7].length` | 0 |
| next owner at `0x2706e` | caller Herds tag, `StringTable[3958]` | excluded |

The exact 33-byte Heroes image is all zero and has SHA-256
`7f9c9e31ac8256ca2f258583df262dbc7d6f68f2a03043d5c99a4ae5a7396ce9`.
Every zero-length pointer array owns only its four-byte length; no allocation
history, presence plane, or Hero body follows. The next five bytes (the Herds
tag and empty `PtrArray<Herd>.length`) have independent SHA-256
`8855508aade16ec573d21e6a485dfd0a7624085c1a14b5ecdd6485de0c6839a4`.

The separate RCX has SHA-256
`558e0cd53dbed4f820e8757c0327a58384d433e5beb8e9eeef5d64df4d67bd54`.
The test independently confirms SVX seed `0x014810ac` and RCX seed
`0x007f93e0`; no state or identity is joined across them.

## Caller order and the eight outer owners

`Heroes::walk_data` `0x0073a510` first emits the tag whose StringTable byte
offset is `0x1354c`. With `sizeof(String)==20`, that is exact index 3959. It
then advances through the eight 28-byte `PtrArray<Hero>` objects at globals
`0x00c0a120..0x00c0a1ff`, one array per 28 bytes. PDB `Heroes::lists` is
exactly `PtrArray<Hero>[8]` at `+0`, size 224.

Each outer owner has this independent grammar:

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
        Hero body
```

The repeated pair is a second physical walk of `[owner+8,owner+0xe)` after
the presence plane. The helper requires the two images to agree while
retaining both values and offsets. It does not supply a host default, compact
holes, or merge histories among the eight owners. Negative or unreasonable
lengths, `capacity < length`, the writer-cleared flags bit, nonboolean
presence bytes, and disagreement between history passes all fail closed.
`cur_index` at `+24` is not walked.

## Complete Hero and ActiveSpell bodies

Every present pointer receives a new 48-byte `Hero` and calls
`Hero::walk_data` `0x00739e20`. There is **no per-Hero tag**. The exact body is:

```text
Array<ActiveSpell> active_spells
i16 hero
i16 o
i8  hero_flags
i8  who
```

`Hero::walk_data` first delegates Hero `+4` to the same active-spell array
walker used by `Caster::walk_data` `0x00739ab0`, then directly walks the six
bytes `[Hero+0x24,Hero+0x2a)`. The virtual-base pointer at `+32` and bytes
from `+42` through the 48-byte object end are not serialized.

`Array<ActiveSpell>::walk_data` `0x0048a9e0` is a value-array grammar:

```text
i32 length
if length != 0:
    i32 capacity
    i16 increment
    u8  flags                       # writer persistently clears bit 0x40
    repeat length times:
        i32 t                       # enum TypeIndex
        i32 start
        i32 frame
```

Unlike `PtrArray<Hero>`, this value array has no presence plane and no repeated
history pass. Each `ActiveSpell` row is exactly 12 bytes. Zero length owns only
four bytes. The helper preserves signed values, capacity, increment, flags,
and row order and rejects the same length/capacity/flags contradictions.

PDB calls the last field `frame`; the independent Sim representation calls it
`end_frame`, matching the existing active-spell expiry semantics. This name
comparison does not infer or replace the retail save image.

## Exact next-owner boundary

After returning from Heroes, the main caller's profiling calls own no stream
bytes. `WalkDataGame::walk_data` then emits
`walk_test(StringTable[3958])` (byte offset `0x13538`) and calls
`PtrArray<Herd>::walk_data` `0x0048d610`. Therefore Heroes ends immediately
before the caller tag at `0x2706e`; the Herds tag is not part of
`Heroes::walk_data`.

## PDB and executable receipts

The matching PDB has SHA-256
`334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`,
GUID `51D4F219-61C6-4F84-9D5B-C3361B0D291F`, age 1. Its JSON export has
SHA-256
`399aeac3bfb7a201a907e49e51388bb4e010420a54ccb781b335828315f28f14`.
The helper loads that export at runtime and requires:

- `sizeof(Heroes)==232`, with the complete 224-byte `lists` field at `+0`;
- `sizeof(PtrArray<Hero>)==28`, with length/capacity/increment/list/flags/cursor
  at `+4/+8/+12/+16/+20/+24`;
- `sizeof(Hero)==48`, with active spells at `+4`, hero/o at `+36/+38`, and
  signed-char hero_flags/who at `+40/+41`;
- `sizeof(Caster)==40`, independently placing the same active-spell array at
  `+4`;
- `sizeof(Array<ActiveSpell>)==28` with the corresponding concrete value-list
  pointer type; and
- `sizeof(ActiveSpell)==12`, with t/start/frame at `+0/+4/+8`.

The canonical PDB-layout receipt has SHA-256
`ead3334b3e08937f889e670fc2b0b91bfb3967148d51e18b2cb8f71d5c33677b`.

The matched PE SHA-256 is
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`.
Tests freeze complete spans for `Heroes::walk_data`, `Hero::walk_data`,
`Caster::walk_data`, the `Array<ActiveSpell>` specialization, the main caller
handoff, and the following `PtrArray<Herd>` specialization.

## Independent Sim comparison

`crates/don-sim/src/systems/casters_animals.rs` independently defines a
`#[repr(C)]` 12-byte `ActiveSpell` triplet:

```text
type_id     : i32
start_frame : i32
end_frame   : i32
```

The test serializes two independently parsed save rows and gets the exact six
little-endian words expected by that representation. The canonical file also
explicitly says a Rust `Vec` does not represent the engine's complete walked
container and leaves capacity/growth metadata to its caller. Consequently
this cross-check never normalizes the nested retail history or supplies
missing container state.

## Mutation and boundary proof

The nonempty fixture contains all eight outer owners. Owner 0 has length 3,
presence `[1,0,1]`, duplicated nondefault allocation history, one Hero with an
empty active-spell array, and one Hero with two active spells and nondefault
nested history; owners 1 through 7 are empty. Every owned-byte one-bit
mutation either violates a structural invariant or changes the parsed receipt
and digest. Every truncation is rejected. Dedicated tests cover both outer
history passes, both flags words, both length/capacity pairs, presence
booleans, signed Hero scalars, exact row order, and exclusion of a mutation at
the first Herds byte.

## Reproduction

```sh
python3 re/scripts/test_savegame_heroes.py

python3 re/scripts/savegame_heroes.py \
  "ron-data/savegames/new save game 2026.08.11 15'42'57 (Tue).SVX" \
  --offset 0x2704d
```

The returned `end` is the exact start of the caller-owned Herds tag.
