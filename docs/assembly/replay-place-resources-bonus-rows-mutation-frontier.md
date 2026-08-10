# Replay `Map::place_resources` later `BONUS` rows mutation frontier

`crates/don-replay/src/place_resources_bonus_rows_mutation_frontier.rs` continues the
first-row receipt at `0x00690215`. It owns the row-pointer recurrence and the
RNG/World/pool/counter mutation projection from `0x0068fc0d` through the next
`0x00690215`, for every remaining row of the **current** `BONUSES` array. The
category-complete residual is `0x00690225`.

This later-row owner is compiled but is not yet connected to the public schedule. It
does not claim category cleanup, the
`GOODIES` or `FISH` XML sections, the `Map::place_resources` return, the caller
continuation at `0x0068c72d`, or source token `0x1ef7`.

Authority is the shipped `ron-bin/riseofnations.exe`, SHA-256
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`, and its
GUID-matched `rise.pdb`, SHA-256
`334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`.

## Exact recurrence and open host-reference seam

The first-row owner stops before:

```text
0x00690215  row pointer += 0x28
0x00690218  --rows_remaining
0x0069021c  rows_remaining != 0 ? 0x0068fbb3 : 0x00690225
```

Each remaining row routes through `0x0068fbb3`. Its first four calls release the old
tail/head references (`0x0068fbc0`, `0x0068fbda`) and acquire the new head/tail
references (`0x0068fbf7`, `0x0068fc0a`). `LaterRowHostRefBoundary` records that exact
call order and the before/after logical handle shapes, but native pointer identity and
refcounts remain owned by the capture host. The executable mutation projection begins
at `0x0068fc0d`; this frontier does not disguise the host-reference transition as a
local port.

The semantic difference from row zero is the native carry in three stack locals:

```text
[ebp-0x90]  last chance-group key
[ebp-0x2c]  signed chance budget
[ebp-0x28]  previous winner flag
```

`RemainingBonusRowsState::from_first_row` reconstructs those values from the first
facts and the exact fact projection now retained by its receipt. It also retains the
logical row vector accepted by the preceding XML receipt. This rejects row substitution
*after* carry construction; it is not a cryptographic proof that the captured XML bytes
contained that vector.

Every later-row evidence record binds the carry, row index, capture ordinal, RNG state,
full World checksum, walked byte count, logical resource-pool digest, and a stable
digest of every behavior-driving row fact. A fact projection cannot therefore be
substituted under the same admitted evidence record. Retail capture authority still
comes from the external capture hash; this source does not infer XML contents from a
merely nonzero digest.

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
(`0x00690480`). A receipt validates the admitted RNG chain,
allocation call sites, allocation identity, exact selector subreceipts, concrete
resource-pool state/digest, and World checksum
before the row cursor, chance carry, RNG, pool digest, counters, or World projection is
committed. The opaque placement host or exact port remains responsible for transcript
completeness; the caller-side validator does not claim a full placement-body port.

The checksum-visible allocation writes remain:

- `Objects::init_good` (`0x00653f30`): `WData.down = -2` at `0x0065403e`, then
  `down_who = slot` at `0x00654043`; good type 5 writes neither field;
- `Objects::init_item` (`0x00653e00`): `WData.down = -3` at `0x00653f05`, then
  `down_who = slot` at `0x00653f0c`.

The only accepted checksum delta is `WData` when at least one occupancy write exists,
or no section when none exists. Rejection rolls back the row cursor and chance carry
as well as the RNG/World/pool/counter projection. An external host-reference side
effect cannot be rolled back here, so capture production must be observational or
two-phase.

## Why later-row schedule integration remains open

The concrete post-placement pool blocker is resolved: selector receipts now re-execute
the exact bitmask transaction, and the compiled schedule advances the first row while
committing the authoritative public pool. The next integration step is to retain the
typed `RemainingBonusRowsState` across repeated public schedule continuations while
preserving the explicit XML host-reference seam at each recurrence. Even after that,
the schedule must retain `checkpoint: None` and token `0x1ef7` as pending until
`GOODIES`, `FISH`, cleanup, return, and caller continuation are all owned.

`crates/don-replay/tests/place_resources_bonus_rows_mutation_frontier.rs` freezes twelve
paths: same-group budget reuse, group-zero redraw, transition-to-`-1` redraw, unknown
type carry preservation, negative-carry suppression, zero-chance winner reuse,
zero-`numrare`, admitted WData mutation, and full rollback on a rejected placement
receipt, plus rejection of a post-construction spliced row array, substituted later-row
facts, and mismatched first-row facts/receipt pairs. The corrected first-row proof adds
the matching zero-`numrare` case.
