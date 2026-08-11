# `Mountains::add_mountain` mode-4 runtime

Lane: `world-mountain-runtime`.  Supported executable:
`ron-bin/riseofnations.exe`, PE32/i386, image base `0x00400000`, SHA-256
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`.
Names, signatures, and layouts come from the matching
`ron-bin/sbl/rise.pdb`; behavior below comes from Capstone over the shipped PE.
The Ghidra C in `re/decomp-all/0089c2e0.c` was used only as a control-flow map.

## Result and honest boundary

`crates/don-sim/src/systems/mountain_add_runtime.rs` executes the complete
`verification_mode == 4` path of `Mountains::add_mountain` `0x0089c2e0`
through its native return.  That is the path map-style 12 reaches through
`TerrainGroup::drop_tile`: mode 4 calls `Mountains::excluding_verify`
`0x00898570`, then the common spacing and commit tail.

The runtime consumes zero RNG.  It owns:

- the temporary `verify_bits` set/clear transaction and player-start exclusion;
- `quick_verify_template`'s flat-world and start-city reservation checks;
- the four ordered mountain/coast/forest/rock spacing scans;
- exact WData mountain flags, blocked mountain TData, and the northern
  three-tile `BEHIND_B` row;
- the two retained WCoord arrays, `Vert3Array`, and type array, including their
  native 4/doubling capacities and walked metadata; and
- atomic rejection/error rollback over both World and MountainsData.

This does **not** close the replay mountain boundary yet.  The sixteen
`MountainRangeData` geometry products are not in `.rcx` and are not present in
the checked-in XML.  `effects_graphics.xml` names proprietary displacement
images such as `.\art\h1_disp_0.tga`; `MountainRange::init` `0x008998b0`
turns those inputs into the three coordinate pairs consumed here.  No lawful,
redistributable extractor/decoder currently supplies them.  Tests use small
synthetic geometry only to pin the instruction-derived transaction.  Synthetic
geometry is not evidence that a shipped template was reconstructed, so the
producer and replay gate remain explicitly red.

Fidelity tier is **C, structural/instruction-derived**.  No call to retail
`Mountains::add_mountain` has been oracle-executed and no checksum agreement is
claimed.

## Native call and mode selection

The PDB signature is:

```text
Liberr __thiscall Mountains::add_mountain(
    int template, WCoord x, WCoord y, int mode,
    int mountain_space, int forest_space, int rock_space,
    int coast_space, int start_min)
```

The body is 1,936 bytes and ends in `ret 0x24`.  Capstone establishes the
argument mapping rather than relying on the decompiler:

| instruction span | behavior |
|---|---|
| `0x0089c2fe`–`0x0089c327` | validate `template` against `ranges.length` and the non-null pointer row |
| `0x0089c32d`–`0x0089c3bd` | switch modes 1–5 |
| `0x0089c372`–`0x0089c390` | mode 4 pushes World `start_y +0x9c`, `start_x +0x80`, stack `+0x28` (`start_min`), y, x, template, then calls `excluding_verify` |
| `0x0089c3c2`–`0x0089c3c4` | any non-zero verifier return jumps to `Liberr 1` |
| `0x0089c697`–`0x0089c6ac` | rejection returns `1` |
| `0x0089c9a2`–`0x0089ca52` | committed path returns `0` |

Modes 1, 2, 3, and 5 remain typed `UnsupportedVerificationMode` boundaries.
They call `verify_template`, `quick_verify_template`, `sliding_verify`, and
`sliding_excluding_verify` respectively; pretending mode 4 covers their
coordinate-adjusting behavior would be a false closure.

An absent/null template is a typed producer error in DoN, rather than retail's
release-build diagnostic fallthrough.  This distinction prevents a missing
proprietary input from being laundered into `LIBERR_OK` with no mutation.

## `MountainRangeData` projection

PDB `MountainRangeData` names six arrays in three X/Y pairs.  The instruction
stream addresses them through the virtual-base displacement at `[range+4]`:

| logical pair | native count | X list | Y list | use in `add_mountain` |
|---|---:|---:|---:|---|
| `mount_tx / mount_ty` | `+0x0c` | `+0x18` | `+0x34` | tile blocker/mountain writes and behind flags |
| `mount_wx / mount_wy` | `+0x44` | `+0x50` | `+0x6c` | verification and spacing footprint |
| `solid_mount_wx / solid_mount_wy` | `+0x7c` | `+0x88` | `+0xa4` | WData mountain-class writes |

`MountainTemplateRuntime` retains these as paired `GridOffset` rows.  Pairing is
the producer boundary: a mismatched X/Y native list cannot enter the runtime as
plausible geometry.

## Mode-4 verification

`Mountains::excluding_verify` is instruction-bounded at
`0x00898570`–`0x0089891c`.

For each `mount_w` cell in template order, an in-bounds row-major WData index is
set in `MountainsData::verify_bits` only if the bit was previously clear.  That
index is retained in the function-static `to_clear` array.  Only newly marked
cells are compared against the paired World start arrays, in player order,
using `vector_dist` `0x0046cff0`; a strict `distance < start_min` rejects.  Both
the success and rejection paths clear exactly `to_clear`, preserving bits that
were set before the call.  Instruction anchors are:

- `0x008986bc`–`0x00898730`: test, set, and retain a WData bit;
- `0x00898740`–`0x00898775`: paired start-array walk and strict distance test;
- `0x008987ca`–`0x00898808`: success-path clear;
- `0x0089880a`–`0x00898853`: rejection-path clear; and
- `0x00898858`–`0x0089885f`: tail-call `quick_verify_template`.

`quick_verify_template` `0x00898ab0` then walks `mount_w` again.  Each cell and
every radius-one neighbor must be in bounds.  The center must satisfy
`WorldData::is_flat`: no `MOUNTAINS|FOREST`, not ocean unless `WATERHALF` is
set, and no `ROCKS|IMPASSABLE_X`.  The center and radius-one ring must not be
present in `WorldData::start_city_locs`.  The ring starts at
`circle_radius[0]` and stops before `circle_radius[1]`, so the separately tested
center is not tested twice.

## Common spacing and commit order

The common pre-commit loop is `0x0089c3ca`–`0x0089c697`.  For every `mount_w`
cell it scans circle offsets beginning at `circle_radius[0]`, skipping off-map
probes.  The four scans occur in this exact order:

| order | stack argument | reject mask |
|---:|---|---:|
| 1 | `mountain_space`, `[ebp+0x18]` | `0x10 | 0x40` |
| 2 | `coast_space`, `[ebp+0x24]` | `0x04` |
| 3 | `forest_space`, `[ebp+0x1c]` | `0x20` |
| 4 | `rock_space`, `[ebp+0x20]` | `0x08` |

The non-argument order is load-bearing: swapping the adjacent stack slots is a
plausible transcription error.  The focused suite gives each field a different
flag and requires the corresponding rejection kind while asserting complete
rollback.

Once all verification passes, retail cannot return another ordinary `Liberr`.
Mutation order is:

1. `0x0089c6b8`–`0x0089c7cf`: for each `solid_mount_w` cell, preserve land,
   preserve `COAST|ORIG_COAST`, clear `LAND_CLASS_MASK`, and add `MOUNTAINS`.
2. `0x0089c7cf`–`0x0089c895`: convert each `mount_t` offset with
   `tx = world_x*4 + offset_x`, call `World::set_blocked_at(..., 1)`, then write
   `(tdata & ~1) | 2`, the mountain blocker kind.
3. `0x0089c897`–`0x0089c8a6`: call `set_behind_flags` `0x00898050`.  Its loop
   reads byte offsets 0, 4, and 8 from `move_x/move_y`, i.e. NW, N, NE; valid
   tiles whose blocker kind is not already mountain gain `BEHIND_B`.
4. `0x0089c8a6`–`0x0089c99f`: append x, y, `(float)(x*0x300),
   (float)(y*0x300), 0`, and template index to the four MountainsData arrays.

The constructors at `MountainsData::MountainsData` `0x00433c40` install
`capacity=0`, `increment=-1`, `flags=0`.  `SimpleArray<WCoord>::increase_size`
`0x00434630`, `SimpleArray<int>::increase_size` `0x004220e0`, and
`ArrayBaseSimpleCopy<Vector<float>>::increase_size` `0x004340f0` therefore grow
0→4→8→16.  `Mountains::walk_data` `0x0089d320` walks length and, when non-zero,
capacity, increment, flags with bit `0x40` masked out, then elements.  The
runtime exposes these exact bytes through `walked_bytes`; a `Vec` length alone
would not be checksum-compatible.

## Transaction and tests

The native body performs no fallible gameplay call after verification, but its
array appends can encounter corrupt metadata or allocation failure.  DoN stages
both the World host and MountainsData clone.  Only a `Liberr 0` receipt installs
them.  A retail `Liberr 1` rejection and every typed producer/runtime error leave
both inputs byte-for-byte unchanged.

`crates/don-sim/tests/map_core_mountain_add_runtime.rs` adapts the source-frozen
trait onto the canonical `map_terrain::World`.  Eleven tests pin:

- successful WData/TData/behind writes, retained vertices, and zero RNG;
- independent mountain/coast/forest/rock argument mapping;
- temporary and pre-existing `verify_bits` ownership;
- start-distance, start-city-neighbor, and edge rejection order;
- malformed independent start-X/start-Y lengths as a typed stop before paired
  indexing or mutation;
- a late array-metadata failure after staged world writes (rollback);
- malformed template input as a typed producer stop;
- 4→8 capacity growth and walked metadata; and
- explicit refusal of modes not reached by map-style 12.

These are mutation-sensitive transcription tests, not retail differential
evidence. Two reversible seeded probes were also run: cross-wiring
`coast_space` to `forest_space` was killed by the four-spacing test, and
changing the NE behind offset from `+1` to `+2` was killed by the success-path
world-mutation test. The exact source was restored before the final green gate.

## Exact integration hooks

No shared registration or replay schedule file is edited by this lane.  The
integration owner needs all of the following, in dependency order:

1. Add `pub mod mountain_add_runtime;` to
   `crates/don-sim/src/systems/mod.rs`.
2. Implement `MountainWorld` for `systems::map_terrain::World`, mapping
   `set_mountain_tile` to `World::set_mountain_at(..., true)` and
   `set_behind_b` to `World::set_behind(..., true, true)`. Expose the retained
   start-X and start-Y lengths separately: the runtime requires equality before
   calling `start_at`.
3. Retain one `MountainAddRuntime` beside the existing `Mountains` range-list
   cursor state in the composed `TerrainGroups::place_all` runtime.  Do not
   initialize its template catalog from synthetic/default geometry.
4. At `DropTileExternalRequest::MountainsAddMountain`, convert the nine fields
   one-for-one into `AddMountainCall`.  A receipt supplies the native `liberr`;
   a typed error remains a named transaction stop and releases no asserted
   external resolution.
5. Build a lawful installed-content producer for the sixteen
   `MountainRangeData` rows.  Only after that producer is mutation-pinned against
   the shipped loader may replay schedules replace
   `place_all_mountains_add_mountain` with the next executed primitive.

The third and fifth hooks are why this tranche is a real runtime advance but
not a replay-compatibility closure claim.
