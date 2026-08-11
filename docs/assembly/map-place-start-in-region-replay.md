# `Map::place_start_in_region` replay continuation

This lane binds the existing complete selector port to the East Meets West
replay caller. Evidence is the supported `riseofnations.exe`, shipped PDB,
direct Capstone disassembly, `re/decomp-all/0068ac00.c`, the retail-oracle
record recorded in `docs/mechanics/map-terrain.md`, and both checksum-bearing
style-19 headers. No VM or live process is touched.

The callee begins at `0x0068ac00`. Its only RNG call is `Random::get` at
`0x0068ac8b`. Success writes X then Y and returns 1 at `0x0068ae16`; exhausting
both passes returns 0 at `0x0068ae49`. The body ends at `0x0068ae4c`, followed
by four `int3` bytes before `find_region_centroid` at `0x0068ae50`.
PDB gives the seven explicit arguments as region id, X output, Y output,
minimum distance, an integer which this shipped body never reads, and two
optional `SimpleArray<WCoord>*` overrides. East Meets West passes `24`, `12`,
null and null for the latter four arguments in both real receipts, so World's
own empty start arrays are the separation source.

East Meets West calls the selector at `0x00696fe3` and tests EAX at
`0x00696fe8`. Success copies both output words at `0x00696fec..0x00696ffb`,
jumps to `0x00697453`, and reaches `World::add_starting_location` at call site
`0x00697461` / callee `0x006b2de0` without another RNG call or walked-World
write. Failure enters the still-unported caller fallback at `0x00697003`.

The replay adapter validates the selected Region and caller-owned start-array
shape before drawing, executes the already-oracle-backed selector, receipts
each consumed RNG word/anchor and exact return edge, and proves the selector
does not mutate the walked World. It does not execute either caller residual.
The selector draw site is appended to `ContinentReceipt::direct_rng_sites` in
the same chronology as all preceding style/growth draws.

## Frozen style-19 receipts

| replay | region / size | RNG before → raw / anchor → after | pass / output | World / WData |
|---|---:|---:|---:|---:|
| 2018-12-01 | `1 / 3164` | `0xd4d996b4 → 21122 / 2138 → 0x751b5283` | `1 / (26,56)` | `0xb2eeff92 / 0x2eaaf2e7`, unchanged |
| 2019-03-24 | `2 / 2793` | `0x986535d1 → 62715 / 1269 → 0x8e6cf4fc` | `1 / (87,78)` | `0xeb4960c6 / 0x69f054b2`, unchanged |

Both first selectors return through `0x0068ae16`. The exact first
`World::add_starting_location` call at caller `0x00697461`, callee
`0x006b2de0`, is documented in
`docs/assembly/world-add-starting-location-replay.md`. The caller continuation
now repeats the exact selector/writer pair for every remaining active fixed
slot and freezes before `Map::check_player_land`; see
`docs/assembly/east-meets-west-remaining-starts-replay.md`. Synthetic pins
separately exercise a singleton
pass-2 success, the zero return into `0x00697003`, and malformed Region refusal
before RNG.

## Validation

- local fresh-source focused suites: selector 3/3, edge canals 4/4, continent
  reconstruction 4/4, and centroid recovery 4/4;
- local reconstruction/ownership suites: 6/6;
- local two-header localizer: both boundaries are
  `map_team_continent_add_start`, both owner ledgers are coherent, and all
  65,081 same-group peer comparisons agree;
- Persvati clean-HEAD overlay compile
  `replay-place-start-check-v3-20260811T192048Z-99461-29566-f80e3692fcd3`;
- Persvati clean-HEAD overlay synthetic suite
  `replay-place-start-tests-v2-20260811T193153Z-18330-2597-cf5392598655`:
  both asset-independent tests pass 2/2. The real replay corpus is not a remote
  overlay asset, so both checksum-bearing receipts are gated locally.
