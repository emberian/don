# wire-tick — the tick now runs

**What runs that did not before.** `Game::do_frame` is executable. `crates/don-sim/src/tick.rs`
drives the declared 29-call schedule in retail's order and dispatches into eleven of the
reviewed `systems/*` modules from the step each one belongs to. Before this lane, the only
thing in the repository that visited `schedule::DO_FRAME` was `World::step`, which incremented
29 counters, ran the object traversal, and advanced two clocks. **Nine of the 29 steps now
execute derived code with real inputs on every tick — 9 of the 14 that are in scope, since 15
of the 29 are presentation, telemetry or session management.** A binary prints the trace.

```sh
cargo run -p don-sim --bin don-tick-trace -- --frames 900 --trace 3
```

```
       step:   01234567890123456789012345678
  f0      ----o---X--XX.XX-X-XX-.Xo--o-  9/29 executed
  ...                                            X executed  o ported-but-vacuous
  f899    ----o---X--XX.XX-X-XX-.Xo--o-  9/29 executed        . unimplemented  - out of scope
```

Nothing here is new derivation. Every system called carries whatever tier its own module
claims; wiring adds execution, not fidelity, and this document never treats one as evidence
for the other.

---

## 1. The number, and what it is a denominator of

| measure | before | now |
|---|---:|---:|
| steps of `DO_FRAME` executing derived code, per tick | 1 | **9** |
| of the 14 in-scope steps | 1 / 14 | **9 / 14** |
| steps that only increment a counter | 28 | 0 |
| `systems/*` modules reached from the tick | 0 | **11** |
| named retail sub-calls skipped, counted per tick | not counted | **17** |

The three-way split is deliberate and the categories are enforced by a test:

* **Executed** — ported code ran *and had inputs*.
* **Vacuous** — ported code ran over an empty population. Never counted as executed;
  `an_empty_world_executes_only_the_counters` asserts an empty world reaches at most 2.
* **Unimplemented** — no port. The retail function that belongs there is named in `Gap`,
  and every skip is counted, so "step 12 executed" cannot hide "two of its seven children
  do not exist".
* **OutOfScope** — correctly absent from a headless deterministic core.

Steps 13 and 22 remain unimplemented retail code: `Armies::process_all` and
`Roads::scan_and_kill_stray_roads`. Steps 17 and 19 now execute the complete recovered
`Leaders::end_process_all` and `Leader::process_event_frame` dispatchers and deterministic bodies. Steps
4, 24 and 27 are runnable but correctly report vacuous until a script, cannon-time window,
or resolved-victory latch supplies work.

## 2. What each executing step actually calls

| # | step | ported calls now driven | named gaps inside it |
|--:|---|---|---|
| 8 | `Leaders::process_all` `0x006ED2A0` | recovered whole dispatcher; hostile scan; `leader_gather`; rare-mask dirty protocol; wall/unit stat-band traversals; resolved `is_active`/`is_captain`, base Object virtuals, full building `Wall::update_hits/update_los`, full `Unit::update_speed`, `ObjectData::armor`, and `Unit::update_armor`; automatic Unit speed/armor packages from shipped type plus live leader/object state; `process_elimination`; grace timers; taunt dispatch | `Wall::update_construct_time`; automatic Wall and Unit base hit/LOS query population; reached `Object::eject_contents`; `process_taunt` AI-chat body |
| 11 | `Leaders::strategy_all` `0x006ED430` | complete 93-byte dispatcher; exact `(flags&3)==3` slot gate and call order; full `check_explore` phase/recount body; `victory_score::compute_score`; semaphore-gated `check_victory`, including the zero-active-leader tail | `plan_strategy`, `diplomacy` AI bodies |
| 12 | `GameDaemon::process_all` `0x00732700` | `victory_score::process_victory`; `map_terrain::World::clear_seen` + `borders_fog::update_seen` per object; `economy::calc_markets` (**on the sim RNG stream**); `borders_fog::check_borders`; `groups_guys::Groups::process` | `calc_danger`, `process_coll_blocks` |
| 14 | `Objects::process_all` `0x0065DCE0` | the `(frame+i)%10` rotation; `Unit::work`→`do_job` arms 0/1/4/5/6/10; `movement::move_step`; `mechanics::damage` + `combat::recharge_frames`; `production::do_construct`; `walls::WallState::process`; `casters_animals::process_herd` at `frame%64` | `Guy::process`, `suffer_attrition`, `process_supply`, `detect_unit_collision`, `needs_transport`, wildlife spawn, anti-air dud roll |
| 15 | `Objects::inc_time` `0x0065DB70` | `ammo::ammo_inc_time` over the pool in slot order, `hit_target`/`check_hit`, `ammo_do_damage_single` | `Unit::inc_time` (the other half) |
| 17 | `Leaders::end_process_all` `0x006ED070` | complete eight-slot dispatcher; matching Player warning-bit cleanup; exact 450-frame feedback limiter and stamp-before-cap compare | localized message/audio are emitted as inspectable presentation events |
| 19 | `Leader::process_event_frame` `0x006EC180` | complete eight-slot dispatcher; exact 50-frame unsigned-rate smoothing; sequential hostile-score combat-mood selection; lopsided-battle threshold, cooldown and sentinel writes | JukeBox mood requests and achievement notifications are emitted as inspectable presentation events |
| 20 | `Game::frame++` `0x005924BF` | the counter, after the object pass | — |
| 23 | `frame % 15 → seconds++` | the counter | — |

Twelve modules are now reached from the tick: `ammo`, `borders_fog`, `casters_animals`,
`combat`, `economy`, `groups_guys`, `leaders`, `map_terrain`, `movement`, `production`,
`victory_score`, `walls`, plus crate-level `mechanics`, `objects`, `order`, `rng`, `trig`,
`balance` and the new
shared `checksum`. Still isolated: `air`, `items`, `naval`, `tech_cities`.

## 3. The ordering facts the driver is obliged to honour, and how each is enforced

* **`(frame + i) % 10` owner rotation.** Step 14 walks `ObjectRegistry::traversal_into`; the
  unit band rotates, the building and wall bands are fixed 0..8, buildings before walls.
* **`Game::frame++` is step 20, after the object pass.** The trace prints the *pre-increment*
  frame and `the_object_pass_uses_the_pre_increment_frame` asserts tick *n* traces frame *n*.
* **`Ammo` runs at step 15, after every unit, building and wall.** Projectile flight, impact
  and the resulting deaths land at the end of the tick, in pool-slot order — the flight pass
  collects `DamageCall`s and `Object::do_damage` is applied after it, exactly as
  `ammo_do_damage_single` issues them. Measured below: a projectile fired during step 14
  kills at step 15 of a later tick, never inside the object pass.
* **`Groups::process` is the tail of step 12**, before anything moves — not a pass of its own.
* **`helpers` is a single-frame accumulator.** `production::construct_frame`'s own doc says
  the worker order "is owned by the scheduler, not this module". It is owned here now:
  `Unit::do_build` runs from inside the traversal, so the second builder in a frame divides
  by 2. `two_builders_in_one_frame_divide_the_rate` asserts `rate + rate/2`, then asserts the
  divisor resets the next frame because `Build::process` cleared `helpers`.
* **One RNG stream.** `ammo.rs` carries its own `Rng`, bit-identical to `crate::rng::Random`.
  Every ammo call bridges the state in and back out (`Random::state` → `Rng` → `reseed`) so
  the scatter and ground-jitter draws stay on `game_random` instead of forking it.

## 4. Measured

`don-tick-trace`, 900 frames, 40 units / 4 buildings / 8 walls / 2 herds / 4 players,
seed `0x5EED`, real `schema/live/balance-real.bin` loaded. Counts are call-site counters
inside the driver, not estimates.

| | |
|---|---:|
| steps executed, every tick | **9 / 29** (9 of 14 in scope) |
| `Unit::process` | 36,000 |
| `Unit::move_step` (ported integrator) | 13,544 |
| `Unit::do_attack` | 454 |
| damage applications (all via projectile impact) | 282 |
| `Build::process` | 3,600 |
| `Wall::do_construct` steps | 3,624 |
| buildings completed | 4 |
| `Wall::process` | 7,200 |
| `Ammo::inc_time` | 57,147 |
| ammo impacts | 417 |
| fog cells newly explored | 814 |
| territory tiles claimed | 4,096 |
| `Groups::process` passes | 900 |
| `Leader::gather` calls | 3,600 |
| market cycles | 900 |
| pathfinder searches / failures | 24 / 0 |
| `Herd::process` steps | 6 |

At 1,500 frames the same scenario reaches 481 attacks, 310 damage applications and **6 deaths
filed in the ring** — the end-to-end path `do_attack` → `fire_ammo` → `Ammo::inc_time` →
`do_damage` → `DeathRing::add_death`, with the kill landing at step 15.

**Counted RNG divergence points**, because they are the reason none of this is
stream-faithful yet: over 900 frames the driver skipped **458 anti-air dud rolls**
(`Ammo::init` draws 1–2 each) and **29 wildlife spawns** (`frame % 32`, unknown draw count).
Each is a point where our stream leaves retail's. Drawing the wrong number would be worse
than drawing none, so the driver draws none and reports the count.

Tests: 9 in `tick::tests`, all green.

* `stepping_is_deterministic` — two sims from one seed agree on the trace, the per-step work
  and the channel digest for 40 frames.
* `parallel_sims_reproduce_serial` — 8 sims × 25 frames at 1, 2 and 4 threads produce the
  digests serial stepping produces. This mirrors the guarantee `Batch` gives for
  `World::step`; **`World::step` and its existing `parallel_matches_serial_regardless_of_thread_count`
  are untouched and still pass.**

## 5. Honest boundaries

* **Two tick entry points now exist in the crate.** `World::step` is unchanged — it is what
  `Batch` drives and what the SIMD/layout benchmarks measure — and `Sim::do_frame` is the
  wired one. That is a real duplication and it should collapse onto `Sim` once `Batch` can
  carry the extra state. It was not collapsed here because `world.rs` is shared with seven
  live lanes; this lane made **zero** edits to it.
* **Fidelity is unchanged.** Nothing here is comparable to a retail checksum.
  `Sim::channel_digest` mixes the channels that have runtime producers and says in its own
  doc comment that it is not a `check_all`; note also that `CheckSums::check_all` returns the
  *fifteenth* channel, not the sum (COVERAGE.md §1.1).
* **Stand-ins, each a named `Gap` rather than a silent guess.** `terrain_z` is flat zero, so
  arcs never clip into a hillside early. `unit_collides` answers "never" because
  `Unit::detect_unit_collision` `0x00617060` is unported — which also weakens
  `PathFinder::valid_ucoord`, since collision is part of A\* validity. `needs_transport`
  answers 0. `turn_rate` is passed as `i32::MAX` because the real arm reads `Unit+0xA1/+0x8C/
  +0xA2` through `0x005DE340` and is unmodelled, so the movement arm reduces to its
  translation half.
* **The scenario in the binary is setup, not derivation.** Placement, LOS values, who builds
  what and who shoots whom are chosen so every wired subsystem has inputs. Only the balance
  matrix and the rule constants come from shipped data.
* **`Sim` has no despawn path**, matching `World`: a dead unit has its active bit cleared and
  a corpse filed, but `Objects::kill_object` is not ported, so rows never move under the
  band indices.

## 6. Corrections to existing artifacts

* **`ammo::ammo_inc_time`'s hit-test hook cannot call the ported `ammo::hit_target`.** The
  parameter is `impl Fn(&AmmoWalk) -> bool` while `hit_target` takes `&mut AmmoWalk` and
  mutates on failure — it clears `whom`/`ox`, and both fields are checksummed. The driver
  therefore runs the probe on a copy and lets `ammo_do_damage_single` re-run it properly;
  the hook should be `FnMut(&mut AmmoWalk)` so the arrival branch matches the retail order.
* **`schedule.rs`'s own status column is now measurable and disagrees with itself.** It marks
  step 14 `Implemented` and steps 8/11/12/15 `Stub`, but all five execute derived code; step
  21 `OrdersMemManager::cycle` is marked out of scope while the new `order_dispatch` module
  has a recycling order queue. The status field should be regenerated from a run of
  `Sim::coverage_report` rather than hand-maintained.

## 7. The next move, precisely

A sibling lane landed `systems/order_dispatch.rs` during this wave — a real `Unit::work`
`0x0060D180` port with the `(frame + o) % 32 / 16 / 64` phasing, `update_order`, `repath`,
`check_target_path`, `kill_current_order` and a `WorkWorld` host trait. It is strictly better
than the five `do_job` arms in `Sim::unit_work` and it is what step 14 should dispatch into.
Wiring it needs exactly three things:

1. a `UnitWork` record per unit row, kept in sync with the generated columns;
2. a `WorkWorld` impl over `Sim` — `frame`, `target`, `attack`, `gather`,
   `draw_path_retry_delay`;
3. a decision on `draw_path_retry_delay`, which **must** consume
   `Random::get(0, 0xFFFF) % 3 + 6` from `game_random` or every later draw in the tick
   desyncs.

It was not wired here because that module's own suite was still red while this file was
written (3 failures at hand-off, all inside `order_dispatch`, none in `tick`). After that,
the ranked order is: `target.rs` (`Unit::fight`) into the attack arm so target *selection*
exists; `Unit::inc_time` to complete step 15; then the wildlife spawn and the anti-air dud
roll, because until those two draw, no run of this tick can be stream-compared with retail.

## 8. Files

| path | what |
|---|---|
| `/Users/ember/dev/don/crates/don-sim/src/tick.rs` | the executable `Game::do_frame`, `Sim`, `TickTrace`, `Gap`, `Coverage` (2,087 lines, 9 tests) |
| `/Users/ember/dev/don/crates/don-sim/src/bin/tick-trace.rs` | `don-tick-trace`: populate, run N ticks, print the trace and the coverage report |
| `/Users/ember/dev/don/crates/don-sim/src/lib.rs` | **shared, +1 line**: `pub mod tick;` |
| `/Users/ember/dev/don/crates/don-sim/Cargo.toml` | **shared, +4 lines**: the `don-tick-trace` `[[bin]]` |

No other file was touched. `world.rs`, `schedule.rs`, `objects.rs` and every `systems/*.rs`
are unmodified by this lane.
