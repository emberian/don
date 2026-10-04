# Replay Units runtime: exact bounded walk over sparse identity

Lane: `replay-units-runtime` · replay-first Gen-8 prerequisite · 2026-08-11.

This tranche answers a narrow question: given Units which already exist in
`don_sim::World`, plus explicit authority for the pointer/container state outside the
generated scalar columns, can replay emit the shipped Units checksum byte order without
silently falling back to the approximate generated walker?

`crates/don-replay/src/units_runtime.rs` does that for a bounded domain. It walks the exact
inherited `SubObject -> Object -> Unit` stream, retains engine-shaped launching/path
history, resolves rows through the canonical sparse `(who,o)` owner, and refuses dynamic
state which `World` cannot represent exactly. It is intentionally path-mounted only by its
focused integration test. It is not registered in the replay harness, does not construct
starting units, and makes **no Units-channel or corpus-match claim**.

## The existing projection is not retail-walk exact

`state::SimBridge::populate(&World)` iterates the dense compatibility `world.objects`
registry, images each generated `UnitCols` row, and sends that image through the generic
schema walker. That remains a useful schema-coverage audit. It is not an exact Unit
adapter:

- one initialized-empty active Unit reports **178 walked bytes** and **17 missed ordered
  operations**;
- the corresponding bounded retail stream is **186 bytes**;
- generated columns do not own the dereferenced ptype index, `ObjectData::launching`,
  exact `Stack<PathData>` allocation history, concrete OrderList nodes, or recursive Guys;
  and
- dense rows may compact, so a dynamic sidecar keyed by row can silently become attached
  to the wrong object.

The new adapter does not call the generic walker. The generated field table is used only
to materialize scalar bytes; the selected windows, inherited call order, dynamic payloads,
and gates are explicit PE-derived operations.

## Corpus deadline, not a fitted input

The fresh `schema/replay-validation.json` report has 21 checksum-bearing recordings. In
all 21, retail Units is already non-empty at turn 2 and our uninstalled initial producer
diverges at turn 2:

- **0 / 222,938** Unit comparisons match;
- there are **0** nontrivial comparisons and `our_bytes_walked = 0`;
- the current harness value is `0x00000001` in all 21 recordings; and
- the 21 first expected values are all distinct.

Those values establish that replay setup must create recording-specific starting state.
They were not used to choose any byte or constant in this adapter.

## Executable source anchors

The byte order comes from the shipped PE bodies, with names and layout checked against the
PDB-derived schema:

| VA | procedure | checksum responsibility |
|---:|---|---|
| `0x009371d0` | `CheckSums::check_units` | fixed owner-major outer traversal and active-object dispatch |
| `0x0060cf40` | `Unit::walk_data` | Object base call, Unit gate, `[+0x48,+0xb7)`, path/orders/Guys mask arms |
| `0x0060d040` | `Unit::must_walk` | concrete inherited gate predicate |
| `0x00647830` | `Object::walk_data` | SubObject base call, gate, `[+0x20,+0x42)`, launching presence/body |
| `0x00647930` | `Object::must_walk` | inherited virtual gate |
| `0x006621d0` | `SubObject::walk_data` | flags, gate, `[+0x09,+0x18)`, ptype dereference |
| `0x0046d8b0` | `Stack<PathData>::walk_data` | capacity, length, byte increment, 16-byte path records |
| `0x00730270` | `OrderList::walk_data` | count, node type/tag, concrete virtual payload |
| `0x0046df30` | `PtrArray<Guy>::walk_data` | array history, slot presence, recursive Guy walk |

The section mask installed by `check_units` is `-1`, so mask bits 2, 4, and 8 all execute.
The checksum visitor's test/tag callback is a no-op on this shipped path; tag strings are
therefore absent from the Adler stream.

## Exact initialized-empty accounting

For one active Unit with a null launching pointer, initialized-empty path stack
`(size=10, length=0, increment=-1)`, empty OrderList, and empty `PtrArray<Guy>`, the stream
has 186 bytes:

| level | contribution | bytes |
|---|---|---:|
| SubObject | flags, concrete must result, `[+0x09,+0x18)`, dereferenced ptype id | 21 |
| Object | concrete must result, `[+0x20,+0x42)`, null launching presence | 36 |
| Unit | concrete must result, `[+0x48,+0xb7)` | 112 |
| path | Stack capacity, length, byte increment | 9 |
| orders | signed zero count | 4 |
| Guys | signed zero count | 4 |
| **total** | | **186** |

There are exactly three emitted `must_walk` bytes, one at every inherited level. For an
active Unit each is `1`, but none may be omitted: each is passed through the checksum
visitor before its corresponding body. The three coordinate scalars in the SubObject
window are materialized with retail's `0x00063637` XOR representation. `ptype = null`
hashes a zero id; a non-null ptype hashes the signed index stored at `ptype + 4`, not its
address.

## Dynamic ownership and refusal

Pointer nullness and container allocation history are checksum state:

- `launching = None` emits only a zero presence byte;
- `launching = Some(empty)` emits a one presence byte and signed zero length;
- a non-empty launching array additionally emits capacity, signed 16-bit increment,
  `flags & !0x40`, then packed i32 elements;
- the path owner is `EngineStack<PathData>`, preserving capacity and increment before its
  16-byte records; and
- explicit initialized-empty orders and Guys each emit their four-byte zero count.

`World::orders` is a flattened `OrderList`, but most concrete virtual order payloads are
not represented. A positive retail list needs its exact node type, node tag, and concrete
walk bytes. The adapter therefore refuses every non-empty flattened list.

A positive `PtrArray<Guy>` needs array history, each pointer-presence value, and recursive
dispatch into the pointed-to concrete Guy. `World` owns no complete equivalent. The
adapter accepts only an explicitly authorized initialized-empty Guy array and returns a
typed stop for positive length. Missing Unit facts are never inferred as null/empty.

## Sparse identity and owner order

The exact checksum loop reaches owner slots **0 through 8** in fixed order. This is the
measured difference between `0x00e3a390` and `0x00e789dc` at stride `0x6eec`: nine slots.
The older `docs/mechanics/movement.md` statement that it reaches only `0..7` is stale.
Owner slot 9 participates in normal object processing but is outside the Units checksum.

`check_world_units` uses `World::object_bands()` as the canonical sparse authority and
walks retained Unit-band slots in ascending `o`. It validates:

- band mark and retained-slot shape, including tombstones and reservations;
- a parallel Unit projection (owners 8–9 legitimately use the `Animal` object class);
- live sparse `{id,generation}` resolution through `unit_row_at(who,o)`;
- agreement with the dense row's Handle, `UnitData::who`, and `UnitData::o`; and
- one sparse identity for every live dense row.

Per-Unit walk facts are keyed by stable `{id,generation}`, not by dense row, so swap-remove
compaction cannot transfer ptype/path facts between Units. Inactive leader slots and
inactive objects are skipped before dynamic authority is demanded. Slot 9 is audited and
reported as outside the walk, but never hashed.

## Tests and evidence level

`crates/don-replay/tests/units_runtime.rs` freezes:

- all three inherited gates, exact fixed windows, coordinate masking, and the 186-byte
  initialized-empty stream;
- null/present launching distinction, masked array flags, and path history/payload;
- owner-major order independent of dense row order;
- stable facts across dense compaction;
- inactive-owner/object and owner-9 behavior;
- fail-closed missing facts, orders, and Guys; and
- the current generic bridge's measured 178-byte / 17-missed gap.

This is an executable, instruction-derived structural prerequisite with focused source
tests. It has no live retail differential and no RCX channel installation, so it does not
promote a fidelity tier or claim behavioral parity.

## Next producer and shared hooks (not edited here)

1. Register one `pub mod units_runtime;` line in `crates/don-replay/src/lib.rs`.
2. Reconstruct replay-bound `Setup::build_units` at `0x005aafc0`, called by
   `Setup::build_empire` after `build_cities`, and atomically allocate its sparse identities.
   The existing `objects_init_unit_authority_frontier` is a receipt validator, not a full
   starting-Unit producer: it does not own sparse reuse, leader accounting, nearby
   placement, or all Unit links.
3. Expose or install canonical live ptype authority. `World` currently keeps its Unit type
   sidecar private to the owner.
4. Map the replay owner's exact path container history into `tick::Sim.paths`. The Sim-side
   `movement::PathStack` now owns capacity/increment and DoNSave v20 preserves them, but the
   replay-to-Sim composition still needs to prove and publish the same header with the records.
5. Add canonical ownership for `ObjectData::launching`, concrete OrderList nodes, and
   recursive `PtrArray<Guy>` state.
6. Only after those owners are populated should replay install this direct Units channel
   and measure the corpus. The approximate generated walker must not be relabeled or
   silently reused as the exact producer.
