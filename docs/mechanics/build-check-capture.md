# `Build::check_capture`: capture admission, strength census, and transfer

Implementation tranche: `crates/don-sim/src/systems/combat/build_check_capture.rs`.

Fidelity tier: **C**. The complete 2,357-byte body at
`0x006276A0..0x00627FD5` is recovered from the pinned retail executable and PDB. The
world circle/down-chain projection and the large nested transfer callees remain mandatory
typed boundaries; no retail differential oracle has promoted this lane yet.

## Pinned provenance and caller composition

- `ron-bin/riseofnations.exe` SHA-256:
  `30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`
- `ron-bin/sbl/rise.pdb` SHA-256:
  `334a3ea1f96e65c0bd7d045449e2cc68d81c51923020f9728e80d1e508d9bff5`
- PDB procedure: `int Build::check_capture(int o, int who)`, VA `0x006276A0`,
  size `2357` (`0x935`) bytes, exclusive end `0x00627FD5`.

The upstream `damage_world` capture-attempt seam calls this body as
`victim.check_capture(attacker.o, attacker.who)`. A nonzero return takes the immediate
`Object::do_damage` epilogue at `0x0064C550..0x0064C558`. Zero enters
`damage_fallthrough`'s recovered notification/raid/charge continuation at
`0x0064C558..0x0064C86B`. This includes the surprising failure edge where capture strength
succeeds, `Build::swap_team` returns negative, the old build is killed, and
`check_capture` still returns zero.

## Admission gates, in retail order

`BuildData::check_capture_eligible` (`0x0062D1D0`) is small enough to inline into the
contract. It requires `BuildTypeData::is_city()`, the virtual active result (the base
`WallData::is_active` is `flags & 4`), and `health_level >= 6` for a city-center-flagged
object (`flags &0x20`) or `>= 5` otherwise.

After eligibility:

1. `inside_down >= 0` sets `BuildData::build_masks |= 0x4000` and returns zero.
2. A same-owner or mutual raw-`diplos == 2` attempt is rejected while the victim leader has
   `leader_flags &2`. The attacker must itself have `leader_flags &2`.
3. The attacker must have land domain `0`, answer `is_unit()`, lack `unit_masks &1`, have
   nonzero `attack()`, and lack `ObjectTypeData::obj_masks &0x08000000`.
4. The city's `frame - capture_stamp` must be at least 75 frames.

The body reads the victim city record even for the later non-city-center transfer arm.

## Radius and census

The radius rule is `CITY_CAPTURE_RADIUS` (`Constants +0x134`) for `flags&0x20`, otherwise
`UNIT_RESPOND_RANGE` (`+0x18`). Retail multiplies the rule by `0xC0` world-coordinate units
and selects circle ring `(radius + 0x2FF) / 0x300`.

For every `circle_x/y` cell in that ring, the body requires a valid world coordinate and
the same `WData::region` as the victim, then walks `WData::down/down_who` followed by each
object's linked `down/down_who` chain. It skips the victim and triggering attacker. A
contributor must be alive (`flags&1`), not domain 1 or 2, lack `unit_masks&1` when it is a
unit, pass exact `Search::valid_filter(o,-1,-1,8)`, and lie within the scaled radius by
retail `vector_dist`.

The triggering attacker's `get_capture_value()` seeds both its per-owner bucket and the
attacker-side total. The defender seed is `CityData::capture_strength` while
`frame-capture_stamp < 900`, otherwise exactly `2`. Every contributor adds its capture
value. A nonzero defending build additionally contributes 6, another 6 if it is a fort,
and `num_inside(1)`.

That defending-build arm also mutates object state before the outcome is known: if the
attacker's bit is absent from `ObjectData::visible`, it sets the bit and immediately calls
`Wall::update_local_seen()`. Therefore a defender-held/zero-return attempt can still reveal
objects. The planner simulates the visible byte so repeated appearances cannot emit the
transition twice.

Side attribution is directional and is not reducible to the raw team test:

- owner == attacker, or `attacker.is_ally(owner) && victim.is_enemy(owner)` adds to attack;
- otherwise owner == victim, or `victim.is_ally(owner) && attacker.is_enemy(owner)` adds to
  defense;
- ambiguous contributors remain in their per-owner bucket but join neither total.

Capture requires `attacker_side > defender_side`; equality defends.

## City-center success

For a city center, retail selects the strictly largest positive per-owner bucket among
leaders satisfying `(leader_flags&3)==3` and either being the triggering attacker or
answering `leader.is_ally(triggering_attacker)`. Iteration is player 0 through 7 and ties do
not replace the earlier winner.

Ownership is then re-homed with a strict priority: an active founder allied to that winner,
if distinct from old owner and winner; otherwise an active allied `race` under the same
conditions; otherwise the strength winner. The call is
`Cities::capture_city(new_owner, old_city_index, old_owner)`. Its returned city index is
used to write the new record's `capture_strength` to the *triggering attacker's original
capture value*, not the aggregate attack total and not the winning ally's bucket. The
function returns 1.

`Cities::capture_city` is an exact residual boundary. It is 7,998 bytes and owns all member
building reassignment, plunder/resource/economy effects, diplomacy/score effects, and
center/dependent-object transfer. The boundary receipt must affirm all four cohorts and the
new city identity; it cannot silently substitute the much smaller `City::capture` model.

## Non-city-center success and presentation boundary

The generic arm calls `old.swap_team(triggering_attacker)`. Negative return calls
`old.die(0,-1,0)` and returns 0. Success optionally plays sound category `0x86` for the
local old owner and `0x7C` for the local attacker. If either is local, a typed presentation
boundary owns `ObjectData::say_name`, localized string composition, decoded new-object
coordinates, and red `MessageWin::add_message`.

The checksum-visible tail order is fixed:

1. new build `activate(0,1,0)`;
2. old build `close(0,-1,0)`;
3. new wall/build `mask_me(1,2)`;
4. return 1.

This composes with the existing Wonders lifecycle lane, but the generic `Build::swap_team`,
`activate`, `close`, and `mask_me` bodies remain typed object/world boundaries here.

## Frozen residual boundary

The recovered body has no unidentified branch or scalar field left. Promotion work is:

- project the engine's static `circle_radius/x/y` tables and world down chains into the
  typed neighborhood adapter;
- recover `FilterIndex` enumerator names (the load-bearing numeric value is frozen as 8);
- implement and verify `Cities::capture_city`'s 7,998-byte building/plunder/resource/
  diplomacy/score/object transaction;
- replace generic build swap/activate/close/mask and localized presentation adapters with
  concrete world owners;
- add retail differential cases for defender-held reveal, stale 900-frame strength reset,
  allied strength-winner/founder/race re-home, swap failure, and the caller return edge.

No part of `Object::do_damage` before the landed capture-attempt seam or after the landed
capture-zero seam is claimed by this tranche.
