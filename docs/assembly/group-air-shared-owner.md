# Canonical AIR_PATROL shared owner

Status: AIR_PATROL order payload, dispatcher conversion, and DoNSave v13 tag 6 are executable;
opcode 11/36 package routing, scenario/type/containment commit, and production unit work remain red.

This tranche removes the first split owner identified by the air-action audit. A canonical
`order::Order` can now own the complete walked `AirPatrolOrder` image:

- both dynamic coordinate arrays, with independent `increment` and normalized flags bytes;
- the waypoint cursor;
- the secondary `AirOrder` base (`home_o`, `home_who`, cruise altitude, sharp-turn, old, and
  returning words).

`order_dispatch::adopt` and `publish` preserve that image in both directions. The executable
`PatrolPoints` owner now retains the array metadata as well as the values, so a load followed by
one dispatcher transition cannot silently normalize the saved payload.

## Save contract

DoNSave's already-reserved typed order tag 6/version 1 is now admitted. The body is the same
independently tested leaf frozen in `air_runtime_authority`:

```text
tag:u8 = 6, version:u8 = 1
x_count:u32, x_increment:i16, x_flags:u8, x_values[count]:i32
y_count:u32, y_increment:i16, y_flags:u8, y_values[count]:i32
waypoint:i32
home_o:i32, home_who:i32, cruising_alt:i32,
sharp_turn:i32, old:i32, returning:i32
```

Tag 6 is admitted only when the enclosing format is v13. A v7--v12 writer refuses a canonical
AIR_PATROL payload instead of silently dropping it, while v12's pre-existing tag-0 AIR_PATROL
image remains byte-identical and tag 6 remains reserved on read.

Decoding checks each count against the stream resource bound before allocation, rejects unequal
or empty routes, rejects allocator bit `0x40`, incoherent home identity, truncation, foreign tags,
and foreign order kinds. Saving validates the whole typed payload before writing the first order
byte. DoNSave versions 7 through 11 keep their byte-exact legacy image; v12's existing None/Move
images are unchanged, while AIR_PATROL is the v13 extension.

## What is still deliberately unavailable

This is not an opcode closure flip. `Sim::unit_work` still does not dispatch order 17 because its
real `AirPatrolHost` must atomically own air physics, home resolution, unit/building target search,
STRAFE insertion, path changes, and RNG. The package path likewise still lacks a single canonical
adapter over the fixed Groups pool, generational object registry, containment chain, synchronized
type relations, and scenario ignore-orders pruning.

The integration test therefore proves only:

1. canonical Order -> executable OrderRec -> canonical Order is lossless;
2. v13 save -> load -> immediate resave is byte-identical;
3. tag/version/count/flags/truncation and kind/payload mutations fail closed;
4. the current production tick does not fabricate a patrol transition or RNG draw.

Closure remains gated on exact packet -> Sim transaction -> save/load -> first resumed
`unit_work` evidence with the same after-image and RNG state as the unsaved branch.
