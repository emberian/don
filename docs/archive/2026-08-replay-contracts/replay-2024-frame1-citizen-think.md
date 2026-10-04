# 2024 frame-1 Citizen `Unit::think -> think_peasant` frontier

Supported executable SHA-256: `30478a44d612d386c1ebb6b552d09c5e731e78e808102db6633ceb1a4a71fd6e`.

This tranche starts at the typed `IdleCitizenThinkRequest` left by the adjacent SetAnim
continuation. The receiver is still detached and `UnitData::unit_masks2 & 0x8000` is temporarily
clear. The hash-bound SetAnim-return whole Sim supplies the live actor coordinates and both
Leader mirrors after the earlier Scout/Merchant processing. The earlier post-command authority
supplies inventory and the serial-1 LeaderOptions after-image; it is not substituted for the
later adjacent Sim.

## Exact prefix

| VA | Instruction/child | Owned effect |
|---|---|---|
| `0x005F6F91` | `unit_masks &= 0x87FFFFFF` | detached Unit mask write |
| `0x005F6FA3` | Leader flags `|= 0x00080000` | staged through the existing dual-mirror pending primitive |
| `0x005F6FDE` | `leader_flags2 & 2` | exact local-return gate |
| `0x005F6FFF` | `unit_masks & 0x100` | selects the supported worker path |
| `0x005F7192` | `ObjectData::is_worker` | type 50 returns one (`0x0046FA10`) |
| `0x005F719D` | `Unit::think_peasant(0)` | entered with the complete detached receiver |

The Leader flags and flags2 values are admitted only when the victory-score and step-8 mirrors
agree. The pending OR is prepared but not committed.

## Peasant wait gate

`Unit::think_peasant` reads `LeaderOptions[who].peasants_wait` at `0x005F5783`. Values 1 through
5 map to thresholds 7, 12, 17, 32, and 62; all other values map to 2. The body is reached at an
exact threshold, or later when `(idle - 2) % 5 == 0`.

The supported serial-1 LeaderOptions pair leaves owner zero `peasants_wait == 2`. The adjacent
check-idle continuation has `idle == 2`, so `2 < 12` and `think_peasant(0)` returns zero locally
at `0x005F5A41`. This does **not** mean the enclosing `Unit::think` returned. The plan therefore
leaves a typed Think-suffix request and keeps the `0x8000` restoration armed.

If a source image instead reaches the build-search body, `Unit::find_build_spot` decodes the
actor's stored coordinates with XOR `0x63637`, reads replay-carried
`Constants::unit_build_respond_range` (`Constants+0x24`, value 12), scales it by 192 and doubles
the radius for worker stance 1 or 2. The stateful call at `0x00603F0C` is exposed as the existing
ten-argument `IdleFindBuildsRequest`; its scratch array is not guessed.

No canonical write occurs in either branch. The `0x0060DD68` restore may be the final local
write only after the entire enclosing Think suffix or FindBuilds continuation has returned.
