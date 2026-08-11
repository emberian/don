# Replay Cities runtime: exact typed walk and setup boundary

Lane: `replay-cities-runtime` · replay-first Gen-8 prerequisite · 2026-08-11.

## Result and claim boundary

`crates/don-replay/src/cities_runtime.rs` provides an exact checksum-direction
`City::walk_data` byte stream over the existing typed
`don_sim::systems::tech_cities::CityRecord`, plus a fail-closed channel adapter over
`CityPool`. The channel adapter preserves the shipped owner-major/full-array order and
checks that each live City resolves to the same canonical center Build, owner, object id,
city slot, encoded position, and current Build type already held by `don_sim::tick::Sim`.

This is not an initial-Cities producer and it is not installed in the replay bridge.
`Sim` does not retain a `CityPool`, and replay setup does not yet execute the exact
map-start-to-leader schedule or the city/build transaction. No recorded checksum is an
input, no checksum was fitted, and no Cities-channel agreement is claimed.

The authority is the supported `ron-bin/riseofnations.exe`, SHA-256
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`, and its
GUID-matched `ron-bin/sbl/rise.pdb`. Procedure names and layouts come from the PDB; branch,
call, and emitted-byte order come from the PE32 instruction stream. This is static/in-tree
Tier C evidence. Nothing in this lane was executed inside retail.

## Complete Cities traversal

`CheckSums::check_cities` is `0x00937600`. Its outer loop has exactly eight iterations.
For each fixed owner slot it reads `LeaderData::leader_flags & 1`; an invalid owner is
skipped. A valid owner is traversed from slot zero through the complete logical
`PtrArray<City>.length`, not through `LeaderData::city_mark` and not through a compact list
of live rows. A slot whose `city_flags & 1` is clear contributes no bytes. An exact `City`
vtable is inlined; a derived object is dispatched through virtual `walk_data`.

The typed owner contains concrete `CityRecord` values only, so the adapter implements the
exact-class path. It scans owners `0..8`, then all logical slots for that owner, and walks
only live rows. The checksum starts at Adler-32 seed 1, as every independent channel does.
It never walks `Sim::builds` in dense row order.

`city_mark` is deliberately treated as owner state rather than a checksum bound. It is the
high-water mark used by `Cities::init_city` `0x007352c0` for first-hole reuse. The adapter
requires every live row to be below it and requires a nonnegative mark no larger than the
logical array length, but hashes against full array length just like retail.

## Exact `City::walk_data` stream

The complete checksum path of `City::walk_data` `0x00489220` is:

| instruction/call | bytes handed to `CheckSum` |
|---|---:|
| `0x00489237` | `city_flags`, `[City+4, City+6)`, 2 bytes |
| active-bit gate | dead rows stop after the flags call; the outer channel never calls them |
| `0x0048924a` | fixed POD `[City+6, City+114)`, 108 bytes |
| `0x0048924c` | test `DataWalk::checksum` at visitor `+8` |
| `0x00489253`, `0x00489265` | two `String::walk_data` calls only when checksum is zero |
| `0x0048926f` | `Array<CaravanLink>::walk_data` for the member at `City+116` |

The two strings (`name` at `+144`, `id` at `+164`) are save/load state but are absent from
the synchronization checksum. Renaming a City therefore changes neither this stream nor
its Adler value.

`Array<CaravanLink>::walk_data` `0x00489040` is container-specific; its metadata cannot be
inferred from a generic `Array<T>` description:

| instruction | checksum-direction contribution |
|---|---|
| `0x00489074` | signed logical length, copied to a 4-byte stack temporary |
| zero-length branch | returns immediately after the length |
| `0x004890d5` | signed capacity, copied to a 4-byte stack temporary |
| `0x004890e3` | growth hint at array `+12`, 2 bytes |
| `0x004890e5..0x004890fb` | array flags masked with `0xbf`, 1 byte |
| `0x00489201` | each `{ cara:i32, who:i32 }` row, 8 bytes, in logical order |

Consequently an active City with no caravan links walks **114 bytes**, not 110: two flags,
108 POD bytes, and the four-byte zero array length. A nonempty array of `n` links walks
`121 + 8*n` bytes; one link walks 129 bytes. `City::init` reserves capacity 10, but capacity
is not emitted while length is zero. That allocation history becomes checksum-critical as
soon as a link exists. The isolated walker also preserves the complete inactive arm: a
direct call on a dead City emits its two flag bytes and returns, although
`CheckSums::check_cities` filters such a row before dispatch.

## Why the generic flat-image projection is incomplete

The generated state schema correctly identifies the 110-byte fixed City span but cannot
execute its control flow or recover pointer/container state:

- generated `OPS_41` recurses into both `String` members without evaluating the
  `visitor+8 != 0` checksum guard;
- it recurses into an `Array<CaravanLink>` embedded at `+116`, but that array schema knows
  only the two directly addressed growth bytes;
- length, capacity, and masked flags are stack temporaries in the extracted schema and are
  marked unresolved;
- element storage is pointer-owned and the loop operand at `0x00489201` is unresolved; and
- a flat 192-byte City image therefore walks 112 resolved bytes, reports unresolved ops,
  and cannot reproduce even the exact empty 114-byte stream.

The adapter does not patch or special-case the generated walker. It obtains the logical
array and allocation metadata from the typed `CityRecord` owner and constructs only the
checksum-direction stream supported by the instruction sequence.

## Present ownership and fail-closed join

The necessary state is currently split:

| fact | current canonical owner |
|---|---|
| 8 × logical City arrays, fixed POD, caravan rows/history | standalone `tech_cities::CityPool` |
| `city_mark` high-water values | currently copied into `CityPool`; retail stores them in `LeaderData` |
| Build rows | `Sim::builds` |
| owner Build bands and object ids beginning at 2000 | `Sim::world.objects` |
| current Build ptype | `Sim::production_runtime.build_types` |
| retail `leader_flags & 1` fact | presently mirrored by `vic_leaders`, `step8`, economy leaders, and the object registry |

`check_sim_cities` refuses rather than selecting a convenient mirror. All four leader
validity views must agree. Every active City, including one below a currently invalid
leader, must have a valid slot/owner identity and resolve through that owner's Build band
to exactly one active Build row. The Build's owner, `SubObjectData::o`, city index, decoded
position, and current ptype presence must agree with the City.

This validation exposes the same concrete creation gap found by the Builds lane:
`Sim::spawn_build` installs the row in the owner's Build band but does not write the
returned object id to `BuildData::other[OBJECT_ID]`, and it cannot infer the retail encoded
position. A City joined to an otherwise current `spawn_build` row is therefore refused as
`CenterBuildObjectIdMismatch`; the adapter never repairs the image.

## Earliest ordinary producer and precise freeze

The shipped ordinary-game call chain is:

| VA | PDB procedure | City-relevant behavior |
|---:|---|---|
| `0x005ac190` | `Setup::build_game` | orders player setup and supplies the map-start index |
| `0x005abb80` | `Setup::build_empire` | calls `build_cities`, then passes its returned Build object id to `build_units` |
| `0x005ab910` | `Setup::build_cities` | reads start arrays, creates the first center Build, and runs city activation follow-ups |
| `0x007352c0` | `Cities::init_city` | first-hole allocation below `city_mark`, otherwise mark/length growth; calls `City::init` |
| `0x00737050` | `City::init` | initializes the walked City body and the caravan allocation history |
| `0x00735c90` | `City::generate_name` | ordinary city-name path; consumes global naming/RNG state even though strings are not checksummed |

`Setup::build_cities(leader, start_index, mode)` reads `World` start-x and start-y arrays at
the supplied index and reads the start tile's region. It gates creation on
`GameInfo::starting_town` and the observer bit, converts the selected tile to world units,
asks `Objects::find_free` for an id in `[2000,3000)`, applies
`BuildTypeData::snap_center`, and calls the Build initializer with literal type `0x19e`
(`VILLAGE`). Later calls activate/upgrade the center, create the City, and return the Build
object id used by starting-unit setup.

`City::init` copies the slot, center object id, owner, region, and center coordinates into
the walked body; initializes stamps and counters; updates leader/game/region population
state; reserves ten caravan links; and follows the ordinary name-generation branch. Thus a
standalone synthetic City record would omit both shared mutations and allocation/RNG
history even when its 114 immediate bytes happened to look plausible.

The exact producer boundary is therefore frozen **before** `Setup::build_game` selects the
second argument passed to `build_empire`. The large caller has two selection paths: one
uses its evolving local start cursor and another indexes a caller-populated local mapping
by leader slot, intertwined with scenario/team ordering. The replay port must first finish
map generation's start arrays/regions and recover that assignment transaction. Active
leader order, team order, or replay player order is not substituted for the missing fact.

## Tests and corpus boundary

`crates/don-replay/tests/cities_runtime.rs` covers:

- the exact 114-byte empty and 129-byte one-link City streams;
- checksum exclusion of both City strings and masking of array flag bit `0x40`;
- the generic flat-walker incompleteness (`112` resolved bytes plus unresolved ops);
- fixed leader then full-`PtrArray` order even when dense Build rows are reversed;
- invalid-leader skipping, City marks, slot/owner identity, center-Build uniqueness;
- leader-mirror splits, missing current ptype, Build city/position mismatch; and
- the current `Sim::spawn_build` object-id gap.

The corpus test reopens local `.rcx` files only to measure the red boundary. In the known
61-file corpus, 21 recordings carry `CheckSumsCommand` `0x39`; their first checkpoint is
turn 2, every first Cities word is nonempty, and all 21 first values are distinct. The
production channel source remains `Absent`. Those recorded words are outputs used for
falsification only and never enter the adapter.

## Shared hooks needed later (not edited by this lane)

1. Register one `pub mod cities_runtime;` line in `crates/don-replay/src/lib.rs`.
2. Move one canonical `CityPool` into `Sim` (with `city_mark` ultimately co-owned by the
   canonical Leader state rather than silently duplicated).
3. Make Build allocation, ptype registration, encoded position, leader validity, City slot
   allocation, `City::init`, and every population/region side effect one atomic setup
   transaction.
4. Recover the replay-bound `Setup::build_game` start-index assignment only after map
   generation has produced the exact start arrays and regions.
5. Run `build_cities` before `build_units`, preserving the returned center Build id and
   name-generation/RNG history.
6. Only after that producer exists, install `CitiesChannelValue` directly as channel 9 and
   let independent corpus comparison decide whether the reconstruction advanced.

Until those hooks land, `ChannelSource::Absent` is the only honest replay status.
