# `Leader::init` active-team diplomacy prefix

## Recovered boundary

This Tier-C, instruction-derived tranche covers one option-independent branch of shipped
`Leader::init(int,int,int)` at `0x006E3930` (6,102 bytes) in
`ron-bin/riseofnations.exe`, SHA-256
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`.
No retail function was executed.

For each target slot, the ordinary initialization path:

1. pushes `IsTeamArg::Zero` and the target index at `0x006E3C63..0x006E3C67`;
2. calls `LeaderData::is_team` at `0x006E3C6D`;
3. on true, selects diplomacy value 2 at `0x006E3C76..0x006E3C79`;
4. writes `LeaderData::diplos[target]` at `0x006E3CB4`.

`leader_init_diplomacy::plan_active_team_alliances` applies exactly that branch to the
complete active frame-zero PlayerSetup cohort. The plan binds the whole expected
`SetupDiplomacy` image; stale setup/team/diplomacy input refuses without mutation. It records
the directional ally masks and write count, including each active self cell.

This prefix remains a focused proof fixture. Product PlayerSetup now uses the complete sequential
loop described in `leader-init-diplomacy-loop.md`, which includes these same teammate writes plus
the non-team, inactive, treaty, interaction, and shared-vision effects. The browser's existing
read-only `game_diplomacy` query therefore still observes real teammate alliances, and its command
ingress refuses ATTACK packets whose target is not hostile before installing a core order.

## Honest red boundary

This is not complete product integration of `Leader::init`, `Leader::set_diplo`, or diplomacy
setup:

- the wider owner in `leader_init_diplomacy_loop.rs` recovers and now supplies PlayerSetup's false
  `is_team` arm and complete loop. Its byte at `Game+0x32` is
  `GameInfo+0x26` (`rush_rules`), and the exact callee is `LeaderData::starting_age`, not
  `GameInfo+0x32`/`Leader::get_age` as an earlier note stated;
- tribe, economy, type, scoring, production-script callbacks, and the remaining Leader
  initialization state in the 6,102-byte body remain red;
- later diplomacy commands still require the atomic `Leader::set_diplo` ejection, vision,
  victory, army, and event tail;
- DoNSave v11 reconstructs the immutable PlayerSetup recipe at frame zero, then restores the
  separately owned mutable Leaders/Match lifecycle. Supported post-step active matches now admit;
  unowned step-8 host answers and later subsystems still refuse before serialization.

The source tests cover alternating teams, free-for-all preservation, inactive rows/columns,
nonzero-frame and missing-leader refusal, and stale-plan atomicity. The PlayerSetup and native
browser ABI tests additionally prove the live Sim table and ATTACK ingress consume the receipt.

## Reproducible gates

```text
tools/swarm-cargo web-team-diplo test -p don-sim --test leader_init_diplomacy
tools/swarm-cargo web-team-diplo test -p don-sim --test player_setup_owner
cargo test --manifest-path web/wasm/Cargo.toml --lib game_abi::tests
node --check web/public/js/play/wasmgame.js
node --check web/public/js/play/client.js
node web/tools/check-play-wasm.mjs
node web/tools/play-smoke.mjs --json web/play-results.json
```
