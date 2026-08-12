# Step-12 detector and visibility producer frontier

This note freezes the retail producer which must replace the live Sim bridge's
`detector: false`. The shipped `riseofnations.exe` and `rise.pdb` are authoritative. The
isolated executable seam is
`crates/don-sim/src/systems/step12_visibility_producer_frontier.rs`; the later live-integration
section records the Sim-attached preflight owner now built on that frozen seam.

Evidence image hashes:

- `ron-bin/riseofnations.exe`: `30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`
- `ron-bin/sbl/rise.pdb`: `334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`

## Recovered facts

| fact | retail evidence | consequence |
|---|---|---|
| full refresh cadence | `GameDaemon::process_all` `0x007327CC..0x007327E6`: signed `idiv 100`, compare remainder with `33`, call `0x00732840` | this is a phase-33 refresh every 100 frames, not an every-frame rebuild |
| no-fog early return | `GameDaemon::update_all_seen` `0x00732856..0x0073286A`: compare `Game +0x30` with `3` before `GameDaemon +0x28 = 4` | option 3 mutates neither `busy` nor any fog plane even when the callback is scheduled |
| clear order | `0x00732871` calls `World::clear_seen`; `0x0073287C..0x00732896` separately zeroes `World +0x164 seen3` | current `seen`/`wcoord_seen` and detector `seen3` are cleared before object stamping; `seen2` persists |
| Unit traversal | `0x007329ED..0x00732A7D`: eight active leader slots, ascending Unit-band rows, virtual `is_valid_unit`, virtual `is_on_map`, `update_seen(0)` | owner activity, instance validity, containment, order, and class are all producer inputs |
| valid Unit leaf | `UnitData::is_valid_unit` `0x0046CDA0`: `flags +0x08 & 1` | an inactive row is skipped; no inferred default is allowed |
| on-map leaf | `UnitData::is_on_map` `0x0046CE30`: high bit of `inside_up +0x82` | negative `inside_up` is on-map; contained/launched rows do not stamp |
| detector source | `Object::update_seen` `0x00651D98..0x00651DB8`: `flags +0x08 & 0x40` selects the whole detection prefix | `seen3` comes from instance `OBJECT_DETECTOR`, not cloak state, viewer state, or a hardcoded boolean |
| detector initialization | `Object::init` `0x006477FC..0x00647813`: virtual `has_objmask(0x02000000)`, then `flags |= 0x40` | `OBJMASK_DETECT` seeds the instance bit once; step 12 does not re-read the type mask |
| LOS source | `Object::update_seen` calls virtual slot `+0x128`; Units dispatch to `UnitData::los` `0x006100C0` | raw `ObjectData::mylos +0x3C` is insufficient because the Unit override adds exact leader/general/type bonuses |
| small-LOS centre | `Object::update_seen` `0x00651C96..0x00651D29`: for radius `<=3`, a domain-zero Unit with clear `unit_flags2&4` and `unit_masks&1` calls `project(x,y,angle,0x180)` `0x0092CF40` | the fog disc can be centred one fog cell ahead of the object's own visibility-query cell; using object coordinates unconditionally shifts checksum-visible fog |
| stamp arithmetic | `Object::update_seen` `0x00651C49..`: `(los * 0xC0) / 0x180`, cap `0x40`; `World::set_seen` gets detect=`circle_index < detect_end` | every point in a detector's LOS disc writes its owner bit to `seen3`; a one-tile LOS still stamps the radius-zero centre |
| consumer query | `UnitData::is_detected` `0x0060A630`: own unit succeeds, otherwise `seen3[cell] & viewer LeaderData::ally_mask +0x6929` | allied detector coverage is shared at query time; `see_all` and fog option 3 do not create detection |
| checksum owner | `World::walk_data` section 6; `CheckSums::check_all` channel 12 | `seen`, `seen2`, and `seen3` are synchronized state. A guessed detector bit is a multiplayer/replay desync, not just an RL observation error |

The phase-33 call is step 12's periodic cadence, not the producer's only entry. Five direct
call sites force the same body immediately:

| call site | caller |
|---|---|
| `0x0058525D` | `Game::run` |
| `0x006295F7` | `Build::close` |
| `0x009FC78D` | `ScenarioFuncSet::add_visibility` |
| `0x009FC82D` | `ScenarioFuncSet::remove_visibility` |
| `0x00A033F5` | `ScenarioFuncSet::set_explored_show_buildings` |

Therefore a complete port must route those state changes through the same transaction rather
than waiting up to 100 frames for the periodic refresh.

The PDB enum names the exact byte flags: `OBJECT_VALID=0x01`, `OBJECT_STARTED=0x02`,
`OBJECT_ACTIVE=0x04`, `OBJECT_IDLING=0x08`, `OBJECT_NEW_THINK=0x10`, `OBJECT_CITY=0x20`,
`OBJECT_DETECTOR=0x40`, and `OBJECT_ATTACKING=0x80`. `ObjMaskType` independently names
`OBJMASK_DETECT=0x02000000`.

The Unit LOS override is now frozen rather than hidden behind a generic callback. It starts
from signed `ObjectData::mylos` and evaluates these arms in order:

1. If `LeaderData::num_units[0x138] != 0`, `UnitTypeData::role +0x2C8 & 0x420 != 0`, and
   `ObjectData::has_general(0, 0x16A /* Ptolemy */) >= 0`, add
   `Constants::ptolemy_los_bonus +0xB64`.
2. If `LeaderData::num_units[0x133] != 0`, virtual `UnitTypeData::is_siege()` is false, and
   `ObjectData::has_general(0, 0x165 /* The CEO */) >= 0`, add
   `Constants::theceo_unit_los +0xCA8`.

The optimized indices map back to TypeIndexes through the shipped Unit table's `0x32` base:
`0x138 + 0x32 = 0x16A` and `0x133 + 0x32 = 0x165`. The isolated resolver preserves that
short-circuit order and rejects a reached-but-missing siege/general fact.

There is a second coordinate source for short-ranged land units. Once the resolved LOS radius
is at most three fog cells, retail uses the ordinary object position if any of these is true:

- the object is not a Unit (excluded by this Unit-only seam);
- `ObjectTypeData::domain +0x218 != 0`;
- `UnitTypeData::unit_flags2 +0x2B8 & 4 != 0`;
- instance `UnitData::unit_masks +0x68 & 1 != 0`.

Otherwise it calls `project(x, y, angle, 0x180)` and uses that result as the fog-disc centre;
`angle` is the live signed dword at `UnitData +0x50`, not `dest_angle +0x58`. The isolated
planner requires the exact projected coordinate on that reached branch rather than recomputing
with host floating point. The external target query still samples `seen` and `seen3` at the
object's own coordinates, exactly like `UnitData::is_seen/is_detected`.

For every admitted disc cell, `World::set_seen` ORs the owner's bit into `seen`, conditionally
into `seen3` for a detector, persistently into `seen2` and `WData::was_seen`, and into the current
coarse `wcoord_seen`. A newly changed `seen2` byte causes `Object::update_seen` to call
`World::reveal_fog`. After that ordinary stamp, nonzero `ObjectData::infiltrated +0x3A` invokes
`World::set_was_seen` for its extra recipient, affecting that recipient's `seen2` and
`WData::was_seen` but not granting live `seen` or detection.

## Complete top-level order and honest residuals

After the option-3 early return, `GameDaemon::update_all_seen` performs:

1. `GameDaemon.busy = 4`;
2. `World::clear_seen` (current `seen` and `wcoord_seen`, plus a presentation invalidation
   callback for old cells);
3. zero `seen3`;
4. traverse the Build and Wall bands;
5. traverse the Unit bands;
6. if `Game +0x821 & 0x10` or `Game +0x822 & 0x02`, stamp scenario reveal points with
   `detect=0`;
7. if `Game::frame +0x550 == 0`, share persistent explored `seen2` across retail's alliance
   sets. A normally scheduled phase-33 call cannot take this tail; only a direct call made at
   frame zero can reach it.

The isolated module owns the cadence, instance detector transition, Unit admission/stamp
plan, and exact post-frame `seen`/`seen3` cell sample. Later bounded tranches mount those
facts, active Build/Unit local-seen bodies, and `Game::run`'s frame-zero explored-sharing
tail, but still do **not** claim the complete 1,221-byte producer. Remaining bodies before
general closure are:

- a nonempty dedicated Wall band (structurally absent in the supported executable);
- `Object::update_seen`'s effectful newly-explored `World::reveal_fog` branches;
- scenario reveal-point storage and its two Game flag gates;
- the other four direct-entry owning actions; and
- incremental `Object::update_seen(1)` calls outside this periodic step-12 refresh.

These are checksum/replay concerns even though only the Unit detector subpass is needed to
remove `detector: false`.

## Safe integration seam

The shared integration should be one preflighted transaction:

1. Resolve every canonical Unit-band row in retail owner/object order. Require live
   `flags`, `inside_up`, deobfuscated coordinates, `infiltrated`, domain/type/instance masks,
   the **resolved** `UnitData::los()` value, and the exact forward-projected centre from live
   `angle +0x50` at distance `0x180` when its branch is reached. Missing
   production/type/leader/general/project facts must stop before clearing the previous planes.
2. Ensure every object allocation path materializes `OBJECT_DETECTOR` from the installed
   type's `object_masks` exactly as `Object::init` does. Do not re-derive it during step 12;
   later instance mutation remains authoritative.
3. On `frame % 100 == 33` and `fog_option != 3`, clear in retail order and feed each admitted
   `Step12UnitStamp` into the full-disc `borders_fog::update_seen` body using its stamp
   coordinates. Commit the returned newly explored cells through the recovered `reveal_fog`
   path. Its `detector` field must come from `object_flags & 0x40`, never `false` on missing
   data. `Game::run` is now mounted; route the remaining four immediate callers above through
   the same body only as their owning mechanics land.
4. After the whole `Sim::do_frame` (including later incremental object visibility work), use
   `sample_visibility_cell` to fill
   `RetailUnitVisibilityFacts::{cell_seen_mask,cell_detected_mask}` for the external owner.
   Copy `ObjectData::visible +0x40` separately as the direct-viewer override.
5. Install the external frame atomically, binding `{Handle, who, o, uid}` and its plane bytes
   to one Sim frame/revision. ATTACK must remain masked if any row or viewer fact is stale.

This placement preserves retail's distinction: the producer writes owner bits into `seen3`;
`UnitData::is_detected` later intersects that plane with the viewer's allied mask. It also
keeps the external policy snapshot read-only—the RL boundary does not manufacture or mutate
simulation visibility.

## Rejuvenation live-owner audit

The follow-up audit compared that seam with the live `Sim` stores rather than assuming the
isolated facts had become available. The current ownership split is:

| input | live source today | production status |
|---|---|---|
| flags, `inside_up`, who/o/uid, x/y, angle, `mylos`, `unit_masks`, `infiltrated` | generated `UnitCols`, `ObjectRegistry`, and `Sim::unit_type` | present |
| detector instance provenance | `World::spawn_typed` and `allocate_typed_at` currently write flags `1`; no allocation owner retains the `Object::init` mask decision | missing; a clear live bit is not proof of non-detector status |
| object masks | `LiveProductionType::object_masks` for installed production types | partial; not joined to every spawn/save/BHS type path or the instance byte |
| domain, `unit_flags2`, role | tracked shipped table and several subsystem projections | no single canonical live owner; `LiveProductionType` does not carry all three |
| Ptolemy/CEO optimized counts | `LiveProductionLeader::unit_counts` | partial; scenario `Sim::spawn_unit` does not update that owner |
| `ObjectData::has_general` radius result | Arena has a typed HeroesData adapter | absent from Sim; step-8 stat rows are not an owner-local identity/radius registry |
| Ptolemy/CEO Constants | shipped derivation records value `2` for both | not a composition-bound live Constants owner; hardcoding would reject mod authority |
| exact projection | live x/y/angle and exact trig primitives exist | no revision-bound projection receipt tied to the same Unit/type/general snapshot |

Consequently the current tick body cannot honestly commit even an apparent “ordinary
non-detector” map. It clears `seen`, reads raw `mylos`, and passes `detector:false` before any of
those missing owners can object. Likewise, retroactively ORing `OBJECT_DETECTOR` from the current
type would be wrong: retail seeds the instance at initialization and later instance mutation is
authoritative.

The exclusive source now exposes `prepare_live_unit_pass` as the maximal non-mutating live
subset. Its input and output rules are deliberate:

- unscheduled step 12 and the producer's fog-option-three early return admit without reading a
  snapshot or authority receipt;
- the snapshot flattens only active leaders' Unit bands in owner/object order and binds their
  complete cardinalities, frame, state revision, type revision, and `{row,who,o,uid,type}`;
- invalid or contained rows stop at retail's lazy gates and need no type/LOS/detector facts;
- a reached row requires a matching `Step12UnitAuthorityReceipt`; zero LOS does not read
  small-LOS type or detector provenance, a radius above three does not read domain/flags, and
  every later optional field becomes mandatory only when retail reaches it;
- detector truth must be proven either by retained `object_masks_at_init` or by an explicit
  same-revision authoritative instance byte. There is no default-false constructor;
- a reached short-LOS projection receipt echoes the exact source x/y, live angle, and `0x180`
  distance before its result is accepted;
- success returns an opaque `PreparedStep12UnitPass` with stable identities and exact stamp/skip
  decisions. It intentionally has no commit method and therefore cannot clear a checksum plane.

The smallest next owner is one Sim-attached, revisioned `Step12VisibilityAuthority`. It must be
created from the same synchronized rules/mod composition as the BHS type owner and own these
joined transitions:

1. retain the immutable visibility type projection `{obj_masks,domain,unit_flags2,role,is_siege}`
   and the two Constants values at the canonical type revision;
2. materialize and preserve `OBJECT_DETECTOR` in every allocation/load path;
3. maintain exact owner-local unit counts and the ordered HeroesData identity/radius registry
   across spawn, death, containment, compaction, save/load, and BHS creation;
4. resolve the two `has_general` calls and exact small-LOS projection against one state revision;
5. emit the authority receipt before step 12 and reject it after any relevant mutation.

Once that owner exists, the prepared Unit pass can join a full-producer transaction. Plane clear
still remains blocked until the active Wall/started-Wonder pass and newly-explored `reveal_fog`
effects are owned; publishing an isolated Unit-only clear would delete legitimate building vision.

## Live Sim integration status

`crates/don-sim/src/systems/step12_visibility_runtime.rs` now supplies that maximal honest
owner and `Sim` carries it as `step12_visibility`. The runtime keeps independent state/type
revisions and a composition digest; an installed type source contains Constants plus the exact
`{object_masks, domain, unit_flags2, role, is_siege}` projection. Per-leader state contains the
complete 352-entry optimized Unit count table and an ordered, identity-bearing HeroesData
registry. Per-instance state retains either the mask observed at `Object::init` or an explicitly
authoritative current flags byte, so a missing record is a fault rather than `detector:false`.

The live preparation walks the canonical sparse Unit bands. It validates every reached live
row against its `{Handle, who, o, uid}` registry identity, validates exact active-owner Unit
counts, resolves the ordered Ptolemy/CEO `has_general` searches and Constants additions, and
binds the exact small-LOS projection receipt to the same authority revision. Invalid tombstone
rows stop at retail's flags gate; reserved rows and any valid tombstone fail closed. No handle is
unwrapped: a reached valid row without a live handle is a typed preflight fault.

`GameDaemon::process_all` invokes this preflight only at retail's exact signed
`frame % 100 == 33` cadence. Fog option 3 remains an early success that reads no authority,
does not set `busy`, and mutates no fog plane. A scheduled ordinary refresh that completes the
Unit preflight returns the named `GameDaemonUpdateAllSeen` gap with
`IncompleteProducer { prepared_unit_stamps, residuals }`; the previous `seen`, `seen2`,
`seen3`, `WData::was_seen`, and `wcoord_seen` bytes remain intact. Thus the former raw-`mylos`,
hardcoded-`detector:false` tick implementation is no longer a producer of checksum-visible
state.

Allocation owners can call `Sim::materialize_step12_object_init` immediately after the
canonical Unit row and type index exist; load or explicit instance mutation can instead call
`record_step12_authoritative_instance`. `replace_step12_visibility_type_source`,
`replace_step12_visibility_leader`, and `retire_step12_visibility_unit` expose the other
revisioned transactions. Canonical `spawn_unit`, BHS creation, despawn, and load owners still
need to call those hooks atomically; until then, their absence is detected during preflight.

The authority digest is diagnostic and deliberately separate from retail checksum channel 12.
Channel 12 continues to come from World section 6, whose visibility planes are unchanged on a
refused refresh. The existing save format also cannot reconstruct detector-init provenance,
HeroesData, or the synchronized type composition: saving a non-default Step-12 authority is
therefore rejected as `Unsupported("step-12 visibility authority")` rather than silently
serializing guessed state. No competing save version or chunk was allocated for this tranche.

The exact residual transaction owners are nonempty dedicated Wall vision,
`World::reveal_fog`, scenario reveal points, the `Build::close` and three Scenario direct entry
routes, and incremental `Object::update_seen(1)`. The `Game::run` direct route and its
frame-zero alliance explored-sharing tail now execute against canonical saved owners, as do
scheduled active Build/Unit local-seen bodies. The general row remains blocked until all
reached contributors and side effects can preflight and commit atomically.
