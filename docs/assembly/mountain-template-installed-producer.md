# Installed mountain displacement-template producer

Lane: `mountain-template-producer`. Supported executable:
`ron-bin/riseofnations.exe`, PE32/i386, image base `0x00400000`, SHA-256
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`.
Names and layouts come from the matching `ron-bin/sbl/rise.pdb`; behavior below comes from
Capstone over that PE. `re/decomp-all/008998b0.c` was used only as a control-flow map.

## Result and boundary

`crates/don-sim/src/systems/mountain_template_producer.rs` is a lawful installed-content
producer for the three `MountainRangeData` coordinate pairs consumed by
`Mountains::add_mountain` and the direct displacement vertices consumed by the later
height-writing `TerrainOut::fill_mountain_data` call:

- `mount_tx / mount_ty`;
- `mount_wx / mount_wy`; and
- `solid_mount_wx / solid_mount_wy`.
- ordered `MountainRangeOut::tcoord_verts` binary32 XYZ words.

The producer parses the existing `MOUNTAINS` catalog in `effects_graphics.xml`, preserves
document order as the native template index, parses its required binary32 `height`, loads each
source-named `TEMPLATE_TEX` from an explicit content root, decodes the supported TGA surface,
and derives the three footprints plus the direct displacement vertices.
It contains no shipped pixel data and no precomputed geometry rows. Proprietary art remains
in the user's installation and is read only at runtime.

This remains a **Tier-C, instruction-derived producer**. The implementation has not yet run
against the sixteen proprietary displacement images or against a retail
`MountainRange::init` oracle. The checked-in shipped XML establishes the sixteen ordered
source bindings (7 `lg`, 8 `med`, 1 `sm`; heights 450/250/100); synthetic TGA tests establish the decoded algorithm
and fail-closed boundary. No release or full replay-compatibility claim follows until a user
supplies the actual installed art and its outputs are compared with retail.

The producer is registered in `systems/mod.rs` and is joined to the canonical replay
place-all owner by `crates/don-replay/src/replay_place_all_owners.rs`. The caller supplies a
`ReplayMountainContentProvider`; `from_installed_content` admits no owner unless the provider
produces exactly sixteen source rows and exactly sixteen derived templates. The resulting
`ReplayInstalledMountainOwnerReceipt` retains the provider, all source strings, XML and TGA
path/byte-length/Adler evidence, and all derived placement/height geometry atomically. Each file receipt and
its geometry are computed from the same read buffer; proprietary bytes are not retained.
`entry_owners(world_cells)` then constructs a fresh
`MountainAddRuntime` whose verification bitset is sized to the caller's actual World.

The automatic reconstruction still calls `cold_process`, because the replay does not own or
name a user's installation. It therefore keeps `mountains: None` in the absence of an explicit
provider. No synthetic/default geometry is a production fallback.

## XML-to-template identity

`Mountains::init` `0x0089ad70` obtains the `MOUNTAIN` node list at `0x0089b1c6`–`0x0089b1df`
and walks its `ObjectArray<XMLElement>` forward. For every row it reads:

| XML child | native use |
|---|---|
| `TEMPLATE_TEX file` | first `String&` passed to `Mountains::add_range`, then the temporary texture decoded by `MountainRange::init` |
| `MAIN_ALPHA_TEX file` | second `String&`; presentation `mountain_alpha` texture |
| `RING_ALPHA_TEX file` | third `String&`; presentation `ring_alpha` texture |

The three non-empty-string tests are at `0x0089b72b`–`0x0089b74d`; only when all pass does
the caller reach `Mountains::add_range` at `0x0089b7af`. `add_range` forwards those three
arguments in the same order at `0x0089934d`–`0x00899358`, and stores the new range pointer in
the first free `ranges` slot. Therefore document row *n* and runtime template index *n* are
one identity for the shipped sixteen-row domain.

The producer retains all three source names and applies the same non-empty triple gate even
though only `TEMPLATE_TEX` affects simulation geometry. It refuses more than sixteen rows;
the native seventeenth-row overflow documented in `docs/derivation/mountain-range-lists.md`
is not reproduced as heap corruption.

Windows `.` and `\` path syntax is normalized under an explicit caller-supplied content root.
Absolute paths and `..` are rejected before I/O, so an XML row cannot turn installed-content
resolution into an arbitrary filesystem read.

## TGA surface semantics

`MountainRange::init` loads `TEMPLATE_TEX` as a temporary `Texture` at
`0x0089991c`–`0x00899940`. The subsequent virtual calls read width (`vtable +0x58`), height
(`+0x5c`), and mip-0 pixels (`+0x38`). Every geometry presence test masks the decoded dword
with `0xff000000`, for example `0x00899b7d`–`0x00899b86`, `0x00899ca8`–`0x00899cb7`, and
`0x0089aa88`–`0x0089aaa0`. Occupancy is therefore **any non-zero alpha**, not luminance and
not an RGB threshold.

The producer follows the image path used by retail:

- `ImageIO::fill_targa_info` `0x00512900` admits uncompressed/RLE true-color forms and
  records TGA descriptor bits 4 and 5;
- `ImageIO::decode_tga` `0x00546e10` starts from the appropriate destination edge and
  applies signed X/Y strides (`0x00546e5d`–`0x00546eae`), leaving a top-left, left-to-right
  texture surface; and
- 32-bit TGA `B,G,R,A` becomes an in-memory R,G,B,A dword: byte zero supplies displacement
  height and byte three supplies occupancy.
  The producer also accepts retail's 24-bit true-color form, whose implicit alpha is `0xff`.

Both TGA image types 2 and 10 are decoded. RLE packets are bounded against the declared pixel
count before expansion. Color-mapped images, non-zero TGA X/Y origins, truncated data, zero
dimensions, packet overflow, and rectangular images remain typed failures. Retail's direct
vertex/face loops use swapped width/height bounds and are only memory-safe on the shipped
square domain. This is intentionally narrower than
every presentation format accepted elsewhere in the renderer; it covers the true-color
domain used by this producer without inventing palette behavior.

## Exact footprint derivation

Let `w`/`h` be decoded dimensions and define:

```text
sx = (w / 2) % 16
sy = (h / 2) % 16
c4x  = (w / 2 - sx) / 4
c4y  = (h / 2 - sy) / 4
c16x = (w / 2 - sx) / 16
c16y = (h / 2 - sy) / 16
```

Dimensions are positive `u16`, so these are the same results as retail's signed
`cdq/sub/sar` and sign-corrected divisions. The non-zero center remainder is load-bearing:
the suite includes a 36×36 source, where `sx = sy = 2`.

### `mount_t`

The first grid pass (`0x00899c47` onward) samples alpha at `(sx + 4k, sy + 4j)` and parks a
vertex index in a temporary 4-pixel lattice. The face pass at `0x00899f39`–`0x0089a358`
accepts a 4×4 cell exactly when all four lattice corners are present. At the same point it
appends:

```text
mount_t = (x / 4 - c4x, y / 4 - c4y)
```

`mount_tx` and `mount_ty` are independent native arrays; the Rust product pairs every row so
a mismatched X/Y length cannot enter `MountainAddRuntime`.

### `mount_w`

For each accepted `mount_t` cell, retail computes:

```text
candidate = (x / 16 - c16x, y / 16 - c16y)
```

It linearly scans the existing `mount_w` pairs at `0x0089a21e`–`0x0089a25c` and appends only
when no equal pair exists (`0x0089a262`–`0x0089a2b9`). Thus order is first occurrence in the
same Y-major/X-minor 4-pixel walk, not sorted order. The producer uses a set only for the
membership test and a separate vector for that first-occurrence order.

### `solid_mount_w`

The final pass starts at `(sx, sy)` and advances 16 pixels while `x + 16 < w` and
`y + 16 < h` (`0x0089aa13`–`0x0089aa5c`). For each candidate it samples a 5×5 alpha grid at
offsets `{0,4,8,12,16}`. The append branch at `0x0089aac9`–`0x0089aad9` is taken when every
sample is present **or the occupied count is greater than 15**. The observable predicate is
therefore `occupied >= 16`; an exactly-16 fixture is accepted and its one-bit 15-sample
mutation is rejected. Appended coordinates use the `c16` formula above.

### Direct height vertices

The first four-pixel pass also appends one direct vertex for every sampled pixel with nonzero
alpha. Order is Y-major then X-minor. With the XML height parsed as binary32, retail executes:

```text
height3 = height * 3.0f
x = ((sample_x - w/2) * 192.0f) * 0.25f
y = ((sample_y - h/2) * 192.0f) * 0.25f
z = (red * height3) / 255.0f
```

The producer retains the exact XYZ words and does not regroup those scalar operations. These
are the vertices selected by `fill_mountain_data(..., arg6=0)` and therefore the only template
mesh rows that can add to `master_land_heights`. The later 8-pixel fcoord vertex/face mesh,
peak coordinates, texture-coordinate arrays, alpha textures, GPU buffers, and presentation
bounds remain excluded.

## Verification and mutation sensitivity

`crates/don-sim/tests/map_core_mountain_template_producer.rs` mounts the source directly and
currently has eleven tests covering:

- non-multiple-of-32 center alignment and Y-major/X-minor output order;
- alpha-vs-RGB occupancy and the `alpha != 0` predicate;
- the exact 16-of-25 solid threshold;
- uncompressed and RLE input under all four descriptor origins, including repeated packets
  that cross scanline boundaries;
- malformed/truncated/overflowing TGA refusal;
- XML document identity, the three-file gate, and the shipped 16-row census;
- source-name-only installed reads and index preservation; and
- content-root confinement.
- red-channel/XML-height Z derivation, exact float ordering, and rectangular-image refusal.

Persvati gate:

```text
cargo test -p don-sim --test map_core_mountain_template_producer
11 passed; 0 failed
```

Two reversible Persvati mutations were executed against the focused assertions, on remote
overlays only. Changing the decoded channel from TGA alpha (`pixel[3]`) to red (`pixel[2]`)
made the alpha-vs-color test fail with 64 `mount_t` rows instead of 1. Changing the solid
threshold from `>= 16` to `> 16` made the exactly-16 test fail with 0 rows instead of 1.
Both jobs exited 101, and the source was restored before the final green gate. The suite is
also order-sensitive: sorting `mount_w` instead of retaining first occurrence changes the
dense 36×36 assertion.

`crates/don-replay/tests/place_all_installed_mountain_owners.rs` adds five integration tests
using generated 32-bit TGA fixtures only:

- a complete 16-row provider retains all sources, exact file evidence, and derived templates in
  one receipt and produces 16 non-null runtime slots with a World-sized verification bitset;
- a 15-row provider is rejected before an owner can be obtained;
- changing one TGA's alpha footprint changes both its retained Adler evidence and the derived
  runtime while leaving the XML/source binding unchanged;
- the real Mediterranean replay consumes the installed mode-4 region owner, records a
  `PlaceAllOwnerSource::Region` mountain receipt, grows `Mountains::walk_data`, and crosses its
  former group-zero stop; and
- the real Great Lakes replay consumes a synthetic installed catalog through the exact
  player-mode-5 owner, retains player-source World/mountain-walk receipts, and crosses its former
  group-two stop without routing the call through mode 4; with the already-derived tileset doober
  rules it reaches the reporting-only boundary and retains a checksum-coherent final staged
  World/RNG/Mountains/owner authority.

Local gate (actual replays present, synthetic displacement art):

```text
cargo test -p don-replay --test place_all_installed_mountain_owners -- --nocapture
5 passed; 0 failed
```

Two reversible local mutations were also killed by the focused integration assertions.
Weakening the complete-catalog gate from `len() != 16` to `len() > 16` admitted a 15-row
provider and failed the incomplete-catalog test. Dropping the first template while constructing
`MountainAddRuntime` produced 15 runtime slots and failed the atomic-retention test. Both
mutations were restored before the final green gate.

## Integration contract

The installed replay owner now enforces the following coherent hook:

1. supply the installed `effects_graphics.xml` path and its matching content root;
2. call `load_mountain_template_catalog` and stop atomically on every typed error or on any
   source-row/file-evidence/template count other than 16;
3. require its source order to be the same order used by the already-landed mountain range
   list producer;
4. retain the provider, source rows, exact file evidence, and derived templates in one immutable
   initialization receipt, then install `catalog.templates` directly as the
   `MountainAddRuntime` vector; and
5. only then release region and player `MountainsAddMountain` requests to the recovered mode-4
   and mode-5 transactions respectively.

No empty vector, all-transparent placeholder, hand-authored shape, checksum-fitted table, or
retail-derived precomputed geometry file is an acceptable fallback.

The supported calls are verification mode 4 (`excluding_verify`), reached by the Mediterranean
region arm, and mode 5 (`sliding_excluding_verify`), reached by the Great Lakes player arm. The
player owner mutates the staged live World at each exact candidate/retry boundary. Installed
templates establish geometry provenance; absent installed geometry remains a typed stop, and no
verification algorithm is substituted for another.
