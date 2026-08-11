# `Unit::process_healing` selection and the Arena's remaining supply boundary

This note records the recovered arm map of `Unit::process_healing` (`0x005E0670`, 2,858 B),
three corrections it forces on the Arena's existing host, and what still stops the registered
blocker `arena-supply-model` from closing. The blocker is **not** closed by this tranche.

Fidelity: **Tier C throughout.** Every address, vftable slot and literal below was read out of
`ron-bin/riseofnations.exe` with capstone, cross-named against `ron-bin/sbl/rise.pdb` through
`re/symtab.json`. Nothing here is differentially tested against retail and nothing is verified.

## Where the code lives

| piece | file |
|---|---|
| the ordered healing transactions | `crates/don-ai/src/arena/retail_systems.rs` |
| the live Arena host | `crates/don-ai/src/arena/world.rs::ArenaSupplyHost` |
| the unit-band call sites | `crates/don-ai/src/arena/world.rs::World::tick_*_healing` |
| the recovered order override sets | `crates/don-sim/src/systems/order_dispatch.rs::MOVE_LIKE` |
| the mutual declaration matrix | `crates/don-ai/src/arena/retail_systems.rs::DiplomacyState` |

## The function, in retail's own order

`Unit::process` calls `process_healing` once per frame. It is a chain of independent arms, not a
switch: several can fire in the same frame, each with its own phase divisor and its own
`Unit::repair_damage` call.

| # | retail step | site | Arena |
|---|---|---|---|
| 1 | `UnitData::is_captain`, inlined as `(u16 +0x8E) >> 15`; zero returns | `0x005E068C` | the Arena's supported object is the root singleton, so this holds |
| 2 | `ObjectTypeData::domain +0x218 == 2` returns | `0x005E06A2` | executed |
| 3 | `UnitData::has_damage(1)`; zero returns | `0x005E06B5` | executed as the live `damage > 0` band gate |
| 4 | `word +0x82 >= 0` takes the `ObjectData::get_inside` garrison arm | `0x005E06C2`, `0x005E06D7` | blocked — the Arena hosts no contained objects |
| 5 | `domain == 1` naval repair arm, which **returns** rather than falling through | `0x005E08F0`, returns `0x005E09EC` | blocked (MODEL 6a/6b) |
| 6 | `UnitData::is_supply` returns | `0x005E09ED` | executed |
| 7 | base rate `Constants +0xBF8` | `0x005E0A19` | executed |
| 8 | Antipater / Wellington hero-aura arm | `0x005E0A1F..0x005E0AE4` | executed |
| 9 | Conquer-the-World arm: `Game +0x822 & 2` and the campaign leader type | `0x005E0AE4..0x005E0B49` | exactly disabled — the arena never sets that mode |
| 10 | Iroquois arm | `0x005E0B49..0x005E0C90` | executed, see below |
| 11 | Senator / President / CEO patriot arms | `0x005E0C90..0x005E0F1A` | executed |
| 12 | supply-source arm: French / Versailles rate, then `Supplies::find_supply` | `0x005E0F1A..0x005E0FFC` | executed |
| 13 | civilian arm | `0x005E1000..0x005E110D` | executed, see below |

## Correction 1 — `Unit` vftable `+0xD8` is `UnitData::is_moving`, not `is_hero`

The Iroquois arm's third gate is a virtual call:

```asm
005e0b75  mov eax, dword ptr [edi]
005e0b77  mov ecx, edi
005e0b79  call dword ptr [eax + 0xd8]
005e0b7f  test eax, eax
005e0b81  jne 0x5e0c90            ; non-zero skips the whole arm
```

Reading the real `Unit` vftable out of the shipped image — `0x00B417D0 + 0xD8`, file offset
`0x73FFD0 + 0xD8` — gives `0x00610AF0`, which `rise.pdb` names `UnitData::is_moving`. The Arena
host answered this slot with `UnitTypeData::unit_flags2 & 0x20` (`UnitData::is_hero`, a different
function at `0x0046CE60`), so a damaged Iroquois hero was refused healing and a damaged Iroquois
unit walking across its own territory was healed. Both are now the retail behaviour: heroes heal,
moving units do not.

`UnitData::is_moving` itself resets the order list to its front node, caches that node's
`UnitOrder*` into `UnitData +0xCC current_data`, and returns the virtual `UnitOrder::is_move`
(`UnitOrder` vftable `+0x14`, PDB signature `int is_move() const`). It answers zero when the unit
has no order at all.

### The `UnitOrder::is_move` override set, read from the vftables

`crates/don-sim/src/systems/order_dispatch.rs` derived `MOVE_LIKE` from four agreeing `cmp/je`
chains and marked the claim that it is also the `is_move` override set **UNVERIFIED**, because
every override is identical-COMDAT-folded onto the shared `mov eax,1; ret` stub. Folding hides
which *function* an override is; it does not hide which stub each *class vftable slot* points at.
Reading slot `+0x10` (`get_type`, the `OrderIndex`) and slot `+0x14` (`is_move`) out of each
`??_7<Class>Order@@6B...@` table and resolving the adjustor thunks gives the complete table:

| `OrderIndex` | class | `is_move` |
|---|---|---|
| 1 `MOVE_TO` | `MoveOrder` | 1 |
| 2 `ATTACK_TO` | `AttackToOrder` | 1 |
| 3 `EXPLORE_TO` | `ExploreToOrder` | 1 |
| 4 `FLEE_TO` | `FleeToOrder` | 1 |
| 18 `CHANGE_FORM` | `FormOrder` | 1 |
| 19 `GROUP_MOVE` | `GroupMoveOrder` | 1 |
| 21 `GROUP_ATTACK_TO` | `GroupAttackToOrder` | 1 |
| 6,7,8,9,10,11,12,13,14,15,16,17,20,22,23,24,25,26,27 | all other concrete classes | 0 |

That is exactly `MOVE_LIKE`. **Cross-lane change, not made here:** the `UNVERIFIED` caveat on
`MOVE_LIKE`'s `is_move` sentence in `crates/don-sim/src/systems/order_dispatch.rs` can be retired
on this evidence; `don-sim` is owned by other lanes.

One incidental oddity: `PatrolOrder::get_type` is folded onto `xor eax,eax; ret`, i.e. it returns
`OrderIndex::NONE` rather than `PATROL` (5). Every other class returns its own index. Nothing in
this note depends on it — `PatrolOrder::is_move` is 0 either way — but it should not be "fixed" by
assumption.

## Correction 2 — the territory gate is `LeaderData::is_ally`, not owner equality

Both the Iroquois arm and the civilian arm read the `WData` cell's territory owner and then call
`LeaderData::is_ally` (`0x006EDB50`) on the **unit's own** leader:

```asm
005e0c22  movsx eax, byte ptr [eax + ecx*4 + 0xf]   ; WData[cell].who
005e0c27  test eax, eax
005e0c29  js 0x5e0c90                               ; unowned skips
005e0c2b  push eax
005e0c2c  movzx eax, byte ptr [edi + 9]             ; unit.who
005e0c30  imul ecx, eax, 0x6eec
005e0c36  add ecx, 0xe3a390                         ; &Leaders[unit.who]
005e0c3c  call 0x6edb50                             ; LeaderData::is_ally(owner)
```

`LeaderData::is_ally(other)` is `other == self || (self.diplos[other] == 2 && leaders[other].diplos[self] == 2)`
— the mutual pair, self-inclusive. The Arena already owns that exact predicate as
`DiplomacyState::is_ally`, which composes the same two cells through
`don_sim::systems::victory_score::effective_diplo`; it was declared on `ArenaPatriotHealingHost`
and consumed only by the patriot arms. It is now declared one level up on
`ArenaIroquoisHealingHost` and consumed by the Iroquois and civilian arms as well, so allied
territory heals exactly as own territory does. The former `BlockedForeignTerritory` receipts,
which named a missing diplomacy matrix that the Arena in fact has, are now
`NonAlliedTerritory { rate, territory_owner }` — a live retail outcome rather than an authority
boundary.

**Deliberate asymmetry, do not "fix" it:** `UnitData::in_supply` (`0x00609EF0`), the query the
siege recharge path uses, compares the same `WData` owner byte against the unit's own `who` with a
plain `cmp` at `0x00609F6B` and never calls `is_ally`. `execute_reload_supply`'s
`territory_who == who` is therefore already exact and must stay owner equality.

## Correction 3 — the civilian arm's family is four predicates, not one

The final arm admits its unit through an ordered chain, all four bodies of which are literal:

| predicate | body | admits |
|---|---|---|
| `ObjectData::is_worker` `0x0046FA10` | `type->TypeIndex +0x04` in `{0x32,0x33,0x34,0x35}` | Citizen, Korean Citizen, Scholar, Korean Scholar |
| `UnitData::is_caravan` `0x0046CE90` | tail-dispatches type virtual `+0x130` `UnitTypeData::is_caravan` `0x00470420` = `unit_flags2 +0x2B8 & 8` | caravans |
| `ObjectData::is_merchant` `0x0046D370` | TypeIndex in `{0x3D,0x3E,0x190}` | Merchant, Armed Merchant, Fur Trapper |
| the literal tail | `type->TypeIndex == 0x13D` | Fishermen |

The Arena previously admitted only the four worker TypeIndexes, so damaged merchants and caravans
standing on friendly territory never healed. All four are now executed; `NotWorker` became
`NotCivilian`.

Two further details of this arm, both now recorded in code:

* its repair call is `Unit::repair_damage(1, 0, 1)` at `0x005E10F3` — the middle argument differs
  from every other arm and there is no `is_captain` `unit_masks &= ~0x4000` postlude;
* its `domain == 1` territory bypass at `0x005E10C2` is **dead code for a live on-map unit**,
  because step 2 returns for `domain == 2` and step 5's naval arm returns for `domain == 1`. The
  Arena's `NotLand` receipt stands for those two earlier returns, which the per-arm entry point
  does not share — it is not a gate inside the civilian arm.

## An accepted narrowing that is *not* corrected here

`ArenaSupplyHost::iroquois_healing_bonus` answers `has_tribe_bonus(0x12)` as
`players[who].tribe == 0x12`. The shipped `rules.xml` `<TRIBES>` order makes tribe index `0x12`
Iroquois, and the cross-checks already in the tree agree that the primary bonus id equals the tribe
index (`building_gather.rs` uses `0x0F` for Japanese and `0x07` for Egyptian, matching
`japanese`/`egyptians` at those indices). But retail's `LeaderData::has_tribe_bonus`
(`0x006E1370`) is wider than that comparison: it also honours a per-leader granted-bonus bitmap at
`LeaderData +0x6D94`, a global option bit, a scenario/campaign gate and the defeated-leader flag,
and only then falls back to `TribeData[tribe] +0x54 == bonus`. The Arena materialises none of that
extra state, so the comparison is a narrowing of a predicate the Arena cannot yet answer in full.
It is left alone rather than widened by guess.

## Why the blocker stays open

`MODEL6_INVENTORY`'s `Supply` row keeps three residuals, all of them missing Arena *state* rather
than missing arithmetic:

1. **multi-slot captain repair.** Every repair path in this family fails closed for
   `ObjectTypeData +0x308 uber_size > 1`, because the Arena materialises no captain/contents object
   graph and cannot run the retail damage cascade across slots.
2. **the naval `domain == 1` arm and the inside-object garrison arm.** Both need hosts the Arena
   does not have (water/naval runtime; contained objects).
3. **the Conquer-the-World arm.** Exactly disabled by game mode rather than approximated, but it is
   still an unexecuted retail arm and is recorded as one.

Nothing in this tranche makes a MODEL substitution unreachable, so the slug stays `KnownDrift` in
`schema/simulation-closure.json`.

## A separate registry defect found while reading

`crates/don-sim/src/deviations.rs` records the retail evidence for `arena-target-acquisition-model`
as `Object::find_auto_target 0x0064DDA0`. There is no `find_auto_target` symbol in `rise.pdb` at
all — `don-sim`'s `systems::target::find_auto_target` is our own name for the composed
`Unit::find_new_target 0x005FF6A0` → `find_melee_target 0x005FF9C0` →
`Object::find_nearby_target 0x00648DA0` chain — and `0x0064DDA0` is `ObjectData::count_inside`
(1,743 B), an unrelated per-cell counting walk. The cited VA is wrong and the cited name is ours.
`deviations.rs` is `don-sim`, so this is reported rather than edited.

## Tests

`crates/don-ai/src/arena/world.rs::supply_attrition_integration`:

* `friendly_worker_healing_preserves_phase_masks_clock_and_authority_boundaries` — extended: one
  declaration is not enough, the mutual pair heals on the same foreign tile.
* `civilian_healing_admits_the_whole_retail_family_and_rejects_everything_else` — new: a live
  Merchant heals, a Catapult reports `NotCivilian`.
* `iroquois_healing_uses_live_age_phase_and_preflights_unsupported_composition` — extended: a live
  `MOVE_TO` reports `Moving`, and mutual alliance heals on foreign territory.

These are Rust tests over the live Arena world. They are not retail differential evidence.
