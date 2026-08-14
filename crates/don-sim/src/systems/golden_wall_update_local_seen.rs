// SPDX-License-Identifier: GPL-3.0-or-later
//! Detached golden Village entry into `Wall::update_local_seen`.
//!
//! `Build::process` stores the selected active-Leader bit in
//! `WallData::ever_seen` at `0x0061FBCD`, then dispatches virtual slot `+0x164`
//! at `0x0061FBD2`; the golden Build vtable resolves that dispatch to
//! `Wall::update_local_seen` `0x0063ED50`. After its prologue, the first nested
//! call is virtual slot `+0x2C` at `0x0063ED64`, resolved by the same Build
//! vtable to `BuildData::is_wonder` `0x00472320`.
//!
//! This module owns exactly that chronology and stops before `is_wonder`.
//! Planning keeps the pre-child `ever_seen` write staged: the input Build is a
//! before-image, [`GoldenEverSeenJournal::rollback`] is the exact rollback byte,
//! and no state is published without the unavailable child continuation.

#![forbid(unsafe_code)]

use crate::systems::golden_build_post_wall::{
    self, GoldenBuildActor, GoldenVillageEverSeenWrite, GoldenVillageTargetGateContinuation,
    GoldenVillageTargetGateExit, GoldenVillageTargetGateReceipt,
};
use crate::systems::leader_get_target_runtime;
use crate::systems::production::BuildData;

pub const WALL_UPDATE_LOCAL_SEEN_ENTRY_VA: u32 = 0x0063_ed50;
pub const WALL_UPDATE_LOCAL_SEEN_FIRST_CHILD_CALL_VA: u32 = 0x0063_ed64;
pub const WALL_UPDATE_LOCAL_SEEN_FIRST_CHILD_SLOT: u32 = 0x002c;
pub const BUILD_DATA_IS_WONDER_VA: u32 = 0x0047_2320;
pub const GOLDEN_BUILD_VTABLE_VA: u32 = 0x00b4_2174;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GoldenJournalState {
    /// The write is ordered and validated but has not changed the caller's Build.
    StagedNotPublished,
}

/// Exact reversible byte write retail performs before entering the child.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GoldenEverSeenJournal {
    pub write: GoldenVillageEverSeenWrite,
    pub rollback: u8,
    pub state: GoldenJournalState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GoldenWallUpdateLocalSeenContinuation {
    BuildDataIsWonder {
        call_va: u32,
        vtable_slot: u32,
        resolved_callee_va: u32,
    },
}

/// Pure prefix receipt. Stack-local prologue writes are not simulation state and are not
/// listed as journal entries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GoldenWallUpdateLocalSeenPrefixReceipt {
    pub frame: i32,
    pub actor: GoldenBuildActor,
    pub build_ordinal: u8,
    pub owner: u8,
    pub object_id: i16,
    pub uid: u16,
    pub caller_write_va: u32,
    pub caller_dispatch_va: u32,
    pub receiver_vtable_va: u32,
    pub entry_va: u32,
    pub journal: GoldenEverSeenJournal,
    pub continuation: GoldenWallUpdateLocalSeenContinuation,
    /// Neither the staged caller write nor this callee prefix consumes RNG.
    pub rng_draws: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GoldenWallUpdateLocalSeenPrefixError {
    PriorDidNotReachUpdateLocalSeen,
    WrongActorOrder,
    BuildSnapshotMismatch,
    InvalidJournalLeaderSlot(u8),
    InvalidJournalTransition {
        before: u8,
        after: u8,
        leader_slot: u8,
    },
    MissingTargetCall(u8),
    TargetCallOwnerMismatch {
        leader_slot: u8,
        target: i32,
        owner: u8,
    },
}

/// Source-own `Wall::update_local_seen` through its first child boundary.
///
/// The Build must still be the immediate pre-`0x0061FBCD` image used by
/// [`golden_build_post_wall::plan_village_target_gate`]. The returned journal is sufficient
/// to apply and roll back retail's caller write later, but this detached transaction does
/// neither because `BuildData::is_wonder` has not executed here.
pub fn plan_wall_update_local_seen_prefix(
    prior: &GoldenVillageTargetGateReceipt,
    build: &BuildData,
) -> Result<GoldenWallUpdateLocalSeenPrefixReceipt, GoldenWallUpdateLocalSeenPrefixError> {
    if prior.exit != GoldenVillageTargetGateExit::UpdateLocalSeenBoundary {
        return Err(GoldenWallUpdateLocalSeenPrefixError::PriorDidNotReachUpdateLocalSeen);
    }
    let GoldenVillageTargetGateContinuation::UpdateLocalSeen {
        call_va,
        callee_va,
        planned_write,
    } = prior.continuation
    else {
        return Err(GoldenWallUpdateLocalSeenPrefixError::PriorDidNotReachUpdateLocalSeen);
    };
    if call_va != golden_build_post_wall::VILLAGE_UPDATE_LOCAL_SEEN_CALL_VA
        || callee_va != WALL_UPDATE_LOCAL_SEEN_ENTRY_VA
        || planned_write.instruction_va != golden_build_post_wall::VILLAGE_EVER_SEEN_WRITE_VA
    {
        return Err(GoldenWallUpdateLocalSeenPrefixError::PriorDidNotReachUpdateLocalSeen);
    }
    if prior.actor != GoldenBuildActor::Village
        || prior.build_ordinal != GoldenBuildActor::Village.build_ordinal()
        || prior.owner != golden_build_post_wall::GOLDEN_OWNER
        || prior.object_id != golden_build_post_wall::GOLDEN_VILLAGE_O
    {
        return Err(GoldenWallUpdateLocalSeenPrefixError::WrongActorOrder);
    }
    let schedule_matches = golden_build_post_wall::golden_build_schedule(prior.frame)
        .map(|rows| rows[GoldenBuildActor::Village.build_ordinal() as usize])
        .is_some_and(|row| {
            row.actor == prior.actor
                && row.build_ordinal == prior.build_ordinal
                && row.owner == prior.owner
                && row.object_id == prior.object_id
                && row.wall_continuation == golden_build_post_wall::GoldenWallContinuation::Returned
        });
    if !schedule_matches {
        return Err(GoldenWallUpdateLocalSeenPrefixError::WrongActorOrder);
    }
    if build.who != prior.owner
        || build.object_id() != prior.object_id
        || build.uid != prior.uid
        || !build.is_active()
        || build.ever_seen != prior.ever_seen_before
        || build.ever_seen != planned_write.before
    {
        return Err(GoldenWallUpdateLocalSeenPrefixError::BuildSnapshotMismatch);
    }
    let leader_slot = planned_write.leader_slot;
    if usize::from(leader_slot) >= leader_get_target_runtime::LEADER_COUNT {
        return Err(GoldenWallUpdateLocalSeenPrefixError::InvalidJournalLeaderSlot(leader_slot));
    }
    let expected_after = planned_write.before | (1u8 << leader_slot);
    if planned_write.after != expected_after {
        return Err(
            GoldenWallUpdateLocalSeenPrefixError::InvalidJournalTransition {
                before: planned_write.before,
                after: planned_write.after,
                leader_slot,
            },
        );
    }
    let target_call = prior.target_calls[usize::from(leader_slot)].ok_or(
        GoldenWallUpdateLocalSeenPrefixError::MissingTargetCall(leader_slot),
    )?;
    if target_call.target != i32::from(prior.owner) {
        return Err(
            GoldenWallUpdateLocalSeenPrefixError::TargetCallOwnerMismatch {
                leader_slot,
                target: target_call.target,
                owner: prior.owner,
            },
        );
    }

    Ok(GoldenWallUpdateLocalSeenPrefixReceipt {
        frame: prior.frame,
        actor: prior.actor,
        build_ordinal: prior.build_ordinal,
        owner: prior.owner,
        object_id: prior.object_id,
        uid: prior.uid,
        caller_write_va: planned_write.instruction_va,
        caller_dispatch_va: call_va,
        receiver_vtable_va: GOLDEN_BUILD_VTABLE_VA,
        entry_va: WALL_UPDATE_LOCAL_SEEN_ENTRY_VA,
        journal: GoldenEverSeenJournal {
            write: planned_write,
            rollback: planned_write.before,
            state: GoldenJournalState::StagedNotPublished,
        },
        continuation: GoldenWallUpdateLocalSeenContinuation::BuildDataIsWonder {
            call_va: WALL_UPDATE_LOCAL_SEEN_FIRST_CHILD_CALL_VA,
            vtable_slot: WALL_UPDATE_LOCAL_SEEN_FIRST_CHILD_SLOT,
            resolved_callee_va: BUILD_DATA_IS_WONDER_VA,
        },
        rng_draws: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::systems::golden_build_post_wall::{
        plan_after_wall_return, plan_village_target_gate, BuildPostWallTypeFacts,
        GoldenVillageTargetGateContinuation, ASSASSIN_TEAM_STYLE, CITY_CAPITAL_FLAG,
        GOLDEN_FIRST_FRAME, GOLDEN_VILLAGE_TYPE,
    };
    use crate::systems::leader_get_target_runtime::{
        LeaderTargetContext, LeaderTargetGame, LeaderTargetRow, ACTIVE_LEADER_MASK, LEADER_COUNT,
    };
    use crate::systems::{production, tech_cities};

    fn build() -> BuildData {
        let mut build = BuildData::default();
        build.flags =
            production::flag::VALID | production::flag::STARTED | production::flag::ACTIVE | 0x20;
        build.who = golden_build_post_wall::GOLDEN_OWNER;
        build.set_object_id(golden_build_post_wall::GOLDEN_VILLAGE_O);
        build.city = 0;
        build.attack_ox = -1;
        build.attack_whom = -1;
        build.other[0x34..0x36].copy_from_slice(&(-1_i16).to_le_bytes());
        build.other[0x36..0x38].copy_from_slice(&(-1_i16).to_le_bytes());
        build
    }

    fn city() -> tech_cities::CityRecord {
        tech_cities::CityRecord {
            city_flags: 1 | CITY_CAPITAL_FLAG,
            city: 0,
            who: golden_build_post_wall::GOLDEN_OWNER as i8,
            ..tech_cities::CityRecord::default()
        }
    }

    fn target_context() -> LeaderTargetContext {
        LeaderTargetContext {
            leaders: std::array::from_fn(|slot| LeaderTargetRow {
                leader_flags: if slot == 0 { ACTIVE_LEADER_MASK } else { 0 },
                who: slot as i32,
            }),
            game: LeaderTargetGame {
                start_list: [0, 1, 2, 3, 4, 5, 6, 7],
                start_index: [7; LEADER_COUNT],
            },
        }
    }

    fn reached_gate(build: &BuildData) -> GoldenVillageTargetGateReceipt {
        let post_wall = plan_after_wall_return(
            GoldenBuildActor::Village,
            GOLDEN_FIRST_FRAME,
            build,
            BuildPostWallTypeFacts {
                type_index: GOLDEN_VILLAGE_TYPE,
                attack: 80,
                build_flags: 0,
            },
        )
        .unwrap();
        plan_village_target_gate(
            &post_wall,
            build,
            &city(),
            ASSASSIN_TEAM_STYLE,
            Some(&target_context()),
        )
        .unwrap()
    }

    #[test]
    fn source_owned_prefix_stages_reversible_write_then_names_first_child() {
        let build = build();
        let before_image = build.image();
        let gate = reached_gate(&build);
        let receipt = plan_wall_update_local_seen_prefix(&gate, &build).unwrap();

        assert_eq!(build.image(), before_image);
        assert_eq!(
            (receipt.actor, receipt.build_ordinal, receipt.object_id),
            (GoldenBuildActor::Village, 0, 2000)
        );
        assert_eq!(receipt.caller_write_va, 0x0061_fbcd);
        assert_eq!(receipt.caller_dispatch_va, 0x0061_fbd2);
        assert_eq!(receipt.entry_va, 0x0063_ed50);
        assert_eq!(receipt.journal.write.before, 0);
        assert_eq!(receipt.journal.write.after, 1);
        assert_eq!(receipt.journal.rollback, 0);
        assert_eq!(
            receipt.journal.state,
            GoldenJournalState::StagedNotPublished
        );
        assert_eq!(
            receipt.continuation,
            GoldenWallUpdateLocalSeenContinuation::BuildDataIsWonder {
                call_va: 0x0063_ed64,
                vtable_slot: 0x2c,
                resolved_callee_va: 0x0047_2320,
            }
        );
        assert_eq!(receipt.rng_draws, 0);
    }

    #[test]
    fn changed_receiver_refuses_the_staged_journal() {
        let mut build = build();
        let gate = reached_gate(&build);
        build.ever_seen = 1;
        assert_eq!(
            plan_wall_update_local_seen_prefix(&gate, &build),
            Err(GoldenWallUpdateLocalSeenPrefixError::BuildSnapshotMismatch)
        );
    }

    #[test]
    fn local_join_does_not_fabricate_an_update_local_seen_entry() {
        let build = build();
        let post_wall = plan_after_wall_return(
            GoldenBuildActor::Village,
            GOLDEN_FIRST_FRAME,
            &build,
            BuildPostWallTypeFacts {
                type_index: GOLDEN_VILLAGE_TYPE,
                attack: 80,
                build_flags: 0,
            },
        )
        .unwrap();
        let ordinary_team = plan_village_target_gate(&post_wall, &build, &city(), 0, None).unwrap();
        assert!(matches!(
            ordinary_team.continuation,
            GoldenVillageTargetGateContinuation::NextBuildCone { .. }
        ));
        assert_eq!(
            plan_wall_update_local_seen_prefix(&ordinary_team, &build),
            Err(GoldenWallUpdateLocalSeenPrefixError::PriorDidNotReachUpdateLocalSeen)
        );
    }
}
