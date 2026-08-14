# Frame-zero starting-City census clear prefix

Lane: `checksum-cities` · replay-correctness · 2026-08-13.

## Result

`starting_city_census_clear` separates the source-complete prefix of the first
`Leader::plan_strategy` City census from the still-blocked Unit walk. For each active
Leader, retail scans `0..city_mark`; a live City has `gatherers`, `busy`, and `free`
cleared and receives `peasant_dist = City slot + 100` as a signed-short after-image.

The supported executable and matching PDB locate the loop at
`0x006B9746..0x006B97CA`. Capstone reread freezes the stores:

| address | City field | value |
|---:|---|---:|
| `0x006B976F` | `gatherers +0x5c` | `0` |
| `0x006B9789` | `busy +0x5b` | `0` |
| `0x006B97A3` | `free +0x5a` | `0` |
| `0x006B97BD` | `peasant_dist +0x50` | low short of `slot + 100` |

The slot term corrects the earlier generic transcription. Retail executes
`lea esi,[edx+0x64]` before loading the City pointer and later stores `si`. The current
one-City-per-owner fixtures all use slot zero, so their value remains 100; a focused
two-City mutation test freezes values 100 and 102 with an inactive slot between them.
The complete Unit-census owner now shares this exact initializer.

The adapter validates the complete canonical Sim City/Leader/center-Build join before
staging, walks the exact high-water marks, and publishes only after the full prefix receipt
is complete. A stale slot identity refuses before any clear leaks. The transaction writes
no World, Build, registry, or RNG state.

## Replay metric

On the six current Cities, `free`, `busy`, and `gatherers` are already zero. Only the low
byte of each slot-zero `peasant_dist` changes from 0 to 100: **6 exact changed checksum
bytes across 6/6 Cities**.

| fixture | constructor before | clear prefix after | retail |
|---|---:|---:|---:|
| 2018-11-17 | `71020a46` | `bb3a0b0e` | `53130d2c` |
| 2020-02-08 | `247c0992` | `6eb40a5a` | `dd170c24` |
| 2020-02-21 | `cd970956` | `17de0a1e` | `7d2d0bd4` |

The same exact prefix can be composed diagnostically with the existing terrain census.
That provisional World/territory input remains explicitly unpromoted:

| fixture | terrain before | terrain + clear | retail |
|---|---:|---:|---:|
| 2018-11-17 | `34a10d02` | `7ed90dca` | `53130d2c` |
| 2020-02-08 | `e80c0c4e` | `32530d16` | `dd170c24` |
| 2020-02-21 | `91360c12` | `db6e0cda` | `7d2d0bd4` |

Both views remain **0/3 matches** and Cities survival remains zero. The later Unit walk
can replace `peasant_dist` with the nearest empty-action Citizen distance and increment
`free`; it still requires the complete canonical Scout/Citizen receiver images for every
active owner. Therefore this prefix is not mounted as a first-checkpoint producer and its
receipt keeps both `city_unit_census_complete` and `first_checksum_city_image_ready` false.
No recorded checksum is an input to the owner.

Focused gates:

```sh
cargo test -p don-replay --test starting_city_census_clear -- --nocapture
cargo test -p don-replay --test starting_city_unit_census -- --nocapture
```
