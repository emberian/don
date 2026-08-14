# Canonical `Map::place_resources` transaction

## Scope

`crates/don-replay/src/place_resources_canonical_transaction.rs` is the source-only
transaction owner for the supported body of retail `Map::place_resources`. It composes,
without reimplementing, the existing exact owners for:

- the first BONUSES mutation row;
- recurrence rows at `0x00690215`;
- the category tail at `0x00690225`;
- the carried row-zero entry at `0x0068fbb3` for FISH and GOODIES;
- the Player placement prefix and complete Player body; and
- the Region prefix and complete concrete-good `World` body.

This is deliberately not a Goods-channel or World-channel installation. The allocation
traits remain observational/two-phase boundaries. A caller may commit their proposed
external writes only after the complete canonical receipt has been accepted.

## Executable identity and native chronology

The composed owners remain bound to the supported shipped artifacts:

- PE SHA-256:
  `30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`
- PDB SHA-256:
  `334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`

The transaction preserves the Capstone/PDB-derived control chronology rather than
fitting checksums:

1. BONUSES row zero enters the existing `0x0068fb9d` owner.
2. Additional rows use the `0x00690215` decrement/recurrence prefix and re-enter the
   mutation body at `0x0068fc0d`.
3. Category cleanup begins at `0x00690225`, releases category and current-row host
   references, increments the category at `0x00690290`, and dispatches through
   `0x0068f754`.
4. FISH and GOODIES enumerate their rows through the transitive call at `0x0068fb98`,
   test the count at `0x0068fba8`, and enter row zero at `0x0068fbb3`.
5. Row zero then reaches the shared mutation cone at `0x0068fc0d`. It does **not** pass
   through the later-row recurrence entry at `0x00690215`.
6. GOODIES cleanup releases the selected/default documents and returns the accumulated
   placement count at `0x00690476`.

The separately typed carried-row API is necessary because the existing first-BONUSES
owner initializes native chance locals, while FISH and GOODIES inherit them. Reusing that
initial API would redraw or reset state at the wrong chronology. Pretending row zero were a
recurrence row would claim a decrement and previous-row host reference that retail did not
execute.

`execute_canonical_carried_first_row` exposes that exact row-zero composition as an atomic
public seam for the map-make schedule. It stages both `CanonicalPlaceResourcesState` and
`RemainingBonusRowsState`, rejects BONUSES/nonzero/empty or detached document handoffs, and
commits only after the generic carried-row and exact Player/Region receipts agree. Recurrence
at `0x00690215` remains a separate child.

## Authoritative state

`CanonicalPlaceResourcesState` contains every projection that the composed transaction may
change:

- RNG state;
- World checksum sections;
- sourced/walked byte count;
- the concrete six-field resource-divvy pool and its stable digest;
- allocated and requested counters;
- stable Good and Item state digests; and
- selected/default XML document host handles.

The native chance locals are carried inside the row/category transaction:

- last chance group (`[ebp-0x90]`);
- signed chance budget (`[ebp-0x2c]`); and
- winner flag (`[ebp-0x28]`).

The outer function clones the complete state, executes every category against the staged
copy, validates the final return receipt, and commits only once. A stale FISH/GOODIES tail,
wrong category plan, rejected exact body receipt, bad allocation, invalid RNG transcript,
or pool mismatch therefore leaves all local authority unchanged.

## Exact placement adapter

The row owners intentionally accept a generic `PlacementHost`. The canonical transaction
provides a private typed adapter rather than weakening that interface.

For Player rows it:

1. projects the exact `PlayerPlacementPrefixRequest` from the XML placement parameters;
2. executes the registered prefix;
3. executes `execute_player_resource_body` with the concrete pool and Good/Item digests;
4. interleaves each selector pool transcript before its corresponding point draw; and
5. projects exact Good/Item allocations, WData writes, RNG, World, and pool state back into
   the generic row receipt.

For Region rows it admits only the concrete selector-zero `World` pattern. It executes the
registered region prefix (including the `Map::find_avail_regions` draw at `0x0068f498` when
present), then the exact Region World body. Pool selectors, items, and non-World region
patterns remain typed refusals; no opaque body receipt is fabricated for them.

After the generic row owner accepts the projected receipt, the transaction compares the
exact body's final RNG, World, pool digest, allocation count, and all three chance locals to
the row owner's state. Good, Item, and concrete-pool authority advances only after that
comparison succeeds.

## Focused proof

`crates/don-replay/tests/place_resources_canonical_transaction.rs` is a path-owned test so
synthetic evidence is confined to the focused fixture. The production module is registered,
but no replay channel or map-make schedule invokes it while the complete caller schedule is
still being assembled. Its focused cases prove:

- exact BONUSES→FISH→GOODIES order and chance carry;
- final selected/default document release;
- late-category rollback of RNG, World, pool, counters, Good, Item, and document handles;
- successful composition of the exact Player body; and
- successful composition of the Region prefix and exact concrete-World body.

The shared carried-row test additionally mutates category, order, row state, and the
placement RNG receipt. It proves that carried evidence is accepted only for FISH/GOODIES
row zero and that refusal is atomic.

Persvati validation (with the allowlisted `final-balance-runtime.bin` asset):

- `replay-carried-category-row-20260811T185251Z-54256-5495-4f5790270304`:
  15 passed;
- `replay-resource-canonical-registered-20260811T191345Z-90728-30968-610c168dad4c`:
  5 passed with the production module registrations overlaid;
- `replay-resource-canonical-lib-20260811T191323Z-90312-10325-610c168dad4c`:
  clean `cargo check -p don-replay --lib`.

No VM, live heap, stage, installed replay channel, or checksum-fitting path was used.

## Typed external boundaries

The source owner intentionally stops short of:

- selector or Item execution in the Region function;
- Region patterns other than `World`;
- committing allocation-host proposals;
- replay Goods/World channel installation;
- the unresolved complete caller schedule and checksum-token reachability; and
- the upstream caller token `0x1ef7`.

Those boundaries remain explicit in `CanonicalPlacementFacts`,
`CanonicalResourceAllocationHost`, and `CanonicalPlaceResourcesError`; they must not be
filled with checksum-derived behavior.
