# Scenario `place_building_with_cost` transaction boundary

This note records the retail body behind BHS builtin 520 and the exact canonical prefix now
mounted in the replay-selected `ScriptRuntime`. The prefix is deliberately not a scalar stub:
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
`0x006E2EB2`), but approximately 6.2 KiB of policy and search precedes those leaves.

The existing package-driven Group Build runtime is therefore not interchangeable with this
call: it begins after a site and builders have already been selected, while
`Leader::produce_building` owns that selection.

## Canonical prefix and receipt

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

The replay host records terminal receipts. On the ready arm it raises the existing unimplemented
host boundary, so `ScriptRuntime` rolls back Program/ref/timers, BHS cursor, research queues,
resources, Cities, Groups, and all Leader mirrors. This preserves builtin 520 as the externally
visible stop until `Leader::produce_building` can be committed as one atomic transaction.

## Installed-content evidence

The replay-selected installed `economic.bhs` success path reaches builtin 520 after the owned
357, 258, 362, 455, 386, and 436 continuation. Its first request is Farm in Athens for the replay
AI owner. The fixture admits the installed Farm cost (`4t`, decoded to 40 Timber) and reconciles
100 units of every Leader resource. The prefix result is nonzero (`100 / 40 = 2`) and the test
still stops at builtin 520, proving that no false placement result leaks past the native boundary.

The focused gate is:

```text
CARGO_TARGET_DIR=/Users/ember/.cache/don-bhs520-target \
  cargo test -p don-replay --test replay_bhs_research_runtime -- --nocapture
```

All four tests pass with installed `ron-data`: terminal refusal, later-VM rollback, direct
read-only/save-resume receipt, and replay-selected shipped economic continuation.
