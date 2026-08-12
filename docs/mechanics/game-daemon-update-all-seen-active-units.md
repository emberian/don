# `GameDaemon::update_all_seen` — bounded active Unit tick path

## Executable cohort

This tranche executes a substantive active-leader path through the shipped 1,221-byte
`GameDaemon::update_all_seen` body at `0x00732840`. It is not a closure claim for the general
producer. The preflight admits a scheduled phase-33 refresh only when all of these facts are
true before `GameDaemon::busy` or a World plane changes:

1. every active owner's canonical sparse Build and Wall marks are exactly their empty band
   bases, 2000 and 3000;
2. neither `Game+0x821 bit 4` nor `Game+0x822 bit 1` enables scenario reveal points;
3. frame is nonzero, so the frame-zero alliance explored-sharing tail is unreachable;
4. the existing revision-bound Unit authority resolves every active owner-local Unit row,
   detector byte, LOS/general result, small-radius projection, and extra explored recipient;
5. every Unit that stamps has `ObjectData::visible +0x40 == 0`, avoiding the unrecovered
   virtual `Unit::update_local_seen`; and
6. every newly explored cell's `World::reveal_fog` call is proven mutation-free: its centre
   TData tile has no `RESOURCE 0x0200`, its WData cell has neither `OIL 0x0800` nor item bit
   `0x8000`, and the source Unit lacks auto-explore mask `0x100`.

The last condition follows the exact branch order in `World::reveal_fog` `0x006B3D30`. With
those four gates clear, the rare-Good, oil-patch, Item-ever-seen, and source-Unit
`get_goody_box` mutations are all skipped. Territory ownership is not a gate in this body.

## PE stage map and transaction

The executed stage map is:

| retail stage | exact live action |
|---|---|
| `0x00732840` option-three entry | existing cadence preflight; no-mutation return retained |
| `0x0073285D` | store `GameDaemon::busy = 4` |
| `World::clear_seen` `0x006B2250` | clear canonical `seen` and `wcoord_seen` |
| `memset(World+0x164)` | clear canonical `seen3` |
| Build/Wall outer passes | visit active leaders; exact sparse base marks prove zero rows |
| Unit outer pass | traverse the opaque prepared Unit rows in retail owner/object order |
| `Object::update_seen(0)` `0x00651B80` | use the exact resolved LOS, projected centre, detector prefix, full circle-table order, and infiltrated `set_was_seen` suffix |
| `World::reveal_fog` `0x006B3D30` | retain exact call chronology; all calls take the preflight-proven no-effect path |
| scenario reveal points | skip because both exact Game gates are clear |
| frame-zero sharing | skip because scheduled phase 33 is nonzero |

Preflight clones only persistent `seen2` to reproduce the exact first-exploration chronology.
It validates every reached `reveal_fog` cell and all residual gates before publishing anything.
The commit then performs the real canonical plane clear and calls the existing exact
`borders_fog::update_seen` body for each prepared stamp. A chronology assertion binds the
preflight to the commit. No danger/visibility sidecar is introduced.

## Atomicity, checksum, and save/resume proof

`game_daemon_update_all_seen_tick.rs` runs the real 29-step `Sim::do_frame` and proves:

- an active empty leader executes the exact zero-row member of this cohort;
- a live canonical Unit writes nonempty `seen`, persistent `seen2`, and `wcoord_seen`, while a
  nondetector leaves the freshly cleared `seen3` plane empty;
- the canonical World checksum changes;
- after removing only the reinstallable type/instance authority, save/load preserves that
  checksum and the next resumed frame converges in channel digest and byte-identical save;
- a resource-cell reveal, nonzero `visible`, or source auto-explore bit refuses before daemon or
  plane mutation.

## Remaining red boundary

The gap remains red. Any nonempty active Build/Wall band, started-Wonder/local-seen call,
effectful reveal, scenario reveal-point pass, direct/incremental entry, or frame-zero explored
sharing fails closed. Those bodies need their complete canonical owners and the same
preflight/commit discipline before the general `update_all_seen` row can close.

## Focused gate

```sh
cargo test -p don-sim --test step12_visibility_producer_frontier
cargo test -p don-sim systems::step12_visibility_runtime --lib
cargo test -p don-sim --test game_daemon_update_all_seen_tick
cargo test -p don-sim systems::game_daemon_step12 --lib
```

The stage map was audited against `re/decomp-all/00732840.c`,
`re/decomp-all/00651b80.c`, `re/decomp-all/006b3d30.c`, the existing shipped PE/PDB
procedure census, and the canonical sparse-object, Unit-column, World, Fog, and semaphore
owners named above.
