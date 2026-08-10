# `Unit::come_out(int)` common-release frontier

Status: second measured, source-only transaction planner; not integrated and not a
full-function closure.

## Provenance and owned addresses

The authoritative image remains `ron-bin/riseofnations.exe` (SHA-256
`30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`) with matching
`ron-bin/sbl/rise.pdb` (GUID `51D4F219-61C6-4F84-9D5B-C3361B0D291F`, age 1). The PDB fixes
`int Unit::come_out(int)` at `0x00617C10`, 9,925 bytes. Ghidra decompilation of that exact
PDB-bound function was cross-checked instruction-for-instruction against local Capstone PE32
reads; retail and both remote hosts were untouched.

The prior typed seam is `0x006186B4`. This tranche owns the sequential interval
`0x006186B4..0x00618B21`, 1,134 bytes, and the three compiler-outlined virtual-call islands
used by that interval:

- `0x0061A24E..0x0061A25E`: non-fast-path `UnitData::is_plane` dispatch;
- `0x0061A25F..0x0061A267`: non-fast-path `Unit::is(0x13B, false)` dispatch;
- `0x0061A268..0x0061A270`: non-fast-path `UnitData::is_captain` dispatch.

That is 1,169 newly recovered logical bytes and leaves 6,032 of the original 7,201-byte
residual. The model is isolated in
`crates/don-sim/src/systems/unit_come_out_common_release_frontier.rs`; no dispatcher or live
world owns it yet.

## Exact chronology

| Retail VA | Recovered operation |
|---|---|
| `0x006186BC` | `Unit::set_new_location(x, y, 1, 1)` at the point carried by the first seam |
| `0x006186F8` | when `UnitData::unit_masks2 & 0x00010000`, call `Unit::update_ceo_position` with retail tile/sentinel arguments |
| `0x00618711` | domain 2 calls the first `Guy::update_z()` |
| `0x00618734`, `0x00618737` | if UnitType flags include `0x20`, set first Guy `last_z` and `z` to post-update `z + 500` |
| `0x0061879C` | active same-owner `UnitData::o_down` target recursively receives `come_out(1)`; result is ignored |
| `0x006187B9` | exact actor types `0x34`/`0x35` call `set_anim(0, 1, 1)` |
| `0x00618813`, `0x0061881E` | eligible non-plane/plane exit clears UnitData `unit_masks` bit `0x04000000` and `path.length` |
| `0x00618828`, `0x0061882F`, `0x00618836` | then `close_orders(0)`, `clear_partial_path()`, `update_action()` |
| `0x0061887C..0x006188D8` | `Unit::is(0x13B,false)` moves the order-list cursor to its head (twice, as shipped) and reads `UnitOrder::get_strafe_order()` when current data is non-null |
| `0x006188FA` | a target with both 32-bit fields unequal to `-1` increments `LeaderData::nukes_launched` |
| `0x00618994`, `0x006189A5` | captain/build and `UnitTypeData::uber_size > 1` adds actor to the scratch Group and pushes it into owner Groups |
| `0x006189CB..0x00618A39` | active, uncontained builds with a city whose `city_flags & 0x40` scan owner object slots |
| `0x00618A7C..0x00618AD0` | active workers with GARRISON action type `0x1A` refresh the action and read `UnitOrder::update_garrison_order()`; first target match to the containing build stops the scan |
| `0x00618B1D` | exhausted scan plus leader flag bit 2 clears only city flag `0x40` |

The cleanup predicate preserves retail's fast-target distinction. With vtable slot `+0xC0`
equal to base `UnitData::is_plane`, retail does not call it and uses:

```text
leader_flags & 4 == 0
&& (domain != 2 || unit_type_flags & 0x20 != 0)
```

With an overridden slot, retail calls the override and uses
`leader_flags & 4 == 0 && !is_plane_override_result`; it does not then reapply the
domain/type-flag test.

A non-captain, missing direct container, or non-build direct container exits to the fallback
setup at `0x0061919D`. A captain inside a build exits to the gather-list body at
`0x00618B22`, carrying the exact scratch-group result when retail created one.

## Receipt and atomicity contract

There is no direct `Random::get` call in the sequential interval or its three outlined islands.
The planner nevertheless requires a continuous `RngStamp` receipt for every stateful or
result-bearing host call, in actual instruction chronology. This includes the broad
`set_new_location` child, the optional recursive `come_out(1)`, order/action refreshes, and
scratch Group publication. A receipt must start at the preceding receipt's ending stamp; seed
change without a draw, wrong call kind, missing/extra receipt, malformed return, or recursive
return outside `{0,1}` fails closed.

Worker observations are also reachability-bounded. With no matching worker the observations
must cover every owner object slot; with a match they must stop exactly on the first matching
slot. Unreached city, worker, first-Guy, order-head, and captain-container payloads are rejected.

The output is only an ordered `CommonReleasePlan`. No step is applied while facts or receipts
are being validated, preserving opcode 49's mandatory all-or-nothing transaction. Integration
must atomically compose:

```text
action_come_out wrapper preflight
  -> first Unit::come_out prefix plan
  -> common-release plan (this frontier)
  -> remaining gather/order/army/late-RNG body
  -> one publication commit
```

Neither opcode 49 nor Step 8 may publish either prefix before the remaining 6,032 bytes and
their host authority are closed.
