//! # Aircraft, airbases, and the anti-air roll
//!
//! A port of the *air* domain of *Rise of Nations: Extended Edition*, derived from
//! `ron-bin/riseofnations.exe`, `ron-bin/sbl/rise.pdb` and the shipped `ron-data/*.xml`.
//! Everything is `[measured]` against those artifacts unless a comment says `UNDERIVED`.
//!
//! ## Why this module exists: the anti-air roll is an RNG consumer
//!
//! `docs/mechanics/ammo.md` §7.3 flagged an open hole: `Ammo::init` (`0x0067BBF0`) draws
//! **one or two** `Random::get(0, 0xFFFF)` values from the *main simulation stream* every
//! time a projectile is spawned at an air unit, and the ammo lane left it unimplemented.
//! Every shot at an aircraft therefore shifted our stream position relative to retail, and
//! no replay containing an air unit could validate.
//! [`antiair_dud_gate`](crate::systems::air::antiair_dud_gate) closes that hole:
//! it reproduces the branch structure of `0x0067BE49..0x0067C16A` instruction for
//! instruction, so the **number of draws** — 0, 1 or 2 — matches for every combination of
//! shooter, target and order.
//!
//! ```text
//! Ammo::init  0x0067BBF0
//!   ...
//!   0x0067BE4F  a.index       = ord.index          (AmmoData +0x34)
//!   0x0067BE55  a.graph_index = ord.graph_index    (AmmoData +0x38)
//!   0x0067BE49..0x0067C16A   <-- THE ANTI-AIR GATE — 0, 1 or 2 Random::get draws
//!   0x0067C172  a.gpiece      = ...                (AmmoData +0x08)
//!   ...
//! ```
//!
//! In `crates/don-sim/src/systems/ammo.rs` that is between the `a.graph_index` store and
//! the `a.gpiece` store inside `ammo_init`. This module owns the gate; `ammo.rs` belongs to
//! another lane and was not edited.
//!
//! ## The gate, verbatim
//!
//! Transcribed from the disassembly (`esi` is loaded with `100` at `0x0067BE77`; `edi` is
//! the `Ammo*`; `who/o` at `+0x3C/+0x40` is the **shooter**, `whom/ox` at `+0x48/+0x4C` is
//! the **target**, per the `AmmoData` PDB record):
//!
//! ```text
//! 0067be75  call [shooter->vt+0x18]         ; ObjectData::is_unit
//! 0067be77  mov  esi, 0x64                  ; the modulus, 100
//! 0067be7e  je   0x67bef8                   ; not a unit -> straight to the gate
//! 0067be99  call UnitData::order_type       ; 0x00616E80
//! 0067bea1  cmp  eax, 0x17 / je             ; 23 ATTACK_GROUND
//! 0067bec1  cmp  eax, 0x18 / jne 0x67bef8   ; 24 AIR_ATTACK_GROUND
//! 0067bec6  ...  get_order()->vt[0xD0]()    ; ground-attack aim; jmp past the gate (0 draws)
//! 0067bef8  if (ox < 0 || whom < 0) return                 ; Ammo::init ABORTS
//! 0067bf23  if (!(target->flags & 1)) return               ; Ammo::init ABORTS
//! 0067bf32  if (!target->is_unit())        goto done       ; 0 draws
//! 0067bf5a  if (target.type->domain != 2)  goto done       ; 0 draws  (not AIR)
//! 0067bf74  if (target.type->unit_flags & 0x20)  goto done ; 0 draws  ('f' helicopter)
//! 0067bf81  if (target.type->obj_masks & 0x08000000) goto done ; 0 draws ('2' missile)
//! 0067bfb9  call [shooter->vt+0x148](0x80000000)           ; ObjectData::has_objmask('6')
//! 0067bfbd  je   0x67c05b                                  ; -> ARM 2 (not anti-air)
//! ; ---- ARM 1: shooter IS anti-air ('6') ----
//! 0067bfe1  if (shooter.type->domain == 2) goto done       ; 0 draws (air-to-air AA never rolls)
//! 0067c007  call UnitData::is_flying_low   on the TARGET
//! 0067c01d  r = Random::get(0,0xFFFF) % 100                ; ONE draw
//! 0067c043  if (r >= shooter.type->fly_low  ) flags |= 0x10   ; target low
//! 0067c15e  if (r >= shooter.type->fly_high ) flags |= 0x10   ; target high
//! ; ---- ARM 2: shooter is NOT anti-air ----
//! 0067c074  call UnitData::is_flying_low   on the TARGET
//! 0067c08a  r1 = Random::get(0,0xFFFF) % 100               ; FIRST draw
//! 0067c0ae  if (r1 >= TARGET.type->fly_low/high) { flags |= 0x10; goto done }  ; SHORT-CIRCUIT
//! 0067c0c7  r2 = Random::get(0,0xFFFF) % 100               ; SECOND draw
//! 0067c0f2  if (r2 >= SHOOTER.type->fly_low/high) flags |= 0x10
//! ```
//!
//! `0x10` is `FLAG_NO_DAMAGE` in `AmmoData::flags` (`+0x04`) — the same bit
//! `systems::ammo` already honours in `ammo_do_damage_single`. A dud projectile still
//! flies, still animates and still hashes into the `ammo` checksum channel; it just does
//! no damage. **The dud decision is taken at spawn, not at impact.**
//!
//! ### The short-circuit is the load-bearing detail
//!
//! Arm 2 draws **once** when the first roll fails and **twice** when it passes. A port that
//! always draws two, or always draws one, desynchronises after the first miss. That is why
//! [`AntiAirGate::draws`](crate::systems::air::AntiAirGate::draws) is returned and asserted
//! in the tests rather than inferred.
//!
//! ### Whose percentage is whose
//!
//! `ron-data/unitrules.xml` documents `FLY_HIGH`/`FLY_LOW` as
//! *"For anti-air unit to hit air unit it must roll within it's own FLY_HIGH percentage.
//! For non-anti-air unit must ALSO roll within it's -own- FLY_HIGH percentage (two separate
//! % checks conducted; if either fails shot misses)"*. The **code disagrees with the
//! comment about the first roll**: at `0x0067C0AE` / `0x0067C11E` the first check is
//! against the **target's** `fly_low`/`fly_high`, read through `whom/ox`, not the
//! shooter's. The second check is against the shooter's. So the target's `FLY_*` is its
//! *evasion*, and the shooter's is its *accuracy vs aircraft*. The binary is ground truth
//! here; the comment is the developers' prose and is off by one operand.
//!
//! That reading is what makes the shipped numbers coherent: every fighter has
//! `FLY_HIGH = 0`, which would make fighters unable to hit anything if it were the
//! shooter's term — but fighters carry `'6'` (AntiAir) *and* `DOMAIN = Air`, so arm 1's
//! `shooter.domain == AIR` exit means a fighter never rolls at all and always hits.
//! `FLY_HIGH = 0` is the fighter's own *evasion* while cruising: nothing that is not
//! anti-air can touch it.
//!
//! ## The mask alphabets
//!
//! `ObjectTypeData::obj_masks` (`+0x1E4`) is a 32-bit field whose bits are the letters
//! `A..Z` then `1..6` — exactly 32 — documented in the comment header of both
//! `ron-data/unitrules.xml` (`OBJ_MASK`) and `ron-data/buildingrules.xml` (`OBJ_MASKS`).
//! The mapping `letter -> bit index` is `[measured]` at three independent points and
//! cross-checked against the shipped data:
//!
//! | letter | bit | mask | evidence |
//! |---|--:|---|---|
//! | `J` Holds Air | 9 | `0x0000_0200` | `Build::train` `0x0062F9B0` branches on `obj_masks & 0x200` into the aircraft-launch path; the only two buildings with `J` in `buildingrules.xml` are **Airbase** (`GJ`) and **Missile Silo** (`GJ`), plus the **Aircraft Carrier** unit (`6NLAJ`) |
//! | `2` Missile | 27 | `0x0800_0000` | `is_flying_low`/`is_flying_high`/the gate all exempt `obj_masks & 0x8000000`; the exact set of units carrying `2` in `unitrules.xml` is V2 Rocket / Cruise Missile / Nuclear Missile / Nuclear ICBM |
//! | `6` AntiAir | 31 | `0x8000_0000` | the gate calls `ObjectData::has_objmask(0x80000000)`; the units carrying `6` are the three AA guns, every fighter, Destroyer/Cruiser/Missile Cruiser/Patrol Boat, the Carrier, and the five AA buildings (`Z6`) |
//!
//! `UnitTypeData::unit_flags` (`+0x2B4`) is the lower-case `a..z` + `1` alphabet from the
//! `FLAGS` column, same scheme from bit 0. `f` (bit 5, `0x20`) = *"Unit flies like a
//! helicopter"* and `w` (bit 22, `0x400000`) = *"Unit strafes targets"* are both
//! `[measured]` in code (`0x0067BF74` and `Unit::do_air_attack_ground` `0x005EA47F`).
//!
//! ## Fidelity
//!
//! **Tier C throughout.** Transcribed from the instruction stream and self-tested here.
//! Nothing in this module has been through `crates/oracle` — no retail execution has
//! confirmed a single value. See §"Honest gaps" in `docs/mechanics/air.md`.

use crate::rng::Random;

// ============================================================================
// 1. Mask alphabets
// ============================================================================

/// The `obj_masks` alphabet: bit *i* is `OBJ_MASK_ALPHABET[i]`.
///
/// From the comment header of `ron-data/unitrules.xml` — *"Mask values for flags that can
/// be common to both units and buildings (capital A-Z and 1-6)"* — which is exactly 32
/// symbols for a 32-bit field. Bit assignment `[measured]`, see the module header.
pub const OBJ_MASK_ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ123456";

/// The `unit_flags` alphabet: bit *i* is `UNIT_FLAG_ALPHABET[i]`.
///
/// `ron-data/unitrules.xml`: *"FLAGS = Mask values for flags that can be common to units
/// only (lower case a-z)"*, plus a documented `1` for the respawning-hero flag — 27 bits.
pub const UNIT_FLAG_ALPHABET: &[u8; 27] = b"abcdefghijklmnopqrstuvwxyz1";

/// `letter -> obj_masks` bit. Returns 0 for a symbol outside the alphabet.
pub const fn obj_mask_bit(letter: u8) -> u32 {
    let mut i = 0;
    while i < 32 {
        if OBJ_MASK_ALPHABET[i] == letter {
            return 1u32 << i;
        }
        i += 1;
    }
    0
}

/// `letter -> unit_flags` bit. Returns 0 for a symbol outside the alphabet.
pub const fn unit_flag_bit(letter: u8) -> u32 {
    let mut i = 0;
    while i < 27 {
        if UNIT_FLAG_ALPHABET[i] == letter {
            return 1u32 << i;
        }
        i += 1;
    }
    0
}

/// Parse an `OBJ_MASK` / `OBJ_MASKS` cell straight out of `unitrules.xml` /
/// `buildingrules.xml`. Unknown symbols contribute nothing.
pub fn obj_mask_from_str(s: &str) -> u32 {
    s.bytes().fold(0u32, |m, c| m | obj_mask_bit(c))
}

/// Parse a `FLAGS` cell straight out of `unitrules.xml`.
pub fn unit_flags_from_str(s: &str) -> u32 {
    s.bytes().fold(0u32, |m, c| m | unit_flag_bit(c))
}

/// `'3'` — the *Air* "vehicle class" tag. **Not** the same thing as `DOMAIN = Air`: the
/// three bird types are `DOMAIN = Air` with no `3`. Use [`AirTypeData::is_air_domain`] for
/// anything simulation-facing.
pub const OBJ_AIR_CLASS: u32 = obj_mask_bit(b'3');
/// `'J'` — *Holds Air*. Airbase, Missile Silo, Aircraft Carrier.
pub const OBJ_HOLDS_AIR: u32 = obj_mask_bit(b'J');
/// `'2'` — *Missile*. Exempt from every anti-air roll and from the low/high flight bands.
pub const OBJ_MISSILE: u32 = obj_mask_bit(b'2');
/// `'6'` — *AntiAir*. Selects arm 1 of [`antiair_dud_gate`].
pub const OBJ_ANTI_AIR: u32 = obj_mask_bit(b'6');
/// `'Z'` — *Detect*. Carried by helicopters and every anti-air building.
pub const OBJ_DETECT: u32 = obj_mask_bit(b'Z');
/// `'L'` — *Large*.
pub const OBJ_LARGE: u32 = obj_mask_bit(b'L');
/// `'A'` — *Armored*.
pub const OBJ_ARMORED: u32 = obj_mask_bit(b'A');

/// `'f'` — *"Unit flies like a helicopter"*. Tested as `unit_flags & 0x20` at
/// `0x0067BF74`, `0x0060A15C`, `0x0060A322` and in `Build::train`.
pub const UF_HELICOPTER: u32 = unit_flag_bit(b'f');
/// `'w'` — *"Unit strafes targets"*. `Unit::do_air_attack_ground` `0x005EA47F` tests
/// `unit_flags & 0x400000`.
pub const UF_STRAFES: u32 = unit_flag_bit(b'w');
/// `'v'` — *"Unit can fire while moving"*. `UnitData::has_repeat_air` (`0x0046CEC0`) is
/// `unit_masks & 0x200000` on the **instance** copy of the flags.
pub const UF_FIRE_WHILE_MOVING: u32 = unit_flag_bit(b'v');

// ============================================================================
// 2. Domain
// ============================================================================

/// `ObjectTypeData::domain` (`+0x218`) — `Land`, `Sea`, `Air` in `unitrules.xml`.
///
/// `AIR == 2` is `[measured]` at a dozen sites (`0x0067BF5A`, `0x0060A14C`, `0x00645360`,
/// `Unit::process`, …). `LAND == 0` / `SEA == 1` follow the documented declaration order
/// and are `[inference]` — nothing in this module branches on them.
pub const DOMAIN_LAND: i32 = 0;
/// See [`DOMAIN_LAND`].
pub const DOMAIN_SEA: i32 = 1;
/// See [`DOMAIN_LAND`]. This one is measured.
pub const DOMAIN_AIR: i32 = 2;

// ============================================================================
// 3. The static rule record
// ============================================================================

/// The air-relevant slice of `ObjectTypeData` / `UnitTypeData`, at the engine's own field
/// offsets. Only the fields this lane reads are present; the offsets are asserted in
/// the `type_offsets_match_the_pdb` test so a layout drift is caught rather than assumed.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct AirTypeData {
    /// `ObjectTypeData::obj_masks` `+0x1E4`.
    pub obj_masks: u32,
    /// `ObjectTypeData::domain` `+0x218`.
    pub domain: i32,
    /// `ObjectTypeData::los` `+0x21C`, in `TCoord`s (tiles).
    pub los: i32,
    /// `ObjectTypeData::fly_high` `+0x250` — percent chance-to-hit vs a high-flying
    /// aircraft when read off the shooter, percent chance-to-be-hit when read off the
    /// target. `_wtoi`-style leading integer: the `%` in the XML is decoration.
    pub fly_high: i32,
    /// `ObjectTypeData::fly_low` `+0x254`.
    pub fly_low: i32,
    /// `UnitTypeData::unit_flags` `+0x2B4`.
    pub unit_flags: u32,
    /// `UnitTypeData::mana` `+0x2EC`. For an aircraft this is the **fuel budget in
    /// frames**: `Unit::process` burns 1 per airborne frame against this cap.
    pub mana: i32,
}

/// Field offsets, for the layout assertion. From `schema/types.json` (PDB TPI).
pub mod type_offsets {
    /// `ObjectTypeData::obj_masks`.
    pub const OBJ_MASKS: usize = 0x1E4;
    /// `ObjectTypeData::domain`.
    pub const DOMAIN: usize = 0x218;
    /// `ObjectTypeData::los`.
    pub const LOS: usize = 0x21C;
    /// `ObjectTypeData::fly_high`.
    pub const FLY_HIGH: usize = 0x250;
    /// `ObjectTypeData::fly_low`.
    pub const FLY_LOW: usize = 0x254;
    /// `UnitTypeData::unit_flags`.
    pub const UNIT_FLAGS: usize = 0x2B4;
    /// `UnitTypeData::mana`.
    pub const MANA: usize = 0x2EC;
}

impl AirTypeData {
    /// `type->domain == DOMAIN_AIR`. Includes birds, helicopters and missiles.
    #[inline]
    pub const fn is_air_domain(&self) -> bool {
        self.domain == DOMAIN_AIR
    }

    /// `ObjectData::has_objmask(0x80000000)` — the `'6'` AntiAir class.
    #[inline]
    pub const fn is_anti_air(&self) -> bool {
        self.obj_masks & OBJ_ANTI_AIR != 0
    }

    /// `'J'` Holds Air — the object can garrison aircraft.
    #[inline]
    pub const fn holds_air(&self) -> bool {
        self.obj_masks & OBJ_HOLDS_AIR != 0
    }

    /// `'2'` Missile.
    #[inline]
    pub const fn is_missile(&self) -> bool {
        self.obj_masks & OBJ_MISSILE != 0
    }

    /// `'f'` — flies like a helicopter.
    #[inline]
    pub const fn is_helicopter(&self) -> bool {
        self.unit_flags & UF_HELICOPTER != 0
    }

    /// `'w'` — strafes its targets rather than stopping to fire.
    #[inline]
    pub const fn strafes(&self) -> bool {
        self.unit_flags & UF_STRAFES != 0
    }

    /// `UnitData::is_plane` `0x0046CE40`, verbatim:
    /// `domain == 2 && !(unit_flags & 0x20)`. Note it does **not** exclude missiles — a
    /// V2 Rocket *is* a plane by this predicate.
    #[inline]
    pub const fn is_plane(&self) -> bool {
        self.is_air_domain() && !self.is_helicopter()
    }

    /// The common head of `UnitData::is_flying_low` (`0x0060A140`) and
    /// `UnitData::is_flying_high` (`0x0060A310`): AIR domain, not a helicopter, not a
    /// missile. Anything failing this has no flight band at all and is never rolled
    /// against by the anti-air gate.
    #[inline]
    pub const fn has_flight_band(&self) -> bool {
        self.is_air_domain() && !self.is_helicopter() && !self.is_missile()
    }
}

// ============================================================================
// 4. Flight band — high, low, grounded
// ============================================================================

/// `UnitData::is_on_map` `0x0046CE30`: `(u16)inside_up >> 15`, i.e. **bit 15 of
/// `UnitData::inside_up` (`+0x82`, `short`)**. Set means the unit is out on the map; clear
/// means it is inside a container (garrisoned, or parked in an airbase/carrier).
#[inline]
pub const fn is_on_map(inside_up: i16) -> bool {
    (inside_up as u16) >> 15 != 0
}

/// `0x900` = 2304 fine units = **12 tiles**. `UnitData::is_flying_low` returns 1 only when
/// the unit is within this of the thing it is engaging (`0x0060A26A`, `0x0060A2CC`).
pub const FLY_LOW_ENGAGE_RANGE: i32 = 0x900;

/// Where an object sits in the two-band air model the anti-air roll indexes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FlightBand {
    /// Not an aircraft with a flight band: ground/naval, a helicopter, or a missile.
    None,
    /// AIR domain and banded, but inside an airbase/carrier — `is_on_map()` false.
    Parked,
    /// `is_flying_low() == 1` — engaging, within [`FLY_LOW_ENGAGE_RANGE`] of the target.
    Low,
    /// On the map and not low — cruising. `is_flying_high()` is exactly this.
    High,
}

/// The state `UnitData::is_flying_low` inspects beyond the type.
///
/// The engine reaches all of this through the unit's current order; we take it as data so
/// the predicate is testable without an order-list port. The *shape* is `[measured]` from
/// `0x0060A140`; the accessors it uses (`vt+0x100` on a `StrafeOrder`, `vt+0xD4` on an
/// `AirAttackGroundOrder`, `vt+0xFC` for the fallback target) are **UNDERIVED** as
/// functions — only their role is recorded.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct FlightContext {
    /// `UnitData::inside_up` `+0x82`.
    pub inside_up: i16,
    /// `UnitData::order_type()` — see [`ORD_STRAFE`] etc.
    pub order: i32,
    /// The strafe/attack-ground order names a live target object.
    pub order_target_active: bool,
    /// `vector_dist` from the unit to that target, in fine units.
    pub order_target_dist: i32,
    /// The fallback `vt+0xFC` target (the order's *destination* target) is live.
    pub fallback_target_active: bool,
    /// `vector_dist` to the fallback target.
    pub fallback_target_dist: i32,
}

/// `UnitData::is_flying_low` `0x0060A140` \[measured, structure\], 455 bytes.
///
/// ```text
/// if (!type.has_flight_band())              return 0
/// if (!is_on_map())                         return 0
/// ord = order_type()
/// if (ord == STRAFE(16)) {
///     t = order->vt[0x100]()                       // the strafe target
///     if (!t.active) goto fallback
/// } else if (ord == AIR_ATTACK_GROUND(24)) {
///     order->vt[0xD4]()                            // refresh the aim point
/// } else {
///     if (ord == NONE(0)) return 0                 // idle aircraft are never "low"
///     goto fallback
/// }
/// if (vector_dist(...) < 0x900) return 1
/// fallback:
/// t2 = order->vt[0xFC]()
/// if (t2 && t2.active && vector_dist(...) < 0x900) return 1
/// return 0
/// ```
///
/// `AIR_PATROL` is deliberately **not** an arm: a patrolling aircraft falls to the
/// `goto fallback` branch and is only "low" if its order still names a live target.
pub fn is_flying_low(t: &AirTypeData, c: &FlightContext) -> bool {
    if !t.has_flight_band() {
        return false;
    }
    if !is_on_map(c.inside_up) {
        return false;
    }
    let direct = match c.order {
        ORD_STRAFE => c.order_target_active,
        ORD_AIR_ATTACK_GROUND => true,
        ORD_NONE => return false,
        _ => false,
    };
    if direct && c.order_target_dist < FLY_LOW_ENGAGE_RANGE {
        return true;
    }
    c.fallback_target_active && c.fallback_target_dist < FLY_LOW_ENGAGE_RANGE
}

/// `UnitData::is_flying_high` `0x0060A310` \[measured\]: banded, on the map, and
/// `!is_flying_low()`.
pub fn is_flying_high(t: &AirTypeData, c: &FlightContext) -> bool {
    t.has_flight_band() && is_on_map(c.inside_up) && !is_flying_low(t, c)
}

/// The band, as one value. `Parked` and `None` are both "the anti-air roll does not
/// apply", but they are distinguished because the airbase accounting cares.
pub fn flight_band(t: &AirTypeData, c: &FlightContext) -> FlightBand {
    if !t.has_flight_band() {
        return FlightBand::None;
    }
    if !is_on_map(c.inside_up) {
        return FlightBand::Parked;
    }
    if is_flying_low(t, c) {
        FlightBand::Low
    } else {
        FlightBand::High
    }
}

// ============================================================================
// 5. THE ANTI-AIR DUD GATE  —  Ammo::init 0x0067BE49..0x0067C16A
// ============================================================================

/// `AmmoData::flags` bit set by the gate when the roll fails. Mirrors
/// `systems::ammo::FLAG_NO_DAMAGE`; duplicated as a `u8` here so this module has no
/// dependency on that lane's file.
pub const FLAG_NO_DAMAGE: u8 = 0x10;

/// The modulus at `0x0067BE77` (`mov esi, 0x64`) and `0x0067C0CD` (`mov ecx, 0x64`).
pub const AA_PERCENT_MODULUS: i32 = 100;

/// Which arm of the gate ran. Reported so a caller can assert on stream position without
/// re-deriving the branch conditions.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AntiAirArm {
    /// `Ammo::init` returns before the projectile exists — negative `whom`/`ox`, or the
    /// target slot is not `flags & 1`. **No ammo is spawned at all.**
    InitAborted,
    /// The shooter is a unit executing `ATTACK_GROUND` or `AIR_ATTACK_GROUND`: the gate is
    /// jumped over entirely (`0x0067BEF3`). 0 draws.
    GroundAttackOrder,
    /// The target is not a unit, not AIR domain, a helicopter, or a missile. 0 draws.
    TargetNotBanded,
    /// Shooter has `'6'` **and** is itself AIR domain (`0x0067BFE8`). 0 draws — fighters
    /// and other airborne AA never roll and never dud.
    AntiAirFromTheAir,
    /// Shooter has `'6'`: a single roll against the shooter's `fly_low`/`fly_high`.
    AntiAirSingleRoll,
    /// Shooter lacks `'6'`: first roll against the **target's** percentage. It failed, so
    /// the second roll never happened. 1 draw.
    PlainFirstRollFailed,
    /// Shooter lacks `'6'`: both rolls happened. 2 draws.
    PlainBothRolls,
}

/// Result of [`antiair_dud_gate`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AntiAirGate {
    /// `true` if `AmmoData::flags |= 0x10` (`FLAG_NO_DAMAGE`) was executed.
    pub dud: bool,
    /// **The number of `Random::get(0, 0xFFFF)` calls consumed.** 0, 1 or 2. This is the
    /// value the replay harness depends on.
    pub draws: u32,
    /// Which branch ran.
    pub arm: AntiAirArm,
    /// `true` when the whole `Ammo::init` call returns early and no projectile exists.
    pub init_aborted: bool,
}

/// Everything the gate reads, flattened.
///
/// Field names follow the engine: `who/o` identify the shooter, `whom/ox` the target
/// (`AmmoData +0x3C/+0x40` and `+0x48/+0x4C`).
#[derive(Clone, Copy, Debug)]
pub struct AntiAirShot<'a> {
    /// `AmmoData::whom` `+0x48`. Negative aborts `Ammo::init`.
    pub whom: i32,
    /// `AmmoData::ox` `+0x4C`. Negative aborts `Ammo::init`.
    pub ox: i32,
    /// `SubObject::flags & 1` on the target — the slot-active bit. False aborts
    /// `Ammo::init`.
    pub target_active: bool,
    /// `ObjectData::is_unit()` on the target (`vt+0x18`). Buildings and walls exit the
    /// gate with 0 draws.
    pub target_is_unit: bool,
    /// The target's type record.
    pub target_type: &'a AirTypeData,
    /// The result of `UnitData::is_flying_low()` on the target. Only consulted once the
    /// target is known to be banded; compute it with [`is_flying_low`].
    pub target_flying_low: bool,
    /// `ObjectData::is_unit()` on the shooter (`vt+0x18` at `0x0067BE75`).
    pub shooter_is_unit: bool,
    /// `UnitData::order_type()` on the shooter, only read when `shooter_is_unit`.
    pub shooter_order: i32,
    /// The shooter's type record.
    pub shooter_type: &'a AirTypeData,
}

/// **`Ammo::init`'s anti-air dud gate, `0x0067BE49..0x0067C16A`.**
///
/// Draws from `game_random` (`0x00E37A8C`, reached as `GameAccess::game_random`
/// `[0x00C06184]`) — the *main simulation stream*. Call this exactly once per spawned
/// projectile, in the position documented in the module header, and pass the same
/// [`Random`] the rest of the tick uses.
///
/// Returns [`AntiAirGate`], whose `draws` field is the contract: 0, 1 or 2.
pub fn antiair_dud_gate(shot: &AntiAirShot<'_>, rng: &mut Random) -> AntiAirGate {
    // 0x0067BE75: shooter->is_unit(), then 0x0067BE99/0x0067BEC1 order_type.
    // ATTACK_GROUND / AIR_ATTACK_GROUND jump clean over the gate at 0x0067BEF3.
    if shot.shooter_is_unit
        && (shot.shooter_order == ORD_ATTACK_GROUND || shot.shooter_order == ORD_AIR_ATTACK_GROUND)
    {
        return AntiAirGate {
            dud: false,
            draws: 0,
            arm: AntiAirArm::GroundAttackOrder,
            init_aborted: false,
        };
    }

    // 0x0067BEF8..0x0067BF27: three `return` paths out of Ammo::init itself.
    if shot.ox < 0 || shot.whom < 0 || !shot.target_active {
        return AntiAirGate {
            dud: false,
            draws: 0,
            arm: AntiAirArm::InitAborted,
            init_aborted: true,
        };
    }

    // 0x0067BF32..0x0067BF8B: four exits with 0 draws.
    let tt = shot.target_type;
    if !shot.target_is_unit || !tt.has_flight_band() {
        return AntiAirGate {
            dud: false,
            draws: 0,
            arm: AntiAirArm::TargetNotBanded,
            init_aborted: false,
        };
    }

    let st = shot.shooter_type;
    // 0x0067BFB9: shooter->has_objmask('6').
    if st.is_anti_air() {
        // 0x0067BFE1: an airborne anti-air shooter never rolls.
        if st.is_air_domain() {
            return AntiAirGate {
                dud: false,
                draws: 0,
                arm: AntiAirArm::AntiAirFromTheAir,
                init_aborted: false,
            };
        }
        // 0x0067C01D / 0x0067C04E: exactly one draw, against the SHOOTER's percentage.
        let r = aa_roll(rng);
        let pct = if shot.target_flying_low {
            st.fly_low
        } else {
            st.fly_high
        };
        AntiAirGate {
            dud: r >= pct,
            draws: 1,
            arm: AntiAirArm::AntiAirSingleRoll,
            init_aborted: false,
        }
    } else {
        // 0x0067C08A / 0x0067C0FA: first draw, against the TARGET's percentage.
        let r1 = aa_roll(rng);
        let (tpct, spct) = if shot.target_flying_low {
            (tt.fly_low, st.fly_low)
        } else {
            (tt.fly_high, st.fly_high)
        };
        if r1 >= tpct {
            // 0x0067C0B4 / 0x0067C124: `jge 0x67C166` — set the flag and skip draw two.
            return AntiAirGate {
                dud: true,
                draws: 1,
                arm: AntiAirArm::PlainFirstRollFailed,
                init_aborted: false,
            };
        }
        // 0x0067C0C7 / 0x0067C133: second draw, against the SHOOTER's percentage.
        let r2 = aa_roll(rng);
        AntiAirGate {
            dud: r2 >= spct,
            draws: 2,
            arm: AntiAirArm::PlainBothRolls,
            init_aborted: false,
        }
    }
}

/// One anti-air roll: `Random::get(0, 0xFFFF) % 100`, signed `idiv` (`cdq; idiv`). The
/// dividend is always in `[0, 0xFFFE]` so the sign of the remainder never matters, but the
/// half-open range does — `Random::get` cannot return `0xFFFF`.
#[inline]
fn aa_roll(rng: &mut Random) -> i32 {
    rng.get(0, 0xFFFF) % AA_PERCENT_MODULUS
}

/// Convenience wrapper: run the gate and fold the result into an `AmmoData::flags` byte,
/// the way `Ammo::init` does at `0x0067C166` (`or byte [edi+4], 0x10`).
pub fn apply_antiair_gate(flags: &mut u8, shot: &AntiAirShot<'_>, rng: &mut Random) -> AntiAirGate {
    let g = antiair_dud_gate(shot, rng);
    if g.dud {
        *flags |= FLAG_NO_DAMAGE;
    }
    g
}

// ============================================================================
// 6. Order indices and the air orders
// ============================================================================

/// `OrderIndex::NONE`.
pub const ORD_NONE: i32 = 0;
/// `OrderIndex::STRAFE` — the air attack order; `Unit::do_strafe` `0x005EAB00`.
pub const ORD_STRAFE: i32 = 16;
/// `OrderIndex::AIR_PATROL` — `Unit::do_air_patrol` `0x005EA620`.
pub const ORD_AIR_PATROL: i32 = 17;
/// `OrderIndex::ATTACK_GROUND` — ground units; skips the anti-air gate.
pub const ORD_ATTACK_GROUND: i32 = 23;
/// `OrderIndex::AIR_ATTACK_GROUND` — `Unit::do_air_attack_ground` `0x005EA420`.
pub const ORD_AIR_ATTACK_GROUND: i32 = 24;

/// The free function `is_air(OrderIndex)` at `0x0046F000`, verbatim: the order is an air
/// order iff it is `STRAFE`, `AIR_PATROL` or `AIR_ATTACK_GROUND`.
///
/// Note what is **not** in the set: `MOVE_TO`. An aircraft flying from A to B carries an
/// ordinary move order; only the three above are `AirOrder` subclasses.
#[inline]
pub const fn is_air_order(order: i32) -> bool {
    order == ORD_STRAFE || order == ORD_AIR_PATROL || order == ORD_AIR_ATTACK_GROUND
}

/// The `AirOrder` base, at its own field offsets (`schema/types.json`, `sizeof == 40`).
///
/// `AirOrder` is a *secondary* base of `StrafeOrder` (84 B), `AirPatrolOrder` (104 B) and
/// `AirAttackGroundOrder` (72 B), so these offsets are relative to the `AirOrder`
/// subobject, not to the concrete order.
///
/// `AirOrder::walk_data` (`0x0047F2D0`) is a **single flat 24-byte range**
/// (`schema/walkops.json`: `walk(this-28, this-4)`), so every field below is on the wire
/// and inside the checksum.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct AirOrderWalk {
    /// `+0x04 oxx` — the home base's object index.
    pub oxx: i32,
    /// `+0x08 whose` — the home base's owner slot.
    pub whose: i32,
    /// `+0x0C cruising_alt`.
    pub cruising_alt: i32,
    /// `+0x10 sharp_turn`.
    pub sharp_turn: i32,
    /// `+0x14 old`.
    pub old: i32,
    /// `+0x18 returning` — **the sortie/return latch.** Set by
    /// [`check_fuel_latch_returning`] when fuel runs out, and by the recall command.
    pub returning: i32,
}

/// Field offsets of [`AirOrderWalk`] inside the `AirOrder` subobject.
pub mod air_order_offsets {
    /// `AirOrder::oxx`.
    pub const OXX: usize = 0x04;
    /// `AirOrder::whose`.
    pub const WHOSE: usize = 0x08;
    /// `AirOrder::cruising_alt`.
    pub const CRUISING_ALT: usize = 0x0C;
    /// `AirOrder::sharp_turn`.
    pub const SHARP_TURN: usize = 0x10;
    /// `AirOrder::old`.
    pub const OLD: usize = 0x14;
    /// `AirOrder::returning`.
    pub const RETURNING: usize = 0x18;
}

// ============================================================================
// 7. Air patrol — waypoints and the target-scan cadence
// ============================================================================

/// `0x240` = 576 fine units = **3 tiles**. `Unit::do_air_patrol` `0x005EA6C4` advances to
/// the next patrol waypoint once `vector_dist` drops below this.
pub const AIR_PATROL_WAYPOINT_ARRIVE: i32 = 0x240;

/// `Unit::do_air_patrol` scans for an **air/unit** target only when
/// `(object_index + Game::frame) % 16 == 0` [measured, `0x005EA6EC`: the MSVC
/// `& 0x8000000F` signed-modulo idiom].
///
/// The phase offset by object index is what spreads the cost across frames — and it is a
/// hard ordering fact: the scan happens on a *specific* frame for a *specific* unit, so
/// any RNG the scan consumes lands at a stream position that depends on the object index.
#[inline]
pub const fn air_patrol_unit_scan_due(object_index: i32, frame: i32) -> bool {
    (object_index.wrapping_add(frame)) % 16 == 0
}

/// The second, coarser scan in `Unit::do_air_patrol` (`0x005EA84C`), for a **building**
/// target: `(object_index + Game::frame) % 32 == 0`.
#[inline]
pub const fn air_patrol_building_scan_due(object_index: i32, frame: i32) -> bool {
    (object_index.wrapping_add(frame)) % 32 == 0
}

/// A patrol leg step. `Unit::do_air_patrol` advances the waypoint cursor when within
/// [`AIR_PATROL_WAYPOINT_ARRIVE`], and wraps to 0 when the cursor has run past the list
/// (`0x005EA64B`).
///
/// Returns the new cursor and whether the patrol order is retired at its last waypoint.
/// `UnitData +0xD8` is `OrderList::length`, not a loop counter: retail retires the patrol
/// only when another order is queued behind it. With a one-order list the cursor remains
/// on the final waypoint; it does not wrap to zero.
#[inline]
pub fn air_patrol_advance(
    cursor: i32,
    count: i32,
    dist: i32,
    queued_order_count: i32,
) -> (i32, bool) {
    let mut cur = if cursor >= count { 0 } else { cursor };
    if dist < AIR_PATROL_WAYPOINT_ARRIVE {
        if cur < count - 1 {
            cur += 1;
        } else if queued_order_count > 1 {
            return (cur, true);
        }
    }
    (cur, false)
}

// ============================================================================
// 8. Airbase capacity — the 'J' flag and the two constants
// ============================================================================

/// `Constants::max_aircraft_per_carrier`, `Constants + 0xA4`, `rules.xml`
/// `<MAX_AIRCRAFT_PER_CARRIER value="7"/>` \[measured,
/// `docs/derivation/rules-constants.json`\].
pub const MAX_AIRCRAFT_PER_CARRIER: i32 = 7;
/// `Constants::max_aircraft_per_airbase`, `Constants + 0xA8`,
/// `<MAX_AIRCRAFT_PER_AIRBASE value="10"/>`.
pub const MAX_AIRCRAFT_PER_AIRBASE: i32 = 10;
/// `Constants::air_unit_mana_recharge`, `Constants + 0xA0`,
/// `<AIR_UNIT_MANA_RECHARGE value="2 craft per frame"/>`. The XML's units are misleading:
/// this is the **fuel recovered per frame while parked**, not a craft count.
pub const AIR_UNIT_MANA_RECHARGE: i32 = 2;
/// `Constants::bombing_mana_cost`, `Constants + 0xC14`, `<BOMBING_MANA_COST value="0 mana"/>`.
/// Added to `mana_burn` after a bomber releases (`Unit::do_air_attack_ground` `0x005EA5A9`).
/// **Zero in this build**, so bombing costs no extra fuel.
pub const BOMBING_MANA_COST: i32 = 0;
/// `Constants::aircraft_heal_rate`, `Constants + 0xC00`, four entries indexed by the
/// player's heal level: `{20, 16, 12, 8}` frames per hit point while parked.
pub const AIRCRAFT_HEAL_RATE: [i32; 4] = [20, 16, 12, 8];

/// Which kind of object is being asked for its aircraft capacity. The engine asks with
/// `ObjectData::is(TypeIndex, strict)` against three specific type ids; `strict` is 1 in
/// all three calls, so upgrade lines do **not** inherit capacity.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AirHost {
    /// `is(351, 1)` — `AIRCRAFTCARRIER` (`schema/live/live-tables-typeids.tsv`).
    Carrier,
    /// `is(447, 1)` — `AIRBASE`.
    Airbase,
    /// `is(520, 1)` — `NUCLEARSILO`, the Missile Silo. Capacity 1.
    MissileSilo,
    /// Anything else. Capacity 0.
    Other,
}

/// Type ids used by `ObjectData::num_aircraft_limit`, from the live type table.
pub mod type_ids {
    /// `AIRCRAFTCARRIER`.
    pub const AIRCRAFT_CARRIER: i32 = 351;
    /// `AIRBASE`.
    pub const AIRBASE: i32 = 447;
    /// `NUCLEARSILO` — the Missile Silo.
    pub const NUCLEAR_SILO: i32 = 520;
    /// `HELICOPTER`.
    pub const HELICOPTER: i32 = 310;
    /// `BOMBER`.
    pub const BOMBER: i32 = 304;
    /// `JETFIGHTER`.
    pub const JET_FIGHTER: i32 = 295;
    /// `SPACEPROGRAM` — gates the `space_air_range` fuel bonus.
    pub const SPACE_PROGRAM: i32 = 542;
}

/// Classify by strict type id, the way `num_aircraft_limit` does.
pub const fn air_host_of(type_id: i32) -> AirHost {
    if type_id == type_ids::AIRCRAFT_CARRIER {
        AirHost::Carrier
    } else if type_id == type_ids::AIRBASE {
        AirHost::Airbase
    } else if type_id == type_ids::NUCLEAR_SILO {
        AirHost::MissileSilo
    } else {
        AirHost::Other
    }
}

/// `ObjectData::num_aircraft_limit` `0x006454A0`, verbatim \[measured\]:
///
/// ```text
/// if (is(AIRCRAFTCARRIER, 1)) return constants.max_aircraft_per_carrier   // 7
/// if (is(AIRBASE,         1)) return constants.max_aircraft_per_airbase   // 10
/// return is(NUCLEARSILO,  1) ? 1 : 0
/// ```
///
/// Note the tail: the return is the *boolean*, so a Missile Silo holds exactly one and
/// everything else holds none — the constants are not consulted for either.
pub const fn num_aircraft_limit(host: AirHost) -> i32 {
    match host {
        AirHost::Carrier => MAX_AIRCRAFT_PER_CARRIER,
        AirHost::Airbase => MAX_AIRCRAFT_PER_AIRBASE,
        AirHost::MissileSilo => 1,
        AirHost::Other => 0,
    }
}

/// One entry in the owner's object list, as `num_aircraft_here` reads it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HostedAircraft {
    /// `SubObject::flags & 1`.
    pub active: bool,
    /// `type->domain`.
    pub domain: i32,
    /// `UnitData::home_base(&owner)` — the object index it calls home, or `-1`.
    pub home_base_o: i32,
    /// The owner slot returned through `home_base`'s out-parameter.
    pub home_base_who: i32,
}

/// `ObjectData::num_aircraft_here(int also_count_queue)` `0x00645330`, the part that is
/// fully derived \[measured\].
///
/// The engine counts every **active, AIR-domain** object of this owner whose
/// `UnitData::home_base` resolves to *this* object, then:
///
/// * if `is(AIRCRAFTCARRIER, 0)` it adds `UnitData::num_queued` (`+0xA0`, `short`) — the
///   carrier's own build queue;
/// * otherwise, if `also_count_queue` and `vt+0x20` (a build-capable predicate), it adds a
///   leader-level difference of two `LeaderData` counters. **That arm is UNDERIVED** and is
///   passed through as `leader_queue_adjust` rather than guessed.
pub fn num_aircraft_here(
    this_o: i32,
    this_who: i32,
    host: AirHost,
    objects: &[HostedAircraft],
    num_queued: i32,
    also_count_queue: bool,
    leader_queue_adjust: i32,
) -> i32 {
    let mut n = 0;
    for e in objects {
        if e.active
            && e.domain == DOMAIN_AIR
            && e.home_base_o == this_o
            && e.home_base_who == this_who
        {
            n += 1;
        }
    }
    if matches!(host, AirHost::Carrier) {
        return n + num_queued;
    }
    if also_count_queue {
        return n + leader_queue_adjust;
    }
    n
}

/// `Build::train` `0x0062F9B0` gates the aircraft-launch path on
/// `type->obj_masks & 0x200` — the `'J'` Holds Air bit — and, inside it, refuses to let a
/// plane come out once `num_aircraft_here(0) > num_aircraft_limit()` [measured,
/// `0x0062FC07` region]. This is the whole airbase-capacity rule.
///
/// Returns `true` if the newly trained aircraft may leave the building.
#[inline]
pub fn airbase_may_launch(host_type: &AirTypeData, here: i32, limit: i32) -> bool {
    host_type.holds_air() && here <= limit
}

// ============================================================================
// 9. Fuel — `mana` on an aircraft is a sortie clock
// ============================================================================

/// `UnitData::mana` `0x00609A50` \[measured\] — the aircraft's **fuel cap**, in frames.
///
/// ```text
/// base = type->mana                     // UnitTypeData +0x2EC, the MANA column
/// if (base == 0) return 0               // helicopters and birds: no fuel model at all
/// if (domain == AIR) {
///     if (!leader_has(SPACEPROGRAM))  return base
///     bonus = constantsc->space_air_range     // Constants +0x55C, "0% bonus" in this build
/// } else { ...spellcaster path, not this lane... }
/// return ((bonus + 100) * base) / 100
/// ```
///
/// The non-air arm (spellcasters, the French `french_special_craft` bonus at
/// `Constants + 0x69C`) belongs to another lane and is not modelled here; this function
/// covers the AIR arm only and returns `base` for anything else.
pub fn mana_cap(t: &AirTypeData, has_space_program: bool, space_air_range_pct: i32) -> i32 {
    let base = t.mana;
    if base == 0 {
        return 0;
    }
    if !t.is_air_domain() {
        return base;
    }
    if !has_space_program {
        return base;
    }
    ((space_air_range_pct + 100) * base) / 100
}

/// `UnitData::mana_left` `0x00609A30`: `max(0, mana() - mana_burn)`.
#[inline]
pub const fn mana_left(cap: i32, mana_burn: i16) -> i32 {
    let left = cap - mana_burn as i32;
    if left < 0 {
        0
    } else {
        left
    }
}

/// The per-frame fuel step inside `Unit::process` `0x00610BC0` \[measured,
/// `0x00611090`ff\].
///
/// ```text
/// if (!(unit_masks & 1) && !(type->unit_flags2 & 2) && domain == AIR) {
///     if (is_on_map()) {                                   // airborne
///         if (mana() - mana_burn > 0) mana_burn += 1       // burn one frame of fuel
///     } else {                                             // parked
///         if (air_unit_mana_recharge < mana_burn) mana_burn -= air_unit_mana_recharge
///         else                                             mana_burn  = 0
///         ...then the aircraft_heal_rate block...
///     }
/// }
/// ```
///
/// So an aircraft **burns 1 fuel per airborne frame and recovers 2 per parked frame** — a
/// 2:1 turnaround. A Jet Fighter's `MANA = 500` is 500 frames aloft ≈ 33 game seconds
/// (15 frames = 1 game second), refuelled in 250 frames.
///
/// Returns the new `mana_burn`.
#[inline]
pub fn air_fuel_step(mana_burn: i16, cap: i32, on_map: bool) -> i16 {
    if on_map {
        if cap - mana_burn as i32 > 0 {
            mana_burn + 1
        } else {
            mana_burn
        }
    } else if AIR_UNIT_MANA_RECHARGE < mana_burn as i32 {
        mana_burn - AIR_UNIT_MANA_RECHARGE as i16
    } else {
        0
    }
}

/// `Unit::check_fuel` `0x005E9BE0`'s latch \[measured,
/// `0x005E9C1A`..`0x005E9C3E`\]:
///
/// ```text
/// if (!air->returning && mana_left() == 0 && !(type->obj_masks & '2')) air->returning = 1
/// ```
///
/// Missiles are exempt — they are one-way and never return. Returns the new `returning`.
#[inline]
pub fn check_fuel_latch_returning(t: &AirTypeData, returning: i32, left: i32) -> i32 {
    if returning == 0 && left == 0 && !t.is_missile() {
        1
    } else {
        returning
    }
}

/// What `Unit::check_fuel` decides once `returning` is latched and it has looked for a
/// base \[measured, `0x005E9D8D`..`0x005E9DC6`\].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FuelVerdict {
    /// `returning` is still 0, or the type has no fuel model: `check_fuel` returns 0 and
    /// the order proceeds untouched.
    KeepFlying,
    /// A host was found; the order's `oxx`/`whose` are repointed at it and the unit is
    /// steered to `base.x - 0xC0`.
    ReturnTo { o: i32, who: i32 },
    /// No host with capacity anywhere and the unit is **not** a helicopter: the unit is
    /// killed with reason 2 (`vt+0x158`) — the aircraft crashes. The local player also
    /// gets a message.
    Crash,
    /// No host, but the unit *is* a helicopter (`unit_flags & 0x20`): it simply lands
    /// where it is. `check_fuel` returns 0 with the unit's own coords.
    LandInPlace,
}

/// `Unit::check_fuel` `0x005E9BE0`, at the level this lane derived it.
///
/// The host search itself (two linear scans of the owner's object list — buildings from
/// index 2000, then units from 0 — keeping the nearest by `vector_dist` that passes
/// `ObjectData::can_carry`) is reproduced by the caller and handed in as
/// `nearest_host`; that keeps the world-access shape out of this module. The *decision
/// table* below is the derived part.
pub fn check_fuel(
    t: &AirTypeData,
    returning: i32,
    left: i32,
    current_host_ok: bool,
    current_host: Option<(i32, i32)>,
    nearest_host: Option<(i32, i32)>,
) -> (i32, FuelVerdict) {
    if t.mana == 0 && returning == 0 {
        return (returning, FuelVerdict::KeepFlying);
    }
    let returning = check_fuel_latch_returning(t, returning, left);
    if returning == 0 {
        return (returning, FuelVerdict::KeepFlying);
    }
    if current_host_ok {
        if let Some((o, who)) = current_host {
            return (returning, FuelVerdict::ReturnTo { o, who });
        }
    }
    match nearest_host {
        Some((o, who)) => (returning, FuelVerdict::ReturnTo { o, who }),
        None if t.is_helicopter() => (returning, FuelVerdict::LandInPlace),
        None => (returning, FuelVerdict::Crash),
    }
}

/// `Unit::do_air_attack_ground` `0x005EA420` adds [`BOMBING_MANA_COST`] to `mana_burn`
/// after the release, but only when the unit `is(BOMBER, 0)` \[measured, `0x005EA5A9`\].
/// Zero in this build, so this is a no-op that exists to be correct if a mod changes it.
#[inline]
pub fn bombing_fuel_cost(is_bomber: bool) -> i32 {
    if is_bomber {
        BOMBING_MANA_COST
    } else {
        0
    }
}

/// `Unit::do_air_attack_ground` `0x005EA5DE`: a **missile** (`'2'`) destroys itself
/// immediately after releasing, via `vt+0x158` (`Object::kill`) with reason 0. Everything
/// else sets its recharge counter instead.
#[inline]
pub const fn air_attack_ground_self_destructs(t: &AirTypeData) -> bool {
    t.is_missile()
}

/// The facing tolerance in `Unit::do_air_attack_ground` [measured, `0x005EA4B2` /
/// `0x005EA4E5`]. Angles are full-circle binary: `2^32` per revolution.
///
/// A non-missile whose heading error exceeds `0x0AAA_AAA9` (= 2^32/24, **15°**) must be a
/// `JETFIGHTER` to keep going, and even then aborts past `0x2AAA_AAAA` (2^32/6, **60°**).
pub const AIR_BOMB_ANGLE_TOLERANCE: u32 = 0x0AAA_AAA9;
/// The hard cut-off. See [`AIR_BOMB_ANGLE_TOLERANCE`].
pub const AIR_BOMB_ANGLE_MAX: u32 = 0x2AAA_AAAA;

/// `Unit::do_air_attack_ground`'s release gate on heading error.
#[inline]
pub const fn air_bomb_release_allowed(
    t: &AirTypeData,
    angle_err: u32,
    is_jet_fighter: bool,
) -> bool {
    if t.is_missile() {
        return true;
    }
    if angle_err <= AIR_BOMB_ANGLE_TOLERANCE {
        return true;
    }
    is_jet_fighter && angle_err <= AIR_BOMB_ANGLE_MAX
}

// ============================================================================
// 10. Detection
// ============================================================================

/// `'Z'` Detect is the only air-relevant detection flag this lane derived: it is carried
/// by both helicopter types (`U3TGZ`) and by every anti-air building (`Z6`), and by no
/// fixed-wing aircraft.
///
/// Fog is otherwise recomputed for everything at once — `GameDaemon::update_all_seen`
/// (which opens with `World::clear_seen`) runs once per tick, *before* `Objects::process_all`
/// — so an aircraft's contribution is its `ObjectTypeData::los` (`+0x21C`, in tiles) like
/// any other object's. **Nothing in the fog path special-cases altitude**: this lane found
/// no `domain == 2` test anywhere in `world.cpp`'s reveal functions. That is a negative
/// result from a targeted search, not an exhaustive proof; see the gaps section of
/// `docs/mechanics/air.md`.
#[inline]
pub const fn is_detector(t: &AirTypeData) -> bool {
    t.obj_masks & OBJ_DETECT != 0
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ---- shipped-data fixtures, copied verbatim from ron-data/*.xml ----

    /// `unitrules.xml`: Jet Fighter — `OBJ_MASK X63T`, `FLAGS j`, `FLY_HIGH 0`,
    /// `FLY_LOW 10%`, `MANA 500`, `DOMAIN Air`, `LOS 16`.
    fn jet_fighter() -> AirTypeData {
        AirTypeData {
            obj_masks: obj_mask_from_str("X63T"),
            domain: DOMAIN_AIR,
            los: 16,
            fly_high: 0,
            fly_low: 10,
            unit_flags: unit_flags_from_str("j"),
            mana: 500,
        }
    }

    /// `unitrules.xml`: Bomber — `BSX3T`, `FLAGS h`, `FH 0`, `FL 10%`, `MANA 600`.
    fn bomber() -> AirTypeData {
        AirTypeData {
            obj_masks: obj_mask_from_str("BSX3T"),
            domain: DOMAIN_AIR,
            los: 16,
            fly_high: 0,
            fly_low: 10,
            unit_flags: unit_flags_from_str("h"),
            mana: 600,
        }
    }

    /// `unitrules.xml`: Anti-Aircraft Gun — `6VT`, `FLAGS hlmjcv`, `FH 50%`, `FL 90%`,
    /// `DOMAIN Land`, `MANA 0`.
    fn aa_gun() -> AirTypeData {
        AirTypeData {
            obj_masks: obj_mask_from_str("6VT"),
            domain: DOMAIN_LAND,
            los: 14,
            fly_high: 50,
            fly_low: 90,
            unit_flags: unit_flags_from_str("hlmjcv"),
            mana: 0,
        }
    }

    /// `unitrules.xml`: Musketeer-class ground unit stand-in — no `'6'`, `FH`/`FL` 0.
    /// (Every non-anti-air land unit in the shipped file has `FLY_HIGH 0 / FLY_LOW 0`.)
    fn plain_ground() -> AirTypeData {
        AirTypeData {
            obj_masks: obj_mask_from_str("FGD"),
            domain: DOMAIN_LAND,
            los: 10,
            fly_high: 0,
            fly_low: 0,
            unit_flags: unit_flags_from_str("i"),
            mana: 0,
        }
    }

    /// `unitrules.xml`: Helicopter — `U3TGZ`, `FLAGS fh`, `FH 10%`, `FL 20%`, `MANA 0`.
    fn helicopter() -> AirTypeData {
        AirTypeData {
            obj_masks: obj_mask_from_str("U3TGZ"),
            domain: DOMAIN_AIR,
            los: 12,
            fly_high: 10,
            fly_low: 20,
            unit_flags: unit_flags_from_str("fh"),
            mana: 0,
        }
    }

    /// `unitrules.xml`: Cruise Missile — `SBX32`, `FLAGS j`, `MANA 400`.
    fn cruise_missile() -> AirTypeData {
        AirTypeData {
            obj_masks: obj_mask_from_str("SBX32"),
            domain: DOMAIN_AIR,
            los: 0,
            fly_high: 0,
            fly_low: 0,
            unit_flags: unit_flags_from_str("j"),
            mana: 400,
        }
    }

    /// `buildingrules.xml`: Airbase — `OBJ_MASKS GJ`, `LOS 12`.
    fn airbase() -> AirTypeData {
        AirTypeData {
            obj_masks: obj_mask_from_str("GJ"),
            domain: DOMAIN_LAND,
            los: 12,
            fly_high: 0,
            fly_low: 0,
            unit_flags: 0,
            mana: 0,
        }
    }

    /// `buildingrules.xml`: SAM Installation — `OBJ_MASKS Z6`, `FH 33`, `FL 75`.
    fn sam_installation() -> AirTypeData {
        AirTypeData {
            obj_masks: obj_mask_from_str("Z6"),
            domain: DOMAIN_LAND,
            los: 9,
            fly_high: 33,
            fly_low: 75,
            unit_flags: 0,
            mana: 0,
        }
    }

    fn shot<'a>(shooter: &'a AirTypeData, target: &'a AirTypeData, low: bool) -> AntiAirShot<'a> {
        AntiAirShot {
            whom: 0,
            ox: 7,
            target_active: true,
            target_is_unit: true,
            target_type: target,
            target_flying_low: low,
            shooter_is_unit: true,
            shooter_order: ORD_STRAFE,
            shooter_type: shooter,
        }
    }

    // ---- masks ----

    #[test]
    fn mask_alphabets_are_the_documented_ones() {
        assert_eq!(OBJ_MASK_ALPHABET.len(), 32, "obj_masks is a 32-bit field");
        assert_eq!(UNIT_FLAG_ALPHABET.len(), 27);
        // A..Z then 1..6.
        assert_eq!(obj_mask_bit(b'A'), 1 << 0);
        assert_eq!(obj_mask_bit(b'Z'), 1 << 25);
        assert_eq!(obj_mask_bit(b'1'), 1 << 26);
        assert_eq!(obj_mask_bit(b'6'), 1 << 31);
        assert_eq!(obj_mask_bit(b'!'), 0, "unknown symbols contribute nothing");
    }

    /// The three bit assignments that are independently measured in code.
    #[test]
    fn measured_obj_mask_bits() {
        assert_eq!(
            OBJ_HOLDS_AIR, 0x0000_0200,
            "Build::train 0x0062F9B0 tests & 0x200"
        );
        assert_eq!(OBJ_MISSILE, 0x0800_0000, "the gate tests & 0x8000000");
        assert_eq!(OBJ_ANTI_AIR, 0x8000_0000, "has_objmask(0x80000000)");
        assert_eq!(
            UF_HELICOPTER, 0x0000_0020,
            "unit_flags & 0x20 at 0x0067BF74"
        );
        assert_eq!(
            UF_STRAFES, 0x0040_0000,
            "unit_flags & 0x400000 at 0x005EA47F"
        );
        assert_eq!(
            UF_FIRE_WHILE_MOVING, 0x0020_0000,
            "has_repeat_air 0x0046CEC0"
        );
    }

    /// Cross-check against the shipped data: the mapping must put `J` on exactly the
    /// air-hosting objects and `6` on exactly the anti-air ones.
    #[test]
    fn shipped_data_agrees_with_the_bit_mapping() {
        assert!(airbase().holds_air(), "Airbase OBJ_MASKS=GJ");
        assert!(!airbase().is_anti_air());
        assert!(jet_fighter().is_anti_air(), "X63T carries 6");
        assert!(!jet_fighter().holds_air());
        assert!(aa_gun().is_anti_air(), "6VT");
        assert!(
            sam_installation().is_anti_air() && is_detector(&sam_installation()),
            "Z6"
        );
        assert!(cruise_missile().is_missile(), "SBX32 carries 2");
        assert!(!bomber().is_missile(), "BSX3T has no 2");
        assert!(
            helicopter().is_helicopter() && is_detector(&helicopter()),
            "FLAGS f, mask Z"
        );
        assert!(!jet_fighter().is_helicopter());
    }

    #[test]
    fn type_offsets_match_the_pdb() {
        // schema/types.json, ObjectTypeData / UnitTypeData.
        assert_eq!(type_offsets::OBJ_MASKS, 0x1E4);
        assert_eq!(type_offsets::DOMAIN, 0x218);
        assert_eq!(type_offsets::LOS, 0x21C);
        assert_eq!(type_offsets::FLY_HIGH, 0x250);
        assert_eq!(type_offsets::FLY_LOW, 0x254);
        assert_eq!(type_offsets::UNIT_FLAGS, 0x2B4);
        assert_eq!(type_offsets::MANA, 0x2EC);
        // fly_high and fly_low are adjacent ints, which is why one `cmp` differs from the
        // other only in the displacement.
        assert_eq!(type_offsets::FLY_LOW - type_offsets::FLY_HIGH, 4);
        // schema/types.json, AirOrder.
        assert_eq!(air_order_offsets::OXX, 4);
        assert_eq!(air_order_offsets::RETURNING, 0x18);
    }

    // ---- predicates ----

    #[test]
    fn is_plane_excludes_helicopters_but_not_missiles() {
        assert!(jet_fighter().is_plane());
        assert!(bomber().is_plane());
        assert!(
            cruise_missile().is_plane(),
            "UnitData::is_plane does not test '2'"
        );
        assert!(!helicopter().is_plane());
        assert!(!aa_gun().is_plane());
    }

    #[test]
    fn flight_band_requires_air_not_heli_not_missile() {
        assert!(jet_fighter().has_flight_band());
        assert!(!helicopter().has_flight_band());
        assert!(!cruise_missile().has_flight_band());
        assert!(!aa_gun().has_flight_band());
    }

    #[test]
    fn is_on_map_is_bit_15_of_inside_up() {
        assert!(is_on_map(-1));
        assert!(is_on_map(i16::MIN));
        assert!(!is_on_map(0));
        assert!(!is_on_map(0x7FFF));
    }

    #[test]
    fn flying_low_only_while_engaging_within_twelve_tiles() {
        let t = jet_fighter();
        let base = FlightContext {
            inside_up: -1,
            order: ORD_STRAFE,
            order_target_active: true,
            order_target_dist: FLY_LOW_ENGAGE_RANGE - 1,
            ..Default::default()
        };
        assert!(is_flying_low(&t, &base));
        assert_eq!(flight_band(&t, &base), FlightBand::Low);

        let far = FlightContext {
            order_target_dist: FLY_LOW_ENGAGE_RANGE,
            ..base
        };
        assert!(
            !is_flying_low(&t, &far),
            "0x900 is exclusive: `jl`, not `jle`"
        );
        assert!(is_flying_high(&t, &far));
        assert_eq!(flight_band(&t, &far), FlightBand::High);

        let parked = FlightContext {
            inside_up: 5,
            ..base
        };
        assert!(!is_flying_low(&t, &parked) && !is_flying_high(&t, &parked));
        assert_eq!(flight_band(&t, &parked), FlightBand::Parked);

        let idle = FlightContext {
            order: ORD_NONE,
            ..base
        };
        assert!(
            !is_flying_low(&t, &idle),
            "ORD_NONE returns 0 before the distance test"
        );

        // The AIR_PATROL arm falls through to the fallback target.
        let patrol = FlightContext {
            order: ORD_AIR_PATROL,
            fallback_target_active: true,
            fallback_target_dist: 100,
            ..base
        };
        assert!(is_flying_low(&t, &patrol));
    }

    #[test]
    fn high_and_low_are_exclusive_and_exhaustive_on_the_map() {
        let t = bomber();
        for dist in [0, 0x8FF, 0x900, 0x2000] {
            for active in [false, true] {
                let c = FlightContext {
                    inside_up: -1,
                    order: ORD_STRAFE,
                    order_target_active: active,
                    order_target_dist: dist,
                    ..Default::default()
                };
                assert_ne!(
                    is_flying_low(&t, &c),
                    is_flying_high(&t, &c),
                    "exactly one band must hold for an on-map banded aircraft"
                );
            }
        }
    }

    #[test]
    fn is_air_order_is_the_three_measured_indices() {
        for o in 0..28 {
            let want = o == 16 || o == 17 || o == 24;
            assert_eq!(is_air_order(o), want, "order {o}");
        }
    }

    // ---- THE GATE ----

    #[test]
    fn ground_attack_order_skips_the_gate_entirely() {
        let (s, t) = (plain_ground(), jet_fighter());
        for ord in [ORD_ATTACK_GROUND, ORD_AIR_ATTACK_GROUND] {
            let mut rng = Random::new(1);
            let before = rng.state();
            let sh = AntiAirShot {
                shooter_order: ord,
                ..shot(&s, &t, true)
            };
            let g = antiair_dud_gate(&sh, &mut rng);
            assert_eq!(g.draws, 0);
            assert_eq!(g.arm, AntiAirArm::GroundAttackOrder);
            assert!(!g.dud);
            assert_eq!(rng.state(), before, "the stream must not move");
        }
    }

    #[test]
    fn a_building_shooter_still_reaches_the_gate() {
        // 0x0067BE7E: `je 0x67BEF8` — a non-unit shooter skips the order test, not the gate.
        let (s, t) = (sam_installation(), jet_fighter());
        let mut rng = Random::new(9);
        let sh = AntiAirShot {
            shooter_is_unit: false,
            shooter_order: 999,
            ..shot(&s, &t, true)
        };
        let g = antiair_dud_gate(&sh, &mut rng);
        assert_eq!(g.draws, 1, "SAM is anti-air and land: exactly one roll");
        assert_eq!(g.arm, AntiAirArm::AntiAirSingleRoll);
    }

    #[test]
    fn a_negative_target_aborts_ammo_init() {
        let (s, t) = (aa_gun(), jet_fighter());
        for (whom, ox, active) in [(-1, 3, true), (3, -1, true), (3, 3, false)] {
            let mut rng = Random::new(4);
            let before = rng.state();
            let sh = AntiAirShot {
                whom,
                ox,
                target_active: active,
                ..shot(&s, &t, false)
            };
            let g = antiair_dud_gate(&sh, &mut rng);
            assert!(g.init_aborted);
            assert_eq!(g.draws, 0);
            assert_eq!(rng.state(), before);
        }
    }

    #[test]
    fn non_air_targets_never_roll() {
        let s = aa_gun();
        for t in [plain_ground(), helicopter(), cruise_missile()] {
            let mut rng = Random::new(11);
            let before = rng.state();
            let g = antiair_dud_gate(&shot(&s, &t, false), &mut rng);
            assert_eq!(g.draws, 0, "target {t:?}");
            assert_eq!(g.arm, AntiAirArm::TargetNotBanded);
            assert!(!g.dud);
            assert_eq!(rng.state(), before);
        }
        // A building target is not a unit at all.
        let t = jet_fighter();
        let mut rng = Random::new(11);
        let sh = AntiAirShot {
            target_is_unit: false,
            ..shot(&s, &t, false)
        };
        assert_eq!(antiair_dud_gate(&sh, &mut rng).draws, 0);
    }

    #[test]
    fn airborne_anti_air_never_rolls_and_never_duds() {
        // A Jet Fighter (`X63T`: has '6', DOMAIN Air) shooting a Bomber.
        let (s, t) = (jet_fighter(), bomber());
        let mut rng = Random::new(1234);
        let before = rng.state();
        let g = antiair_dud_gate(&shot(&s, &t, true), &mut rng);
        assert_eq!(g.arm, AntiAirArm::AntiAirFromTheAir);
        assert_eq!(g.draws, 0);
        assert!(!g.dud, "fighters always connect");
        assert_eq!(rng.state(), before);
    }

    #[test]
    fn ground_anti_air_draws_exactly_once() {
        let (s, t) = (aa_gun(), bomber());
        for low in [false, true] {
            let mut rng = Random::new(77);
            let g = antiair_dud_gate(&shot(&s, &t, low), &mut rng);
            assert_eq!(g.draws, 1);
            assert_eq!(g.arm, AntiAirArm::AntiAirSingleRoll);
        }
    }

    /// The AA gun's own percentages decide, not the target's.
    #[test]
    fn anti_air_uses_the_shooters_percentage() {
        let t = bomber(); // FH 0, FL 10 — would dud almost always if it were the term used
                          // fly_high = 100 -> always hits a high flier; fly_low = 0 -> never hits a low one.
        let always = AirTypeData {
            fly_high: 100,
            fly_low: 100,
            ..aa_gun()
        };
        let never = AirTypeData {
            fly_high: 0,
            fly_low: 0,
            ..aa_gun()
        };
        let mut rng = Random::new(5);
        for _ in 0..500 {
            assert!(!antiair_dud_gate(&shot(&always, &t, false), &mut rng).dud);
            assert!(!antiair_dud_gate(&shot(&always, &t, true), &mut rng).dud);
            assert!(antiair_dud_gate(&shot(&never, &t, false), &mut rng).dud);
            assert!(antiair_dud_gate(&shot(&never, &t, true), &mut rng).dud);
        }
    }

    /// The single most important behavioural fact in the module: the non-anti-air arm
    /// short-circuits, so it draws once on a failed first roll and twice otherwise.
    #[test]
    fn plain_shooter_short_circuits_the_second_draw() {
        let t_never = AirTypeData {
            fly_high: 0,
            fly_low: 0,
            ..bomber()
        };
        let s = plain_ground();
        let mut rng = Random::new(3);
        for _ in 0..200 {
            let g = antiair_dud_gate(&shot(&s, &t_never, false), &mut rng);
            assert_eq!(g.draws, 1, "target evasion 0 -> first roll always fails");
            assert_eq!(g.arm, AntiAirArm::PlainFirstRollFailed);
            assert!(g.dud);
        }

        let t_always = AirTypeData {
            fly_high: 100,
            fly_low: 100,
            ..bomber()
        };
        let mut rng = Random::new(3);
        for _ in 0..200 {
            let g = antiair_dud_gate(&shot(&s, &t_always, false), &mut rng);
            assert_eq!(
                g.draws, 2,
                "target evasion 100 -> the second roll always happens"
            );
            assert_eq!(g.arm, AntiAirArm::PlainBothRolls);
        }
    }

    /// Both rolls must pass. This is the developers' *"if either fails shot misses"*.
    #[test]
    fn plain_shooter_needs_both_rolls() {
        let t = AirTypeData {
            fly_high: 100,
            fly_low: 100,
            ..bomber()
        };
        let s_never = AirTypeData {
            fly_high: 0,
            fly_low: 0,
            ..plain_ground()
        };
        let s_always = AirTypeData {
            fly_high: 100,
            fly_low: 100,
            ..plain_ground()
        };
        let mut rng = Random::new(19);
        for _ in 0..300 {
            let a = antiair_dud_gate(&shot(&s_never, &t, false), &mut rng);
            assert!(a.dud && a.draws == 2);
            let b = antiair_dud_gate(&shot(&s_always, &t, false), &mut rng);
            assert!(!b.dud && b.draws == 2);
        }
    }

    /// The shipped numbers, exercised as a distribution. Not a fidelity claim — it checks
    /// that the *structure* produces the documented "two independent percent checks".
    #[test]
    fn shipped_percentages_produce_the_expected_shape() {
        let s = plain_ground(); // FH 0 / FL 0
        let t = jet_fighter(); // FH 0 / FL 10
        let mut rng = Random::new(0x5EED);
        let mut hits = 0;
        let mut draws = 0;
        for _ in 0..10_000 {
            let g = antiair_dud_gate(&shot(&s, &t, false), &mut rng);
            draws += g.draws;
            if !g.dud {
                hits += 1;
            }
        }
        assert_eq!(
            hits, 0,
            "a cruising fighter has FLY_HIGH 0: nothing non-AA can hit it"
        );
        assert_eq!(
            draws, 10_000,
            "and every one of those shots cost exactly one draw"
        );

        // The same fighter, low, against an AA gun: one draw, ~90% hit (the gun's FL).
        let g = aa_gun();
        let mut rng = Random::new(0x5EED);
        let mut hits = 0;
        for _ in 0..10_000 {
            if !antiair_dud_gate(&shot(&g, &t, true), &mut rng).dud {
                hits += 1;
            }
        }
        assert!(
            (8_500..9_500).contains(&hits),
            "AA gun FLY_LOW=90 gave {hits}/10000"
        );
    }

    /// The regression this lane exists to prevent: leaving the gate out changes the
    /// stream position, so a later consumer sees a different value.
    #[test]
    fn omitting_the_gate_desynchronises_the_stream() {
        let (s, t) = (aa_gun(), bomber());
        let mut with = Random::new(0xC0FFEE);
        let mut without = Random::new(0xC0FFEE);
        for _ in 0..8 {
            antiair_dud_gate(&shot(&s, &t, true), &mut with);
        }
        assert_ne!(
            with.state(),
            without.state(),
            "if this ever passes the gate has stopped consuming RNG"
        );
        // And the amount consumed is exactly 8 draws.
        for _ in 0..8 {
            without.advance();
        }
        assert_eq!(with.state(), without.state());
    }

    /// Frozen stream fingerprint. Any change to draw counts or ordering breaks this.
    #[test]
    fn gate_stream_fingerprint_is_frozen() {
        let types = [aa_gun(), plain_ground(), jet_fighter(), sam_installation()];
        let targets = [bomber(), jet_fighter(), helicopter(), cruise_missile()];
        let mut rng = Random::new(2024);
        let mut total_draws = 0u32;
        let mut duds = 0u32;
        for (i, s) in types.iter().enumerate() {
            for (j, t) in targets.iter().enumerate() {
                for k in 0..25 {
                    let g = antiair_dud_gate(&shot(s, t, (i + j + k) % 2 == 0), &mut rng);
                    total_draws += g.draws;
                    duds += u32::from(g.dud);
                }
            }
        }
        // A change detector over *this* port, not a retail fingerprint — nothing here has
        // been through the oracle. It bites if draw counts, branch order or the roll
        // formula change. 154 draws over 400 shots: the two arms that never roll
        // (`AntiAirFromTheAir` for the Jet Fighter shooter, `TargetNotBanded` for the
        // helicopter and missile targets) account for the rest.
        assert_eq!(
            (total_draws, duds, rng.state()),
            (154, 88, -2_024_157_294),
            "gate RNG consumption changed"
        );
    }

    #[test]
    fn apply_sets_only_the_no_damage_bit() {
        let s = AirTypeData {
            fly_high: 0,
            fly_low: 0,
            ..plain_ground()
        };
        let t = AirTypeData {
            fly_high: 100,
            fly_low: 100,
            ..bomber()
        };
        let mut rng = Random::new(6);
        let mut flags = 0x03u8; // FLAG_ALIVE | FLAG_FLYING already set by ammo.rs
        let g = apply_antiair_gate(&mut flags, &shot(&s, &t, false), &mut rng);
        assert!(g.dud);
        assert_eq!(flags, 0x13, "0x10 or'd in, nothing else touched");
    }

    #[test]
    fn aa_roll_is_get_0_ffff_mod_100() {
        let mut a = Random::new(1);
        let mut b = Random::new(1);
        for _ in 0..1000 {
            assert_eq!(aa_roll(&mut a), b.get(0, 0xFFFF) % 100);
        }
        // And it is one LCG step per roll.
        let mut c = Random::new(42);
        let mut d = Random::new(42);
        aa_roll(&mut c);
        d.advance();
        assert_eq!(c.state(), d.state());
    }

    // ---- airbase capacity ----

    #[test]
    fn capacity_is_seven_ten_one_zero() {
        assert_eq!(num_aircraft_limit(AirHost::Carrier), 7);
        assert_eq!(num_aircraft_limit(AirHost::Airbase), 10);
        assert_eq!(num_aircraft_limit(AirHost::MissileSilo), 1);
        assert_eq!(num_aircraft_limit(AirHost::Other), 0);
        assert_eq!(air_host_of(type_ids::AIRBASE), AirHost::Airbase);
        assert_eq!(air_host_of(type_ids::AIRCRAFT_CARRIER), AirHost::Carrier);
        assert_eq!(air_host_of(type_ids::NUCLEAR_SILO), AirHost::MissileSilo);
        assert_eq!(air_host_of(type_ids::JET_FIGHTER), AirHost::Other);
    }

    #[test]
    fn num_aircraft_here_counts_only_this_owners_air_units_homed_here() {
        let here = |o, w, d, a| HostedAircraft {
            active: a,
            domain: d,
            home_base_o: o,
            home_base_who: w,
        };
        let objs = [
            here(4, 1, DOMAIN_AIR, true),  // counts
            here(4, 1, DOMAIN_AIR, false), // inactive
            here(4, 1, DOMAIN_LAND, true), // not air
            here(5, 1, DOMAIN_AIR, true),  // different base
            here(4, 2, DOMAIN_AIR, true),  // different owner slot
            here(4, 1, DOMAIN_AIR, true),  // counts
        ];
        assert_eq!(
            num_aircraft_here(4, 1, AirHost::Airbase, &objs, 3, false, 0),
            2
        );
        // The carrier arm adds its own queue.
        assert_eq!(
            num_aircraft_here(4, 1, AirHost::Carrier, &objs, 3, false, 0),
            5
        );
        // The non-carrier queue arm is the UNDERIVED leader term, passed through.
        assert_eq!(
            num_aircraft_here(4, 1, AirHost::Airbase, &objs, 3, true, 9),
            11
        );
    }

    #[test]
    fn launch_needs_the_j_flag_and_spare_capacity() {
        let a = airbase();
        assert!(airbase_may_launch(&a, 0, 10));
        assert!(
            airbase_may_launch(&a, 10, 10),
            "the test is `here <= limit`"
        );
        assert!(!airbase_may_launch(&a, 11, 10));
        assert!(
            !airbase_may_launch(&plain_ground(), 0, 10),
            "no 'J', no launch path"
        );
    }

    // ---- fuel ----

    #[test]
    fn mana_cap_is_the_type_value_without_the_space_program() {
        assert_eq!(mana_cap(&jet_fighter(), false, 0), 500);
        assert_eq!(
            mana_cap(&jet_fighter(), true, 0),
            500,
            "space_air_range is 0% in this build"
        );
        assert_eq!(mana_cap(&jet_fighter(), true, 50), 750);
        assert_eq!(
            mana_cap(&helicopter(), true, 50),
            0,
            "MANA 0 short-circuits to 0"
        );
    }

    #[test]
    fn fuel_burns_one_per_airborne_frame_and_recovers_two_parked() {
        let cap = 500;
        let mut burn: i16 = 0;
        for _ in 0..500 {
            burn = air_fuel_step(burn, cap, true);
        }
        assert_eq!(burn, 500);
        assert_eq!(mana_left(cap, burn), 0);
        assert_eq!(air_fuel_step(burn, cap, true), 500, "clamped at the cap");

        // 500 frames of burn take 250 frames of parking to undo.
        let mut frames = 0;
        while burn > 0 {
            burn = air_fuel_step(burn, cap, false);
            frames += 1;
        }
        assert_eq!(frames, 250);
        assert_eq!(burn, 0);
    }

    #[test]
    fn odd_fuel_lands_exactly_on_zero() {
        // `if (2 < burn) burn -= 2 else burn = 0` — a burn of 1 or 2 goes straight to 0.
        assert_eq!(air_fuel_step(1, 500, false), 0);
        assert_eq!(air_fuel_step(2, 500, false), 0);
        assert_eq!(air_fuel_step(3, 500, false), 1);
    }

    #[test]
    fn empty_fuel_latches_returning_except_for_missiles() {
        assert_eq!(check_fuel_latch_returning(&bomber(), 0, 0), 1);
        assert_eq!(check_fuel_latch_returning(&bomber(), 0, 1), 0);
        assert_eq!(
            check_fuel_latch_returning(&bomber(), 1, 5),
            1,
            "latched stays latched"
        );
        assert_eq!(
            check_fuel_latch_returning(&cruise_missile(), 0, 0),
            0,
            "missiles are one-way"
        );
    }

    #[test]
    fn check_fuel_decision_table() {
        let b = bomber();
        assert_eq!(
            check_fuel(&b, 0, 10, false, None, None).1,
            FuelVerdict::KeepFlying
        );
        assert_eq!(
            check_fuel(&b, 0, 0, true, Some((4, 1)), None).1,
            FuelVerdict::ReturnTo { o: 4, who: 1 }
        );
        assert_eq!(
            check_fuel(&b, 1, 5, false, Some((4, 1)), Some((9, 1))).1,
            FuelVerdict::ReturnTo { o: 9, who: 1 },
            "a dead current host is replaced by the nearest"
        );
        assert_eq!(
            check_fuel(&b, 1, 0, false, None, None).1,
            FuelVerdict::Crash
        );
        assert_eq!(
            check_fuel(&helicopter(), 1, 0, false, None, None).1,
            FuelVerdict::LandInPlace
        );
        // A type with MANA 0 that has not been recalled never enters the machine.
        assert_eq!(
            check_fuel(&helicopter(), 0, 0, false, None, None).1,
            FuelVerdict::KeepFlying
        );
    }

    #[test]
    fn missiles_self_destruct_after_release() {
        assert!(air_attack_ground_self_destructs(&cruise_missile()));
        assert!(!air_attack_ground_self_destructs(&bomber()));
        assert_eq!(
            bombing_fuel_cost(true),
            0,
            "BOMBING_MANA_COST is 0 in this build"
        );
    }

    #[test]
    fn bomb_release_angle_window() {
        let b = bomber();
        assert!(air_bomb_release_allowed(&b, 0, false));
        assert!(air_bomb_release_allowed(
            &b,
            AIR_BOMB_ANGLE_TOLERANCE,
            false
        ));
        assert!(!air_bomb_release_allowed(
            &b,
            AIR_BOMB_ANGLE_TOLERANCE + 1,
            false
        ));
        assert!(air_bomb_release_allowed(
            &b,
            AIR_BOMB_ANGLE_TOLERANCE + 1,
            true
        ));
        assert!(!air_bomb_release_allowed(&b, AIR_BOMB_ANGLE_MAX + 1, true));
        assert!(
            air_bomb_release_allowed(&cruise_missile(), u32::MAX, false),
            "missiles bypass the facing gate"
        );
        // Angles are full-circle binary: 2^32 per revolution.
        const FULL_TURN: u64 = 0x1_0000_0000;
        assert_eq!(
            u64::from(AIR_BOMB_ANGLE_TOLERANCE) + 1,
            FULL_TURN / 24,
            "0x0AAAAAA9 is one tick under 2^32/24 = 15 degrees"
        );
        assert_eq!(
            u64::from(AIR_BOMB_ANGLE_MAX),
            FULL_TURN / 6,
            "0x2AAAAAAA is 2^32/6 = 60 degrees"
        );
    }

    // ---- patrol ----

    #[test]
    fn patrol_scan_cadence_is_phase_offset_by_object_index() {
        // (o + frame) % 16 == 0 and % 32 == 0.
        assert!(air_patrol_unit_scan_due(0, 0));
        assert!(air_patrol_unit_scan_due(3, 13));
        assert!(!air_patrol_unit_scan_due(3, 14));
        assert!(air_patrol_building_scan_due(0, 32));
        assert!(!air_patrol_building_scan_due(0, 16));
        // Two aircraft with different indices never scan on the same frame pattern.
        let a: Vec<i32> = (0..64)
            .filter(|f| air_patrol_unit_scan_due(1, *f))
            .collect();
        let b: Vec<i32> = (0..64)
            .filter(|f| air_patrol_unit_scan_due(2, *f))
            .collect();
        assert_ne!(a, b);
        assert_eq!(a.len(), 4);
    }

    #[test]
    fn patrol_waypoint_advance_and_queue_retirement() {
        // Not yet arrived.
        assert_eq!(air_patrol_advance(0, 4, 0x241, 3), (0, false));
        // Arrived, advance.
        assert_eq!(air_patrol_advance(0, 4, 0x23F, 3), (1, false));
        // Arrived at the last waypoint with queued follow-on work -> retire patrol.
        assert_eq!(air_patrol_advance(3, 4, 0, 3), (3, true));
        // Arrived at the last waypoint with no follow-on order -> hold.
        assert_eq!(air_patrol_advance(3, 4, 0, 1), (3, false));
        // Cursor past the end wraps first (0x005EA64B).
        assert_eq!(air_patrol_advance(9, 4, 0x300, 3), (0, false));
    }

    #[test]
    fn air_order_walk_is_six_ints() {
        let w = AirOrderWalk {
            oxx: 4,
            whose: 1,
            cruising_alt: 300,
            sharp_turn: 0,
            old: 0,
            returning: 1,
        };
        assert_eq!(
            core::mem::size_of::<AirOrderWalk>(),
            24,
            "AirOrder::walk_data walks 24 bytes"
        );
        assert_eq!(w.returning, 1);
    }
}
