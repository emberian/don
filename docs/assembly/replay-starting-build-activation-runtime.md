# Replay starting-Build activation runtime

This tranche completes the checksum-visible Build row for an ordinary first Village. It
does not allocate a parallel object pool and does not copy a replay checksum. The producer
joins the existing canonical `Sim::builds` row, dense/sparse band-2000 registry, current
ptype, and the landed fresh-City constructor receipt. All work is staged; the Build row,
World subset, and `BuildsWalkAuthority` publish together only after the exact Build walk
succeeds.

The source-backed entrypoint now takes `BuildInitPrefixTerrainRequest`, which has no raw Z,
plus a coherent `TerrainHeightAuthority`.  It executes the complete shipped
`TerrainOut::find_tcoord_z` leaf and joins that receipt to this activation transaction before
publication.  The older raw-Z entrypoint remains available for explicit measured receipts and
unit isolation; it is not the replay-exact worldgen path.  Details and the fresh-SVX
falsification of flat Z are in `docs/assembly/replay-terrain-height-runtime.md`.

## Temporal boundary

The source order retained by the receipt is:

1. validate the canonical row and ptype;
2. replay `BuildData` construction and the `Build::init` prefix through the instruction
   before `Object::add_to_world`;
3. add the object to an empty center WData cell;
4. complete `Wall::init` and the `Build::init` tail;
5. execute the checksum-relevant final state of `Wall::start` and attest its native
   `Wall::mask_city` call boundary;
6. execute common `Wall::activate` writes, emit the normalized CITY-transaction handoff,
   and execute the already-proven City join in
   `Build::activate`;
7. retain the final Setup visibility bytes after `Object::update_seen(0)` and
   `Object::update_seen_ally`;
8. retain frame-zero `Leader::process_all` hit/LOS results;
9. normalize through at least one active `Build::process`; and
10. walk the resulting Build row.

The first active process matters. `Wall::init` computes an under-construction
`construct_hits` value with job counter zero. Activation resets both job counters, then
frame-zero Wall stats can change full hits and LOS. `Build::process` finally copies the
active full-hit value into `construct_hits`. A receipt that stops at activation is not the
first-checkpoint Build image.

## Exact Build owner

The transaction owns every checksum-visible byte or pointer fact reached by
`BuildData::walk_data`:

- founder, max age, owner, object id, encoded XYZ, current ptype, Object body, Wall body,
  Build sentinels, City link, stance, visibility bytes, and original type;
- the null `ObjectData::launching` pointer;
- the Village `BuildQueue::init(20)` result: 20 zeroed records with only record zero's
  type short set to `-1`, and logical `queued == 0`;
- the empty MiningList with constructor capacity 5, doubling increment `-1`, flags 0, and
  `mtn == cliff == -1`; and
- an empty gather-point list.

One such Build walks 491 bytes: the 131-byte empty-container base plus 20 queue records
times 18 walked bytes. The capacity-5 Mining allocation is retained in the receipt even
though `Array<TCoordData>::walk_data` returns after its zero length and therefore does not
hash capacity on this path.

Type/rule/Leader-derived values are explicit source facts: Wall-init and first-checkpoint
full hits, construction time, final LOS, stance, visibility masks, and the Indian city
radius gate. They are not inferred from TypeIndex alone and are not replay checksum
inputs. Admission requires positive hits/time, frame-zero setup, at least one active Build
pass, owner visibility, no Village air-carry arm, and no terrain-gather arm.

## Source-owned World subset and CITY handoff

The transaction performs the instruction-owned World mutations needed to preserve object
identity and the ordinary building footprint:

- `Object::add_to_world` installs the Build as the head of an otherwise empty center
  WData cell and writes the Build's `up/down/down_who` links;
- `BuildType::mask_me` ORs center WData bit `0x4000`;
- the final 7-by-7 footprint clears transient STARTED/STARTED2 bits and installs blocker
  kind 3.

`Wall::start` reaches the CITY writer through the exact chain
`Wall::mask_me -> BuildType::mask_me -> Wall::mask_city`. At the native call the Build
flags are `0x23` (STARTED, not ACTIVE), the City link is still `-1`, and the `0x20` city
gate is set. This owner emits a typed
`WallMaskCityRequest` containing the decoded center TCoord, resolved radius 20 or 24,
exact `on == 1`, canonical owner/object identity, those native call facts, and normalized
post-activation flags `0x27`. It deliberately does **not** stamp TData CITY. The separate
World transaction must validate and consume that request before the City census. That
owner also preserves the retail `even_circle_*` table and write order.

This is not a World-channel readiness claim. The receipt keeps the following residuals
red: terrain terraform/height refresh, the activation CITY-mask transaction, the content
footprint mask's blocked counters, perimeter roads, behind masks, fog/reveal callbacks,
collision/graphics callbacks, and Leader/global statistics. No residual is silently
replaced by a plausible default.

## Failure and installation semantics

The adapter supports only an empty center object-list head. A nonempty WData head would
require validating and mutating the prior object's `up` link, so it refuses rather than
splicing an incomplete list. It likewise refuses wrong registry identity, ptype, City
constructor join, prefix facts, map shape, footprint bounds, visibility, and walk length.

All checks and mutations occur against staged copies. An error preserves the Build row,
World, and any prior walk authority. On success the row and World commit first, then the
complete `BuildWalkFacts` row is installed. A caller may run `check_sim_builds` only after
every live starting row has produced the same complete receipt; channel installation
remains a cohort decision outside this exclusive module.

## Verification

`crates/don-replay/tests/starting_build_activation_runtime.rs` pins the PE/PDB addresses
and ordered boundary, constructs a canonical City-linked center, verifies the final
491-byte walk and `check_sim_builds` result, checks the intrusive WData link, all 49
building blockers, the radius-20 CITY request, proves this owner leaves CITY bits
unchanged, and checks atomic refusal for invalid source facts, occupied center cells, and
missing owner visibility.
