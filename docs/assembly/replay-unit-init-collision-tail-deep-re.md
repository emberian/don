# Replay setup Unit collision and common-tail deep RE

This tranche closes the ordinary-land setup seam left by
`replay-unit-init-location-deep-re.md`.  It applies the ordered
`Guy::set_new_location -> CollCheck::move_unit` journals to the canonical `World`, then
reconstructs the common return of `Unit::init` from `0x00612CD9` through `0x00612F7F`.
The executable producer is isolated in
`crates/don-replay/src/unit_init_collision_tail_deep_re.rs`; its tests import it by path so
the shared replay channel need not be edited before convergence.

Authoritative evidence:

- `ron-bin/riseofnations.exe` SHA-256
  `30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`;
- `ron-bin/sbl/rise.pdb` SHA-256
  `334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`;
- full Capstone/LLVM disassembly of `CollCheck::move_unit` `0x00682AD0..0x00682F05`
  and `Unit::init` `0x00612CD9..0x00612F82`;
- the PDB `UnitData`, `ObjectData`, `UnitTypeData`, and `LeaderData` layouts;
- the address-bound stable-Unit/Guy and location receipts from the preceding two producers.

No live process or VM observation was used, and no checksum value was fitted.

## Atomic seam

`produce_unit_init_collision_tail` accepts only a location receipt whose declared next
instruction is `0x00612CD9`.  It also requires:

- one collision journal for every live squad Guy, in increasing sparse pointer-slot order;
- stable `{id,generation,who,o,type,guy_num}` identities matching the final Guy array;
- ordinary land domain zero (other domain paths are deliberately rejected);
- four scalar stat receipts bound to both their native call site and child-body address;
- explicit type/leader predicates for the branches that retail actually reaches.

All receipt validation occurs before mutation.  The producer clones `World`, performs the
collision calls on the clone in journal order, derives per-block allocation/flag/bit deltas,
and commits the clone only after the entire Unit tail has validated.  A bad stat address, a
foreign identity, a malformed collision journal, or an unsupported domain therefore leaves
the original collision owner byte-for-byte unchanged.

## `CollCheck::move_unit` `0x00682AD0`

The producer reuses the instruction-derived implementation in
`don_sim::systems::collision::move_unit`; it does not approximate the footprint with a new
shape.  The complete native behavior relevant to setup is preserved:

1. A zero radius returns at `0x00682EFE` without reading or allocating a collision block.
2. The `0x00682AEB..0x00682BD2` fast path recognizes two footprints wholly inside one
   16-by-16 UCoord collision block.  Region disagreement returns, an absent block returns,
   and an admitted block is reused for both passes.
3. The old-footprint pass `0x00682BDC..0x00682D6A` walks the shipped `RING_X/RING_Y` prefix
   selected by `ring_count[radius]`, skips cells still covered by the new square, respects
   world bounds and terrain-region ownership, clears the bit, and changes a clean block's
   flag to `2`.
4. The new-footprint pass `0x00682D6A..0x00682EFC` symmetrically skips cells covered by the
   old square.  It enforces the destination terrain region, lazily calls
   `World::new_coll_block` for an admitted empty block, sets the collision bit, and changes a
   clean block's flag to `2`.
5. The shipped ring tables are used verbatim, including their known ordering quirks.  The
   footprint is the dense `(2r+1)^2` square; there is no invented parity filter.

Fresh setup Guys normally arrive from an out-of-world sentinel, so the clear pass is inert
and the set pass allocates/stamps their destination footprint.  The general clear, overlap,
same-block, region, and allocation behavior remains in the reused body rather than being
special-cased away.  Collision is per squad Guy.  Crew do not journal a move because
`guy_num >= squad_size`; air-domain squad Guys do not journal one either.

## Post-location Unit stores

The first post-location instruction is `or ecx,-1` at `0x00612CD9`.  The following table is
the complete direct synchronized-store prefix before the first child call:

| VA | `UnitData` field / offset | value |
|---|---|---:|
| `0x00612CDE` | `collide_o +0x8A` | `-1` |
| `0x00612CE5` | `collide_who +0xB3` | `-1` |
| `0x00612CEB` | `collide_guy +0x8C` | `-1` |
| `0x00612CF2` | `collide_frame +0x48` | `-1` |
| `0x00612CF5` | `announce_frame +0x14C` | `-1` |
| `0x00612CFB` | `o_up +0x8E` | `-1` |
| `0x00612D02` | `o_down +0x90` | `-1` |
| `0x00612D09` | `cavarch_o +0xA2` | `-1` |
| `0x00612D10` | `play +0xB6` | `-1` |
| `0x00612D16` | `collide +0x88` | `0` |
| `0x00612D1D` | `cavarch_who +0xA8` | `0` |
| `0x00612D23` | `cavarch_uid +0xA6` | `0` |
| `0x00612D2A..0x00612D42` | `openlist`, `openlistrefs`, `closedlist`, `validlist`, `blocklist` | null |
| `0x00612D48` | `start_dist +0x130` | `0` |
| `0x00612D4E` | `avoid_sea +0x13C` | `0` |
| `0x00612D54` | `avoid_land +0x138` | `0` |
| `0x00612D5A` | `avoid_y +0x124` | `-1` |
| `0x00612D60` | `avoid_x +0x120` | `-1` |

The remaining common-tail effects occur in this exact order:

1. `ObjectData::can_carry(2)` at call `0x00612D6A`, body `0x00646C40`, ORs instance
   `unit_masks +0x68` with `0x00200000` if any strict query for type `0x1BF`, `0x15F`, or
   `0x208` succeeds.
2. For setup's playable owner `< 8`, `LeaderData::leader_flags` gains `0x00800000`.
3. Calls at `0x00612D8F`, `0x00612D99`, `0x00612DA1`, and `0x00612DA8` dispatch respectively
   to `Unit::update_hits` `0x0060E930`, `Unit::update_los` `0x0060E4D0`,
   `Unit::update_speed` `0x006055C0`, and `Unit::update_armor` `0x006054C0`.
4. `Object::update_seen(0)` is invoked at `0x00612DB3` through virtual slot `+0x174` to body
   `0x00651B80`.
5. Unless `(leader_flags & 0x0C) == 4`, `unit_masks` gains `0x00040000`.
6. Non-strict type `0x3A` is queried at `0x00612DE0`; on success `UnitData::mana`
   `0x00609A50` is called and truncation-toward-zero `mana / 2` is stored in
   `spell_time +0x96`.
7. Non-strict type `0x143` is queried at `0x00612E7D`.  Success sets instance
   `unit_masks2 +0x6C` bit `4`; if leader flag `4` is clear, `set_stance(3,0)`
   `0x00605310` runs.  Failure clears bit `4`.
8. `LeaderData::has_tribe_bonus(19)` is called at `0x00612EA8`.  With nonzero Constants
   `+0x848`, type IDs `0x32/0x33`, non-strict type `0x45`, or type-line `0x1AC` set leader
   flag `0x02000000`.  The type-`0x45` query is short-circuited for IDs `0x32/0x33`; when
   reached its devirtualized call is `0x00612EE4`.
9. `LeaderData::has_tribe_bonus(20)` is called at `0x00612F07`.  With nonzero Constants
   `+0x888` and type-line `0x1AB`, a false non-strict type-`0x45` query at `0x00612F3F`
   sets the same leader flag.

`ObjectData::is` `0x00653790` is a 12-byte type-query forwarder.  The optimized sites compare
the instance vtable slot against that address and then call the type vtable's `+0x60` slot
directly; receipts name the semantic forwarder while preserving each reached native call VA.
The Lakota and Americans type-`0x45` entries are emitted only when their native short-circuit
gates reach them.

## Common-tail control-flow braid

The apparent backwards jump at `0x00612E1F/0x00612E33/0x00612E42` is part of the earlier
special/citizen entrance to the Guy/location lattice.  Ordinary types `0x32..0x35` enter that
lattice once through `0x00612DE5`, eventually return to `0x00612CD9`, and then take the
devirtualized `is(0x3A,0)` route from `0x00612DCE`; they do not loop and place the Unit twice.
This is why the producer accepts one completed location receipt and then runs the common
tail once.

Because `o_down` was just set to `-1`, each of the four stat child bodies has exactly one
synchronized Unit-channel result at this seam: `myhits`, `mylos`, `myspeed`, or `myarmor`.
The continuation therefore accepts address-checked scalar receipts and range-checks the
byte/word stores.  It does not synthesize those values from replay checksums.  Similarly,
`set_stance` has no subordinate Unit to propagate to and stores only the new stance value.

No instruction in `CollCheck::move_unit` or `Unit::init 0x00612CD9..0x00612F7F` calls the game
RNG.  The receipt proves `rng_before == rng_after == location.rng_after`.  This tail also
writes no City POD field; City checksum authority remains outside this setup Unit chain.

## Exact first external residual

Collision blocks, Guys, every direct Unit tail field, the two Unit masks, stance/spell state,
and the reached leader flags are owned by this transaction.  The first unapplied world
mutation in native chronology is:

```text
Unit::init call 0x00612DB3
  -> Object::update_seen(0) 0x00651B80
     -> first newly-explored cell enters World::reveal_fog 0x006B3D30
```

The returned `VisibilityUpdateRequest` binds the stable Unit identity, caller/body addresses,
owner/object slot, coordinates, angle, source `mylos`, domain, type `unit_flags2`, instance
`unit_masks` as it exists at the call, object flags, and `infiltrated=0`.  `source_mylos` is
deliberately not mislabeled as the final LOS: `Object::update_seen` first calls the Unit LOS
override at virtual slot `+0x128`, which may add leader/general bonuses.  The existing
step-12 visibility authority owns that resolution, the exact small-LOS projection receipt,
fog-plane writes, and `World::reveal_fog` object/item/leader effects.  Skipping that body is an
explicit residual, not a claim that setup visibility is inert.

## Runnable receipt

Persvati job
`unit-init-collision-tail-deep-re-final-20260811T182805Z-14215-24846-27c48c816a79` ran:

```text
cargo test -p don-replay --test unit_init_collision_tail_deep_re
```

Result: **8 passed, 0 failed**.  The suite constructs the actual stable Unit/Guy prefix and
the actual location continuation before invoking this producer.  It covers the fresh
radius-one allocation/stamp, radius-zero native no-op, complete sentinel/stat/mana/stance/
mask/leader results, Lakota/Americans gates and query short-circuits, leader-flag-four
suppression, non-land refusal,
and precommit rejection of a forged stat-body address.

After convergence, the only replay-library registration required for this exclusive source
is:

```rust
pub mod unit_init_collision_tail_deep_re;
```
