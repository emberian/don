# Replay starting-City territory projection

Lane: `checksum-cities`. Source base: shipped PE32/PDB plus the canonical replay setup and
procedural-map owners. No recorded checksum is an input.

## Result

`crates/don-replay/src/starting_city_territory.rs` now binds the canonical starting Villages
to retail's synchronous `World::compute_all_territory` call at `0x006B5700`. It stages the
targeted World, Regions, and Cities after-images atomically and retains the unreconciled
Leader/Region effects in its receipt. The full projection is diagnostic while
`TerrainGroups::place_all` and the final World owner remain incomplete.

One narrower result is final-map independent and is exposed as
`StartingCityBorderingAuthority`: all six Cities in the three admitted replays have
`CityData::bordering +0x65 == 0` at the first checksum.

## Native writer

The setup schedule calls the full territory pass after every starting empire and before the
first `Leader::plan_strategy`. `World::compute_all_territory` resets every nonempty land
Region cursor and calls `World::compute_reg_territory(region, 1)`. The reached body:

- clears every live City `bordering` byte when a Region cursor starts at zero
  (`0x006B0C40`);
- scores every City/Fort for each Region coordinate, writes `WData::who/who2`, and retains
  the winning City-list index;
- at `0x006B17C8..0x006B17EC`, sets the winner and runner-up player bits in that City's
  `bordering` byte only when both are non-negative, the runner-up differs from the winner's
  team owner, and either reached diplomacy declaration is literal zero;
- increments per-Leader/per-Region territory counters and ORs `0x02000000` into each active
  Leader after a completed Region; and
- finally forces `who/who2 = -1` for every coordinate in sea Regions 64 through 127.

The declaration test is not interchangeable with `WorldData::is_enemy_territory`: the
later query tests mutual Ally value 2, whereas the bordering writer compares with zero.
`don-sim::systems::borders_fog::city_border_is_contested` preserves the native predicate.

## Why `bordering == 0` does not need the final map

The fresh setup contains exactly one capital Village and no Fort per active owner. Replay
Player rows supply the two owner/nation identities; starting technology zero supplies the
unupgraded age/government inputs; both World-resident territory-limit triples are retained
from the exact constructor. For each fixture the adapter evaluates the native scorer at
every WCoord in the entire rectangular map under **both** possible Region limit triples.

Region membership can only remove coordinates from that superset. No coordinate admits a
winning City plus a non-negative hostile runner-up. Therefore no possible completion of
the still-provisional Region/WData content can reach the bordering set arm. Since the fresh
constructor byte is zero and the territory pass either clears it or leaves it untouched,
zero is the source-exact final value.

The exhaustive proof covers 10,000 cells in the 2018 map, 4,900 in the 2020-02-08 map, and
3,600 in the 2020-02-21 map. It observes zero potential contested cells in all three.

## Current diagnostics

Running `cargo test -p don-replay --test starting_city_territory -- --nocapture` against the
local retail corpus yields:

| replay | territory-only Cities | territory + census | retail | active territory totals |
|---|---:|---:|---:|---:|
| 2018-11-17 | `71020a46` | `34a10d02` | `53130d2c` | 388 / 388 |
| 2020-02-08 | `247c0992` | `e80c0c4e` | `dd170c24` | 380 / 388 |
| 2020-02-21 | `cd970956` | `91360c12` | `7d2d0bd4` | 388 / 388 |

Across 3 fixtures / 6 Cities, territory creates zero bordering writes and the subsequent
City census rejects zero cells as foreign-owned. Consequently the three City checksum
candidates do not move and first-checkpoint survival remains 0. This is a useful negative
result: neither final `who/who2` nor `bordering` can explain the remaining City mismatch for
these starts.

The provisional input Worlds still report 305,813 / 151,699 / 110,765 unsourced walked
bytes. The complete territory projection therefore stays unmounted. Its remaining
integration dependencies are the final post-`place_all` World/Regions receipt and canonical
publication of the returned per-Region territory counts plus the active-Leader
`0x02000000` flag effect. The standalone zero `bordering` authorities do not waive either
dependency for World or Leaders, and do not by themselves complete the Cities channel.
