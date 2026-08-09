# Construction builder policy

`crates/don-sim/src/systems/construction_builder.rs` ports the deterministic unit-side
policy around the site lifecycle. It contains no alternative distance, stance, or
assignment heuristic. Every world lookup is an explicit input and every output is an
ordered mutation plan for the order/group host.

Evidence is the shipped PDB plus instruction/decompiler reads of:

| body | VA |
|---|---:|
| `Unit::add_build_order` | `0x005E5210` |
| `Unit::do_build` | `0x005EEBF0` |
| `Unit::check_build_order` | `0x00603470` |
| `Unit::build_done` | `0x00603BF0` |
| `Unit::find_build_spot` | `0x00603E20` |

All policy helpers consume zero RNG directly. Applying a plan can reach animation, unit
death, group, terrain, or spawn code; the construction runtime must retain the transitive
receipt from those calls.

## Direct `do_build`

Direct execution deliberately validates only `(whom, ox)` and `is_valid_wall()`. It does
not compare the order's saved UID. This differs from `check_build_order`, which rejects a
slot whose live UID differs. `preflight` preserves the asymmetry.

The exact head is:

1. Negative address or invalid Wall: bare-kill BUILD_AT. Keep a newly exposed queued action;
   if none remains, enter `build_done`.
2. Active target: bare-kill BUILD_AT and enter the active-target tail.
3. Not adjacent, or builder tile covered by a non-Farm target: bare-kill, create a temporary
   one-member group, then `action_swarm_around(..., FIRST, BUILD_AT, old_group_flag)`.
4. Otherwise select `CHAR_SOW` (`0x23`) for Farm or `CHAR_BUILD` (`0x21`), compute
   `find_angle(target-builder)`, and call `set_angle` only when it differs.
5. Raw `unit_masks & 1` (`UNIT_DECOY`) stops after animation/facing with no contribution.
6. The ready branch enters `construction::execute_builder`.

Farm's footprint exception is load-bearing: a citizen standing in a Farm's covered tile is
allowed to sow, while the same geometry inside any other building reswarms.

## Active and completed target tails

An active target with another action always routes to `check_build_order`. With no next
action, auto-gather requires:

```text
(target is Oil Platform || (!AI && stance in {GATHER, BUILD_AND_GATHER}))
&& builder.who == target.who
&& target is gather type
&& target is not University
```

Success adds GATHER with `QUEUE_NEW`, removes the builder from a multi-member selection
group, and writes `group=-1`; failure enters `build_done`.

Completion is selected only after `Build::activate(0,1,1)` has returned and BUILD_AT has
been bare-killed. A queued action normally calls `check_build_order`. The exception is a
gather-type non-University target with a grouped builder whose normalized group count is
strictly greater than one; it is allowed through the same auto-gather test. An empty queue
tries auto-gather directly and falls back to `build_done`.

## `build_done`

If a real queued action remains, nothing else runs. The AI order is:

```text
find_build_spot
find_repair_spot
if wonderwin == 8: stop
find_gather_spot
```

The non-AI order is:

```text
if stance in {BUILD_AND_GATHER, BUILD}: find_build_spot
if stance in {BUILD_AND_GATHER, GATHER}: find_gather_spot
if stance in {BUILD_AND_GATHER, BUILD}: find_repair_spot
final target fallback
```

Successful non-AI gather search detaches the selection group. The final fallback notably
does not validate target flags or UID. It requires a nonnegative `ox`, same owner, gather
type, neither University nor Oil Platform, and:

```text
num_gatherers < sign_extend(BuildData::gather_max)
```

Equality is full; it does not admit another gatherer.

`build_done_plan` is the pure policy oracle for already-known results. It is not the
runtime adapter: each successful `find_*` body has already installed an order, so eagerly
evaluating all three results would mutate state retail never reaches. `execute_build_done`
and its mandatory `BuildDoneHost` invoke only the next retail branch, short-circuit on the
first success, defer the final target lookup until every applicable search fails, and
reject any unexpected RNG reported by these zero-draw bodies.

## `check_build_order` candidate balancing

Queued candidate validation does compare `(who,o,uid)` and active state. A valid inactive
target becomes a candidate. Raw target flag `0x20`, nonzero worker stance, or nonzero caster
stance stops the scan at that candidate; otherwise the old action is retired and scanning
continues.

For multiple candidates, retail scans friendly `FILTER_BUILDREPAIR` units in range and
counts current BUILD_AT actions aimed at `(builder.who, candidate_o)`. The comparison ignores
UID. The scanning builder itself is excluded. A strict `<` selects the least-contended site,
so the earliest candidate wins ties.

Retail swaps the winner into candidate slot zero, then calls
`add_build_order(candidate, builder.who, FIRST, group=1)` in reverse. Repeated front
insertions reconstruct the winner as the first executable action while retaining the
deterministic remaining order. `balance_candidates` returns both the reverse call order and
the final execution order so an adapter cannot confuse them.

## Host contract

The host applies returned plans using retail operations, not field-only approximations:

- bare `kill_current_order(0)` versus `repath(); kill_current_order(0)` must remain distinct;
- reswarm uses a temporary Group and `QUEUE_FIRST`;
- animation/facing runs before the DECOY gate and propagates into Guy/group state;
- activation returns before builder order retirement;
- `check_build_order` uses `update_action` behind queued move legs;
- selection removal and `group=-1` occur only on successful auto-gather arms;
- generic Unit death retains its one shared RNG draw, or three when reason is 4, under the
  measured corpse gates. Ordinary order cancellation and all policy bodies draw zero.

These policies are locally executable and tested, but the construction runtime gate remains
closed until their host mutations are wired to full unit/group/object stores and compared
against retail oracle traces.
