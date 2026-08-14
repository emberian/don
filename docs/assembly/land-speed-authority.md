# Land-speed authority for canonical Group→Move

Status: source-only prerequisite, fail closed. No Wasm artifact, browser method, capability bit,
or product default is enabled by this tranche.

## Retail body and exact chronology

The authoritative body is `UnitData::speed` at `0x0060AAE0` (1,358 bytes). Its static
`UnitData+0x9A` `myspeed` is only the starting value. A non-land type (`domain +0x218 != 0`)
returns it immediately. Land units execute these branches in order:

1. A non-strict `ObjectTypeData::is(IRQ_SPEAR=0x0A6, 0)` enables the territory arm. The
   object's coordinate selects `WorldData::WData.who +0x0F`; `LeaderData::is_ally`
   `0x006EDB50` requires the mutual relation. Strict type tests `0x0A6`, `0x0A7`, `0x0A8`,
   `0x0A9` add `Constants+0x838`, `+0x83C`, `+0x840`, `+0x844`, respectively.
2. `leader_flags & 0x8000` enables the aura arm. A qualifying hero Unit with
   `unit_masks & 0x8000` answers directly; otherwise `HeroesData::find_hero` at
   `0x0073A1B0` scans the owner-local registry in order, validates its live on-map object,
   applies `HeroData::get_radius * 0xC0`, then the mask. Alexander (`0x166`) or Napoleon
   (`0x167`) on either the actor or selected hero yields
   `Constants+0xC50 * (+0xB4C) * (+0x004) / 256`; otherwise it yields
   `+0xC50 * +0x004`. The result is compared with original `myspeed`, so a qualifying aura
   replaces, rather than adds to, the terrain-adjusted value.
3. Five later modifiers run sequentially:
   Spitamenes `0x16D` while direct `where == Stable 0x1AC`, `* (+0xB78) / 256`;
   Blucher `0x175` while Stable, `* (+0xBD4) / 100`;
   Porus `0x16F` for non-strict Heavy Elephant `0x105`, `* (+0xB7C) / 256`;
   Charles `0x171`, `* (+0xBC0) / 100`; and Napoleon `0x167` for
   `UnitTypeData::is_siege`, `* (+0xBB0) / 100`.

`ObjectData::has_general` at `0x00646B00` first accepts a Unit that is itself a hero and
matches the requested non-strict type. Otherwise it honors the exact optimized
`LeaderData::num_units[type - 50]` gate and delegates to the same ordered HeroesData scan.
For a Unit receiver the Build-size extra-radius arm is unreachable, so its extra radius is
exactly zero.

`ObjectTypeData::is` at `0x00661AE0` has two distinct relations. Identity always succeeds.
The non-strict arm accepts `graft +0x25C`, then walks `from +0x3C` and each ancestor's graft.
The strict arm accepts only identity or a Unit's graft to a target Unit whose
`unit_flags +0x2B4 & 0x01000000` is clear. Cycles and missing ancestors are authority faults.

## Canonical owners and atomic contract

`land_speed_authority.rs` joins the existing canonical Sim owners without copying answers:

- `World.units`: generation-safe Handle, uid, owner-local `o`, type row, coordinate,
  `myspeed`, masks, active/on-map and containment identity;
- `MapState.world.wdata`: the exact checksum-owned territory cell;
- `vic_leaders`: mutual diplomacy, `leader_flags`, and the five exact `num_units` cells;
- `step12_visibility`: the ordered identity/radius HeroesData projection;
- `LandSpeedContent`: composition revision/digest, exact `from`/`where`/`graft`, domain and
  type flags, plus all twelve Constants cells.

Production is a pure all-or-error pass over every live land Unit. It rejects missing type or
Constants rows, malformed/mismatched counters, incomplete/duplicate/stale hero identities,
invalid terrain owners, relation cycles, and stale generations. The result binds its Sim and
content for the lifetime of the Group-Move projection; any relevant state, composition,
terrain, diplomacy, hero, mask, speed, or generation mutation makes the prior snapshot fail.
The existing Group-Move producer remains the owner of path availability, on-map,
captain/subordinate, containment, blown-unit, entering/exiting `SpecialAnim`, and destination
water/effective-type gates.

## Replay-carried content owner

`replay_land_speed_content` now supplies the immutable side of that contract directly from the
recording's admitted Rules section. It revalidates the decompressed payload SHA, the complete
serialized Rules SHA and checkpoints, projects all 364 shipped Unit rows (including direct
`from`, `where`, and `graft` relations), and reads the twelve Constants cells from their exact
runtime offsets inside the serialized Constants image. Its revision and composition digest bind
the complete Rules section. It neither depends on the separately installed DONPACK nor turns a
Unit row's `MOVES` value into a resolved speed.

`produce_replay_setup_group_move_authority` additionally requires that this replay content and
the canonical setup receipts name the same raw replay-file SHA. It then resolves live speed and
produces Group-Move authority against one immutable Sim borrow. This closes the downstream
content adapter for replay setup, while still requiring the real post-worldgen Unit, WData,
Leader, diplomacy, hero-registry, path, and collision owners.

## DONPACK4

`DONPACK4` replaces `DONPACK3`; older packs are rejected by magic and shape, never defaulted
into speed authority. Each of the 364 Unit records now carries `from`, direct `where`, and
`graft` (28 fields total). The rules block now carries the twelve Constants cells above (28
rules total). The complete bytes produce the content revision and 32-byte composition digest.
The synthetic fallback has neither Constants nor authoritative type facts and therefore cannot
install land-speed authority.

The shipped live Constants specimen resolves to `{+0x004:1, +0x838..+0x844:1,
+0xB4C:384, +0xB78:307, +0xB7C:307, +0xBB0:150, +0xBC0:120, +0xBD4:120,
+0xC50:42}`.

## Activation gate

This source tranche does not mount authority in `game_abi`, rebuild the canonical Wasm, or
advertise Group→Move. Activation remains red until a real browser lifecycle can install the
same live terrain, leader counters/diplomacy, ordered hero registry, exact pack, paths,
containment and animation facts, then demonstrate two-tab package receipt plus native/Wasm
frame, digest, and RNG agreement.
