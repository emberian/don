# Movement — the real unit pathfinder and integrator

Lane: `mech:movement`. Module: `crates/don-sim/src/systems/movement.rs` (1,880 lines, 26 tests).
Checksum channel served: **`units`**.

Everything is marked **[measured]** (verified here against `ron-bin/riseofnations.exe` and
`ron-bin/sbl/rise.pdb`) or **UNVERIFIED** (structure taken from Ghidra output, not checked against
behaviour). No differential test against retail has been run for this lane, so every fidelity
claim is **tier C**. Nothing here is "verified" in the proof-assistant sense.

---

## What now works

| thing | state | evidence |
|---|---|---|
| `find_angle` `0x0092D130` | ported exact | instruction-for-instruction; 4 cardinal directions asserted, 6 arbitrary vectors round-trip through the integrator to within 3 world units |
| `sin_table` `0x00A46A00` + its 256-entry table | ported exact | table regenerated from the retail initialiser's own arithmetic; 1024-sample sweep of all four quadrants, worst error 0.0032 of full scale |
| `sinx` / `cosx` quadrant fold | ported exact | the fold is inlined at `0x005FB5CC` and `0x00683194`; without it two quadrants are 30% wrong |
| `vector_dist` `0x0046CFF0` | ported exact | all three arms including the unsigned-shift overflow guard |
| `PathFinderData::get_estimate` `0x00688310` | ported exact | both the `0x30` and the `/step` arms |
| `PathFinder::calc_cost` `0x00684E50`, **unit domain** | ported | 32 straight / 40 diagonal asserted; transport surcharges ported, not exercised |
| `PathFinder::valid_ucoord` `0x00687C80` | ported, incl. the memo | bounds → memo → `invalid_loc` → collision, in that order |
| open list = **unbalanced BST** `Tree<PathNode*,int>::ordered_insert` `0x004796F0` | ported exact | all 126 bytes read; LIFO tie-break asserted |
| `PathFinder::first_open_node` `0x00687970` | ported | leftmost descent + dual-container removal |
| `PathFinder::astar_path` `0x00683770`, **unit domain, fresh search** | ported | routes around walls, fails when sealed, deterministic across repeats, tolerance shortens the path, `quick` halves the branching factor, budget arm asserted both ways |
| `PathFinder::find_upath` `0x00682F30` pre-pass | ported | the straight-line probe, the cell-centre snapping, the three-record frame |
| waypoint compression | ported | collinear-and-equidistant drop, transport-exempt |
| `Unit::move_step` `0x005FAF30` | ported, integrator only | walks a 500-unit leg at speed 40 in under 30 ticks, refuses off-map, reports blocked bodies |
| `Stack<PathData>` + `adler32` for the `units` channel | ported | adler asserted against the zlib definition and against order sensitivity |

`cargo test -p don-sim systems::movement` → **26 passed, 0 failed**. The full crate is
`478 passed, 4 failed`; all four failures are in `crate::trig`, a sibling lane's file, and are
expectation bugs there (see §7).

---

## 1. The correction that matters most: there is no floating point here

`docs/derivation/architecture.md` §6.2 carries this warning:

> ⚠ **`Unit::move_step` and `Unit::find_path` contain floating point** — `sin_table`
> (`0x00A46A00`), `cosx` (`0x0092D0C0`), `find_angle` (`0x0092D130`). The "no float anywhere in
> the movement cone" conclusion does not survive into the integrator, only into the search.

**That is wrong, and the names are what caused it.** Disassembling every function in the cone and
counting SSE and x87 opcodes [measured]:

| function | VA | insns | FP insns |
|---|---|---:|---:|
| `Unit::move_step` | `0x005FAF30` | 775 | **0** |
| `Unit::do_move` | `0x005F7B30` | 1430 | **0** |
| `Unit::find_path` | `0x005FB910` | 840 | **0** |
| `Unit::set_new_location` | `0x005F8D20` | 567 | **0** |
| `Unit::detect_unit_collision` | `0x00617060` | 654 | **0** |
| `Unit::resolve_unit_collision` | `0x005F9D30` | 919 | **0** |
| `PathFinder::find_upath` | `0x00682F30` | 601 | **0** |
| `PathFinder::astar_path` | `0x00683770` | 1645 | **0** |
| `PathFinder::calc_cost` | `0x00684E50` | 860 | **0** |
| `UnitData::invalid_loc` | `0x00607C30` | 336 | **0** |
| `find_angle` + `sin_table` + `cosx` + `vector_dist` | | 302 | **0** |
| `Guy::process` | `0x005E0230` | 174 | **0** |
| `Guy::move` | `0x005D9240` | 938 | 4 |

`Guy::move`'s four are `movss` of one `xmm0` into four adjacent floats at `[ebp-0x28..-0x1C]` —
a stack-local float quad being initialised, i.e. struct moves, exactly the category
`README-LLM.md` already flags as noise in the `PathFinder` class.

`find_angle` is an integer polynomial arctangent. `sin_table` is a 256-entry table with integer
linear interpolation. `cosx` is `sin_table` behind an integer quadrant fold. The **only** float
anywhere in the lane is `trig_init` `0x00A46980`, which fills the table once at startup:

```asm
00a469a5  cvtdq2pd xmm0, xmm1               ; {i, i+1}
00a469a9  mulpd    xmm0, [0xb69bb0]         ; * 1.570796327   (pi/2)
00a469b1  divpd    xmm0, [0xb69bd0]         ; / 255.0         <-- 255, not 256
00a469b9  call     0xa55fc0                 ; sin (double)
00a469be  mulpd    xmm0, [0xb69be0]         ; * 65535.0
00a469c6  cvttpd2dq xmm0, xmm0              ; truncate toward zero
00a469ca  movq     [esi*4 + 0xe32f40], xmm0
```

So `T[i] = (int)(sin(i * (pi/2) / 255) * 65535)`, i in 0..256, and `T[255] == 65535` exactly.
The module carries that as a `const [i32; 256]`, which makes it **integer-only at runtime**.

**Consequence for the port**: unit movement is bit-reproducible without any IEEE reasoning at
all. The only residual float risk is one ULP in the startup table, and it is a 256-value
one-time constant that can be pinned by a single live-memory read of `0x00E32F40` (not yet done —
see §8).

Two sub-facts that fall out and are worth carrying:

- The 255 divisor against a 256-slot table means the quarter turn is sampled in 255 steps but
  indexed over 256 units. A ~0.4% scale error baked into the shipped engine.
- The `(u8)(i+1)` neighbour index wraps `T[256] -> T[0] == 0` at `i == 255`, and the resulting
  `imul` overflows 32 bits. It comes out **right**: the wrapped product is `+4259839`, `>> 22`
  is `1`, `unit` lands on exactly `65536`, and the trailing `>> 16` absorbs it. `wrapping_mul` is
  mandatory in Rust (debug would panic where retail wraps) but there is no visible glitch.
  `mech:sim-core` reached the same conclusion independently.

---

## 2. The real hazard in `sin_table` is amplitude, not angle

The low regime is `amp < 0xFFFF` on a **signed** compare, and it computes `unit * amp` in 32 bits
with `unit` reaching 65536. That overflows for `|amp| >= 32768`. The middle regime exists to
pre-shift larger amplitudes but only engages at `0xFFFF`, so `[32768, 65534]` is broken **in
retail**. Every real caller is far below it — `Unit::move_step` passes the unit's speed (tens of
world units per tick), `PathFinder::find_upath` passes `24` — so the gap is unreachable in play.

This is called out because a test written at amplitude 65535 does not measure a sine, and both
this lane and `mech:sim-core` wrote one before noticing.

---

## 3. The heuristic is inflated ~15x: the unit search is greedy, not optimal

`get_estimate` `0x00688310`, verbatim [measured]:

```
d = vector_dist(|ax - bx|, |ay - by|)
step == 0x30  ->  h = d * 10
otherwise     ->  h = d * 60 / step
```

`calc_cost` for the unit domain returns `(0x100 * 32) >> 8` = **32** for a straight step and
**40** for a diagonal (`(dir & 1) * 8`). A straight step of 48 world units therefore costs `32`
of `g` and moves `480` of `h`.

**The unit A\* is heuristic-dominated by a factor of ~15.** It is closer to greedy best-first than
to A\*, its paths are not cost-optimal, and its node counts are correspondingly tiny. This is the
engine's behaviour and must be reproduced, not fixed. Measured on the port: a 296-cell straight
run over open ground expands **2,360 nodes** — 8 per cell, i.e. it walks the straight line and
expands each node's full neighbourhood once, with essentially no lateral search.

`vector_dist` itself is the octagonal approximation `M + m^2 / (2M)`, so a perfect diagonal comes
out at `1.5 * M` against a true `1.4142 * M` — ~6% long, again the engine's own metric.

---

## 4. The unit cost function charges pure geometry

This is the structural surprise of the lane. `calc_cost`'s unit arm is:

```c
if (param_6 == 0x30) { extra = 0; base = 0x100; }   // and that is the whole terrain model
```

Terrain, danger maps, borders, building avoidance, region-transition penalties, the
`FUN_006EBAA0` diplomacy check and the `+5000` ally-territory surcharges all live in the `0xC0`
(tile) and `0x300` (water) arms. **The quarter-tile unit search charges geometry only**, plus a
transport surcharge:

| condition | surcharge |
|---|---|
| `needs_transport >= 1` and unit can board and `depth >= 2`, `end_class == 2` | `INT_MAX` (edge rejected) |
| same, `needs_transport != 1` | `(500 or 2000) << 2` = 2000 or 8000 |
| `depth == 1`, re-query against the search origin tile, `needs_transport != 1` | 250 or 1000 (**not** shifted) |
| always | `(dir & 1) * 8` |

`depth` is `PathNode::timeout`, which despite the name is `parent.timeout + 1` — the path depth,
not a timer.

The architecture: `Unit::do_move` runs a **coarse** `find_tpath`/`find_wpath` leg for terrain
preference and a **fine** `find_upath` leg for local obstacle avoidance. Anyone porting terrain
cost into the unit search will produce paths the engine never produces.

`calc_cost` makes **zero** RNG calls, confirming the standing correction. Per-edge RNG jitter is
`calc_road_cost` `0x00686300` and `calc_river_cost` `0x00687930` only.

---

## 5. The open list is an unbalanced BST, and that is load-bearing

`Tree<PathNode*,int>::ordered_insert` `0x004796F0`, all 126 bytes [measured]:

```asm
00479737  cmp edi, [ecx+0x10]      ; new key vs incumbent
0047973a  jle 0x47974b             ; <=  -> go LEFT
```

No rotation, no rebalancing, no colour bits. `first_open_node` `0x00687970` descends
`while (node->left) node = node->left` and takes the leftmost.

Two consequences:

1. **Ties pop last-in-first-out.** An equal `f` goes *left* of the incumbent, and leftmost
   extraction therefore returns the newest. A binary heap does not reproduce that, and the
   difference is a divergence in expansion order, hence in the number of failed searches, hence
   in RNG stream position. The module uses a faithful arena BST and asserts the LIFO order.
2. **It degenerates.** With a heuristic-dominated search the `f` keys arrive nearly monotonically,
   so the tree becomes a left spine and insert/extract go O(n). Measured on the port: 400-node
   searches at ~366 µs each in a release build — ~0.9 µs per node, which is the spine walk, not
   the arithmetic. **This is faithful**, and it explains why the engine caps at 32,000 nodes and
   suspends searches across frames rather than making the search better.

The other three containers (`openlistrefs`, `closedlist`, `validlist`) are red-black trees keyed
by a unique cell metric and only ever `seek`ed, so a map is behaviourally equivalent; the module
uses `BTreeMap` for determinism.

`PathNode` layout, confirmed against every access site:
`x@0 y@4 length(g)@8 estimate(h)@12 value(f)@16 timeout(depth)@20 metric@24 z_val@28
transport@30 building@31 parent@32`, 36 bytes.

The **metric** is a linear cell index maintained incrementally:
`child.metric = parent.metric + move_x[dir] + move_y[dir] * row_stride`, with
`row_stride = World[0] * 16` (map width in quarter-tiles) for the unit grid. It keys the closed
list, the open-list index, *and* the `valid_ucoord` memo — so two world positions in the same
48-unit cell share a passability answer for the whole search.

---

## 6. Ordering and control flow that a reimplementation has to get right

### 6.1 The stack protocol

`astar_path` takes its endpoints off the caller's `Stack<PathData>`:

```
[ .., anchor, start, goal ]      goal popped first, start second
arrival tolerance = anchor.tolerance          (`piVar7[-2]`, the record BELOW start)
goal test         = |x - gx| + |y - gy| <= anchor.tolerance / 2 + unit_size * 48
```

`find_upath` builds that frame with **both endpoints snapped to unit-cell centres**
(`cell * 48 + 24`) and leaves the caller's original record underneath as the anchor. On success
the waypoints are pushed back goal-end-first, so the top of the stack is the waypoint nearest the
unit — which is what `move_step` pops.

Unit waypoints always get `tolerance = 0` and `flags = 2` (plus `4` for a transport step, `0x10`
for a building step). Both arms of the `local_24` test land on `param_3 = 0` for `step == 0x30`.

### 6.2 The direction seed is fixed for the whole search

```
dx = sx - gx;  dy = sy - gy
seed = (|dy| < |dx|) ? ((gx < sx) ? 7 : 3)
                     : ((sy <= gy) ? 5 : 1)
```

Neighbours are then visited as `seed+1 .. seed+8` wrapped into `[1,8]`, with stride
`(quick != 0) + 1`. The seed is computed **once**, from the start-to-goal vector, and is not
recomputed per node. All four seeds are diagonals. `quick != 0` therefore visits only 4 of the 8
directions — alternating parity, so a quick search is either all-diagonal or all-orthogonal.

### 6.3 Budget exhaustion is a failure, not a partial path

[measured `0x0068487C`] `if (expanded < 32000 || quick != 0) -> emit`. Falling through on the
unit arm means the hard budget is spent **and** `quick == 0`, and the engine then draws the retry
delay and **returns 0**. A quick search that runs out of budget *does* return its best node.

The first port of this lane had it backwards and produced a spurious path where retail gives up;
the module now asserts both directions.

### 6.4 The RNG obligation

Unit pathfinding's *entire* RNG consumption is `Random::get(0, 0xFFFF) % 3 + 6` — a 6-to-8-tick
retry delay — drawn from `GameAccess::game_random` (the main simulation stream) in the two failure
epilogues at `0x006848C4` and `0x00684E02`, plus `unit[0xB2] += 30`. The module raises
`PathFinder::pending_retry_draw` rather than inventing a stream. **Skipping the draw does not
corrupt path state; it shifts the shared stream position for every later draw in the tick**, which
is a whole-sim desync.

### 6.5 `find_upath` tries very hard not to search at all

Before any A\*, for a small non-transport unit, `find_upath` walks a straight line from the unit
toward the destination in **24-world-unit** `sin`/`cos` steps and gives up the moment it lands in
the destination cell, or the destination's tile region matches and the current cell is passable,
or it is within 24 units on both axes. Short unobstructed moves never reach the pathfinder.
Adjacent cells (`|dqx-qx| + |dqy-qy| < 2`) also short-circuit.

### 6.6 Where movement sits in the tick

`Objects::process_all` -> vtable `+0x9C` `Unit::process` -> vtable `+0x188` `Unit::work` ->
`Unit::do_job` (28-entry jump table) -> `Unit::do_move` `0x005F7B30`, which is the single funnel
for **all** locomotion (`MOVE_TO`, `FLEE_TO`, `EXPLORE_TO`, `ATTACK_TO`, `GUARD`, `FOLLOW`,
`GROUP_MOVE` all reach it). `Unit::do_move` and `Unit::resolve_unit_collision` are the **only two
callers of `find_upath` in the whole binary**.

Owner-slot order rotates as `(frame + i) % 10` every frame, so the order in which units path — and
therefore the order in which they consume the shared RNG on failure — rotates too.

---

## 7. Cross-lane notes

- **`crate::trig` and `crate::systems::groups_guys` also carry this trigonometry.** The three
  copies are not equivalent: `crate::trig::sin_table` is the *raw* entry point and is wrong in two
  of four quadrants unless the caller folds first. Measured drift off the ray: raw 0.3041, folded
  0.0105 (`mech:groups-guys`). This module's copy is folded and agrees with `groups_guys` to the
  bit. **When merging, keep one folded pair and delete the rest; do not expose a raw `sin_table`
  as public API.** Flagged in the module header too.
- The 4 failing `crate::trig` tests are expectation bugs in that lane's file, not implementation
  bugs — in particular `the_index_255_wrap_is_reproduced` asserts a glitch that does not happen
  (§1). Not touched from here; that file belongs to `mech:sim-core`.
- `Object` coordinates are XOR-obfuscated with `0x00063637`; `GuyData`'s are not. The constant is
  `COORD_XOR` in this module, next to the code that consumes those fields. Nothing here stores
  obfuscated values.
- Grid ladder, agreed across lanes: unit cell 48 = 1/4 tile, tile 192, fog cell 384 = 2 tiles,
  water cell 768 = 4 tiles, region 1536 = 8 tiles.
- `mech:objects` measures target acquisition, not movement, as the quadratic cost in a dense world
  (30x throughput loss from 128 to 4096 units). This module makes no neighbour queries of its own;
  `UnitWorld::unit_collides` is the one hook that will hit that structure, and it is called from
  `valid_ucoord` — i.e. **once per distinct cell per search**, memoised — which is the cheap
  pattern, not the quadratic one.

---

## 8. Checksum channel: `units`

`CheckSums::check_units` `0x009371D0` iterates leader slots `0..7` in fixed order, and within each
slot iterates objects from `*[0x00C06198]` upward, calling vtable slot 31 (`+0x7C`) =
`Unit::walk_data` `0x0060CF40` on each active one, accumulating adler-32 **initialised to 1**.

`Unit::walk_data` includes `Stack<PathData>` at `UnitData+0xB8` behind mask bit 2 (the three
optional sections are the path stack, the order list, and the garrison `PtrArray<Guy>`). **So the
path buffer this lane produces is checksummed state, not scratch** — a wrong waypoint is a desync,
not a cosmetic difference.

`PathStack::walk_bytes` emits records in stack order as 4 little-endian `i32` each;
`adler32(1, bytes)` is the channel accumulator. The module asserts the adler primitive against the
zlib definition and asserts that record order changes the result.

Separately, `CheckSums::check_pathfinder` `0x00936E30` is **not** one of the fifteen `check_all`
channels but is not empty either: it walks a flat 108-byte window at `pathfinder + 0x58`, i.e. the
scalar search state (`origin_tile`, `in_upath`, `start_land`, `end_class`, `valid_hits`, the
budget and suspend fields), not the containers.

---

## 9. Honest gaps

Ordered by how much they matter.

1. **`Unit::detect_unit_collision` `0x00617060` (2,410 B) and `Unit::resolve_unit_collision`
   `0x005F9D30` (2,943 B) are not ported.** They are behind `UnitWorld::unit_collides` and a
   `MoveStep::Blocked` return. What *is* established: both are pure integer; collision is part of
   **A\* validity** (`valid_ucoord` calls `detect_unit_collision`), not just of the integrator, so
   a standing body makes a cell impassable for the search and the answer is memoised per cell;
   and `resolve_unit_collision` implements pushing as a **local detour `find_upath`**, not as an
   impulse — which is why a blocked unit consumes pathfinder budget. This is the largest single
   remaining piece of the lane.
2. **`Unit::do_move` `0x005F7B30` (4,582 B) is not ported.** The module has its two children
   (`find_upath`, `move_step`) but not the driver that sequences the coarse tile/water legs, the
   `find_upath_restore` resume, the repath conditions, and its own RNG site at `0x005F89AF`.
3. **Suspend/resume is not implemented.** `astar_path` can park its five containers on the unit at
   `Unit+0x104..+0x114` and return -1 for `find_upath_restore` `0x00688F40` to resume next frame.
   The module detects the condition and returns `SearchResult::Suspended` but does not park state.
   Until this exists, `soft_node_limit` should be left at `i32::MAX`.
4. **`Tree::remove_current` `0x00479770` (422 B) was not read.** The module uses in-order-successor
   (Hibbard) deletion. Leftmost removal — the hot path, once per expansion — has at most one child
   and is unambiguous; this only bites on the "found a cheaper route to an open node" branch, where
   a different splice changes tree shape and therefore later tie order. Marked UNVERIFIED in code.
5. **The turn-rate arm of `move_step` is not modelled.** `0x005DE340` reads `Unit+0xA1`, `+0x8C`,
   `+0xA2` and the unit-type tables; the module takes the rate as a parameter. The thresholds are
   measured (`0x02222220` = 4.8 deg ignore, `0x20000000` = 45 deg, `0x38E38E3A` = 80 deg,
   `0x40000000` = 90 deg) but which of the desired/limited angle feeds the step is UNVERIFIED —
   the module uses the post-turn angle.
6. **Formation movement is not in this lane.** `Groups::compute_form` / `update_positions`
   (`groups.cpp`) run inside `GameDaemon::process_all`, *before* `Objects::process_all`, and set
   the per-unit targets that `do_move` then chases. `mech:groups-guys` owns it.
7. **The `0xC0` (tile) and `0x300` (water) domains of `astar_path` and `calc_cost` are not ported.**
   Their structure is read and documented above but only the unit arm is implemented. These are
   needed for `do_move`'s coarse legs and for caravans/ships.
8. **`UnitData::invalid_loc` `0x00607C30` (1,034 B) is a trait hook, not a port.** Its argument
   list at the `valid_ucoord` call site is `(tile_x, tile_y, 0, 1, 0, 1, 0, y>>6)` — note the last
   argument is a raw `y >> 6`, not a tile index, which is either an engine quirk or a misread.
9. **Nothing here has been differentially tested against retail.** The oracle can call
   `find_angle`, `sin_table`, `vector_dist` and `get_estimate` directly — they are leaf `__fastcall`
   / `__cdecl` integer functions with no global state — and that would move four of them from tier
   C to tier B in one afternoon. That is the highest-value next step for this lane.
10. **`SIN_QUARTER` is regenerated, not captured.** It reproduces the retail initialiser's
    arithmetic in Rust `f64`, which should be bit-identical to `_libm_sse2_sin_precise` but has not
    been checked. One `ReadProcessMemory` of 1,024 bytes at `0x00E32F40` in the live game settles it.

---

## 10. Provenance index

| symbol | VA | source | status |
|---|---|---|---|
| `PathFinder::astar_path` | `0x00683770` | pathfinder.cpp:2329 | unit domain ported |
| `PathFinder::calc_cost` | `0x00684E50` | pathfinder.cpp:1888 | unit domain ported |
| `PathFinder::find_upath` (inner) | `0x00682F30` | pathfinder.cpp:3684 | pre-pass + compression ported |
| `PathFinder::find_upath` (thin) | `0x00688EB0` | pathfinder.cpp:3922 | trivial forwarder |
| `PathFinder::find_upath_restore` | `0x00688F40` | pathfinder.cpp:3938 | not ported |
| `PathFinder::valid_ucoord` | `0x00687C80` | | ported |
| `PathFinder::first_open_node` | `0x00687970` | | ported |
| `PathFinder::add_to_openlist` | `0x00687AA0` | | ported (inlined) |
| `PathFinderData::find_node_open` | `0x006882B0` | | ported |
| `PathFinderData::find_node_closed` | `0x00688270` | | ported |
| `PathFinderData::get_estimate` | `0x00688310` | | ported exact |
| `Tree<PathNode*,int>::ordered_insert` | `0x004796F0` | | ported exact |
| `Tree<PathNode*,int>::remove_current` | `0x00479770` | | UNVERIFIED |
| `vector_dist` | `0x0046CFF0` | | ported exact |
| `find_angle` | `0x0092D130` | gamemath.cpp:36 | ported exact |
| `sin_table` | `0x00A46A00` | trig.cpp:30 | ported exact |
| `cosx` | `0x0092D0C0` | | ported exact |
| `trig_init` | `0x00A46980` | trig.cpp | table baked as a const |
| `Unit::move_step` | `0x005FAF30` | unit.cpp:13628 | integrator ported |
| `Unit::do_move` | `0x005F7B30` | unit.cpp:15757 | not ported |
| `Unit::find_path` | `0x005FB910` | unit.cpp:13276 | not ported |
| `Unit::set_new_location` | `0x005F8D20` | unit.cpp:15429 | not ported |
| `Unit::detect_unit_collision` | `0x00617060` | unit.cpp:14024 | trait hook |
| `Unit::resolve_unit_collision` | `0x005F9D30` | unit.cpp:14765 | not ported |
| `UnitData::invalid_loc` | `0x00607C30` | | trait hook |
| `UnitData::needs_transport` | `0x00609920` | | trait hook |
| `WorldData::get_tregion` | `0x006B52E0` | | trait hook |
| `CheckSums::check_units` | `0x009371D0` | checksums.cpp:502 | channel documented, walker partial |
| `adler32` | `0x00A46830` | | ported (tier B elsewhere) |
| `PathFinder::find_wpath_army` | `0x00683730` | | **dead code, zero callers — do not implement** |

Constants: `SIN_QUARTER` from `0x00E32F40` / `trig_init`; `MOVE_X` `0x00ADCAF0`;
`MOVE_Y` `0x00ADC400`; coordinate table `[0x00CAE5FC]` built by `0x00681DB0`;
`UNIT_NODE_BUDGET` = `500 * 0x40` = 32,000; probe step `0x18`; unit cell centre `cell*0x30 + 0x18`.
