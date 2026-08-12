# Retail save boundary: Cities

Status: **complete PE/PDB-derived owner**.  This lane begins at the exact
`0x2701f` end returned by the Armies parser, consumes the complete
`Cities::walk_data` save image, and stops at the caller-owned Forms walk-test
before `ObjectArray<Form>::walk_data`.  It does not search for bytes, normalize
container history, or edit the shared parser.

The exclusive helper is `re/scripts/savegame_cities.py`; tests are in
`re/scripts/test_savegame_cities.py`.

## Fresh-SVX splice

The user-created SVX has compressed SHA-256
`161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7` and
decompressed SHA-256
`fa31f89f9bf435e28847e9e54d62cfe712e6bafc6cedb9ab98c8ab6730adb7c8`.
The exclusive chain from Leaders through Armies reaches Cities without a byte
search:

| stream range | exact owner | fresh image |
|---|---|---|
| `0x2701f..0x27020` | `Cities::walk_test`, `StringTable[513]` | tag `0x00` |
| `0x27020..0x27024` | `lists[0].length` | 0 |
| `0x27024..0x27028` | `lists[1].length` | 0 |
| `0x27028..0x2702c` | `lists[2].length` | 0 |
| `0x2702c..0x27030` | `lists[3].length` | 0 |
| `0x27030..0x27034` | `lists[4].length` | 0 |
| `0x27034..0x27038` | `lists[5].length` | 0 |
| `0x27038..0x2703c` | `lists[6].length` | 0 |
| `0x2703c..0x27040` | `lists[7].length` | 0 |
| next owner at `0x27040` | caller Forms tag, then `ObjectArray<Form>::walk_data` `0x00481190` | excluded |

The 33-byte fresh Cities image is all zero and has SHA-256
`7f9c9e31ac8256ca2f258583df262dbc7d6f68f2a03043d5c99a4ae5a7396ce9`.
As with Armies, an empty pointer array emits only its length.  This specimen
therefore does not contain or imply initialized CityPool allocation history.

The RCX fixture remains independent.  Its SHA-256 is
`558e0cd53dbed4f820e8757c0327a58384d433e5beb8e9eeef5d64df4d67bd54`;
the test separately confirms SVX seed `0x014810ac` and RCX seed `0x007f93e0`.
No identity, state, or boundary is joined between the artifacts.

## Outer `PtrArray<City>[8]` grammar

`Cities::walk_data` is at `0x00735410` (PDB length 1054).  Its first
`walk_test` uses StringTable byte offset `0x2814`; with 20-byte `String`
records this is index 513.  The loop starts at the global `Cities` object
`0x00c09960`, advances by `sizeof(PtrArray<City>) == 0x1c`, and stops at
`0x00c09a40`, proving exactly eight fixed owner rows.

Each owner emits:

```text
i32 length
if length != 0:
    i32 capacity
    i16 increment
    u8  flags                    # writer persistently clears bit 0x40
    u8  presence[length]         # exact values 0 or 1
    i32 repeated_capacity
    i16 repeated_increment
    for each presence == 1:
        City body
```

The direct `[PtrArray+8, PtrArray+0x0e)` walk at `0x00735773` repeats the
capacity and increment after the presence plane.  Writer-produced images must
carry the same values in both locations.  The parser rejects contradictions,
but it does not replace valid history with logical length: a synthetic
`length=3, capacity=23` round-trips as 23, and a Caravan child capacity of 13
round-trips independently.

There is **no per-City walk-test byte**.  The exact-class path at
`0x007357a9` begins by walking `City+4` immediately.  The load branch allocates
and constructs exact 192-byte `City` objects for every present pointer at
`0x0073571a`; the matched PDB contains no class derived from City.  The
otherwise present virtual-dispatch branch is therefore not a hidden save type
code.

## Complete City save body

For every present pointer:

```text
u16 city_flags                       # PDB type short; bit 0 is active
if city_flags & 1:
    bytes City[+6..+114)             # exact 108-byte POD
    String name                      # u32 code-unit count + UTF-16LE units
    String id                        # same grammar
    Array<CaravanLink> vans
```

The inactive arm is exactly two bytes.  An active body first walks
`[City+4,+6)` and `[City+6,+0x72)`.  SaveGame has `DataWalk::checksum == 0`,
so the branch at `0x007357cd` walks `name` at `City+0x90` and `id` at
`City+0xa4`.  Only then does it walk `vans` at `City+0x74`.

That stream order—POD, name, id, Caravan array—is not memory-offset order.
`vans` resides before the two Strings in the object, but its call comes after
them.  `City::walk_data` at `0x00489220` independently proves the same order.

`String::walk_data` at `0x00a1b2d0` zero-extends the PDB's 16-bit
`String::curr_len` to a four-byte stream count and then emits exactly that many
UTF-16LE code units, without a terminator.  Zero length emits only the count.
The helper preserves the raw 16-bit units rather than Unicode-normalizing
them and refuses counts a writer's `unsigned short curr_len` cannot produce.

## City POD and Caravan child

The PDB's fixed City fields cover `[+4,+114)` without a gap:

| City range | fields | type |
|---|---|---|
| `+4..+6` | `city_flags` | `short` |
| `+6..+12` | `city`, `o`, `reg` | 3 × `short` |
| `+12..+20` | `x`, `y` | 2 × `Coord` |
| `+20..+44` | `attack_stamp`, `raid_stamp`, `reduce_stamp`, `capture_stamp`, `assimilation_timer`, `capture_strength` | 6 × `int` |
| `+44..+76` | `traded_with` | `int[8]` |
| `+76..+86` | `scouted`, `in_port`, `peasant_dist`, `trade_val`, `conquest_node` | 5 × `short` |
| `+86..+94` | `granary`, `lumber_mill`, `smelter`, `refinery`, `free`, `busy`, `gatherers`, `pop` | 8 × `unsigned char` |
| `+94..+97` | `who`, `race`, `founder` | 3 × `char` |
| `+97..+105` | `plundered`, `ocean`, `land`, `filled`, `bordering`, `ocean_filled`, `dock_tile`, `was_capital_flags` | 8 × `unsigned char` |
| `+105..+108` | `space` | `unsigned char[3]` |
| `+108..+114` | `ter` | `unsigned char[6]` |

The two padding bytes at `+114..+116` are not walked.  The following child is
`Array<CaravanLink>` at `+116`, `sizeof == 28`; each `CaravanLink` is exactly
`{ cara:i32, who:i32 }`, eight bytes.

`Array<CaravanLink>::walk_data` at `0x00489040` emits:

```text
i32 length
if length != 0:
    i32 capacity
    i16 increment
    u8  flags & 0xbf
    CaravanLink links[length]         # cara, then who
```

Unlike the outer pointer array, this value array has no repeated capacity/
increment pass and no presence plane.  A zero-length child emits only four
bytes.  Its capacity and growth history are still stream-owned whenever a link
exists and must never be reconstructed from a host `Vec`.

## PDB and executable receipts

The matching PDB has SHA-256
`334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`,
GUID `51D4F219-61C6-4F84-9D5B-C3361B0D291F`, age 1.  Its JSON export has
SHA-256
`399aeac3bfb7a201a907e49e51388bb4e010420a54ccb781b335828315f28f14`.
The helper loads it at runtime and validates:

- `sizeof(Cities) == 232`, with `PtrArray<City> lists[8]` at `+0`, size 224;
- `sizeof(PtrArray<City>) == 28` and all six compiler fields;
- `sizeof(City) == 192`, its complete fixed/POD/dynamic field map;
- `sizeof(Array<CaravanLink>) == 28` and its concrete array fields;
- `sizeof(CaravanLink) == 8`; and
- `sizeof(String) == 20`, with `curr_len: unsigned short` at `+8`.

The canonical receipt over those exact records has SHA-256
`24fe2a7a1980c4d2a37b9f8f03de1df2afe80cfa1555dc6351b5dd0029ef3a5a`.

The matched PE SHA-256 is
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`.
Tests freeze complete spans for Cities, City, Caravan-array, and String
walkers, the main caller's Cities call, the following Forms walk-test sequence,
and the following `ObjectArray<Form>` walker.

## Independent canonical CityPool comparison

The landed Sim CityPool model is used only after deriving the save grammar.
`Cities::init` `0x007358c0` independently constructs eight owner arrays with
twenty present City objects each; the PE proves canonical outer history
`length=capacity=20`, `increment=-1`, flags zero.  The synthetic canonical save
retains those exact headers rather than replacing them with a logical host
shape.

One active City with two Caravan links and 159 inactive City slots gives a
772-byte save image.  Projecting the synchronization direction requires:

1. excluding the Cities tag and outer pointer-array histories/presence planes;
2. excluding inactive City flags, because `CheckSums::check_cities` filters
   them before dispatch; and
3. excluding both Strings, because checksum visitors have
   `DataWalk::checksum != 0`.

What remains is exactly `city_flags + 108-byte POD + Caravan array`, 137 bytes
for the two-link fixture.  The test independently builds this byte image and
matches the landed `cities_runtime::city_walk_bytes` authority.  Renaming the
City changes the save digest while leaving that projection unchanged.

This comparison does not authorize normalizing either outer or Caravan
allocation history.  Both histories remain first-class parsed state.

## Mutation and boundary proof

The synthetic fixture covers empty/nonempty owners, present/absent pointers,
inactive/active Cities, all named POD fields, two nonempty UTF-16 strings, and
a nonempty Caravan child.  Every owned-byte single-bit mutation either fails a
structural invariant or changes the parsed receipt and digest.  Every
truncation fails.  Dedicated failures cover both levels of length/capacity/
flags history, boolean presence, repeated outer headers, impossible String
length, and malformed Caravan history.  A mutation at the first Forms byte is
excluded.

`WalkDataGame::walk_data` calls Cities at `0x005a2acf`.  After it returns, the
caller emits `walk_test(StringTable[2690])` at `0x005a2ade..0x005a2af3`, then
calls `ObjectArray<Form>::walk_data` at `0x005a2af5`.  That native call order,
not the all-zero following bytes in the fresh artifact, establishes the exact
`0x27040` boundary.

## Reproduction

```sh
python3 re/scripts/test_savegame_cities.py

python3 re/scripts/savegame_cities.py \
  "ron-data/savegames/new save game 2026.08.11 15'42'57 (Tue).SVX" \
  --offset 0x2701f

cargo test -p don-replay --test cities_runtime
```

The returned `end` is the exact beginning of the following Forms owner.
