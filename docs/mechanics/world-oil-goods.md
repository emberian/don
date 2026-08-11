# `World::set_oil_at`: oil-cell and base-`Good` transaction

This note closes the first object-system leaf reached by Great Lakes (map style 14) in
`TerrainGroups::place_all`.  The supported image is
`ron-bin/riseofnations.exe` (SHA-256
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`).
Names and layouts come from the shipped PDB; control flow and constants below were read
from the PE32 instruction stream with Capstone and checked against `re/decomp-all`.

The executable owner is `crates/don-sim/src/systems/world_oil_goods.rs`.  Its claim is
static/in-tree, Tier C.  No retail differential run was used and no checksum was fitted.

## The call is not a scalar World setter

The PDB names `World::set_oil_at(WCoord const&, WCoord const&, int)` at `0x006b2a10`
(248 bytes).  Its instruction stream performs this sequence:

1. compute `wdata[(wy * xs + wx) * 28]`;
2. scan slots `0 .. ObjectsData::good_mark` in ascending order;
3. for each active `Good` whose TypeIndex is 5 and whose decoded X/Y map to the requested
   WCoord, dispatch virtual `Good::close` (`vtable + 0x90`);
4. if disabled, clear `WData::flags & 0x0800` and return;
5. if enabled, set `0x0800` and call `Objects::init_good(5, wx*0x300+0x180,
   wy*0x300+0x180)`.

The coordinate comparisons decode `SubObject::{x,y}` with XOR key `0x63637`, arithmetic
shift by eight, and `div_3_table`.  Thus the scan compares WCoords, not exact centre
coordinates, and closes **every** duplicate in the cell.  None of these operations reads
the main RNG.

`map_terrain::World::set_oil_at` already owns step 4/5's WData bit.  It intentionally did
not claim the object work.  `world_oil_goods::apply_world_set_oil_at` composes that scalar
method with the exact Good owner transaction rather than adding a second World bit writer.

## `Objects::init_good` and sparse storage

`Objects::init_good` is at `0x00653f30` (680 bytes).  The global `goods` object is the
PDB-named `PtrArray<Good>` at `0x00c0a0e0`; its relevant fields are length `+4`, capacity
`+8`, signed-short increment `+0xc`, list `+0x10`, flags `+0x14`, and iterator cursor
`+0x18`. `ObjectsData::good_mark` is `Objects + 0x154`.

Allocation scans the **full array length**, not `good_mark`, and reuses the first slot
whose active bit is clear.  Only when no hole exists does it append.  `Objects::init`
sets length 0, increment -1, and flags 0; at cold process start the static capacity is 0
(a later init can retain an already nonnegative capacity). `ArrayBase::increase_size`
(`0x00422300`) maps a negative increment to the current capacity, except empty capacity
grows by four.  The cold sequence is therefore `0 -> 4 -> 8 -> 16 ...`; a positive
increment is a fixed step.  The port refuses increment zero at a full array because retail
would not enlarge the buffer, and it refuses nonzero array flags because the style-14
mapgen owner is entered after `Objects::init` cleared them.

After virtual initialization, `good_mark = max(good_mark, slot + 1)`.  Closing a Good
does not lower the mark, remove the pointer, compact the array, or shrink capacity.  These
facts are mutation-sensitive in `crates/don-sim/tests/world_oil_goods.rs`.

## Exact type-5 initialization

The base `Good` is 48 bytes.  `Good::init` (`0x0066da20`) enters `SubObject::init`
(`0x00662300`) with owner -1, TypeIndex 5, the allocated slot, and the cell-centre Coords.
The resulting scalar state is:

| field | initialized value |
|---|---:|
| active flags byte | `1` |
| `who` | `0xff` |
| `o` | allocated slot truncated to `i16` |
| `x`, `y` | raw Coord XOR `0x63637` |
| type | TypeIndex `5` (`Oil`) |
| `ever_seen` | `0` |
| `cur_time` | `0` |

`GoodTypeData::is_flat` (`0x004780c0`) returns false for this type, so initialization does
not add another flag bit.  The type-5 arm returns before the ordinary resource footprint
and `World::set_down` writes: a newly initialized oil Good adds no TData `RESOURCE` bit,
no WData footprint bit, and no `down=-2/down_who=slot` occupancy marker.

`SubObject::init` also calls `TerrainOut::find_tcoord_z` (`0x008544a0`).  Setup chronology
is decisive here: `Map::make` calls `TerrainGroups::place_all` before `TerrainOut::init` in
`Setup::build_game` (`0x005ac190`).  `find_tcoord_z` sees an empty
`master_land_heights` array and returns the zeroed `land_height` fallback.  The logical Z
is therefore 0 and its walked word is `0 ^ 0x63637`.  The API is intentionally a mapgen
owner; using the fixed Z after terrain initialization would be a false generalization.

## Exact oil close and the fail-closed object-list edge

`Good::close` is at `0x0066d860`.  The shipped live catalog row
`schema/live/live-tables-good.tsv` establishes type 5 (`Oil`) with `x_size=1`, `y_size=1`,
and `block=0`.  Its deterministic close effects are consequently:

- clear TData `RESOURCE` (`0x0200`) at the Good's decoded centre tile;
- clear WData footprint bit `0x0001` at the Good's decoded WCoord;
- call `World::clear_down` (`0x006b3ad0`) for that WCoord;
- set the flags byte to zero, set Z/X/Y to constructor sentinel `0xfff9c9c8`, and null the
  type pointer;
- preserve `who`, `o`, `ever_seen`, and `cur_time`.

For a negative `WData::down`, `clear_down` unconditionally normalizes both `down` and
`down_who` to -1.  A nonnegative head traverses the general object linked list and rewrites
its tail.  The map-generation runtime does not own those object bands.  It therefore
refuses a matching oil close with nonnegative `down`, transactionally, rather than
inventing an unlink.  The ordinary post-`World::wipe` style-14 state has `down=-1` and is
inside the implemented arm.

All request/runtime/world validation and the entire mutation execute against cloned owners.
An error commits neither the World nor goods storage.

## Checksum consequences

`CheckSums::check_goods` (`0x00937710`) scans the full PtrArray logical length, skips
inactive objects, and walks active Goods in slot order. This is deliberately different
from the `good_mark` bound used by `World::set_oil_at` and scenario saving.
`Good::walk_data` (`0x0066e5d0`) plus
`SubObject::walk_data` (`0x006621d0`) contributes exactly 21 scalar bytes per active row:

```text
ever_seen : u8
flags     : u8
who       : u8
o         : i16 LE
z         : i32 LE, XOR-encoded
x         : i32 LE, XOR-encoded
y         : i32 LE, XOR-encoded
TypeIndex : i32 LE
```

`GoodData::cur_time` is not walked.  Neither `PtrArray` length/capacity/increment nor
`good_mark` enters channel 11's special checker.  They still determine future slot order
and are save/DataWalk-critical state, so the runtime and receipt retain them.

World effects land only in section 5 (WData) and, when closing a footprint whose resource
bit is present, section 6 (TData).  The receipt records before/after full World digests,
the exact changed WData/TData indices, changed section numbers, before/after goods digests,
walked-byte counts, and zero RNG draws.

One subtle but essential point: `economy::GoodNode` is reused as the 21-byte walker row,
but its Z/X/Y values here are the internal XOR-encoded words.  Hashing decoded display
coordinates would diverge immediately.

## Scenario and generic-save consequences

`ScenarioWrite::save_goods_chunk` (`0x009a7380`) scans `0 .. good_mark`, counts active
base Goods, and writes them in slot order in chunk type `0x17`.  Its semantic record is the
resolved type name plus decoded raw X/Y.  For type 5 the synchronized catalog name is
`Oil`. `OilGoodRuntime::scenario_rows` exposes `(slot, TypeIndex, decoded X, decoded Y)`;
the catalog owner supplies names for non-oil rows.

The scenario format does not preserve inactive holes, capacity, increment, or
`good_mark`. `ScenarioRead` resolves each name and calls `Objects::init_good`, compacting
the active rows through the ordinary allocation path.  By contrast, generic `PtrArray`
DataWalk/save state writes length and, for a nonempty array, capacity, increment, flags,
pointer-presence rows, and objects.  The runtime therefore does not confuse a matching
scenario row stream with matching in-memory storage.

The fixed-size scenario record contains a 255-wide-character type-name buffer.  Bytes
after the terminating NUL can contain stale stack data, so this tranche claims semantic
row order/content, not a fabricated byte-identical padding blob.

## Integration hook

Registration is one future `pub mod world_oil_goods;` in `systems/mod.rs`; this lane does
not edit that shared file.  At a typed `DropTileExternalRequest::OilGoodMutation`, the
terrain continuation must:

1. validate/copy all six fields into `world_oil_goods::OilGoodMutation`;
2. call `apply_world_set_oil_at` with its owned `World` and `OilGoodRuntime`;
3. replace the external request with `OilGoodsApplied` only on `Ok(receipt)`;
4. retain the receipt in the enclosing region/place-all transaction.

No synthetic void acknowledgement is evidence that this owner ran.  The caller must also
clone its enclosing terrain/RNG state (or otherwise transact it) so a refusal cannot leave
the prefix committed.  This leaf itself consumes zero main RNG words.
