# Replay Cities channel from the canonical Sim owner

Lane: `checksum-cities` · replay-correctness · 2026-08-13.

## Result

Channel 9 (`cities`, zero-based channel id 8) now has a conditional, exact producer. The
producer reads `Sim::cities`, validates every live City against the Sim-owned Leader and
center-Build identities, and then executes the retail dynamic walk. A failed join clears the
channel before returning the refusal, so no checksum from an older state can survive.

This is a producer result, not a retail-equality claim. The replay setup owner currently reaches
only three ordinary two-human, starting-town-1 recordings, and its City state is frozen after the
fresh constructor. `Leader::plan_strategy` and the starting Units still change checksum-visible
City bytes before the first recorded checkpoint. The new scoreboard therefore measures those
bytes as a substantive disagreement instead of treating Cities as absent.

## Retail traversal

The instruction-derived walker remains the one documented in
[`docs/mechanics/tech-cities.md`](../mechanics/tech-cities.md):

- `CheckSums::check_cities` `0x00937600` visits eight Leader slots in ascending order and
  admits an owner only when `LeaderData::leader_flags & 1` is set;
- it scans the complete logical `PtrArray<City>::length`, not `city_mark`, and calls
  `City::walk_data` only for rows with `city_flags & 1`;
- `City::walk_data` `0x00489220` emits `[+4,+6)`, then `[+6,+114)`, skips both `String`
  members for a checksum visitor, and finally walks `Array<CaravanLink>`; and
- `Array<CaravanLink>::walk_data` `0x00489040` hashes length and, when nonempty, capacity,
  growth short, masked flags, and every `{cara,who}` pair.

One fresh City with an empty caravan array therefore walks exactly 114 bytes. The admitted
two-player constructor image walks 228 bytes. The generic PDB-layout walker is intentionally not
used: it cannot follow the dynamic caravan array and reports an unresolved operation.

## Canonical-owner admission

`cities_runtime::check_sim_owned_cities` accepts only `Sim::cities`. Its lower-level explicit-pool
form remains available for offline reconstruction tests, but the replay bridge cannot select a
parallel pool. Before hashing, the adapter proves:

1. all four mirrors of the retail active-Leader bit agree (`vic_leaders`, step 8, economy
   Leaders, and the object registry);
2. `city_mark` is nonnegative and within the allocated pool, with no live row beyond it;
3. each live row's owner and City slot match its position in the pool;
4. its center object is a unique live Build in that owner's Build band;
5. the Build body and registry agree on object id, owner, City link, and exact position; and
6. the production runtime has a current type for that Build row.

`SimBridge::populate_sim_cities` clears channel 9, performs that admission, and publishes a direct
checksum only after it succeeds. The report carries zero unsourced bytes and the exact-producer
bit. A mutation test changes a walked City dword and observes the channel change, then corrupts
the center City link and proves the old direct value is removed on refusal.

`StartingSetupState` now copies its completed constructor pool into the canonical `Sim::cities`
owner before computing the constructor receipt. At the first harness step, `WorldSim` expires the
atomic setup pair and transfers only its canonical Sim owner to an explicitly frozen Cities slot
for later comparisons. This prevents a future completed Builds value from accidentally remaining
installed while keeping the independent City traversal observable. The Build/City setup *pair*
remains unpublished today: the incomplete Build initializer does not become authoritative merely
because the independent Cities traversal is now observable.

## Replay evidence

The full retail corpus was regenerated in fidelity mode after the bridge landed. Cities changed
from an absent channel with zero non-trivial/substantive comparisons to a conditional exact
producer on the three admitted recordings: **36,955** comparisons now walk 228 bytes with zero
unsourced bytes. Survival and matches remain zero: the constructor image is already stale at
retail's first checksum, which is the expected and useful result of this tranche. The per-recording
values are recorded in `schema/replay-validation.json` and summarized in
`docs/tracks/replay-validation.md`.

## Residual

The exact starting-Unit owner for the pre-checkpoint `Leader::plan_strategy` census now exists.
It clears each live City's `free` (`+0x5a`), `busy` (`+0x5b`), `gatherers` (`+0x5c`), and
`peasant_dist` (`+0x50`), then joins the validated setup allocation receipts to canonical
Scout/Citizen rows and applies the empty-action Citizen arm. It is not yet mountable in
`StartingSetupState`: that host does not materialize the receipt-backed Units in `Sim::world`.
The setup owner now installs the source-exact activation-time TData `CITY` footprint (CITY is
not a WData bit). The installed-content mode-one `World::gather_at` owner now closes the next
terrain-census boundary for `+0x62..+0x71`; its six-City candidate still matches 0/3 admitted
first checkpoints and remains unmounted. Final setup territory, including `WData::who/who2`
and City `bordering`, remains separate. These owners and the remaining setup schedule must
join before the frozen constructor can become a first-checkpoint-correct image.
No recorded Cities checksum is an input. See
[`replay-starting-city-unit-census.md`](replay-starting-city-unit-census.md).
