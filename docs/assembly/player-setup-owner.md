# Sim-owned browser PlayerSetup convergence

## Closed boundary

The instruction-derived `team_setup_mutation` proof is now consumable by an executable
`don_sim::tick::Sim` without creating a lobby model in JavaScript. `PlayerSetupOwner` is embedded
in the authoritative victory leader table and retains the exact eight-record `PlayerSetup` image,
the fully applied `TeamSetupState`, and the ordered `InitTeamsReceipt` script-call plan.

`Sim::start_manual_player_setup` is one frame-zero transaction:

1. validate a nonempty roster, local-player membership, `team_style <= 12`, pristine setup owner,
   synchronized zero world/victory frames, and explicit team bytes;
2. reject ranked setup, random-team byte 5, negative/out-of-range teams, and latent teams on
   inactive slots;
3. materialize exact `PlayerSetup` and `LeaderTeamState` records and apply the recovered
   deterministic `Game::init_teams` body on a detached state image;
4. synchronize `MatchOptions::team_style`, `Match::on_team`, `num_sides`, and the team-scoring
   semaphore, install the owner, and only then call `Sim::activate` for the complete roster.

No fallible work follows the owner swap. A refusal therefore leaves the checksum digest, RNG,
victory state, leader flags, and setup owner unchanged. The transaction consumes no RNG.

## Browser ABI

`game_start_manual_teams` carries a four-slot active mask, one packed byte per team, team style,
local player, and an explicit ranked flag. Read-only `game_team_configured_mask` and
`game_team_style` queries prove what the Sim installed. `game_set_team` remains forbidden: there
is no incremental player setter, so the browser cannot create a partially configured roster.
`game_set_victory_mode` also remains forbidden because this tranche does not own victory setup.

The playable client exposes three deterministic presets (free-for-all, alternating 2v2, adjacent
2v2). It packs the selected preset only at manual start and immediately queries every team and the
active mask back from Wasm. Shared URLs and command-journal v3 reconstruct the same transaction;
v1/v2 journals map to the older inactive/own-slot-team baseline.

## Honest red boundaries

- The retained `InitTeamsReceipt` contains the exact ordered BHS callback facts; no browser BHS
  host is installed, so the callbacks are not claimed executed.
- `Game::init_teams` does not call `Leader::set_diplo`. Configured teammates therefore remain at
  the separately owned diplomacy declarations (war by default) until diplomacy initialization is
  integrated.
- The setup owner is not in DoNSave v6. `save_sim` rejects `player setup owner` before emitting
  bytes, and the browser keeps live-match save disabled.
- Random-team RNG, ranked ELO balancing, AI slots, and victory-mode mutation remain red.

## Frozen source validation handoff

Root convergence formatted the complete overlap. The first combined run exposed inactive owner
rows retaining Rust's zero team rather than retail `TEAM_AUTO`; the owner was corrected, not the
fixture. Persvati job `gen7-integration-batch-v3-20260809T234324Z-74734-7021-05c01f206acb`
then passed all five PlayerSetup tests. The native Wasm ABI passed all eight focused tests, the
rebuilt artifact passed its 76-export contract, and the full Chrome/WebGPU smoke passed. The
reproducible gates are:

```text
cargo test -p don-sim --test player_setup_owner
cargo test --manifest-path web/wasm/Cargo.toml --lib game_abi::tests
cargo run --manifest-path web/wasm/Cargo.toml --bin playcheck -- --help
node --check web/public/js/play/wasmgame.js
node --check web/public/js/play/client.js
node --check web/tools/play-smoke.mjs
node web/tools/check-play-wasm.mjs
node web/tools/play-smoke.mjs --json web/play-results.json
```

The checked-in `don_web.wasm` is the rebuilt owner-aware artifact; raw team/victory setters remain
forbidden.
