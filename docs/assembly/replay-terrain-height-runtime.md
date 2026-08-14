# Replay terrain-height runtime and Builds-channel falsification

Lane: `replay-builds-checksum` · 2026-08-13.

This lane removes raw Z from the exact starting-Build entrypoint and installs a bounded
pre-mountain height-plane producer. A completed-worldgen intermediate authority plus the
canonical World now executes the height-producing slice of `TerrainOut::generate_land`. No
desired checksum, desired Z, SVX Z, or master height word is an input.

The result is deliberately typed `TerrainHeightPreMountainPlane`, not
`TerrainHeightAuthority`, and therefore cannot feed the Build entrypoint. Normal
`Terrain::init` later calls `adjust_for_mountains`; its `fill_mountain_data` child can add to
height words at `0x008695f9`. Installing that post-pass is required before the derived plane
may answer `find_tcoord_z` or make a first-checkpoint claim. The earlier complete-plane/live
snapshot authority remains the only admitted query source today.

The producer boundary is intentionally upstream of render-resolution samples but downstream
of three still-unported deterministic owners: the two initialized `Fractal` byte planes and
the WCoord-resolution `CoordInfo::flags` grid built by `generate_land_lists`. The shipped
`Fractal::get_height` body now samples both planes inside the producer; callers cannot supply
its output grids. Reconstructing the initialized Fractal states and CoordInfo flags from the
map seed and canonical completed World is the exact remaining upstream terrain residual. The
three installed scalar words (`land_height`, mountain height, and height scale) are also
explicit, so a tileset/configuration change cannot silently reuse a plane.

## Exact pre-mountain producer

`TerrainHeightPreMountainPlane::from_completed_worldgen` freezes and executes these
supported-PE bodies:

| body | VA | bytes | SHA-256 |
|---|---:|---:|---|
| `Fractal::get_height(int,int)` | `0x006aa870` | 360 | `93057be843aa676b22710c7b79d22861e052c889fbcc2647aaea061dde4dbe02` |
| `TerrainOut::generate_land(int,int)` | `0x0085f8d0` | 2,789 | `5cc1f8e243fcf7556492ca1215df4785a5bd810c3550c7a5a0cb27b79e38445a` |
| `TerrainOut::get_vert_codes` | `0x0086bf70` | 609 | `8eee1044d69671f26222f6801813dbd9e72547a6f9dca4a7b9c4cfbfedaf6695` |
| `TerrainOut::determine_land_height_color` | `0x0086c1e0` | 434 | `720e687b621b9fb72acb922e6097956462adb0fe31b59d4afd589cae57a1eb68` |
| `TerrainOut::find_closest_coordinfo` | `0x0086a260` | 664 | `7968498a530c49764f8b0a2b6c2453a2384d807e1cde2b2090182add5fba75b2` |
| `TerrainOut::smooth_tcoord` | `0x0086c3a0` | 410 | `7f8898b51371d97cd937b44c8a2a40c3b192e6d739f3f7faf935bb117852b8b9` |

The exact downstream red boundary is frozen too:

| residual body | VA | bytes | SHA-256 |
|---|---:|---:|---|
| `TerrainOut::adjust_for_mountains(int)` | `0x008703c0` | 3,305 | `4aea3b9267fa1a3bc09b3bcd49cd52e9e4b326f6cf6edc42ae754fdfe5edda5e` |
| `TerrainOut::fill_mountain_data(...)` | `0x00869380` | 1,030 | `8a85b4f8a9beeff18e562523fbfec5d626c1035e5541c225cad9972732a71cd0` |

`Terrain::init` fixes the render tesselation at four vertices per WCoord. `generate_land`
first appends `land_height` for `(tile_xs+1)*(tile_ys+1)` vertices, then overwrites every
vertex in row-major order. The port executes the normal `(0,0)` initialization arm:

1. `get_vert_codes(...,1)` ORs `TData::RIVER` across the four tiles touching a vertex. Any
   hit locks the vertex and returns height zero.
2. `get_vert_codes(...,0)` ORs the touching `CoordInfo::flags`. Bits `0x1000`, `0x4`, and
   `0x2` select, in that order, scaled mountain height, zero, and fixed height 100.
3. All other vertices call `Fractal::get_height(x,y)` on both initialized Fractal states.
   The port reproduces its double-precision half-coordinate scaling, bilinear interpolation,
   `cvttsd2si` clamp, and either raw-byte, percentage, or 16-threshold partition return. The
   native outer-X/inner-Y `(xs+1)*(ys+1)` byte planes, flags, partitions, Random seed, exact
   increments, and source identities are all bound before sampling. The resulting two bytes
   enter:

   ```text
   base = coarse * 7.5f + land_height + detail * 1.875f
   ```

4. `find_closest_coordinfo(...,4)` searches the four native square shells (WCoord radius
   zero through three) for flags bit `0x20`. It measures binary32 distance to every render
   vertex of matching cells. A hit blends `base` toward the native coastal ramp with the
   exact binary32 instruction order.
5. Every coastal hit inside the native interior bound enters `temp_smooth`. At
   `0x0086c360..0x0086c366`, retail compares both coordinates with the same X-derived
   `tesselation * world_xs` extent; the port preserves that rectangular-map quirk exactly.
   The producer replays `smooth_tcoord` in list order, including the second pass when either
   coordinate is odd. Zero samples are excluded from each 3x3 average; only positive centers
   are replaced.

The completed-worldgen digest binds both fully initialized Fractal-state digests, the
CoordInfo source identity and flags, and World dimensions. The derived pre-mountain-plane
digest additionally binds all three scalar words, the internally sampled byte grids, the
canonical TData plane, and every output height word. Receipt counters expose each native
branch plus smoothing vertices/passes. Invalid Fractal increments, sample bounds, shape
mismatches, and anonymous authorities fail before any plane is published.

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

`TerrainHeightAuthority` carries exact float words and a nonzero identity for a coherent
completed-worldgen or live snapshot. The new derived plane cannot become that authority until
the mountain pass is installed. Retail saves do not walk this render-owned plane. The
authority validates the canonical World dimensions, TData length, full height-plane length,
and query bounds before reading. Invalid pointer-domain queries are refused rather than
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
