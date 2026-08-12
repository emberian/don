//! Exact East Meets West caller transaction for `Map::check_player_land`.
//!
//! This module begins at the native loop-exit edge after every active player
//! has a starting location.  The 1,145-byte leaf itself is owned by
//! [`crate::player_land`]; this layer fixes the style's fastcall arguments,
//! proves that the leaf consumes no RNG, records the only legal World/Regions
//! mutations, and leaves a typed post-call cleanup residual.

use crate::player_land::{
    execute_check_player_land, CheckPlayerLandCall, CheckPlayerLandError, CheckPlayerLandReceipt,
    MAP_CHECK_PLAYER_LAND_VA,
};
use don_sim::systems::map_terrain::{WData, World, WorldChecksum, WorldSection};
use don_sim::systems::regions::{Region, Regions, REGION_COUNT};

pub const EAST_MEETS_WEST_PLAYER_LAND_CALLER_ENTRY_VA: u32 = 0x0069_7484;
pub const EAST_MEETS_WEST_PLAYER_LAND_CALL_VA: u32 = 0x0069_7492;
pub const EAST_MEETS_WEST_PLAYER_LAND_RESUME_VA: u32 = 0x0069_7497;
pub const EAST_MEETS_WEST_PLAYER_LAND_GUARD_STORE_VA: u32 = 0x0069_749a;
pub const EAST_MEETS_WEST_PLAYER_LAND_STRING_CLOSE_CALL_VA: u32 = 0x0069_74a1;
pub const STRING_CLOSE_VA: u32 = 0x00a1_cf40;

pub const MAP_CHECK_PLAYER_LAND_END_VA: u32 = 0x0068_f379;
pub const MAP_CHECK_PLAYER_LAND_RET_VA: u32 = 0x0068_f378;
pub const MAP_CHECK_PLAYER_LAND_SIZE: u32 = 1_145;
pub const MAP_CHECK_PLAYER_LAND_INSTRUCTION_COUNT: u32 = 301;
pub const MAP_CHECK_PLAYER_LAND_SHA256: &str =
    "9577c20dbe3e6cd13409355c2f45983cbc01e573c30e3b0e16958967fb109140";
pub const RISE_EXE_SHA256: &str =
    "30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079";

pub const ARRAY_WCOORD_REMOVE_CALL_VA: u32 = 0x0068_f089;
pub const ARRAY_WCOORD_REMOVE_VA: u32 = 0x0046_d4f0;
pub const SEED_ARRAY_GROW_INDIRECT_CALL_VA: u32 = 0x0068_f0cf;
pub const OUTER_ARRAY_GROW_INDIRECT_CALL_VA: u32 = 0x0068_f2ea;

/// Frozen instruction-level provenance for the complete native body.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct CheckPlayerLandNativeBody {
    pub entry_va: u32,
    pub end_va_exclusive: u32,
    pub ret_va: u32,
    pub size: u32,
    pub instruction_count: u32,
    pub sha256: &'static str,
    pub direct_rng_sites: &'static [u32],
    pub direct_calls: &'static [(u32, u32)],
    pub indirect_array_grow_calls: &'static [u32],
}

pub const CHECK_PLAYER_LAND_NATIVE_BODY: CheckPlayerLandNativeBody = CheckPlayerLandNativeBody {
    entry_va: MAP_CHECK_PLAYER_LAND_VA,
    end_va_exclusive: MAP_CHECK_PLAYER_LAND_END_VA,
    ret_va: MAP_CHECK_PLAYER_LAND_RET_VA,
    size: MAP_CHECK_PLAYER_LAND_SIZE,
    instruction_count: MAP_CHECK_PLAYER_LAND_INSTRUCTION_COUNT,
    sha256: MAP_CHECK_PLAYER_LAND_SHA256,
    direct_rng_sites: &[],
    direct_calls: &[(ARRAY_WCOORD_REMOVE_CALL_VA, ARRAY_WCOORD_REMOVE_VA)],
    indirect_array_grow_calls: &[
        SEED_ARRAY_GROW_INDIRECT_CALL_VA,
        OUTER_ARRAY_GROW_INDIRECT_CALL_VA,
    ],
};

/// Values live at the exact caller edge.  `unread_stack_word` is retained
/// because retail pushes ECX at `0x00697487`, although the leaf never reads
/// its fourth formal parameter.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct EastMeetsWestPlayerLandCall {
    pub expected_start_count: usize,
    pub avoid_continent: i32,
    pub unread_stack_word: i32,
    pub random_state: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegionMutation {
    pub region: u8,
    pub before: Region,
    pub after: Region,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorldCellMutation {
    pub x: i32,
    pub y: i32,
    pub before: WData,
    pub after: WData,
}

/// The native leaf is `void`; execution resumes in the style virtual and its
/// first still-unported work is local-string cleanup.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum EastMeetsWestPlayerLandNext {
    PostCallCleanup {
        entry_va: u32,
        guard_store_va: u32,
        string_close_call_va: u32,
        string_close_va: u32,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EastMeetsWestPlayerLandReceipt {
    pub caller_entry_va: u32,
    pub call_va: u32,
    pub body: CheckPlayerLandNativeBody,
    pub native_call: CheckPlayerLandCall,
    pub native_return: (),
    pub body_receipt: CheckPlayerLandReceipt,
    pub random_state_before: i32,
    pub random_state_after: i32,
    pub direct_rng_sites: Vec<u32>,
    pub world_before: WorldChecksum,
    pub world_after: WorldChecksum,
    pub world_sections_changed: Vec<WorldSection>,
    pub start_arrays_checksum_before: u32,
    pub start_arrays_checksum_after: u32,
    pub world_cell_mutations: Vec<WorldCellMutation>,
    pub region_mutations: Vec<RegionMutation>,
    pub next: EastMeetsWestPlayerLandNext,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EastMeetsWestPlayerLandError {
    StartArrayShape {
        expected_starts: usize,
        start_x: usize,
        start_y: usize,
        start_city_x: usize,
        start_city_y: usize,
    },
    PlayerLand(CheckPlayerLandError),
    UnexpectedWorldMutation {
        sections: Vec<WorldSection>,
    },
    UnexpectedRegionsContainerMutation,
}

/// Execute the exact `0x00697484..0x00697497` East Meets West continuation.
///
/// The native call is `check_player_land(1, Map+0x30, -1, unread)`: `-1`
/// selects the leaf's radius-five default, while the exclusion radius is
/// capped to ten inside the leaf.  No RNG object is passed by native code, so
/// the supplied chronology word is copied through unchanged and the receipt's
/// direct-site list is necessarily empty.
pub fn execute_east_meets_west_player_land(
    world: &mut World,
    regions: &mut Regions,
    call: EastMeetsWestPlayerLandCall,
) -> Result<EastMeetsWestPlayerLandReceipt, EastMeetsWestPlayerLandError> {
    let starts = call.expected_start_count;
    if world.start_x.items.len() != starts
        || world.start_y.items.len() != starts
        || world.start_city_x.items.len() != starts.saturating_mul(4)
        || world.start_city_y.items.len() != starts.saturating_mul(4)
    {
        return Err(EastMeetsWestPlayerLandError::StartArrayShape {
            expected_starts: starts,
            start_x: world.start_x.items.len(),
            start_y: world.start_y.items.len(),
            start_city_x: world.start_city_x.items.len(),
            start_city_y: world.start_city_y.items.len(),
        });
    }

    let world_before = world.checksum_sections();
    let wdata_before = world.wdata.clone();
    let regions_before = regions.clone();
    let native_call = CheckPlayerLandCall {
        enabled: 1,
        avoid_continent: call.avoid_continent,
        radius: -1,
        unused: call.unread_stack_word,
    };
    let body_receipt = execute_check_player_land(world, regions, native_call)
        .map_err(EastMeetsWestPlayerLandError::PlayerLand)?;
    let world_after = world.checksum_sections();
    let world_sections_changed = world_before.differing_sections(&world_after);
    if world_sections_changed
        .iter()
        .any(|&section| section != WorldSection::WData)
    {
        return Err(EastMeetsWestPlayerLandError::UnexpectedWorldMutation {
            sections: world_sections_changed,
        });
    }
    if regions_before.sea != regions.sea
        || regions_before.land != regions.land
        || regions_before.coords != regions.coords
    {
        return Err(EastMeetsWestPlayerLandError::UnexpectedRegionsContainerMutation);
    }

    let region_mutations = (0..REGION_COUNT)
        .filter(|&index| regions_before.list[index] != regions.list[index])
        .map(|index| RegionMutation {
            region: index as u8,
            before: regions_before.list[index].clone(),
            after: regions.list[index].clone(),
        })
        .collect();
    let world_cell_mutations = wdata_before
        .into_iter()
        .zip(world.wdata.iter().cloned())
        .enumerate()
        .filter(|(_, (before, after))| before != after)
        .map(|(index, (before, after))| WorldCellMutation {
            x: index as i32 % world.xs,
            y: index as i32 / world.xs,
            before,
            after,
        })
        .collect();

    Ok(EastMeetsWestPlayerLandReceipt {
        caller_entry_va: EAST_MEETS_WEST_PLAYER_LAND_CALLER_ENTRY_VA,
        call_va: EAST_MEETS_WEST_PLAYER_LAND_CALL_VA,
        body: CHECK_PLAYER_LAND_NATIVE_BODY,
        native_call,
        native_return: (),
        body_receipt,
        random_state_before: call.random_state,
        random_state_after: call.random_state,
        direct_rng_sites: Vec::new(),
        start_arrays_checksum_before: world_before.section(WorldSection::StartArrays).adler,
        start_arrays_checksum_after: world_after.section(WorldSection::StartArrays).adler,
        world_cell_mutations,
        world_before,
        world_after,
        world_sections_changed,
        region_mutations,
        next: EastMeetsWestPlayerLandNext::PostCallCleanup {
            entry_va: EAST_MEETS_WEST_PLAYER_LAND_RESUME_VA,
            guard_store_va: EAST_MEETS_WEST_PLAYER_LAND_GUARD_STORE_VA,
            string_close_call_va: EAST_MEETS_WEST_PLAYER_LAND_STRING_CLOSE_CALL_VA,
            string_close_va: STRING_CLOSE_VA,
        },
    })
}
