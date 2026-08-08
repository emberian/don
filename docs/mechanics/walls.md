# Walls recovery boundary

Status: **Tier C; compiled base-class and channel primitives, not connected to the live
world.** The implementation lives in `crates/don-sim/src/systems/walls.rs` and is declared by
`systems/mod.rs`. Its focused suite passes 43 tests after the recovery audit. No wall/building
routine here has been differentially exercised against retail.

The evidence boundary is:

- `riseofnations.exe` supplies instruction-level control flow, constants, walk order, and
  static references;
- the matching shipped PDB supplies class, function, field, and vtable identities;
- local Rust tests check the transcription and byte-layout contracts only.

PDB names are identity evidence, not behavioral evidence. These results remain Tier C.

## What `Wall` means in this engine

The shipped PDB lays out this inheritance chain:

```text
SubObjectData -> SubObjectOut -> SubObject -> ObjectData -> ObjectOut -> Object
  -> WallData -> WallOut -> Wall -> BuildData -> BuildOut -> Build
```

`Wall` is therefore the construction/health/visibility base of every building, rather than a
player-drawn fence system. The reverse-call inventory reported by the interrupted lane found
the corresponding `Build::*` wrapper as the sole direct caller of each `Wall::*` method;
`Wall::inc_time` is shared through the same vtable slot instead of a direct wrapper call.

The Rust module recovers these local pieces:

| area | retail source | implemented boundary |
|---|---|---|
| object bands | `Objects::init` `0x0065EA80`, `Objects::clear` `0x0065D740` | unit/build/wall band constants and the eight-leader wall-channel loop |
| walked state | `SubObject::walk_data` `0x006621D0`, `Object::walk_data` `0x00647830`, `WallData::walk_data` `0x00642510` | walked scalar bytes plus an engine-shaped `SimpleArray<int>` header for `launching` |
| construction | `Wall::do_construct` `0x006434D0` | start/reject gate, same-frame helper divisor, credited work, and completion latch |
| hit-point slices | `Wall::update_hits` `0x0063F0D0`, `BuildData::hits` `0x0062E740`, `WallData::armor` `0x0063FA60` | construction ramp, razing interpolation, and inactive-armor halving; not the full modifier chain |
| footprint | `WallData::tile_corner` `0x00643440`, `covers_tile` `0x006439B0` | footprint rectangle over an inferred coordinate-to-tile helper |
| per-frame bookkeeping | `Wall::process` `0x00640450` | targeted decay, 16/32-frame phase flags, seen toggles, and helper reset/latches |

## The retail `walls` channel is structurally empty

`Objects::init` sets `obj_base[2] = 3000` and `obj_end[2] = 3000`, leaving the dedicated wall
band with zero capacity. `Objects::clear` resets every `wall_mark` to 3000. The interrupted
lane's static scan found no other writer to those marks, found no direct caller of
`Objects::init_wall` (`0x00658C50`), and found no wall type in the shipped building/type data.

On that measured retail shape, `CheckSums::check_walls` (`0x00937360`) walks zero objects, so
its Adler-32 accumulator remains the seed `1`. This is a static Tier-C conclusion about the
examined binary, not a formal proof and not evidence that the broader building base class is
unused. `BuildData::walk_data` calls the `WallData` walker inside the separate `builds`
channel.

`WallChannel` can also walk synthetic non-empty bands for local byte-order tests. Those bands
are test scaffolding, not states known to occur in retail.

## Recovery-audit corrections

Two concrete defects were fixed while landing the file:

- `WallState::launching` used `Option<Vec<i32>>` and emitted elements without the
  `SimpleArray<int>` length/capacity/increment/flags header. It now uses `EngineArray<i32>`
  and reproduces the checksum/write-path header, including clearing transient flag `0x40`.
- `mark_ever_seen_by` clamped owner slots 8..31 onto bit 7. Retail shifts a 32-bit `1` by
  `owner & 31` and then truncates to the byte at `WallData +0x62`, so those slots write zero.
  The implementation and regression test now preserve that behavior.

The module comment was also corrected to avoid the stale claim that `CheckSums::check_all`
returns the sum of all channels. It resets every channel to Adler seed 1; the checksum command
serializes fifteen channel values plus their total, as documented in
`docs/mechanics/COVERAGE.md` §1.1.

## Wiring status

No code outside `walls.rs` constructs a `WallChannel`, calls its checksum, calls
`do_construct`, advances `WallState::process`, or applies the recovered hit-point helpers.
`World::step` does not own retail-shaped building state, and the replay bridge does not
populate the `WallData` base of buildings.

Declaring the module therefore means “compiled and locally tested,” not:

- building construction executes through these routines;
- the `builds` channel includes this `WallData` prefix;
- the replay `walls` result is measured from a populated world;
- the complete building hit-point, armor, territory, or activation behavior exists.

## Honest gaps before integration

1. Give the world a retail-shaped building store and invoke the `Wall` base operations from
   the matching `Build` lifecycle points in retail order.
2. Connect `WallData::walk_data` as the base prefix of the `builds` channel, preserving stable
   slot identity and every `EngineArray` capacity/growth field.
3. Derive and port the complete `Wall::update_hits` percentage chain. `update_hits` currently
   accepts its already-computed `full_hits`; it does not establish that value.
4. Port activation, initialization, closure, ownership swaps, terrain masks, site clearing,
   and the omitted world/leader effects surfaced by `ProcessEffects`.
5. Replace `world_to_tile`'s documented floor-division inference with the retail lookup-table
   behavior over the relevant coordinate domain.
6. Populate building/wall state from replay or save setup and make both the `builds` and
   structurally empty `walls` channel comparisons non-trivial in the validation harness.
7. Add registered oracle cases before promoting any mechanic above Tier C.

Until then, the module is useful low-level recovery work and an explicit map of what remains;
it is not an end-to-end building or wall simulation.
