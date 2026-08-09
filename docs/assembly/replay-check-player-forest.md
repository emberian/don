# Replay `Map::check_player_forest` boundary

## Result

The narrowest World-mutating continuation after a successful
`TerrainGroups::place_all` is `Map::check_player_forest` at `0x0068e8f0`.
`crates/don-replay/src/check_player_forest.rs` reconstructs that deterministic
call transactionally and returns the unchanged RNG state at the entry to
`TerrainGroups::nubify_forest` (`0x006a93b0`).

This closes one exact dynamic-World producer. It does **not** claim that the
current corpus can cross the upstream live-fact boundary, that `nubify_forest`
or later map construction is reconstructed, or that channel 12 agrees with a
retail turn.

## Native schedule and evidence

The ordinary `Map::make` path at `0x0068bc90` has this order:

| address | operation |
|---:|---|
| `0x0068bffa..0x0068c001` | test `Game::semaphore` byte `+0x821`, bit `0x02` |
| `0x0068c010` | call `TerrainGroups::place_all` at `0x006a70d0` |
| `0x0068c015` | resume after `place_all` |
| `0x0068c039` | checksum-log instrumentation, source token `0x1eb3` |
| `0x0068c04d` | call `Map::check_player_forest` at `0x0068e8f0` |
| `0x0068c052` | resume after the call |
| `0x0068c076` | checksum-log instrumentation, source token `0x1eb5` |
| `0x0068c08b` | call RNG-consuming `TerrainGroups::nubify_forest` at `0x006a93b0` |

The complete helper occupies the half-open range
`0x0068e8f0..0x0068ef00`; its final `ret` is at `0x0068eeff`. The primary
instruction-derived sources are:

- `re/decomp-all/0068bc90.c`, the caller and checksum checkpoints;
- `re/decomp-all/0068e8f0.c`, the complete helper;
- PDB `map.cpp` lines 6856–6953 and `Constants::city_center_radius +0x12c`;
- circle globals `circle_x 0x00cb7e90`, `circle_y 0x00cbb0e0`, and cumulative
  `circle_radius 0x00cbe330`; and
- cardinal globals `dx 0x00add254 = [0,1,0,-1]` and
  `dy 0x00add214 = [-1,0,1,0]`.

The shipped radius value is independently triangulated by
`don-rules::Rules::city_center_radius()` (`raw[75]`, byte offset 300) and
`don-sim::systems::tech_cities::CityRules::RETAIL` as 20. The replay adapter
admits that value only when the replay-carried Constants projection reached
the independently reproduced `RETAIL_AFTER_CONSTANTS` checkpoint.

## Exact algorithm

Retail calculates `ring = city_center_radius / 4 - 1` with signed truncation
toward zero. For shipped radius 20, `ring == 4`.

For every `start_x` row, in array order:

1. Count existing `FOREST (0x20)` cells over canonical-circle indices
   `[circle_radius[0], circle_radius[ring])`. The origin is excluded because
   `circle_radius[0] == 1`. Shipped range: `1..69`.
2. If the count is at least three, leave that start untouched.
3. Otherwise try patch sizes 3, then 2, then 1. Candidate anchors are scanned
   over `[circle_radius[2], circle_radius[ring])`; shipped range: `21..69`.
4. Admit an anchor or cardinal neighbour only when it is in bounds, flat,
   outside `start_city_locs`, and non-coast. `WATERHALF` makes a water-land
   value non-ocean and is deliberately admissible.
5. Append the anchor, then attempt neighbours N, E, S, W. Retail compares the
   accumulated length with the requested size **after every neighbour
   attempt**, including an inadmissible one. It does not test immediately
   after the anchor and does not select a subset after all four attempts.
6. On the first exact length, apply the native `set_land` write to every
   accumulated coordinate and stop processing that start.

The persistent update is:

```text
land     = old_land, except a coast would use 2
land_sub = old_land_sub
flags   &= 0xffc3
flags   |= 0x20
```

Admissibility already excludes coast, so selected cells preserve land and
subtype. Other flag bits, including `WATERHALF` and `ORIG_COAST`, survive.
`forest_size`, TData, region lists, start arrays, and every other checksum
section remain untouched.

## RNG and temporary containers

The body has no call to the main RNG at `0x00a39d70`; the place-all receipt's
`random_state_after` is returned unchanged and `random_draws == 0`.

Retail uses two parallel 28-byte `Array<WCoord>` scratch objects. Each has
length/capacity/list at `+4/+8/+16`, `increment == -1`, and flags zero. Lengths
are cleared between anchors while capacity is retained; append order is
anchor, N, E, S, W, and growth is the native doubling policy beginning at
four. The adapter records the logical native capacity independently of Rust
`Vec` allocation. Those scratch allocations are freed before return and never
enter a checksum.

## Typed transaction and checksum scope

`execute_check_player_forest` requires:

- the plan still stopped at the exact `place_all` entry boundary;
- an admitted shipped Rules/Constants checkpoint and radius;
- proof that the caller's semaphore bit 9 was clear;
- a successful place-all receipt with matching entry/return, map style, start
  count, TData length, stored checksum, sourced-byte count, and return value;
  and
- a self-consistent World shape, parallel start arrays, and exact start-city
  mask length.

Mutation occurs in a staged World. A validation error leaves World, its stored
checksum, sourced-byte accounting, and the RNG handoff unchanged. A successful
receipt exposes ordered per-start outcomes, exact circle ranges, patches,
before/after checksums, zero draws, and `next_va = 0x006a93b0`. The adapter
rejects any checksum movement outside `WorldSection::WData`.

The sourced byte count is deliberately unchanged. `city_center_radius` is
static shipped content admitted through replay-carried checksum evidence, and
the generated World was produced upstream; this helper does not discover new
serialized replay bytes. No coverage report or survival counter should be
increased merely because this transaction exists.

## Integration handoff

The module and tests are isolated in new files. The shared convergence owner
must add the module to `crates/don-replay/src/lib.rs`, and may re-export the
following typed surface:

```rust
pub mod check_player_forest;

pub use check_player_forest::{
    execute_check_player_forest, CheckPlayerForestError, CheckPlayerForestFacts,
    CheckPlayerForestReceipt, ForestPatch, PlayerForestResult,
    MAP_CHECK_PLAYER_FOREST_VA, TERRAIN_GROUPS_NUBIFY_FOREST_VA,
};
```

After a successful `execute_replay_place_all`, pass that receipt and facts from
`CheckPlayerForestFacts::from_admitted_replay_rules(plan, false)` into
`execute_check_player_forest`. Its receipt is the typed RNG/World handoff for a
future `nubify_forest` reconstruction.

The focused real-corpus proof command, after the shared module declaration is
wired, is:

```text
cargo test -p don-replay --test check_player_forest \
  retail_replay_proves_the_static_radius_and_a_nonempty_world_deadline_only \
  -- --exact --nocapture
```

That test proves only that the supported retail recording independently admits
the shipped Constants fact and carries a non-empty recorded World checksum. It
also asserts that the unmodified corpus plan still stops at installed map-style
content. Synthetic mutation tests pin the helper's exact behavior, but neither
test is presented as retail checksum agreement.

## Files

```text
crates/don-replay/src/check_player_forest.rs
crates/don-replay/tests/check_player_forest.rs
docs/assembly/replay-check-player-forest.md
```
