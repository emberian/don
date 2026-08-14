# Replay Goods resource-schedule splice

## Result

`crates/don-replay/src/replay_goods_resource_schedule.rs` connects the landed
initial/oil Goods runtime to the allocation transcript already retained by the
`Map::place_resources` schedule. It is an exclusive, unregistered adapter: the
integration test path-mounts it, and no shared orchestrator or module index is
changed.

For every admitted `ResourceAllocation { kind: Good }`, it reconstructs the
exact checksum-visible base-Good row and the exact sparse `PtrArray<Good>`
allocation chronology. Each accepted allocation owns 21 additional walked
Goods bytes:

```text
ever_seen : u8       = 0
flags     : u8       = 1 | (GoodTypeData::is_flat ? 0x20 : 0)
who       : u8       = 0xff
o         : i16 LE   = allocated slot
z         : i32 LE   = 0 ^ 0x63637
x         : i32 LE   = Coord.x ^ 0x63637
y         : i32 LE   = Coord.y ^ 0x63637
TypeIndex : i32 LE   = allocation type
```

The Z value is zero because `Map::place_resources` still executes inside
`Map::make`, before `TerrainOut::init`; `TerrainOut::find_tcoord_z` therefore
uses the zero fallback. `GoodData::cur_time` is reset to zero but is not walked.

## Exact allocation splice

The adapter starts from the same public `OilGoodRuntime` projection exposed by
`ReplayInitialGoodsRuntime::goods`. It reuses the first inactive slot across the
full logical length, otherwise appends and grows engine capacity with the
retail `0 -> 4 -> 8 -> 16` negative-increment rule. The schedule's claimed slot
must equal that result. `good_mark` rises to `max(mark, slot + 1)` and never
shrinks.

The schedule receipt already binds each allocation to the exact region/player
call site, `Objects::init_good` (`0x00653f30`), coordinates, type, slot, and
ordinary `WData.down=-2/down_who=slot` writes. This splice revalidates those
Goods-relevant fields. Type 5 remains the special no-occupancy Oil arm. Item
allocations are counted but do not mutate the base-Goods owner.

One behavior-driving field is not present in `ResourceAllocation`:
`GoodTypeData::is_flat` (`0x004780c0`), called by `SubObject::init`
(`0x00662300`). The caller must provide one evidence-bearing
`ResourceGoodTypeFact` for every non-oil type. Missing, duplicate, or stale
facts refuse the entire splice without mutating the input runtime.

## Quantified ownership and checkpoint boundary

For an input oil prefix with `B0` active walked bytes and an admitted placement
history containing `G` Good allocations, the output owns exactly:

```text
B0 + 21 * G Goods bytes
```

The receipt publishes before/after Goods checksums, exact active row counts,
logical length, engine capacity, `good_mark`, each reused/appended slot, and the
per-allocation checksum transition. It does not add array metadata to channel
11: retail `CheckSums::check_goods` walks only active Good rows in logical slot
order.

No replay checkpoint becomes owned in this tranche. The caller checkpoint at
`0x0068c72d`, source token `0x1ef7`, remains pending. The current public schedule
can cross BONUSES cleanup and reach the first FISH row at `0x0068fbb3` (or the
next `0x00690225` cleanup for an empty FISH section); FISH row execution,
`GOODIES`, document cleanup, the `Map::place_resources` return, and the caller
continuation remain red.

There are therefore two honest residuals:

1. without an admitted placement receipt, the first red Good producer is the
   opaque `Map::place_region_resource` (`0x00690480`) or
   `Map::place_player_resource` (`0x00691f70`) body selected by the first winning
   BONUS row;
2. after all admitted BONUS placement receipts are spliced and BONUSES cleanup is
   accepted, the first remaining schedule boundary is FISH row zero at
   `0x0068fbb3`, or the FISH category tail at `0x00690225` when empty.

The corpus status is unchanged by design: 21 checksum-bearing recordings have
nonempty first Goods checkpoints, while the installed replay producer still
walks zero Goods bytes. This adapter quantifies bytes derivable from a concrete
admitted schedule; it does not fabricate the missing per-recording placement
facts or install a partial channel.

## Future hooks

After the source is registered, the replay map-generation owner should pass its
single `ReplayInitialGoodsRuntime::goods()` projection and the final admitted
`MapMakeResourceScheduleReceipt` to
`continue_goods_through_resource_schedule`. The returned `goods_after` must be
retained for the `GOODIES`/`FISH` continuation. Only after the schedule reaches
the caller checkpoint may its `active_nodes()` be installed in `SimState` for
channel 11.
