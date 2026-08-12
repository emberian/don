# Scenario `place_building_with_cost` transaction boundary

This note records the retail body behind BHS builtin 520 and the exact canonical prefixes now
mounted in the replay-selected `ScriptRuntime`. The prefixes are deliberately not scalar stubs:
an affordable request stops at the first unowned native transaction instead of reporting a
building that was never placed.

## Retail call chain

`ScenarioFuncSet::place_building_with_cost` is the 123-byte wrapper at `0x009F54A0` with
arguments `(who, building_type, city_name)`. It:

1. subtracts one from the one-based Leader argument and rejects an unsigned owner above seven;
2. requires both low Leader flag bits;
3. resolves the City with `get_city_index` at `0x009E2BA0` and requires an active City;
4. passes `CityData::o` to `place_orphan_building_with_cost` at `0x009F5520`.

The 348-byte builtin-521 body validates the object with `valid_build_o` at `0x009E3300`, resolves
the requested Type with `get_type_index(name, 0)` at `0x00A03480`, and accepts only Build Types
414 through 542. The cost City's index is `BuildData::city` only when the origin Build has both
ACTIVE and CAPTURED; otherwise it is `-1`.

The Type virtual at vtable offset `+0x84` resolves to `TypeData::can_pay_cost` at `0x00667570`.
Its ordinary branch gates each of six goods through `LeaderData::is_possible_good(good, 1)`,
loads the dynamic `TypeData::get_cost`, and divides the corresponding unreserved Leader resource
by that cost. A reached quotient of zero refuses the request. Otherwise it retains the maximum
quotient; the no-cost case returns 10. Builtin 521 only distinguishes zero from nonzero.

A zero result returns zero without mutation. A nonzero result enters
`Leader::produce_building(type, origin_build_object, 0)` at `0x006E1400`. That function is 7,406
bytes and performs the native placement search, City association, resource payment, Build
allocation, builder selection, and BUILD_AT order installation. It eventually calls
`Objects::init_build` (call site `0x006E2CA3`) and `Group::action_swarm_around` (call site
`0x006E2EB2`). The exact mounted entry/City gate owns `0x006E1400..0x006E150D`; the next
1,492-byte frame-zero setup owns `0x006E150D..0x006E1AE1`. The candidate loop and mutation tail
remain 5,645 bytes.

The existing package-driven Group Build runtime is therefore not interchangeable with this
call: it begins after a site and builders have already been selected, while
`Leader::produce_building` owns that selection.

The recovery is pinned to retail `riseofnations.exe` SHA-256
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079` and `rise.pdb`
SHA-256 `334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`. PDB layouts name
`SubObjectData::x_internal/y_internal` (`+0x10/+0x14`) and
`ObjectTypeData::x_size/y_size` (`+0x234/+0x238`). The PE body establishes:

- `0x006E150D..0x006E15B7`: coordinate decode, `WData +0x04` region read, and
  `LeaderData::get_radius`;
- the nonzero-frame policy beginning at the `Game+0x550` test, excluded by this tranche;
- the distinct Oil Well (`type 421`) existing-object scan, also excluded; and
- `0x006E1ACE..0x006E1AE0`: compare first offset with `circle_radius[radius_index]` and jump
  to the common failure epilogue when exhausted. The next instruction is `0x006E1AE1`.

## Canonical prefixes and receipts

`bhs_place_building_runtime` reads and reconciles:

- Leader activation across the Type, victory, and step-8 mirrors;
- the canonical `CityPool` identity and active center;
- the World Build-band mapping and exact `BuildData` identity;
- the canonical Type row and the production runtime's Building-class projection;
- resources across production, `Sim::leaders`, step 8, and victory/economy;
- revision- and composition-bound dynamic cost/possible-good answers for the exact
  `(owner, type, origin object, city constraint)` tuple.

Invalid retail arms return a receipt with `-1`; an unaffordable request returns a fully terminal
receipt with zero. An affordable request returns
`ReadyForLeaderProduceBuilding` plus the exact continuation boundary
`{ va: 0x006E1400, bytes: 7406, owner, type, origin, mode: 0 }` and no scalar result.

The entry tranche resolves the origin Build and reproduces retail's active-City versus target
`build_flags & 0x10` admission. Its rejection returns native one, which builtin 521 converts to
scenario zero. An admitted request continues at `0x006E150D`.

The frame-zero search-setup tranche consumes only canonical owners:

- decoded coordinates from `BuildData::x_internal/y_internal` and `WData::region` from the map;
- `LeaderData::get_radius` from retail `CityRules`, the origin production Type, and the live
  Indian tribe-bonus bit;
- the exact `BuildTypeData::is(DOCK, 0)` relation from the mutable Type table;
- `ObjectTypeData::x_size/y_size` from the installed production Type projection; and
- `circle_radius[]` from the instruction-derived canonical circle generator.

It deliberately refuses nonzero `Game::frame` because that retail arm changes the radius and
first offset through additional Type policy, and it refuses Oil Well because that Type takes a
separate existing-well/map scan. For the ordinary admitted cohort it records the origin world
cell/region, radius and circle-table endpoint, Dock/footprint search arguments, and the
resource-sensitive search flag. A non-exhausted scan returns no scalar: its continuation is
`{ va: 0x006E1AE1, bytes_remaining: 5645, ...exact live locals }`.

The replay host records terminal receipts from the earlier gates and both native-prefix receipts
on admitted execution. Reaching the candidate loop raises the existing unimplemented host
boundary, so `ScriptRuntime` rolls back Program/ref/timers, BHS cursor, research queues,
resources, Cities, Groups, and all Leader mirrors. This preserves builtin 520 as the externally
visible stop until candidate selection and the placement mutation tail are one atomic
transaction.

## Installed-content evidence

The replay-selected installed `economic.bhs` success path reaches builtin 520 after the owned
357, 258, 362, 455, 386, and 436 continuation. Its first request is Farm in Athens for the replay
AI owner. The fixture admits the installed Farm cost (`4t`, decoded to 40 Timber) and reconciles
100 units of every Leader resource. The cost result is nonzero (`100 / 40 = 2`); the active
Athens Build passes the City gate; and the 4x4 Farm advances through radius 20 / circle index 5
to the exact candidate-loop boundary. The test still stops at builtin 520, proving that no false
placement result leaks past the native boundary.

The focused gate is:

```text
CARGO_TARGET_DIR=/Users/ember/.cache/don-bhs520c9-target \
  cargo test -p don-replay --test replay_bhs_research_runtime -- --nocapture
```

All five tests pass with installed `ron-data`: terminal refusal, later-VM rollback, direct
read-only/save-resume receipts, insufficient-resource refusal, and replay-selected shipped
economic continuation.
