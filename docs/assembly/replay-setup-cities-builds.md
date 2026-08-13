# Replay starting Cities and center Builds

This lane reconstructs the ordinary first Village for the five two-player, human-only
checksum replays. It is a source-derived constructor owner, not a copied replay checksum.
The supported PE, matching PDB, replay command bytes, and shipped map-style branches are
the accepted evidence inputs. No live process, checksum fitting, or VM observation was
used.

## Assignment and position

The `InitialGame` bytes are serialized before `Setup::build_game`. In all five target
recordings, `start_list[8]`, `start_index[8]`, and `num_players` are still zero. They are
retained by the parser as negative evidence and are never treated as player assignments.

`Game::zoom_to_first_unit` (`0x0058DB30`) selects object 2000 when the first Unit exists,
reads that Build's XOR-decoded X/Y, and calls `Camera::set_loc` (`0x00844A60`). The first
command group carries one `CameraCommand` (`0x48`) per `Player::play`. The producer joins
that play id to the unique active `Player::who` owner and admits a coordinate only when it
is in bounds and is congruent to the Village snap-center offset (`96 mod 768`) on both
axes. Duplicate owners, plays, cameras, teams, observers, AI players, scenarios, and
malformed positions all refuse the transaction.

The five observed pairs are useful corpus coverage, not constants in the implementation:

| replay date | style | play 0 camera | play 1 camera | admission |
|---|---:|---:|---:|---|
| 2018-11-17 | 9 Himalayas | `(56160, 13152)` | `(21600, 64608)` | fresh constructor |
| 2020-02-08 | 6 Old World | `(15456, 10080)` | `(37728, 46176)` | fresh constructor |
| 2020-02-21 | 9 Himalayas | `(10080, 36192)` | `(36960, 10848)` | fresh constructor |
| 2024-02-23 | 14 Great Lakes | `(74592, 10080)` | `(1632, 67680)` | unresolved region |
| 2024-03-29 | 14 Great Lakes | `(37728, 43872)` | `(7776, 3168)` | starting-town 2 |

Old World and Himalayas take the source-proven all-land continent branch and the common
`Regions::find_all` pass gives its sole land component region 1. Great Lakes is refused
until its start-cell region is independently produced. Starting-town 2 is refused because
its Woodcutter/Farm/Library and nation-specific follow-ons are a larger Build/City-chain
transaction.

## Atomic constructor transaction

For each owner, `spawn_canonical_build` allocates the next retail Build id, stamps owner,
ptype, XOR position, and both dense/sparse object registries. The staged center begins with
`VALID|CITY`, ordinary `-1` link sentinels, and current/original ptype Village.
`apply_fresh_starting_village_projection` then performs the source-proven activation and
City constructor writes, reads the region from WData at the actual center, applies
`City::fix_world_vals`, and returns the exact fresh City body. The producer links that body
to the 160-slot `CityPool`, the Build's City slot, its canonical registry row, and the
current ptype table. `check_sim_cities` validates all joins and walks exactly 114 bytes per
empty-caravan City.

All fallible work targets local staged owners. A refusal publishes no partial CityPool,
Build row, registry append, or channel.

## Why the constructor checksum is not first-checkpoint-correct

The fresh City body is not the first replay-checksum body. `Game::do_frame`
(`0x00591EF0`) calls `Leaders::strategy_all` (`0x006ED430`), which calls
`Leader::plan_strategy` (`0x006B9620`). On frame zero that function clears and recomputes
walked City bytes `+0x50`, `+0x5a..+0x5c`, and `+0x62..+0x71`: the first four come from its
starting-Unit census, while ocean, land, filled, dock/space, and six gathered-resource bytes
come from surrounding WData. The first recorded Cities values occur on turn 2 and remain
stable until later gameplay changes them. Comparing the fresh constructor walk to those
values therefore correctly falsifies promotion; it does not falsify `City::init`.

The Build is also incomplete: inherited `SubObject/Object/Wall/Build` initialization,
WData intrusive links, queue allocation, subtype virtuals, and the full activation body
remain outside this owner. The receipt consequently exposes the exact constructor-time
Cities walk but declares both `first_checksum_city_image_ready` and
`builds_channel_ready` false.

State integration still treats the setup-owned Cities/Builds *pair* as inseparable and
installs neither pair value unless both are complete. The later checksum-cities tranche now
separately publishes the canonical `Sim::cities` traversal as a conditional exact producer.
That producer makes the frozen constructor image observable to the replay scoreboard; it does
not set `first_checksum_city_image_ready`, install Builds, or assert a retail match. See
[`replay-cities-sim-channel.md`](replay-cities-sim-channel.md).
The bounded exact Unit-census transaction is documented in
[`replay-starting-city-unit-census.md`](replay-starting-city-unit-census.md); it remains
unmounted until this setup host materializes its receipt-backed canonical Units.

## Verification

`crates/don-replay/tests/setup_cities_builds.rs` covers the three admitted all-land
recordings, verifies canonical owner/play/center/region/ptype/City joins and the two-city
228-byte constructor walk, asserts that Builds remains uninstalled, and verifies that the
independent Sim-owned Cities producer is exact, fully sourced, and frozen across harness steps.
It also covers fail-closed Great Lakes region, starting-town 2, and corrupt-camera cases.
The local corpus gate passes 2/2 active tests; Persvati clean-HEAD isolated overlay job
`replay-setup-cities-builds2-20260811T181107Z-89456-9395-ee618ca8197f` passes 2/2 source
unit tests. The replay corpus is intentionally not copied to the remote host.
