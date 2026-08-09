# Roads step 22 — incremental stray-road cleanup

`Roads::scan_and_kill_stray_roads` at retail VA `0x008956A0` is recovered in full in
`crates/don-sim/src/systems/roads.rs` and executes from the real 29-step driver after the
frame increment. The port includes both direct children:
`scan_and_kill_bad_tcoord` at `0x0088E100` and
`scan_and_kill_straggled_tcoord` at `0x0088E050`.

## Persistent state and scan cadence

The shipped PDB gives `Roads` size `0x698`. The reached persistent fields are:

| offset | field |
|---:|---|
| `0x128` | legacy road cache accidentally cleared by the off-map arm |
| `0x5FC`, `0x600` | `curscan_x`, `curscan_y` |
| `0x604` | `road_cache2[9]` |
| `0x628` | `build_cache[9]` |
| `0x64C` | `points_cache[9]` |
| `0x670` | `aqua_cache[9]` |

Each call visits exactly signed `WorldData::size / 500` WCoord cells. It increments the
x cursor before scanning, wraps x into y, and scans all sixteen terrain tiles of each cell.
The initial `(0,0)` cursor therefore visits cell `(1,0)` first. A world smaller than 500
WCoord cells performs no scan and the scheduled step reports vacuous.

For each tile, retail fills the nine cache entries in the order center, northwest, north,
northeast, east, southeast, south, southwest, west. The corresponding road flag masks are
`0`, then bits 31 down through 24. Building and aqua facts come from the terrain mask;
all nine connection points come from the **current scanned tile's** single selected
`RoadElementCandidate`. The instruction stream computes that pointer once at
`0x00895849..0x00895858`, then applies each direction mask to its `flags` dword at
`0x0089585E..0x00895870`. Neighbour road bits gate a direction, but neighbour candidate
records are never read.

The out-of-bounds store is a retail quirk, not a cleanup: disassembly writes zero at
`Roads+0x128+i*4`. It leaves the newer `road_cache2` entry at `+0x604+i*4` stale while
clearing the three other current caches. The port retains both arrays and mutation-pins
that behavior.

## Cleanup bodies

The bad-road body removes a road when its candidate is absent, when no connection points
remain, or when a connection points at a non-road neighbor. Terrain-creation and configured
support elements take retail's separate any-neighbor test. The straggled-road body then runs
from the cache populated before the first child: it removes one-ended roads with no adjacent
building or aqua support.

Road removal calls the reached `TerrainOut::road_changed(add=0)` behavior before
`World::set_road_at(false, quiet=false)`. Candidate release consumes a pending camel step
before decrementing `ref_count`; a candidate becomes absent only when both reach zero. The
headless form records the reached `road_cleared` presentation edge as `last_cleared`, which
is cleared at the start of every scheduled call so stale events cannot replay.

## Renderer-owned boundary

`Terrain::CoordInfo::roads_in_wcoord` at `+0x5C` points to sixteen
`RoadElementCandidate` records maintained by the product road renderer, not `WorldData`.
The deterministic core therefore accepts sparse `CandidateFact` inputs. A roadless tile
proves that no candidate is required. A live road without a supplied fact is preserved and
increments the named step-22 gap; connectivity is never invented from neighboring road
bits. Only the current live road's missing candidate can block its evaluation. Candidate
facts attached to neighbouring roads cannot block or alter that evaluation; those facts
become relevant later only when their own tiles are scanned. `Absent` and `Present` facts
execute the full recovered mutation path.

## Verification

Ten module tests pin the budget and increment-before-scan order, known-absent and missing
candidate branches, bad and straggled cleanup, building support, candidate-release order,
per-call event clearing, the stale off-map cache quirk, current-candidate ownership of all
direction masks, and an east/west mutation pair that fails if neighbour flags are consulted.
Three integration tests prove
the real tick executes 128 tile evaluations on a 64-by-64 WCoord world, charges only missing
renderer facts, and clears a live road when an exact absence fact is supplied.

Evidence tier remains C: shipped PDB layouts, executable disassembly/decompiler, and shipped
`roads.xml`, without a retail oracle comparison.

The corrected current-candidate ownership passed both independent profiles on 2026-08-09:
hbox `roads-step22-maskfix-v2-20260809T221638Z-57786-13777-c05be0c65aa3` and persvati release
`roads-step22-maskfix-release-v2-20260809T221638Z-57787-3664-c05be0c65aa3`, each with 10/10
focused module tests and exit 0. The first convergence attempt correctly failed the stale
neighbor-owned legacy fixture; v2 pins the reciprocal current-tile EAST/WEST masks.
