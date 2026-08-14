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
//! This module owns that child for the ordinary `BuildTypeData` vtable, follows
//! the golden Village's false result through mask composition, source-owns
//! `WallData::tile_corner` at `0x0063EDD6 -> 0x00643440`, and walks the
//! x-outer/y-inner footprint prefix to its next child, `World::set_seen2` at
//! `0x0063EE58 -> 0x006B4BB0`.
//! Planning keeps the pre-child `ever_seen` write staged: the input Build is a
//! before-image, [`GoldenEverSeenJournal::rollback`] is the exact rollback byte,
//! and no state is published before the unavailable `World::set_seen2` child.

#![forbid(unsafe_code)]

use crate::systems::golden_build_post_wall::{
    self, GoldenBuildActor, GoldenVillageEverSeenWrite, GoldenVillageTargetGateContinuation,
    GoldenVillageTargetGateExit, GoldenVillageTargetGateReceipt,
};
use crate::systems::leader_get_target_runtime;
use crate::systems::map_terrain::{self, Coord, TCoord};
use crate::systems::production::{self, BuildData};

pub const WALL_UPDATE_LOCAL_SEEN_ENTRY_VA: u32 = 0x0063_ed50;
pub const WALL_UPDATE_LOCAL_SEEN_FIRST_CHILD_CALL_VA: u32 = 0x0063_ed64;
pub const WALL_UPDATE_LOCAL_SEEN_FIRST_CHILD_SLOT: u32 = 0x002c;
pub const BUILD_DATA_IS_WONDER_VA: u32 = 0x0047_2320;
pub const BUILD_DATA_IS_WONDER_DYNAMIC_TAIL_VA: u32 = 0x0047_2349;
pub const BUILD_TYPE_IS_WONDER_SLOT: u32 = 0x001c;
pub const GOLDEN_BUILD_VTABLE_VA: u32 = 0x00b4_2174;
pub const BUILD_TYPE_VTABLE_VA: u32 = 0x00b4_2b94;
pub const WONDER_TYPE_FIRST: i32 = 0x020e;
pub const WONDER_TYPE_END: i32 = 0x021f;
pub const WALL_UPDATE_LOCAL_SEEN_TILE_CORNER_CALL_VA: u32 = 0x0063_edd6;
pub const WALL_DATA_TILE_CORNER_VA: u32 = 0x0064_3440;
pub const WALL_UPDATE_LOCAL_SEEN_SET_SEEN2_CALL_VA: u32 = 0x0063_ee58;
pub const WORLD_SET_SEEN2_VA: u32 = 0x006b_4bb0;
pub const WALL_UPDATE_LOCAL_SEEN_RETURN_VA: u32 = 0x0063_ee98;

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
pub enum BuildDataIsWonderExit {
    Returned { is_wonder: bool },
    DynamicTypeVirtual { jump_va: u32, vtable_slot: u32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildDataIsWonderTypeFacts {
    pub type_index: i32,
    pub type_vtable_va: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GoldenWallLocalSeenTypeFacts {
    pub is_wonder: BuildDataIsWonderTypeFacts,
    pub footprint: production::Footprint,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildDataIsWonderReceipt {
    pub function_va: u32,
    pub type_index: i32,
    pub type_vtable_va: u32,
    pub exit: BuildDataIsWonderExit,
    pub writes: u8,
    pub rng_draws: u8,
}

/// Complete `BuildData::is_wonder` leaf for the ordinary `BuildTypeData` vtable. A custom
/// subtype's tail is returned as its exact first dynamic child rather than guessed.
pub const fn build_data_is_wonder(facts: BuildDataIsWonderTypeFacts) -> BuildDataIsWonderReceipt {
    let exit = if facts.type_vtable_va == BUILD_TYPE_VTABLE_VA {
        BuildDataIsWonderExit::Returned {
            is_wonder: facts.type_index >= WONDER_TYPE_FIRST && facts.type_index < WONDER_TYPE_END,
        }
    } else {
        BuildDataIsWonderExit::DynamicTypeVirtual {
            jump_va: BUILD_DATA_IS_WONDER_DYNAMIC_TAIL_VA,
            vtable_slot: BUILD_TYPE_IS_WONDER_SLOT,
        }
    };
    BuildDataIsWonderReceipt {
        function_va: BUILD_DATA_IS_WONDER_VA,
        type_index: facts.type_index,
        type_vtable_va: facts.type_vtable_va,
        exit,
        writes: 0,
        rng_draws: 0,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GoldenWallLocalSeenMask {
    pub visible_signed: i32,
    pub ever_seen_at_entry: u8,
    pub owner_bit: u8,
    pub player_mask: i32,
    pub explored_only: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WallDataTileCornerReceipt {
    pub function_va: u32,
    pub position_x: i32,
    pub position_y: i32,
    pub footprint: production::Footprint,
    pub corner_x: i32,
    pub corner_y: i32,
    pub writes: u8,
    pub rng_draws: u8,
}

/// Complete source-owned `WallData::tile_corner` result over the canonical coordinate
/// lookup semantics (`TCoord::from_coord`).
pub fn wall_data_tile_corner(
    build: &BuildData,
    footprint: production::Footprint,
) -> WallDataTileCornerReceipt {
    let (position_x, position_y) = build.position();
    let corner_axis = |position: i32, size: i32| {
        let tile = TCoord::from_coord(Coord(position)).0;
        let mut snapped = tile.wrapping_mul(map_terrain::COORD_PER_TILE);
        if size & 1 != 0 {
            snapped = snapped.wrapping_add(map_terrain::COORD_PER_TILE / 2);
        }
        TCoord::from_coord(Coord(snapped)).0.wrapping_sub(size >> 1)
    };
    WallDataTileCornerReceipt {
        function_va: WALL_DATA_TILE_CORNER_VA,
        position_x,
        position_y,
        footprint,
        corner_x: corner_axis(position_x, footprint.x_size),
        corner_y: corner_axis(position_y, footprint.y_size),
        writes: 0,
        rng_draws: 0,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GoldenWallSetSeen2Call {
    pub tile_x: i32,
    pub tile_y: i32,
    pub fog_x: i32,
    pub fog_y: i32,
    pub player_mask: u8,
    pub explored_only: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GoldenWallUpdateLocalSeenContinuation {
    WorldSetSeen2 {
        call_va: u32,
        callee_va: u32,
        call: GoldenWallSetSeen2Call,
    },
    NoValidFootprintCell {
        return_va: u32,
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
    pub type_index: i32,
    pub caller_write_va: u32,
    pub caller_dispatch_va: u32,
    pub receiver_vtable_va: u32,
    pub entry_va: u32,
    pub journal: GoldenEverSeenJournal,
    pub is_wonder_call_va: u32,
    pub is_wonder_vtable_slot: u32,
    pub is_wonder: BuildDataIsWonderReceipt,
    pub mask: GoldenWallLocalSeenMask,
    pub tile_corner_call_va: u32,
    pub tile_corner: WallDataTileCornerReceipt,
    /// Exact `World +0x18/+0x1C` bounds read by the inlined validity checks.
    pub world_tile_xs: i32,
    pub world_tile_ys: i32,
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
    TypeFactsMismatch {
        expected: i32,
        actual: i32,
    },
    DynamicBuildTypeIsWonder {
        jump_va: u32,
        vtable_slot: u32,
    },
    GoldenVillageReportedAsWonder,
    InvalidFootprint {
        x_size: i32,
        y_size: i32,
        tile_xs: i32,
        tile_ys: i32,
    },
    FootprintRangeOverflow,
}

/// Source-own `Wall::update_local_seen` through `BuildData::is_wonder` and
/// `WallData::tile_corner`, stopping at the next child boundary.
///
/// The Build must still be the immediate pre-`0x0061FBCD` image used by
/// [`golden_build_post_wall::plan_village_target_gate`]. The returned journal is sufficient
/// to apply and roll back retail's caller write later, but this detached transaction does
/// neither because the later `World::set_seen2` transaction has not executed here.
pub fn plan_wall_update_local_seen_prefix(
    prior: &GoldenVillageTargetGateReceipt,
    build: &BuildData,
    type_facts: GoldenWallLocalSeenTypeFacts,
    world: &map_terrain::World,
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
        || prior.type_index != golden_build_post_wall::GOLDEN_VILLAGE_TYPE
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
    if type_facts.is_wonder.type_index != prior.type_index {
        return Err(GoldenWallUpdateLocalSeenPrefixError::TypeFactsMismatch {
            expected: prior.type_index,
            actual: type_facts.is_wonder.type_index,
        });
    }
    let is_wonder = build_data_is_wonder(type_facts.is_wonder);
    let is_wonder_result = match is_wonder.exit {
        BuildDataIsWonderExit::Returned { is_wonder } => is_wonder,
        BuildDataIsWonderExit::DynamicTypeVirtual {
            jump_va,
            vtable_slot,
        } => {
            return Err(
                GoldenWallUpdateLocalSeenPrefixError::DynamicBuildTypeIsWonder {
                    jump_va,
                    vtable_slot,
                },
            )
        }
    };
    if is_wonder_result {
        return Err(GoldenWallUpdateLocalSeenPrefixError::GoldenVillageReportedAsWonder);
    }

    // `is_wonder == 0` jumps to 0x0063EDAD, selects explored-only mode, then
    // sign-extends ObjectData::visible and composes the post-journal ever-seen/owner mask.
    let visible_signed = i32::from(build.other[production::off::VISIBLE] as i8);
    let owner_bit = 1u8 << build.who;
    let player_mask = visible_signed | i32::from(planned_write.after) | i32::from(owner_bit);
    let mask = GoldenWallLocalSeenMask {
        visible_signed,
        ever_seen_at_entry: planned_write.after,
        owner_bit,
        player_mask,
        explored_only: true,
    };
    let footprint = type_facts.footprint;
    // Retail trusts its type/World invariants. This detached golden planner rejects
    // impossible dimensions rather than constructing an unbounded synthetic loop.
    if footprint.x_size <= 0
        || footprint.y_size <= 0
        || world.tile_xs <= 0
        || world.tile_ys <= 0
        || footprint.x_size > world.tile_xs
        || footprint.y_size > world.tile_ys
    {
        return Err(GoldenWallUpdateLocalSeenPrefixError::InvalidFootprint {
            x_size: footprint.x_size,
            y_size: footprint.y_size,
            tile_xs: world.tile_xs,
            tile_ys: world.tile_ys,
        });
    }
    let tile_corner = wall_data_tile_corner(build, footprint);
    let start_x = tile_corner
        .corner_x
        .checked_sub(1)
        .ok_or(GoldenWallUpdateLocalSeenPrefixError::FootprintRangeOverflow)?;
    let start_y = tile_corner
        .corner_y
        .checked_sub(1)
        .ok_or(GoldenWallUpdateLocalSeenPrefixError::FootprintRangeOverflow)?;
    let end_x = tile_corner
        .corner_x
        .checked_add(footprint.x_size)
        .ok_or(GoldenWallUpdateLocalSeenPrefixError::FootprintRangeOverflow)?;
    let end_y = tile_corner
        .corner_y
        .checked_add(footprint.y_size)
        .ok_or(GoldenWallUpdateLocalSeenPrefixError::FootprintRangeOverflow)?;
    let mut first_valid_cell = None;
    'outer: for tile_x in start_x..=end_x {
        for tile_y in start_y..=end_y {
            if world.valid_t(tile_x, tile_y) {
                first_valid_cell = Some(GoldenWallSetSeen2Call {
                    tile_x,
                    tile_y,
                    fog_x: tile_x >> 1,
                    fog_y: tile_y >> 1,
                    player_mask: mask.player_mask as u8,
                    explored_only: mask.explored_only,
                });
                break 'outer;
            }
        }
    }
    let continuation = if let Some(call) = first_valid_cell {
        GoldenWallUpdateLocalSeenContinuation::WorldSetSeen2 {
            call_va: WALL_UPDATE_LOCAL_SEEN_SET_SEEN2_CALL_VA,
            callee_va: WORLD_SET_SEEN2_VA,
            call,
        }
    } else {
        GoldenWallUpdateLocalSeenContinuation::NoValidFootprintCell {
            return_va: WALL_UPDATE_LOCAL_SEEN_RETURN_VA,
        }
    };

    Ok(GoldenWallUpdateLocalSeenPrefixReceipt {
        frame: prior.frame,
        actor: prior.actor,
        build_ordinal: prior.build_ordinal,
        owner: prior.owner,
        object_id: prior.object_id,
        uid: prior.uid,
        type_index: prior.type_index,
        caller_write_va: planned_write.instruction_va,
        caller_dispatch_va: call_va,
        receiver_vtable_va: GOLDEN_BUILD_VTABLE_VA,
        entry_va: WALL_UPDATE_LOCAL_SEEN_ENTRY_VA,
        journal: GoldenEverSeenJournal {
            write: planned_write,
            rollback: planned_write.before,
            state: GoldenJournalState::StagedNotPublished,
        },
        is_wonder_call_va: WALL_UPDATE_LOCAL_SEEN_FIRST_CHILD_CALL_VA,
        is_wonder_vtable_slot: WALL_UPDATE_LOCAL_SEEN_FIRST_CHILD_SLOT,
        is_wonder,
        mask,
        tile_corner_call_va: WALL_UPDATE_LOCAL_SEEN_TILE_CORNER_CALL_VA,
        tile_corner,
        world_tile_xs: world.tile_xs,
        world_tile_ys: world.tile_ys,
        continuation,
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

    const TEST_POSITION: i32 = 10 * map_terrain::COORD_PER_TILE;

    fn set_position(build: &mut BuildData, x: i32, y: i32) {
        build.other[production::off::X_INTERNAL..production::off::X_INTERNAL + 4]
            .copy_from_slice(&(x ^ 0x63637).to_le_bytes());
        build.other[production::off::Y_INTERNAL..production::off::Y_INTERNAL + 4]
            .copy_from_slice(&(y ^ 0x63637).to_le_bytes());
    }

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
        set_position(&mut build, TEST_POSITION, TEST_POSITION);
        build
    }

    fn world() -> map_terrain::World {
        map_terrain::World::init(20, 20, 44, 4, 4)
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

    fn test_village_type_facts() -> GoldenWallLocalSeenTypeFacts {
        GoldenWallLocalSeenTypeFacts {
            is_wonder: BuildDataIsWonderTypeFacts {
                type_index: GOLDEN_VILLAGE_TYPE,
                type_vtable_va: BUILD_TYPE_VTABLE_VA,
            },
            footprint: production::Footprint {
                x_size: 4,
                y_size: 4,
            },
        }
    }

    #[test]
    fn source_owned_tile_corner_stages_write_then_names_first_set_seen2() {
        let build = build();
        let world = world();
        let before_image = build.image();
        let gate = reached_gate(&build);
        let receipt =
            plan_wall_update_local_seen_prefix(&gate, &build, test_village_type_facts(), &world)
                .unwrap();

        assert_eq!(build.image(), before_image);
        assert_eq!(
            (receipt.actor, receipt.build_ordinal, receipt.object_id),
            (GoldenBuildActor::Village, 0, 2000)
        );
        assert_eq!(receipt.caller_write_va, 0x0061_fbcd);
        assert_eq!(receipt.caller_dispatch_va, 0x0061_fbd2);
        assert_eq!(receipt.entry_va, 0x0063_ed50);
        assert_eq!(receipt.type_index, GOLDEN_VILLAGE_TYPE);
        assert_eq!(receipt.journal.write.before, 0);
        assert_eq!(receipt.journal.write.after, 1);
        assert_eq!(receipt.journal.rollback, 0);
        assert_eq!(
            receipt.journal.state,
            GoldenJournalState::StagedNotPublished
        );
        assert_eq!(
            receipt.is_wonder,
            BuildDataIsWonderReceipt {
                function_va: 0x0047_2320,
                type_index: GOLDEN_VILLAGE_TYPE,
                type_vtable_va: 0x00b4_2b94,
                exit: BuildDataIsWonderExit::Returned { is_wonder: false },
                writes: 0,
                rng_draws: 0,
            }
        );
        assert_eq!(receipt.is_wonder_call_va, 0x0063_ed64);
        assert_eq!(receipt.is_wonder_vtable_slot, 0x2c);
        assert_eq!(
            receipt.mask,
            GoldenWallLocalSeenMask {
                visible_signed: 0,
                ever_seen_at_entry: 1,
                owner_bit: 1,
                player_mask: 1,
                explored_only: true,
            }
        );
        assert_eq!(receipt.tile_corner_call_va, 0x0063_edd6);
        assert_eq!((receipt.world_tile_xs, receipt.world_tile_ys), (80, 80));
        assert_eq!(
            receipt.tile_corner,
            WallDataTileCornerReceipt {
                function_va: 0x0064_3440,
                position_x: TEST_POSITION,
                position_y: TEST_POSITION,
                footprint: production::Footprint {
                    x_size: 4,
                    y_size: 4,
                },
                corner_x: 8,
                corner_y: 8,
                writes: 0,
                rng_draws: 0,
            }
        );
        assert_eq!(
            receipt.continuation,
            GoldenWallUpdateLocalSeenContinuation::WorldSetSeen2 {
                call_va: 0x0063_ee58,
                callee_va: 0x006b_4bb0,
                call: GoldenWallSetSeen2Call {
                    tile_x: 7,
                    tile_y: 7,
                    fog_x: 3,
                    fog_y: 3,
                    player_mask: 1,
                    explored_only: true,
                },
            }
        );
        assert_eq!(receipt.rng_draws, 0);
    }

    #[test]
    fn tile_corner_reproduces_even_odd_and_negative_coordinate_snapping() {
        let mut build = build();
        set_position(
            &mut build,
            TEST_POSITION + map_terrain::COORD_PER_TILE - 1,
            -1,
        );
        let even = wall_data_tile_corner(
            &build,
            production::Footprint {
                x_size: 4,
                y_size: 4,
            },
        );
        assert_eq!((even.corner_x, even.corner_y), (8, -3));
        let odd = wall_data_tile_corner(
            &build,
            production::Footprint {
                x_size: 3,
                y_size: 3,
            },
        );
        assert_eq!((odd.corner_x, odd.corner_y), (9, -2));
        assert_eq!((even.writes, even.rng_draws), (0, 0));
        assert_eq!((odd.writes, odd.rng_draws), (0, 0));
    }

    #[test]
    fn wholly_off_map_footprint_returns_without_a_world_child() {
        let mut build = build();
        set_position(
            &mut build,
            100 * map_terrain::COORD_PER_TILE,
            100 * map_terrain::COORD_PER_TILE,
        );
        let gate = reached_gate(&build);
        let receipt =
            plan_wall_update_local_seen_prefix(&gate, &build, test_village_type_facts(), &world())
                .unwrap();
        assert_eq!(
            receipt.continuation,
            GoldenWallUpdateLocalSeenContinuation::NoValidFootprintCell {
                return_va: 0x0063_ee98,
            }
        );
        assert_eq!(receipt.journal.rollback, build.ever_seen);
        assert_eq!(
            receipt.journal.state,
            GoldenJournalState::StagedNotPublished
        );
    }

    #[test]
    fn impossible_footprint_fails_before_the_tile_loop() {
        let build = build();
        let gate = reached_gate(&build);
        let world = world();
        let mut type_facts = test_village_type_facts();
        type_facts.footprint.x_size = 0;
        assert_eq!(
            plan_wall_update_local_seen_prefix(&gate, &build, type_facts, &world),
            Err(GoldenWallUpdateLocalSeenPrefixError::InvalidFootprint {
                x_size: 0,
                y_size: 4,
                tile_xs: 80,
                tile_ys: 80,
            })
        );
    }

    #[test]
    fn build_data_is_wonder_range_and_dynamic_tail_are_exact() {
        let ordinary = |type_index| {
            build_data_is_wonder(BuildDataIsWonderTypeFacts {
                type_index,
                type_vtable_va: BUILD_TYPE_VTABLE_VA,
            })
            .exit
        };
        assert_eq!(
            ordinary(WONDER_TYPE_FIRST - 1),
            BuildDataIsWonderExit::Returned { is_wonder: false }
        );
        assert_eq!(
            ordinary(WONDER_TYPE_FIRST),
            BuildDataIsWonderExit::Returned { is_wonder: true }
        );
        assert_eq!(
            ordinary(WONDER_TYPE_END - 1),
            BuildDataIsWonderExit::Returned { is_wonder: true }
        );
        assert_eq!(
            ordinary(WONDER_TYPE_END),
            BuildDataIsWonderExit::Returned { is_wonder: false }
        );
        assert_eq!(
            build_data_is_wonder(BuildDataIsWonderTypeFacts {
                type_index: GOLDEN_VILLAGE_TYPE,
                type_vtable_va: 0x1234_5678,
            })
            .exit,
            BuildDataIsWonderExit::DynamicTypeVirtual {
                jump_va: 0x0047_2349,
                vtable_slot: 0x1c,
            }
        );
    }

    #[test]
    fn changed_receiver_refuses_the_staged_journal() {
        let mut build = build();
        let gate = reached_gate(&build);
        build.ever_seen = 1;
        assert_eq!(
            plan_wall_update_local_seen_prefix(&gate, &build, test_village_type_facts(), &world()),
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
            plan_wall_update_local_seen_prefix(
                &ordinary_team,
                &build,
                test_village_type_facts(),
                &world(),
            ),
            Err(GoldenWallUpdateLocalSeenPrefixError::PriorDidNotReachUpdateLocalSeen)
        );
    }
}
