# Replay terrain-height runtime and Builds-channel falsification

Lane: `replay-builds-checksum` · 2026-08-13.

This lane removes raw Z from the exact starting-Build entrypoint and installs bounded pre- and
post-mountain height-plane producers. A completed-worldgen intermediate authority plus the
canonical World executes the height-producing slice of `TerrainOut::generate_land`. The
post-pass then joins installed displacement geometry with retained `MountainsData` placements
and produces a `TerrainHeightAuthority` able to feed Builds. No desired checksum, desired Z,
SVX Z, or master height word is an input.

The intermediate remains deliberately typed `TerrainHeightPreMountainPlane`, not
`TerrainHeightAuthority`. It can become an authority only through
`finish_new_map_mountains`, the height-writing (`arg7=0`) new-map half of
`adjust_for_mountains`. Consuming `self` makes accidental double application unavailable
through this API. Load/rebuild calls pass `arg7=1`, preserve existing height, and are not
admitted by this producer.

The producer boundary is intentionally upstream of render-resolution samples. The canonical
constructor now derives both initialized `Fractal` byte planes from the parsed replay initial
state and canonical World. A sealed producer now derives the WCoord-resolution
`CoordInfo::flags` grid from the same final World. A second sealed producer installs the three
supported-executable words by their actual identities: global `land_height`,
`TerrainOut::coast_depth`, and global `beach_steepness`. They are neither sampled from SVX nor
accepted as caller-selected height parameters.

## Exact pre-mountain producer

`TerrainHeightPreMountainPlane::from_completed_worldgen` freezes and executes these
supported-PE bodies:

| body | VA | bytes | SHA-256 |
|---|---:|---:|---|
| `TerrainOut::refresh_data()` | `0x00870050` | 867 | `6b97b38457dfc025efe5f050cc37b4123be39fd4fe25d9ccf56a6bceb02d7ec5` |
| `Fractal::init(...)` | `0x006aa2d0` | 1,428 | `44e3a9c196de3c8be8291398dd6608976285fdffb3937180bf697b16ba380546` |
| `Fractal::get_height(int,int)` | `0x006aa870` | 360 | `93057be843aa676b22710c7b79d22861e052c889fbcc2647aaea061dde4dbe02` |
| `TerrainOut::generate_land_lists()` | `0x0085f6a0` | 550 | `ee12fedf562dff5c1f76424883b3ecb02495c3ab101a066519c70bd8d25cee60` |
| `TerrainList::add_new_coord_info(...)` | `0x0084ba10` | 956 | `be78a6cf99430a312db6c87383cf174df1f6e1dbbe7bf99aac0e5ba920c7bbb2` |
| `CoordInfo::CoordInfo(...)` | `0x0084d490` | 164 | `7f245e88623b7d50a0b2b5b096041c9dc42b123afd0cab2c418078c45e5f3804` |
| `TerrainOut::fill_coord_info_mapper()` | `0x0086beb0` | 178 | `f0e859ce0718c7768ac267545c80d2d68a4b946ff98903851c8acdd32b489b86` |
| `Terrain::init(...)` | `0x00850f70` | 6,886 | `af106348bbbc415564cc4048f1a2ed19ce8ce253759841e5517b0a0c1795c2fd` |
| scalar-write slice | `0x00851bf2` | 30 | `59502d7d89f00c03eeff18c8636bd277080cf2ab6b1a7e2f4ddd2765d1b27791` |
| `beach_steepness` initialized-data word | `0x00c0629c` | 4 | `e00e5eb9444182f352323374ef4e08ebcb784725fdd4fd612d7730540b3e0c8c` |
| `TerrainOut::generate_land(int,int)` | `0x0085f8d0` | 2,789 | `5cc1f8e243fcf7556492ca1215df4785a5bd810c3550c7a5a0cb27b79e38445a` |
| `TerrainOut::get_vert_codes` | `0x0086bf70` | 609 | `8eee1044d69671f26222f6801813dbd9e72547a6f9dca4a7b9c4cfbfedaf6695` |
| `TerrainOut::determine_land_height_color` | `0x0086c1e0` | 434 | `720e687b621b9fb72acb922e6097956462adb0fe31b59d4afd589cae57a1eb68` |
| `TerrainOut::find_closest_coordinfo` | `0x0086a260` | 664 | `7968498a530c49764f8b0a2b6c2453a2384d807e1cde2b2090182add5fba75b2` |
| `TerrainOut::smooth_tcoord` | `0x0086c3a0` | 410 | `7f8898b51371d97cd937b44c8a2a40c3b192e6d739f3f7faf935bb117852b8b9` |

The exact downstream bodies are frozen too:

| residual body | VA | bytes | SHA-256 |
|---|---:|---:|---|
| `TerrainOut::adjust_for_mountains(int)` | `0x008703c0` | 3,305 | `4aea3b9267fa1a3bc09b3bcd49cd52e9e4b326f6cf6edc42ae754fdfe5edda5e` |
| `TerrainOut::fill_mountain_data(...)` | `0x00869380` | 1,030 | `8a85b4f8a9beeff18e562523fbfec5d626c1035e5541c225cad9972732a71cd0` |
| `MountainRange::init(...)` | `0x008998b0` | 5,190 | `1b7ac0662a6c23c7a74a23d301255763e636ef952b2e0a90dfd8929478d6f819` |

`Terrain::init` fixes the render tesselation at four vertices per WCoord. `generate_land`
first appends `land_height` for `(tile_xs+1)*(tile_ys+1)` vertices, then overwrites every
vertex in row-major order. The port executes the normal `(0,0)` initialization arm:

`TerrainHeightWorldgenInputs::from_refresh_data` reconstructs both guarded initialized
Fractals before this loop. At `0x00870275`, retail calls `Fractal::init(tile_xs+1,
tile_ys+1, smooth, &World, 2, World::seed)`; at `0x008702a8`, it repeats with
`smooth-2` and wrapping `seed*2`. Smooth is normally 5, but Game semaphore bit 9 changes it
to zero (making the detail request -2, which `Fractal::init` clamps to zero). Each private RNG
is independently reseeded; no map RNG handoff is consumed. The receipt binds the replay
payload, exact call sites, requested smooths, seeds, full guarded planes, draw counts, final
private RNG states, flags, increments, and authority digests.

`TerrainCoordInfoFlagsAuthority::from_world` executes the flags-producing projection of
`generate_land_lists` directly from final `WData`. `CoordInfo` construction zeros its flags
word. `add_new_coord_info` then applies these exact branches:

- A current COAST cell (or raw land value 3) receives `0x8004`; ORIG_COAST on any other cell
  contributes `0x0004`.
- Deep water (`land == 2`) always receives `0x1000`. It additionally receives `0x0080` when
  any effective coast appears in the shipped 24-offset radius-two scan.
- Fertile land (`land == 0`) receives `0x0020` only when effective coast appears in the first
  eight radius-one offsets.

The source digest binds dimensions plus every read `WData` flags/land/land-sub tuple, all four
function bodies, and both 96-byte signed-offset tables (`0x00adcaf4` X SHA-256
`e62c4912f7f7ecd8429aff8e54ba126652c04a9415eae8483f877b4b813dd36c`, `0x00adc404` Y
SHA-256 `253d6dedba6291c57f618feaaec4b772b4f28d412911712b22fe0d2f9352bd45`).
The authority digest separately binds every output word. List allocation and presentation
children are omitted because `fill_coord_info_mapper` projects exactly one node per WCoord and
does not mutate flags. The native diagonal traversals and mapper are square-only—the mapper
allocates `xs*xs` and loops `xs` on both axes—so rectangular Worlds are rejected at this
producer instead of claiming a generalized retail domain.

The scalar producer uses PDB field/global identity and the instruction bytes rather than names
inferred from arithmetic. `Terrain::init` unconditionally writes `land_height = 30.0f`
(`0x41f00000`) and `TerrainOut::coast_depth = -303.0f` (`0xc3978000`). The supported PE's
initialized `beach_steepness` word is `1.0f` (`0x3f800000`). Its only other absolute reference
before the consumers is registration as a live debug parameter; the admitted replay path is
the unmodified supported default. A sealed authority and receipt bind the full init body, the
consecutive scalar-write slice, the initialized-data word, all addresses, and all three exact
binary32 words.

1. `get_vert_codes(...,1)` ORs `TData::RIVER` across the four tiles touching a vertex. Any
   hit locks the vertex and returns height zero.
2. `get_vert_codes(...,0)` ORs the touching `CoordInfo::flags`. Bits `0x1000`, `0x4`, and
   `0x2` select, in that order, `(coast_depth + land_height) * beach_steepness`, zero, and
   fixed height 100. `0x1000` originates on deep-water WCoords; it is not a mountain bit.
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

## Exact post-mountain producer

`MountainRange::init` parses each shipped `<MOUNTAIN height>` as binary32 and decodes the
referenced TGA into a top-left, row-major R,G,B,A surface. The installed producer retains red
and alpha from the same decoded byte buffer and file receipt. On the first four-pixel pass,
alpha decides whether a direct tcoord vertex exists and red supplies Z:

```text
height3 = xml_height * 3.0f
x = ((sample_x - width/2) * 192.0f) * 0.25f
y = ((sample_y - height/2) * 192.0f) * 0.25f
z = (red * height3) / 255.0f
```

All operations remain separate scalar-binary32 operations. A rounding discriminator locks
the ordering: XML height bits `0x3d002717` and red 63 produce Z bits `0x3cbdf7af`; regrouping
the multiplies produces `0x3cbdf7b0`. Vertices append y-major then x-minor, only at nonzero
alpha samples. Retail compares y with width and x with height in this pass while addressing
`pixels[y*width+x]`; shipped inputs are square, so the producer rejects rectangular TGAs
rather than publish a false general-domain claim or reproduce native out-of-bounds access.

New-map `adjust_for_mountains` walks retained placements in ordinal order, obtains the
template index from `mountain_types[i]`, and uses `mountain_locs[i]` as the translation. The
producer requires those locations to equal the source-built WCoord locations and requires the
placement runtime's immutable template footprints to match the installed catalog. For each
ordered direct vertex it reproduces the `addss` translation, strict
`abs(component - lattice) < 0.01f` match, and height-word `addss`. Catalog, placement walk,
pre-plane, and final words receive independent digests. Receipt metrics expose placement,
source-vertex, matched, and unmatched counts.

This closes the deterministic height operation once upstream state exists. Exact mode-5
mountain placement is now available in `MountainAddRuntime`; reaching this seam for Great
Lakes still requires the 16 installed displacement TGAs. The other explicit residuals are
the remaining map-generation path into a final World and retained placements. Installed
displacement bytes remain a mandatory explicit source; no scalar or geometry is fitted from a
save or desired checksum.

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
completed-worldgen or live snapshot. Retail saves do not walk this render-owned plane. The
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
  --test starting_build_activation_runtime \
  --test place_all_installed_mountain_owners
cargo test -p don-sim --test map_core_mountain_template_producer
python3 re/scripts/test_savegame_unit_orderlist_census.py -v
```
