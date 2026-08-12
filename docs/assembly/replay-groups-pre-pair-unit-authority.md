# Replay Groups pre-pair Unit/content authority

Status: exact replay-carried UnitType/Tribe projection; same-frame substantive installation
remains red.

## What this tranche owns

`groups_pre_pair_unit_authority` inverts the already-admitted serialized Rules walk for one
Unit TypeIndex. It rechecks the complete `InitialRules` span, SHA-256, walked-byte count, and
Types/Constants/Balance/Tribes checkpoints before returning any field. It then exposes exact
payload spans for:

- `TypeData + 0x04 .. +0x5e`, including `upgrade` and `jump`;
- `ObjectTypeData + 0x1e4 .. +0x27c`, including masks, attack/range/domain, formation spacing,
  graft and age; and
- the four `UnitTypeData` ranges walked by `UnitType::walk_rules_data`, including unit flags,
  moves, turn speed, role, military level, squad/uber/crew sizes and base formation.

`replay_tribe_type_facts` separately binds `Tribe + 0x54` and the exact
`Tribe + 0x70 + 4 * (type - 50)` graft word. A byte mutation in the returned Type50 role span is
rejected by the Rules SHA gate before it can become content authority. No recorded Groups or Units
checksum is consulted.

This is not a Unit constructor. It creates no object, Handle, UID, position, angle, form,
formation width, order, path, Guy, terrain answer or Group. It therefore does not change
`GroupChannelTransitionReceipt::installed_in_scoreboard()`.

## The actual strict adapter witness

The existing `ReplayGroupMoveSource` admits this same-package checksum witness:

| field | retail recording |
|---|---|
| replay | `Playback___2018.11.17_13_21_42__Sat_.rcx` |
| SHA-256 | `c006ecb860273605d2b48bf69f5dcb048596de5fc748aa664fa0a04452df2da0` |
| setup | seed `0x01558db7`, map style 9, size 6, starting town 1, resources 2, reveal 1 |
| package | lockstep serial 48, frame 259, play 1, who 0 |
| pair/checksum | command indexes 0/1 followed immediately by checksum index 2; TurnData suffix |
| selection | object `[0]` |
| MoveTo | `(22374,57828)`, set-angle 1, angle `-138149888`, orders 1, queued 2, form 0, width 50 |

Who 0 is the tribe-14 player. The replay-carried Tribe row resolves base Scout 69 to nation
variant 69. Its admitted UnitType row is:

```text
upgrade=71 jump=71 obj_masks=33718308 attack=0 max_range=3 domain=0
guy/x/y_spacing=144 unit_flags=6277 unit_flags2=18 moves=34
turn_speed=322122544 role=262416 squad=1 uber=1 crew=1 base_form=0
```

That proves the exact content inputs available to setup and formation. It does **not** decide
`LeaderData::current_upgrade`: that function also reads the live Leader technology/type bitsets.
The spawned root's effective type must come from the existing setup receipt and agree with
`Sim.unit_type`; choosing 69 or 71 because either looks plausible is forbidden.

The package is not a clear-pool transition. Eight opcode-0 commands precede it:

```text
serial 14: who0 [4]
serial 17: who0 []
serial 20: who0 []
serial 23: who1 [1,2,3,4]
serial 23: who0 []
serial 26: who0 []
serial 27: who1 []
serial 32: who1 []
```

Therefore the synthetic `retail_fresh_groups()` state used by the adapter's execution test is
evidence that the bounded host accepts real wire bytes, not evidence for retail's pre-pair Group
pool. A substantive same-frame receipt must replay these packages and their following actions
through canonical hosts first.

## The useful clear-pool comparison

`Playback___2024.02.23_20_49_35__Fri_.rcx` (SHA-256
`1690431a5ef19b38a3425d3dd7311e8e83ca0d27c56fabe49d776a9f1421b251`) contains the
smallest observed first Group mutation:

- serial 64, frame 379, play/who 0, objects `[3,4,5,6]`;
- shell `[PlayerSpeed, Group, MoveTo, TurnData]`;
- zero prior opcode-0 commands;
- Groups checksum `0x1c78f3f5` through serial 65, then `0x22a5074d` at serial 66;
- tribe 22, whose setup schedule is base Scout, two Dutch Merchants, then four Citizens;
- exact identity grafts 69, 62 and 50; and
- Citizen 50 content with 144 spacings, unit flags 6273/2, role 262912, squad/uber/crew
  1/1/0 and base form 0.

This comparison sharply reduces the setup identity question, but it cannot replace the strict
adapter witness: its checksum is delayed by two serials and TurnData lies between the pair and the
later checksum. Treating it as a same-package checksum would loosen the admission boundary.

Before this clear-pool pair, the conservative command inventory is 93 PlayerSpeed, 128 TurnData,
and two LeaderOptions commands. PE `CommandPackage::process_leader_options` `0x009441d0` shows
that LeaderOptions can cascade into Unit stance/mask writes, so they cannot be globally discarded.
For this selected Citizen cohort, the observed options are retained as provenance; a future
whole-package replay still has to execute their exact tails rather than inferring harmlessness from
the eventual Groups checksum.

## PE/PDB comparison

| owner | VA / PDB facts | integration meaning |
|---|---|---|
| `Setup::build_units` | `0x005aafc0`, 1,952 bytes | reuse `setup_units_producer::build_units_plan`; do not make a second setup schedule |
| `Setup::place_unit` | `0x005abca0`, 749 bytes | exact offset table, terrain/collision tests and spawn/queue result still required |
| `Objects::init_unit` | `0x0065e0c0`, 1,603 bytes | must produce the existing `BuildUnitsPrefixReceipt` identities and RNG spans |
| `LeaderData::has_tribe_bonus` | `0x006e1370`, 133 bytes | fallback compares `Tribe + 0x54`; the decoded value is a tribe/bonus id, not a separate bonus count |
| `LeaderData::current_upgrade` | `0x006e3140`, 342 bytes | join replay content's upgrade/jump fields with canonical Leader bitsets |
| `UnitData::is_modern_infantry` | `0x00607b40`, 113 bytes | join unit flags/age with Leader tech 0x12 |
| `FormData::type_cat` | `0x0072dfc0`, 615 bytes | reuse the existing postload category owner; do not hardcode a category from the checksum target |
| `Group::action_move_near` | `0x00704990`, 9,205 bytes | canonical Group host already owns the bounded transaction once authority is complete |
| `CommandPackage::process_move_to` | `0x009497c0`, 421 bytes | real retail package provenance is already retained by `groups_sim_channel` |

PDB sizes/offsets used by the projection are `UnitTypeData sizeof 1496`, `unit_flags +0x2b4`,
`unit_flags2 +0x2b8`, `role +0x2c8`, `uber_size +0x308`; and `UnitData sizeof 344`, with
`angle +0x50`, `group +0x80`, `form +0xaa`, and `form_mod +0xab`.

## Exact integration hooks

The shortest honest path is:

1. Build the setup schedule with `setup_units_producer::build_units_plan` from replay GameInfo,
   `replay_tribe_type_facts`, `replay_unit_type_facts`, and canonical Leader bitsets.
2. Execute `Setup::place_unit`/`Objects::init_unit` through their existing receipt boundary and
   validate with `validate_build_units_prefix_receipt`. For the strict witness, bind the returned
   captain for the base-Scout call to object 0; for the clear-pool comparison, bind the four
   Citizen calls to objects 3..6. Never fabricate a receipt merely because the numbers are
   consecutive.
3. Resolve each receipt's `(id,generation,owner,o)` against `Sim.world.handle_at_row`, require the
   same `Sim.unit_type`, and take dynamic `uid/group/form/form_mod/angle/x/y/orders/path` from the
   canonical Sim owners at the package frame.
4. Build `MoveMemberAuthority` from that Handle plus the shared postload formation/category owner.
   The remaining exact facts are on-map/captain/order-installability, can-move/plane/domain/flags,
   speed, move-near split admission, destination water, forced-facing-zero, and modern-infantry.
   `don-env::typecaps::FormationTypeCap` currently owns the postload category/spacing projection
   privately; the product hook should rehome or expose that owner rather than adding another TSV
   parser in replay.
5. Replay every preceding Group package/action into `Sim.groups`. Only then install the authority
   with `Sim::replace_group_move_authority` and call the already-landed
   `groups_sim_channel::issue_replay_group_move` at the recorded frame.
6. Compare the independent don-replay Group walk with the recorded package checksum. Until steps
   2-5 are receipt-complete, scoreboard installation stays false.

The stage map is therefore:

| stage | status |
|---|---|
| real RCX package/checksum provenance | green |
| replay-carried UnitType and Tribe content | green in this tranche |
| outer `Setup::build_units` call schedule | green owner exists; strict-witness type resolution not yet joined |
| `place_unit` / `init_unit` real receipt | red |
| exact Sim Unit state at frame 259 | red |
| eight prior Group command/action transactions | red |
| Handle-bound `GroupMoveAuthority` product adapter | red |
| canonical pair execution + independent Group walk | green for caller-supplied state |
| retail same-frame substantive comparison | red |

## Gates

```sh
cargo test -p don-replay --test groups_pre_pair_unit_authority -- --nocapture

tools/swarm-cargo-remote submit hbox groups-pre-pair-owner \
  --path crates/don-replay/src/groups_pre_pair_unit_authority.rs \
  --path crates/don-replay/tests/groups_pre_pair_unit_authority.rs \
  --asset schema/live/final-balance-runtime.bin \
  --asset ron-data/replays/multi/Playback___2024.02.23_20_49_35__Fri_.rcx \
  --asset ron-data/replays/multi/Playback___2018.11.17_13_21_42__Sat_.rcx \
  -- test -p don-replay --test groups_pre_pair_unit_authority -- --nocapture
```
