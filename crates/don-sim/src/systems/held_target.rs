//! Retail held-target range and pursuit boundary.
//!
//! This module is the measured, ordinary-order seam between automatic target acquisition
//! ([`super::target`]) and the direct volley geometry ([`super::fight`]).  It ports the parts
//! of `Unit::fight` `0x005FD4D0` and `ObjectData::{attack_dist,is_in_range}`
//! `0x006488F0`/`0x006486B0` which can be evaluated without an object hierarchy, terrain
//! search, collision world, or order queue.
//!
//! The important correction over a centre-distance or target-radius approximation is that
//! retail:
//!
//! 1. snaps **both** anchors to 48-world-unit cell centres;
//! 2. subtracts the target footprint independently from the x and y legs;
//! 3. subtracts the attacker's footprint independently from the remaining x and y legs;
//! 4. only then calls the integer `vector_dist` metric.
//!
//! `Unit::find_attack_pos` `0x00601280` is not a pure "walk toward the target" calculation.
//! Its unit arm calls `UnitType::find_nearby_spot`; its building arm searches a perimeter,
//! checks terrain and ordered collisions, and consumes the game RNG while scoring candidates.
//! [`HeldTargetStep::FindAttackPosition`] therefore exposes an exact, fail-closed request.  A
//! host must satisfy it with the retail-equivalent spatial services; treating the target anchor
//! or the reflected "back-off" point as the answer is deliberately outside this API.

use super::movement::{ucell_centre, ucell_of, vector_dist};
use super::target::ObjRef;

/// `ObjectData::attack_dist` footprint facts resolved from the object/type virtuals.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectFootprint {
    /// Non-building: `ObjectTypeData::block_radius + 0x18`, at type offset `+0x240`.
    Unit { block_radius: i32 },
    /// Building: `x_size * 0x60` and `y_size * 0x60`, at type offsets
    /// `+0x234/+0x238`.  The two axes are not collapsed to `max(x_size, y_size)`.
    Building { x_size: i32, y_size: i32 },
}

impl ObjectFootprint {
    #[inline]
    fn extents(self) -> (i32, i32) {
        match self {
            Self::Unit { block_radius } => {
                let radius = block_radius.wrapping_add(0x18);
                (radius, radius)
            }
            Self::Building { x_size, y_size } => {
                (x_size.wrapping_mul(0x60), y_size.wrapping_mul(0x60))
            }
        }
    }

    /// `ObjectData` unit virtual `+0x18` selects the `+0x244` contribution in
    /// `is_in_range`'s minimum-range rescue.
    #[inline]
    pub const fn is_unit(self) -> bool {
        matches!(self, Self::Unit { .. })
    }
}

/// The one early branch in `ObjectData::attack_dist` which skips both footprints.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttackDistanceMode {
    /// The ordinary object path: subtract target and attacker extents before `vector_dist`.
    Footprints,
    /// `UnitData::is_plane` returned non-zero while attacker type `obj_masks` (`+0x1E4`)
    /// did not contain `0x0800_0000`.
    PlaneWithoutObjmask08000000,
}

impl AttackDistanceMode {
    /// Resolve `ObjectData::attack_dist`'s early footprint-bypass gate for a `Unit`.
    ///
    /// The shipped `Unit` vtable at `0x00B417D0` has `UnitData::is_plane` `0x0046CE40` in
    /// slot `+0xC0`.  Its complete body is:
    ///
    /// ```text
    /// return type.domain(+0x218) == 2 && !(type.unit_flags(+0x2B4) & 0x20);
    /// ```
    ///
    /// `attack_dist` bypasses both footprints only when that result is true and type
    /// `obj_masks(+0x1E4) & 0x08000000` is clear.  A direct-land unit (`domain == 0`) is
    /// therefore measured to use [`Self::Footprints`]; this is not a host default.
    #[inline]
    pub const fn for_unit_type(domain: i32, unit_flags: u32, obj_masks: u32) -> Self {
        let is_plane = domain == 2 && unit_flags & 0x20 == 0;
        if is_plane && obj_masks & 0x0800_0000 == 0 {
            Self::PlaneWithoutObjmask08000000
        } else {
            Self::Footprints
        }
    }
}

/// Fully resolved inputs to `ObjectData::attack_dist(o, who, x, y)` `0x006488F0`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AttackDistanceInput {
    pub attacker_x: i32,
    pub attacker_y: i32,
    pub target_x: i32,
    pub target_y: i32,
    pub attacker: ObjectFootprint,
    pub target: ObjectFootprint,
    pub mode: AttackDistanceMode,
}

#[inline]
fn snap_anchor(v: i32) -> i32 {
    // `DAT_00CAE5FC[(v >> 4)] * 0x30 + 0x18`.  movement's conversion uses the
    // measured C-truncating div-three table semantics.
    ucell_centre(ucell_of(v))
}

#[inline]
fn subtract_extent(delta: i32, extent: i32) -> i32 {
    // Retail uses a strict signed comparison: equality collapses the leg to zero.
    if extent < delta {
        delta.wrapping_sub(extent)
    } else {
        0
    }
}

/// Exact `ObjectData::attack_dist` integer distance for resolved ordinary objects.
#[inline]
pub fn attack_distance(i: AttackDistanceInput) -> i32 {
    let mut dx = snap_anchor(i.attacker_x)
        .wrapping_sub(snap_anchor(i.target_x))
        .wrapping_abs();
    let mut dy = snap_anchor(i.attacker_y)
        .wrapping_sub(snap_anchor(i.target_y))
        .wrapping_abs();

    if i.mode == AttackDistanceMode::PlaneWithoutObjmask08000000 {
        return vector_dist(dx, dy);
    }

    let (tx, ty) = i.target.extents();
    dx = subtract_extent(dx, tx);
    dy = subtract_extent(dy, ty);
    let (ax, ay) = i.attacker.extents();
    dx = subtract_extent(dx, ax);
    dy = subtract_extent(dy, ay);
    vector_dist(dx, dy)
}

/// The target-location precondition dispatched by `ObjectData::is_in_range`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetLocationGate {
    /// Target virtual `+0x08` returned zero, so the secondary location virtual is skipped.
    NotRequired,
    /// Target virtual `+0x08` returned non-zero; the target data virtual at `+0xBC` (or its
    /// cached high-bit fast path) must also return non-zero.
    Required { location_virtual_nonzero: bool },
}

/// The two reach formulae selected by attacker type field `+0x1FC`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReachProfile {
    /// Attacker type `+0x1FC == 0`.  `has_objmask(0x84)` selects a hard inclusive reach of
    /// `0xF6`; otherwise it is `0x66`.
    Fixed { has_objmask_0x84: bool },
    /// Attacker type `+0x1FC != 0`.  Range fields are virtual `+0x12C/+0x130`, in tiles.
    /// `big_radius` is type offset `+0x244` and contributes only for unit objects.
    MinMax {
        min_range_tiles: i32,
        max_range_tiles: i32,
        attacker_big_radius: Option<i32>,
        target_big_radius: Option<i32>,
        /// Raw `is_in_range` parameter 6.  When non-zero retail adds `0x90` to the measured
        /// distance before the maximum-range comparison.  Calling it a bonus would be
        /// misleading: it makes this particular check stricter.
        param_6_nonzero: bool,
    },
}

/// Resolved inputs to `ObjectData::is_in_range` `0x006486B0`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IsInRangeInput {
    /// Target `ObjectData +0x08` bit 0.
    pub target_active: bool,
    pub target_location: TargetLocationGate,
    /// The terrain word at the attacker's tile.  `(word & 0x30) == 0x30` rejects the shot.
    pub attacker_terrain_word: u16,
    pub distance: AttackDistanceInput,
    pub reach: ReachProfile,
}

/// Why `ObjectData::is_in_range` returned zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutOfRangeReason {
    TargetInactive,
    TargetLocation,
    AttackerTerrain,
    FixedReach,
    BelowMinimum,
    AboveMaximum,
}

/// Exact result including the optional out-distance write.
///
/// Retail does not write the out pointer when one of the three pre-distance gates rejects,
/// hence `distance: None` in that case.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InRangeResult {
    InRange {
        distance: i32,
    },
    OutOfRange {
        distance: Option<i32>,
        reason: OutOfRangeReason,
    },
}

impl InRangeResult {
    #[inline]
    pub const fn distance(self) -> Option<i32> {
        match self {
            Self::InRange { distance } => Some(distance),
            Self::OutOfRange { distance, .. } => distance,
        }
    }

    #[inline]
    pub const fn is_in_range(self) -> bool {
        matches!(self, Self::InRange { .. })
    }
}

/// Exact resolved `ObjectData::is_in_range` decision.
pub fn is_in_range(i: IsInRangeInput) -> InRangeResult {
    if !i.target_active {
        return InRangeResult::OutOfRange {
            distance: None,
            reason: OutOfRangeReason::TargetInactive,
        };
    }
    if matches!(
        i.target_location,
        TargetLocationGate::Required {
            location_virtual_nonzero: false
        }
    ) {
        return InRangeResult::OutOfRange {
            distance: None,
            reason: OutOfRangeReason::TargetLocation,
        };
    }
    if i.attacker_terrain_word & 0x30 == 0x30 {
        return InRangeResult::OutOfRange {
            distance: None,
            reason: OutOfRangeReason::AttackerTerrain,
        };
    }

    let distance = attack_distance(i.distance);
    match i.reach {
        ReachProfile::Fixed { has_objmask_0x84 } => {
            let limit = if has_objmask_0x84 { 0xF6 } else { 0x66 };
            if distance > limit {
                InRangeResult::OutOfRange {
                    distance: Some(distance),
                    reason: OutOfRangeReason::FixedReach,
                }
            } else {
                InRangeResult::InRange { distance }
            }
        }
        ReachProfile::MinMax {
            min_range_tiles,
            max_range_tiles,
            attacker_big_radius,
            target_big_radius,
            param_6_nonzero,
        } => {
            let minimum = min_range_tiles.wrapping_mul(0xC0).wrapping_sub(6);
            if distance < minimum {
                let radii = attacker_big_radius
                    .unwrap_or(0)
                    .wrapping_add(target_big_radius.unwrap_or(0));
                if radii.wrapping_add(distance) < minimum {
                    return InRangeResult::OutOfRange {
                        distance: Some(distance),
                        reason: OutOfRangeReason::BelowMinimum,
                    };
                }
            }

            let biased_distance = distance.wrapping_add(if param_6_nonzero { 0x90 } else { 0 });
            let maximum = max_range_tiles.wrapping_mul(0xC0).wrapping_add(6);
            if maximum < biased_distance {
                InRangeResult::OutOfRange {
                    distance: Some(distance),
                    reason: OutOfRangeReason::AboveMaximum,
                }
            } else {
                InRangeResult::InRange { distance }
            }
        }
    }
}

/// Target binding state established before `Unit::fight`'s ordinary range slice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeldTargetBinding {
    /// The order target no longer resolves or is inactive. `Unit::do_attack` retires it.
    MissingOrInactive,
    /// The resolved object's `uid` differs from the uid captured in the attack order.
    UidMismatch,
    /// `Object::valid_target` rejected the resolved target.  `Unit::fight` has several
    /// mode-, duty-, and retarget-dependent arms; this pure seam cannot choose among them.
    RejectedByValidTarget,
    Bound,
}

/// Exact failure retirement reason exposed to an order host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeldTargetRetirement {
    MissingOrInactive,
    UidMismatch,
    /// `Unit::fight 0x005FE05B..0x005FE0E5`: this attack order had previously been in range,
    /// type `+0x2B8 & 4` is set, and `unit_masks & 0x80000` is clear.
    LostRangeAfterContact,
}

/// Why this module refuses to choose a retail action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeldTargetBoundary {
    InvalidTargetRetargetFlow,
    ContradictoryBoundTarget,
}

/// The exact ordinary wrapper request made by `Unit::fight` at `0x005FE617`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FindAttackPositionRequest {
    pub target: ObjRef,
    pub attacker_x: i32,
    pub attacker_y: i32,
    pub target_x: i32,
    pub target_y: i32,
    pub target_is_building: bool,
    pub range_result: InRangeResult,
    /// Third argument of `Unit::find_attack_pos`'s six-argument wrapper; zero on this path.
    pub wrapper_param_3: i32,
    /// Sixth argument of the wrapper; zero on this path.
    pub wrapper_param_6: i32,
}

/// Inputs to the ordinary held-target slice of `Unit::fight`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeldTargetInput {
    pub target: ObjRef,
    pub binding: HeldTargetBinding,
    pub range: IsInRangeInput,
    /// Attack-order byte at `+0x1F`: retail sets it after any in-range observation.
    pub order_ever_in_range: bool,
    /// Attacker type `+0x2B8 & 4`.
    pub retire_on_lost_range_type_flag: bool,
    /// `UnitData +0x68`.
    pub unit_masks: u32,
    pub target_is_building: bool,
}

/// One exact held-target decision.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeldTargetStep {
    Retire(HeldTargetRetirement),
    /// Target is in range.  The host may run the already-ported recharge and volley pipeline;
    /// it must persist `mark_order_ever_in_range` to attack-order byte `+0x1F`.
    Engage {
        distance: i32,
        mark_order_ever_in_range: bool,
    },
    /// Invoke retail `Unit::find_attack_pos`.  This is intentionally not a destination.
    FindAttackPosition(FindAttackPositionRequest),
    FailClosed(HeldTargetBoundary),
}

/// Plan the ordinary bound-target range/pursuit slice of `Unit::fight`.
pub fn plan_held_target(i: HeldTargetInput) -> HeldTargetStep {
    match i.binding {
        HeldTargetBinding::MissingOrInactive => {
            return HeldTargetStep::Retire(HeldTargetRetirement::MissingOrInactive);
        }
        HeldTargetBinding::UidMismatch => {
            return HeldTargetStep::Retire(HeldTargetRetirement::UidMismatch);
        }
        HeldTargetBinding::RejectedByValidTarget => {
            return HeldTargetStep::FailClosed(HeldTargetBoundary::InvalidTargetRetargetFlow);
        }
        HeldTargetBinding::Bound => {}
    }

    // A Bound target has passed the active-object check before fight.  Do not silently turn
    // contradictory adapter facts into pursuit.
    if !i.range.target_active {
        return HeldTargetStep::FailClosed(HeldTargetBoundary::ContradictoryBoundTarget);
    }

    let range_result = is_in_range(i.range);
    if let InRangeResult::InRange { distance } = range_result {
        return HeldTargetStep::Engage {
            distance,
            mark_order_ever_in_range: true,
        };
    }

    if i.retire_on_lost_range_type_flag && i.unit_masks & 0x0008_0000 == 0 && i.order_ever_in_range
    {
        return HeldTargetStep::Retire(HeldTargetRetirement::LostRangeAfterContact);
    }

    HeldTargetStep::FindAttackPosition(FindAttackPositionRequest {
        target: i.target,
        attacker_x: i.range.distance.attacker_x,
        attacker_y: i.range.distance.attacker_y,
        target_x: i.range.distance.target_x,
        target_y: i.range.distance.target_y,
        target_is_building: i.target_is_building,
        range_result,
        wrapper_param_3: 0,
        wrapper_param_6: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn distance_at(distance: i32) -> AttackDistanceInput {
        AttackDistanceInput {
            attacker_x: 24,
            attacker_y: 24,
            target_x: 24 + distance,
            target_y: 24,
            attacker: ObjectFootprint::Unit { block_radius: 0 },
            target: ObjectFootprint::Unit { block_radius: 0 },
            mode: AttackDistanceMode::PlaneWithoutObjmask08000000,
        }
    }

    /// Construct an ordinary-footprint case whose snapped residual x leg is exactly `distance`.
    fn exact_footprint_distance(distance: i32) -> AttackDistanceInput {
        let snapped_leg = (distance.wrapping_add(95) / 48) * 48;
        let target_block_radius = snapped_leg.wrapping_sub(distance).wrapping_sub(48);
        assert!(target_block_radius >= 0);
        AttackDistanceInput {
            attacker_x: 24,
            attacker_y: 24,
            target_x: 24 + snapped_leg,
            target_y: 24,
            attacker: ObjectFootprint::Unit { block_radius: 0 },
            target: ObjectFootprint::Unit {
                block_radius: target_block_radius,
            },
            mode: AttackDistanceMode::Footprints,
        }
    }

    fn ranged(distance: i32, min: i32, max: i32) -> IsInRangeInput {
        IsInRangeInput {
            target_active: true,
            target_location: TargetLocationGate::NotRequired,
            attacker_terrain_word: 0,
            distance: distance_at(distance),
            reach: ReachProfile::MinMax {
                min_range_tiles: min,
                max_range_tiles: max,
                attacker_big_radius: Some(0),
                target_big_radius: Some(0),
                param_6_nonzero: false,
            },
        }
    }

    #[test]
    fn attack_distance_snaps_and_subtracts_both_unit_footprints_per_axis() {
        let i = AttackDistanceInput {
            attacker_x: 49, // snaps to 72
            attacker_y: 49,
            target_x: 1_000, // snaps to 984
            target_y: 49,
            attacker: ObjectFootprint::Unit { block_radius: 48 },
            target: ObjectFootprint::Unit { block_radius: 48 },
            mode: AttackDistanceMode::Footprints,
        };
        // 984 - 72 - (48 + 24) - (48 + 24)
        assert_eq!(attack_distance(i), 768);
    }

    #[test]
    fn rectangular_building_extent_is_not_collapsed_to_one_scalar() {
        let i = AttackDistanceInput {
            attacker_x: 24,
            attacker_y: 24,
            target_x: 984,
            target_y: 984,
            attacker: ObjectFootprint::Unit { block_radius: 48 },
            target: ObjectFootprint::Building {
                x_size: 2,
                y_size: 4,
            },
            mode: AttackDistanceMode::Footprints,
        };
        // Residual legs: (960 - 192 - 72, 960 - 384 - 72) = (696, 504).
        assert_eq!(attack_distance(i), vector_dist(696, 504));
        assert_eq!(attack_distance(i), 878);
    }

    #[test]
    fn is_plane_without_objmask_branch_skips_both_footprints() {
        let mut i = AttackDistanceInput {
            attacker_x: 24,
            attacker_y: 24,
            target_x: 984,
            target_y: 984,
            attacker: ObjectFootprint::Building {
                x_size: 100,
                y_size: 100,
            },
            target: ObjectFootprint::Building {
                x_size: 100,
                y_size: 100,
            },
            mode: AttackDistanceMode::Footprints,
        };
        assert_eq!(attack_distance(i), 0);
        i.mode = AttackDistanceMode::PlaneWithoutObjmask08000000;
        assert_eq!(attack_distance(i), vector_dist(960, 960));
    }

    #[test]
    fn unit_type_fields_resolve_the_is_plane_footprint_bypass() {
        assert_eq!(
            AttackDistanceMode::for_unit_type(0, 0, 0),
            AttackDistanceMode::Footprints,
            "every direct-land Unit fails UnitData::is_plane"
        );
        assert_eq!(
            AttackDistanceMode::for_unit_type(2, 0, 0),
            AttackDistanceMode::PlaneWithoutObjmask08000000
        );
        assert_eq!(
            AttackDistanceMode::for_unit_type(2, 0x20, 0),
            AttackDistanceMode::Footprints,
            "the +0x2B4 bit is part of UnitData::is_plane"
        );
        assert_eq!(
            AttackDistanceMode::for_unit_type(2, 0, 0x0800_0000),
            AttackDistanceMode::Footprints,
            "the +0x1E4 objmask cancels the bypass"
        );
    }

    #[test]
    fn fixed_reach_limits_are_inclusive() {
        let mut i = ranged(96, 0, 0);
        i.reach = ReachProfile::Fixed {
            has_objmask_0x84: false,
        };
        i.distance = exact_footprint_distance(0x66);
        assert_eq!(is_in_range(i), InRangeResult::InRange { distance: 0x66 });

        i.distance = exact_footprint_distance(0x67);
        assert_eq!(
            is_in_range(i),
            InRangeResult::OutOfRange {
                distance: Some(0x67),
                reason: OutOfRangeReason::FixedReach,
            }
        );

        i.reach = ReachProfile::Fixed {
            has_objmask_0x84: true,
        };
        i.distance = exact_footprint_distance(0xF6);
        assert_eq!(is_in_range(i), InRangeResult::InRange { distance: 0xF6 });
        i.distance = exact_footprint_distance(0xF7);
        assert_eq!(
            is_in_range(i),
            InRangeResult::OutOfRange {
                distance: Some(0xF7),
                reason: OutOfRangeReason::FixedReach,
            }
        );
    }

    #[test]
    fn min_range_uses_minus_six_and_unit_big_radius_rescue() {
        let mut i = ranged(288, 2, 10);
        i.reach = ReachProfile::MinMax {
            min_range_tiles: 2,
            max_range_tiles: 10,
            attacker_big_radius: Some(45),
            target_big_radius: Some(45),
            param_6_nonzero: false,
        };
        // 2*192-6 = 378; 288+90 reaches that boundary exactly.
        assert_eq!(is_in_range(i), InRangeResult::InRange { distance: 288 });

        if let ReachProfile::MinMax {
            ref mut target_big_radius,
            ..
        } = i.reach
        {
            *target_big_radius = Some(44);
        }
        assert_eq!(
            is_in_range(i),
            InRangeResult::OutOfRange {
                distance: Some(288),
                reason: OutOfRangeReason::BelowMinimum,
            }
        );
    }

    #[test]
    fn max_range_uses_plus_six_and_raw_param6_bias() {
        let mut i = ranged(576, 0, 3);
        i.distance = exact_footprint_distance(3 * 192 + 6);
        assert_eq!(
            is_in_range(i),
            InRangeResult::InRange {
                distance: 3 * 192 + 6
            }
        );
        i.distance = exact_footprint_distance(3 * 192 + 7);
        assert_eq!(
            is_in_range(i),
            InRangeResult::OutOfRange {
                distance: Some(3 * 192 + 7),
                reason: OutOfRangeReason::AboveMaximum,
            }
        );

        let mut biased = ranged(432, 0, 3);
        if let ReachProfile::MinMax {
            ref mut param_6_nonzero,
            ..
        } = biased.reach
        {
            *param_6_nonzero = true;
        }
        // 432+144 == 3*192; still inside the +6 inclusive edge.
        assert_eq!(
            is_in_range(biased),
            InRangeResult::InRange { distance: 432 }
        );
        biased.distance = distance_at(480);
        assert_eq!(
            is_in_range(biased),
            InRangeResult::OutOfRange {
                distance: Some(480),
                reason: OutOfRangeReason::AboveMaximum,
            }
        );
    }

    #[test]
    fn pre_distance_rejections_do_not_publish_a_distance() {
        let mut i = ranged(96, 0, 3);
        i.attacker_terrain_word = 0x30;
        assert_eq!(
            is_in_range(i),
            InRangeResult::OutOfRange {
                distance: None,
                reason: OutOfRangeReason::AttackerTerrain,
            }
        );
    }

    fn held(range: IsInRangeInput) -> HeldTargetInput {
        HeldTargetInput {
            target: ObjRef::new(7, 1),
            binding: HeldTargetBinding::Bound,
            range,
            order_ever_in_range: false,
            retire_on_lost_range_type_flag: false,
            unit_masks: 0,
            target_is_building: false,
        }
    }

    #[test]
    fn in_range_marks_order_contact_for_later_lost_range_gate() {
        assert_eq!(
            plan_held_target(held(ranged(192, 0, 3))),
            HeldTargetStep::Engage {
                distance: 192,
                mark_order_ever_in_range: true,
            }
        );
    }

    #[test]
    fn prior_contact_type_flag_retires_only_when_mask_80000_is_clear() {
        let mut i = held(ranged(960, 0, 3));
        i.order_ever_in_range = true;
        i.retire_on_lost_range_type_flag = true;
        assert_eq!(
            plan_held_target(i),
            HeldTargetStep::Retire(HeldTargetRetirement::LostRangeAfterContact)
        );

        i.unit_masks = 0x0008_0000;
        assert!(matches!(
            plan_held_target(i),
            HeldTargetStep::FindAttackPosition(_)
        ));
    }

    #[test]
    fn out_of_range_emits_exact_wrapper_request_not_a_guessed_destination() {
        let i = held(ranged(960, 0, 3));
        let HeldTargetStep::FindAttackPosition(r) = plan_held_target(i) else {
            panic!("expected find_attack_pos request");
        };
        assert_eq!(r.target, ObjRef::new(7, 1));
        assert_eq!((r.wrapper_param_3, r.wrapper_param_6), (0, 0));
        assert_eq!(r.range_result.distance(), Some(960));
    }

    #[test]
    fn invalid_target_flow_fails_closed_but_stale_binding_retires() {
        let mut i = held(ranged(192, 0, 3));
        i.binding = HeldTargetBinding::RejectedByValidTarget;
        assert_eq!(
            plan_held_target(i),
            HeldTargetStep::FailClosed(HeldTargetBoundary::InvalidTargetRetargetFlow)
        );
        i.binding = HeldTargetBinding::UidMismatch;
        assert_eq!(
            plan_held_target(i),
            HeldTargetStep::Retire(HeldTargetRetirement::UidMismatch)
        );
    }
}
