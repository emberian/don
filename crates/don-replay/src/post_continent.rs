//! Executable common `Map::make` tail through coastline reconstruction.
//!
//! Every supported style virtual returns into the same sequence:
//! `Regions::clear_all/find_all`, copy the six Map territory fields into
//! `World`, `Map::fix_diag_land`, `Map::make_coastlines`, then rebuild the
//! regions again.  The next call is `TerrainGroups::fill_fertile`.

use don_sim::systems::map_terrain::{land, wflag, World};
use don_sim::systems::regions::{
    make_coastlines_and_rebuild_regions, RegionBuildReceipt, Regions, RegionsError,
};

pub const REGIONS_CLEAR_ALL_VA: u32 = 0x0068_0060;
pub const REGIONS_FIND_ALL_VA: u32 = 0x0067_eff0;
pub const MAP_FIX_DIAG_LAND_VA: u32 = 0x0069_c250;
pub const MAP_MAKE_COASTLINES_VA: u32 = 0x0069_47a0;
pub const TERRAIN_GROUPS_FILL_FERTILE_VA: u32 = 0x006a_6f90;

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

    // At 0x0068be50 the caller invokes clear_all then find_all before copying
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
