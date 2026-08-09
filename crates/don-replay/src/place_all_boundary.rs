//! Replay adapter for the completed `TerrainGroups::place_all` transaction.
//!
//! The replay reconstruction already owns the generated WData, regions, start
//! arrays and exact main-RNG state at `0x006a70d0`. Ordinary `.rcx` files do
//! not own the expression-resolved terrain-group catalog, installed TData,
//! tileset doober rules, player score/helping globals, or gameplay-subsystem
//! resolutions consumed by selected groups. This adapter makes every absent
//! producer a typed caller input and never substitutes an empty/default value.

use crate::continent::{ContinentReceipt, ContinentStop};
pub use crate::fractal_boundary::TERRAIN_GROUPS_PLACE_ALL_VA;
use crate::initial::{InitialItemBoundary, InitialItemReconstruction, InitialWorld};
use crate::post_continent::REGIONS_CLEAR_ALL_VA;
use don_sim::rng::Random;
use don_sim::systems::map_terrain::WorldChecksum;
use don_sim::systems::mountains::Mountains;
use don_sim::systems::terrain_doobers::DooberTilesetRules;
use don_sim::systems::terrain_groups::{
    PlaceAllError, PlaceAllGroupInput, PlaceAllHostEvent, PlacementReportingInputs, TerrainGroups,
};
use don_sim::systems::terrain_region_placement::RegionHelpingState;

pub const TERRAIN_GROUPS_PLACE_ALL_RETURN_VA: u32 = 0x006a_937d;

/// Expression-resolved native singleton state absent from an `.rcx`.
///
/// The object is caller-owned and updated only after `place_all` returns one.
/// A failed transaction leaves both fields byte-for-byte unchanged.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReplayPlaceAllRuntime {
    pub terrain_groups: TerrainGroups,
    pub mountains: Mountains,
}

/// The installed tile plane consumed by mountain treeification.
///
/// Dimensions are repeated deliberately. They make a capture or static-data
/// resolver attest which World shape its TData belongs to instead of letting a
/// same-length plane be attached to an unrelated map.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayPlaceAllTDataFacts {
    pub tile_xs: i32,
    pub tile_ys: i32,
    pub tile_size: i32,
    pub cells: Vec<u16>,
}

/// Player and helping globals read by the completed placement transaction.
///
/// Start coordinates are intentionally absent: the preceding continent
/// receipt and replay-owned World already produced them lawfully. The adapter
/// verifies their count against `ContinentReceipt::starts_added`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayPlaceAllPlayerFacts {
    pub progress: i32,
    pub place_players: i32,
    pub helping: Option<RegionHelpingState>,
    pub reporting: PlacementReportingInputs,
}

/// Gameplay-subsystem results consumed by selected player and region arms.
///
/// Each row is tagged with its native group index and arm kind. The simulation
/// rejects an omitted, reordered, or wrong-kind row at the first exact call
/// boundary; the replay adapter does not invent a successful host response.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReplayPlaceAllHostFacts {
    pub group_inputs: Vec<PlaceAllGroupInput>,
}

/// All non-replay facts required to cross `TerrainGroups::place_all`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayPlaceAllFacts {
    pub tdata: ReplayPlaceAllTDataFacts,
    pub doober_rules: DooberTilesetRules,
    pub players: ReplayPlaceAllPlayerFacts,
    pub host: ReplayPlaceAllHostFacts,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReplayPlaceAllError {
    Blocked {
        boundary: InitialItemBoundary,
    },
    MissingFillFertileReceipt,
    ContinentMapStyleMismatch {
        replay_map_style: u8,
        continent_map_style: u8,
    },
    ContinentDidNotReachCommonTail {
        next_va: Option<u32>,
    },
    GeneratedStartCountMismatch {
        receipt_starts: usize,
        start_x: usize,
        start_y: usize,
    },
    TDataShapeMismatch {
        world_tile_xs: i32,
        world_tile_ys: i32,
        world_tile_size: i32,
        supplied_tile_xs: i32,
        supplied_tile_ys: i32,
        supplied_tile_size: i32,
    },
    TDataLengthMismatch {
        expected: usize,
        supplied: usize,
    },
    PlaceAll(PlaceAllError),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayPlaceAllReceipt {
    pub entry_va: u32,
    pub return_va: u32,
    pub return_value: i32,
    pub map_style: u8,
    pub generated_starts: usize,
    pub tdata_cells: usize,
    pub random_state_before: i32,
    pub random_state_after: i32,
    pub host_events: Vec<PlaceAllHostEvent>,
    pub checksum_before: WorldChecksum,
    pub checksum_after: WorldChecksum,
    /// This adapter recomputes the current World digest but does not inflate
    /// replay-source coverage for caller-supplied installed/runtime facts.
    pub sourced_walked_bytes: u64,
}

/// Execute the complete native transaction at the replay's post-fertility
/// boundary.
///
/// The caller callback receives only committed events. The don-sim transaction
/// first writes into staged World/runtime/RNG objects while this adapter buffers
/// its ordered host receipts. On any validation or placement error, neither
/// replay state, runtime state, checksum nor the external callback is touched.
#[allow(clippy::too_many_arguments)]
pub fn execute_replay_place_all(
    plan: &InitialItemReconstruction,
    map: &mut InitialWorld,
    continent: &ContinentReceipt,
    runtime: &mut ReplayPlaceAllRuntime,
    facts: &ReplayPlaceAllFacts,
    mut host: impl FnMut(PlaceAllHostEvent),
) -> Result<ReplayPlaceAllReceipt, ReplayPlaceAllError> {
    match plan.boundary {
        InitialItemBoundary::MapTerrainGroupsPlaceAllUnavailable { next_va }
            if next_va == TERRAIN_GROUPS_PLACE_ALL_VA => {}
        boundary => return Err(ReplayPlaceAllError::Blocked { boundary }),
    }
    if plan.fill_fertile.is_none() {
        return Err(ReplayPlaceAllError::MissingFillFertileReceipt);
    }
    if continent.map_style != plan.inputs.map_style {
        return Err(ReplayPlaceAllError::ContinentMapStyleMismatch {
            replay_map_style: plan.inputs.map_style,
            continent_map_style: continent.map_style,
        });
    }
    let next_va = match &continent.stop {
        ContinentStop::HookComplete { next_va } => Some(*next_va),
        _ => None,
    };
    if next_va != Some(REGIONS_CLEAR_ALL_VA) {
        return Err(ReplayPlaceAllError::ContinentDidNotReachCommonTail { next_va });
    }

    let start_x = map.world.start_x.items.len();
    let start_y = map.world.start_y.items.len();
    if start_x != continent.starts_added || start_y != continent.starts_added {
        return Err(ReplayPlaceAllError::GeneratedStartCountMismatch {
            receipt_starts: continent.starts_added,
            start_x,
            start_y,
        });
    }

    if facts.tdata.tile_xs != map.world.tile_xs
        || facts.tdata.tile_ys != map.world.tile_ys
        || facts.tdata.tile_size != map.world.tile_size
    {
        return Err(ReplayPlaceAllError::TDataShapeMismatch {
            world_tile_xs: map.world.tile_xs,
            world_tile_ys: map.world.tile_ys,
            world_tile_size: map.world.tile_size,
            supplied_tile_xs: facts.tdata.tile_xs,
            supplied_tile_ys: facts.tdata.tile_ys,
            supplied_tile_size: facts.tdata.tile_size,
        });
    }
    if facts.tdata.cells.len() != map.world.tdata.len() {
        return Err(ReplayPlaceAllError::TDataLengthMismatch {
            expected: map.world.tdata.len(),
            supplied: facts.tdata.cells.len(),
        });
    }

    let checksum_before = map.checksum.clone();
    let mut staged_world = map.world.clone();
    staged_world.tdata.clone_from(&facts.tdata.cells);
    let mut staged_groups = runtime.terrain_groups.clone();
    let mut staged_mountains = runtime.mountains.clone();
    let mut staged_random = Random::new(continent.rng_final);
    let mut staged_host_events = Vec::new();

    let return_value = staged_groups
        .place_all_with_group_reporting_inputs(
            &mut staged_world,
            &map.generation_regions,
            &mut staged_random,
            &mut staged_mountains,
            facts.players.progress,
            facts.players.place_players,
            facts.players.helping,
            &facts.host.group_inputs,
            facts.doober_rules,
            plan.inputs.map_style,
            facts.players.reporting,
            |event| staged_host_events.push(event),
        )
        .map_err(ReplayPlaceAllError::PlaceAll)?;

    let checksum_after = staged_world.checksum_sections();
    map.world = staged_world;
    map.checksum = checksum_after.clone();
    runtime.terrain_groups = staged_groups;
    runtime.mountains = staged_mountains;
    for event in staged_host_events.iter().copied() {
        host(event);
    }

    Ok(ReplayPlaceAllReceipt {
        entry_va: TERRAIN_GROUPS_PLACE_ALL_VA,
        return_va: TERRAIN_GROUPS_PLACE_ALL_RETURN_VA,
        return_value,
        map_style: plan.inputs.map_style,
        generated_starts: continent.starts_added,
        tdata_cells: facts.tdata.cells.len(),
        random_state_before: continent.rng_final,
        random_state_after: staged_random.state(),
        host_events: staged_host_events,
        checksum_before,
        checksum_after,
        sourced_walked_bytes: map.sourced_walked_bytes,
    })
}
