//! Executable common `Map::make` tail through coastline reconstruction.
//!
//! Every supported style virtual returns into the same sequence:
//! `Regions::clear_all/find_all`, copy the six Map territory fields into
//! `World`, `Map::fix_diag_land`, `Map::make_coastlines`, then rebuild the
//! regions again.  The next call is `TerrainGroups::fill_fertile`.

use don_sim::systems::map_terrain::{land, wflag, World, WorldChecksum, WorldSection};
use don_sim::systems::regions::{
    make_coastlines_and_rebuild_regions, Region, RegionBuildReceipt, Regions, RegionsError,
    WCoordList, REGION_COUNT,
};

pub const REGIONS_CLEAR_ALL_VA: u32 = 0x0068_0060;
pub const REGIONS_FIND_ALL_VA: u32 = 0x0067_eff0;
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
pub const MAP_MAKE_FIRST_REGIONS_CLEAR_CALL_VA: u32 = 0x0068_be36;
pub const MAP_MAKE_FIRST_REGIONS_CLEAR_RESUME_VA: u32 = 0x0068_be3b;
pub const MAP_MAKE_FIRST_REGIONS_FIND_ARGUMENT_PUSH_VA: u32 = 0x0068_be3b;
pub const MAP_MAKE_FIRST_REGIONS_FIND_CALL_VA: u32 = 0x0068_be3c;
pub const MAP_FIX_DIAG_LAND_VA: u32 = 0x0069_c250;
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
pub enum RegionsAllocationState {
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
