//! Direct land-unit volley geometry from `Unit::fight` `0x005FD4D0`.
//!
//! This is the measured unit-versus-unit firing arm that joins the persistent squad state
//! in [`super::groups_guys`] to the attack-direction and flank helpers in [`super::target`].
//! It deliberately is not a small replacement combat loop: target acquisition, range,
//! projectile creation, splash, retaliation, damage and death remain with their owning
//! systems.  The output here is the exact state `Unit::fight` prepares immediately before
//! its loop of `Object::do_damage` calls.
//!
//! The relevant retail sequence is:
//!
//! * `0x005FE872..0x005FE8A4`: `find_angle(target - attacker)` produces the direction the
//!   attack travels.
//! * `0x005FEBC0..0x005FEBCB`: a body-tracking unit writes that direction to
//!   `UnitData +0x50 angle` through `UnitData::set_angle`.
//! * `0x005FEBD0..0x005FECA3`: a land squad with more than one soldier and no crew aims
//!   each live Guy from its own `des_x/des_y`; a one-soldier squad or a squad with crew
//!   writes the Unit angle to every live squad Guy and every crew Guy.
//! * `0x005FEE4D..0x005FEE8B`: `Object::do_damage` runs once for every live squad Guy,
//!   bounded by `UnitData +0xB5 guy_mark`. Crew never produce a hit.
//! * `0x005FF094..0x005FF0A4`: recharge is computed and stored once after the whole volley,
//!   as an unsigned byte. [`super::combat::AttackCycle`] owns that byte transaction.
//!
//! ## Graphics turrets are a mandatory boundary
//!
//! `Unit::target_guy` `0x005FCE70` calls the PDB-named `Guy::set_all_pivots`
//! `0x005D8BC0`. A Guy with raw `guy_flags & 0x0100` can aim graphics turret nodes while
//! the Unit body keeps its current facing; in that arm `Unit::fight` passes the retained
//! body facing as `attack_dir` to `Object::do_damage`.
//! [`super::graphics_turret::resolve_turret_aim`] evaluates the exact restriction/angle arm
//! when a loaded-hierarchy provider is available. The caller must pass its measured result
//! as [`AimMode::GraphicsTurret`]. [`AimMode::UnresolvedGraphicsTurret`] fails closed; it
//! never silently substitutes the target bearing.

use super::groups_guys::{UnitGuys, GUY_FLAG_TURRETS};
use super::target::{attack_dir, flank_tier};

/// How `Unit::target_guy` / `Guy::set_all_pivots` resolved body aim this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AimMode {
    /// No graphics turret arm retained the body facing. The Unit turns to the target.
    BodyTracksTarget,
    /// A graphics-turret Guy was evaluated with its exact graphics graph.
    ///
    /// `aligned == true` is the non-zero return from `Guy::set_all_pivots`: turret nodes may
    /// fire while the Unit keeps its current facing. `false` falls through to body tracking.
    GraphicsTurret { aligned: bool },
    /// Raw Guy state advertises graphics turrets, but the host has not evaluated their
    /// graphics node/animation graph. Planning a volley must return an error.
    UnresolvedGraphicsTurret,
}

impl AimMode {
    /// Conservative mode from the checksum-visible Guy state.
    ///
    /// A squad without `GUY_FLAG_TURRETS` takes the measured body-tracking arm. Any live
    /// squad or crew Guy carrying the flag requires the graphics resolver; absence of that
    /// resolver is represented, not guessed around.
    pub fn from_guys(guys: &UnitGuys) -> Self {
        if guys
            .guys
            .iter()
            .flatten()
            .any(|g| g.guy_flags & GUY_FLAG_TURRETS != 0)
        {
            AimMode::UnresolvedGraphicsTurret
        } else {
            AimMode::BodyTracksTarget
        }
    }
}

/// The persistent geometry and squad state read by the direct land firing arm.
#[derive(Clone, Copy, Debug)]
pub struct UnitVolleyInput<'a> {
    /// Attacker `ObjectData +0x10/+0x14`, unmasked world coordinates.
    pub attacker_x: i32,
    pub attacker_y: i32,
    /// Attacker `UnitData +0x50 angle` before this volley.
    pub attacker_facing: i32,
    /// Target `ObjectData +0x10/+0x14`, unmasked world coordinates.
    pub target_x: i32,
    pub target_y: i32,
    /// Defender `UnitData +0x50 angle`, the exact damage-chain flank operand.
    pub defender_facing: i32,
    /// Attacker `UnitTypeData +0x304 squad_size`.
    pub squad_size: i32,
    /// Attacker `UnitData +0xE4 guys` and `+0xB5 guy_mark`.
    pub guys: &'a UnitGuys,
    /// Result of the graphics/body aim arm.
    pub aim_mode: AimMode,
}

/// One `GuyData +0x64 des_angle` write made by `Unit::fight`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GuyAim {
    /// Index in `UnitData::guys`.
    pub slot: usize,
    /// Binary angle written to `GuyData +0x64`.
    pub des_angle: i32,
}

/// Exact pre-damage result for one ready direct land-unit volley.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnitVolleyPlan {
    /// Value written to attacker `UnitData +0x50 angle` (or retained for an aligned turret).
    pub unit_facing: i32,
    /// Whether `UnitData::set_angle` toggles `UnitData +0x68 & 2` for a turn of at least
    /// one quarter and at most three quarters. Aligned turrets retain the facing and never
    /// call the setter.
    pub toggle_unit_mask_2: bool,
    /// Third `Object::do_damage` argument. Normally target travel direction; for the
    /// measured aligned-turret arm it is the retained Unit facing.
    pub damage_attack_dir: i32,
    /// Composed caller-guarded flank tier from the defender's Unit angle.
    pub flank_tier: u32,
    /// Final desired-angle writes for live squad and crew Guys.
    pub guy_aims: Vec<GuyAim>,
    /// Number of `Object::do_damage` calls. Exactly the live squad prefix (`guy_mark`).
    pub shot_count: usize,
}

/// A state that retail's direct land-unit arm cannot execute safely or that requires an
/// unported mandatory dependency.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VolleyPlanError {
    /// This adapter is intentionally only the land unit-versus-unit arm.
    UnsupportedDomain(i32),
    /// Negative type data cannot name a retail squad.
    InvalidSquadSize(i32),
    /// `guy_mark` must be a live prefix inside `squad_size`.
    InvalidGuyMark { guy_mark: i8, squad_size: i32 },
    /// Retail dereferences this pointer unconditionally in the measured arm.
    MissingGuy { slot: usize },
    /// A graphics-turret Guy exists, but its animation/node graph was not evaluated.
    UnresolvedGraphicsTurret,
}

/// Plan the exact direct land unit-versus-unit volley arm of `Unit::fight`.
///
/// `attacker_domain` is passed separately because it lives in `UnitTypeData +0x218`, not
/// in [`UnitVolleyInput`]'s squad state. Only domain 0 is accepted: sea has a measured
/// broadside-facing branch and air has separate projectile/aircraft behavior. Failing those
/// domains is preferable to laundering the land arm into a general combat claim.
pub fn plan_direct_land_volley(
    attacker_domain: i32,
    i: &UnitVolleyInput<'_>,
) -> Result<UnitVolleyPlan, VolleyPlanError> {
    if attacker_domain != 0 {
        return Err(VolleyPlanError::UnsupportedDomain(attacker_domain));
    }
    if i.squad_size < 0 {
        return Err(VolleyPlanError::InvalidSquadSize(i.squad_size));
    }
    let live = i.guys.guy_mark as i32;
    if live < 0 || live > i.squad_size || live as usize > i.guys.guys.len() {
        return Err(VolleyPlanError::InvalidGuyMark {
            guy_mark: i.guys.guy_mark,
            squad_size: i.squad_size,
        });
    }
    let travel = attack_dir(i.attacker_x, i.attacker_y, i.target_x, i.target_y);
    let body_tracks_target = match i.aim_mode {
        AimMode::BodyTracksTarget | AimMode::GraphicsTurret { aligned: false } => true,
        AimMode::GraphicsTurret { aligned: true } => false,
        AimMode::UnresolvedGraphicsTurret => {
            return Err(VolleyPlanError::UnresolvedGraphicsTurret);
        }
    };
    let damage_attack_dir = match i.aim_mode {
        AimMode::BodyTracksTarget | AimMode::GraphicsTurret { aligned: false } => travel,
        AimMode::GraphicsTurret { aligned: true } => i.attacker_facing,
        AimMode::UnresolvedGraphicsTurret => unreachable!("handled above"),
    };
    let turn_delta = (damage_attack_dir as u32).wrapping_sub(i.attacker_facing as u32);
    let toggle_unit_mask_2 =
        body_tracks_target && (0x4000_0000..=0xC000_0000).contains(&turn_delta);

    // PtrArray::length > squad_size means crew exists. This is the exact unsigned-looking
    // `cmp [unit+0xE8], type.squad_size; jg` at 0x005FEBD9, after valid type data has made
    // both values non-negative.
    let shared_aim = i.guys.guys.len() > i.squad_size as usize || i.squad_size == 1;
    let mut guy_aims =
        Vec::with_capacity(live as usize + i.guys.guys.len().saturating_sub(i.squad_size as usize));

    for slot in 0..live as usize {
        let Some(g) = i.guys.guys.get(slot).and_then(Option::as_ref) else {
            return Err(VolleyPlanError::MissingGuy { slot });
        };
        let des_angle = if shared_aim {
            damage_attack_dir
        } else {
            // 0x005FEC18..0x005FEC30 reads GuyData.des_x/des_y, not current x/y.
            attack_dir(g.des_x, g.des_y, i.target_x, i.target_y)
        };
        guy_aims.push(GuyAim { slot, des_angle });
    }

    // UnitData::set_angle propagates into crew, then Unit::fight writes the same final
    // attack angle to every crew slot at 0x005FEC78..0x005FECA3.
    for slot in i.squad_size as usize..i.guys.guys.len() {
        if i.guys.guys[slot].is_none() {
            return Err(VolleyPlanError::MissingGuy { slot });
        }
        guy_aims.push(GuyAim {
            slot,
            des_angle: damage_attack_dir,
        });
    }

    Ok(UnitVolleyPlan {
        unit_facing: damage_attack_dir,
        toggle_unit_mask_2,
        damage_attack_dir,
        flank_tier: flank_tier(i.defender_facing, damage_attack_dir),
        guy_aims,
        shot_count: live as usize,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::systems::groups_guys::{GuyData, UnitGuys};
    use crate::trig::{HALF_TURN, QUARTER_TURN};

    fn guys(squad: usize, crew: usize, live: usize) -> UnitGuys {
        let mut u = UnitGuys {
            guy_mark: live as i8,
            ..Default::default()
        };
        u.guys.resize(squad + crew, None);
        for slot in 0..live {
            u.guys[slot] = Some(GuyData {
                guy_num: slot as i8,
                ..Default::default()
            });
        }
        for slot in squad..squad + crew {
            u.guys[slot] = Some(GuyData {
                guy_num: slot as i8,
                ..Default::default()
            });
        }
        u
    }

    fn input<'a>(u: &'a UnitGuys, squad_size: i32) -> UnitVolleyInput<'a> {
        UnitVolleyInput {
            attacker_x: 100,
            attacker_y: 100,
            attacker_facing: 0x5555_5555,
            target_x: 300,
            target_y: 100,
            defender_facing: 0,
            squad_size,
            guys: u,
            aim_mode: AimMode::BodyTracksTarget,
        }
    }

    #[test]
    fn direction_uses_retail_find_angle_not_an_eight_way_lattice() {
        let u = guys(1, 0, 1);
        let p = plan_direct_land_volley(0, &input(&u, 1)).unwrap();
        assert_eq!(
            p.damage_attack_dir, QUARTER_TURN,
            "east is one quarter turn"
        );
        assert_eq!(p.unit_facing, QUARTER_TURN);
        assert!(
            !p.toggle_unit_mask_2,
            "the initial 120-degree facing is nearby"
        );
        assert_eq!(p.guy_aims[0].des_angle, QUARTER_TURN);
    }

    #[test]
    fn body_turn_toggles_mask_bit_two_on_the_measured_quarter_turn_boundary() {
        let u = guys(1, 0, 1);
        let mut i = input(&u, 1);
        i.attacker_facing = HALF_TURN;
        let p = plan_direct_land_volley(0, &i).unwrap();
        assert_eq!(p.damage_attack_dir, QUARTER_TURN);
        assert!(
            p.toggle_unit_mask_2,
            "three-quarter wrapping delta is inclusive"
        );
    }

    #[test]
    fn no_crew_multi_squad_aims_from_each_guy_destination() {
        let mut u = guys(3, 0, 3);
        let positions = [(100, 0), (0, 100), (100, 200)];
        for (g, (x, y)) in u.guys.iter_mut().flatten().zip(positions) {
            g.des_x = x;
            g.des_y = y;
        }
        let mut i = input(&u, 3);
        i.target_x = 100;
        i.target_y = 100;
        let p = plan_direct_land_volley(0, &i).unwrap();
        assert_eq!(p.shot_count, 3);
        assert_eq!(p.guy_aims[0].des_angle, HALF_TURN);
        assert_eq!(p.guy_aims[1].des_angle, QUARTER_TURN);
        assert_eq!(p.guy_aims[2].des_angle, 0);
        // Damage direction is still anchor-to-anchor, not any individual Guy's aim.
        assert_eq!(p.damage_attack_dir, attack_dir(100, 100, 100, 100));
    }

    #[test]
    fn crew_forces_shared_aim_but_never_adds_damage_calls() {
        let mut u = guys(3, 2, 2);
        for (slot, g) in u
            .guys
            .iter_mut()
            .enumerate()
            .filter_map(|(n, g)| g.as_mut().map(|g| (n, g)))
        {
            g.des_x = slot as i32 * 47;
            g.des_y = slot as i32 * -31;
        }
        let p = plan_direct_land_volley(0, &input(&u, 3)).unwrap();
        assert_eq!(
            p.shot_count, 2,
            "guy_mark, not squad_size or PtrArray length"
        );
        assert_eq!(
            p.guy_aims.len(),
            4,
            "two live squad Guys plus two crew Guys"
        );
        assert!(p
            .guy_aims
            .iter()
            .all(|a| a.des_angle == p.damage_attack_dir));
    }

    #[test]
    fn flank_uses_defender_unit_angle_not_a_guy_or_formation_offset() {
        let mut u = guys(2, 0, 2);
        u.guys[0].as_mut().unwrap().angle = 0x1234_5678;
        u.guys[1].as_mut().unwrap().angle = -0x1234_567;
        let mut i = input(&u, 2);
        i.attacker_x = 0;
        i.attacker_y = 100;
        i.target_x = 0;
        i.target_y = 0;
        i.defender_facing = 0;
        let p = plan_direct_land_volley(0, &i).unwrap();
        assert_eq!(p.damage_attack_dir, 0);
        assert_eq!(
            p.flank_tier, 1,
            "attack travels with defender nose: rear arc"
        );
    }

    #[test]
    fn aligned_graphics_turret_retains_body_facing_for_damage_and_flank() {
        let u = guys(1, 0, 1);
        let mut i = input(&u, 1);
        i.attacker_facing = HALF_TURN;
        i.aim_mode = AimMode::GraphicsTurret { aligned: true };
        let p = plan_direct_land_volley(0, &i).unwrap();
        assert_eq!(p.damage_attack_dir, HALF_TURN);
        assert_eq!(p.unit_facing, HALF_TURN);
        assert!(!p.toggle_unit_mask_2);
        assert_eq!(p.guy_aims[0].des_angle, HALF_TURN);
    }

    #[test]
    fn unresolved_graphics_turret_fails_closed() {
        let mut u = guys(1, 0, 1);
        u.guys[0].as_mut().unwrap().guy_flags |= GUY_FLAG_TURRETS;
        let mut i = input(&u, 1);
        i.aim_mode = AimMode::from_guys(&u);
        assert_eq!(i.aim_mode, AimMode::UnresolvedGraphicsTurret);
        assert_eq!(
            plan_direct_land_volley(0, &i),
            Err(VolleyPlanError::UnresolvedGraphicsTurret)
        );
    }

    #[test]
    fn a_hole_inside_guy_mark_is_invalid_retail_state_not_a_smaller_volley() {
        let mut u = guys(3, 0, 3);
        u.guys[1] = None;
        assert_eq!(
            plan_direct_land_volley(0, &input(&u, 3)),
            Err(VolleyPlanError::MissingGuy { slot: 1 })
        );
    }

    #[test]
    fn non_land_domains_cannot_accidentally_take_the_direct_land_arm() {
        let u = guys(1, 0, 1);
        assert_eq!(
            plan_direct_land_volley(1, &input(&u, 1)),
            Err(VolleyPlanError::UnsupportedDomain(1))
        );
        assert_eq!(
            plan_direct_land_volley(2, &input(&u, 1)),
            Err(VolleyPlanError::UnsupportedDomain(2))
        );
    }
}
