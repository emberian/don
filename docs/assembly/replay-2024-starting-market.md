# Golden 2024 Dutch starting Market

The golden replay does not enter `Setup::build_units` with only its starting Village.
`Setup::build_cities` (`0x005AB910`) always calls `Setup::build_civ_specific`
(`0x005AB760`) after constructing a center, including starting-town mode 1. Owner 0 is
Dutch (Tribe 22), so the exact bonus probes are:

```text
4 false, 22 true, 5 false, 16 false, 7 false, 10 false, 18 false
```

That schedule issues exactly one call before all seven starting Unit calls:

```text
Leader::produce_building(type=436, origin_o=2000, mode=0)
```

The name `free_build` used in older setup notes is only descriptive. The PDB/PE function
at `0x006E1400` is `Leader::produce_building`.

## Exact placement boundary

The Market Rules row is replay-derived: footprint 4x4, land domain 0, and build flags
`0x80001201`. The generic raw-zero footprint owner in
`leader_produce_building_blocked_site_prefix` owns the next source-complete Market interval
after the candidate prefix. It walks sixteen
`BuildTypeData::blocked_tcoord` calls in x-outer/y-inner order. Each ordinary Market tile
returns raw zero directly; unlike Farm, Market does not enter `LandData::get_amount`.
The receipt then emits a typed `BuildTypeData::blocked_location` request at call site
`0x00636D18`, stopping at its first generated-World child, `WorldData::get_tregion`.

Market consumes no Farm-style coarse RNG. After a coarse site wins, the parent re-probes a
2x2 fine grid in x-outer/y-inner order. Each successful fine site consumes one
`Random::get(0, 65535)` at `0x006E2C00`, scores `raw % 100`, and replaces the current winner
on `<=`, so a later equal score wins. The count is one through four on a successful call.

## City checksum after-image

`GoldenStartingMarketCityReceipt` requires the source-derived plan, exact 16-tile placement
receipt, supported executable identity, adjacent pre/post Sim hashes, and the complete fine
RNG trace. It then validates dense Build allocation as owner-0 o2001/type436 and the City
chain `center(2000).city_down=2001`, `market.city=0`, `market.city_down=-1`. The Market is
VALID|STARTED|ACTIVE, has Object flag `0x20` clear, and owns the training queue's 20 slots.

For the reached grade-4 site, these are all City checksum bytes written:

| City offset | writer | exact effect |
|---:|---|---|
| `+4..+6` | `Build::activate` `0x00623E20` | `city_flags |= 0x0800` (`0x4011 -> 0x4811`; only byte `+5` changes value) |
| `+100` | `Leader::produce_building` | `filled = filled + 1` (`1 -> 2`) |
| `+105` | `Leader::produce_building` | `space[0] = max(space[0] - 1, 0)` |
| `+106` | `Leader::produce_building` | `space[1] = max(space[1] - 1, 0)` |
| `+107` | `Leader::produce_building` | `space[2] = max(space[2] - 1, 0)` |

The binder clones the complete pre-City record, applies only those operations, and requires
byte-for-byte equality with the post-City. Every other City slot and `city_mark` must remain
unchanged. It walks canonical Cities before and after, but remains unmounted: no real golden
pre/post process capture or complete generated World is currently installed.

## Residual

The first unresolved placement child is `BuildTypeData::blocked_location` (`0x006375B0`).
Later coarse candidates and the fine probes require the final generated World, City territory,
and live type relations. Allocation, `Build::init`, activation visibility/road effects, and
the full non-City Sim mutation surface still need one supported retail capture before this
receipt can become the golden frame-zero publisher. Recorded checksum words are never inputs.
