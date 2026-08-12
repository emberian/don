# `GameDaemon::update_all_seen` — bounded active Build-plus-Unit tick path

## Executable cohort

This tranche executes a substantive active-leader path through the shipped 1,221-byte
`GameDaemon::update_all_seen` body at `0x00732840`. It is not a closure claim for the general
producer. The preflight admits a scheduled phase-33 refresh only when all of these facts are
true before `GameDaemon::busy` or a World plane changes:

1. every active owner's canonical sparse Build band starts at 2000 and resolves each retained
   slot to the exact save-owned `BuildData {who,o}` row, while its dedicated Wall mark remains
   the retail-empty `[3000,3000)` band;
2. neither `Game+0x821 bit 4` nor `Game+0x822 bit 1` enables scenario reveal points;
3. frame is nonzero, so the frame-zero alliance explored-sharing tail is unreachable;
4. invalid retained Build rows take the PE's two false validity virtuals and skip; each valid
   incomplete row resolves its canonical current type through `LiveProductionRuntime`, and an
   ordinary non-Wonder or an unstarted Wonder takes the PE's exact no-visibility return;
5. a valid started Wonder carries exact `ObjectTypeData +0x234/+0x238` footprint and, when
   uncaptured, the reached `BuildTypeData::is_fort` fact; the captured branch returns before
   that virtual. Its complete `Wall::update_local_seen` body writes the canonical World planes
   in x-outer/y-inner tile order through `World::set_locally_seen`;
6. each admitted active Build gets signed LOS from `ObjectData::mylos +0x3C`; negative LOS refuses,
   zero returns without reading `visible`, and positive LOS requires `visible +0x40 == 0` and an
   `infiltrated +0x3A` recipient in `0..=7`;
7. the existing revision-bound Unit authority resolves every active owner-local Unit row,
   detector byte, LOS/general result, small-radius projection, and extra explored recipient;
8. every Unit that stamps has `ObjectData::visible +0x40 == 0`, avoiding the unrecovered
   virtual `Unit::update_local_seen`; and
9. every newly explored cell's `World::reveal_fog` call is proven mutation-free: its centre
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
| `Wall::update_local_seen` `0x0063ED50` | for a started Wonder, compose the exact all-player or captured/fort mask and walk its canonical footprint in retail order |
| `WallData::tile_corner` `0x00643440` | derive footprint corner from canonical Build position and exact type `x_size/y_size`; reject off-map or malformed type facts during preflight |
| `World::set_locally_seen` `0x006B4BB0` | write `seen2` and `WData::was_seen`, plus `seen` and `wcoord_seen` unless the captured/fort explored-only flag is set; `seen3` is untouched |
| `WallData::los` `0x0063FA50` | read signed canonical `ObjectData::mylos +0x3C`; zero LOS returns before `visible` |
| dedicated Wall outer pass | exact retail structural band `[3000,3000)` proves zero rows; any nonempty Wall band refuses |
| Unit outer pass | traverse the opaque prepared Unit rows in retail owner/object order |
| `Object::update_seen(0)` `0x00651B80` | stamp Builds before Units, using each exact resolved LOS, centre, detector prefix, full circle-table order, and infiltrated `set_was_seen` suffix |
| `World::reveal_fog` `0x006B3D30` | retain exact call chronology; all calls take the preflight-proven no-effect path |
| scenario reveal points | skip because both exact Game gates are clear |
| frame-zero sharing | skip because scheduled phase 33 is nonzero |

Preflight clones persistent `seen2` to reproduce both started-Wonder footprint exploration and
the exact later first-exploration chronology. It validates every footprint cell, reached
`reveal_fog` cell, and residual gate before publishing anything. The commit then performs the
real canonical plane clear and replays the opaque Build action sequence—Wonder local footprints
and active Build stamps interleaved in sparse object order—before each prepared Unit stamp.
A chronology assertion binds the preflight to the commit. No danger/visibility sidecar is
introduced.

## Atomicity, checksum, and save/resume proof

`game_daemon_update_all_seen_tick.rs` runs the real 29-step `Sim::do_frame` and proves:

- an active empty leader executes the exact zero-row member of this cohort;
- a canonical active-complete Build stamps nonempty `seen` and persistent `seen2`, with Object
  flag `0x40` driving the freshly cleared detector `seen3` plane;
- an incomplete ordinary Build executes without a vision write, while a 6x6 started Wonder
  executes 36 exact tile calls folding onto 3x3 fog cells, changes the World checksum, and
  survives save/reload plus a resumed frame byte-for-byte;
- a live canonical Unit writes nonempty `seen`, persistent `seen2`, and `wcoord_seen`, while a
  nondetector leaves the freshly cleared `seen3` plane empty;
- the canonical World checksum changes;
- after removing only the reinstallable type/instance authority, save/load preserves that
  checksum and the next resumed frame converges in channel digest and byte-identical save;
- a missing incomplete-Build current type, missing started-Wonder footprint facts, a reached
  but absent fort predicate, a positive-LOS active Build with nonzero `visible`, a resource-cell
  reveal, or a Unit with nonzero `visible` refuses before daemon or plane mutation;
- a zero-LOS Build returns before its deliberately nonzero `visible` byte is read.

## Remaining red boundary

The gap remains red. A nonempty dedicated Wall band, active-Build or Unit local-seen call,
effectful reveal, scenario reveal-point pass, direct/incremental entry, or frame-zero explored
sharing fails closed. Missing current Build type or reached Wonder footprint/fort facts also
refuse atomically rather than inventing an ordinary row. Those bodies need their complete
canonical owners and the same
preflight/commit discipline before the general `update_all_seen` row can close.

## Focused gate

```sh
cargo test -p don-sim --test step12_visibility_producer_frontier
cargo test -p don-sim systems::step12_visibility_runtime --lib
cargo test -p don-sim --test game_daemon_update_all_seen_tick
cargo test -p don-sim systems::game_daemon_step12 --lib
```

The stage map was audited against `re/decomp-all/00732840.c`,
`re/decomp-all/00651b80.c`, `re/decomp-all/0063ed50.c`,
`re/decomp-all/00643440.c`, `re/decomp-all/006b4bb0.c`, `re/decomp-all/0063fa50.c`,
`re/decomp-all/006b3d30.c`, exact shipped Build/Wall vtable bytes, the existing PE/PDB
procedure census, and the canonical sparse-object, Unit-column, World, Fog, and semaphore
owners named above. The installed retail `buildingrules.xml` independently fixes Space Program
type `0x21E` at a 6x6 footprint for the save/resume witness.
