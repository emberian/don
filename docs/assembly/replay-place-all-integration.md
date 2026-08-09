# Replay `TerrainGroups::place_all` integration

## Closed boundary

The replay map prefix reaches `TerrainGroups::place_all` at `0x006a70d0` after
the selected tileset's fractal plane has been consumed by `fill_fertile`.
`crates/don-replay/src/place_all_boundary.rs` now composes that replay-owned
state into don-sim's complete heterogeneous placement transaction through the
native return of `1` at `0x006a937d`.

The adapter reuses only facts already proved by the replay prefix:

- `InitialWorld::world` supplies generated WData and start arrays;
- `InitialWorld::generation_regions` supplies the rebuilt region lists;
- `InitialItemReconstruction::inputs.map_style` supplies the replay selector;
- `ContinentReceipt::rng_final` supplies the exact main-RNG handoff; and
- `ContinentReceipt::starts_added` proves the generated start-array lengths.

It rejects any plan not stopped at `0x006a70d0`, any missing `fill_fertile`
receipt, a mismatched continent style/common-tail receipt, a start-count
mismatch, or TData whose shape and length do not match the replay World.

## Explicit absent producers

Ordinary `.rcx` setup does not serialize the remaining runtime inputs. The
adapter therefore requires them as named typed facts rather than constructing
defaults:

| typed input | required producer |
|---|---|
| `ReplayPlaceAllRuntime::terrain_groups` | installed map-style/tileset expression resolution into the native group catalog, subtype frequencies and `console_info` |
| `ReplayPlaceAllRuntime::mountains` | installed mountain-range lists and cursors |
| `ReplayPlaceAllTDataFacts` | exact installed/generated tile plane plus its three World dimensions |
| `DooberTilesetRules` | selected tileset's doober and mountain-fringe fields |
| `ReplayPlaceAllPlayerFacts` | `progress`, `place_players`, helping globals, player count and the full `[8][5]` score table |
| `ReplayPlaceAllHostFacts` | ordered group-indexed player/region external resolutions |
| callback | ordered daemon, doober and presentation effects emitted by a successful call |

An empty group input stream is accepted only when the supplied resolved catalog
selects no arm that needs a row. If the native selection reaches an omitted or
wrong-kind row, don-sim returns its exact `PlaceAllError` boundary. This is not
converted into a fabricated successful host effect.

## Transaction and checksum behavior

World, terrain groups, mountain cursors and RNG advance in clones. Host events
are buffered rather than executed as the simulation walks the transaction.
Only a full return of `1` commits World/runtime state, recomputes
`InitialWorld::checksum`, and releases the ordered event stream to the caller.
Any adapter validation error or don-sim placement error leaves World, runtime,
stored checksum and the external callback untouched.

The checksum byte-coverage counter is deliberately not increased. TData and
runtime facts are caller-owned installed/captured evidence, while the existing
counter measures bytes sourced from replay reconstruction. The receipt exposes
both the pre-call and committed checksums without relabeling external evidence
as replay bytes.

## Remaining replay frontier

This is a lawful injection adapter, not a parser for the missing producers.
The checked-in replay corpus still stops at `terrain_groups_place_all` until a
local installed-data resolver or live capture supplies the typed runtime,
TData, player/helping/score and host-resolution facts. Once supplied, the next
unclosed replay stage is the first call after `TerrainGroups::place_all`
returns at `0x006a937d`; this tranche makes no claim about initial goody/item
configuration downstream of that address.

Focused mutation tests live in
`crates/don-replay/tests/place_all_boundary.rs`. They pin the continent RNG
handoff and three mountain-list draws, TData checksum installation, reporting
host-event commit, invalid-report rollback, TData shape rejection, and generated
start-count proof.

## Frozen paths

```text
crates/don-replay/src/place_all_boundary.rs
crates/don-replay/src/lib.rs
crates/don-replay/tests/place_all_boundary.rs
docs/assembly/replay-place-all-integration.md
```
