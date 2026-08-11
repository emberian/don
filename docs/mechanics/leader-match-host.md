# Canonical Leaders/Match terminal host

## Result

`crates/don-sim/src/systems/leader_match_host.rs` is now the single staged transaction
for the state part of `Leader::victory(int,int)` at `0x006EC9B0` and
`Leader::defeat(int,int,int)` at `0x006ECB00`.

It operates on the existing `victory_score::Leaders` and `victory_score::Match` owners.
There is no smaller lifecycle table and no Arena-specific victory state machine. A request
is rejected before mutation if it cannot name `leaders[8]`; otherwise both owners are
cloned, the recovered body runs, and both are published together. Its receipt carries
Adler-32 before/after values for the owned Leader and Game walks, the exact emitted match
events, and the Build/Unit owner masks requested by recursive wins and defeats.

The pair transaction deliberately does not consume Build/Unit cleanup requests. A
concrete product owns those object stores. `Sim::apply_leader_match_transaction` publishes
the pair and immediately drains them through the executable tick adapter; the Arena
consumes the same masks against its own queues.

## Existing command host, consolidated

The lifecycle wire was not missing: `systems/lifecycle_host.rs` already decoded opcodes
70/71/80, planned `Player::resign` / `quit` / `drop`, and reached real terminal state.
That implementation had called `Leaders::defeat` directly. It now calls the canonical
pair transaction at the exact `LifecycleCall::LeaderDefeat` position, preserving the
instruction-ordered semaphore effects and its existing receipt. The focused test drives
a decoded opcode-70 packet and observes a changed Leader channel, `DEFEAT_RESIGN`, the
survivor's win, and `GAME_OVER`.

## Arena shared victory

`ArenaDiplomacy` now persists an `ArenaLeaderMatch` containing the same recovered pair.
The existing `DiplomacyState` and ally-mask rows remain the Arena's object/fog projections,
but a hosted declaration is staged across all three stores. Before planning, the adapter
proves the pair agrees with every `Leader::set_diplo` input bit, raw declaration,
ally-mask byte, and Game semaphore word. It then applies both declarations and shared
vision, executes `SetDiploAuthority::Victory` through the canonical host, applies
`SetVictoryBit22`, validates the original receipt including the authority acknowledgement,
and publishes the staged owners.

The pure planner without a host continues to return `UnhostedVictory`; this preserves the
representation-neutral boundary test. A live two-player Arena now commits the alliance,
marks both leaders `WON`, retains semaphore bit 22, and clears the exact winner queue mask.

## DoNSave v11

Format 11 adds chunk `0x000A` (`LEADER_MATCH`). It stores:

- Match options, constants, category tables, clocks, score/victory scalars, team counts,
  semaphores, and World-size inputs;
- the static score type table and every mutable field in all eight `LeaderState` rows;
- pending match events; and
- the optional live lifecycle `PlayerTable` used by opcodes 70/71/80.

The format does not serialize the immutable PlayerSetup receipt twice. `PLAYER_SETUP`
still retains its request and reconstructs that receipt at frame zero. The loader then
installs the saved World/map owners and restores the mutable v11 pair over the receipt.
Setup-time semaphore facts are derived only from the receipt fields that the setup body
read; current game-over/quit bits come from `LEADER_MATCH` and are never fed backwards into
setup.

Formats 7 through 10 remain accepted. A loaded v11 state must itself pass the save gate and
resave byte-identically. The writer refuses a mismatched World/Match clock, malformed fixed
leader/type vectors, invalid identities, or any undrained terminal cleanup request.

This closes the PlayerSetup frame-zero persistence boundary. It does not claim an `.svx`
writer, nor make an arbitrary executed frame saveable: dynamic step-8 AI, visibility,
projectile, Wonder, wall/herd, and installed-rule owners retain their typed refusals.

## Evidence

- `crates/don-sim/tests/leader_match_host.rs`
- `crates/don-sim/tests/save_load_leader_match.rs`
- `crates/don-sim/tests/victory_endgame_wire.rs`
- `crates/don-ai/tests/arena_diplomacy_runtime.rs`
