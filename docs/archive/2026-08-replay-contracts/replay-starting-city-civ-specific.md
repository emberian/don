# Starting City civ-specific continuation

`Setup::build_cities` `0x005ab910` calls `Setup::build_civ_specific`
`0x005ab760` after every successful starting center. This is unconditional with respect to
the starting-town selector: mode 2 first calls `small_city_buildings`, while every other mode
skips that helper, and both control-flow arms then call `build_civ_specific`.

The standalone authority is
`crates/don-replay/src/starting_city_civ_specific.rs`. It proves only the exact empty-call
cohort. If retail would call `Leader::free_build`, it refuses rather than inventing the later
Build initializer or its checksum-visible City link.

## Source-owned query image

For each canonical center in `StartingSetupState`, the adapter joins:

- the setup receipt's replay payload digest, owner, City slot, and object 2000 identity;
- the replay Player row's selected Tribe;
- the admitted serialized Rules digest and selected `Tribe +0x54` dword;
- `city_num=1`, established by the just-completed `City::init` increment and the sole live
  City in the canonical owner;
- `leader_flags2=0`, established by the inactive `Leader::init` store at `0x006e3b25` and
  the absence of a later `+0x04` write before setup (the nearby `0x02000000` OR at
  `0x006e3b28` targets `leader_flags +0x00`, not `leader_flags2`);
- three zero conquest-power bytes inherited from the initial full-row reset and the exact
  24-bit mask initialization at `0x006e3a6c..0x006e3a7f`; and
- `GameInfo.flags & 4` and the replay victory byte used by
  `LeaderData::has_tribe_bonus` `0x006e1370`.

The recovered call order is bonuses `4`, conditionally `22`, then `5`, `16`, `7`, `10`, and
`18`. Bonus 22 is short-circuited when bonus 4 grants the Market. The remaining granted
branches conditionally read exact Constants offsets `+0x5e0`, `+0x7d0`, `+0x640`, `+0x6c0`,
and `+0x810`. Each read retains its decompressed replay byte span. The admitted old-map
fixtures grant none of the seven probes, so retail reads none of those Constants dwords and
issues no `Leader::free_build` call.

## Corpus result

The three source-complete starting-City fixtures contain two centers each. Their six selected
Tribe defaults are `{12,14}`, `{8,12}`, and `{8,12}`. All 42 executed bonus probes take the
exact `NotGranted` fallback exit.

| Replay | candidate before | after empty continuation | retail first Cities | matches | survival |
|---|---:|---:|---:|---:|---:|
| 2018-11-17 | `71020a46` | `71020a46` | `53130d2c` | 0 | 0 |
| 2020-02-08 | `247c0992` | `247c0992` | `dd170c24` | 0 | 0 |
| 2020-02-21 | `cd970956` | `cd970956` | `7d2d0bd4` | 0 | 0 |

This closes a constructor chronology omission but produces zero City bytes and zero City
mutations. Its receipt therefore remains uninstalled. The 2024 Dutch setup is intentionally
outside this cohort: Tribe bonus 22 schedules a Market, so promotion requires the complete
free-Build transaction and City-list join.

## Verification

`starting_city_civ_specific` covers all six replay-backed centers, exact probe order and read
receipts, the three before/after candidates, payload/setup provenance mutation rejection, and
canonical City-owner mutation rejection. No recorded checksum is an input to the authority.
