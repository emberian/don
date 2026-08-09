# Lane report: `order-dispatch`

`crates/don-sim/src/systems/order_dispatch.rs` — `Unit::work` `0x0060D180`, a real
`Unit::do_job` `0x00617A10` dispatch, and `OrderList` maintenance.

Wave: assembly. Tier **C** throughout (behaviourally faithful, divergence unmeasured). No
differential test against retail has been run for this lane; every number below is either a
`[measured]` read of the instruction stream on this Mac or a local test count.

---

## 1. What now runs that did not before

**Orders now advance and retire.** Before this lane, `crates/don-sim/src/world.rs::unit_work`
read the head order's type and matched on it. That is the jump table, not the driver: nothing
walked the queue, nothing retired an order on a path failure, nothing noticed a stale target,
and `COVERAGE.md` §3 recorded `Unit::work` (2,885 B) as **uncited by any Rust file** — "even
the three implemented arms have no retail-derived thing to dispatch them."

Concretely, these execute now:

1. **`crate::systems::movement` has its first caller.** `order_dispatch::find_path` composes
   `PathFinder::find_upath_prepare` → `astar_path_unit` → `compress_path`, and `do_move`
   drives `movement::move_step` against the waypoint stack. A `MOVE_TO` is searched, walked
   waypoint by waypoint, and retired on arrival. The 40-unit / 625-frame soak test runs
   **25,000 `Unit::work` calls** through the real A\* and integrator and retires all 40
   orders.
2. **The `do_job` jump table dispatches all 28 arms.** 5 implemented (`NONE`, `MOVE_TO`,
   `FLEE_TO`, `ATTACK`, `GATHER`), 1 faithfully empty (`PATROL`), 22 recorded. A test drives
   every arm and asserts each is counted exactly once.
3. **`OrderList` maintenance with retail's cursor.** `reset` / `advance` / `at_tail` /
   `remove_current`, plus `update_order` `0x006179D0`, `UnitData::order_type` `0x00616E80`,
   `UnitData::get_action` `0x00608450`, `Unit::update_action` `0x0060A870`, `Unit::repath`
   `0x005E29B0`, `Unit::kill_current_order` `0x005E2CB0`, `Unit::clear_partial_path`
   `0x005E3920`.
4. **The failure/completion distinction runs.** `repath(); kill_current_order(0)` versus a
   bare `kill_current_order(0)` — see §3.
5. **The pathfinder's RNG obligation is serviced.** `movement::PathFinder::pending_retry_draw`
   previously had nobody to raise it to; `do_move` now converts it into a
   `WorkWorld::draw_path_retry_delay` call and counts it.

**Not run:** nothing in this module is reached from `World::step`. §6 says exactly what is
missing and why that gap is not this lane's to close.

## 2. Measured

| quantity | value | method |
|---|---:|---|
| tests added, all passing | **36** | `cargo test -p don-sim --lib order_dispatch` |
| `don-sim` lib tests | **946** passing, 0 failing | `cargo test -p don-sim --lib` |
| workspace | **1,249** passing, 0 failing, 0 FAILED suites | `cargo test --workspace` |
| clippy warnings attributable to this file | **0** | `cargo clippy -p don-sim --lib \| grep -c order_dispatch.rs` |
| distinct `0x00xxxxxx` literals in the file | 87 | regex, the ledger's own method |
| distinct retail procedures cited | **58** | resolved against `schema/rise-procs.tsv` |
| retail bytes cited (all 58) | **94,726 B** | sum of PDB `size` for distinct procs |
| retail bytes whose *structure* is ported | **21,414 B** | the 17 procedures listed below |
| `Unit::work` calls exercised in the soak test | 25,000 | test assertion |
| `do_job` arms dispatched at least once in tests | 28 / 28 | test assertion |

The 21,414 B figure is the honest one for a fidelity conversation. The 94,726 B figure is what
`tools/coverage-ledger.py` will pick up, and it is inflated by 41 procedures this module cites
**as boundaries it does not port** (`Unit::fight` 8,157 B, `Unit::do_trade` 4,519 B,
`Unit::do_cast` 4,191 B, `Unit::do_strafe` 3,676 B, and so on — the 22 unported arms are named
with their addresses in the `ARMS` table). The ledger's own §0 warns that "named" is an upper
bound on fidelity; this lane is a clean example of the gap, and the ledger number should not be
quoted for this file without the second column.

Structure ported (17 procedures, 21,414 B):

```
0x0060D180  2885  Unit::work                 0x005F7B30  4582  Unit::do_move
0x00617A10   500  Unit::do_job               0x005F1B80  1822  Unit::do_attack
0x006179D0    62  Unit::update_order         0x005EF2A0  3780  Unit::do_gather
0x00616E80    66  UnitData::order_type       0x005FB910  2623  Unit::find_path
0x00608450   260  UnitData::get_action       0x0046D620   190  LinkListBase<...>::remove_current
0x0060A870   485  Unit::update_action        0x0046D6E0    30  LinkListBase<...>::prev
0x005E29B0   499  Unit::repath               0x0046D700    28  LinkListBase<...>::tail
0x005E2CB0  1312  Unit::kill_current_order
0x005E22D0  1616  Unit::check_target_path
0x005E3920   674  Unit::clear_partial_path
```

"Structure ported" means the control flow and the branch conditions were read from the
instruction stream or the decompiled body and reproduced. It does not mean complete: `do_move`
omits transport legs and collision resolution, `do_gather` omits the deposit ledger,
`kill_current_order` omits four of its five per-type epilogues. Each omission is named in the
source at the point it happens.

## 3. Derived findings

Four things came out of the read that were not in any existing artifact.

### 3.1 Failure versus completion is a *call pair*, not a flag

`Unit::kill_current_order(int)` takes one argument, and a scan of all **120 direct call sites**
of it and `Unit::repath` in `.text` shows almost every one passing `0`. The argument suppresses
arrival bookkeeping; it does not mean "failed". The distinction the engine draws is the
sequence:

| retail sequence | meaning |
|---|---|
| `kill_current_order(0)` alone | the order **completed** |
| `repath(); kill_current_order(0)` | the order **failed** |

Because `Unit::repath` `0x005E29B0` — despite the name — does not recompute a path. It strips
every *leading* order whose type is in `{MOVE_TO, ATTACK_TO, EXPLORE_TO, FLEE_TO, CHANGE_FORM,
GROUP_MOVE, GROUP_ATTACK_TO}` and returns the moment the front order is not one of those. So
the pair means "throw away the walk **and** the thing it was walking to". It appears at
`0x0060D948` inside `Unit::work`, twice inside `Unit::check_target_path`, and at 14 more sites.

Conflating the two was a real defect in the first draft of this module and the test suite
caught it: a bare-kill path failure was routed through the pair, which ate a follow-on `GUARD`
that retail leaves alone.

### 3.2 An unreachable destination cancels a follow-on `ATTACK` or `BUILD_AT` — and only those

[measured, `0x005F8B5F`, inside `Unit::do_move`]

```
005f8b5f  call 0x5e2cb0     ; kill_current_order(0)   -- retire the failed move
005f8b66  call 0x616e80     ; UnitData::order_type()
005f8b6b  cmp  eax, 0xa     ; ATTACK
005f8b6e  je   0x5f82a3     ; -> kill_current_order(0) again
005f8b76  call 0x616e80
005f8b7b  cmp  eax, 6       ; BUILD_AT
005f8b7e  jne  0x5f82ac     ; -> return 1
005f8b84  jmp  0x5f82a3     ; -> kill_current_order(0) again
```

Both kills are **bare**. This is why "attack that thing across the water" silently cancels
rather than leaving a unit standing on the shore with a live attack order, and why a queued
`GUARD` or `GATHER` behind an unreachable move survives. Three tests pin all three cases.

### 3.3 `Unit::update_action` is what computes "where will this unit end up"

`Unit::update_action` `0x0060A870` walks the queue from the front, skipping every order that is
`is_move() && !is_group()` or is `CHANGE_FORM`, and returns the first survivor. While skipping,
it writes each skipped move order's destination into `UnitData::orders_x` (+112),
`orders_y` (+116) and `dest_angle` (+88). Those three fields therefore describe the position
the unit will be standing in when the *action* starts — not its current position, and not the
first waypoint. It is seeded from the unit's own position and `angle`, so a unit with no queued
movement reports where it is. `Unit::work` calls it three times per frame.

### 3.4 The `OrderList` is a circular list linked *backwards*

`UnitData::orderlist` at +200 is `OrderList` (28 B) wrapping
`LinkListBase<UnitOrder*, unsigned char, RecycledOrderNode>` (24 B) at +4:

```
+204 current_data   UnitOrder*            +216 length     int
+208 current_metric unsigned char         +220 head_node  RecycledOrderNode*
+212 current_node   RecycledOrderNode*    +224 ordered    int
```

Seven functions begin with the identical four-instruction inlined `reset()`:
`current_node = head_node->prev; current_data = current_node->data; current_metric =
current_node->metric`. And `UnitData::get_action` `0x00608450` iterates with
`current_node = current_node->prev`, terminating on `current_node == head_node`. So iteration
advances by `prev`, `head_node` is the **last** element in iteration order, and
`head_node->prev` is the **front** — the order being executed. An empty list is
`head_node == NULL`; `length` at +216 is the field `Unit::do_move` tests as `== 1` before its
"last order" notification. All offsets `[measured]` from the PDB LF_FIELDLIST records.

## 4. Corrections to existing artifacts

Two sentences each, per the standing rule.

* **`crates/don-sim/src/systems/movement.rs` — `find_upath_prepare` discards `straight_hit`.**
  The straight-line probe sets `straight_hit = true` and breaks when `valid_ucoord` accepts the
  current position, and the flag is then thrown away by `let _ = straight_hit;`, so on an open
  map the probe always breaks on its **first** iteration and every move falls through to the
  full A\* — the "avoid pathfinding entirely" optimisation the module's own doc comment
  describes never fires. Not fixed here because that file belongs to another lane.

* **`crates/don-sim/src/systems/movement.rs` — `Body::stuck_budget` is documented as
  `Unit+0x60`, which the PDB says is `UnitData::tolerance`.** `UnitData` +96 = `0x60` is
  `tolerance` (`int`), the same field `Unit::do_move` compares `vector_dist` against for
  arrival, so either the offset or the name is wrong and the two readings imply different
  arrival semantics. Worth one instruction-level check by the movement lane before either is
  relied on.

* **This wave's lane brief — "the pathfinder returns FAILURE on budget exhaustion for
  `quick == 0`, not a partial path".** `movement.rs`'s own header states the opposite (a soft
  budget hit *suspends*; a hard budget hit **with `quick != 0`** fails), and only `movement`'s
  version carries an instruction citation. `find_path` honours `movement`; whichever is right,
  the two artifacts currently disagree and one of them needs correcting.

* **`docs/mechanics/COVERAGE.md` §3 — `Unit::work` "is also uncited".** No longer true as of
  this file; §3's arm table should also move `GATHER` from absent to implemented-at-the-order-
  layer and note that `ATTACK`'s *target selection* is now the `assembly:target-selection`
  lane's, not this one's.

## 5. Two places where the port is a reconciliation, not a citation

Both are marked as such in the source. They are the honest edges.

1. **The path anchor.** After a successful search `movement`'s stack is `[anchor, w_n .. w_1]`,
   `emit_path` gives every waypoint `FLAG_WAYPOINT` with bit 0 clear, and `Unit::do_move`
   treats popping a record carrying `FLAG_MORE` (bit 0) as "this order is finished"
   [measured, `0x005F8827: test byte ptr [eax+0xc], 1`]. Only the caller's own record carries
   that bit, so the anchor must be the **order destination** — but `find_upath_prepare` reads
   its probe *start* from that same record. `find_path` writes the start before the call and
   rewrites the destination after. Without this the unit walks its waypoints and then walks
   back to where it began; the soak test found it.

2. **`check_target_path`'s "the target moved" test.** Retail's is a `find_angle` /
   `is_in_range` pair over the target's live position inside 1,616 bytes of object queries.
   The port reduces it to "the target moved further than `max(tolerance, 48)` from where the
   order recorded it" and is marked **UNVERIFIED**.

Two type-classification sets are also softer than they look. `MOVE_LIKE` is `[measured]` —
the same seven-way `cmp/je` chain appears in four independent places (`Unit::work`,
`Unit::repath`, `Unit::kill_current_order`, and `Unit::work`'s boat-collision gate) and they
agree exactly. `TARGETED` is **UNVERIFIED**: it is taken from which arms dereference a
`TargetOrder` in the decompiled bodies, because MSVC's identical-COMDAT folding collapses every
`UnitOrder::is_targeted` / `is_move` override onto the shared `mov eax,1; ret` stub at
`0x0047EF8E`, so the class hierarchy cannot be read out of the vtables.

## 6. What blocks `World::step`, precisely

`order_dispatch::adopt` / `publish` bridge `crate::order::OrderList` (what `World` stores per
row) to the executable `OrderQueue`, and a test runs a `World`-shaped `OrderList` through the
driver and gets it back one order shorter. So the order layer is one call away.

What is missing is the **world adapter**: `WorkWorld` extends `movement::UnitWorld`, which needs
`invalid_loc`, `unit_collides`, `tregion` and `needs_transport`. `World` implements none of
them — they are terrain and collision, i.e. `map_terrain.rs` and the unported
`Unit::detect_unit_collision` `0x00617060`. Until something can answer "may this unit stand on
this tile", the driver cannot be pointed at a real world, and that is a different lane's
foundation rather than a gap in this one.

The `WorkWorld` trait also needs three game answers this lane deliberately does not invent:
`target(who, o)` (object lookup), `attack(...)` (the `crate::mechanics::damage` pipeline, which
`world.rs` already wires) and `gather(...)` (`economy.rs`'s rates). All three are one impl
block away for a host that has those tables.

## 7. Known behaviour that is a hazard, not a bug

`movement::move_step` has **no anti-orbit guard**. A unit whose turning circle
`speed / (2 pi * turn_rate / 2^32)` is wider than the distance to its next waypoint circles it
forever: it never satisfies the `dist <= speed` snap, never pops the record, and never
finishes. With the unit A\*'s 48-unit cell that means `turn_rate` must exceed roughly
`2^32 * speed / (2 pi * 48)`. The first version of the soak test used `0x08000000`
(11.25 deg/frame) at speed 32 — a 163-unit circle — and 30 of 40 units orbited for all 625
frames. `UnitWork::at` now defaults to `0x20000000` (a 41-unit circle at speed 32) and a test
pins both sides of the threshold.

`UnitWork::turn_rate` is a **host-supplied placeholder**: retail reads it from `UnitType`
through `0x005DE340`, which nobody has ported. Whatever supplies it must clear that floor or
units will silently never arrive.

## 8. Files

* **written:** `crates/don-sim/src/systems/order_dispatch.rs` (new, ~1,900 lines with 36 tests)
* **written:** `docs/assembly/order-dispatch.md` (this file)
* **shared file, one-block edit:** `crates/don-sim/src/systems/mod.rs` — added `pub mod
  order_dispatch;` with the three-line doc comment the file's header asks each lane to write.
  Nothing else in that file was touched.
* **not touched:** `crates/don-sim/src/order.rs`, `crates/don-sim/src/world.rs`,
  `crates/don-sim/src/systems/movement.rs`, `crates/don-sim/src/lib.rs`.

`crate::order::EXECUTORS` remains the single source of truth for the jump-table addresses; this
module owns only the per-arm implementation status, and a test asserts the two tables stay
index-aligned so a sibling lane editing either one breaks here rather than silently.
