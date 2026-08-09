//! Transaction plans for the three missing coordinate-target order executors.
//!
//! This file deliberately contains no `World` adapter.  The retail functions read through
//! the object table, terrain ownership, the order vtable and animation data before they
//! mutate anything.  Flattening those reads into permissive booleans would make a missing
//! host callback look like a legitimate negative answer, so every conditional read is a
//! [`HostFact`].  A plan is returned only after the branch's complete read set is present.
//!
//! Instruction-level sources (Extended Edition `riseofnations.exe`):
//!
//! * `Unit::do_explore_to` `0x005F24A0..0x005F253B`;
//! * `Unit::do_attack_ground` `0x005F1410..0x005F190F`;
//! * `Unit::do_air_attack_ground` `0x005EA420..0x005EA61C`.
//!
//! Numeric field identities come from the shipped PDB: `AttackGroundOrder` is 32 bytes
//! (`att_x +4`, `att_y +8`, `accuracy +12`, `attack_unit +16`) and
//! `AirAttackGroundOrder` is 72 bytes (`AirOrder` at +20, `returning` at concrete +44).
//! These planners preserve mutation order; an adapter must apply each [`OrderEffect`] in
//! sequence and must not skip presentation-independent effects.

/// A fact which the retail executor obtained from a mandatory host read.
///
/// `Missing` is distinct from `Known(false)`: the former stops the transaction, while the
/// latter follows retail's negative branch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostFact<T> {
    Known(T),
    Missing(&'static str),
}

impl<T> HostFact<T> {
    pub const fn known(value: T) -> Self {
        Self::Known(value)
    }

    pub const fn missing(name: &'static str) -> Self {
        Self::Missing(name)
    }

    fn require(self) -> Result<T, MissingHostFact> {
        match self {
            Self::Known(value) => Ok(value),
            Self::Missing(name) => Err(MissingHostFact(name)),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MissingHostFact(pub &'static str);

/// `UnitAnim` values used by the two executors, from the PDB enum.
pub mod anim {
    pub const DEFAULT: i32 = 0;
    pub const ATTACK1: i32 = 11;
    pub const ATTACK2: i32 = 12;
}

/// One ordered world mutation/callback selected by an executor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OrderEffect {
    /// `Unit::do_move(order)`; always first in `do_explore_to`.
    DoMove,
    /// `Unit::explore()` `0x005F2540`.
    Explore,
    /// `Unit::kill_current_order(reason)`.
    KillCurrentOrder(i32),
    /// `AttackGroundOrder::attack_unit = 1`.
    StoreAttackUnit(i32),
    /// `Unit::update_action()` through virtual slot `+0x188`.
    UpdateAction,
    /// `Unit::set_attack(-1, -1)`.
    SetAttack { object: i32, owner: i32 },
    /// Exact spell redirect at `0x005F1827..0x005F1848`.
    AddCastOrder {
        object: i32,
        owner: i32,
        x: i32,
        y: i32,
        spell: i32,
        queue: i32,
        group: i32,
    },
    /// OR into `ObjectData::flags` at actor `+0x68`.
    OrObjectFlags(u32),
    /// `Unit::set_angle(angle, 0, 0)`.
    SetAngle(u32),
    /// OR into the virtual `UnitOrder::flags` byte.
    OrOrderFlags(u8),
    /// `Unit::set_anim(anim, b, c)`.
    SetAnimation { anim: i32, b: i32, c: i32 },
    /// `Object::fire_ammo(o, who)`.
    FireAmmo { object: i32, owner: i32 },
    /// Store `UnitData::recharging` at actor `+0xAE`.
    StoreRecharge(u8),
    /// Add to `UnitData::mana_burn` at actor `+0x96`.
    AddManaBurn(i16),
    /// `Object::die(0, -1, 0.0)`; the float is the literal zero at the call site.
    Die,
    /// The exact `add_move_facing_order` request selected by attack-ground repositioning.
    AddMoveFacing(AttackGroundMove),
    /// The local-player error side effect at `0x005F1707..0x005F1754`.
    PresentCannotReachAttackGround,
}

pub const ORDER_FACING_TARGET: u8 = 0x80;
pub const OBJECT_ATTACK_GROUND_ACTIVE: u32 = 0x0001_1000;
pub const ATTACK_GROUND_SPECIAL_SPELL: i32 = 0x28C;

/// PDB `sizeof(AttackGroundOrder)` / `sizeof(AirAttackGroundOrder)`.
pub const SIZEOF_ATTACK_GROUND_ORDER: usize = 32;
pub const SIZEOF_AIR_ATTACK_GROUND_ORDER: usize = 72;

/// Checksum-visible fields owned by `AttackGroundOrder`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AttackGroundOrderState {
    pub att_x: i32,
    pub att_y: i32,
    pub accuracy: i32,
    pub attack_unit: i32,
}

/// Checksum-visible fields owned by `AirAttackGroundOrder` and its two bases.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AirAttackGroundOrderState {
    pub attack: AttackGroundOrderState,
    pub air: crate::systems::air::AirOrderWalk,
    pub total_time: i32,
    pub sx: i32,
    pub sy: i32,
}

/// Concrete offsets in the retail objects. `AirOrder`'s own offsets are documented by
/// `systems::air::air_order_offsets`; its subobject begins at concrete +20 here.
pub mod order_offsets {
    pub const ATT_X: usize = 4;
    pub const ATT_Y: usize = 8;
    pub const ACCURACY: usize = 12;
    pub const ATTACK_UNIT: usize = 16;
    pub const AIR_BASE: usize = 20;
    pub const AIR_RETURNING: usize = AIR_BASE + 24;
    pub const TOTAL_TIME: usize = 48;
    pub const SX: usize = 52;
    pub const SY: usize = 56;
}

// ---------------------------------------------------------------------------
// EXPLORE_TO (arm 3)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExploreToFacts {
    pub frame: i32,
    pub object_index: i16,
    /// Identity check performed after `do_move` refreshes the current order pointer.
    pub current_order_is_same: HostFact<bool>,
    /// `UnitData::is_on_map()` through vtable slot `+0xE8`.
    pub actor_is_on_map: HostFact<bool>,
}

/// Exact signed remainder gate at `0x005F24B9..0x005F24C8`.
#[inline]
pub const fn explore_scan_due(frame: i32, object_index: i16) -> bool {
    frame.wrapping_add(object_index as i32) % 15 == 0
}

/// Plan `Unit::do_explore_to` without allowing a missing post-move read to become false.
pub fn plan_explore_to(facts: ExploreToFacts) -> Result<Vec<OrderEffect>, MissingHostFact> {
    let mut effects = vec![OrderEffect::DoMove];
    if !explore_scan_due(facts.frame, facts.object_index) {
        return Ok(effects);
    }
    if !facts.current_order_is_same.require()? {
        return Ok(effects);
    }
    if facts.actor_is_on_map.require()? {
        effects.push(OrderEffect::Explore);
    }
    Ok(effects)
}

// ---------------------------------------------------------------------------
// Shared binary-angle operations
// ---------------------------------------------------------------------------

/// The unsigned circular-distance idiom used by all three recovered executors.
///
/// Retail subtracts, compares to `0x80000000`, and complements values above the midpoint.
/// The off-by-one versus a mathematical absolute value is intentional.
#[inline]
pub const fn retail_angle_error(actual: u32, desired: u32) -> u32 {
    let delta = actual.wrapping_sub(desired);
    if delta > 0x8000_0000 {
        !delta
    } else {
        delta
    }
}

/// `do_attack_ground`'s side-firing (`unit_flags & 0x40`) facing selection.
///
/// The strict `cmovb` means an exact tie keeps the `+90 degree` candidate.
#[inline]
pub const fn attack_ground_facing(
    raw_target_angle: u32,
    actor_angle: u32,
    side_firing: bool,
) -> u32 {
    if !side_firing {
        return raw_target_angle;
    }
    let minus = raw_target_angle.wrapping_sub(0x4000_0000);
    let plus = raw_target_angle.wrapping_add(0x4000_0000);
    if retail_angle_error(actor_angle, minus) < retail_angle_error(actor_angle, plus) {
        minus
    } else {
        plus
    }
}

// ---------------------------------------------------------------------------
// ATTACK_GROUND (arm 23)
// ---------------------------------------------------------------------------

/// Result of `UnitType::find_nearby_spot` followed by retail's second
/// `ObjectData::is_in_range` check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NearbyAttackGroundSpot {
    NotFound,
    Found { x: i32, y: i32, in_range: bool },
}

/// Exact request values handed to `UnitType::find_nearby_spot` at `0x005F1664`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AttackGroundSearch {
    pub target_x: i32,
    pub target_y: i32,
    pub radius: i32,
    pub facing_filter: u32,
    pub filter_index: i32,
    pub actor_owner: i32,
    pub actor_object: i32,
}

/// Exact `Unit::add_move_facing_order` transaction after a successful search.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AttackGroundMove {
    pub x: i32,
    pub y: i32,
    pub facing: u32,
    pub queue: i32,
    pub mandatory: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttackGroundFacts {
    pub actor_owner: i32,
    pub actor_object: i32,
    pub actor_angle: u32,
    pub target_x: i32,
    pub target_y: i32,
    pub raw_target_angle: u32,
    pub side_firing: bool,
    pub can_attack_ground: HostFact<bool>,
    /// `AttackGroundOrder::attack_unit` at +16.
    pub attack_unit: i32,
    /// Owner byte of the terrain cell, required only for a non-unit attack.
    pub terrain_owner: HostFact<i32>,
    /// `LeaderData::is_peace(terrain_owner)`, required only for an owned cell.
    pub at_peace_with_terrain_owner: HostFact<bool>,
    /// Direct range test; `attack_unit != 0` bypasses this in retail.
    pub target_is_in_range: HostFact<bool>,
    /// `UnitData::recharging` at +0xAE.
    pub recharge: u8,
    /// Virtual `UnitOrder::flags & 0x80`, required while recharging.
    pub order_facing_target: HostFact<bool>,
    /// Current animation class byte at `GuyType +0x9C`, required while recharging and not
    /// already facing. Negative or greater than three restarts the default animation.
    pub current_animation: HostFact<i8>,
    /// `unit_type_flags2 & 4 && object_flags & 0x80000`.
    pub redirects_to_special_cast: bool,
    /// `unit_flags & 0x02000000`.
    pub directional_attack_animation: bool,
    /// Whether the type has an `AmmoType` at +0x2CC.
    pub has_ammo: bool,
    /// Exact return byte from virtual `get_recharge(&local)` at +0x134.
    pub recharge_delay: u8,
    /// Fine distance from actor to target, before subtracting `UnitTypeData +0x244`.
    pub vector_distance: i32,
    pub range_bias: i32,
    /// The two virtual range values used at +0x12C/+0x130.
    pub near_range: i32,
    pub far_range: i32,
    pub nearby_spot: HostFact<NearbyAttackGroundSpot>,
    /// Whether this actor belongs to `Game::our_player`, needed only on failed reposition.
    pub is_local_player: HostFact<bool>,
}

/// Fine-unit search radius selected by `0x005F1571..0x005F1664`.
#[inline]
pub const fn attack_ground_reposition_radius(
    vector_distance: i32,
    range_bias: i32,
    near_range: i32,
    far_range: i32,
) -> i32 {
    let effective = vector_distance.wrapping_sub(range_bias);
    let near_scaled = near_range.wrapping_mul(0xC0);
    if effective < near_scaled.wrapping_sub(6) {
        near_scaled.wrapping_add(0x90)
    } else {
        let far_scaled = far_range.wrapping_mul(0xC0);
        if effective > far_scaled.wrapping_sub(6) {
            far_scaled.wrapping_sub(0x30)
        } else {
            effective
        }
    }
}

/// Form the exact non-pointer portion of the `find_nearby_spot` call. The adapter owns
/// the two out-pointers and the remaining literal zero/negative-one arguments.
#[inline]
pub const fn attack_ground_search(
    actor_owner: i32,
    actor_object: i32,
    target_x: i32,
    target_y: i32,
    facing: u32,
    vector_distance: i32,
    range_bias: i32,
    near_range: i32,
    far_range: i32,
) -> AttackGroundSearch {
    AttackGroundSearch {
        target_x,
        target_y,
        radius: attack_ground_reposition_radius(vector_distance, range_bias, near_range, far_range),
        facing_filter: facing.wrapping_add(0x8000_0000),
        filter_index: 3,
        actor_owner,
        actor_object,
    }
}

fn failed_attack_ground_plan(local: HostFact<bool>) -> Result<Vec<OrderEffect>, MissingHostFact> {
    let mut effects = Vec::new();
    if local.require()? {
        effects.push(OrderEffect::PresentCannotReachAttackGround);
    }
    effects.push(OrderEffect::KillCurrentOrder(0));
    Ok(effects)
}

/// Plan all state-changing branches of `Unit::do_attack_ground`.
pub fn plan_attack_ground(facts: AttackGroundFacts) -> Result<Vec<OrderEffect>, MissingHostFact> {
    if !facts.can_attack_ground.require()? {
        return Ok(vec![OrderEffect::KillCurrentOrder(0)]);
    }

    if facts.attack_unit == 0 {
        let terrain_owner = facts.terrain_owner.require()?;
        if terrain_owner >= 0 && facts.at_peace_with_terrain_owner.require()? {
            return Ok(vec![OrderEffect::KillCurrentOrder(0)]);
        }
    }

    let facing = attack_ground_facing(facts.raw_target_angle, facts.actor_angle, facts.side_firing);
    let in_range = if facts.attack_unit != 0 {
        true
    } else {
        facts.target_is_in_range.require()?
    };

    if !in_range {
        let _request = attack_ground_search(
            facts.actor_owner,
            facts.actor_object,
            facts.target_x,
            facts.target_y,
            facing,
            facts.vector_distance,
            facts.range_bias,
            facts.near_range,
            facts.far_range,
        );
        return match facts.nearby_spot.require()? {
            NearbyAttackGroundSpot::Found {
                x,
                y,
                in_range: true,
            } => Ok(vec![OrderEffect::AddMoveFacing(AttackGroundMove {
                x,
                y,
                facing,
                queue: 1,
                mandatory: 0,
            })]),
            NearbyAttackGroundSpot::NotFound
            | NearbyAttackGroundSpot::Found {
                in_range: false, ..
            } => failed_attack_ground_plan(facts.is_local_player),
        };
    }

    if facts.recharge != 0 {
        if facts.order_facing_target.require()? {
            return Ok(Vec::new());
        }
        let current_animation = facts.current_animation.require()?;
        if current_animation < 0 || current_animation > 3 {
            return Ok(vec![OrderEffect::SetAnimation {
                anim: anim::DEFAULT,
                b: 1,
                c: 1,
            }]);
        }
        return Ok(Vec::new());
    }

    let mut effects = Vec::new();
    if facts.attack_unit > 0 {
        if facts.attack_unit == 1 {
            effects.push(OrderEffect::KillCurrentOrder(0));
            effects.push(OrderEffect::UpdateAction);
            return Ok(effects);
        }
        effects.push(OrderEffect::StoreAttackUnit(1));
    }

    effects.push(OrderEffect::SetAttack {
        object: -1,
        owner: -1,
    });
    if facts.redirects_to_special_cast {
        effects.push(OrderEffect::AddCastOrder {
            object: -1,
            owner: -1,
            x: -1,
            y: -1,
            spell: ATTACK_GROUND_SPECIAL_SPELL,
            queue: 0,
            group: 0,
        });
        return Ok(effects);
    }

    effects.push(OrderEffect::OrObjectFlags(OBJECT_ATTACK_GROUND_ACTIVE));
    if facing != facts.actor_angle {
        effects.push(OrderEffect::SetAngle(facing));
    }
    effects.push(OrderEffect::OrOrderFlags(ORDER_FACING_TARGET));
    let (attack_anim, variation) = if facts.directional_attack_animation {
        let signed = facts.raw_target_angle.wrapping_sub(facing) as i32;
        (
            if signed < 0 {
                anim::ATTACK1
            } else {
                anim::ATTACK2
            },
            0,
        )
    } else {
        (anim::ATTACK1, 1)
    };
    effects.push(OrderEffect::SetAnimation {
        anim: attack_anim,
        b: 0,
        c: variation,
    });
    if facts.has_ammo {
        effects.push(OrderEffect::FireAmmo {
            object: -1,
            owner: -1,
        });
    }
    effects.push(OrderEffect::StoreRecharge(
        facts.recharge_delay.wrapping_add(1),
    ));
    Ok(effects)
}

// ---------------------------------------------------------------------------
// AIR_ATTACK_GROUND (arm 24)
// ---------------------------------------------------------------------------

pub const AIR_BOMB_ANGLE_TOLERANCE: u32 = 0x0AAA_AAA9;
pub const AIR_BOMB_ANGLE_MAX: u32 = 0x2AAA_AAAA;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AirAttackGroundFacts {
    /// Return from `Unit::do_air_physics(order, att_x, att_y)`.
    pub physics_complete: HostFact<bool>,
    pub recharge: u8,
    /// `AirOrder::returning` at concrete `AirAttackGroundOrder +44`.
    pub returning: i32,
    pub actor_angle: u32,
    pub target_angle: u32,
    pub target_is_in_range: HostFact<bool>,
    pub is_missile: bool,
    pub is_jet_fighter: HostFact<bool>,
    pub strafes: bool,
    pub is_bomber: HostFact<bool>,
    pub has_ammo: bool,
    pub recharge_delay: u8,
    pub bombing_mana_cost: i16,
}

/// Plan `Unit::do_air_attack_ground`; `DoAirPhysics` remains the caller's mandatory
/// preflight because it mutates flight state before any branch in this body.
pub fn plan_air_attack_ground(
    facts: AirAttackGroundFacts,
) -> Result<Vec<OrderEffect>, MissingHostFact> {
    if !facts.physics_complete.require()? || facts.recharge != 0 || facts.returning != 0 {
        return Ok(Vec::new());
    }
    if !facts.target_is_in_range.require()? {
        return Ok(Vec::new());
    }

    let angle_error = retail_angle_error(facts.actor_angle, facts.target_angle);
    if !facts.is_missile && angle_error > AIR_BOMB_ANGLE_TOLERANCE {
        if !facts.is_jet_fighter.require()? || angle_error > AIR_BOMB_ANGLE_MAX {
            return Ok(Vec::new());
        }
    }

    let mut effects = vec![OrderEffect::SetAttack {
        object: -1,
        owner: -1,
    }];
    let is_bomber = facts.is_bomber.require()?;
    if !facts.strafes && !is_bomber && facts.has_ammo {
        effects.push(OrderEffect::FireAmmo {
            object: -1,
            owner: -1,
        });
    } else {
        effects.push(OrderEffect::SetAnimation {
            anim: anim::ATTACK2,
            b: 0,
            c: 1,
        });
    }

    if facts.is_missile {
        effects.push(OrderEffect::Die);
        return Ok(effects);
    }

    effects.push(OrderEffect::StoreRecharge(
        facts.recharge_delay.wrapping_add(1),
    ));
    if is_bomber {
        effects.push(OrderEffect::AddManaBurn(facts.bombing_mana_cost));
    }
    Ok(effects)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k<T>(value: T) -> HostFact<T> {
        HostFact::known(value)
    }

    fn ground() -> AttackGroundFacts {
        AttackGroundFacts {
            actor_owner: 2,
            actor_object: 19,
            actor_angle: 0x1000_0000,
            target_x: 480,
            target_y: 960,
            raw_target_angle: 0x2000_0000,
            side_firing: false,
            can_attack_ground: k(true),
            attack_unit: 0,
            terrain_owner: k(-1),
            at_peace_with_terrain_owner: HostFact::missing("unused peace fact"),
            target_is_in_range: k(true),
            recharge: 0,
            order_facing_target: HostFact::missing("unused facing fact"),
            current_animation: HostFact::missing("unused animation fact"),
            redirects_to_special_cast: false,
            directional_attack_animation: false,
            has_ammo: true,
            recharge_delay: 4,
            vector_distance: 2000,
            range_bias: 0,
            near_range: 4,
            far_range: 8,
            nearby_spot: HostFact::missing("unused spot fact"),
            is_local_player: HostFact::missing("unused local-player fact"),
        }
    }

    fn air() -> AirAttackGroundFacts {
        AirAttackGroundFacts {
            physics_complete: k(true),
            recharge: 0,
            returning: 0,
            actor_angle: 0,
            target_angle: 0,
            target_is_in_range: k(true),
            is_missile: false,
            is_jet_fighter: HostFact::missing("unused jet fact"),
            strafes: false,
            is_bomber: k(false),
            has_ammo: true,
            recharge_delay: 8,
            bombing_mana_cost: 13,
        }
    }

    #[test]
    fn explore_move_is_unconditional_and_scan_is_post_move() {
        let p = plan_explore_to(ExploreToFacts {
            frame: 14,
            object_index: 1,
            current_order_is_same: k(true),
            actor_is_on_map: k(true),
        })
        .unwrap();
        assert_eq!(p, vec![OrderEffect::DoMove, OrderEffect::Explore]);

        let p = plan_explore_to(ExploreToFacts {
            frame: 13,
            object_index: 1,
            current_order_is_same: HostFact::missing("not read off cadence"),
            actor_is_on_map: HostFact::missing("not read off cadence"),
        })
        .unwrap();
        assert_eq!(p, vec![OrderEffect::DoMove]);
        assert!(explore_scan_due(-16, 1), "signed -15 remainder is zero");
    }

    #[test]
    fn explore_missing_post_move_identity_fails_closed() {
        assert_eq!(
            plan_explore_to(ExploreToFacts {
                frame: 0,
                object_index: 0,
                current_order_is_same: HostFact::missing("current order after do_move"),
                actor_is_on_map: k(true),
            }),
            Err(MissingHostFact("current order after do_move"))
        );
    }

    #[test]
    fn mutation_side_firing_tie_keeps_plus_quarter_turn() {
        assert_eq!(attack_ground_facing(0, 0, false), 0);
        assert_eq!(
            attack_ground_facing(0, 0, true),
            0x4000_0000,
            "retail uses strict cmovb, so the tie does not select minus"
        );
    }

    #[test]
    fn mutation_reposition_radius_preserves_both_strict_boundaries() {
        // near=4 -> scaled 768, lower threshold 762, near placement 912.
        assert_eq!(attack_ground_reposition_radius(761, 0, 4, 8), 912);
        assert_eq!(attack_ground_reposition_radius(762, 0, 4, 8), 762);
        // far=8 -> scaled 1536, upper threshold 1530, far placement 1488.
        assert_eq!(attack_ground_reposition_radius(1530, 0, 4, 8), 1530);
        assert_eq!(attack_ground_reposition_radius(1531, 0, 4, 8), 1488);
        assert_eq!(
            attack_ground_search(2, 19, 480, 960, 0x2000_0000, 761, 0, 4, 8),
            AttackGroundSearch {
                target_x: 480,
                target_y: 960,
                radius: 912,
                facing_filter: 0xA000_0000,
                filter_index: 3,
                actor_owner: 2,
                actor_object: 19,
            }
        );
    }

    #[test]
    fn order_payload_sizes_and_concrete_offsets_match_pdb() {
        assert_eq!(SIZEOF_ATTACK_GROUND_ORDER, 32);
        assert_eq!(SIZEOF_AIR_ATTACK_GROUND_ORDER, 72);
        assert_eq!(order_offsets::ATTACK_UNIT, 16);
        assert_eq!(order_offsets::AIR_BASE, 20);
        assert_eq!(order_offsets::AIR_RETURNING, 44);
        assert_eq!(order_offsets::TOTAL_TIME, 48);
        assert_eq!(order_offsets::SY, 56);
    }

    #[test]
    fn conditional_world_reads_fail_closed() {
        let mut g = ground();
        g.terrain_owner = HostFact::missing("terrain owner");
        assert_eq!(plan_attack_ground(g), Err(MissingHostFact("terrain owner")));

        let mut a = air();
        a.physics_complete = HostFact::missing("air physics completion");
        assert_eq!(
            plan_air_attack_ground(a),
            Err(MissingHostFact("air physics completion"))
        );
    }

    #[test]
    fn attack_ground_fire_mutations_are_in_retail_order() {
        let p = plan_attack_ground(ground()).unwrap();
        assert_eq!(
            p,
            vec![
                OrderEffect::SetAttack {
                    object: -1,
                    owner: -1
                },
                OrderEffect::OrObjectFlags(OBJECT_ATTACK_GROUND_ACTIVE),
                OrderEffect::SetAngle(0x2000_0000),
                OrderEffect::OrOrderFlags(ORDER_FACING_TARGET),
                OrderEffect::SetAnimation {
                    anim: anim::ATTACK1,
                    b: 0,
                    c: 1
                },
                OrderEffect::FireAmmo {
                    object: -1,
                    owner: -1
                },
                OrderEffect::StoreRecharge(5),
            ]
        );
    }

    #[test]
    fn mutation_attack_ground_forced_unit_one_retires_before_update_action() {
        let mut f = ground();
        f.attack_unit = 1;
        f.target_is_in_range = HostFact::missing("forced unit bypasses range");
        assert_eq!(
            plan_attack_ground(f).unwrap(),
            vec![OrderEffect::KillCurrentOrder(0), OrderEffect::UpdateAction]
        );
    }

    #[test]
    fn attack_ground_special_cast_stops_after_set_attack() {
        let mut f = ground();
        f.redirects_to_special_cast = true;
        assert_eq!(
            plan_attack_ground(f).unwrap(),
            vec![
                OrderEffect::SetAttack {
                    object: -1,
                    owner: -1
                },
                OrderEffect::AddCastOrder {
                    object: -1,
                    owner: -1,
                    x: -1,
                    y: -1,
                    spell: 0x28C,
                    queue: 0,
                    group: 0,
                },
            ]
        );
    }

    #[test]
    fn attack_ground_reposition_requires_second_range_acceptance() {
        let mut f = ground();
        f.target_is_in_range = k(false);
        f.nearby_spot = k(NearbyAttackGroundSpot::Found {
            x: 111,
            y: 222,
            in_range: true,
        });
        assert_eq!(
            plan_attack_ground(f).unwrap(),
            vec![OrderEffect::AddMoveFacing(AttackGroundMove {
                x: 111,
                y: 222,
                facing: 0x2000_0000,
                queue: 1,
                mandatory: 0,
            })]
        );

        let mut f = ground();
        f.target_is_in_range = k(false);
        f.nearby_spot = k(NearbyAttackGroundSpot::Found {
            x: 111,
            y: 222,
            in_range: false,
        });
        f.is_local_player = k(false);
        assert_eq!(
            plan_attack_ground(f).unwrap(),
            vec![OrderEffect::KillCurrentOrder(0)]
        );
    }

    #[test]
    fn air_attack_ground_direct_fire_precedes_recharge() {
        assert_eq!(
            plan_air_attack_ground(air()).unwrap(),
            vec![
                OrderEffect::SetAttack {
                    object: -1,
                    owner: -1
                },
                OrderEffect::FireAmmo {
                    object: -1,
                    owner: -1
                },
                OrderEffect::StoreRecharge(9),
            ]
        );
    }

    #[test]
    fn mutation_air_angle_gates_are_inclusive_at_both_edges() {
        let mut f = air();
        // Put the unsigned subtraction on its direct (non-complemented) side so the
        // mutated value is the actual error under test.  With actor=0,target=N retail's
        // `not(actor-target)` wrap arm produces N-1, which made the old test mutate the
        // angle rather than the recovered error boundary.
        f.target_angle = 0;
        f.actor_angle = AIR_BOMB_ANGLE_TOLERANCE;
        assert!(!plan_air_attack_ground(f.clone()).unwrap().is_empty());

        f.actor_angle = AIR_BOMB_ANGLE_TOLERANCE + 1;
        f.is_jet_fighter = k(false);
        assert!(plan_air_attack_ground(f.clone()).unwrap().is_empty());

        f.is_jet_fighter = k(true);
        f.actor_angle = AIR_BOMB_ANGLE_MAX;
        assert!(!plan_air_attack_ground(f.clone()).unwrap().is_empty());
        f.actor_angle = AIR_BOMB_ANGLE_MAX + 1;
        assert!(plan_air_attack_ground(f).unwrap().is_empty());

        assert_eq!(
            retail_angle_error(0, AIR_BOMB_ANGLE_TOLERANCE + 1),
            AIR_BOMB_ANGLE_TOLERANCE,
            "the complemented wrap arm is deliberately one tick below abs(delta)"
        );
    }

    #[test]
    fn air_missile_bypasses_facing_and_dies_after_release() {
        let mut f = air();
        f.is_missile = true;
        f.target_angle = u32::MAX;
        f.is_jet_fighter = HostFact::missing("missile does not read jet type");
        assert_eq!(
            plan_air_attack_ground(f).unwrap(),
            vec![
                OrderEffect::SetAttack {
                    object: -1,
                    owner: -1
                },
                OrderEffect::FireAmmo {
                    object: -1,
                    owner: -1
                },
                OrderEffect::Die,
            ]
        );
    }

    #[test]
    fn bomber_animates_then_mutates_recharge_and_fuel() {
        let mut f = air();
        f.is_bomber = k(true);
        assert_eq!(
            plan_air_attack_ground(f).unwrap(),
            vec![
                OrderEffect::SetAttack {
                    object: -1,
                    owner: -1
                },
                OrderEffect::SetAnimation {
                    anim: anim::ATTACK2,
                    b: 0,
                    c: 1
                },
                OrderEffect::StoreRecharge(9),
                OrderEffect::AddManaBurn(13),
            ]
        );
    }
}
