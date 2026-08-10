# `Unit::come_out(int)` gather-selection frontier

Status: third measured, source-only transaction planner; not integrated and not a
full-function closure.

## Provenance and boundary

This recovery uses the shipped `ron-bin/riseofnations.exe` (SHA-256
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`) and matching
`ron-bin/sbl/rise.pdb` (GUID `51D4F219-61C6-4F84-9D5B-C3361B0D291F`, age 1). The complete
9,925-byte `Unit::come_out(int)` was decompiled from the existing PDB-bound Ghidra project and
the owned instructions were cross-read with local Capstone PE32 disassembly. No retail process,
VM, or remote host was touched.

The sequential interval is `0x00618B22..0x006191A4`, 1,667 bytes. It also owns two compiler
islands used by this interval:

- `0x0061A271..0x0061A277`, the non-fast `SubObject::is(0x1A4,false)` return;
- `0x0061A278..0x0061A280`, the non-base `UnitData::is_captain` dispatch.

This is 1,683 logical bytes. It reduces the prior 6,032-byte residual to 4,349 bytes and ends
at `0x006191A5`, immediately before retail consumes the selected/fallback point.

## Selection machine

The loop is entered only for the captain/build path recovered by the common-release frontier.
It first requires a non-null `BuildData::gather.head_node`, `gather_inside() == 0`, actor
`order_type() == 0`, and positive `num_gather()`. `GatherPointList::head()` runs before the
positive-count test and therefore remains in receipt chronology even for zero count.

For list index zero, retail uses the raw GatherPoint coordinates. Every later index first calls
`UnitType::find_nearby_spot` with centre = raw point, radii `0..0x600`, radial step 0, angle
`0x55555555`, filter 3, actor identity, no collision acceptance, overlap object/owner `-1/0`,
and required region `-1`. A nonzero return skips the point. A zero return carries the child
coordinates for movement, but terminal selection always uses the raw GatherPoint coordinates.

The last point is accepted before its action byte or any building lookup is interpreted. This
is a shipped fallback, not a simplification.

### Non-terminal action handling

- action 0 reaches movement handling directly;
- other actions call `ObjectsData::find_any_building_at` at the raw point;
- no building plus actor attack, non-worker actor, and action 2 skips the point;
- all other no-building cases reach movement handling.

With a building, retail maintains independent `valid` and `blocked` locals:

1. Peasant + action 1 + same-owner wall-build:
   - inactive building or nonzero `ObjectData::myhits` sets `valid`;
   - otherwise a valid wall whose type is not a gather type and which is not University
     (`0x1A4`) sets `valid` when `num_gatherers < signed gather_max`;
   - invalid wall, gather type, University, or full gather capacity sets `blocked`.
2. An active allied building with nonzero garrison limit, actor `can_garrison(type)`, and
   action 1 performs the second authoritative limit read. `num_inside(0) + control_cost(0)`
   uses wrapping 32-bit addition; `<= limit` sets `valid`, otherwise `blocked`.
3. Actor attack + non-worker + enemy owner + action 2 forces the blocked-selection edge.

`valid` skips the point. With no valid result, blocked selects the raw point; otherwise retail
reaches movement handling.

### Movement and list cursor

Retail rechecks `is_captain`. The base slot is folded to the captain bit; an override is called
through the `0x0061A278` island. For a captain with `uber_size > 1` and non-negative scratch
group, `Unit::find_nearby_spot` supplies the movement point (its return code is ignored), then
`Group::action_move_to` receives the authoritative `find_angle` result. Otherwise retail emits
`Unit::add_move_facing_order` using the selected tile-data coordinates and angle. The movement
point becomes the next angle baseline.

After every non-terminal candidate, retail increments the list index and, when the current
`head_node` remains non-null, advances the mutable gather cursor fields in this order: node,
data, metric (`0x00619176`, `0x0061917C`, `0x00619184`). These are planned writes, never
published during validation.

If the loop exhausts, `0x0061919D` copies the first frontier's `container_gpiece` local into
both selected coordinate registers and leaves action zero. The planner preserves this unusual
retail square fallback exactly.

## RNG, receipts, and atomic publication

The owned 1,683 bytes contain no direct call to either shipped `Random::get` overload. Every
result-bearing or stateful child is nevertheless represented by a typed, ordered `RngStamp`
receipt: gates, searches, predicates, capacity reads, angle calculation, and order/group
publication. Missing, extra, wrong-kind, return-shape, or discontinuous receipts fail closed.

The isolated planner returns only ordered steps plus:

```text
GatherSelectionContinuation {
    resume_va = 0x006191A5,
    selected,
    action,
    terminal_selection,
    rng,
}
```

Opcode 49 still may not publish the wrapper, first prefix, common-release, or this selection
plan independently. The remaining 4,349 bytes must be recovered and the complete reachable
host transaction must commit atomically.
