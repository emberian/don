# `Unit::process_attrition` recovery and the Arena's remaining attrition boundary

This note records the recovered structure of `Unit::process_attrition` (`0x005E11A0`), what
the Arena can now execute of it against live state, and the exact retail state that still
stops it. It targets the registered blocker `arena-attrition-model`; the blocker is **not**
closed by this tranche.

Fidelity: **Tier C throughout.** Everything below comes from the shipped PDB layouts, the
decompiled control flow in `re/decomp-all/005e11a0.c` and the small callee bodies named in
each row. Nothing here is differentially tested against retail and nothing is verified.

## Where the code lives

| piece | file |
|---|---|
| `UnitData::get_attrition`, `attrition_period`, `attrition_damage`, `supply_state` | `crates/don-sim/src/systems/borders_fog.rs` (unchanged, called not copied) |
| the ordered `Unit::process_attrition` transaction | `crates/don-ai/src/arena/retail_systems.rs::execute_attrition_recompute` |
| the live Arena host | `crates/don-ai/src/arena/world.rs::ArenaSupplyHost` |
| the 32-frame call site | `crates/don-ai/src/arena/world.rs::World::tick_attrition_recompute` |

## The function, in retail's own order

`Unit::process` calls `process_attrition` on the unit's own 32-frame stagger, immediately
before the due-period supply/attrition branch that this crate already executes.

| # | retail step | evidence | Arena |
|---|---|---|---|
| 1 | `unit_masks &= ~0x400080`; `UnitData::attrition +0x9E = 0` | `0x005E11A8` | executed |
| 2 | read the `WData` owner byte under the unit | `0x005E11B4`, coords de-obfuscated with `0x00063637` | executed |
| 3 | walk `ScenarioData` attrition-free points; a hit returns | `0x005E1213`; writers `ScenarioData::add_attrition_free_point 0x00996970`, `ScenarioFuncSet::set_attrition_free_point 0x00A02AA0`, `clear 0x00A02B60` | the empty-list case is executed (the Arena hosts no `ScenarioData`); a populated list is blocked, because the comparison at `0x005E122C` uses `DAT_00CAE5FC[coord >> 6]` rather than the raw coordinates and that table's output space is not derived |
| 4 | unowned territory: `LeaderData::neutral_attrition +0x800`, zero returns, otherwise it becomes the period unless `domain == 1` | `0x005E1298` | zero case executed; the non-zero case writes the period and then stops (see below) |
| 5 | friendly territory returns | `0x005E12C5` | executed |
| 6 | territory owner's `leader_flags & 1` and `& 2` | `0x005E12CD..0x005E12E3` | executed |
| 7 | mutual `LeaderData::diplos == 2` pair | `0x005E12FD..0x005E1319`, both directional cells | executed |
| 8 | `take_att_disabled +0x7FC` (victim), `give_att_disabled +0x7F8` (owner) | `0x005E131F`, `0x005E132C` | executed |
| 9 | worker / merchant / hero / supply / ordinary object gauntlet | `0x005E133B..0x005E1445`, see below | executed except the live `GatherOrder` byte |
| 10 | `ObjectTypeData::domain +0x218 == 1` returns; `neutral_attrition != 0` returns | `0x005E1456`, `0x005E1473` | executed |
| 11 | Conquer-the-World guard: `Game +0x822 & 2` with `LeaderData::has_conquest_bonus(9)` | `0x005E1486`, `0x005E14A1` | blocked (the Arena is never a CTW game) |
| 12 | `LeaderData::is_enemy 0x006EBAA0` selects the peace/trespass arm (`Constants::peace_attrition +0xD34`) or the war arm; inside the war arm `Game.info.team_style == 2` selects `Constants::assassin_attrition +0xD38` unless `LeaderData::get_target 0x006DA000` is the territory owner | `0x005E14BA` `is_enemy`, `0x005E14CF` `war_allowed`, `0x005E14E9` `get_target`, `0x005E15DD` assassin period, `0x005E18E2` peace period | branch executed; both trespass arms blocked |
| 13 | land tail: `is_supply` gate, `UnitData::get_attrition 0x00608FD0`, `max(1, (Constants::attrition * v) / 256)`, `min` against any special period, `unit_masks |= 0x80`, `Leader::meet 0x006E1250` | `0x005E18F1` domain, `0x005E191A` `is_supply`, `0x005E192E` `get_attrition`, `0x005E1942` `Constants::attrition`, `0x005E1962` the `min`, `0x005E1973` the period write, `0x005E19C1` `Leader::meet` | executed; the two leader scalars are blocked |

### The object gauntlet, resolved

Each predicate is a named shipped function; none of them is a heuristic.

| predicate | body |
|---|---|
| `ObjectData::is_worker 0x0046FA10` | TypeIndex `0x32..=0x35` |
| `UnitData::is_gathering 0x006089B0` | worker TypeIndex, current action order kind 7, order owner equals the unit's owner |
| the gathering exemption | `GatherOrder::non_flat_gather` at `+0x25` (`schema/types.json`) |
| `ObjectData::is_merchant 0x0046D370` | TypeIndex `0x3D`, `0x3E`, `0x190` |
| `UnitData::is_hero 0x0046CE60` | `UnitTypeData::unit_flags2 +0x2B8 & 0x20` |
| `UnitData::is_supply 0x0046CE80` | `unit_flags2 & 0x40` |
| `UnitData::is_special 0x0046CEA0` | `unit_flags2 & 0x10` |
| `UnitTypeData::is_caravan 0x00470420` | `unit_flags2 & 0x08` |
| ordinary-unit attack gate | `ObjectTypeData::attack +0x1E8` |
| the tech gate | devirtualized `ObjectData::is(0x3A, 0)` |
| `UnitTypeData::is_siege 0x00470460` | `unit_flags +0x2B4 & 0x20000` |

Workers, merchants, heroes and supply units **skip** the attack / special / tech / caravan
ladder entirely; only the ordinary-unit arm runs it. That asymmetry is load-bearing: a
Citizen has zero attack and would otherwise be exempt.

## A retail control-flow finding: the unowned-territory fall-through

The decompiled C reads as if the unowned-territory arm returns after storing
`neutral_attrition`. It does not. Disassembling `0x005E1294..0x005E12D3` shows the arm
falling straight into the owned-territory chain with the territory index still negative:

```asm
005e1294  test edi, edi              ; edi = the WData owner byte, sign-extended
005e1296  jns 0x5e12c5
005e1298  imul ecx, esi, 0x6eec      ; leader[unit owner]
005e129e  cmp dword ptr [ecx + 0xe3ab90], 0   ; neutral_attrition
005e12a5  je 0x5e19c6                ; zero -> return
005e12ab  mov eax, dword ptr [ebx + 0x18]
005e12ae  cmp dword ptr [eax + 0x218], 1     ; domain
005e12b5  je 0x5e12c5
005e12b7  mov ax, word ptr [ecx + 0xe3ab90]
005e12be  mov word ptr [ebx + 0x9e], ax      ; the period write
005e12c5  cmp edi, esi                       ; <- fall-through, edi still negative
005e12cd  imul edx, edi, 0x6eec
005e12d3  mov eax, dword ptr [edx + 0xe3a390]  ; a read *before* the Leaders array
```

So a non-zero `neutral_attrition` in unowned territory makes retail read `leader_flags`
from memory preceding `Leaders`. That continuation is not derivable from anything available
here, so the transaction performs the exact period write and then reports
`Blocked { UnownedTerritoryFallthrough }` naming `0x005E12CD`. The arm is unreachable in
the Arena because `neutral_attrition` is written only by scenario and Conquer-the-World
paths the Arena does not host.

This is also why `execute_attrition_recompute` never applies retail's
`if (period == 0 || new < period)` guard at `0x005E1917` with a non-zero incumbent: the
only writers of an incumbent period are the two trespass arms, both blocked.

## Two corrections to existing records

1. **`AttritionInput::type_class` is `ObjectTypeData::domain` (`+0x218`).**
   `crates/don-sim/src/systems/borders_fog.rs` documents `+0x218` as an unnamed "type
   class" with `1` suppressing attrition and `2` halving it. `schema/types.json` names the
   field `domain`. So `1` is the sea domain — which in fact returns from
   `process_attrition` before any selection — and `2` is the air domain, whose only effect
   is halving the two special trespass periods. The land tail runs only for `domain == 0`.

2. **`don_sim::systems::borders_fog::get_attrition` is missing one retail return.**
   At `0x00609152`/`0x00609161`, on the non-militia path only, retail returns zero when
   `LeaderData::has_preq(0x2FE)` and `UnitData::is_idle 0x0046FA40` both hold. The don-sim
   kernel has no such arm. `execute_attrition_recompute` applies it before calling the
   kernel and reports `NoAttritionRate`. **This is a required cross-lane change in
   `crates/don-sim`, which this lane does not own.**

## Why the blocker stays open

`UnitData::get_attrition` reads two walked `LeaderData` scalars:

* `att` `+0x7F0` of the **territory owner**, produced by `Leader::calc_attrition`
  (`0x006CDEA0`): zero unless the leader answers `has_preq` for the consecutive BonusType
  chain `0x2DD..=0x2E0`, then scaled by `has_wonder(0x212)` (`Constants::colosseum_attrition`),
  `has_tribe_bonus(0xD)` (`Constants::russian_attrition`), the Conquer-the-World bonus
  (`Constants::ctw_attrition`) and `has_wonder(0x21A)` (`Constants::kremlin_attrition`).
  The four levels come from `Constants::attrition_improved +0x1D8`.
* `anti_att` `+0x7F4` of the **unit's owner**, produced by `Leader::calc_anti_attrition`
  (`0x006CDCC0`) from the `0x2FE..=0x300` BonusType chain, `has_wonder(0x219)`,
  `has_tribe_bonus(0x11)` and the titanium rare-resource flag.

The Arena materializes none of that graph. `LeaderData::has_preq` (`0x006DB810`) is a
recursive walk over each BonusType's own prerequisite list, and those lists are built by the
executable rather than shipped in `ron-data/` — `typenames.xml` has no bonus section and
`live-tables-typeids.tsv` dumps the `BonusType` rows with empty names. `PlayerState::techs`
holds `TechType` indices `544..=628` only, so a research command that would grant an
attrition bonus in retail grants nothing here.

Answering zero would therefore be a fabricated value with a real gameplay consequence: it
asserts that every arena nation has no attrition research, when the arena's own bots do
research from the Military line. `ArenaSupplyHost::leader_attrition_rate` returns `None`
and the transaction reports `Blocked { LeaderAttritionRate }`.

Two further arms are blocked for state, not for arithmetic:

* the peace/trespass arm needs `LeaderData::attrition_stamp{,2,3}` (`+0x1F4..+0x1FC`), the
  `broke_alliance +0x2B0` / `made_peace +0x2D0` pair timestamps against
  `Constants::ally_to_war_delay +0xCF8` and `ally_to_war_grace +0xCFC`, plus
  `Game::say_no_war 0x00592B00`, `MessageWin::add_message` and `SoundGlobal::play`;
* the assassin arm needs `LeaderData::get_target` over `Game::start_list`/`start_index`
  and the same stamp/message transaction.

## Observable effect

None yet, and that is the honest summary. `World::tick_attrition_recompute` already reset
the period to zero on every 32-frame recompute, so no arena unit has ever taken attrition
damage; it still does not. What changed is that the reason is now a specific typed receipt
naming the retail state that is missing, and that thirteen of retail's own returns — the
empty-scenario, neutral, friendly, inactive-leader, ally, give/take-disable, six-way object
gauntlet, neutral-override, supply, non-land and zero-rate arms plus the complete land
selection — execute against live Arena state instead of stopping at a single "non-friendly
territory" boundary. Seven typed blockers replace that one boundary, each naming the retail
address and state it stops at: `ScenarioAttritionFreePoints`, `UnownedTerritoryFallthrough`,
`GatherOrderState`, `ConquestWorldGame`, `PeaceTrespassTransaction`,
`AssassinTrespassTransaction` and `LeaderAttritionRate`.

If the two leader scalars become available (either by recovering the BonusType
prerequisite graph or by capturing `LeaderData +0x7F0`/`+0x7F4` from the live process), the
land tail already runs and attrition becomes live in the arena without further wiring.

## Tests

`crates/don-ai/src/arena/retail_systems.rs`:

* `attrition_recompute_selects_the_land_period_and_meets_the_territory_owner`
* `attrition_recompute_takes_every_recovered_return_in_retail_order`
* `unowned_territory_writes_the_leader_neutral_attrition_period_then_stops`

`crates/don-ai/src/arena/world.rs`:

* `live_unit_band_applies_reload_healing_attrition_and_source_lifetime`

These are Rust unit tests over a synthetic host and the live Arena world. They are not
retail differential evidence.
