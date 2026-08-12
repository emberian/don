//! Executable common `Map::make` tail through coastline reconstruction.
//!
//! Every supported style virtual returns into the same sequence:
//! `Regions::clear_all/find_all`, copy the six Map territory fields into
//! `World`, `Map::fix_diag_land`, `Map::make_coastlines`, then rebuild the
//! regions again.  The next call is `TerrainGroups::fill_fertile`.

use don_sim::systems::map_terrain::{land, wflag, WData, World, WorldChecksum, WorldSection};
use don_sim::systems::regions::{
    make_coastlines_and_rebuild_regions, Region, RegionBuildReceipt, Regions, RegionsError,
    WCoordList, REGION_COUNT,
};

pub const REGIONS_CLEAR_ALL_VA: u32 = 0x0068_0060;
pub const REGIONS_FIND_ALL_VA: u32 = 0x0067_eff0;
pub const REGIONS_FIND_ALL_END_VA: u32 = 0x0067_f7c9;
pub const REGIONS_FIND_ALL_SIZE: u32 = 2_009;
pub const REGIONS_FIND_ALL_INSTRUCTION_COUNT: u32 = 558;
pub const REGIONS_FIND_ALL_SHA256: &str =
    "3e356054293f63e1be4b36681473d000c1ec5bd6e4eb3fa368d8fce98b2cc1cd";
pub const REGIONS_FIND_ALL_RET_VA: u32 = 0x0067_f7c6;
pub const REGIONS_FIND_ALL_OLD_SCRATCH_FREE_CALL_VA: u32 = 0x0067_f042;
pub const REGIONS_FIND_ALL_SCRATCH_MALLOC_CALL_VA: u32 = 0x0067_f06b;
pub const REGIONS_FIND_ALL_STRING_CONSTRUCTOR_VA: u32 = 0x00a1_d660;
pub const REGIONS_FIND_ALL_ERROR_REPORT_VA: u32 = 0x00a2_e550;
pub const REGIONS_FIND_ALL_STRING_CLOSE_VA: u32 = 0x00a1_cf40;
pub const REGIONS_FIND_VA: u32 = 0x0068_0180;
pub const REGIONS_FIND_ALL_FIND_CALL_VA: u32 = 0x0067_f549;
pub const REGIONS_SET_COASTALS_VA: u32 = 0x0067_fd70;
pub const REGIONS_SORT_REGIONS_VA: u32 = 0x0067_fb90;
pub const REGIONS_REBUILD_COORDS_VA: u32 = 0x0067_f800;
pub const DO_ALL_NON_INPUT_VA: u32 = 0x0053_8810;
pub const REGIONS_FIND_ALL_SET_COASTALS_CALL_VA: u32 = 0x0067_f774;
pub const REGIONS_FIND_ALL_SORT_REGIONS_CALL_VA: u32 = 0x0067_f77b;
pub const REGIONS_FIND_ALL_FINAL_SCRATCH_FREE_CALL_VA: u32 = 0x0067_f788;
pub const REGIONS_FIND_ALL_REBUILD_COORDS_CALL_VA: u32 = 0x0067_f7ac;
pub const REGIONS_FIND_ALL_NON_INPUT_CALL_VA: u32 = 0x0067_f7b1;
pub const REGIONS_CLEAR_ALL_END_VA: u32 = 0x0068_0173;
pub const REGIONS_CLEAR_ALL_SIZE: u32 = 275;
pub const REGIONS_CLEAR_ALL_INSTRUCTION_COUNT: u32 = 79;
pub const REGIONS_CLEAR_ALL_SHA256: &str =
    "86333bac51dacb209b15048ecb33aa57b77c2d143a1ad797631e1992994b7317";
pub const REGIONS_CLEAR_ALL_RET_NONEMPTY_WORLD_VA: u32 = 0x0068_014e;
pub const REGIONS_CLEAR_ALL_RET_EMPTY_WORLD_VA: u32 = 0x0068_015e;
pub const REGIONS_CLEAR_ALL_RET_NULL_WORLD_DATA_VA: u32 = 0x0068_0172;
pub const REGIONS_CLEAR_ALL_COORD_FREE_CALL_VA: u32 = 0x0068_00c5;
pub const FREE_IMPORT_IAT_VA: u32 = 0x00ac_5500;
pub const MALLOC_IMPORT_IAT_VA: u32 = 0x00ac_54f0;
pub const MAP_MAKE_FIRST_REGIONS_CLEAR_CALL_VA: u32 = 0x0068_be36;
pub const MAP_MAKE_FIRST_REGIONS_CLEAR_RESUME_VA: u32 = 0x0068_be3b;
pub const MAP_MAKE_FIRST_REGIONS_FIND_ARGUMENT_PUSH_VA: u32 = 0x0068_be3b;
pub const MAP_MAKE_FIRST_REGIONS_FIND_CALL_VA: u32 = 0x0068_be3c;
pub const MAP_MAKE_FIRST_REGIONS_FIND_RESUME_VA: u32 = 0x0068_be41;
pub const MAP_MAKE_FIRST_REGIONS_FIND_CALLER_END_VA: u32 = MAP_MAKE_FIRST_REGIONS_FIND_RESUME_VA;
pub const MAP_MAKE_FIRST_REGIONS_FIND_CALLER_SIZE: u32 = 6;
pub const MAP_MAKE_FIRST_REGIONS_FIND_CALLER_INSTRUCTION_COUNT: u32 = 2;
pub const MAP_MAKE_FIRST_REGIONS_FIND_CALLER_SHA256: &str =
    "ff5c2df80ce0958c86143fb098cb30488633cc03971c09d64b1bf9f918a9b1da";
pub const MAP_MAKE_FIRST_TERRITORY_WORLD_LOAD_VA: u32 = 0x0068_be41;
pub const MAP_MAKE_FIRST_TERRITORY_MAP_LOAD_VA: u32 = 0x0068_be47;
pub const MAP_MAKE_FIRST_TERRITORY_STORE_VA: u32 = 0x0068_be4a;
pub const MAP_MAKE_FIRST_TERRITORY_PREP_END_VA: u32 = MAP_MAKE_FIRST_TERRITORY_STORE_VA;
pub const MAP_MAKE_FIRST_TERRITORY_PREP_SIZE: u32 = 9;
pub const MAP_MAKE_FIRST_TERRITORY_PREP_INSTRUCTION_COUNT: u32 = 2;
pub const MAP_MAKE_FIRST_TERRITORY_PREP_SHA256: &str =
    "cf0aa5dda08cbce5603defb8fddc99471fdbb7d6f4bd348812170223b59ea05f";
pub const MAP_PLAYER_TERRITORY_LIMIT_OFFSET: u32 = 0x4c;
pub const WORLD_PLAYER_TERRITORY_LIMIT_OFFSET: u32 = 0x38;
pub const MAP_MAKE_TERRITORY_LIMITS_END_VA: u32 = 0x0068_be75;
pub const MAP_MAKE_TERRITORY_LIMITS_SIZE: u32 = 43;
pub const MAP_MAKE_TERRITORY_LIMITS_INSTRUCTION_COUNT: u32 = 13;
pub const MAP_MAKE_TERRITORY_LIMITS_SHA256: &str =
    "9cd159ea34e9aeebc982230cc0a6234d628c2b4588e0e01261611d85bf0cb5fa";
pub const MAP_MAKE_TERRITORY_LIMIT_LOAD_VAS: [u32; 6] = [
    0x0068_be47,
    0x0068_be4d,
    0x0068_be53,
    0x0068_be59,
    0x0068_be5f,
    0x0068_be65,
];
pub const MAP_MAKE_TERRITORY_LIMIT_STORE_VAS: [u32; 6] = [
    0x0068_be4a,
    0x0068_be50,
    0x0068_be56,
    0x0068_be5c,
    0x0068_be62,
    0x0068_be68,
];
pub const MAP_TERRITORY_LIMIT_OFFSETS: [u32; 6] = [0x4c, 0x50, 0x54, 0x58, 0x5c, 0x60];
pub const WORLD_TERRITORY_LIMIT_OFFSETS: [u32; 6] = [0x38, 0x3c, 0x40, 0x44, 0x48, 0x4c];
pub const MAP_MAKE_STYLE_COMPARE_VA: u32 = 0x0068_be6b;
pub const MAP_MAKE_STYLE_BRANCH_VA: u32 = 0x0068_be6f;
pub const MAP_MAKE_STYLE_BRANCH_VALUE: u8 = 23;
pub const MAP_MAKE_STYLE_BRANCH_TARGET_VA: u32 = 0x0068_c84a;
pub const MAP_MAKE_FIRST_FIX_DIAG_LAND_CALL_VA: u32 = 0x0068_be75;
pub const MAP_FIX_DIAG_LAND_VA: u32 = 0x0069_c250;
pub const MAP_FIX_DIAG_LAND_END_VA: u32 = 0x0069_c458;
pub const MAP_FIX_DIAG_LAND_RET_VA: u32 = 0x0069_c457;
pub const MAP_FIX_DIAG_LAND_SIZE: u32 = 520;
pub const MAP_FIX_DIAG_LAND_INSTRUCTION_COUNT: u32 = 173;
pub const MAP_FIX_DIAG_LAND_SHA256: &str =
    "cf6610b0c5d5df3e5bfeb40010cdf729e587f69cdf1c3c3eaefd8c72c4fe65dd";
pub const MAP_FIX_DIAG_LAND_CORNER_X_VA: u32 = 0x00ad_c3c4;
pub const MAP_FIX_DIAG_LAND_CORNER_Y_VA: u32 = 0x00ad_c3e4;
pub const MAP_FIX_DIAG_LAND_CORNER_X: [i32; 4] = [-1, 1, 1, -1];
pub const MAP_FIX_DIAG_LAND_CORNER_Y: [i32; 4] = [-1, -1, 1, 1];
pub const MAP_MAKE_FIX_DIAG_LAND_RESUME_VA: u32 = 0x0068_be7a;
pub const MAP_MAKE_POST_FIX_DIAG_STRING_LITERAL_PUSH_VA: u32 = 0x0068_be7a;
pub const MAP_MAKE_POST_FIX_DIAG_STRING_LOCAL_LOAD_VA: u32 = 0x0068_be7f;
pub const MAP_MAKE_POST_FIX_DIAG_STRING_CONSTRUCTOR_CALL_VA: u32 = 0x0068_be82;
pub const STRING_CONSTRUCTOR_VA: u32 = 0x00a1_d660;
pub const MAP_MAKE_COASTLINES_VA: u32 = 0x0069_47a0;
pub const TERRAIN_GROUPS_FILL_FERTILE_VA: u32 = 0x006a_6f90;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct RegionsClearAllNativeBody {
    pub entry_va: u32,
    pub end_va_exclusive: u32,
    pub ret_vas: &'static [u32],
    pub size: u32,
    pub instruction_count: u32,
    pub sha256: &'static str,
    pub direct_calls: &'static [(u32, u32)],
    pub indirect_import_calls: &'static [(u32, u32)],
}

pub const REGIONS_CLEAR_ALL_NATIVE_BODY: RegionsClearAllNativeBody = RegionsClearAllNativeBody {
    entry_va: REGIONS_CLEAR_ALL_VA,
    end_va_exclusive: REGIONS_CLEAR_ALL_END_VA,
    ret_vas: &[
        REGIONS_CLEAR_ALL_RET_NONEMPTY_WORLD_VA,
        REGIONS_CLEAR_ALL_RET_EMPTY_WORLD_VA,
        REGIONS_CLEAR_ALL_RET_NULL_WORLD_DATA_VA,
    ],
    size: REGIONS_CLEAR_ALL_SIZE,
    instruction_count: REGIONS_CLEAR_ALL_INSTRUCTION_COUNT,
    sha256: REGIONS_CLEAR_ALL_SHA256,
    direct_calls: &[],
    indirect_import_calls: &[(REGIONS_CLEAR_ALL_COORD_FREE_CALL_VA, FREE_IMPORT_IAT_VA)],
};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct RegionsFindAllNativeBody {
    pub entry_va: u32,
    pub end_va_exclusive: u32,
    pub ret_va: u32,
    pub callee_stack_argument_bytes_popped: u8,
    pub size: u32,
    pub instruction_count: u32,
    pub sha256: &'static str,
    pub direct_calls: &'static [(u32, u32)],
    pub indirect_import_calls: &'static [(u32, u32)],
    pub indirect_virtual_calls: &'static [u32],
}

pub const REGIONS_FIND_ALL_NATIVE_BODY: RegionsFindAllNativeBody = RegionsFindAllNativeBody {
    entry_va: REGIONS_FIND_ALL_VA,
    end_va_exclusive: REGIONS_FIND_ALL_END_VA,
    ret_va: REGIONS_FIND_ALL_RET_VA,
    callee_stack_argument_bytes_popped: 4,
    size: REGIONS_FIND_ALL_SIZE,
    instruction_count: REGIONS_FIND_ALL_INSTRUCTION_COUNT,
    sha256: REGIONS_FIND_ALL_SHA256,
    direct_calls: &[
        (0x0067_f252, REGIONS_FIND_ALL_STRING_CONSTRUCTOR_VA),
        (0x0067_f26f, REGIONS_FIND_ALL_ERROR_REPORT_VA),
        (0x0067_f27d, REGIONS_FIND_ALL_STRING_CLOSE_VA),
        (0x0067_f28c, REGIONS_FIND_ALL_STRING_CLOSE_VA),
        (0x0067_f4e0, REGIONS_FIND_ALL_STRING_CONSTRUCTOR_VA),
        (0x0067_f500, REGIONS_FIND_ALL_ERROR_REPORT_VA),
        (0x0067_f511, REGIONS_FIND_ALL_STRING_CLOSE_VA),
        (0x0067_f520, REGIONS_FIND_ALL_STRING_CLOSE_VA),
        (REGIONS_FIND_ALL_FIND_CALL_VA, REGIONS_FIND_VA),
        (
            REGIONS_FIND_ALL_SET_COASTALS_CALL_VA,
            REGIONS_SET_COASTALS_VA,
        ),
        (
            REGIONS_FIND_ALL_SORT_REGIONS_CALL_VA,
            REGIONS_SORT_REGIONS_VA,
        ),
        (
            REGIONS_FIND_ALL_REBUILD_COORDS_CALL_VA,
            REGIONS_REBUILD_COORDS_VA,
        ),
        (REGIONS_FIND_ALL_NON_INPUT_CALL_VA, DO_ALL_NON_INPUT_VA),
    ],
    indirect_import_calls: &[
        (
            REGIONS_FIND_ALL_OLD_SCRATCH_FREE_CALL_VA,
            FREE_IMPORT_IAT_VA,
        ),
        (
            REGIONS_FIND_ALL_SCRATCH_MALLOC_CALL_VA,
            MALLOC_IMPORT_IAT_VA,
        ),
        (
            REGIONS_FIND_ALL_FINAL_SCRATCH_FREE_CALL_VA,
            FREE_IMPORT_IAT_VA,
        ),
    ],
    indirect_virtual_calls: &[0x0067_f2f5],
};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MapMakeFirstRegionsFindAllCallerBody {
    pub entry_va: u32,
    pub end_va_exclusive: u32,
    pub size: u32,
    pub instruction_count: u32,
    pub sha256: &'static str,
    pub direct_calls: &'static [(u32, u32)],
}

pub const MAP_MAKE_FIRST_REGIONS_FIND_ALL_CALLER_BODY: MapMakeFirstRegionsFindAllCallerBody =
    MapMakeFirstRegionsFindAllCallerBody {
        entry_va: MAP_MAKE_FIRST_REGIONS_FIND_ARGUMENT_PUSH_VA,
        end_va_exclusive: MAP_MAKE_FIRST_REGIONS_FIND_CALLER_END_VA,
        size: MAP_MAKE_FIRST_REGIONS_FIND_CALLER_SIZE,
        instruction_count: MAP_MAKE_FIRST_REGIONS_FIND_CALLER_INSTRUCTION_COUNT,
        sha256: MAP_MAKE_FIRST_REGIONS_FIND_CALLER_SHA256,
        direct_calls: &[(MAP_MAKE_FIRST_REGIONS_FIND_CALL_VA, REGIONS_FIND_ALL_VA)],
    };

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MapMakeFirstTerritoryPrepBody {
    pub entry_va: u32,
    pub end_va_exclusive: u32,
    pub size: u32,
    pub instruction_count: u32,
    pub sha256: &'static str,
}

pub const MAP_MAKE_FIRST_TERRITORY_PREP_BODY: MapMakeFirstTerritoryPrepBody =
    MapMakeFirstTerritoryPrepBody {
        entry_va: MAP_MAKE_FIRST_TERRITORY_WORLD_LOAD_VA,
        end_va_exclusive: MAP_MAKE_FIRST_TERRITORY_PREP_END_VA,
        size: MAP_MAKE_FIRST_TERRITORY_PREP_SIZE,
        instruction_count: MAP_MAKE_FIRST_TERRITORY_PREP_INSTRUCTION_COUNT,
        sha256: MAP_MAKE_FIRST_TERRITORY_PREP_SHA256,
    };

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RegionsAllocationState {
    Unallocated,
    Live,
    Freed,
}

/// One logical `WCoordList` allocation released by the loop's imported
/// `_free`. Native pointer identity is intentionally not represented.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegionsClearAllCoordFreeReceipt {
    pub call_va: u32,
    pub import_iat_va: u32,
    pub region: u8,
    pub elements: Vec<(i32, i32)>,
    pub capacity: i32,
    pub element_width: u8,
    pub state_before: RegionsAllocationState,
    pub state_after: RegionsAllocationState,
    pub native_return: (),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegionsClearAllRegionReceipt {
    pub region: u8,
    pub before: Region,
    pub after: Region,
    pub size_nonzero_path: bool,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct RegionsClearAllWorldMutation {
    pub cell: usize,
    pub region_before: i16,
    pub region_after: i16,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MapMakeFirstRegionsClearAllNext {
    FindAll {
        argument_push_va: u32,
        call_va: u32,
        primitive_va: u32,
        stack_argument_is_unread: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapMakeFirstRegionsClearAllReceipt {
    pub caller_call_va: u32,
    pub body: RegionsClearAllNativeBody,
    pub executed_ret_va: u32,
    pub caller_resume_va: u32,
    pub region_records_visited: usize,
    pub region_records: Vec<RegionsClearAllRegionReceipt>,
    pub coordinate_frees: Vec<RegionsClearAllCoordFreeReceipt>,
    pub regions_coords_before: WCoordList,
    pub regions_coords_after: WCoordList,
    pub regions_land_before: i32,
    pub regions_land_after: i32,
    pub regions_sea_before: i32,
    pub regions_sea_after: i32,
    pub world_cells_visited: usize,
    pub world_region_mutations: Vec<RegionsClearAllWorldMutation>,
    pub world_region2_unchanged: bool,
    pub world_before: WorldChecksum,
    pub world_after: WorldChecksum,
    pub world_sections_changed: Vec<WorldSection>,
    pub random_state_before: i32,
    pub random_state_after: i32,
    pub direct_rng_sites: Vec<u32>,
    pub next: MapMakeFirstRegionsClearAllNext,
}

/// Execute the first common-driver `Regions::clear_all` after a style virtual
/// returns, and freeze before `Regions::find_all(int)`.
pub fn execute_map_make_first_regions_clear_all(
    world: &mut World,
    regions: &mut Regions,
    random_state: i32,
) -> MapMakeFirstRegionsClearAllReceipt {
    let world_before = world.checksum_sections();
    let region2_before = world
        .wdata
        .iter()
        .map(|cell| cell.region2)
        .collect::<Vec<_>>();
    let region_labels_before = world
        .wdata
        .iter()
        .map(|cell| cell.region)
        .collect::<Vec<_>>();
    let regions_before = regions.clone();

    regions.clear_all(world);

    let world_after = world.checksum_sections();
    let region_records = regions_before
        .list
        .iter()
        .zip(regions.list.iter())
        .enumerate()
        .map(|(region, (before, after))| RegionsClearAllRegionReceipt {
            region: region as u8,
            before: before.clone(),
            after: after.clone(),
            size_nonzero_path: before.size != 0,
        })
        .collect();
    let coordinate_frees = regions_before
        .list
        .iter()
        .enumerate()
        .filter(|(_, region)| region.size != 0 && region.coords.capacity != 0)
        .map(|(region, record)| RegionsClearAllCoordFreeReceipt {
            call_va: REGIONS_CLEAR_ALL_COORD_FREE_CALL_VA,
            import_iat_va: FREE_IMPORT_IAT_VA,
            region: region as u8,
            elements: record.coords.items.clone(),
            capacity: record.coords.capacity,
            element_width: 8,
            state_before: RegionsAllocationState::Live,
            state_after: RegionsAllocationState::Freed,
            native_return: (),
        })
        .collect();
    let world_region_mutations = region_labels_before
        .into_iter()
        .zip(world.wdata.iter().map(|cell| cell.region))
        .enumerate()
        .filter_map(|(cell, (region_before, region_after))| {
            (region_before != region_after).then_some(RegionsClearAllWorldMutation {
                cell,
                region_before,
                region_after,
            })
        })
        .collect();
    let world_sections_changed = WorldSection::all()
        .into_iter()
        .filter(|section| world_before.section(*section) != world_after.section(*section))
        .collect();

    MapMakeFirstRegionsClearAllReceipt {
        caller_call_va: MAP_MAKE_FIRST_REGIONS_CLEAR_CALL_VA,
        body: REGIONS_CLEAR_ALL_NATIVE_BODY,
        executed_ret_va: REGIONS_CLEAR_ALL_RET_NONEMPTY_WORLD_VA,
        caller_resume_va: MAP_MAKE_FIRST_REGIONS_CLEAR_RESUME_VA,
        region_records_visited: REGION_COUNT,
        region_records,
        coordinate_frees,
        regions_coords_before: regions_before.coords.clone(),
        regions_coords_after: regions.coords.clone(),
        regions_land_before: regions_before.land,
        regions_land_after: regions.land,
        regions_sea_before: regions_before.sea,
        regions_sea_after: regions.sea,
        world_cells_visited: world.wdata.len(),
        world_region_mutations,
        world_region2_unchanged: region2_before
            == world
                .wdata
                .iter()
                .map(|cell| cell.region2)
                .collect::<Vec<_>>(),
        world_before,
        world_after,
        world_sections_changed,
        random_state_before: random_state,
        random_state_after: random_state,
        direct_rng_sites: Vec::new(),
        next: MapMakeFirstRegionsClearAllNext::FindAll {
            argument_push_va: MAP_MAKE_FIRST_REGIONS_FIND_ARGUMENT_PUSH_VA,
            call_va: MAP_MAKE_FIRST_REGIONS_FIND_CALL_VA,
            primitive_va: REGIONS_FIND_ALL_VA,
            stack_argument_is_unread: true,
        },
    }
}

pub(crate) fn validate_map_make_first_regions_clear_all_receipt(
    world: &World,
    regions: &Regions,
    receipt: &MapMakeFirstRegionsClearAllReceipt,
) -> bool {
    if receipt.caller_call_va != MAP_MAKE_FIRST_REGIONS_CLEAR_CALL_VA
        || receipt.body != REGIONS_CLEAR_ALL_NATIVE_BODY
        || receipt.executed_ret_va != REGIONS_CLEAR_ALL_RET_NONEMPTY_WORLD_VA
        || receipt.caller_resume_va != MAP_MAKE_FIRST_REGIONS_CLEAR_RESUME_VA
        || receipt.region_records_visited != REGION_COUNT
        || receipt.region_records.len() != REGION_COUNT
        || receipt.regions_coords_before != receipt.regions_coords_after
        || receipt.regions_coords_after != regions.coords
        || receipt.regions_land_after != 0
        || receipt.regions_land_after != regions.land
        || receipt.regions_sea_after != 64
        || receipt.regions_sea_after != regions.sea
        || receipt.world_cells_visited != world.wdata.len()
        || !receipt.world_region2_unchanged
        || world.wdata.iter().any(|cell| cell.region != 0)
        || receipt.world_after != world.checksum_sections()
        || receipt.world_sections_changed
            != receipt
                .world_before
                .differing_sections(&receipt.world_after)
        || receipt.random_state_before != receipt.random_state_after
        || !receipt.direct_rng_sites.is_empty()
        || receipt.next
            != (MapMakeFirstRegionsClearAllNext::FindAll {
                argument_push_va: MAP_MAKE_FIRST_REGIONS_FIND_ARGUMENT_PUSH_VA,
                call_va: MAP_MAKE_FIRST_REGIONS_FIND_CALL_VA,
                primitive_va: REGIONS_FIND_ALL_VA,
                stack_argument_is_unread: true,
            })
    {
        return false;
    }

    for (region, record) in receipt.region_records.iter().enumerate() {
        if usize::from(record.region) != region
            || record.size_nonzero_path != (record.before.size != 0)
            || record.after != regions.list[region]
        {
            return false;
        }
        let mut expected = record.before.clone();
        expected.flags = 0;
        expected.climate = 0;
        expected.goodies = 0;
        expected.common_factor = 8;
        expected.goody_factor = 8;
        if expected.size != 0 {
            expected.size = 0;
            expected.coords.items.clear();
            expected.coords.capacity = 0;
            expected.coords.flags = 0;
            expected.borders = 0;
            expected.border_id = 0;
        }
        if record.after != expected {
            return false;
        }
    }

    let expected_frees = receipt
        .region_records
        .iter()
        .filter(|record| record.before.size != 0 && record.before.coords.capacity != 0)
        .map(|record| record.region)
        .collect::<Vec<_>>();
    if receipt.coordinate_frees.len() != expected_frees.len() {
        return false;
    }
    for (free, expected_region) in receipt.coordinate_frees.iter().zip(expected_frees) {
        if free.region != expected_region {
            return false;
        }
        let Some(record) = receipt.region_records.get(usize::from(free.region)) else {
            return false;
        };
        if free.call_va != REGIONS_CLEAR_ALL_COORD_FREE_CALL_VA
            || free.import_iat_va != FREE_IMPORT_IAT_VA
            || free.elements != record.before.coords.items
            || free.capacity != record.before.coords.capacity
            || free.element_width != 8
            || free.state_before != RegionsAllocationState::Live
            || free.state_after != RegionsAllocationState::Freed
        {
            return false;
        }
    }
    receipt
        .world_region_mutations
        .iter()
        .enumerate()
        .all(|(index, mutation)| {
            mutation.cell < world.wdata.len()
                && (index == 0 || receipt.world_region_mutations[index - 1].cell < mutation.cell)
                && mutation.region_before != 0
                && mutation.region_after == 0
        })
}

/// One imported allocator call over the logical shared `Regions::coords`
/// scratch queue. No native pointer value is retained or compared.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegionsFindAllScratchAllocatorCall {
    pub call_va: u32,
    pub import_iat_va: u32,
    pub elements: i32,
    pub bytes: u64,
    pub state_before: RegionsAllocationState,
    pub state_after: RegionsAllocationState,
    pub native_return: (),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegionsFindAllScratchReceipt {
    pub before: WCoordList,
    pub old_allocation_free: Option<RegionsFindAllScratchAllocatorCall>,
    pub allocation: Option<RegionsFindAllScratchAllocatorCall>,
    pub capacity_during_body: i32,
    pub final_free: Option<RegionsFindAllScratchAllocatorCall>,
    pub after: WCoordList,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegionsFindAllRegionReceipt {
    pub region: u8,
    pub before: Region,
    pub after: Region,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct RegionsFindAllWorldMutation {
    pub cell: usize,
    pub region_before: i16,
    pub region_after: i16,
    pub region2_before: i16,
    pub region2_after: i16,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MapMakeFirstRegionsFindAllNext {
    TerritoryLimitStore {
        prep: MapMakeFirstTerritoryPrepBody,
        world_owner_load_va: u32,
        map_value_load_va: u32,
        store_va: u32,
        map_field_offset: u32,
        world_field_offset: u32,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapMakeFirstRegionsFindAllReceipt {
    pub caller: MapMakeFirstRegionsFindAllCallerBody,
    pub body: RegionsFindAllNativeBody,
    pub executed_ret_va: u32,
    pub callee_stack_argument_bytes_popped: u8,
    pub caller_resume_va: u32,
    pub stack_argument_is_unread: bool,
    pub build: RegionBuildReceipt,
    pub successful_find_calls: i32,
    pub diagnostic_calls_executed: Vec<u32>,
    pub scratch: RegionsFindAllScratchReceipt,
    pub region_records_visited: usize,
    pub region_records: Vec<RegionsFindAllRegionReceipt>,
    pub regions_land_before: i32,
    pub regions_land_after: i32,
    pub regions_sea_before: i32,
    pub regions_sea_after: i32,
    pub world_cells_visited: usize,
    pub world_mutations: Vec<RegionsFindAllWorldMutation>,
    pub world_before: WorldChecksum,
    pub world_after: WorldChecksum,
    pub world_sections_changed: Vec<WorldSection>,
    pub random_state_before: i32,
    pub random_state_after: i32,
    pub direct_rng_sites: Vec<u32>,
    pub next: MapMakeFirstRegionsFindAllNext,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MapMakeFirstRegionsFindAllError {
    PriorClearReceiptMismatch,
    FindAll(RegionsError),
}

/// Execute the exact first common-driver `Regions::find_all(int)` body and
/// freeze before the first World territory-limit store at `0x0068be4a`.
pub fn execute_map_make_first_regions_find_all(
    world: &mut World,
    regions: &mut Regions,
    random_state: i32,
    prior_clear: &MapMakeFirstRegionsClearAllReceipt,
) -> Result<MapMakeFirstRegionsFindAllReceipt, MapMakeFirstRegionsFindAllError> {
    if !validate_map_make_first_regions_clear_all_receipt(world, regions, prior_clear)
        || prior_clear.random_state_after != random_state
    {
        return Err(MapMakeFirstRegionsFindAllError::PriorClearReceiptMismatch);
    }

    let world_before = world.checksum_sections();
    let world_region_before = world
        .wdata
        .iter()
        .map(|cell| (cell.region, cell.region2))
        .collect::<Vec<_>>();
    let regions_before = regions.clone();
    let build = regions
        .find_all_after_clear(world)
        .map_err(MapMakeFirstRegionsFindAllError::FindAll)?;
    let world_after = world.checksum_sections();

    let scratch_grows = regions_before.coords.capacity < world.size;
    let old_scratch_live = regions_before.coords.capacity > 0;
    let allocated_elements = if scratch_grows {
        world.size.max(0)
    } else {
        regions_before.coords.capacity.max(0)
    };
    let allocated_bytes = u64::try_from(allocated_elements).unwrap_or(0) * 8;
    let old_allocation_free =
        (scratch_grows && old_scratch_live).then(|| RegionsFindAllScratchAllocatorCall {
            call_va: REGIONS_FIND_ALL_OLD_SCRATCH_FREE_CALL_VA,
            import_iat_va: FREE_IMPORT_IAT_VA,
            elements: regions_before.coords.capacity,
            bytes: u64::try_from(regions_before.coords.capacity).unwrap_or(0) * 8,
            state_before: RegionsAllocationState::Live,
            state_after: RegionsAllocationState::Freed,
            native_return: (),
        });
    let allocation =
        (scratch_grows && world.size > 0).then(|| RegionsFindAllScratchAllocatorCall {
            call_va: REGIONS_FIND_ALL_SCRATCH_MALLOC_CALL_VA,
            import_iat_va: MALLOC_IMPORT_IAT_VA,
            elements: world.size,
            bytes: u64::try_from(world.size).unwrap_or(0) * 8,
            state_before: RegionsAllocationState::Unallocated,
            state_after: RegionsAllocationState::Live,
            native_return: (),
        });
    let scratch_live_during_body = allocated_elements > 0;
    let final_free = scratch_live_during_body.then(|| RegionsFindAllScratchAllocatorCall {
        call_va: REGIONS_FIND_ALL_FINAL_SCRATCH_FREE_CALL_VA,
        import_iat_va: FREE_IMPORT_IAT_VA,
        elements: allocated_elements,
        bytes: allocated_bytes,
        state_before: RegionsAllocationState::Live,
        state_after: RegionsAllocationState::Freed,
        native_return: (),
    });
    let scratch = RegionsFindAllScratchReceipt {
        before: regions_before.coords.clone(),
        old_allocation_free,
        allocation,
        capacity_during_body: allocated_elements,
        final_free,
        after: regions.coords.clone(),
    };
    let region_records = regions_before
        .list
        .iter()
        .zip(regions.list.iter())
        .enumerate()
        .map(|(region, (before, after))| RegionsFindAllRegionReceipt {
            region: region as u8,
            before: before.clone(),
            after: after.clone(),
        })
        .collect();
    let world_mutations = world_region_before
        .into_iter()
        .zip(world.wdata.iter().map(|cell| (cell.region, cell.region2)))
        .enumerate()
        .filter_map(
            |(cell, ((region_before, region2_before), (region_after, region2_after)))| {
                (region_before != region_after || region2_before != region2_after).then_some(
                    RegionsFindAllWorldMutation {
                        cell,
                        region_before,
                        region_after,
                        region2_before,
                        region2_after,
                    },
                )
            },
        )
        .collect();
    let world_sections_changed = WorldSection::all()
        .into_iter()
        .filter(|section| world_before.section(*section) != world_after.section(*section))
        .collect();

    Ok(MapMakeFirstRegionsFindAllReceipt {
        caller: MAP_MAKE_FIRST_REGIONS_FIND_ALL_CALLER_BODY,
        body: REGIONS_FIND_ALL_NATIVE_BODY,
        executed_ret_va: REGIONS_FIND_ALL_RET_VA,
        callee_stack_argument_bytes_popped: 4,
        caller_resume_va: MAP_MAKE_FIRST_REGIONS_FIND_RESUME_VA,
        stack_argument_is_unread: true,
        successful_find_calls: build.land_components_found + build.sea_components_found,
        diagnostic_calls_executed: Vec::new(),
        build,
        scratch,
        region_records_visited: REGION_COUNT,
        region_records,
        regions_land_before: regions_before.land,
        regions_land_after: regions.land,
        regions_sea_before: regions_before.sea,
        regions_sea_after: regions.sea,
        world_cells_visited: world.wdata.len(),
        world_mutations,
        world_before,
        world_after,
        world_sections_changed,
        random_state_before: random_state,
        random_state_after: random_state,
        direct_rng_sites: Vec::new(),
        next: MapMakeFirstRegionsFindAllNext::TerritoryLimitStore {
            prep: MAP_MAKE_FIRST_TERRITORY_PREP_BODY,
            world_owner_load_va: MAP_MAKE_FIRST_TERRITORY_WORLD_LOAD_VA,
            map_value_load_va: MAP_MAKE_FIRST_TERRITORY_MAP_LOAD_VA,
            store_va: MAP_MAKE_FIRST_TERRITORY_STORE_VA,
            map_field_offset: MAP_PLAYER_TERRITORY_LIMIT_OFFSET,
            world_field_offset: WORLD_PLAYER_TERRITORY_LIMIT_OFFSET,
        },
    })
}

pub(crate) fn validate_map_make_first_regions_find_all_receipt(
    world: &World,
    regions: &Regions,
    prior_clear: &MapMakeFirstRegionsClearAllReceipt,
    receipt: &MapMakeFirstRegionsFindAllReceipt,
) -> bool {
    if prior_clear.region_records.len() != REGION_COUNT
        || receipt.world_mutations.len() != world.wdata.len()
    {
        return false;
    }
    let mut clear_world = world.clone();
    for mutation in &receipt.world_mutations {
        if mutation.cell >= clear_world.wdata.len() {
            return false;
        }
        clear_world.wdata[mutation.cell].region = mutation.region_before;
        clear_world.wdata[mutation.cell].region2 = mutation.region2_before;
    }
    let mut clear_regions = regions.clone();
    for (region, record) in prior_clear.region_records.iter().enumerate() {
        if usize::from(record.region) != region {
            return false;
        }
        clear_regions.list[region] = record.after.clone();
    }
    clear_regions.coords = prior_clear.regions_coords_after.clone();
    clear_regions.land = prior_clear.regions_land_after;
    clear_regions.sea = prior_clear.regions_sea_after;
    if !validate_map_make_first_regions_clear_all_receipt(&clear_world, &clear_regions, prior_clear)
    {
        return false;
    }

    let expected_next = MapMakeFirstRegionsFindAllNext::TerritoryLimitStore {
        prep: MAP_MAKE_FIRST_TERRITORY_PREP_BODY,
        world_owner_load_va: MAP_MAKE_FIRST_TERRITORY_WORLD_LOAD_VA,
        map_value_load_va: MAP_MAKE_FIRST_TERRITORY_MAP_LOAD_VA,
        store_va: MAP_MAKE_FIRST_TERRITORY_STORE_VA,
        map_field_offset: MAP_PLAYER_TERRITORY_LIMIT_OFFSET,
        world_field_offset: WORLD_PLAYER_TERRITORY_LIMIT_OFFSET,
    };
    if receipt.caller != MAP_MAKE_FIRST_REGIONS_FIND_ALL_CALLER_BODY
        || receipt.body != REGIONS_FIND_ALL_NATIVE_BODY
        || receipt.executed_ret_va != REGIONS_FIND_ALL_RET_VA
        || receipt.callee_stack_argument_bytes_popped != 4
        || receipt.caller_resume_va != MAP_MAKE_FIRST_REGIONS_FIND_RESUME_VA
        || !receipt.stack_argument_is_unread
        || receipt.successful_find_calls
            != receipt.build.land_components_found + receipt.build.sea_components_found
        || receipt.build.non_input_pumps != receipt.successful_find_calls + 1
        || !receipt.diagnostic_calls_executed.is_empty()
        || receipt.region_records_visited != REGION_COUNT
        || receipt.region_records.len() != REGION_COUNT
        || receipt.regions_land_before != prior_clear.regions_land_after
        || receipt.regions_land_after != regions.land
        || receipt.regions_sea_before != prior_clear.regions_sea_after
        || receipt.regions_sea_after != regions.sea
        || receipt.world_cells_visited != world.wdata.len()
        || receipt.world_mutations.len() != world.wdata.len()
        || receipt.world_before != prior_clear.world_after
        || receipt.world_after != world.checksum_sections()
        || receipt.world_sections_changed
            != receipt
                .world_before
                .differing_sections(&receipt.world_after)
        || receipt.world_sections_changed != [WorldSection::WData]
        || receipt.random_state_before != prior_clear.random_state_after
        || receipt.random_state_before != receipt.random_state_after
        || !receipt.direct_rng_sites.is_empty()
        || receipt.next != expected_next
    {
        return false;
    }

    if receipt
        .region_records
        .iter()
        .enumerate()
        .any(|(region, record)| {
            usize::from(record.region) != region
                || record.before != prior_clear.region_records[region].after
                || record.after != regions.list[region]
        })
    {
        return false;
    }
    if receipt
        .world_mutations
        .iter()
        .enumerate()
        .any(|(cell, mutation)| {
            mutation.cell != cell
                || mutation.region_before != 0
                || mutation.region_after != world.wdata[cell].region
                || mutation.region2_after != world.wdata[cell].region2
                || (mutation.region_before == mutation.region_after
                    && mutation.region2_before == mutation.region2_after)
        })
    {
        return false;
    }

    let before_capacity = receipt.scratch.before.capacity;
    let scratch_grows = before_capacity < world.size;
    let expected_capacity = if scratch_grows {
        world.size.max(0)
    } else {
        before_capacity.max(0)
    };
    if receipt.scratch.before != prior_clear.regions_coords_after
        || receipt.scratch.after != regions.coords
        || !receipt.scratch.after.items.is_empty()
        || receipt.scratch.after.capacity != 0
        || receipt.scratch.after.flags != 0
        || receipt.scratch.capacity_during_body != expected_capacity
        || receipt.scratch.old_allocation_free.is_some() != (scratch_grows && before_capacity > 0)
        || receipt.scratch.allocation.is_some() != (scratch_grows && world.size > 0)
        || receipt.scratch.final_free.is_some() != (expected_capacity > 0)
    {
        return false;
    }
    if let Some(call) = &receipt.scratch.old_allocation_free {
        if call.call_va != REGIONS_FIND_ALL_OLD_SCRATCH_FREE_CALL_VA
            || call.import_iat_va != FREE_IMPORT_IAT_VA
            || call.elements != before_capacity
            || call.bytes != u64::try_from(before_capacity).unwrap_or(0) * 8
            || call.state_before != RegionsAllocationState::Live
            || call.state_after != RegionsAllocationState::Freed
        {
            return false;
        }
    }
    if let Some(call) = &receipt.scratch.allocation {
        if call.call_va != REGIONS_FIND_ALL_SCRATCH_MALLOC_CALL_VA
            || call.import_iat_va != MALLOC_IMPORT_IAT_VA
            || call.elements != world.size
            || call.bytes != u64::try_from(world.size).unwrap_or(0) * 8
            || call.state_before != RegionsAllocationState::Unallocated
            || call.state_after != RegionsAllocationState::Live
        {
            return false;
        }
    }
    if let Some(call) = &receipt.scratch.final_free {
        if call.call_va != REGIONS_FIND_ALL_FINAL_SCRATCH_FREE_CALL_VA
            || call.import_iat_va != FREE_IMPORT_IAT_VA
            || call.elements != expected_capacity
            || call.bytes != u64::try_from(expected_capacity).unwrap_or(0) * 8
            || call.state_before != RegionsAllocationState::Live
            || call.state_after != RegionsAllocationState::Freed
        {
            return false;
        }
    }
    true
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct TerritoryLimits {
    pub player_base: i32,
    pub player_civic: i32,
    pub player_city: i32,
    pub colonized_base: i32,
    pub colonized_civic: i32,
    pub colonized_city: i32,
}

impl TerritoryLimits {
    /// Map's six constructor fields come from Constants+0x118/+0x11c/+0x120,
    /// with the same triplet copied to the colonized fields.
    pub fn from_world_prefix(world: &World) -> Self {
        Self {
            player_base: world.player_territory_limit,
            player_civic: world.player_territory_limit_civic,
            player_city: world.player_territory_limit_city,
            colonized_base: world.colonized_territory_limit,
            colonized_civic: world.colonized_territory_limit_civic,
            colonized_city: world.colonized_territory_limit_city,
        }
    }

    fn values(self) -> [i32; 6] {
        [
            self.player_base,
            self.player_civic,
            self.player_city,
            self.colonized_base,
            self.colonized_civic,
            self.colonized_city,
        ]
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MapMakeTerritoryLimitsNativeBody {
    pub entry_va: u32,
    pub end_va_exclusive: u32,
    pub size: u32,
    pub instruction_count: u32,
    pub sha256: &'static str,
    pub direct_calls: &'static [(u32, u32)],
}

pub const MAP_MAKE_TERRITORY_LIMITS_NATIVE_BODY: MapMakeTerritoryLimitsNativeBody =
    MapMakeTerritoryLimitsNativeBody {
        entry_va: MAP_MAKE_FIRST_TERRITORY_STORE_VA,
        end_va_exclusive: MAP_MAKE_TERRITORY_LIMITS_END_VA,
        size: MAP_MAKE_TERRITORY_LIMITS_SIZE,
        instruction_count: MAP_MAKE_TERRITORY_LIMITS_INSTRUCTION_COUNT,
        sha256: MAP_MAKE_TERRITORY_LIMITS_SHA256,
        direct_calls: &[],
    };

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TerritoryLimitField {
    PlayerBase,
    PlayerCivic,
    PlayerCity,
    ColonizedBase,
    ColonizedCivic,
    ColonizedCity,
}

pub const TERRITORY_LIMIT_FIELDS: [TerritoryLimitField; 6] = [
    TerritoryLimitField::PlayerBase,
    TerritoryLimitField::PlayerCivic,
    TerritoryLimitField::PlayerCity,
    TerritoryLimitField::ColonizedBase,
    TerritoryLimitField::ColonizedCivic,
    TerritoryLimitField::ColonizedCity,
];

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct TerritoryLimitStoreReceipt {
    pub field: TerritoryLimitField,
    pub map_value_load_va: u32,
    pub store_va: u32,
    pub map_field_offset: u32,
    pub world_field_offset: u32,
    pub source_value: i32,
    pub value_before: i32,
    pub value_after: i32,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MapMakeTerritoryStyleBranchReceipt {
    pub compare_va: u32,
    pub branch_va: u32,
    pub map_style: u8,
    pub compared_value: u8,
    pub taken: bool,
    pub target_va: u32,
    pub fallthrough_va: u32,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MapMakeTerritoryLimitsNext {
    FixDiagLand { call_va: u32, primitive_va: u32 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapMakeTerritoryLimitsReceipt {
    pub body: MapMakeTerritoryLimitsNativeBody,
    pub source: TerritoryLimits,
    pub stores: Vec<TerritoryLimitStoreReceipt>,
    pub branch: MapMakeTerritoryStyleBranchReceipt,
    pub world_before: WorldChecksum,
    pub world_after: WorldChecksum,
    pub world_sections_changed: Vec<WorldSection>,
    pub random_state_before: i32,
    pub random_state_after: i32,
    pub direct_rng_sites: Vec<u32>,
    pub next: MapMakeTerritoryLimitsNext,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MapMakeTerritoryLimitsError {
    PriorFindAllReceiptMismatch,
    AlternateStyleBranch { map_style: u8, target_va: u32 },
}

fn world_territory_limits(world: &World) -> TerritoryLimits {
    TerritoryLimits {
        player_base: world.player_territory_limit,
        player_civic: world.player_territory_limit_civic,
        player_city: world.player_territory_limit_city,
        colonized_base: world.colonized_territory_limit,
        colonized_civic: world.colonized_territory_limit_civic,
        colonized_city: world.colonized_territory_limit_city,
    }
}

fn set_world_territory_limit(world: &mut World, field: TerritoryLimitField, value: i32) {
    match field {
        TerritoryLimitField::PlayerBase => world.player_territory_limit = value,
        TerritoryLimitField::PlayerCivic => world.player_territory_limit_civic = value,
        TerritoryLimitField::PlayerCity => world.player_territory_limit_city = value,
        TerritoryLimitField::ColonizedBase => world.colonized_territory_limit = value,
        TerritoryLimitField::ColonizedCivic => world.colonized_territory_limit_civic = value,
        TerritoryLimitField::ColonizedCity => world.colonized_territory_limit_city = value,
    }
}

/// Execute the six exact `Map::make` territory-limit stores and the style-23
/// branch. The admitted East Meets West path freezes before `Map::fix_diag_land`.
pub fn execute_map_make_territory_limits(
    world: &mut World,
    regions: &Regions,
    map_style: u8,
    source: TerritoryLimits,
    random_state: i32,
    prior_clear: &MapMakeFirstRegionsClearAllReceipt,
    prior_find_all: &MapMakeFirstRegionsFindAllReceipt,
) -> Result<MapMakeTerritoryLimitsReceipt, MapMakeTerritoryLimitsError> {
    if !validate_map_make_first_regions_find_all_receipt(
        world,
        regions,
        prior_clear,
        prior_find_all,
    ) || prior_find_all.random_state_after != random_state
    {
        return Err(MapMakeTerritoryLimitsError::PriorFindAllReceiptMismatch);
    }
    if map_style == MAP_MAKE_STYLE_BRANCH_VALUE {
        return Err(MapMakeTerritoryLimitsError::AlternateStyleBranch {
            map_style,
            target_va: MAP_MAKE_STYLE_BRANCH_TARGET_VA,
        });
    }

    let world_before = world.checksum_sections();
    let before_values = world_territory_limits(world).values();
    let source_values = source.values();
    let mut stores = Vec::with_capacity(TERRITORY_LIMIT_FIELDS.len());
    for index in 0..TERRITORY_LIMIT_FIELDS.len() {
        let field = TERRITORY_LIMIT_FIELDS[index];
        set_world_territory_limit(world, field, source_values[index]);
        stores.push(TerritoryLimitStoreReceipt {
            field,
            map_value_load_va: MAP_MAKE_TERRITORY_LIMIT_LOAD_VAS[index],
            store_va: MAP_MAKE_TERRITORY_LIMIT_STORE_VAS[index],
            map_field_offset: MAP_TERRITORY_LIMIT_OFFSETS[index],
            world_field_offset: WORLD_TERRITORY_LIMIT_OFFSETS[index],
            source_value: source_values[index],
            value_before: before_values[index],
            value_after: source_values[index],
        });
    }
    let world_after = world.checksum_sections();
    let world_sections_changed = world_before.differing_sections(&world_after);

    Ok(MapMakeTerritoryLimitsReceipt {
        body: MAP_MAKE_TERRITORY_LIMITS_NATIVE_BODY,
        source,
        stores,
        branch: MapMakeTerritoryStyleBranchReceipt {
            compare_va: MAP_MAKE_STYLE_COMPARE_VA,
            branch_va: MAP_MAKE_STYLE_BRANCH_VA,
            map_style,
            compared_value: MAP_MAKE_STYLE_BRANCH_VALUE,
            taken: false,
            target_va: MAP_MAKE_STYLE_BRANCH_TARGET_VA,
            fallthrough_va: MAP_MAKE_FIRST_FIX_DIAG_LAND_CALL_VA,
        },
        world_before,
        world_after,
        world_sections_changed,
        random_state_before: random_state,
        random_state_after: random_state,
        direct_rng_sites: Vec::new(),
        next: MapMakeTerritoryLimitsNext::FixDiagLand {
            call_va: MAP_MAKE_FIRST_FIX_DIAG_LAND_CALL_VA,
            primitive_va: MAP_FIX_DIAG_LAND_VA,
        },
    })
}

pub(crate) fn validate_map_make_territory_limits_receipt(
    world: &World,
    regions: &Regions,
    prior_clear: &MapMakeFirstRegionsClearAllReceipt,
    prior_find_all: &MapMakeFirstRegionsFindAllReceipt,
    receipt: &MapMakeTerritoryLimitsReceipt,
) -> bool {
    if receipt.stores.len() != TERRITORY_LIMIT_FIELDS.len() {
        return false;
    }
    let mut find_world = world.clone();
    for store in &receipt.stores {
        set_world_territory_limit(&mut find_world, store.field, store.value_before);
    }
    if !validate_map_make_first_regions_find_all_receipt(
        &find_world,
        regions,
        prior_clear,
        prior_find_all,
    ) {
        return false;
    }

    let source_values = receipt.source.values();
    let current_values = world_territory_limits(world).values();
    for index in 0..TERRITORY_LIMIT_FIELDS.len() {
        let store = receipt.stores[index];
        if store.field != TERRITORY_LIMIT_FIELDS[index]
            || store.map_value_load_va != MAP_MAKE_TERRITORY_LIMIT_LOAD_VAS[index]
            || store.store_va != MAP_MAKE_TERRITORY_LIMIT_STORE_VAS[index]
            || store.map_field_offset != MAP_TERRITORY_LIMIT_OFFSETS[index]
            || store.world_field_offset != WORLD_TERRITORY_LIMIT_OFFSETS[index]
            || store.source_value != source_values[index]
            || store.value_after != source_values[index]
            || store.value_after != current_values[index]
        {
            return false;
        }
    }

    receipt.body == MAP_MAKE_TERRITORY_LIMITS_NATIVE_BODY
        && receipt.branch
            == (MapMakeTerritoryStyleBranchReceipt {
                compare_va: MAP_MAKE_STYLE_COMPARE_VA,
                branch_va: MAP_MAKE_STYLE_BRANCH_VA,
                map_style: 19,
                compared_value: MAP_MAKE_STYLE_BRANCH_VALUE,
                taken: false,
                target_va: MAP_MAKE_STYLE_BRANCH_TARGET_VA,
                fallthrough_va: MAP_MAKE_FIRST_FIX_DIAG_LAND_CALL_VA,
            })
        && receipt.world_before == prior_find_all.world_after
        && receipt.world_before == find_world.checksum_sections()
        && receipt.world_after == world.checksum_sections()
        && receipt.world_sections_changed
            == receipt
                .world_before
                .differing_sections(&receipt.world_after)
        && receipt.random_state_before == prior_find_all.random_state_after
        && receipt.random_state_before == receipt.random_state_after
        && receipt.direct_rng_sites.is_empty()
        && receipt.next
            == (MapMakeTerritoryLimitsNext::FixDiagLand {
                call_va: MAP_MAKE_FIRST_FIX_DIAG_LAND_CALL_VA,
                primitive_va: MAP_FIX_DIAG_LAND_VA,
            })
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MapFixDiagLandNativeBody {
    pub entry_va: u32,
    pub end_va_exclusive: u32,
    pub ret_va: u32,
    pub size: u32,
    pub instruction_count: u32,
    pub sha256: &'static str,
    pub direct_calls: &'static [(u32, u32)],
    pub corner_x_va: u32,
    pub corner_y_va: u32,
    pub corner_x: [i32; 4],
    pub corner_y: [i32; 4],
}

pub const MAP_FIX_DIAG_LAND_NATIVE_BODY: MapFixDiagLandNativeBody = MapFixDiagLandNativeBody {
    entry_va: MAP_FIX_DIAG_LAND_VA,
    end_va_exclusive: MAP_FIX_DIAG_LAND_END_VA,
    ret_va: MAP_FIX_DIAG_LAND_RET_VA,
    size: MAP_FIX_DIAG_LAND_SIZE,
    instruction_count: MAP_FIX_DIAG_LAND_INSTRUCTION_COUNT,
    sha256: MAP_FIX_DIAG_LAND_SHA256,
    direct_calls: &[],
    corner_x_va: MAP_FIX_DIAG_LAND_CORNER_X_VA,
    corner_y_va: MAP_FIX_DIAG_LAND_CORNER_Y_VA,
    corner_x: MAP_FIX_DIAG_LAND_CORNER_X,
    corner_y: MAP_FIX_DIAG_LAND_CORNER_Y,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapFixDiagLandWorldMutation {
    pub x: i32,
    pub y: i32,
    pub cell: usize,
    pub before: WData,
    pub after: WData,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MapFixDiagLandNext {
    StringConstructor {
        caller_resume_va: u32,
        string_literal_push_va: u32,
        string_local_load_va: u32,
        call_va: u32,
        primitive_va: u32,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapFixDiagLandReceipt {
    pub caller_call_va: u32,
    pub body: MapFixDiagLandNativeBody,
    pub cells_scanned: usize,
    pub mutations: Vec<MapFixDiagLandWorldMutation>,
    pub world_before: WorldChecksum,
    pub world_after: WorldChecksum,
    pub world_sections_changed: Vec<WorldSection>,
    pub random_state_before: i32,
    pub random_state_after: i32,
    pub direct_rng_sites: Vec<u32>,
    pub next: MapFixDiagLandNext,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MapFixDiagLandError {
    PriorTerritoryLimitsReceiptMismatch,
}

/// Execute the complete call-free retail `Map::fix_diag_land` body. The scan
/// is X-major and in-place; every changed WData record is retained verbatim.
pub fn execute_map_fix_diag_land(
    world: &mut World,
    regions: &Regions,
    random_state: i32,
    prior_clear: &MapMakeFirstRegionsClearAllReceipt,
    prior_find_all: &MapMakeFirstRegionsFindAllReceipt,
    prior_limits: &MapMakeTerritoryLimitsReceipt,
) -> Result<MapFixDiagLandReceipt, MapFixDiagLandError> {
    if !validate_map_make_territory_limits_receipt(
        world,
        regions,
        prior_clear,
        prior_find_all,
        prior_limits,
    ) || prior_limits.random_state_after != random_state
    {
        return Err(MapFixDiagLandError::PriorTerritoryLimitsReceiptMismatch);
    }

    let before_world = world.clone();
    let world_before = before_world.checksum_sections();
    world.fix_diag_land();
    let world_after = world.checksum_sections();
    let mut mutations = Vec::new();
    for x in 0..world.xs {
        for y in 0..world.ys {
            let cell = world.w_index(x, y);
            if before_world.wdata[cell] != world.wdata[cell] {
                mutations.push(MapFixDiagLandWorldMutation {
                    x,
                    y,
                    cell,
                    before: before_world.wdata[cell].clone(),
                    after: world.wdata[cell].clone(),
                });
            }
        }
    }

    Ok(MapFixDiagLandReceipt {
        caller_call_va: MAP_MAKE_FIRST_FIX_DIAG_LAND_CALL_VA,
        body: MAP_FIX_DIAG_LAND_NATIVE_BODY,
        cells_scanned: world.wdata.len(),
        mutations,
        world_sections_changed: world_before.differing_sections(&world_after),
        world_before,
        world_after,
        random_state_before: random_state,
        random_state_after: random_state,
        direct_rng_sites: Vec::new(),
        next: MapFixDiagLandNext::StringConstructor {
            caller_resume_va: MAP_MAKE_FIX_DIAG_LAND_RESUME_VA,
            string_literal_push_va: MAP_MAKE_POST_FIX_DIAG_STRING_LITERAL_PUSH_VA,
            string_local_load_va: MAP_MAKE_POST_FIX_DIAG_STRING_LOCAL_LOAD_VA,
            call_va: MAP_MAKE_POST_FIX_DIAG_STRING_CONSTRUCTOR_CALL_VA,
            primitive_va: STRING_CONSTRUCTOR_VA,
        },
    })
}

pub(crate) fn validate_map_fix_diag_land_receipt(
    world: &World,
    regions: &Regions,
    prior_clear: &MapMakeFirstRegionsClearAllReceipt,
    prior_find_all: &MapMakeFirstRegionsFindAllReceipt,
    prior_limits: &MapMakeTerritoryLimitsReceipt,
    receipt: &MapFixDiagLandReceipt,
) -> bool {
    let mut before_world = world.clone();
    let mut previous = None;
    for mutation in &receipt.mutations {
        if mutation.x < 0
            || mutation.x >= world.xs
            || mutation.y < 0
            || mutation.y >= world.ys
            || mutation.cell != world.w_index(mutation.x, mutation.y)
            || previous.is_some_and(|(x, y)| (mutation.x, mutation.y) <= (x, y))
            || world.wdata[mutation.cell] != mutation.after
        {
            return false;
        }
        let mut expected = mutation.before.clone();
        expected.land = land::OCEAN;
        expected.land_sub = 0;
        if mutation.before.land != 0 || mutation.after != expected {
            return false;
        }
        before_world.wdata[mutation.cell] = mutation.before.clone();
        previous = Some((mutation.x, mutation.y));
    }
    if !validate_map_make_territory_limits_receipt(
        &before_world,
        regions,
        prior_clear,
        prior_find_all,
        prior_limits,
    ) {
        return false;
    }
    let mut replayed = before_world.clone();
    replayed.fix_diag_land();

    receipt.caller_call_va == MAP_MAKE_FIRST_FIX_DIAG_LAND_CALL_VA
        && receipt.body == MAP_FIX_DIAG_LAND_NATIVE_BODY
        && receipt.cells_scanned == world.wdata.len()
        && receipt.world_before == before_world.checksum_sections()
        && receipt.world_before == prior_limits.world_after
        && receipt.world_after == world.checksum_sections()
        && receipt.world_after == replayed.checksum_sections()
        && replayed.wdata == world.wdata
        && receipt.world_sections_changed
            == receipt
                .world_before
                .differing_sections(&receipt.world_after)
        && receipt
            .world_sections_changed
            .iter()
            .all(|section| *section == WorldSection::WData)
        && receipt.random_state_before == prior_limits.random_state_after
        && receipt.random_state_before == receipt.random_state_after
        && receipt.direct_rng_sites.is_empty()
        && receipt.next
            == (MapFixDiagLandNext::StringConstructor {
                caller_resume_va: MAP_MAKE_FIX_DIAG_LAND_RESUME_VA,
                string_literal_push_va: MAP_MAKE_POST_FIX_DIAG_STRING_LITERAL_PUSH_VA,
                string_local_load_va: MAP_MAKE_POST_FIX_DIAG_STRING_LOCAL_LOAD_VA,
                call_va: MAP_MAKE_POST_FIX_DIAG_STRING_CONSTRUCTOR_CALL_VA,
                primitive_va: STRING_CONSTRUCTOR_VA,
            })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PostContinentReceipt {
    pub first_clear_va: u32,
    pub first_find_va: u32,
    pub first_regions: RegionBuildReceipt,
    pub limits: TerritoryLimits,
    pub fix_diag_va: u32,
    pub fix_diag_cells_changed: usize,
    pub make_coastlines_va: u32,
    pub coastline_cells_changed: usize,
    pub second_clear_va: u32,
    pub second_find_va: u32,
    pub second_regions: RegionBuildReceipt,
    pub final_land_cells: usize,
    pub final_coast_cells: usize,
    pub final_ocean_cells: usize,
    pub final_half_water_cells: usize,
    pub next_va: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PostContinentError {
    FirstRegionBuild(RegionsError),
    SecondRegionBuild(RegionsError),
}

/// Execute the common post-style chain transactionally.
pub fn execute_post_continent(
    world: &mut World,
    regions: &mut Regions,
    limits: TerritoryLimits,
) -> Result<PostContinentReceipt, PostContinentError> {
    let mut next_world = world.clone();
    let mut next_regions = regions.clone();

    // At 0x0068be36 the caller invokes clear_all then find_all before copying
    // the Map fields. This public composite is exactly that pair.
    let first_regions = next_regions
        .rebuild_after_coastlines(&mut next_world)
        .map_err(PostContinentError::FirstRegionBuild)?;

    next_world.player_territory_limit = limits.player_base;
    next_world.player_territory_limit_civic = limits.player_civic;
    next_world.player_territory_limit_city = limits.player_city;
    next_world.colonized_territory_limit = limits.colonized_base;
    next_world.colonized_territory_limit_civic = limits.colonized_civic;
    next_world.colonized_territory_limit_city = limits.colonized_city;

    let before_diag = terrain_words(&next_world);
    next_world.fix_diag_land();
    let after_diag = terrain_words(&next_world);
    let fix_diag_cells_changed = changed_cells(&before_diag, &after_diag);

    // The don-sim composite owns the exact make_coastlines + second
    // clear_all/find_all transaction. Region rebuilding changes region fields,
    // not the terrain words measured here.
    let before_coastline = after_diag;
    let second_regions = make_coastlines_and_rebuild_regions(&mut next_world, &mut next_regions)
        .map_err(PostContinentError::SecondRegionBuild)?;
    let after_coastline = terrain_words(&next_world);
    let coastline_cells_changed = changed_cells(&before_coastline, &after_coastline);

    let final_land_cells = next_world
        .wdata
        .iter()
        .filter(|cell| !cell_is_ocean(cell))
        .count();
    let final_coast_cells = next_world
        .wdata
        .iter()
        .filter(|cell| cell.flags & wflag::COAST != 0)
        .count();
    let final_ocean_cells = next_world
        .wdata
        .iter()
        .filter(|cell| cell_is_ocean(cell))
        .count();
    let final_half_water_cells = next_world
        .wdata
        .iter()
        .filter(|cell| cell.flags & wflag::WATERHALF != 0)
        .count();

    *world = next_world;
    *regions = next_regions;
    Ok(PostContinentReceipt {
        first_clear_va: REGIONS_CLEAR_ALL_VA,
        first_find_va: REGIONS_FIND_ALL_VA,
        first_regions,
        limits,
        fix_diag_va: MAP_FIX_DIAG_LAND_VA,
        fix_diag_cells_changed,
        make_coastlines_va: MAP_MAKE_COASTLINES_VA,
        coastline_cells_changed,
        second_clear_va: REGIONS_CLEAR_ALL_VA,
        second_find_va: REGIONS_FIND_ALL_VA,
        second_regions,
        final_land_cells,
        final_coast_cells,
        final_ocean_cells,
        final_half_water_cells,
        next_va: TERRAIN_GROUPS_FILL_FERTILE_VA,
    })
}

fn terrain_words(world: &World) -> Vec<(u16, i8, u8)> {
    world
        .wdata
        .iter()
        .map(|cell| (cell.flags, cell.land, cell.land_sub))
        .collect()
}

fn changed_cells(before: &[(u16, i8, u8)], after: &[(u16, i8, u8)]) -> usize {
    before
        .iter()
        .zip(after)
        .filter(|(before, after)| before != after)
        .count()
}

fn cell_is_ocean(cell: &don_sim::systems::map_terrain::WData) -> bool {
    cell.flags & wflag::WATERHALF == 0 && matches!(cell.land, land::COASTAL | land::OCEAN)
}
