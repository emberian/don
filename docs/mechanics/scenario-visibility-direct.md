# Scenario reveal points and direct visibility

## Executable owner

This tranche owns four complete `ScenarioFuncSet` bodies and the scenario-point stage
they reach inside `GameDaemon::update_all_seen`:

| BHS builtin | shipped body | result |
|---|---:|---|
| `add_reveal_point` (#70) | `0x009E4750`, 177 bytes | validates one-based active player, tile bounds, and positive radius, then appends `{x,y,radius}` |
| `clear_reveal_points` (#71) | `0x009E4810`, 68 bytes | validates the active player and clears length without shrinking |
| `add_visibility` (#691) | `0x009FC710`, 150 bytes | sets the target bit in both `ScenarioData::ally_mask[source]` and `LeaderData::ally_mask`, then calls `update_all_seen` at `0x009FC78D` |
| `remove_visibility` (#692) | `0x009FC7B0`, 150 bytes | clears the same two bits, then calls `update_all_seen` at `0x009FC82D` |

Both visibility arguments are one-based and both leaders must satisfy
`(leader_flags & 3) == 3`. Invalid calls return `-1` without a write. Valid direct calls
preflight the whole reached refresh before publishing either ally-mask mirror. This is a
deliberate atomic adaptation of retail's store-then-call order: an unowned child cannot
leave a half-applied BHS command.

## Exact point owner and producer order

`ScenarioData::reveal_points` is `ObjectArray<ScenarioRevealPoint>[8]` at
`0x00ED63F0`, eight 24-byte headers. The dynamic initializer at `0x004131C0` constructs
each at `length=0,size=0,increment=-1,flags=0`; the first append therefore grows to four.
The walked point payload is the three signed words `x`, `y`, and `radius` after its
process-local vtable.

The scenario block at `0x00732A7D..0x00732BD8` runs when `Game+0x821 & 0x10` or
`Game+0x822 & 0x02` is set. It follows Build and Unit stamps and precedes frame-zero
explored sharing. For every VALID leader it:

1. rereads the **first** global point-array header, not the array indexed by the leader;
2. converts point tile coordinates with arithmetic `>> 1`;
3. computes fog radius `(radius * 0xC0) / 0x180`, clamped to `0..=64`;
4. walks the canonical circle table in index order, clipping each cell to the fog map;
5. calls `World::set_seen(cell, leader, detect=0)` and, on first exploration, calls
   `World::reveal_fog`.

The repeated array-zero read is a shipped executable quirk, preserved in tests: points
stored for slots 1-7 are checksum/save state but this producer body does not stamp them.
The commit writes canonical `seen`, `seen2`, `WData::was_seen`, and `wcoord_seen`; it
never writes detector plane `seen3`.

## Persistence, checksum, and atomic boundary

DoNSave v18 adds one `SCENARIO_DATA` leaf containing population overrides, all 31 find
counters, eight ally masks, and every reveal array's exact
`length,size,increment,flags` plus ordered points. V17 loads the retail-fresh owner.
`Sim::channel_digest` now includes the walked find counters, ally masks, array headers,
and point payloads; population overrides remain outside `ScenarioData::walk_data`.

Preflight clones `seen2`, walks Build, Unit, scenario points, and frame-zero sharing in
retail order, and proves every reached `World::reveal_fog` takes the currently owned
no-effect branch. A resource, oil, or item cell rejects before ally masks, daemon state,
or any World plane changes. Focused tests cover the two direct after-images, invalid and
effectful atomic refusals, BHS dispatch, empty-array growth/clear metadata, checksum
change, byte-identical save/resave, and resumed direct-call equality.

## Explicit residuals

This does not close the general visibility row. Still red are a nonempty dedicated Wall
band, effectful `World::reveal_fog`, `Build::close`,
`set_explored_show_buildings`, and incremental `Object::update_seen(1)`. Retail also sets
`Scene+0x229 = 1` after add/remove visibility; that is presentation invalidation, has no
canonical simulation/checksum owner here, and is not represented as a sidecar.

The bodies and offsets above are Tier C: derived from the exact shipped PE/PDB and
Capstone disassembly, with no retail execution claim.
