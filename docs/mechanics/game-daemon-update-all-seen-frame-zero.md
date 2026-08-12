# `Game::run` frame-zero visibility refresh

## Executed retail body

The shipped `Game::run` body calls `GameDaemon::update_all_seen` directly at
`0x0058525D`. This route bypasses `GameDaemon::process_all`'s signed phase-33 cadence. When
`Game::frame +0x550` is zero, it therefore reaches the explored-sharing tail at
`0x00732BD8..0x00732CF3` after the ordinary Build, Unit, and scenario-point passes.

The implementation is instruction-derived from the supported PE (SHA-256
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`) and the matching
PDB identities. It is fidelity tier C: the Rust transaction has executable mutation tests,
but this tranche did not run the body against retail machine code.

## Exact set construction

Retail walks active Leader slots `0..7`. For each active outer slot `slot`, it starts a
one-byte set with `1 << slot`, then scans target slots `0..7`, excluding `target == slot`.
A target joins when any of these instruction-ordered predicates succeeds:

1. `target == leader.who`;
2. both directed cells are alliance: `leader.diplos[target] == 2` and
   `leaders[target].diplos[leader.who] == 2`; or
3. `leader.ally_mask & (1 << target)` is nonzero.

The second predicate is deliberately mutual; a one-way alliance does not share exploration.
`ally_mask` is the independent `LeaderData +0x6929` fallback initialized by the shared-vision
setup loop.

An accumulated processed mask begins at zero. If the new set is already a subset of that
mask, retail skips it. Otherwise, when either `Game +0x2D` (`starting_resources`) or
`Game +0x30` (the fog/reveal option) is nonzero, retail scans every `World::seen2` byte. A
byte intersecting the set is replaced with `byte | set`. The set then joins the processed
mask. This tail writes no current visibility, detector, coarse visibility, or
`WData::was_seen` byte.

## Canonical transaction

`Sim::game_run_update_all_seen` is the sole mounted direct route in this tranche. It binds:

- active object-owner gates to the corresponding saved `LeaderState::leader_flags`;
- `LeaderState::{who,diplos,init_diplomacy.ally_mask}` from the canonical Leader/Match owner;
- `MatchOptions::starting_resources` and canonical `FogOption(Game+0x30)`; and
- the checksum-owned World visibility planes.

The existing Build/Unit/reveal-fog preflight runs first. It applies the sharing sets to the
same cloned `seen2` after-image that already models those earlier stages. Only after all
leader identities, relations, options, cells, and earlier visibility effects pass does commit
store `GameDaemon::busy = 4`, clear current visibility, replay object stamps, and finally OR
the prepared sets into canonical `seen2`. Leader activity disagreement or any earlier residual
returns before the daemon or World changes.

The integration witness uses two frame-zero allied leaders with separately explored cells. It
asserts `{1,2} -> {3,3}`, the exact option gate, one-way-alliance rejection, `ally_mask`
fallback, unchanged `WData::was_seen`, World-checksum movement, save/reload equality, and a
byte-identical resumed direct refresh.

## Remaining red boundary

This is not a general direct-entry or visibility closure. `Build::close` and the three
Scenario direct callers are not routed through canonical owning actions. Scenario reveal
points and effectful `World::reveal_fog` still lack a complete Sim/save owner. A nonempty
dedicated Wall band remains invalid for this supported executable: retail initializes the
band as `[3000,3000)` and has no writer that grows it.

Focused gate:

```sh
tools/swarm-cargo cycle9-step12 test -p don-sim \
  --test game_daemon_update_all_seen_tick
```
