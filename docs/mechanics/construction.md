# Construction lifecycle

`crates/don-sim/src/systems/construction.rs` is the authoritative seam between a citizen's
`BUILD_AT` order and the arithmetic already recovered in `systems/production.rs`.  It is
designed to replace Arena MODEL 2; it is not a relabelled builder-frame counter.

Status: **measured structure, Tier C behavior; runtime gate closed.**  The local mutations
are derived from the retail executable/PDB, but the large placement and lifecycle bodies
remain mandatory host transactions.  `construction::RUNTIME_FIDELITY_READY` stays `false`
until those bodies have implementations and retail-oracle coverage.

## 1. Measured call chain

| retail body | VA | construction responsibility |
|---|---:|---|
| `Unit::add_build_order` | `0x005E5210` | installs order 6 and copies target `(o, whom, uid)` |
| `Unit::do_build` | `0x005EEBF0` | validates target, drives position/facing gates, calculates rate, calls site |
| `Unit::check_build_order` | `0x00603470` | movement/order traversal and alternate-site selection |
| `Unit::build_done` | `0x00603BF0` | post-build movement and follow-on work |
| `Wall::do_construct` | `0x006434D0` | start/admission gate, helper divisor, counters, completion call |
| `Wall::process` | `0x00640450` | consumes/resets the per-frame helper accumulator |
| `Build::start` | `0x006273A0` | foundation transaction |
| `Build::activate` | `0x00623E20` | completion/ownership/city/world transaction |
| `Object::disband` | `0x006455C0` | rejected-site teardown |

The target snapshot is three fields, not an entity index: owner `who`, object slot `o`, and
the target's `ObjectData::uid`. `BuildOrderTarget` carries all three and pins the retail
order kind to 6. There is a measured asymmetry: `Unit::do_build` indexes `(who,o)` and calls
`is_valid_wall()` but does **not** compare the stored UID, while `check_build_order` does
compare it and prunes a recycled slot. The adapter must preserve that asymmetry rather than
globally treating UID mismatch as invalid before every build activation.

`install_build_order` ports the full unit-level installer. `QUEUE_NEW` clears raw unit mask
`0x04000000`, path/search state and the old queue. Every case sets raw `UNIT_BUILDER`
`0x400`; `QUEUE_LAST` appends, and `QUEUE_FIRST` produces the measured circular-list
rotation with the new order at the front. Group assignment is raw order flag `0x04`. When
builder and target terrain regions differ, the transport threshold is 3 for leader flag
`0x100`, 2 for `0x200`, otherwise `(leader_flags >> 10) & 1`; a builder whose
`transport_type() <= threshold` and `can_ever_transport()` sets raw unit mask `0x800000`.
The installer consumes zero RNG draws.

## 2. One builder activation

`execute_builder` asks the host for the measured `Unit::do_build` gate.  Its branches are:

1. Negative indices or missing/non-wall target: retire the current order through the
   mandatory `finish_builder` transaction. `check_build_order` scans only leading
   BUILD_AT/MOVE_TO orders, validates `(who,o,uid)`, and can install another candidate
   deterministically; the direct `do_build` entry does not perform that UID comparison.
2. Already-active target: retire and enter the measured `build_done`/reassignment tail.
3. Not adjacent, or covering a non-Farm footprint tile: kill the current order and use a
   one-member temporary group to `action_swarm_around(..., QUEUE_FIRST, BUILD_AT)`,
   preserving the old order's group flag. Credit nothing on this activation.
4. Raw unit mask bit 0 (`UNIT_DECOY`) remains set after Build/Sow animation and facing:
   keep the order and credit nothing.
5. Ready: enter the site lifecycle below.

The builder gate returns an `EffectReceipt` because reswarm, animation, facing and group
changes touch units/guys/groups and can have transitive RNG consequences. Those effects are
accounted even when the site receives no work.

An unstarted site calls the exact `blocked_site` evaluator.  The return partition used by
`Wall::do_construct` is:

- `0`, `0x27`, `0x28`, `0x29`, `0x2B`: call `Build::start(1)`;
- `0x2A`: start only when `BuildData::city >= 0` and the linked city's
  `num_wonders(1) <= 1 + LeaderData::has_tribe_bonus(7)`;
- all other values: call `Object::disband(1)`.

Starting does **not** consume the activation. `Build::start(1)` falls through directly into
the inactive progress body, so the first legal touch also bumps `recharging`/`helpers` and
credits work. A host may not replace `blocked_site` with its own `placement_ok`, nor return
zero as a universal default. `blocked_site`, `start_site`, and `disband_site` are required
callbacks. Lifecycle callbacks receive the checksummed `BuildData` record and must mutate
it; the core rejects a receipt whose `flags_after` disagrees with the record or lacks the
required STARTED/ACTIVE bit.

For an already-started, inactive site, the contribution is exact and ordered:

```text
rate = Constants.accel_construct
if Korean bonus 0x10 && korean_build_under_fire != 0: keep rate
else if build_masks & 0x20: rate /= 4
if ai_speed > 1: rate *= ai_speed

rate /= helpers + 1
if !(build_masks & 0x800):
    recharging += 1                 // signed i16 wrapping write at BuildData +0x7A
    build_masks |= 0x800
helpers += 1                        // u8 wrapping
rate = max(rate, 1)
job_counter_2 += rate               // u32 wrapping
job_counter   += rate
```

If `construct_time(0) <= job_counter`, retail calls `Build::activate(0,1,1)` *inside*
`Wall::do_construct`, returns 1, and only then does the unit retire its current order and
enter follow-on assignment. `Wall::activate` resets **both** `job_counter` and
`job_counter_2` to zero; the activation callback must apply those walked writes.
`execute_builder` preserves that order. The activation and finish callbacks both return
transitive RNG-draw counts and checksum stores. Farm completion is already known to consume
exactly 15 shared-stream draws: five animals, each drawing species, y offset, then x offset.
The zero species branch selects type `0x196` FARMCHICKEN; the nonzero masked branch selects
`0x195` FARMPIG. Other activation branches remain under audit and must report their own total.

There is no persistent site-side list or count of builders on this path. Assignment lives
in each citizen's order queue.  Only live citizens which reach `do_construct` during the
current object traversal contribute. Death/cancel therefore removes the order/unit; it
does not decrement a stored site membership count. Target death does not proactively visit
builders: their next `do_build`/`check_build_order` observes invalid flags or a stale UID
and retires lazily. Credited site work is never rewound by any of these paths.

Rejected unstarted sites enter gameplay `Object::disband(1)`, not a presentation-only
sink. It closes/dies the Build and loops all six resources, recomputes the full type cost,
and refunds each nonzero amount into the owner's XOR-`0x8221` resource totals. The Build
rejection arm consumes zero shared RNG draws (the generic disband RNG branch is unreachable
for this Build vtable), while leader/build/world checksum stores change. Text and sound are
the only cosmetic tail.

## 3. Frame boundary and multi-builder fidelity

`begin_site_frame` must run once per live building after the rotating unit bands, matching
retail's unit-band-before-fixed-building-band scheduler. It consumes the helper
contributions made earlier in that same object pass and performs:

```text
if helpers == 0: build_masks &= ~0x400
else: helpers = 0; build_masks |= 0x400
build_masks &= ~0x800
construct_hits = update_hits_progress(...)
```

The helper divisor is read before the increment, so equal rate `R` contributions in one
frame are `R/1, R/2, R/3, ...`, each floored to at least one.  Scheduler order is therefore
simulation state.  Do not gather contributors in a hash set, sort them by distance, or
replace the sequence with `R * builder_count`.

Construction HP is also not a linear full-precision ratio.  Retail prescales both progress
and duration with `>> 5` before dividing.  For 1,000 full HP at 500/1,000 progress, the
effective maximum is 483, not 500:

```text
1000 * max(500 >> 5, 1) / max(1000 >> 5, 1) = 483
```

That value, damage, construction flags, both job counters, `recharging`, build masks and
helpers all participate in the builds checksum channel.  Unit movement/facing/order-tail
effects can also change `units`, `guys`, `groups`, `leaders`, `cities`, `world`, and other
object stores.  Every required callback returns an explicit `ChecksumEffects` receipt.
The wrapper itself draws no RNG; callback receipts must report complete transitive draws.

## 4. Arena adapter contract

Arena can remove MODEL 2 only after its adapter provides all of the following:

1. **Persistent site state:** use a `production::BuildData` record (or a lossless owner of
   the same fields) for flags, `uid`, both job counters, cached construction time,
   construction HP, masks, helper accumulator, `recharging`, city link, and damage.
2. **Persistent target identity:** every `Job::Work`/build order retains `(who,o,uid)`.
   `EntId` alone is not a sufficient reuse token.
3. **Scheduler order:** call `execute_builder` for live builders in the rotating unit-band
   traversal, then `begin_site_frame` for sites in the fixed building bands.
4. **Authoritative builder gate:** implement target validation, `check_build_order`, and
   the turn-pending test.  The current "within one tile" test is not this gate.
5. **Authoritative placement:** call a complete port or coherent extractor-backed
   `BuildTypeData::blocked_site`; Arena's custom `placement_ok` is not a fidelity input.
6. **Lifecycle transactions:** implement `Build::start(1)`, `Build::activate(0,1,1)`,
   `Object::disband(1)`, and `build_done`/reassignment with all ownership, city, terrain,
   event, queue and checksum effects.
7. **Interruption:** builder death/cancel uses its unit close/order transaction; target
   destruction routes through the target's real close/disband transaction and is observed
   lazily by builders. The site retains credited work.
8. **Accounting:** return total transitive RNG draws and every checksum store dirtied by
   each callback.  Unknown counts are an adapter error, never zero-by-default.

Until these are satisfied, Arena should keep a loud fidelity blocker rather than adapting
`build_left -= 1` to the new result enum.

Arena's playable `ResearchModel` now exercises this boundary for the shipped Barracks/Tower
outside-city land cohort rather than duplicating its state machine. Before lifecycle entry
it snapshots the live 4x4 or 2x2 footprint in `blocked_site`'s x-outer/y-inner order,
retaining every TData terrain
mask, explored bit, WData/diplomacy territory class and overlapping building identity in an
`ArenaPlacementReceipt`. It then executes the recovered admission, `Wall::start`, progress,
`Build::activate`, and builder-finish ordering and stores an `ArenaConstructionReceipt`
containing the site, builder, BUILD_AT target, outcome, RNG count, and checksum-effect set.
An occupied ordinary-family footprint takes the recovered rejected-disband/refund loop and
invalidates the actual Arena target slot.

That is an executable integration result, not a fidelity promotion. Command-time
`placement_ok` is deliberately not an input to the new query: the focused test mutates a
real completed building into the footprint after command acceptance and the construction
call rejects the stale decision using that blocker's `(who,o,uid)`. Arena's generated map
is still a declared map model, and initial capital territory is settled through the
recovered border scorer rather than establishing whole-game border lifecycle fidelity.
`Wall::start_me`'s transient STARTED/STARTED2 tile pair is also live: the first unstarted
site owns STARTED and a second overlap raises STARTED2. For the ordinary `ean` cohort the
other site's exact `(who,o,uid)` produces blocker code 1, so the attempted site disbands and
refunds before the surviving claimant is admitted. Market/Temple's accepted `0x27/0x28` city-linked
arms are the first families that can reach `kill_competing_at` with a remaining site and
stay separate until their town transaction exists.
Missing reswarm/animation bodies remain compact projections and the activation host does
not implement the full retail graph. `FailClosedRetail` still stops before those inputs.
Market/Temple, gather buildings, cities, Wonders, captured sites, Farm animals,
alternate-site `build_done`, incremental border invalidation and cancellation/death remain
outside the integrated subdomain.

## 5. Open runtime blockers

- The remaining `BuildTypeData::blocked_location` `0x006375B0` and `blocked_tcoord`
  `0x00636DB0` type families: fort/city spacing, cliffs/water, adjacency, dock, gather and
  city-limit graphs. The shipped Barracks/Tower outside-city land terrain/occupancy/
  visibility/territory footprints are executable and identity-bearing; Market/Temple still
  require the linked-town and max-one-per-city arms.
- A claim-bearing Arena host for the ordered `construction_lifecycle` start, activation,
  rejection/refund, terrain, registry, city, leader, Farm-spawn and visibility calls. The
  live ResearchModel host covers a compact gameplay projection only.
- Wiring `construction_builder`'s exact plans and executable short-circuiting `build_done`
  driver into the real unit/order/group stores, including `check_build_order` movement and
  alternate-site mutation bodies.
- Retail oracle cases for start, rejection, 1–N builders, under-fire Korean/non-Korean,
  completion, cancellation, builder death, target death, and checksum/RNG deltas.

These blockers are why `RUNTIME_FIDELITY_READY` is false.  They are interfaces here, not
empty defaults, so incomplete work cannot silently ship as fidelity.

## 6. Oracle boundary

`ConstructionOracleSnapshot` and `verify_oracle_receipt` define the minimum coherent retail
capture accepted for promotion. A case must bracket the exact transaction with:

- shared `game_random` state before and after;
- isolated builds, units, guys, leaders, cities, groups, world, and other-object channel
  accumulators;
- the addressed site's full 220-byte `BuildData` image;
- builder `(who,o,uid)`, raw unit masks/angle/group and ordered BUILD_AT target payload;
- callback trace sufficient to distinguish blocked-site, start, activate, disband and
  builder-tail ordering.

The verifier advances the measured LCG exactly `receipt.rng_draws` times and requires the
captured post-seed to agree. It also requires the reported checksum-effect set to equal the
channels which changed. A UI-only video, a wall-clock completion time, the current negative
ordinary-building attempt, or a capture without the RNG seed/channel split is not an oracle.

The currently deployed retail hook cannot expose these fields or callback sequence, so no
Cycle 6 test is promoted above Tier C. The boundary is executable and tested so a future
hook can add cases without redefining what counts as evidence.
