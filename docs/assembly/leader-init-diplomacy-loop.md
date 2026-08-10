# `Leader::init` diplomacy/shared-vision loop

## Recovered source boundary

`leader_init_diplomacy_loop` is a source-only, whole-image-bound reconstruction of
`Leader::init(int who, int tribe, int local)` at `0x006E3BF9..0x006E3D93` in shipped PE32
`ron-bin/riseofnations.exe`, SHA-256
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`.
The function begins at `0x006E3930` and is 6,102 bytes. No retail function was executed.

The recovered slice is one complete eight-target loop:

1. `0x006E3BF9..0x006E3C03` initializes `LeaderData::ally_mask` to `1 << who`.
2. `0x006E3C0D..0x006E3C49` clears the target's agenda, deed, attack/raid/capital/ally,
   tribute/gift/hire stamps, sets `hire_who = -1`, and initializes the treaty cell from
   `reveal_map == 3`.
3. A negative `tribe` takes `0x006E3D29..0x006E3D31`: self becomes raw Ally and every
   other target becomes raw War. It skips both team queries and shared-vision expansion.
4. The ordinary arm:
   - `Game+0x822 & 2` skips the first team/relation initialization call;
   - otherwise `is_team(target, 0)` selects raw Ally, while the false arm selects War or
     Peace through `rush_rules`, `LeaderData::starting_age`, and `Game::teams_locked`;
   - `Game+0x821 & 2` then forces every non-self raw cell to War;
   - a mutual `is_ally(target)` result expands `ally_mask` only when
     `has_preq(0x2B0)` succeeds or `reveal_map >= 1`;
   - a second `is_team(target, 0)` ORs treaty bit zero.
5. `0x006E3D34..0x006E3D8D` derives `aggression[target] = (raw diplo == War)` and clears
   the strong/weak/DOW/invader/alliance/peace, message, speech, counteroffer, tribute and
   taunt cells before advancing to the next target.

The plan preserves this instruction order. Team and ally callees can therefore observe raw
declarations written for earlier targets; it does not collapse the body into independent
array formulas.

## Correct option offsets and callees

`GameAccess::game` at `0x00C061EC` is a `Game&`. Because `GameInfo` begins at `Game+0x0C`,
the relevant bytes are:

| absolute Game offset | GameInfo field | use |
|---:|---|---|
| `+0x2A` | `game_rules` `+0x1E` | selects `starting_age`'s team-zero combined arm |
| `+0x30` | `reveal_map` `+0x24` | treaty base and shared-vision fallback |
| `+0x32` | `rush_rules` `+0x26` | permits pre-lock Peace while greater than starting age |
| `+0x34` | `starting_technology` `+0x28` | primary age, capped at 7 |
| `+0x35` | `starting_technology2` `+0x29` | team-zero secondary age, capped at 7 |
| `+0x36` | `ending_technology` `+0x2A` | cap for the combined team-zero age |

`LeaderData::starting_age` is the 74-byte body at `0x006D7320`; `Game::teams_locked` is
the 29-byte body at `0x00594880`. The latter returns false only for team styles 0, 8, and
11. Earlier notes calling the `Game+0x32` byte `GameInfo+0x32` or calling the callee
`get_age` were offset/name errors; the executable and PDB jointly establish the mapping
above.

## Exact product boundary

This wider plan is intentionally not called by `Sim::start_manual_player_setup` or the
browser ABI. Product publication still needs one owner for:

- the exact `GameInfo` option image and both semaphore bits;
- `has_preq(0x2B0)` at the point each Leader row initializes;
- sequential initialization of the complete eight-Leader table, because a mutual-alliance
  read can see which reciprocal rows have already run;
- the treaty, aggression, interaction and `ally_mask` fields, which the current victory
  `LeaderState` does not own or serialize.

The existing PlayerSetup integration therefore remains limited to its option-independent
active-team raw declarations. DoNSave continues to refuse the active PlayerSetup owner, and
this tranche neither changes checksum/save channels nor creates a browser shadow table.

The remaining `Leader::init` body before and after this loop still owns tribe, economy,
technology, type, personality, scoring, production-script and host callback state. Later
diplomacy mutation remains the separate atomic `Leader::set_diplo` transaction, including
ejection, shared-vision removal/addition, victory, army and event tails.

## Reproducible gate

```text
tools/swarm-cargo leader-init-loop test -p don-sim --test leader_init_diplomacy_loop
```

The six focused cases cover ordinary team/non-team initialization, exact starting-age and
team-lock ordering, scenario preservation, forced-war override before shared vision,
negative-tribe bypass, complete interaction resets, invalid identity and stale-plan refusal.
