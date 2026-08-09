# CAST_SPELL executor frontier

Status: **source-only proof pack; strict order row 14 remains red**. The isolated planner in
`crates/don-sim/src/systems/cast_order_frontier.rs` freezes the complete concrete payload,
state-writing control-flow cones, typed host observations, and an atomic preflight contract for
`Unit::do_cast`. It is intentionally absent from `systems/mod.rs` and does not change the live
dispatcher, tick, save format, or closure ledger.

## Binary and PDB authority

The authority is shipped `ron-bin/riseofnations.exe` plus matching
`ron-bin/sbl/rise.pdb`, checked against `schema/rise-procs.tsv`, `schema/rise-symbols.tsv`,
`schema/types.json`, direct PE32 disassembly, and `re/decomp-all/005ebfe0.c`.
The inspected SHA-256 identities are `30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`
for the executable and `334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`
for the PDB.

| symbol | VA | bytes | role |
|---|---:|---:|---|
| `CastOrder::walk_data` | `0x004860A0` | 70 | checksum/save ranges |
| `Unit::add_cast_order` | `0x005E4A60` | 541 | payload construction and pack/deploy canonicalization |
| `Unit::do_cast` | `0x005EBFE0` | 4,191 | one CAST_SPELL activation |
| `SpellTypeData::get_job_time` | `0x00675800` | 959 | frame duration |
| `SpellTypeData::is_valid_target` | `0x006763C0` | 1,726 | target legality |
| `SpellTypeData::get_range` | `0x00676A80` | 435 | approach threshold |
| `SpellType::pay_cast_costs` | `0x00676C40` | 152 | paid-latch transaction |
| `SpellType::cast` | `0x00676CE0` | 776 | terminal spell mutation |

The function's last instruction ends at `0x005ED03E`; `0x005ED03F` is alignment and
`Unit::do_await_board` begins at `0x005ED040`. `CAST_REACHABLE_CFG` records every semantic
state-writing cone from entry through the non-spell building-transfer tail.

## Concrete payload and checksum range

PDB type `CastOrder` is 48 bytes: `TargetOrder@+0`, four concrete dwords, and virtual
`UnitOrder@+0x28`. The fields are:

| offset | field |
|---:|---|
| `+0x08/+0x0C/+0x10` | target object, owner, UID |
| `+0x14/+0x18` | order point `x/y` |
| `+0x1C` | paid-cost latch |
| `+0x20` | spell/type index |
| `+0x2C` | virtual-base flags byte |

`CastOrder::walk_data` walks the virtual flag byte, the ten-byte target identity range, and
the sixteen-byte `x..spell` range: 27 checksum-visible bytes. Padding, vptrs, and vbptr are not
walked. `Unit::add_cast_order` initializes the UID from the target pool, clears `paid`, and
stores the queue flag on the virtual base.

The constructor also canonicalizes generic PACK `0x28B` and DEPLOY `0x28C`. Attribute `0x7B`
selects `0x28D/0x28E`; object types `61`, `62`, or `400` select `0x28F/0x290`; otherwise
attribute `0x13D` selects `0x291/0x292`. The planner freezes that priority and leaves every
other type unchanged.

## Paid latch and domain split

The first activation calls `pay_cast_costs(spell, actor owner/object)`. A nonzero result kills
the order; only the locally controlled owner receives feedback. A zero result stores
`CastOrder.paid=1` before any target resolution. Later frames do not pay again. Successful
ordinary completion clears `paid` immediately before `kill_current_order`; two transport exits
deliberately retain it.

Retail asks the type object whether `order.spell` is a real SpellType. Non-spell type objects
use sentinel `0x296`. The targeted domain is reached only when the effective spell is not that
sentinel and `SpellType+0x1C8 & 0x0E != 0`. All other types enter the untargeted domain. The
planner rejects a host cone that disagrees with this exact split.

## Targeted executor

An inactive target can be replaced by `ObjectData::get_inside`; retail rewrites object, owner,
and UID before `is_valid_target`. Missing/invalid targets kill the order. BRIBE `0x275` has an
additional alliance/leader rejection cone. A valid target publishes actor held-target fields
at `+0xA2/+0xA8/+0xA6` in that order.

Coordinate-mode spells (`flags & 8`) use `CastOrder.x/y`; the other targeted arms use live
target coordinates. `get_range` returns the threshold and some target categories contribute a
retail margin. Outside `range+margin`, retail calls the sixteen-argument
`UnitType::find_nearby_spot`: a nonzero return simply holds; a zero return installs MOVE only
if the returned point now falls within the same boundary. The proof keeps request/result and
second-distance evidence typed, so a stale or fabricated point cannot become an order.

The channel tail then performs, where applicable, cross-owner visibility publication, facing,
animation, one-shot presentation, cloak/actor flags, and the spell timer. On job-time it resets
the timer, calls `SpellType::cast` with either the target identity or order point, clears paid,
and kills the order. The planner's ordered steps make those mutations non-commutative.

## Untargeted, pack/deploy, and transport executor

The untargeted first frame has separate pack/deploy, rare-collector reposition, merchant, and
ordinary animation cones. Pack/general arms can increment the actor spell timer twice and
increment contained-object timers before the common duration test. This is not interchangeable
with the targeted one-increment clock.

TRANSPORT `0x28A` is exceptional. Its opening nearby-spot failure kills the order without
clearing paid. At apparent completion a still-current CAST order decrements the just-incremented
timer and returns. Once completion proceeds, the real transport spell call returns without the
ordinary clear-paid/kill tail. These outcomes have distinct terminal types
(`KilledPaidRetained` and `TransportRetained`).

Real untargeted spells call `SpellType::cast(actor, 0, 0)` at completion. A non-spell sentinel
executes the type-specific side effects but makes no fabricated spell call. Its distinct tail
may resolve a building and perform `go_inside`/`come_out`; unlike real TRANSPORT, that tail then
continues to the ordinary paid-clear and order-retirement sequence.

## Atomicity contract and honest closure

Preflight binds actor/order versions, object/leader/spell/world epochs, queue digest, effect
epoch, complete branch facts, and the exact ordered plan. Commit must compare the entire
snapshot and recompute. Any paid-latch, target pool, alliance, coordinate, range, visibility,
timer, contained-object, transport, or effect change invalidates the receipt and authorizes no
partial publication.

Thirteen host tails remain explicit: concrete payload save/resume; spell table/flags; costs;
target/inside resolution; target validity/alliance; range/nearby spot; visibility/facing/anim;
presentation; cloak/actor flags; timer/contained objects; spell effects (including transport
completion); non-spell building transfer; and dispatcher atomic commit. This source pack
therefore changes the strict closure count by **0**. Integration must reuse existing
nearby-spot, visibility, movement, containment, and
spell-system owners rather than creating duplicate state.

## Validation boundary

Root convergence formatted the isolated files and validated all 19 tests in persvati batch
`gen7-five-pack-20260809T231109Z-3866-5144-5f896c0277b5`. Retail was not run. The focused
reproduction is:

```text
cargo test -p don-sim --test cast_order_frontier
```
