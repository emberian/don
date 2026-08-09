# Setup diplomacy: retail team query proof pack

This is the bounded convergence seam for the setup-owned team lookup used by victory,
Arena, browser setup, and replay reconstruction. It is **Tier C, instruction-derived**:
the supported retail code has not been called by an oracle, so this is not a differential
claim.

## Ground truth

Supported executable: PE32 SHA-256
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`.
Names and layouts come from the matching shipped PDB; behavior below comes from Capstone
disassembly of that executable, not from the decompiler.

| retail function | address | recovered contract |
|---|---:|---|
| `LeaderData::get_player` | `0x006EC0F0..0x006EC12B` | ordered eight-record lookup with a last-deferred-match fallback |
| `LeaderData::get_team` | `0x006EC040..0x006EC0EB` | signed setup team, including auto-team first-match resolution and the semaphore high-bit gate |
| `LeaderData::is_team` | `0x006EBD30..0x006EBE3F` | self, neutral-auto, frame-zero configured-team, and runtime-alliance branches |
| `LeaderData::is_ally` | `0x006EDB50` | self or two directional declarations exactly equal to `DIPLO_ALLY` (2) |

Load-bearing instruction observations:

- `get_player` reads `Player` records at stride `0x8C`, tests the `u16 flags` low bit,
  compares `Player+0x33 who` against `LeaderData+0x08 who`, and treats `flags & 0x50`
  as deferred rather than absent. A later clean match wins immediately; otherwise the
  last deferred match wins, with slot zero as the initial fallback.
- `get_team` repeats that lookup, returns 8 if the selected record lacks the low flag bit,
  and sign-extends `Player+0x34 team`. Only raw team 8 enters its candidate scan, and only
  while `Game+0x820` is nonnegative as `i8` (`bit 0x80` clear). Candidate leaders need
  only leader flag bit zero and are visited in slot order; the first `is_team(candidate,0)`
  wins.
- `is_team` compares `target` with `LeaderData::who`, not with the receiver's storage slot.
  Team style 7 rejects distinct players whose selected setup record is present and has
  raw team 8. When `Game::frame != 0`, argument zero dispatches directly to mutual
  `is_ally`. Otherwise setup teams compare as signed bytes and only 0 through 3 qualify.
  At nonzero frame, a nonzero second argument retains that configured result except for
  team styles 0, 8, and 11, which require mutual alliance too.

## Product seam

`crates/don-sim/src/systems/setup_diplomacy.rs` owns a fixed eight-player/eight-leader
`SetupDiplomacy` image. It deliberately depends on neither `World` nor the tick:

- replay can map `InitialState.info.settings.team_style`, all eight exact `InitialPlayer`
  flags/who/team records, `InitialState.game.frame`, and the reconstructed leader table;
- Arena can stop treating unequal owners as an implicit permanent FFA once it accepts a
  real setup descriptor;
- the browser can expose team setup only after its world constructor accepts the same
  descriptor, rather than changing the current honest `team unavailable` label first;
- `victory_score::Leaders::team_of` can materialize this query instead of returning `who`,
  closing its documented musical-chairs team-game stub.

The proof test imports the new source by path. This is intentional: the lane owns no
shared module list or leader struct, so convergence can wire it without resolving another
agent's simultaneous edits.

## Honest residuals

This pack does not make a complete diplomacy game:

- `Game::init_teams` `0x0058AE70` random-team assignment and initial declaration writes
  remain to be ported with the main RNG and team-style inputs.
- `Leader::action_declare` `0x006DAB50`, `action_respond` `0x006D03C0`, and the compact
  proposal/offer actions behind command opcodes 37 through 45 remain inert in the command
  bridge.
- `Leader::set_diplo` `0x006EC6A0` is 783 bytes and includes shared-vision changes,
  retargeting, victory checks, events, and presentation. A relation-matrix write is not a
  substitute for that transaction.
- `victory_score`, replay, Arena, and the browser still need adapters. Until convergence
  lands those callers, this is a source/test proof pack, not a reachable gameplay claim.
- No retail oracle or live-process comparison has promoted this above Tier C.

## Validation handoff

The authoring lane intentionally ran no compiler, formatter, test, or remote job. The
convergence owner should run:

```sh
tools/swarm-cargo-remote submit hbox setup-diplomacy \
  --path crates/don-sim/src/systems/setup_diplomacy.rs \
  --path crates/don-sim/tests/setup_diplomacy_retail_contract.rs \
  -- test -p don-sim --test setup_diplomacy_retail_contract
```

After adding the shared module/export and concrete adapters, also run the narrow tests for
`victory_score`, replay initial setup, Arena, and the wasm game before an umbrella gate.
