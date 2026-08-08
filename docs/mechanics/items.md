# Items / ruins recovery boundary

Status: **Tier C; safe to declare for isolated compilation, not safe to wire into
`World::tick` yet.** The implementation is
`crates/don-sim/src/systems/items.rs`. No item behavior has been differentially tested
against retail, and the replay bridge does not populate or hash this state.

Everything described as measured below comes from `riseofnations.exe` (sha256
`30478a44…625079`), its matching shipped PDB, or the shipped `itemrules.xml`. Community
documentation was not used.

## What an item is

The retail item class contains only goody boxes / ruins:

- PDB `TypeIndex`: `GOODY = BASE_ITEMTYPES = 543`, `END_ITEMTYPES = 544`.
- `ron-data/itemrules.xml` contains one item row.
- Every `Objects::init_item` call site passes `543`.

Rare resources are `Good` objects and terrain clutter is `Doober`; neither belongs in the
`items` checksum channel.

## Recovered behavior

| capability | retail source | current boundary |
|---|---|---|
| stable first-free slot allocation and item placement | `Objects::init_item` `0x00653E00`, `Item::init` `0x006770E0` | complete inside the item registry; terrain height is injected |
| removal and occupancy unlink | `Item::close` `0x00677050`, `World::clear_down` `0x006B3AD0`, `Unit::explore_goody` `0x005F9780` | complete for sentinel heads and through the `ItemObjectChain` integration trait |
| lookup behind heterogeneous objects | `ObjectsData::find_goody_at` `0x0065C040` / `0x0065B7C0` | complete through `find_goody_at_with_objects`; sentinel-only APIs are named and documented as such |
| per-player visibility | `ItemData::is_seen` `0x00677850`, `World::reveal_fog` | full three-stage predicate: shared `ever_seen`, Spanish owned-cell `was_seen`, then current `is_seen` |
| ruins resource award and RNG placement | `Unit::explore_goody` `0x005F9780` | five ordered resource candidates at most; Knowledge skipped; one main-stream draw per available candidate; full lookup/unlink available through `explore_goody_with_objects` |
| scout target search | `Unit::find_goody_box` `0x005F2540` | exact 49-cell retail spiral, region/item gates, four fog probes, and full item visibility; movement-order emission remains outside this module |
| checksum bytes | `CheckSums::check_items` `0x00937790`, `Item::walk_data` `0x00677150` | 22 bytes per live item, Adler-32 seeded with 1, live slots in stable array order |

All of these claims remain Tier C: instruction-stream transcriptions with local tests, not
oracle comparisons.

## Two important contracts

### `explore_goody` is caller-gated

`Unit::set_new_location` calls `Unit::explore_goody` only after testing the destination
cell's `WFLAG_ITEM` bit and the unit/domain conditions. The callee itself does **not** return
early when `find_goody_at` returns `-1`; a direct call still performs the normal payout and
RNG draws. The former test claiming “an empty cell is a no-op” was therefore a hand-written
expectation that contradicted the binary. The implementation preserves the retail callee and
tests the caller precondition explicitly; an integration wrapper must perform the gate.

### `ever_seen` uses a 32-bit shift

Fog reveal computes `1 << (who & 31)` in a 32-bit register, then truncates it into the
8-bit `ever_seen` field. Players 8..31 therefore write zero; they do not wrap to bits 0..7.
This is irrelevant for valid player slots but matters for faithful malformed-state behavior.

## Object-chain boundary

W-cells hold `(down, down_who)`. Non-negative `down` values identify a live object and its
`(next, next_who)` link; negative values are sentinels, including `-3` for an item. The
`ItemObjectChain` trait is the minimal interface needed to:

- traverse a live head to find the terminal item;
- replace a terminal `next < -1` with `-1` during `World::clear_down`;
- perform the second unlink pass in `Unit::explore_goody`.

Invalid references and cycles return `ItemChainError` instead of dereferencing arbitrary
memory or hanging. Valid retail object chains are acyclic. The trait is ready for the World
object store to implement; this module does not invent a parallel object registry.

## `Terrain::move_goody` is quarantined

There is deliberately no approximate `move_goody` API. Retail's `Terrain::move_goody`
`0x0084A3D0` is a 2,798-byte terrain-repair transaction, not an item relocation helper. It
handles items, `Good` objects, forests, terrain visibility, height, and occupancy together.
The item arm:

1. snapshots the pre-change land values of 48 spiral neighbors;
2. removes the source item;
3. scans retail spiral entries 9..24;
4. applies bounds plus a long set of old-land, new-cell flag, occupancy, good, visibility,
   terrain metadata, and south-forest predicates;
5. reinitializes the same item slot at the first accepted cell with a terrain height query.

This item-only module does not own enough state to evaluate those predicates. Moving a ruin
with a shorter predicate would be plausible and wrong. Land that function only as a
transaction spanning the terrain, goods, forest, object, and items owners.

## Tests

Because `items.rs` is not yet declared in `systems/mod.rs`, the isolated gate includes the
real crate RNG by path:

```sh
rustc --edition 2021 --test --crate-name items_harness -o /tmp/don-items-harness - <<'RS'
#[path = "/Users/ember/dev/don/crates/don-sim/src/rng.rs"]
mod rng;
#[path = "/Users/ember/dev/don/crates/don-sim/src/systems/items.rs"]
mod items;
RS
/tmp/don-items-harness --test-threads=1
```

Current result: **55 passed, 0 failed** (49 item tests plus 6 tests from the included real RNG
module). Tests cover the walked byte order, slot identity, RNG draw counts, visibility branch
order, 32-bit reveal shift, heterogeneous object unlink, cycle rejection, and scout spiral.

## Remaining integration gates

1. Declare the module so its tests join the workspace umbrella.
2. Implement `ItemObjectChain` on the real World object store and use the `*_with_objects`
   APIs; do not wire sentinel-only helpers into the simulation.
3. Gate `explore_goody` at the `Unit::set_new_location` call site and integrate leader buckets,
   rule constants, fog state, and the main simulation RNG.
4. Populate item slots, W-cell markers, `ever_seen`, and stable slot identities from replay or
   save-game setup; then connect `checksum_items` to the bridge.
5. Land `Terrain::move_goody` only through the cross-subsystem transaction above.
6. Add retail differential cases before promoting any behavior above Tier C.

Module declaration means “compiled and available for integration,” not “items are simulated”
and not “the replay `items` channel is non-trivial.”
