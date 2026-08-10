# Replay `Map::place_resources` later `BONUS` rows mutation frontier

`crates/don-replay/src/place_resources_bonus_rows_mutation_frontier.rs` continues the
first-row receipt at `0x00690215` and owns every remaining row of the **current**
`BONUSES` array as a sequence of atomic row transactions. The category-complete
residual is `0x00690225`.

This remains a source-only frontier. It does not claim category cleanup, the
`GOODIES` or `FISH` XML sections, the `Map::place_resources` return, the caller
continuation at `0x0068c72d`, or source token `0x1ef7`.

Authority is the shipped `ron-bin/riseofnations.exe`, SHA-256
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`, and its
GUID-matched `rise.pdb`, SHA-256
`334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`.

## Exact recurrence

The first-row owner stops before:

```text
0x00690215  row pointer += 0x28
0x00690218  --rows_remaining
0x0069021c  rows_remaining != 0 ? 0x0068fbb3 : 0x00690225
```

Each remaining row therefore reuses the already recovered
`0x0068fbb3..0x00690215` body. The semantic difference from row zero is the native
carry in three stack locals:

```text
[ebp-0x90]  last chance-group key
[ebp-0x2c]  signed chance budget
[ebp-0x28]  previous winner flag
```

`RemainingBonusRowsState::from_first_row` reconstructs those values from the first
facts and receipt and retains the category source plus the complete ordered row
ordinal/handle topology. Every later-row evidence record binds the carry together with
row index, capture ordinal, RNG state, full World checksum, walked byte count, and the
logical resource-pool digest. This prevents a capture for one bucket position from
being replayed at another or applied to a spliced row array.

## Chance-bucket state machine

For a recognized row, retail performs this exact state transition:

```text
if group != last_group || group == 0:
    last_group = group
    budget = Random::get(0, 0xffff) % 100
    winner_register = false
else:
    winner_register = winner

if budget < 0 && (chance != 0 || !winner_register):
    winner = false
    skip without subtracting

budget = wrapping_sub(budget, chance)
if budget >= 0:
    winner = false
    skip

winner = true
numrare = Map::scale_number(...)
if numrare == 0:
    skip placement while retaining winner = true
else:
    call player or region placement
```

Consequences frozen by the focused proof:

- group zero redraws on every row, even when it equals the previous key;
- changing from another key to `-1` draws; `-1` is draw-free only when it was already
  the carried key;
- an unknown type exits before reading or changing chance carry;
- a negative carried budget blocks a later nonzero-chance row and clears the winner;
- a zero-chance row immediately following a winner reuses the negative budget and may
  place again;
- `numrare == 0` performs no placement but leaves the native winner flag set.

The direct draw remains the main simulation stream at `[0x00c06184]`, calling
`Random::get(0, 0xffff)` at `0x0068fd64` and using signed remainder by 100.

## Placement and World mutation boundary

Winning rows retain the first owner’s typed two-phase boundary around
`Map::place_player_resource` (`0x00691f70`) and `Map::place_region_resource`
(`0x00690480`). A receipt must validate its complete admitted RNG transcript,
allocation call sites, allocation identity, resource-pool digest, and World checksum
before the row cursor, chance carry, RNG, pool digest, counters, or World projection is
committed.

The checksum-visible allocation writes remain:

- `Objects::init_good` (`0x00653f30`): `WData.down = -2` at `0x0065403e`, then
  `down_who = slot` at `0x00654043`; good type 5 writes neither field;
- `Objects::init_item` (`0x00653e00`): `WData.down = -3` at `0x00653f05`, then
  `down_who = slot` at `0x00653f0c`.

The only accepted checksum delta is `WData` when at least one occupancy write exists,
or no section when none exists. Rejection rolls back the row cursor and chance carry
as well as the RNG/World/pool/counter projection.

## Why schedule integration remains open

The existing schedule reaches a fully bound `PlaceResourcesBonusRowsHandoff`, but the
opaque selector placement receipt exposes only a post-call pool **digest**, not the
validated post-call `ResourceDivvyPoolState`. Wiring later rows into the public
schedule would leave its mutable pool object at the prefix while claiming a newer
logical digest. That is not an executable continuity proof.

The integration owner should first add a concrete post-placement pool projection (or
an exact placement-body port), validate its digest and bitmask mutation, and then wire
the first and later-row receipts. Even after that, the public schedule must retain
`checkpoint: None` and token `0x1ef7` as pending until `GOODIES`, `FISH`, cleanup,
return, and caller continuation are all owned.

`crates/don-replay/tests/place_resources_bonus_rows_mutation_frontier.rs` freezes ten
paths: same-group budget reuse, group-zero redraw, transition-to-`-1` redraw, unknown
type carry preservation, negative-carry suppression, zero-chance winner reuse,
zero-`numrare`, admitted WData mutation, and full rollback on a rejected placement
receipt, plus rejection of a spliced row array. The corrected first-row proof adds the
matching zero-`numrare` case.
