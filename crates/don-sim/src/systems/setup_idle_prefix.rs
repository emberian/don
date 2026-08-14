//! Typed, rollback-safe boundaries for the supported replay's frame-one Unit idle pass.
//!
//! These request values are deliberately state-bearing. A replay command proves neither the
//! post-frame-zero Unit image nor the transitive Guy/World work reached by `Unit::do_idle`.
//! Capture binders consume these exact requests and return adjacent-call receipts; no boolean
//! child result is sufficient to publish a tick mutation.

#![forbid(unsafe_code)]

use crate::systems::groups_guys::UnitGuys;
use crate::systems::unit_inctime::{anim_class, CLASS_ATTACK, SUPPORTED_RETAIL_EXE_SHA256};
use crate::world::Handle;

pub use super::frame1_caster_process::CasterProcessSpellsRequest;

/// `Unit::set_anim(0, 0, 1)` at the `Unit::do_idle` call site `0x0060DD48`.
///
/// `guys_before` is the complete checksum/save-owned pointer-array image. Retail tests the
/// lead Guy's animation family before clearing `hold_attack` on every addressed Guy, and the
/// nested Guy calls may recurse and consume the main RNG; a lead-only projection is unsound.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdleSetAnimRequest {
    pub authority_revision: u64,
    pub authority_digest: [u8; 32],
    pub frame: i32,
    pub unit: Handle,
    pub who: u8,
    pub o: i16,
    pub animation: i8,
    pub arg2: i32,
    pub arg3: i32,
    pub guys_before: UnitGuys,
    pub random_before: i32,
}

/// `Unit::set_angle(trench_angle, 0, 0)` reached when idle becomes four in a trench.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IdleSetAngleRequest {
    pub authority_revision: u64,
    pub authority_digest: [u8; 32],
    pub frame: i32,
    pub unit: Handle,
    pub who: u8,
    pub o: i16,
    pub angle: i32,
    pub arg2: i32,
    pub arg3: i32,
}

/// External entrenchment/graphic lookup reached from `Unit::check_idle`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IdleEntrenchRequest {
    pub authority_revision: u64,
    pub authority_digest: [u8; 32],
    pub frame: i32,
    pub unit: Handle,
    pub who: u8,
    pub o: i16,
}

/// Exact argument image at `Unit::find_build_spot -> Objects::find_builds`, call site
/// `0x00603F0C`. The child owns the ordered scratch-array mutation and must return that whole
/// image, not merely the integer count or a chosen Build.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IdleFindBuildsRequest {
    pub authority_revision: u64,
    pub authority_digest: [u8; 32],
    pub frame: i32,
    pub unit: Handle,
    pub x: i32,
    pub y: i32,
    pub search: i32,
    pub who: u8,
    pub radius: i32,
    pub relation_mask: i32,
    pub filter: i32,
    pub arg8: i32,
    pub origin_x: i32,
    pub arg10: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IdleProcessHealingRequest {
    pub authority_revision: u64,
    pub authority_digest: [u8; 32],
    pub frame: i32,
    pub unit: Handle,
    pub who: u8,
    pub o: i16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IdleBoatCollisionRequest {
    pub authority_revision: u64,
    pub authority_digest: [u8; 32],
    pub frame: i32,
    pub unit: Handle,
    pub who: u8,
    pub o: i16,
    pub collide_frame: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IdleAnimationGateRequest {
    pub authority_revision: u64,
    pub authority_digest: [u8; 32],
    pub frame: i32,
    pub unit: Handle,
    pub who: u8,
    pub o: i16,
}

/// First external boundary reached by a detached idle-prefix plan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IdleOpenRequest {
    CasterProcessSpells(CasterProcessSpellsRequest),
    ProcessHealing(IdleProcessHealingRequest),
    DetectBoatCollision(IdleBoatCollisionRequest),
    AnimationGate(IdleAnimationGateRequest),
    SetAnim(IdleSetAnimRequest),
    SetAngle(IdleSetAngleRequest),
    Entrench(IdleEntrenchRequest),
    FindBuilds(IdleFindBuildsRequest),
}

/// Supported-retail adjacent capture after both frame-one LeaderOptions commands and before
/// the frame-one step-14 object walk.  The inventory is structural on purpose: the old
/// center-plus-seven setup receipt omitted the Dutch Market and is not admissible here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GoldenFrame1EntrySource {
    SupportedRetailPostCommandAdjacentCapture,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GoldenFrame1BuildIdentity {
    pub who: u8,
    pub o: i16,
    pub uid: u16,
    pub type_index: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GoldenFrame1UnitIdentity {
    pub handle: Handle,
    pub who: u8,
    pub o: i16,
    pub uid: u16,
    pub type_index: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GoldenFrame1EntryAuthority {
    pub revision: u64,
    pub composition_digest: [u8; 32],
    pub source: GoldenFrame1EntrySource,
    pub executable_sha256: [u8; 32],
    pub replay_file_sha256: [u8; 32],
    pub post_command_sim_sha256: [u8; 32],
    pub frame: i32,
    pub random_state: i32,
    pub center: GoldenFrame1BuildIdentity,
    pub market: GoldenFrame1BuildIdentity,
    pub units: [GoldenFrame1UnitIdentity; 7],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IdleCitizenTypeFacts {
    pub type_index: i32,
    pub domain: i32,
    pub unit_flags: u32,
    pub unit_flags2: u32,
    pub role: i32,
}

/// Exact live post-command image read before `Unit::process` for one Citizen.  Instance masks
/// are independent live fields: they must never be seeded from `ObjectTypeData::obj_masks`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdleCitizenPreimage {
    pub frame: i32,
    pub unit: GoldenFrame1UnitIdentity,
    pub type_facts: IdleCitizenTypeFacts,
    pub object_flags: u8,
    pub inside_up: i16,
    pub orders_empty: bool,
    pub path_empty: bool,
    pub recharging: u8,
    pub full: u8,
    pub waiting: u8,
    pub healing: i16,
    pub mana_burn: i16,
    pub num_queued: i16,
    pub attrition: i16,
    pub healing_has_damage: bool,
    pub unit_masks: u32,
    pub unit_masks2: u32,
    pub spell_time: i16,
    pub safe: i8,
    pub collide_frame: i32,
    pub collide: i16,
    pub idle: u8,
    pub worker_stance: i8,
    pub guys: UnitGuys,
    pub random_state: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdleLocalWrite {
    ClearProcessMask2Bit10 { before: u32, after: u32 },
    ClearObjectCastingBit { before: u8, after: u8 },
    ClearSpellTime { before: i16, after: i16 },
    DecrementSafe { before: i8, after: i8 },
    TemporaryClearMask2Bit8000 { before: u32, after: u32 },
}

/// Detached transaction prefix. `after_local` has not been committed to a `Sim`.
/// `restore_mask2_bit8000` remains armed across the child and is discharged only after the
/// complete `check_idle -> think` continuation returns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdleCitizenPrefixPlan {
    pub authority_revision: u64,
    pub authority_digest: [u8; 32],
    pub before: IdleCitizenPreimage,
    pub after_local: IdleCitizenPreimage,
    pub journal: Vec<IdleLocalWrite>,
    pub restore_mask2_bit8000: bool,
    pub open: IdleOpenRequest,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IdleCitizenPrefixError {
    MissingAuthorityRevision,
    MissingAuthorityDigest,
    UnsupportedExecutable,
    MissingReplayIdentity,
    MissingCaptureIdentity,
    WrongFrame,
    MissingCenter,
    MissingMarket,
    WrongUnitInventory,
    UnitNotInInventory,
    WrongCitizen,
    InactiveOrContained,
    NonEmptyOrderOrPath,
    NonFreshCountdown,
    Phase32Unexpected,
    RandomMismatch,
}

fn exact_inventory(authority: &GoldenFrame1EntryAuthority) -> bool {
    authority.center.who == 0
        && authority.center.o == 2000
        && authority.center.type_index == 414
        && authority.market.who == 0
        && authority.market.o == 2001
        && authority.market.type_index == 436
        && authority.units.iter().enumerate().all(|(ordinal, unit)| {
            unit.who == 0
                && unit.o == ordinal as i16
                && unit.type_index
                    == match ordinal {
                        0 => 69,
                        1 | 2 => 62,
                        _ => 50,
                    }
        })
}

/// Plan the exact frame-one Citizen prefix through the first adjacent child call.
///
/// The planner deliberately does not mutate a `Sim`. Missing healing, collision, or Guy
/// authority returns a typed request with every preceding local write still detached. The
/// ordinary captured Citizen path reaches `Unit::set_anim(0,0,1)` after the on-map mask clear.
pub fn plan_frame1_citizen_idle_prefix(
    authority: &GoldenFrame1EntryAuthority,
    before: &IdleCitizenPreimage,
) -> Result<IdleCitizenPrefixPlan, IdleCitizenPrefixError> {
    if authority.revision == 0 {
        return Err(IdleCitizenPrefixError::MissingAuthorityRevision);
    }
    if authority.composition_digest == [0; 32] {
        return Err(IdleCitizenPrefixError::MissingAuthorityDigest);
    }
    if authority.executable_sha256 != SUPPORTED_RETAIL_EXE_SHA256 {
        return Err(IdleCitizenPrefixError::UnsupportedExecutable);
    }
    if authority.replay_file_sha256 == [0; 32] {
        return Err(IdleCitizenPrefixError::MissingReplayIdentity);
    }
    if authority.post_command_sim_sha256 == [0; 32] {
        return Err(IdleCitizenPrefixError::MissingCaptureIdentity);
    }
    if authority.frame != 1 || before.frame != 1 {
        return Err(IdleCitizenPrefixError::WrongFrame);
    }
    if authority.center.o != 2000 || authority.center.type_index != 414 {
        return Err(IdleCitizenPrefixError::MissingCenter);
    }
    if authority.market.o != 2001 || authority.market.type_index != 436 {
        return Err(IdleCitizenPrefixError::MissingMarket);
    }
    if !exact_inventory(authority) {
        return Err(IdleCitizenPrefixError::WrongUnitInventory);
    }
    if !authority.units.iter().any(|unit| *unit == before.unit) {
        return Err(IdleCitizenPrefixError::UnitNotInInventory);
    }
    if before.unit.who != 0
        || !(3..=6).contains(&before.unit.o)
        || before.unit.type_index != 50
        || before.type_facts
            != (IdleCitizenTypeFacts {
                type_index: 50,
                domain: 0,
                unit_flags: 0x1881,
                unit_flags2: 2,
                role: 0x40300,
            })
    {
        return Err(IdleCitizenPrefixError::WrongCitizen);
    }
    if before.object_flags & 1 == 0 || before.inside_up >= 0 {
        return Err(IdleCitizenPrefixError::InactiveOrContained);
    }
    if !before.orders_empty || !before.path_empty {
        return Err(IdleCitizenPrefixError::NonEmptyOrderOrPath);
    }
    if before.recharging != 0
        || before.full != 0
        || before.waiting != 0
        || before.healing != 0
        || before.mana_burn != 0
        || before.num_queued != 0
        || before.attrition != 0
    {
        return Err(IdleCitizenPrefixError::NonFreshCountdown);
    }
    if before.frame.wrapping_add(i32::from(before.unit.o)) % 32 == 0 {
        return Err(IdleCitizenPrefixError::Phase32Unexpected);
    }
    if authority.random_state != before.random_state {
        return Err(IdleCitizenPrefixError::RandomMismatch);
    }

    let mut after = before.clone();
    let mut journal = Vec::new();
    if before.healing_has_damage {
        return Ok(IdleCitizenPrefixPlan {
            authority_revision: authority.revision,
            authority_digest: authority.composition_digest,
            before: before.clone(),
            after_local: after,
            journal,
            restore_mask2_bit8000: false,
            open: IdleOpenRequest::ProcessHealing(IdleProcessHealingRequest {
                authority_revision: authority.revision,
                authority_digest: authority.composition_digest,
                frame: before.frame,
                unit: before.unit.handle,
                who: before.unit.who,
                o: before.unit.o,
            }),
        });
    }

    let masks2_before = after.unit_masks2;
    after.unit_masks2 &= !0x10;
    journal.push(IdleLocalWrite::ClearProcessMask2Bit10 {
        before: masks2_before,
        after: after.unit_masks2,
    });
    let flags_before = after.object_flags;
    after.object_flags &= 0x7f;
    journal.push(IdleLocalWrite::ClearObjectCastingBit {
        before: flags_before,
        after: after.object_flags,
    });
    let spell_before = after.spell_time;
    after.spell_time = 0;
    journal.push(IdleLocalWrite::ClearSpellTime {
        before: spell_before,
        after: 0,
    });
    if after.safe != 0 {
        let safe_before = after.safe;
        after.safe = after.safe.wrapping_sub(1);
        journal.push(IdleLocalWrite::DecrementSafe {
            before: safe_before,
            after: after.safe,
        });
    }
    if after.collide_frame > after.frame - 4 {
        return Ok(IdleCitizenPrefixPlan {
            authority_revision: authority.revision,
            authority_digest: authority.composition_digest,
            before: before.clone(),
            after_local: after.clone(),
            journal,
            restore_mask2_bit8000: false,
            open: IdleOpenRequest::DetectBoatCollision(IdleBoatCollisionRequest {
                authority_revision: authority.revision,
                authority_digest: authority.composition_digest,
                frame: after.frame,
                unit: after.unit.handle,
                who: after.unit.who,
                o: after.unit.o,
                collide_frame: after.collide_frame,
            }),
        });
    }

    let restore_mask2_bit8000 = after.unit_masks2 & 0x8000 != 0;
    if restore_mask2_bit8000 {
        let before_clear = after.unit_masks2;
        after.unit_masks2 &= !0x8000;
        journal.push(IdleLocalWrite::TemporaryClearMask2Bit8000 {
            before: before_clear,
            after: after.unit_masks2,
        });
    }
    let Some(lead) = after.guys.guys.first().and_then(Option::as_ref) else {
        return Ok(IdleCitizenPrefixPlan {
            authority_revision: authority.revision,
            authority_digest: authority.composition_digest,
            before: before.clone(),
            after_local: after.clone(),
            journal,
            restore_mask2_bit8000,
            open: IdleOpenRequest::AnimationGate(IdleAnimationGateRequest {
                authority_revision: authority.revision,
                authority_digest: authority.composition_digest,
                frame: after.frame,
                unit: after.unit.handle,
                who: after.unit.who,
                o: after.unit.o,
            }),
        });
    };
    if after.guys.guys.len() != 1
        || after.guys.guy_mark != 1
        || lead.ty != 50
        || lead.who != 0
        || lead.o != after.unit.o
        || lead.guy_num != 0
    {
        return Ok(IdleCitizenPrefixPlan {
            authority_revision: authority.revision,
            authority_digest: authority.composition_digest,
            before: before.clone(),
            after_local: after.clone(),
            journal,
            restore_mask2_bit8000,
            open: IdleOpenRequest::AnimationGate(IdleAnimationGateRequest {
                authority_revision: authority.revision,
                authority_digest: authority.composition_digest,
                frame: after.frame,
                unit: after.unit.handle,
                who: after.unit.who,
                o: after.unit.o,
            }),
        });
    }
    // Attack-family Guys skip SetAnim only for a hero. Citizen50 has flags2=2, so both the
    // ordinary and attack-family gates reach the same call; computing the class here keeps
    // the exact lead-Guy dependency executable and mutation-testable.
    let _lead_is_attack_family = anim_class(lead.cur_anim) == CLASS_ATTACK;
    let open = IdleOpenRequest::SetAnim(IdleSetAnimRequest {
        authority_revision: authority.revision,
        authority_digest: authority.composition_digest,
        frame: after.frame,
        unit: after.unit.handle,
        who: after.unit.who,
        o: after.unit.o,
        animation: 0,
        arg2: 0,
        arg3: 1,
        guys_before: after.guys.clone(),
        random_before: after.random_state,
    });
    Ok(IdleCitizenPrefixPlan {
        authority_revision: authority.revision,
        authority_digest: authority.composition_digest,
        before: before.clone(),
        after_local: after,
        journal,
        restore_mask2_bit8000,
        open,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::systems::groups_guys::{GuyData, UnitGuys};

    fn authority() -> GoldenFrame1EntryAuthority {
        let units = std::array::from_fn(|ordinal| GoldenFrame1UnitIdentity {
            handle: Handle {
                id: ordinal as u32,
                generation: 4,
            },
            who: 0,
            o: ordinal as i16,
            uid: (ordinal + 1) as u16,
            type_index: match ordinal {
                0 => 69,
                1 | 2 => 62,
                _ => 50,
            },
        });
        GoldenFrame1EntryAuthority {
            revision: 9,
            composition_digest: [9; 32],
            source: GoldenFrame1EntrySource::SupportedRetailPostCommandAdjacentCapture,
            executable_sha256: SUPPORTED_RETAIL_EXE_SHA256,
            replay_file_sha256: [8; 32],
            post_command_sim_sha256: [7; 32],
            frame: 1,
            random_state: 123,
            center: GoldenFrame1BuildIdentity {
                who: 0,
                o: 2000,
                uid: 1,
                type_index: 414,
            },
            market: GoldenFrame1BuildIdentity {
                who: 0,
                o: 2001,
                uid: 1,
                type_index: 436,
            },
            units,
        }
    }

    fn citizen() -> IdleCitizenPreimage {
        let authority = authority();
        let mut guy = GuyData::default();
        guy.ty = 50;
        guy.who = 0;
        guy.o = 3;
        guy.guy_num = 0;
        guy.cur_anim = 0;
        IdleCitizenPreimage {
            frame: 1,
            unit: authority.units[3],
            type_facts: IdleCitizenTypeFacts {
                type_index: 50,
                domain: 0,
                unit_flags: 0x1881,
                unit_flags2: 2,
                role: 0x40300,
            },
            object_flags: 0x99,
            inside_up: -1,
            orders_empty: true,
            path_empty: true,
            recharging: 0,
            full: 0,
            waiting: 0,
            healing: 0,
            mana_burn: 0,
            num_queued: 0,
            attrition: 0,
            healing_has_damage: false,
            unit_masks: 0x1234_5678,
            unit_masks2: 0x8010,
            spell_time: 4,
            safe: 2,
            collide_frame: -100,
            collide: 7,
            idle: 1,
            worker_stance: 1,
            guys: UnitGuys {
                guys: vec![Some(guy)],
                size: 1,
                increment: 1,
                flags: 0,
                guy_mark: 1,
            },
            random_state: 123,
        }
    }

    #[test]
    fn set_anim_request_retains_generation_complete_guys_and_rng() {
        let request = IdleSetAnimRequest {
            authority_revision: 7,
            authority_digest: [0x5a; 32],
            frame: 1,
            unit: Handle {
                id: 3,
                generation: 9,
            },
            who: 0,
            o: 3,
            animation: 0,
            arg2: 0,
            arg3: 1,
            guys_before: UnitGuys::default(),
            random_before: 0x1234_5678,
        };
        let IdleOpenRequest::SetAnim(bound) = IdleOpenRequest::SetAnim(request.clone()) else {
            unreachable!()
        };
        assert_eq!(bound, request);
        assert_eq!(bound.unit.generation, 9);
        assert_eq!((bound.animation, bound.arg2, bound.arg3), (0, 0, 1));
    }

    #[test]
    fn find_builds_request_retains_all_ten_native_arguments() {
        let request = IdleFindBuildsRequest {
            authority_revision: 1,
            authority_digest: [1; 32],
            frame: 1,
            unit: Handle {
                id: 3,
                generation: 4,
            },
            x: 100,
            y: 200,
            search: 1,
            who: 0,
            radius: 4_608,
            relation_mask: 0x200,
            filter: 6,
            arg8: 0,
            origin_x: 100,
            arg10: 0,
        };
        assert_eq!(
            (
                request.x,
                request.y,
                request.search,
                request.who,
                request.radius,
                request.relation_mask,
                request.filter,
                request.arg8,
                request.origin_x,
                request.arg10
            ),
            (100, 200, 1, 0, 4_608, 0x200, 6, 0, 100, 0)
        );
    }

    #[test]
    fn golden_citizen_reaches_set_anim_with_detached_temporary_mask() {
        let before = citizen();
        let plan = plan_frame1_citizen_idle_prefix(&authority(), &before).unwrap();
        let IdleOpenRequest::SetAnim(request) = &plan.open else {
            panic!("expected SetAnim, got {:?}", plan.open)
        };
        assert_eq!((request.animation, request.arg2, request.arg3), (0, 0, 1));
        assert_eq!(request.unit, before.unit.handle);
        assert_eq!(plan.before.unit_masks, 0x1234_5678);
        assert_eq!(plan.after_local.unit_masks, 0x1234_5678);
        assert_eq!(plan.after_local.unit_masks2, 0);
        assert!(plan.restore_mask2_bit8000);
        assert_eq!(plan.after_local.object_flags, 0x19);
        assert_eq!(plan.after_local.spell_time, 0);
        assert_eq!(plan.after_local.safe, 1);
        assert_eq!(plan.after_local.collide, 7);
        assert_eq!(plan.after_local.idle, 1);
        assert_eq!(plan.journal.len(), 5);
    }

    #[test]
    fn missing_market_refuses_before_any_plan_exists() {
        let mut authority = authority();
        authority.market.type_index = 435;
        assert_eq!(
            plan_frame1_citizen_idle_prefix(&authority, &citizen()),
            Err(IdleCitizenPrefixError::MissingMarket)
        );
    }

    #[test]
    fn live_instance_masks_are_not_replaced_by_type_masks() {
        let mut before = citizen();
        before.unit_masks = 0xa5a5_1234;
        before.unit_masks2 = 0x55;
        let plan = plan_frame1_citizen_idle_prefix(&authority(), &before).unwrap();
        assert_eq!(plan.after_local.unit_masks, 0xa5a5_1234);
        assert_eq!(plan.after_local.unit_masks2, 0x45);
        assert!(!plan.restore_mask2_bit8000);
    }

    #[test]
    fn recent_collision_stops_before_idle_and_keeps_temporary_bit_unopened() {
        let mut before = citizen();
        before.collide_frame = 0;
        let plan = plan_frame1_citizen_idle_prefix(&authority(), &before).unwrap();
        assert!(matches!(plan.open, IdleOpenRequest::DetectBoatCollision(_)));
        assert_eq!(plan.after_local.unit_masks2, 0x8000);
        assert!(!plan.restore_mask2_bit8000);
    }

    #[test]
    fn missing_lead_guy_is_a_typed_gate_not_an_empty_unit() {
        let mut before = citizen();
        before.guys.guys[0] = None;
        let plan = plan_frame1_citizen_idle_prefix(&authority(), &before).unwrap();
        assert!(matches!(plan.open, IdleOpenRequest::AnimationGate(_)));
        assert!(plan.restore_mask2_bit8000);

        let mut wrong_identity = citizen();
        wrong_identity.guys.guys[0].as_mut().unwrap().o = 4;
        let plan = plan_frame1_citizen_idle_prefix(&authority(), &wrong_identity).unwrap();
        assert!(matches!(plan.open, IdleOpenRequest::AnimationGate(_)));
    }

    #[test]
    fn stale_generation_and_damaged_healing_paths_bite() {
        let mut stale = citizen();
        stale.unit.handle.generation += 1;
        assert_eq!(
            plan_frame1_citizen_idle_prefix(&authority(), &stale),
            Err(IdleCitizenPrefixError::UnitNotInInventory)
        );

        let mut damaged = citizen();
        damaged.healing_has_damage = true;
        let plan = plan_frame1_citizen_idle_prefix(&authority(), &damaged).unwrap();
        assert!(matches!(plan.open, IdleOpenRequest::ProcessHealing(_)));
        assert!(plan.journal.is_empty());
    }
}
