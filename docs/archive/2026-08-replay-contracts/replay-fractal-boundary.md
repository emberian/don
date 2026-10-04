# Replay fertility-fractal boundary

Status: typed, executable proof pack; not yet exported from `don-replay` or
composed into `InitialItemReconstruction`.

The post-continent replay frontier no longer needs to treat the byte plane read
by `TerrainGroups::fill_fertile` as an opaque host receipt. For an ordinary
procedural replay, the plane is a pure function of the replay's world edge and
seed plus installed static data. The implementation deliberately keeps that
static content explicit rather than embedding copyrighted retail data.

## Proven input chain

`TerrainGroups::init_tileset_data` begins at `0x006a61f0`. Its call at
`0x006a6453` is:

```text
Fractal::init(world.xs, world.ys,
              TileSetData::group_data.clump_factor,
              &world, flags = 0, world.seed)
```

The PDB type records establish:

- `TerrainGroups::fractal` is at `+0x80`.
- `Fractal` is 120 bytes: `frac` at `+0`, `xs` at `+24`, `ys` at `+28`,
  `flags` at `+32`, `parts` at `+36`, its private `Random` at `+100`, and the
  two increments at `+104` and `+112`.
- `TileSetData::group_data` begins at `+0x614`, and
  `TileSetGroupData::clump_factor` is its first field.

The corresponding checked-in evidence is `schema/pdb-types.json`,
`schema/rise-procs.tsv`, and the bulk decompilations
`re/decomp-all/006a61f0.c`, `re/decomp-all/006aa2d0.c`, and
`re/decomp-all/006a6f90.c`. Instruction-stream inspection was used wherever
the decompiler obscured byte narrowing, loop boundaries, or float conversion.

The replay contributes:

- `GameInfo::seed`;
- the selected map-style ordinal, whose admitted XML path is already retained
  by `MapStyleStaticData`;
- the shipped map-size table's world edge; and
- `scenario_type == 0`, because custom scenarios can supply non-procedural
  dimensions/state.

Static content contributes:

- ordered `MAP/TILESET/TILECHANCE` rows: `default.xml` is applied first, and a
  selected-map table is applied second when present;
- effective `TILESET_DATA/<selected>/LANDKEY[name=baseland]` frequencies; and
- `TILESET/TERRAINGROUP/CLUMP_FACTOR` plus the number of
  `TILESET/BASELAND/BASE` rows from the user's installed `Data/tilesets.xml`.

## Tileset selection

`Map::init_map_data` starts at `0x0069f250`. At `0x0069f97e`, a chance table
whose total is at least two consumes
`game_random.get(0, 0xffff) % total`. A total below two takes the no-draw fast
path. Selection compares the bucket to cumulative weights with `jle`, so an
exact boundary value remains in the earlier row. This seemingly biased edge is
frozen by seed 108: the draw is 218, the bucket for a total of 100 is 18, and a
first row with chance 18 is selected.

`Map::load_map_data` calls `init_map_data` first for the default MAP and then
for the selected MAP. A selected TILESET table is therefore an override pass,
not a replacement parsed in isolation: both tables consume a draw when both
totals are at least two. `TileSelectionBoundary::passes` preserves that order.

The resolver returns the main RNG state after the final pass as evidence. This is
separate from fractal generation. Each `Fractal::init` random call sets `ECX`
to the `Random` at `Fractal + 100`; it is reseeded directly from `world.seed`
and is independent of all continent and tileset-selection draws.

Replay integration now threads this state into the explicit-RNG continent
entry point before the orientation branch. Seed-start continent wrappers remain
only for isolated style-virtual tests.

## Exact byte-plane algorithm

`Fractal::init` is at `0x006aa2d0`. For the `flags = 0` TerrainGroups call:

1. Allocate `(xs + 1)` columns of `(ys + 1)` zero bytes.
2. Validate the requested smooth factor, then clamp it to `0..=5`.
3. Seed the private RNG with `world.seed`.
4. Iterate levels from `smooth` down through zero. At each level use
   `step = 1 << level` and `mask = (1 << (level + 1)) - 1`.
5. Copy column zero to the X guard column before every level. On the coarsest
   level, fill the active lattice with `Random::get(0, 0xff)`.
6. On later levels, retain old coarse lattice points. New axial points use the
   rounded two-neighbor mean; new diamond points use the rounded four-corner
   mean.
7. Add
   `get(0, (1 << (8 - smooth + level)) - 1)
    - (1 << (7 - smooth + level))`, then clamp to `0..=255`.
8. Copy the X guard once more. The Y guard remains zero.

The proof test freezes all 64 interior bytes, both guard behaviors, draw count,
and final private RNG state for `(8, 8, smooth=2, seed=1)`.

## Partition construction

`0x006a6474..0x006a64c6` converts all baseland frequencies except the last to
partition thresholds. Each row is converted to binary32, divided by binary32
`100.0`, multiplied by binary32 `255.0`, truncated to an integer, narrowed to
its low byte, and cumulatively added with byte storage. Thus the ordinary Dirty
row `[45, 25, 20, 10]` produces `[114, 177, 228]`.

## History archaeology and remaining provenance

`cv search` passes over `fill_fertile`, `init_terrain_data`, `random_frac`, and
the post-continent replay work found several earlier notes that correctly named
the fractal and partitions as missing, but no recovered implementation or
additional serialized replay source. The static map-style frequency rows were
already present under `ron-data/mapstyles`; the overlooked lawful input was the
installed `Data/tilesets.xml`.

That file exists in the game VM at:

```text
C:\Program Files (x86)\Steam\steamapps\common\Rise of Nations\Data\tilesets.xml
```

It has not been copied into the repository. The path-based API therefore makes
the missing provenance precise: root integration must either point at a user's
installed file or admit it into a gitignored local asset directory. It must not
hardcode observed `CLUMP_FACTOR` values or assume every tileset has four
baseland textures.

## Frozen handoff and validation

This lane owns exactly these new files:

```text
crates/don-replay/src/fractal_boundary.rs
crates/don-replay/tests/fractal_boundary_reconstruction.rs
docs/assembly/replay-fractal-boundary.md
```

No compiler, formatter, test, Cargo, or remote job was run in this lane. Root
can validate the isolated proof pack without touching the local Cargo lock:

```sh
tools/swarm-cargo-remote submit hbox replay-fractal-boundary \
  --path crates/don-replay/src/fractal_boundary.rs \
  --path crates/don-replay/tests/fractal_boundary_reconstruction.rs \
  -- test --locked -p don-replay --test fractal_boundary_reconstruction
```

The subsequent integration and its `TerrainGroups::place_all` frontier are
documented in `docs/assembly/replay-fractal-integration.md`.
