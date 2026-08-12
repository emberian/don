# Retail save boundary: Specials

Status: **complete concrete eight-owner container**. This lane starts at the
tag emitted inside `Specials::walk_data`, consumes all eight
`PtrArray<Special>` histories and presence planes plus every complete Special and
nested active-spell value array, and stops before the next owner,
`Wonders::walk_data`. It does not edit the shared parser, normalize either container layer, or
join the independent SVX and RCX identities.

The exclusive helper is `re/scripts/savegame_specials.py`; exhaustive tests are
in `re/scripts/test_savegame_specials.py`.

## Fresh-SVX splice

The user-created SVX has compressed SHA-256
`161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7` and
decompressed SHA-256
`fa31f89f9bf435e28847e9e54d62cfe712e6bafc6cedb9ab98c8ab6730adb7c8`.
Chaining the exclusive parsers from Leaders through Herds reaches:

| stream range | exact owner | fresh value |
|---|---|---:|
| `0x27073..0x27074` | Specials `walk_test(StringTable[6188])` | 0 |
| `0x27074..0x27078` | `lists[0].length` | 0 |
| `0x27078..0x2707c` | `lists[1].length` | 0 |
| `0x2707c..0x27080` | `lists[2].length` | 0 |
| `0x27080..0x27084` | `lists[3].length` | 0 |
| `0x27084..0x27088` | `lists[4].length` | 0 |
| `0x27088..0x2708c` | `lists[5].length` | 0 |
| `0x2708c..0x27090` | `lists[6].length` | 0 |
| `0x27090..0x27094` | `lists[7].length` | 0 |
| next owner at `0x27094` | `Wonders::walk_data` `0x0073ca40` | excluded |

The exact 33-byte Specials image is all zero and has SHA-256
`7f9c9e31ac8256ca2f258583df262dbc7d6f68f2a03043d5c99a4ae5a7396ce9`.
Every zero-length pointer array owns only its four-byte length; no allocation
history, presence plane, or Special body follows. The following fresh Wonders
image is another 33-byte all-zero eight-owner image with independent SHA-256
`7f9c9e31ac8256ca2f258583df262dbc7d6f68f2a03043d5c99a4ae5a7396ce9`.

The separate RCX has SHA-256
`558e0cd53dbed4f820e8757c0327a58384d433e5beb8e9eeef5d64df4d67bd54`.
The test independently confirms SVX seed `0x014810ac` and RCX seed
`0x007f93e0`; no state or identity is joined across them.

## Caller order and the eight outer owners

`Specials::walk_data` `0x007403b0` first emits the tag whose StringTable byte
offset is `0x1e370`. With `sizeof(String)==20`, that is exact index 6188. It
then advances through the eight 28-byte `PtrArray<Special>` objects at globals
`0x00c0a280..0x00c0a35f`, one array per 28 bytes. PDB `Specials::lists` is
exactly `PtrArray<Special>[8]` at `+0`, size 224.

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
        Special body
```

The repeated pair is a second physical walk of `[owner+8,owner+0xe)` after
the presence plane. The helper requires the two images to agree while
retaining both values and offsets. It does not supply a host default, compact
holes, or merge histories among the eight owners. Negative or unreasonable
lengths, `capacity < length`, the writer-cleared flags bit, nonboolean
presence bytes, and disagreement between history passes all fail closed.
`cur_index` at `+24` is not walked.

## Complete Special and ActiveSpell bodies

Every present pointer receives a new 48-byte `Special` and calls
`Special::walk_data` `0x00740030`. There is **no per-Special tag**. The exact body is:

```text
Array<ActiveSpell> active_spells
i16 special
i16 o
i8  special_flags
i8  who
```

`Special::walk_data` first delegates Special `+4` to the same active-spell array
walker used by `Caster::walk_data` `0x00739ab0`, then directly walks the six
bytes `[Special+0x24,Special+0x2a)`. The virtual-base pointer at `+32` and bytes
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

Unlike `PtrArray<Special>`, this value array has no presence plane and no repeated
history pass. Each `ActiveSpell` row is exactly 12 bytes. Zero length owns only
four bytes. The helper preserves signed values, capacity, increment, flags,
and row order and rejects the same length/capacity/flags contradictions.

PDB calls the last field `frame`; the independent Sim representation calls it
`end_frame`, matching the existing active-spell expiry semantics. This name
comparison does not infer or replace the retail save image.

## Exact next-owner boundary

After returning from Specials, the main caller's profiling call owns no stream
bytes. `WalkDataGame::walk_data` then invokes `Wonders::walk_data`
`0x0073ca40`. That next owner starts by emitting
`walk_test(StringTable[7131])`, whose byte offset is `0x22d1c`. Therefore
Specials ends immediately before the Wonders tag at `0x27094`.

## PDB and executable receipts

The matching PDB has SHA-256
`334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`,
GUID `51D4F219-61C6-4F84-9D5B-C3361B0D291F`, age 1. Its JSON export has
SHA-256
`399aeac3bfb7a201a907e49e51388bb4e010420a54ccb781b335828315f28f14`.
The helper loads that export at runtime and requires:

- `sizeof(Specials)==232`, with the complete 224-byte `lists` field at `+0`;
- `sizeof(PtrArray<Special>)==28`, with length/capacity/increment/list/flags/cursor
  at `+4/+8/+12/+16/+20/+24`;
- `sizeof(Special)==48`, with active spells at `+4`, special/o at `+36/+38`, and
  signed-char special_flags/who at `+40/+41`;
- `sizeof(SpecialData)==48`, independently retaining the same complete field
  image;
- `sizeof(Caster)==40`, independently placing the same active-spell array at
  `+4`;
- `sizeof(Array<ActiveSpell>)==28` with the corresponding concrete value-list
  pointer type; and
- `sizeof(ActiveSpell)==12`, with t/start/frame at `+0/+4/+8`.

The canonical PDB-layout receipt has SHA-256
`6f421d1d70bb92b6211c982cf2493d331fcd9209244c4c54909b1da4f1aeb6e3`.

The matched PE SHA-256 is
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`.
Tests freeze complete spans for `Specials::walk_data`, `Special::walk_data`,
`Caster::walk_data`, the `Array<ActiveSpell>` specialization, the main caller
handoff, and the following `Wonders::walk_data` owner.

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
presence `[1,0,1]`, duplicated nondefault allocation history, one Special with an
empty active-spell array, and one Special with two active spells and nondefault
nested history; owners 1 through 7 are empty. Every owned-byte one-bit
mutation either violates a structural invariant or changes the parsed receipt
and digest. Every truncation is rejected. Dedicated tests cover both outer
history passes, both flags words, both length/capacity pairs, presence
booleans, signed Special scalars, exact row order, and exclusion of a mutation at
the first Wonders byte.

## Reproduction

```sh
python3 re/scripts/test_savegame_specials.py

python3 re/scripts/savegame_specials.py \
  "ron-data/savegames/new save game 2026.08.11 15'42'57 (Tue).SVX" \
  --offset 0x27073
```

The returned `end` is the exact start of `Wonders::walk_data`.
