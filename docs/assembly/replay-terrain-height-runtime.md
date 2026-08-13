# Replay terrain-height runtime and Builds-channel falsification

Lane: `replay-builds-checksum` · 2026-08-13.

This tranche removes raw Z from the exact starting-Build entrypoint.  A completed
`TerrainOut::master_land_heights` plane plus the canonical World's TData now produces the
`SubObject::init` Z through the shipped function before the existing atomic starting-Village
activation and 491-byte Build walk execute.  No desired checksum or desired Z is an input.

It does **not** generate the replay's height plane.  The first-checkpoint replay producer
remains blocked on a coherent completed-worldgen authority for the actual recording, plus the
other already-named setup/frame-zero residuals.

## Exact shipped body

The supported PE body is `TerrainOut::find_tcoord_z(TCoord,TCoord,int)` at
`0x008544a0`, 196 bytes, SHA-256
`f9f2e5c9f818c640dd38e8ba9a055ead1eb03d029ac42a62139366c4c9dd94ef`.
The public Coord wrappers are `GameAccessConst::find_tcoord_z` `0x005834b0` and
`TerrainOut::find_tcoord_z(Coord,Coord,int)` `0x00866710`; both convert Coord through the
signed `div_3_table[c >> 6]` ladder before calling the TCoord body.

For an initialized plane, the body reads exactly two diagonal vertices from the
`(tile_xs + 1) * (tile_ys + 1)` binary32 height grid:

```text
a = height[(ty + 1) * (tile_xs + 1) + tx]
b = height[ ty      * (tile_xs + 1) + tx + 1]
raw_z = cvttss2si((a + b) * 0.5f)
```

It returns zero before reading either height vertex when the addressed TData surface field
`(mask & 0x30)` is `0x20` (water), or after conversion when
`raw_z < 0 && final_arg == 1`.  `cvttss2si` truncates toward zero and returns
`i32::MIN` for NaN/overflow; the port preserves that behavior rather than using Rust's
saturating float cast.  When the height array length is zero, retail returns
`cvttss2si(land_height)` immediately, before bounds, water, or negative-zero handling.  That
fallback is exposed by the read-only query but refused by the starting-Build wrapper because
`Setup::build_cities` runs after `Terrain::init`.

`TerrainHeightAuthority` carries exact float words and a nonzero identity for a completed
worldgen or coherent live snapshot.  Retail saves do not walk this render-owned plane.  The
authority validates the canonical World dimensions, TData length, full height-plane length,
and query bounds before reading.  Invalid pointer-domain queries are refused rather than
trying to reproduce retail's out-of-range memory access; every replay Build query is inside
the admitted initialized domain.
`apply_build_init_prefix_from_terrain` and
`complete_starting_village_build_from_terrain` accept a request shape with no Z field; the
query receipt and Build/activation receipt stay joined through the publication boundary.

## Fresh-SVX Builds projection

The existing exact Objects census now retains the decoded XOR-obfuscated XYZ and strips only
save-only `walk_test` tags from every Build body.  Concatenating live rows in native owner/band
order produces the checksum-path bytes; the recorded save is not used to fit a value.

For the independently captured fresh v16 SVX:

| measurement | result |
|---|---:|
| Build rows / live rows | 800 / 37 |
| exact live Builds walk bytes | 11,578 |
| isolated Builds checksum | `0x673e8820` |
| counterfactual checksum with every logical Z flattened to zero | `0x73b77f08` |
| nonzero live Z rows | 36 / 37 |
| distinct live Z values | 36 |
| Village rows (`TypeIndex 414`) | 7 |
| Village Z values | `104, 468, 551, 592, 510, 150, 276` |

Every Village is a 491-byte walk, matching the starting-Village runtime independently.  A
one-bit mutation of the first saved Village's encoded Z preserves all parse boundaries and
11,578-byte length while changing both the Build manifest and channel checksum.  Thus flat Z
is not a harmless render approximation: it is checksum-visible through the inherited
SubObject image.

The SVX and target replay are different matches and remain deliberately unjoined.  This
projection validates traversal shape and falsifies flat terrain; it is not an attestation for
the replay's worldgen plane and does not increase first-turn checksum survival by itself.

## Gates

```sh
cargo test -p don-replay --test terrain_height_runtime \
  --test starting_build_activation_runtime
python3 re/scripts/test_savegame_unit_orderlist_census.py -v
```
