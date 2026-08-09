# Late fixed-size command state plans

This proof pack isolates two still-red `CommandTypes` rows without changing the shared
command bridge.  It is Tier C recovery from the supported Extended Edition executable and
the shipped PDB, not a differential or formal-equivalence claim.  Unless stated otherwise,
the addresses, layouts, branches, and constants below are **[measured]** directly from those
two artifacts.

| opcode | handler | recovered boundary | closure status |
|---:|---|---|---|
| 77 | `process_cannon_time` `0x009464F0` | player→leader indirection, use counter, five TurnControl fields, denial/start branches, exact ordered sound/UI requests | planner only; not wired |
| 80 | `process_ungraceful_player_drop` `0x00943EA0` | raw byte fields and exact `Game+0x820 & 0x10` network gate | still red: `DropControl::process_drop` tail is delegated, not ported |

The new source is `crates/don-sim/src/systems/late_command_plans.rs`; path-import mutation
tests live at `crates/don-sim/tests/late_command_plans.rs`.  It is deliberately absent from
`systems/mod.rs` until root convergence can wire it atomically with `command.rs`.

## Opcode 77: cannon time

The wire `state` byte is diagnostic only.  `process_cannon_time` logs it with `Game.frame`,
then uses signed `CommandPackage::play` to load the issuing `PlayerData` row and zero-extends
the row's `who` byte at `+0x77`.  That leader identity—not the package slot and not the wire
byte—is passed to `TurnControl::start_cannon_time`.

The success path at `0x009565E4..0x009567AD` performs these deterministic writes in order:

1. decrement unsigned `LeaderData::cannon_time`;
2. set active player (`TurnControl+0x24`);
3. copy signed `Game.frame` to start frame (`+0x28`);
4. copy current speed (`+0x30`) to saved speed (`+0x2C`);
5. copy `Game+0x560` to `TurnControl+0x44`;
6. request `SoundGlobal::play(0x139)`;
7. if speed was nonzero, set speed to zero, retarget wall-clock pacing to 200, update UI,
   and request `SoundGlobal::play(0x156)`.

An already-active interval or zero remaining uses is silent for a remote leader.  The same
denial for the display leader emits the matching UI notice followed by
`SoundGlobal::play(0x40)`.  The active denial does not read the use counter.

Sound requests are typed, ordered effects.  The planner consumes no random number.  The
adapter must execute each request through the retail-compatible `SoundGlobal::play` path,
whose category table and half-open `Random::get` call own the sound RNG stream.  UI text and
wall-clock retargeting do not enter headless simulation state.

## Opcode 80: ungraceful drop

The handler logs the zero-extended `play` and `state` bytes.  Only when
`Game+0x820 & 0x10` is nonzero does it call
`DropControl::process_drop(play, state)` at `0x00959500`.  Solo mode is therefore an exact
post-diagnostic no-op.

The network branch remains deliberately incomplete.  `process_drop` contains state-specific
player-validity scans, diplomacy resets, connection/UI work, and a special state-3 last-peer
transition.  This pack retains a typed `DelegateToDropControl` effect so those semantics
cannot be replaced by a boolean or silently skipped.  Opcode 80 must stay red until that
receipt is recovered and wired.

## Explicit exclusions

Opcodes 70/71 (`Player::resign`/`Player::quit`) cross victory, diplomacy, restart flags, log,
audio, and application-quit callbacks; opcode 73 fans option changes through live unit and
building pools; opcode 75 is owned by the independent object-command lane; opcode 78 invokes
the console command interpreter.  They were not folded into this low-dependency pack.  No
opcode table or generated closure status is changed here.
