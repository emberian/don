# Replay starting City / Build constructor continuation

This lane follows the ordinary first-Village path from the previous
`Object::add_to_world` stop through `Build::activate(0,0,0)`,
`Cities::init_city`, and the complete checksum-visible `City::init` body. Evidence is the
supported `ron-bin/riseofnations.exe`, its matching `rise.pdb`, and Capstone instruction
reads. No replay bytes, live heap values, or fitted outputs are accepted as constructor
inputs.

## Shipped call chain

| VA | shipped symbol | source-proven role |
|---|---|---|
| `0x0064d8c0` | `Object::add_to_world` | installs the Object down-chain and terrain-cell head |
| `0x00629740` | `Build::init` | returns after the Wall/Object chain and Build-specific registration |
| `0x0063e4b0` | `Wall::activate` | sets STARTED/ACTIVE, clears both construction counters, ORs mask `0x1000` |
| `0x00623e20` | `Build::activate` | selects the City branch on Object flag `0x20` |
| `0x007352c0` | `Cities::init_city` | first dead-slot reuse, otherwise `city_mark`/array growth; calls `City::init` |
| `0x00737050` | `City::init` | fills the City body, counters, caravan allocation and world-value footprint |
| `0x00735c90` | `City::generate_name` | advances shared-tribe city-name counters; no main `Random` call |
| `0x00735aa0` | `City::fix_world_vals` | shifts the checksum-visible `WData::val` disc |
| `0x007384c0` | `City::find_buildings` | rebuilds the City building chain |
| `0x00738aa0` | `City::regen_roads` | final City-side activation call |

The setup caller at `0x005ab910` first invokes virtual `Build::init` (`vtable +0x1a0`) and
then virtual `Build::activate(0,0,0)` (`+0x1a8`). After activation it increments
`LeaderData::cities_built`, calls `Object::update_seen(0)`, then
`Object::update_seen_ally`. The City slot is not created by `Setup` itself: the only direct
call to `Cities::init_city` in this chain is inside `Build::activate`.

Capstone fixes the four arguments at `0x00623fbd..0x00623fc7`:

```text
push first_city_capital
push activate_arg0          ; 0 for Setup::build_cities
push build_object_id
push build_owner
call Cities::init_city
```

`Cities::init_city` then pushes `(first_city_capital, activate_arg0, object_id, owner,
slot)` into `City::init`. This settles two easily confused fields: `activate_arg0 == 0`
selects `City::generate_name`, and the capital boolean adds both `0x10` and `0x4000` to
the initial `city_flags == 1`.

## Exact City body

For the ordinary fresh first Village, the post-activation record is:

- `city_flags = 0x4011`, `city = allocated slot`, `o = center Build object id`;
- `reg = WData(center).region`, not a caller-supplied region;
- `x/y =` the deobfuscated center Build position;
- five frame/timer stamps and `capture_strength` are zero, as are `traded_with[8]`;
- `conquest_node = -1`, enhancer/gather bytes zero, `pop = 1`;
- `land = 9`, `filled = 1`;
- `City::init` temporarily writes `race = 0`, `founder = -1`; the fresh-city suffix of
  `Build::activate` then writes the owner into both bytes;
- the caravan array is empty with capacity **10**, grow `-1`, flags `0`; its allocated 20
  dwords are all `-1` in retail, though an empty checksum walk emits length only;
- name/id are save state and are absent from the sync checksum.

The corresponding side-effect ledger is not optional. `City::init` adds the level-derived
population value (Village `1`, Town `3`, Metropolis/Forbidden City `5`) to
`LeaderData::pop`, `Game::world_pop`, `LeaderData::reg_pop[reg]`, and
`LeaderData::reg_cities[reg]`. It calls `Leader::calc_pop_cap` and clears the `borders`
cache in all 64 `Region` records. The activation suffix increments `city_num`, `city_mine`,
and `Game::world_cities`, then calls `Leader::calc_pop_cap` again. Setup separately
increments `cities_built`.

`apply_fresh_starting_village_projection` owns the City body, exact region read, common
Build activation writes, and world-value disc atomically. The receipt reports all remaining
Leader/Game/Region calls rather than silently mutating a second copy of those canonical
owners.

## `City::fix_world_vals` is part of the World checksum

The initializer converts City X/Y from `Coord` to `WCoord`, computes
`index = min((city_radius + 3) / 4, 61)`, and walks the engine `circle_init` tables through
`circle_radius[index + 3]`. Entries before `circle_radius[index]` receive
`WData::val >>= 2`; the annulus receives `val >>= 1`.

The adapter reuses DoN's instruction-derived `CircleTable::build`; it does not embed the
retail arrays. A normal non-Indian Village has radius 20, index 5, **105** inner entries and
**237** total entries (132 in the outer annulus). These writes affect section 5 of the
World checksum and therefore cannot be omitted merely because City itself is checksum
channel 9.

## Name state and RNG

`City::generate_name` contains no call to the main `Random` object. It does mutate
`LeaderData::city_name` and later names use a private arithmetic LFSR seeded from
`tribe + World::seed`:

```text
if candidate & 1: candidate ^= 0x170
candidate >>= 1
```

For the first city in a tribe cohort, the one-based XML ordinal is the number of earlier
Leader slots using that tribe plus one. Later calls advance the greatest existing counter
among same-tribe Leaders and choose the requested accepted LFSR value beyond the cohort's
reserved first-name prefix. `plan_city_name` ports that state transition and explicitly
reports `main_rng_draws = 0`; XML string resolution stays with the shipped-content owner.

## Starting-town 2 follow-on

After the center returns, starting-town 2 calls `Setup::small_city_buildings`
`0x005aae10` and then `Setup::build_civ_specific` `0x005ab760`. The base call order is:

1. Woodcutter (`418`);
2. three Farms (`417`) unless the relevant tribe-bonus branch changes/suppresses the count;
3. Library (`435`).

The civ-specific continuation may then append Market (`436`), University (`420`), Temple
(`437`), first/second Smelter rows (`423`/`424`), and Capitol (`438`) in that exact order.
Both exposed planners produce the `Leader::free_build(type, center_object_id, 0)` schedule.
Those extra Build initializers and their `BuildData::city_down` joins can change City flags
and remain separate typed transactions; this lane does not collapse them into the center.

## Exact residual

This is deliberately not yet a Builds-channel producer. The 13,088-byte
`Build::activate` body and the remainder of `Build::init` contain BuildType virtual results,
hit/LOS/stance values, world/list mutations, visibility bytes and subtype counters not
owned by the City record. The projection writes only the common proven fields (STARTED,
ACTIVE, construction counters, activation mask and City link), so its receipt states:

- `city_init_complete = true`;
- `build_init_complete = false`;
- `build_activation_complete = false`;
- `builds_channel_ready = false`.

Promotion requires an independently sourced post-activate Build body sufficient for the
131-byte empty Builds walk, plus execution of every side effect in the returned ledger.
