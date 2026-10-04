# Frame-zero starting-City `in_port` clear

Lane: `checksum-cities` · replay-correctness · 2026-08-13.

## Result

`starting_city_in_port` owns the remaining `Leader::plan_strategy` City scalar that does
not depend on the unfinished starting Units, final World, Regions, or installed content.
For every active Leader, retail scans `0..city_mark`; each live City receives signed short
zero at `CityData::in_port +0x4e`.

The supported executable and matching PDB identify `Leader::plan_strategy` at
`0x006B9620`. Capstone reread freezes the loop at `0x006B9DF7..0x006B9E2C` and the sole
City store at `0x006B9E1F`:

```text
006b9df7  xor  edx,edx
006b9df9  cmp  dword ptr [ebx+0x408],edx   ; LeaderData::city_mark
006b9e14  mov  eax,dword ptr [eax+edx*4]   ; City pointer
006b9e17  test byte ptr [eax+4],1          ; City active
006b9e1f  mov  word ptr [eax+0x4e],cx      ; cx == 0
006b9e2c  ...                              ; following Build/Unit census
```

The atomic adapter first validates the canonical Sim-owned City pool against all Leader
validity mirrors, center-Build registry identities, positions, City links, and current
Build types. It then stages every active Leader's exact `city_mark` walk and publishes only
after the complete receipt is built. A stale join refuses before changing any City byte.

## Replay metric

All six active Cities in the three admitted style-6/style-9 recordings already contain
`in_port = 0` from `City::init`. The source-exact frame-zero writer therefore proves six
zero after-images but changes **0/6 City values and 0 checksum bytes**:

| fixture | candidate before | candidate after | retail |
|---|---:|---:|---:|
| 2018-11-17 | `71020a46` | `71020a46` | `53130d2c` |
| 2020-02-08 | `247c0992` | `247c0992` | `dd170c24` |
| 2020-02-21 | `cd970956` | `cd970956` | `7d2d0bd4` |

Matches remain **0/3** and Cities survival remains zero.

This is intentionally a no-change ownership increment, not a fitted checksum repair.
`free`, `busy`, `gatherers`, and `peasant_dist` still require the complete canonical
starting-Unit receivers. The terrain census and `bordering` have separate exact owners,
while the complete first-checkpoint scheduling join remains unproved. Consequently the
receipt keeps `first_checksum_city_image_ready = false` and the replay channel remains on
its existing fail-closed boundary.

Focused gate:

```sh
cargo test -p don-replay --test starting_city_in_port -- --nocapture
```
