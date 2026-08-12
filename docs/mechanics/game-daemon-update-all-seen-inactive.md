# `GameDaemon::update_all_seen` — exact all-inactive tick path

## Scope

The shipped `GameDaemon::update_all_seen` body is `0x00732840`, 1,221 bytes.  The general
active-player producer is still open. A later tranche now executes the bounded active Build,
started-Wonder, and Unit cohort documented in `game-daemon-update-all-seen-active-units.md`;
nonempty Wall vision, active-object local-seen, effectful `World::reveal_fog`, scenario reveal
points, and frame-zero explored sharing remain outside both cohorts.

This tranche executes a different complete PE path through the same body.  When all eight
`LeaderData::flags & 1` gates are clear, retail reaches the following replay-visible mutations:

1. return without mutation when `Game +0x30 == 3`;
2. otherwise store `GameDaemon::busy = 4`;
3. call `World::clear_seen` `0x006B2250`, clearing `seen` and `wcoord_seen`;
4. clear `seen3` over `World::fog_size` bytes; and
5. visit and reject every later Build/Wall, Unit, scenario-point, and frame-zero alliance outer
   loop at its inactive-leader gate.

No unowned virtual, object registry, scenario record, diplomacy row, or reveal effect is read on
that path.  `step12_visibility_runtime::preflight_full_producer` therefore returns an opaque
`InactiveLeadersClear` plan when all eight canonical activity gates are false.

## Atomic tick seam

`Sim::game_daemon_process_all` prepares visibility before moving the daemon or region state into
the exact step-12 shell.  The shell callback receives the daemon's actual `busy` field, allowing
the option-three plan to leave it untouched and the entered all-inactive plan to store four
before clearing the canonical channel-12 World planes.  A failed active-leader preflight occurs
before the daemon countdown, victory child, fog clear, markets, regions, borders, collision
cursor, or Groups child.

`game_daemon_update_all_seen_tick.rs` proves through the real 29-step `Sim::do_frame`:

- non-empty `seen`, `seen3`, and `wcoord_seen` clear while persistent `seen2` survives;
- the World checksum changes and `busy` ends at four;
- save/load preserves the post-clear checksum and a resumed frame converges byte-for-byte; and
- an active empty leader takes the later exact empty Unit cohort rather than this inactive plan.

The gap remains red for active leaders.  This is an executable input cohort, not a closure flip
for the full active visibility producer.

## Provenance and focused gate

The stage map was checked against `re/decomp-all/00732840.c`,
`re/decomp-all/006b2250.c`, `schema/rise-procs.tsv`, and the shipped PE/PDB hashes already frozen
by the step-12 visibility lane.

```sh
cargo test -p don-sim --test game_daemon_update_all_seen_tick
cargo test -p don-sim systems::step12_visibility_runtime --lib
cargo test -p don-sim systems::game_daemon_step12 --lib
```
