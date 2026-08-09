# Completed Wonders and Wonder-victory points

Implementation: `crates/don-sim/src/systems/wonders.rs`, integrated at step 12 by
`crates/don-sim/src/tick.rs`.

Fidelity tier: **C**. The state and control flow below are derived from the shipped
`riseofnations.exe` and `rise.pdb`; no retail differential oracle case exists yet.

## Bounded scope

This tranche begins at the completed-Wonder call inside `Build::activate` and ends at the
two arrays consumed by the existing Wonder victory state machine. It implements:

- the caller-owned completion transaction: first-match unbuilt removal by swap-with-tail,
  wrapping `wonders_built` increment, completed-record registration, and the returned
  `BuildData::wonder` link;
- completed-record allocation/reuse and `LeaderData::wonder_mark` maintenance;
- `WonderData` initialization, map-scaled timer arithmetic, and the prerequisite-complete
  game-bit write;
- close/invalidate and trailing-mark compaction;
- the lifetime `LeaderData::wonders_held` high-water update;
- individual, allied-team, strongest-hostile-team, and net Wonder values;
- exact individual/allied completed and unbuilt Wonder counts used by leader/AI queries;
- completed-build close, including mandatory receipts for the leader dirty flag and the
  type-specific bonus/terrain recalculation remainder;
- capture transfer as retail performs it: receipt-confirmed generic build swap, new-owner
  completion, old-owner close, and receipt-confirmed `mask_me(1, 2)`;
- live tick supply to `GameDaemon::process_victory`.

It does not implement the rest of `Build::activate`, capture eligibility/combat,
`Build::swap_team`'s generic object copy, Wonder powers, or the full
`Wonders::walk_data`/replay bridge. Those generic building/world effects remain mandatory
host operations; the Wonder transaction refuses to commit locally without identity-bound
receipts.

## Retail evidence

| body / site | address | load-bearing behavior |
|---|---:|---|
| `Build::activate` call site | `0x00625B32..0x00625B64` | remove unbuilt record, increment `wonders_built`, call `Wonders::init_wonder(who,o)`, store returned short at `BuildData+0x76` |
| `UnbuiltWonders::remove_unbuilt_wonder` | `0x0073C220` | scan the owner's list for the first matching `o`; overwrite it with the final logical entry and decrement count; missing is a no-op |
| `Wonder::init` | `0x0073C5E0` | slot/object/frame/owner/valid writes, timer call, prerequisite word `|=1` |
| `Wonder::get_timer` | `0x0073C660` | map-width scaling with signed 32-bit arithmetic and round-half-up numerator |
| `Wonders::init_wonder` | `0x0073C860` | first inactive slot below mark, otherwise mark append/growth; active recount and `wonders_held=max(old,active)` |
| `Wonders::close_wonder` | `0x0073C7E0` | clear valid/who/o, trim inactive tail only, always return `-1` |
| `LeaderData::get_wonder_value` | `0x006EBB90` | sum the current object virtual value for every valid record below `wonder_mark` |
| `LeaderData::get_team_wonder_value` | `0x006DA990` | sum valid self plus mutual allies |
| `LeaderData::get_wonder_net` | `0x006EBB10` | own team total minus the largest non-allied valid leader's team total, clamped to zero |
| `Build::close` Wonder caller | `0x00628EE3..0x00628FE4` | close linked registry slot, set `UNIT_STATS_DIRTY`, run type-specific recalculation, clear `BuildData::wonder`, conditionally remove unbuilt entry |
| `Build::check_capture` success tail | `0x00627DFA..0x00627FBA` | `swap_team(new_owner)`, new `activate(0,1,0)`, old `close(0,-1,0)`, then new `mask_me(1,2)` |

PDB layout confirms `WonderData` is 16 bytes in field order: `wonder:i16`, `o:i16`,
`stamp:i32`, `timer:i32`, `wonder_flags:i8`, `who:i8`. `LeaderData::wonder_mark` is `+0x424`;
`wonders_built` and `wonders_held` are `+0x854` and `+0x858`.

The timer body reads `Constants+0xD08` (`WONDER_TIMER`) and `+0xD0C` (`WONDER_AGE`). For
each positive rule value it computes:

```text
scaled = max(1, (World::xs * rule + standard_map_xs/2) / standard_map_xs)
timer  = scaled(WONDER_TIMER)
       + (0x21e - completed_object.type_index) * scaled(WONDER_AGE)
```

The shipped `WONDER_AGE` is zero, but the recovered term remains executable rather than
being constant-folded away.

## Mandatory world boundary

Retail gets the completed object's type, prerequisite, and current Wonder value through
global object/type stores, then mutates a `Game` availability word. `WonderWorld` owns those
facts and that write in DoN. It has no fallback implementation.

Every query must return zero RNG draws and zero world writes. Initialization accepts the
external prerequisite-bit mutation only when its receipt matches owner, object, type, and
prerequisite identity, confirms the bit is set, reports exactly one world write, and reports
zero RNG draws. A missing or stale host leaves no local registry record. An active record
whose live value cannot be verified blocks the tick's entire victory sweep for that frame;
the error is retained in `Sim::wonder_error` and charged to `Gap::WonderValueWorld`. This
avoids silently cancelling a real Wonder countdown with invented zero points.

`WonderLifecycleHost` owns the generic caller effects which cannot be reproduced inside the
Wonder registry. It has no defaults. A capture swap receipt must match both owners, both
object indices, and the old Wonder slot, report zero RNG draws and at least one world write,
and confirm completion. Close must resolve a valid Wonder type, confirm the unit-stats dirty
write, and confirm type-specific recalculation exactly for types `0x20f`, `0x212`, `0x214`,
and `0x21c`. The final capture receipt must match the new object and exact `(1,2)` mask
arguments. Stale identities, zero-write claims, unexpected RNG, refused effects, or missing
receipts abort the local commit.

## Caller transaction and capture semantics

`Wonders::apply_build_lifecycle` is the single executable entry point for completion, close,
and capture. It stages the completed registry, unbuilt lists, `wonders_built`,
`wonders_held`, and each affected `BuildData::wonder` field. Those local values commit only
after all required receipts validate. External host effects cannot be rolled back by this
module, so a later refusal is surfaced as an incomplete lifecycle error rather than being
reported as success.

Capture does not transfer or retag the old `WonderRecord`. Retail first runs ordinary
activation for the new build, which removes any matching new-owner unbuilt entry, increments
the new owner's lifetime `wonders_built`, allocates a new completed slot, and raises that
owner's `wonders_held` high-water if necessary. It then closes the old owner's record and
clears the old build link. The old owner's `wonders_built` and `wonders_held` remain lifetime
statistics.

## Executable integration

`Sim::game_daemon_process_all` now supplies `process_victory` from the completed registry in
Standard, Sudden Death, and Wonder modes. With no active completed Wonder, zero arrays remain
exact and no host is required. With an active Wonder, values are queried live each frame, so
a changed object/type result immediately changes qualification rather than using a cached
completion-time score. The retail mode gate is executable too: Wonder and Sudden Death are
immediate; Standard counts down only in multiplayer/recorded play, with prerequisite
`0x2B9` providing the recovered alliance-wide bypass. When a win fires, the victory lane
also reproduces the aggregate `Build::clean_queue(0)` effects and defeated-player
`Game::check_victory` tail, so the match reaches both game-over and victory-resolved
semaphores.

The integration regression begins below threshold, changes the mandatory host's live value,
arms the Standard-mode countdown on the next frame, and wins on the exact map-scaled expiry.
A companion regression removes the host after registration and proves that no Wonder winner
or countdown mutation occurs.

## Tests and remaining promotion work

Focused tests cover first-match unbuilt swap-removal, missing-unbuilt no-op behavior,
wrapping `wonders_built`, caller-link storage, transactional rollback on refused completion,
mandatory close effects, malformed close receipts, capture ordering and lifetime counters,
stale swap identity, failed final mask, slot reuse, middle versus tail closure, preserved
retired fields, `wonders_held` high-water behavior, timer/type sensitivity, mutual-alliance
semantics, strongest-hostile subtraction, live-value changes, stale receipts, mutating
queries, refused external writes, exact countdown expiry, and missing-host refusal.

Promotion beyond Tier C needs a retail oracle or live capture for lifecycle/registry bytes and
value/net outputs, a concrete generic build-swap/dirty/recalculation/mask host, and a complete
Wonder walker/replay-channel owner.
