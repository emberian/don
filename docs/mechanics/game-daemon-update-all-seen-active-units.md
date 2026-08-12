# `GameDaemon::update_all_seen` — bounded active Build-plus-Unit tick path

## Executable cohort

This tranche executes a substantive active-leader path through the shipped 1,221-byte
`GameDaemon::update_all_seen` body at `0x00732840`. It is not a closure claim for the general
producer. The preflight admits a scheduled phase-33 refresh only when all of these facts are
true before `GameDaemon::busy` or a World plane changes:

1. every active owner's canonical sparse Build band starts at 2000 and resolves each retained
   slot to the exact save-owned `BuildData {who,o}` row, while its dedicated Wall mark remains
   the retail-empty `[3000,3000)` band;
2. when `Game+0x821 bit 4` or `Game+0x822 bit 1` enables scenario reveal points, the
   canonical eight-array owner and exact VALID leader slice are present;
3. frame-zero alliance explored sharing has its canonical leader/options context;
4. invalid retained Build rows take the PE's two false validity virtuals and skip; each valid
   incomplete row resolves its canonical current type through `LiveProductionRuntime`, and an
   ordinary non-Wonder or an unstarted Wonder takes the PE's exact no-visibility return;
5. every active Build resolves its current type before LOS, reproducing
   `Object::update_seen`'s `is_wonder` / captured / `is_fort` / started preamble; a
   missing reached fact refuses rather than treating the row as ordinary;
6. a reached `Wall::update_local_seen` carries exact `ObjectTypeData +0x234/+0x238`
   footprint. Its complete body composes the all-player or relation mask and writes the
   canonical World planes in x-outer/y-inner tile order through `World::set_locally_seen`;
7. each admitted active Build gets signed LOS from `ObjectData::mylos +0x3C`; negative LOS
   refuses, zero returns before `visible`, and positive LOS executes the local-seen virtual
   when `visible +0x40` is nonzero or the preamble selects a started Wonder, then requires an
   `infiltrated +0x3A` recipient in `0..=7`;
8. the existing revision-bound Unit authority resolves every active owner-local Unit row,
   detector byte, LOS/general result, small-radius projection, extra explored recipient, and,
   when `visible` is nonzero, exact `UnitTypeData +0x234` local-seen radius;
9. `Unit::update_local_seen` walks the canonical circle table from the Unit's unprojected
   position, stamps the supplied visibility mask through `World::set_locally_seen`, and then
   yields to the ordinary object stamp; and
10. every newly explored cell's `World::reveal_fog` call is proven mutation-free: its centre
   TData tile has no `RESOURCE 0x0200`, and its WData cell has neither `OIL 0x0800` nor item
   bit `0x8000`.

The last condition follows the exact branch order in `World::reveal_fog` `0x006B3D30`. With
those three gates clear, the rare-Good and oil-patch branches are skipped, then the signed-clear
item flag returns before either Item-ever-seen or the source-Unit `get_goody_box` suffix.
Territory ownership is not a gate in this body.

## PE stage map and transaction

The executed stage map is:

| retail stage | exact live action |
|---|---|
| `0x00732840` option-three entry | existing cadence preflight; no-mutation return retained |
| `0x0073285D` | store `GameDaemon::busy = 4` |
| `World::clear_seen` `0x006B2250` | clear canonical `seen` and `wcoord_seen` |
| `memset(World+0x164)` | clear canonical `seen3` |
| Build vtable `0x00B42174`, `+0x0C/+0x10` | visit each canonical sparse Build identity; invalid retained rows skip |
| `BuildData::is_wonder` `0x00472320`, active `0x00472350`, started `0x00472360` | resolve valid incomplete type identity; ordinary incomplete and unstarted Wonder rows skip, while complete Builds take `Object::update_seen` |
| `Wall::update_local_seen` `0x0063ED50` | for any reached Build call, compose the exact all-player or relation mask and walk its canonical footprint in retail order |
| `WallData::tile_corner` `0x00643440` | derive footprint corner from canonical Build position and exact type `x_size/y_size`; reject off-map or malformed type facts during preflight |
| `World::set_locally_seen` `0x006B4BB0` | write `seen2` and `WData::was_seen`, plus `seen` and `wcoord_seen` unless the captured/fort explored-only flag is set; `seen3` is untouched |
| `WallData::los` `0x0063FA50` | read signed canonical `ObjectData::mylos +0x3C`; zero LOS returns before `visible` |
| dedicated Wall outer pass | exact retail structural band `[3000,3000)` proves zero rows; any nonempty Wall band refuses |
| Unit outer pass | traverse the opaque prepared Unit rows in retail owner/object order |
| `Object::update_seen(0)` `0x00651B80` | run the Build type preamble, then stamp Builds before Units using each exact resolved LOS, centre, detector prefix, full circle-table order, and infiltrated `set_was_seen` suffix |
| `Unit::update_local_seen` `0x0060E410` | when Unit `visible` is nonzero, walk `circle_radius[UnitTypeData::x_size]` from the original position and stamp that exact mask before the ordinary disc |
| `World::reveal_fog` `0x006B3D30` | retain exact call chronology; all calls take the preflight-proven no-effect path |
| scenario reveal points | when enabled, stamp the first canonical point array for every VALID leader, preserving retail's repeated-array-zero quirk |
| frame-zero sharing | execute for direct frame-zero routes; scheduled phase 33 skips |

Preflight clones persistent `seen2` to reproduce Build/Unit local-seen exploration and the
exact later first-exploration chronology. It validates every local-seen cell, reached
`reveal_fog` cell, and residual gate before publishing anything. The commit then performs the
real canonical plane clear and replays the opaque Build action sequence—local footprints and
active Build stamps interleaved in sparse object order—then each Unit local disc immediately
before that Unit's ordinary stamp.
A chronology assertion binds the preflight to the commit. No danger/visibility sidecar is
introduced.

## Atomicity, checksum, and save/resume proof

`game_daemon_update_all_seen_tick.rs` runs the real 29-step `Sim::do_frame` and proves:

- an active empty leader executes the exact zero-row member of this cohort;
- a canonical active-complete Build stamps nonempty `seen` and persistent `seen2`, with Object
  flag `0x40` driving the freshly cleared detector `seen3` plane;
- an incomplete ordinary Build executes without a vision write, while a 6x6 started Wonder
  executes 64 exact tile visits across its one-tile perimeter, folding onto 5x5 fog cells,
  changes the World checksum, and
  survives save/reload plus a resumed frame byte-for-byte;
- a live canonical Unit writes nonempty `seen`, persistent `seen2`, and `wcoord_seen`, while a
  nondetector leaves the freshly cleared `seen3` plane empty;
- an ordinary Build with a relation mask, an implicit active Wonder with a zero `visible` byte,
  and a Unit with a type-sized local disc each execute their retail local-seen path; the Build
  and Unit witnesses preserve checksum and resumed-frame byte equality across save/load;
- the canonical World checksum changes;
- after removing only the reinstallable type/instance authority, save/load preserves that
  checksum and the next resumed frame converges in channel digest and byte-identical save;
- a missing reached Build current type, missing local footprint, absent reached fort predicate,
  malformed footprint, invalid Unit local radius, or resource-cell reveal refuses before daemon
  or plane mutation;
- a zero-LOS Build returns before its deliberately nonzero `visible` byte is read.

## Remaining red boundary

The gap remains red. A nonempty dedicated Wall band, effectful reveal, `Build::close`,
`set_explored_show_buildings`, or incremental entry fails closed. The paired
`add_visibility`/`remove_visibility` routes and scenario reveal-point pass are mounted in
`scenario-visibility-direct.md`; `Game::run`'s frame-zero explored sharing is mounted in
`game-daemon-update-all-seen-frame-zero.md`. Missing current Build type, reached footprint/fort
facts, or Unit local radius also refuses atomically rather than inventing an ordinary row.
Those remaining bodies need their complete canonical owners and the same
preflight/commit discipline before the general `update_all_seen` row can close.

## Focused gate

```sh
cargo test -p don-sim --test step12_visibility_producer_frontier
cargo test -p don-sim systems::step12_visibility_runtime --lib
cargo test -p don-sim --test game_daemon_update_all_seen_tick
cargo test -p don-sim systems::game_daemon_step12 --lib
```

The stage map was audited against `re/decomp-all/00732840.c`,
`re/decomp-all/00651b80.c`, `re/decomp-all/0060e410.c`, `re/decomp-all/0063ed50.c`,
`re/decomp-all/00643440.c`, `re/decomp-all/006b4bb0.c`, `re/decomp-all/0063fa50.c`,
`re/decomp-all/006b3d30.c`, exact shipped Build/Wall vtable bytes, the existing PE/PDB
procedure census, and the canonical sparse-object, Unit-column, World, Fog, and semaphore
owners named above. The installed retail `buildingrules.xml` independently fixes Space Program
type `0x21E` at a 6x6 footprint for the save/resume witness.
