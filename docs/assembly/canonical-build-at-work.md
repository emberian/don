# Canonical saved `BUILD_AT` work

## Scope

This tranche executes one substantive saved `BUILD_AT` branch through production
`Sim::unit_work`: the fresh-save owner-2 Peasant `(o=3, uid=9, type=50)` contributes one
frame to the owner-2 Village `(o=2007, uid=16, type=414)`. The site is started, inactive,
adjacent, not a Farm, not under fire, and far from completion. The order remains installed.

It deliberately does not generalize construction. Unstarted placement, invalid/reused
targets, active targets, reswarming, Farms, angle changes, decoys, under-fire/Korean rate
changes, animation changes, 84-percent presentation, completion/activation, and builder
finish tails fail closed before mutation.

## Frozen retail evidence

The executable authority is the matched retail pair:

- `riseofnations.exe`: 9,925,120 bytes, SHA-256
  `30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`;
- `rise.pdb`: 57,290,752 bytes, SHA-256
  `334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`,
  GUID `{51d4f219-61c6-4f84-9d5b-c3361b0d291f}`.

The bounded bodies are:

| Body | VA | PDB size | raw `.text` SHA-256 |
|---|---:|---:|---|
| `Unit::do_build` | `0x005EEBF0` | 1,711 | `0ddd82c78a65cd13956f0d33b2ebdfaaa5942000339102b40f33b9a6d102587e` |
| `Wall::do_construct` | `0x006434D0` | 1,245 | `b92b6180e910b076a4de2458f94ed2ccb530e6d0db292ae70eb201561fd59589` |

`Unit::do_job` selects BUILD through the 14-byte thunk at `0x00617A7D`; its call at
`0x00617A80` targets `Unit::do_build`.

The saved witness is
`ron-data/savegames/new save game 2026.08.11 15'42'57 (Tue).SVX`:

- file SHA-256 `161af6242fe17f4780beac488097aa383d743bb93eb94594c53ceb4fb325f3d7`;
- decompressed 2,923,161-byte payload SHA-256
  `fa31f89f9bf435e28847e9e54d62cfe712e6bafc6cedb9ab98c8ab6730adb7c8`;
- structurally walked from Objects offset `0x4F21F` by
  `re/scripts/savegame_unit_orderlist_census.py`, SHA-256
  `31041e3d547a67e92c8044099e4c15f0e7fb68df4d716b8efb2091c4e0c5c93a`.

The sole order node on owner 2 Unit 3 occupies `[0x58FC0,0x58FD0)`:

```text
06 00 00 00 00 04 d7 07 00 00 02 00 00 00 10 00
```

The first four bytes are `OrderIndex::BUILD_AT`, then node metric zero, then the exact
11-byte `TargetOrder` walk image:

```text
04 d7 07 00 00 02 00 00 00 10 00
```

That payload has SHA-256
`026217de3bb8dbbaafc4dbde5f641bedd55c3b4144ff9f0147ad270c46dcc895`;
the complete node has SHA-256
`2c727e3424927616773913b0cdc595882237ac2460a90546ba661c40a875ded2`.
The order fields are flags `0x04`, target `o=2007`, `who=2`, `uid=16`.

The actor image SHA-256 is
`9c9c7ffc492a0b3da9cd1a549392f4306267cc73ea1fac883390e2ab7d38402c`.
It has coordinates `(4440,24648)`, angle `0x0DFA0000`, masks `0x0004040A`, an empty
Path, and one present Guy. The Guy image SHA-256 is
`9591baee4dd8bb895ff9f6e00305e5f83a55114a7e3184439d68de6f32fcc61f`;
its current animation/class is `0x21`, time is 12 of 14, and `hold_attack` is zero.

The Village image occupies `[0x5A822,0x5AA12)` and has SHA-256
`e919959a17ca603d5d901b75c55d4caa94df92d762caa96574bfe987c66684c3`.
Its relevant before-image is:

| Field | Before |
|---|---:|
| flags | `0x23` (valid, started, inactive) |
| coordinates | `(4704,23904)` |
| `job_counter`, `job_counter_2` | 25,100 |
| `constr_time` | 60,000 |
| `build_masks` | `0x0400` |
| `recharging` | 251 |
| `helpers` | 0 |

## Selected executable path

`Unit::do_build` resolves `(who,o)`, requires a valid inactive Wall/Build, observes the
builder adjacent and outside the non-Farm footprint, selects animation `0x21`, and computes
the target angle. The saved angle already equals `find_angle(target-builder)`, so there is
no angle write. The decoy bit is clear. Shipped `accel_construct` is 100, Korean bypass is
false, and the target is not under fire.

`Wall::do_construct(100)` sees `ai_speed=1`, a started inactive site, divides by
`helpers+1` (one), increments `recharging` once because latch `0x0800` was clear, sets that
latch, increments helpers, and adds 100 to both counters. `25,200 < 60,000`, so activation
and all completion effects are unreachable. The old and new progress fractions are about
41.83 and 42 percent, so the 84-percent side effect is also unreachable.

The immediate transaction is therefore:

| Field | Before | After |
|---|---:|---:|
| `recharging` | 251 | 252 |
| `build_masks` | `0x0400` | `0x0C00` |
| `helpers` | 0 | 1 |
| `job_counter_2` | 25,100 | 25,200 |
| `job_counter` | 25,100 | 25,200 |

The order is retained and RNG/effect counts are zero. `Unit::set_anim` performs the
same-value lead-Guy `hold_attack=0` store; `Guy::set_anim` takes its equal-class/time-before-
end return and performs no later animation or RNG mutation.

Later in the same step-14 traversal, `Build::process` calls `begin_frame_construction`.
It consumes the helper state, leaving helpers zero and masks `0x0400`; progress and
recharging remain 25,200 and 252. Tests distinguish this end-of-frame image from the
immediate receipt.

## Canonical ownership and atomicity

The adapter reads actor identity, generation, Unit columns, and the complete current order
from `World`; resolves the target through owner 2's Build band; validates the addressed
`BuildData` identity/UID; validates current type through
`LiveProductionRuntime::build_types`; and reads rules, global AI speed, and the owner's
Korean tribe bit from their canonical `Sim` stores. Adjacency, footprint/type predicates,
Guy container/animation, and virtual Wall validity are revision/digest-bound installed
authority and are intentionally absent from DoNSave.

Preparation runs both `construction_builder::preflight` and
`construction::execute_builder` against a staged Build clone. Commit reruns the entire
preparation and compares actor Handle, authority revision/digest, order, target registry,
site before-image, current type, rules, AI/tribe facts, and staged result before publishing
the five `BuildData` fields. Every refusal is non-mutating.

The saved `flags=0x23` scalar was already serialized by the Build section, but its validator
previously rejected the `0x20` flag despite needing no subordinate reconstruction. The
validator now admits that already-owned byte while retaining every wonder/gather/garrison/
special-family rejection.

## Tests

`canonical_build_at_runtime.rs` freezes binary/save hashes and literal bytes, mutation-kills
every adjacent branch, proves the exact five-field pure and staged transition, proves stale
authority cannot write, and compares direct execution with save/load, content/authority
reinstallation, and resumed `do_frame`. Missing or mutated authority cannot fall through to
the older compact-sim row-index compatibility path for this exact saved request.
