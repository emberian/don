# Deterministic save/load

## Status and scope

`systems::save_load::{save_sim, load_sim}` is an executable, deterministic roundtrip for
the authoritative headless state currently owned end-to-end by `don-sim`. It is not a
Rise of Nations `.svx` writer. The distinction is load-bearing: the retail save walker has
91 top-level operations, including pointer-owned systems that this crate cannot yet
reconstruct. Writing a stream with those sections omitted would look successful and resume
from different state.

The first supported tranche contains:

- `WorldData::seed`, `Game::frame`, `Game::seconds`, and the exact `Random` state word;
- the complete `map_terrain::World`: scalar geometry/counters, all six start/oil arrays,
  start-city mask, every `WData` and optional 768-bit collision block, tile data, eight
  danger planes, four fog planes, and all four terrain-sync arrays;
- mutable fog policy, region border cursors/coordinates, and ordered border sources;
- every generated `UnitData` column plane, the full stable-handle permutation and
  generation array, live/capacity counts, cached movement steps, unit type ids, per-unit
  order queues, path stacks, and path-unit facts;
- the object registry facts derivable from each live unit's `(who, o)` address, plus the
  ten owner-active bits;
- the eight inactive leader economy blocks, recompute stamps/dirty flags, and market
  state;
- checksum channel 10's authoritative item runtime, including producer absence versus
  initialized-empty, every live and dead stable slot, and its checksum channel 12
  `WData` occupancy coupling;
- ordinary construction and production records: the full `BuildData` image, construction
  counters/HP/masks/helpers, exact `(who,o,uid)` identity, logical and allocated queue
  lengths, every live or stale queue record, elapsed progress, repeat latch, and the
  per-player band-2000 traversal rows.
- constructor-empty and exact post-sync step-8 adapter views. The latter are admitted only
  when their leader economy tuple exactly mirrors the serialized `LeaderSlot` and every
  unit/build view exactly mirrors the serialized `World`/`BuildData` row. They are derived
  again before step 8 after load; no second object or leader owner is written.

The save call rejects a live section before returning bytes when that section cannot yet
be restored exactly. Current explicit refusals include active leaders, independent step-8
query packages/counters/hosts, walls,
herds, groups, Wonders, projectiles, death records, crash hosts,
unsupported static-world rules, modified economy/territory rules, and modified circle
tables. Heterogeneous item/object occupancy and captured, Wonder, gather, garrison,
razing, or externally linked building families are refused until their mandatory world
owners can preserve the complete transaction. This is a bounded tranche, not an
allow-list intended to make unsupported games look saveable.

## Recovered retail primitives

The implementation reuses the shipped chunk semantics rather than inventing a container
header:

- PDB `ChunkHeader`, size 8: `u32 size` at `+0`, `u16 id` at `+4`, `u16 num_chunks` at
  `+6`.
- `ChunkWrite::chunk_begin` `0x00A48390` reserves eight bytes, increments the parent's
  child count, and initializes the new header.
- `ChunkWrite::chunk_write` `0x00A48460` adds copied bytes to every open header's size.
- Consequently, `size` includes the chunk's own eight-byte header and every nested byte;
  a parent's size includes every child.
- `ChunkRead::init` `0x00A48500`, `open_chunk` `0x00A48570`, and `close_chunk`
  `0x00A485B0` use those same bounds.

The top-level retail save stream is a separate raw walker:

- `SaveGame::walk_function` `0x0043D730` writes exactly the visited byte range;
- `LoadGame::walk_function` `0x0043D950` reads the mirror range;
- `SaveGame::walk_test` `0x0043D840` / `LoadGame::walk_test` `0x0043DA60` write and check
  the one-byte section tag;
- `SaveGame::save_game` `0x005A8220` writes the save magic, version 16, then reaches
  `WalkDataGame::walk_data` `0x005A2360` through `do_save` `0x005A81F0`.

That inventory is why DoN uses its own `DoNSave\0` magic and format version. Container
compatibility is claimed only for the measured `ChunkHeader` behavior, not for `.svx`
object-graph coverage.

## Container and validation

The root chunk (`0x444e`) has seven required leaf children in deterministic order. The
current DoN format version is 2; version 1 predates authoritative item state and is
rejected rather than being interpreted as an absent producer.

| id | section |
|---:|---|
| `0x0001` | format version, map seed, frame, seconds, RNG |
| `0x0002` | terrain/map/fog/border state |
| `0x0003` | generated unit columns, objects, handles, orders |
| `0x0004` | leader economy and market |
| `0x0005` | unit type/path state |
| `0x0006` | item-producer state and exact stable-slot records |
| `0x0007` | `BuildData`, construction state, and production queues |

Loading rejects unknown, missing, duplicate, nested top-level, escaping, truncated, or
trailing chunks. It also bounds the total stream, map geometry, population, order count,
and path count before allocation. Booleans must be canonical `0` or `1`; order tags must
be one of the PDB-derived 28 `OrderIndex` values.

The world import adapter does not trust serialized derived indices. It validates:

- exact length agreement among live columns, order/type/movement side vectors, capacity,
  handle permutation, and generation storage;
- a true permutation of every handle id in `0..capacity`;
- valid owner slots and dense, unique per-owner `o` indices;
- no building or wall band smuggled into the unit-owned adapter.

Only then does it rebuild `row_of_handle` and `ObjectRegistry` into a replacement `World`.
The live value is swapped at the end, so malformed state cannot leave a partially mutated
world. Registry traversal scratch and coverage counters are derived/instrumentation and
are reset rather than serialized.

The Items leaf starts with an explicit producer-state byte. `0` means the runtime is
absent and requires that the map contain no item flags or sentinels. `1` means the
runtime is initialized, even when its stable-slot count is zero; this preserves the
difference between an unavailable channel and retail Adler-32's initialized-empty value
of `1`. Present state stores the attached map shape and every checksum/lifecycle field
from each retail 44-byte `Item` object, including dead records. Dead slot identity is
observable because the next item creation reuses the first dead slot before growing the
registry.

Save and load both reconcile all live slots with the channel-12 map plane: each live
record must have one and only one `WFLAG_ITEM` / `DOWN_ITEM` cell whose `down_who` is that
stable slot and whose coordinates match its snapped center. Orphan slots, duplicate or
out-of-range references, malformed records, mismatched map shapes, and item flags behind
a nonnegative heterogeneous object head all fail closed.

The object section also stores every owner's band-2000 row vector. Those vectors must be
a permutation of the `BuildData` array, are limited to retail's eight building-owner
slots and 601 slots per player, and are rebuilt in the same order. Each body must agree
with the resulting absolute `(who,o)` address (`o = 2000 + band slot`). This preserves
both stable target identity and the fixed per-player traversal/checksum order rather than
reconstructing buildings from a global vector order.

The Builds leaf stores the complete Rust-owned `BuildData` state plus each dynamic
allocation. In particular, queue `num` remains distinct from the `u8 queued` logical
length, records after `queued` remain present, the non-walked two-byte record tail is
preserved, and `REPEAT_QUEUE` is not derived. Loading bounds every allocation and rejects
logical lengths beyond their physical arrays, negative live type/progress values,
unregistered or multiply registered bodies, and queues attached to unfinished sites.

Queue completion still requires type classification, `Build::finished`, stockpile,
per-player/type queued-counter, and repeat-payment hosts. Those owners are not fabricated
or serialized as a shadow. A loaded queue can resume the exact owned progress kernel, but
the complete finish/unqueue transaction remains fail-closed until a caller supplies the
same mandatory hosts.

## Dynamic-array metadata

Retail `SimpleArray<T>::walk_data` writes a nonempty array as length, capacity (`size`),
signed 16-bit increment, flags byte, and elements. Capacity, increment, and flags affect
the walked/checksummed image; reconstructing only the logical elements is not exact.

DoN preserves all four values even for an empty array. On load, capacity must be
nonnegative and at least the logical length. This slightly stronger DoN container rule is
necessary because the retail walker omits the other three fields for an empty array and
therefore cannot preserve them by itself.

## Executable determinism gate

Focused tests build a state containing two units, queued orders, a nonempty path, mutated
leader economy/market values, terrain/fog bytes, a collision block, and nontrivial
`WalkedArray` metadata. They assert:

1. save -> load preserves the composed `Sim::channel_digest`, RNG state, map checksum,
   order queues, and byte-for-byte save image;
2. advancing both original and loaded simulations by one complete `Sim::do_frame`
   produces the same digest, frame, RNG state, and remaining orders;
3. header sizes/child counts and array metadata survive exactly;
4. corrupt sizes, ids, duplication, trailing bytes, handle permutations, order tags, and
   unsupported live sections fail closed;
5. a failed private-world import leaves the original digest, RNG, and handle permutation
   unchanged;
6. absent and initialized-empty item producers remain distinct, while live/dead stable
   slots, channel-10 evidence, channel-12 checksum, and first-dead-slot reuse roundtrip;
7. corrupt item identity, orphan map markers, and heterogeneous object/item occupancy are
   rejected;
8. an in-progress construction site resumes through the object scheduler with identical
   checksum state, while unit and research queue progress, allocated stale records,
   repeat state, and byte-for-byte resave output agree;
9. corrupt band identity, impossible queue lengths, and unsupported special building
   families are rejected.
10. an otherwise-supported simulation can save immediately after a complete frame even
    though step 8 materialized leader/unit/build mirrors; loading drops those derived views,
    the next step rebuilds them, and any non-derived package, counter, or stale row refuses.

## Next ownership cuts

Retail-complete saves require the remaining owners to join one receipt-checked
transaction, especially active step-8 leaders/victory, special-family buildings and
walls, groups,
projectile/death pools, Wonders and their mandatory world host, captured/gather/garrison
building families, production completion's leader/type/stockpile counter hosts, static
type tables, and script/runtime state. Adding a chunk without both exact export and exact
import is not progress: the correct behavior remains refusal until the complete lifecycle
is owned.
