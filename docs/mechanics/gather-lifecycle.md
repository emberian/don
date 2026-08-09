# Ordinary on-map gather lifecycle

`crates/don-sim/src/systems/gather_lifecycle.rs` joins the retail gather-chain, collision,
and order boundaries for ordinary Farm, Woodcutter/Camp, and Mine Citizens. It does not
model a worker as a building seat. The covered routines are:

| Routine | VA | Covered boundary |
|---|---:|---|
| `GatherOrder::GatherOrder` | `0x00487390` | checksum-visible initial suffix |
| `Unit::do_gather` | `0x005EF2A0` | membership retry, Farm activation/move gate |
| `Unit::do_non_flat_gather` | `0x005F0170` | Camp/Mine approach and arrival activation |
| `Unit::add_gather_order` | `0x0061A5C0` | attach-before-order-link ordering |
| `Build::add_gatherer` | `0x0062F640` | owner-local gather-chain head insertion |
| `UnitType::find_nearby_spot` | `0x0061DE70` | deterministic collision-valid destination |
| `Unit::add_move_order` | `0x00616ED0` | queued movement, never coordinate mutation |

## Persistent identity and attachment

Ordinary worker TypeIndexes are `0x32` and `0x33`. Farm, Camp, and Mine properties are
respectively `0x1A1`, `0x1A2`, and `0x1A3`. A new `GatherOrder` starts with
`tx=ty=build_type=-1`, `wait=0`, `goto_build=1`, and zero phase bytes. After a successful
immediate attachment, `add_gather_order` stores the target property, keeps Farm flat, and
marks Camp/Mine non-flat with `dist_mod=4/10`.

Attachment is only an owner-local intrusive link:

```text
worker.gather_down = site.gather_down
site.gather_down   = worker.o
```

It does not write worker coordinates, `inside_up/down`, `on_map`, the WData object chain,
guy positions, or collision bits. `ensure_ordinary_attachment` first proves the worker has
a live on-map collision row. A worker marked seated/off-map is rejected as a model defect.
An already-linked worker is checked before capacity, matching `do_gather`; a site that is
exactly full does not reject one of its existing members.

A new worker refused by the signed-byte capacity is retired through the existing Gather
epilogue and reported as `RetiredAtCapacity`; the order host must then install retail's
follow-on Think order. The worker remains on-map throughout the refusal and retirement.

The current gathering adapter requires the staged `GatherAssignment` to carry the exact
target owner, object, and UID. Retail's low-level `Build::add_gatherer` runs before the new
order is linked and does not itself inspect UID, so an integration must stage assignment and
attachment as one issue transaction. It must not expose a half-issued order to the tick.

## Camp and Mine approach

The admitted phase is `goto_build=1`, `non_flat_gather=1`, `wait>=0`. Adjacency is the
virtual `Unit::is_at` / `Object::adjacent_to` result. The host must implement
`AuthoritativeOrdinaryGatherGeometry`; a centre-distance tolerance is not accepted.

When the worker is not adjacent, the normal finder tuple is:

```text
centre = building centre
min    = min(x_size,y_size)*0x60 + 0x30
max    = -1
step   = 0
angle  = find_angle(building - worker)
filter = 3
actor  = (worker_o, owner)
tail   = (0,0,-1,0,-1)
```

The concrete finder requires both the synchronized collision bitmap and the separate
ordered-object collision query. On success the lifecycle emits
`add_move_order(x,y,1,0,0,0,-1,-1)`. The normal movement/order systems must enqueue that
MOVE_TO ahead of the still-existing Gather order. The lifecycle does not move the anchor.

If worker and target terrain regions differ and exact `UnitData::can_transport` is true,
retail queues the building centre directly. `ordinary_can_transport` evaluates the three
source fields; there is no generic “reachable” boolean. If the normal finder exhausts while
an inactive worker is farther than `0x600`, retail retries with `min=0x600`. Exhausting that
retry retires the Gather order and detaches the gather chain. A nearer exhaustion stores
`wait=-1` and leaves the order attached for the tile-preparation phase.

On an authoritative adjacent tick, retail decrements `wait` and sets `been_there=1`. That
single byte is the payout boundary used by `num_gatherers(Active)`. No arrival transition
changes physical placement. Every admitted non-flat tick also applies the existing exact
entry primitive: `worker.group=-1`, and the first tick that observes an active worker latches
site mask `0x800` while incrementing `recharging` once.

## Farm activation and movement

Farm does not use the Camp/Mine building finder. Once membership/capacity gates pass, its
first admitted `do_gather` branch sets `been_there=1` before the optional footprint move.
For `FarmData::update == 1`, action `0x23` is selected and the deterministic gate is:

```text
(global_value + worker_o*7 + owner) & 0xff
```

Only zero queues a move. It draws `rnd(x_size/2)` and then `rnd(y_size/2)`; either draw is
skipped when its range is at most one. A nonzero gate consumes no RNG. The move destination
is inside the Farm footprint and is returned as the same exact MoveOrder plan.

The alternate `FarmData::update != 1` branch reads the retail Farm cell-state table and
current animation before choosing an action or the full-footprint relocation. That table
host is not yet recovered here. `farm_first_gather_tick` rejects the alternate return value;
it does not pretend it took the admitted branch. `gathering::farm_gather_relocation` remains
an exact arithmetic/RNG primitive, but it must not be invoked without the missing table gate.

## Integration contract and remaining boundary

Arena or another runtime consumer must retain, atomically:

- `GatherSite`, `GatherWorker`, and the `NonFlatGatherState` suffix;
- a real on-map `CollUnits` row and guy/collision stamp;
- complete worker TypeData used by `NearbyUnitType`;
- synchronized `World`, bitmap collision, and ordered object collision;
- an authoritative adjacency host;
- a normal order-list adapter that queues the returned MoveOrder ahead of Gather.

Missing any mandatory input returns an error. A legitimate finder exhaustion is a distinct
modeled outcome. Tests snapshot unit rows, guy rows, and collision-bit payloads across attach,
search, activation, and error paths; they also cover full/already-attached ordering, both
finder exhaustions, retirement, Farm RNG/no-RNG gates, and rejection of a seated ordinary
worker.

Still outside this module are the Farm cell-state/animation branch, the complete atomic
command issue/link operation, animation/doober execution, and the later Camp/Mine terrain
tile loop (already exposed as exact primitives by `systems::gathering`). These are explicit
host or recovery boundaries, not simplified fidelity behavior. The 128-frame Camp/Mine
over-cap audit gate in `do_gather` also remains outside this initial approach entry point.
