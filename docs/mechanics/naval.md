# Naval recovery boundary

Status: **Tier C; static and structural recovery only.** No differential oracle has run this
module against retail, and none of it is wired into `World::tick` or replay reconstruction.

The implementation lives in `crates/don-sim/src/systems/naval.rs`. It now compiles in an
isolated harness, but public names deliberately distinguish measured fragments from convenience
proxies. Declaring the module is safe for compilation and further testing; treating it as a
complete naval simulation is not.

## Recovered and frozen

- The coordinate ladder (`192` world units/tile, `768`/WCoord, `48`/unit-grid cell) and XOR
  coordinate mask `0x63637`.
- Water/coast predicates and mutations recovered from the named retail functions.
- The first 289 `move_x`/`move_y` entries used by `Unit::think_fish`, read directly from
  `riseofnations.exe` at `0x00ADCAF0` and `0x00ADC400`. Their combined little-endian FNV-1a
  digest is `e72de577726887fc`. Retail entry 288 is the surprising `(-8, -16)`, and the port
  preserves it rather than generating an idealized `(-8, -7)`.
- Dock slot/region bookkeeping, the free `Dock::default()` sentinel, and the conditional gull
  RNG arm. `Dock::gull_angle_after_spawn` consumes no draw for a failed spawn and exactly one
  draw for a successful spawn.
- The shipped XML roster: 43 sea-domain rows, including **30** rows trained at a Dock. This is
  data coverage, not construction or training-queue behavior.
- Fish candidate filtering/scoring and its RNG placement, now over the full 289-entry retail
  scan. The post-loop `best_i > 0x120` fallback is unreachable because `best_i` is initialized
  to zero and assigned only loop indices `0..=0x120`; it should not be used to justify an extra
  RNG draw.

All statements remain Tier C until a retail oracle exercises the same shipped functions.

## APIs that are intentionally proxies

| API | What it does | What it does not do |
|---|---|---|
| `Docks::init_dock_registry_only` | assigns/reuses a dock slot and updates `reg_docks` | spawn the gull, consume its conditional RNG draw, add the strafe order, or write the building backlink |
| `Docks::close_dock_registry_only` | clears registry fields and decrements the region count | dispatch gull destruction or validate the live building object |
| `classify_board_step_proxy` | reproduces the three-way branch in `Unit::do_board` | mutate orders, call `go_inside`, or implement unloading/disembarkation |
| `find_wpath_entry_proxy` | reproduces the measured stack/bounds/one-cell early exits | coarse straight-line walk, search flags, retail A*, `calc_cost`, failure epilogue, or list cleanup |
| `water_route_proxy` | deterministic water-only route useful for tests | retail's unbalanced-tree A*, tie order, cost function, scratch state, or checksum behavior |

These names are a guardrail: integration code should not silently substitute a proxy for the
retail routine named in its documentation.

## Missing before faithful integration

1. Reuse the movement lane's retail-shaped `astar_path` and port the complete `find_wpath`
   wrapper, including its coarse pre-pass, pathfinder scratch writes, exact open-list ordering,
   `calc_cost`, and RNG-consuming failure epilogue.
2. Complete `Dock::init`/`close` around real object allocation, gull lifetime and orders, the
   building backlink, and exact failure behavior. The registry-only API cannot be called from a
   faithful construction path.
3. Port passenger containment, `go_inside`, unloading/disembarkation, rendezvous orders, and
   their object-list mutations.
4. Port Dock/Shipyard construction, production queues, upgrades and auto-formed transport and
   merchant units. The roster proves values only.
5. Read and port `World::compute_reg_territory` across ocean cells. Storage in `WData::who` is
   known; water-specific radius/falloff is not.
6. Recover naval supply semantics. The known `reg_docks`, `reg_naval` and `reg_transports`
   counters are indices, not a supply model.
7. Add retail differential cases for water predicates, dock transitions, gull RNG, fish scans,
   and pathing before promoting any claim above Tier C.

Until those land, module declaration means “available for isolated recovery tests,” not “ready
for `World::tick` or replay checksum claims.”
