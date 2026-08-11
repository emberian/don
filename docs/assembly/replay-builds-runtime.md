# Replay Builds runtime: canonical owner join and exact inherited walk

Lane: `replay-builds-runtime` · replay-first Gen-8 prerequisite · 2026-08-11.

This tranche makes one bounded producer honest: given an already-created `don_sim::tick::Sim`
Build row plus explicit authority for the pointer/container state outside its 220-byte image,
`crates/don-replay/src/builds_runtime.rs` emits the exact shipped `BuildData::walk_data` byte
order and executes the exact per-owner band traversal of `CheckSums::check_builds`.

It does **not** construct retail's starting city and does not match or consume a recorded
checksum. The first-turn Builds channel remains red until replay setup owns the actual rows.

## Why Builds, not Units

The replay harness already has a Units bridge: `state::SimBridge::populate(&don_sim::World)`
walks `World.units` through the canonical object registry in `check_units` order. Its present
first-turn failure is upstream: setup creates no retail-derived starting units.

Builds had the opposite architecture. `don_sim::tick::Sim` already owns:

- `builds: Vec<production::BuildData>`;
- `world.objects`, including every owner's dense band beginning at object id 2000; and
- `production_runtime.build_types`, the current type index by Build row.

But replay's `WorldSim` owns only `don_sim::World`, and the replay bridge has no Builds
adapter. Closing this owner join is higher leverage because retail creates the starting city
*before* the starting units and passes the city object id into unit setup.

The RCX corpus rejects a constant or synthetic prefix: over the 21 checksum-bearing
recordings, `units` and `builds` both have **21 distinct values on the first checksummed turn**,
and both are non-empty from turn 2 in every recording. That result is recorded in
`docs/assembly/groups-initial-state.md` and `docs/assembly/check-all.md`. Recorded channel
bytes are used only as an eventual comparison oracle.

## Shipped setup chronology

PDB procedure names and VAs are independently listed in `schema/rise-procs.tsv` and
`schema/rise-symbols.tsv`:

| VA | shipped PDB name | relevant behavior |
|---:|---|---|
| `0x005ac190` | `Setup::build_game` | schedules per-player empire setup |
| `0x005abb80` | `Setup::build_empire` | calls `build_cities`, then calls `build_units` with its return |
| `0x005ab910` | `Setup::build_cities` | reads map start arrays and `GameInfo::starting_town`; initializes type `0x19e` in `[2000,3000)` |
| `0x005aafc0` | `Setup::build_units` | creates rule/tribe-dependent starting units around the returned city |

`re/decomp-all/005abb80.c` shows the call order directly. In
`re/decomp-all/005ab910.c`, a nonzero `Game+0x2c` starting-town byte and a non-observer
leader select a map start coordinate, allocate the first Build-band object, call Build
initialization with literal type `0x19e`, and then run activation/city follow-ups.
`InitialState` already parses the replay's `starting_town` byte and `InitialWorld` carries
the map start arrays. The missing prerequisite is the exact `Setup::build_game` assignment
from leaders/teams to start-array indices; active-slot order is not substituted for it.

## Exact Builds channel owner order

`CheckSums::check_builds` is PDB `0x00937290` (`checksums.cpp:646`), 203 bytes. The body in
`re/decomp-all/00937290.c` pins:

```text
if Objects+0x1f4 is initialized:
  for owner = 0..7, fixed order:
    if leaders[owner].flags & 1:
      for object_id = obj_base[1] .. build_mark[owner]:
        build = objects[owner][object_id]->get_build()   # vtable +0xac
        if build.flags & 1:
          build->walk_data(checksum)                     # vtable +0x7c
```

The adapter therefore does not walk the dense `Sim::builds` vector. It first validates that
every row appears exactly once in an owner band, that `BuildData::who` matches the registry,
and that the `SubObjectData::o` short at `+0x0a` equals the band object id. Only then does it
hash active owners in fixed owner/band order. Owners 8 and 9 carrying Builds are refused:
the shipped loop never reaches their Build bands.

This validation exposed a concrete integration gap: current `Sim::spawn_build` receives the
band object id from `ObjectRegistry::insert` but does not write it into `BuildData + 0x0a`.
The adapter reports `BuildObjectIdMismatch`; it does not patch the checksum image.

## Exact `BuildData::walk_data` call order

The source of truth is the shipped PE bodies below, with class sizes/field offsets from the
PDB TPI stream (`schema/pdb-types.json`):

| VA | PDB procedure | contribution on the checksum path |
|---:|---|---|
| `0x0062f270` | `BuildData::walk_data` | derived fields, Wall base call, fourth must-walk byte, Build containers |
| `0x00642510` | `WallData::walk_data` | Object base call, third must-walk byte, `[+0x48,+0x66)` |
| `0x00647830` | `Object::walk_data` | SubObject base call, second must-walk byte, `[+0x20,+0x42)`, launching presence/array |
| `0x00647930` | `Object::must_walk` | emits its boolean result into the stream |
| `0x006621d0` | `SubObject::walk_data` | flags, first must-walk byte, `[+0x09,+0x18)`, dereferenced ptype id |
| `0x006305f0` | `BuildQueue::walk_data` | signed count, then first 18 bytes of each 20-byte entry |
| `0x00471c30` | `Array<TCoordData>::walk_data` | mining length/header and 8-byte coordinates |
| `0x004708a0` | `PtrLinkListAbstract<GatherPoint>::walk_data` | count; per node zero token, node tag, 9-byte point body |
| `0x00473120` | `SimpleArray<int>::walk_data` | launching length/header and packed i32 elements |

For one valid Build with null launching and empty queue/mining/gather containers, the stream
is 131 bytes:

```text
founder[1], max_age[1]
flags[1], must[1], subobject[15], ptype_index[4]
must[1], object[34], launching_present[1]
must[1], wall[30]
must[1], build[22]
queue_count[4]
mining_tail[2], mining_length[4]
gather_count[4]
orig_type[4]
```

The four `must` bytes are not commentary: every level calls `Object::must_walk`, and its
result is itself handed to the visitor. A valid object always makes the checksum-path
predicate true because `flags & 1` is set, but all four literal `1` bytes still affect Adler.

### Correction to the older production helper

`production::BuildData::walk` is useful for production-owned windows, but it is not a full
retail Build checksum walk. Relative to the PE chain above, it omits:

- all four emitted `Object::must_walk` result bytes;
- SubObject flags, `[+0x09,+0x18)`, and the dereferenced ptype id;
- the launching presence byte and `SimpleArray<int>` body;
- `Array<TCoordData>`'s capacity/increment/flags and 8-byte element shape; and
- the four-byte zero class/factory token emitted before each gather-list node.

The replay adapter does not call that helper. It uses `BuildData::image` for owned scalar
windows and explicit `BuildWalkFacts` for launching plus the engine-shaped
`GatherMiningList`. Mining `mtn`/`cliff` must agree between the record and sidecar or the
walk refuses. A non-empty legacy `BuildData::gather_from.tiles: Vec<u32>` also refuses: it
cannot be proven equal to retail's 8-byte `TCoordData` rows, and silently choosing one of two
live payloads would create split authority. Empty first-turn city mining remains supported;
later Mine state requires the owner migration listed below.

## Dynamic-container authority

Pointer-nullness and array history are checksum state:

- `launching = None` is an explicitly asserted null pointer;
- `launching = Some(empty)` emits a presence byte plus a four-byte zero length;
- a non-empty `EngineArray` additionally emits capacity, increment, `flags & !0x40`, then
  elements;
- mining uses an `EngineArray<GatherTile>` because every TCoordData is two i32 values and
  the header is walked; and
- gather-list order, per-node tag, and per-node zero token are preserved.

Consequently `BuildsWalkAuthority` has one optional facts row per `Sim::builds` row. Missing
facts on an invalid Build are harmless because retail skips it before `walk_data`; missing
facts on a valid Build are a typed stop. No default-null inference occurs at the channel API.

## Tests and current boundary

`crates/don-replay/tests/builds_runtime.rs` covers:

- exact inherited-prefix offsets and all four gate bytes;
- null versus present-empty launching arrays;
- mutation and byte-length sensitivity for launching, queue, mining and gather containers;
- owner-then-band order even when dense row order is reversed;
- inactive-owner and invalid-object skips;
- missing ptype/dynamic authority;
- current `Sim::spawn_build` identity mismatch;
- duplicate, out-of-range and unregistered rows; and
- unsupported owner slots and mining-tail split authority.

This proves an executable owner/runtime/adapter slice once a Build exists. It does not prove
initial city construction, the leader-to-start-index schedule, or a match against the RCX
Builds channel.

## Hooks needed (not edited by this lane)

1. Register one `pub mod builds_runtime;` line in `crates/don-replay/src/lib.rs`.
2. Give replay simulation a `don_sim::tick::Sim` owner (or an equivalent exclusive adapter),
   rather than only `don_sim::World`.
3. Make Build allocation atomically write the returned band object id into
   `BuildData + 0x0a` and register the current ptype in `production_runtime.build_types`.
4. Retain per-Build launching state and migrate the legacy packed mining payload to the
   existing engine-shaped `GatherMiningList`; only then install `BuildWalkFacts`.
5. Implement the replay-bound `Setup::build_game -> build_empire -> build_cities` producer.
   Stop at the unresolved start-index assignment instead of assuming active-player order.
6. Feed the resulting `BuildsChannelValue` into channel 1 and let the corpus comparison
   decide whether the reconstruction advanced. Do not use the recorded checksum as input.
