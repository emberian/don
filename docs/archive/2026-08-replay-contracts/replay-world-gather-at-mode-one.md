# Retail `World::gather_at` mode-one owner

This tranche closes the installed-content boundary reached by the starting-City terrain
census. Evidence is the supported `riseofnations.exe`, GUID-matched `rise.pdb`, the shipped
`ron-data/rules.xml`, and the canonical `map_terrain::World`. Recorded replay checksums are
comparison outputs only.

## Exact body

`World::gather_at` is `0x006B07F0`. The City caller passes mode 1. That arm:

1. clears six signed-int outputs;
2. calls `WorldData::get_land(wx, wy, 1)` (`0x006B4730`), whose precedence is Coast,
   Forest, Mountains/impassable, Rocks/Oil, then the signed `WData::land` byte;
3. walks the selected `LandData.make[4]`/`num_make[4]` pairs in order, ignoring zero
   quantities and resource ids outside `0..6`; and
4. reads only the center TData word at `(wx*4+2, wy*4+2)`.

Mode 1 never enters the nine-neighbor loop and never reads placed Good, Mountain, or Cliff
objects. `GoodTypeData::is_flat` (`0x004780C0`) is exactly the inverse of
`TypeData::is(TypeIndex, 0)` for Timber, Metal, and Oil (`1`, `4`, `5`). Consequently:

- Food, Wealth, and Knowledge add the ordinary `num_make`;
- Timber, Metal, and Oil add `num_make * 2` while the center lacks TData `GATHERED`; and
- those three depletable resources add nothing once the center is `GATHERED`.

All arithmetic uses wrapping i32 behavior.

## Content and state admission

`InstalledLandCatalog` is deliberately narrower than `GatherTerrainMaterialization`. It
admits only the exact supported `rules.xml` identity (88,632 bytes, SHA-256
`2cad6156f257c2faf79c3fa2de293a249f61ae245160b92fb5a76d0dbf3a9988`) and validates the
nine ordered Land names and every four-slot MAKE row. It does not claim that procedural
Mountain/Cliff arrays exist.

`CityTerrainGatherFacts::from_installed_land_catalog` evaluates every in-bounds WCoord and
retains the installed digest, World shape/seed/full checksum, Land histogram, gathered
count, and a deterministic checksum over all coordinates and six outputs. The City
transaction revalidates the World and fact-table checksums before staging any write.
Missing rows, invalid Land indexes, stale World state, and fact mutation are typed atomic
refusals.

## Replay result and residual

Across the three admitted style-6/style-9 recordings, both starting centers now cross the
former `MissingGatherAtFact` boundary: six Cities execute 402 mode-one calls and 42 City POD
bytes change. The constructor candidate and terrain-census candidate both match 0/3 first
retail Cities checkpoints. The exact candidates are:

| replay | constructor | with terrain census | retail |
|---|---:|---:|---:|
| 2018-11-17 | `71020a46` | `34a10d02` | `53130d2c` |
| 2020-02-08 | `247c0992` | `e80c0c4e` | `dd170c24` |
| 2020-02-21 | `cd970956` | `91360c12` | `7d2d0bd4` |

No producer is promoted from that zero-match result. Final territory/City `bordering`, the
canonical starting Scout/Citizen set and its City scalar census, and the full pre-checkpoint
schedule remain explicit red gates.

Focused gates:

```sh
cargo test -p don-sim --lib mode_one
cargo test -p don-replay --test starting_village_suffix
```
