# Arena construction and gather product boundaries

This note audits the seven construction/gather blockers that remain reachable from the
Arena product surface. It records the authoritative islands that are now wired and, just
as importantly, the inputs which still send a playable match through MODEL 2 or MODEL 3.
No closure-registry entry is removed by this tranche.

## Construction

| blocker | executable authoritative island | reachable reason the blocker remains |
|---|---|---|
| placement | shipped Barracks and Tower outside-city land placement consumes live `(who,o,uid)`, footprint TData, occupancy, exploration, territory and transient start markers | Market/Temple linked-town limits, fort/city/gather/dock/adjacency/water families and incremental border invalidation are absent |
| lifecycle | ResearchModel Barracks/Tower executes the shared start/reject/activate transaction against the live site and ordered builder identity | claim-bearing reswarm/animation, broad city/leader/registry activation, special building families and the complete `build_done` reassignment tail remain compact projections |
| interruption | an installed `BUILD_AT` replaced by a player command records `OrderRetired(OrderCancelled)`; reaping a dead builder records `OrderRetired(BuilderDied)`. Both execute `construction::interrupt_builder`, retire the motion order, clear the stored generational target and report zero RNG plus the units checksum channel | target destruction deliberately does not visit builders; its next `do_build` invalid-target arm and the target close/disband/refund/terrain transaction remain incomplete |

Command replacement runs the interruption transaction before gather retirement, seat removal,
generic order clearing, or installation of the replacement command. Builder death runs it in
`reap` before the object is unlinked. Internal path failure, target invalidation and normal
completion use a non-interruption detach path, so none can be mislabeled as an explicit cancel.

## Gathering

`World::bind_authoritative_farm_site` is the only new product admission seam. It accepts a live,
completed shipped Farm only after the map retains the supported `rules.xml` identity and the
retail-shaped WData/TData owner has the same shape and seed. It then performs, transactionally:

1. the shared completed-Farm `BuildTypeData::calc_gather` evaluation over the real footprint,
   territory, diplomacy, LandData and river bits;
2. authoritative payout-source and per-worker-vector binding in `ArenaGatherRuntime`;
3. agreement between the evaluator's signed capacity and the runtime site capacity; and
4. publication of the binding plus the compatibility `Ent::worker_cap` only after every
   fallible step succeeds.

An admitted Farm Gather command executes the shared first-tick attachment and the persistent
owner-local worker/site chain. Exact active occupancy composes with the retained six-slot vector
in `tick_economy`; `PEASANT_RATE` is not consulted for that site. Halt or command replacement
executes the shared gather-order retirement before clearing the Arena seat.

The generated Arena map does **not** retain a retail LandData plane. Merely attaching supported
`rules.xml` bytes does not turn its terrain labels into retail resources: the exact Farm
evaluator can therefore return a real zero footprint and zero payout. Arena preserves that zero
and never substitutes MODEL yield. A loader needs an extracted/coherent retail WData/TData image
before this seam can produce retail terrain income.

| blocker | executable authoritative island | reachable reason the blocker remains |
|---|---|---|
| capacity | Farm's literal signed capacity of one is runtime-owned and rechecked against the shipped Farm evaluator; retained-source Camp/Mine capacity evaluators exist in `ArenaGatherRuntime` | the default generator still derives Camp/Mine slots from terrain labels and has no retained terrain-object plane |
| occupancy | an admitted completed Farm uses exact attach, active-count and retirement chains with stable owner-local identity | generated Farm/Camp/Mine commands still have reachable MODEL seating; Camp/Mine product command adapters are absent |
| reservation | retained-source Camp/Mine discovery, ordered MiningList, verification and close primitives exist in `ArenaGatherRuntime` | no World product binding installs the retained Mountain/Cliff object arrays, so generated Camp/Mine commands still bypass this lifecycle |
| payout | admitted Farm per-worker evaluation and exact occupancy composition feed Arena's existing resource-tick tail without `PEASANT_RATE` | the complete Leader `do_gather` income/cap/expense/checksum transaction is not the Arena product owner, and generated sites retain MODEL local arithmetic |

## Blocker decision

All seven slugs remain `KnownDrift` in `schema/simulation-closure.json`. Each approximation is
still reachable in a normal generated Arena match, and the new authoritative Farm path itself
still depends on a separately supplied retail terrain plane. Removing any entry now would turn a
bounded exact island into an unsupported product-fidelity claim.

Focused executable proofs:

- `crates/don-ai/tests/arena_construction_interruption.rs`
- `crates/don-ai/tests/arena_authoritative_farm_product.rs`
