//! Receipt-bound owner transitions for the replay World-generation prefix.
//!
//! These adapters do not claim that a completed port matches retail. They bind
//! the exact typed receipt and implementation to the before/after model images,
//! then let [`WorldOwnerLedger`] own only bytes which observably changed inside
//! the stage's proved output sections.

#![forbid(unsafe_code)]

use crate::continent::{ContinentReceipt, ContinentStop};
use crate::initial::InitialWorld;
use crate::post_continent::{
    PostContinentReceipt, MAP_FIX_DIAG_LAND_VA, MAP_MAKE_COASTLINES_VA, REGIONS_CLEAR_ALL_VA,
    REGIONS_FIND_ALL_VA, TERRAIN_GROUPS_FILL_FERTILE_VA,
};
use crate::world_owner_frontier::{
    sha256, ExactPortTransitionProof, TransitionReceipt, WorldOwnerError, WorldSectionMask,
};
use don_sim::systems::map_terrain::{land, wflag, WorldSection};
use don_sim::systems::terrain_groups::FillFertileReceipt;

pub const PROOF_DOCUMENT: &str = "docs/assembly/replay-world-offline-localization.md";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplayWorldOwnerStage {
    Continent,
    PostContinent,
    FillFertile,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayWorldOwnerTransition {
    pub stage: ReplayWorldOwnerStage,
    pub receipt: TransitionReceipt,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReplayWorldOwnerTransitionError {
    ReceiptMismatch {
        stage: ReplayWorldOwnerStage,
        field: &'static str,
    },
    CachedMetadataMismatch,
    Ownership(WorldOwnerError),
}

impl From<WorldOwnerError> for ReplayWorldOwnerTransitionError {
    fn from(error: WorldOwnerError) -> Self {
        Self::Ownership(error)
    }
}

fn receipt_digest(label: &str, receipt: &impl std::fmt::Debug) -> [u8; 32] {
    sha256(format!("{label}\0{receipt:?}").as_bytes())
}

fn continent_implementation_digest() -> [u8; 32] {
    let mut source = Vec::with_capacity(
        include_bytes!("continent.rs").len()
            + include_bytes!("east_indies_tail.rs").len()
            + include_bytes!("team_continent_partition.rs").len(),
    );
    source.extend_from_slice(include_bytes!("continent.rs"));
    source.extend_from_slice(include_bytes!("east_indies_tail.rs"));
    source.extend_from_slice(include_bytes!("team_continent_partition.rs"));
    sha256(&source)
}

fn continent_resume_va(receipt: &ContinentReceipt) -> u32 {
    match &receipt.stop {
        ContinentStop::HookComplete { next_va } => *next_va,
        ContinentStop::MakeRegion { primitive_va, .. }
        | ContinentStop::GrowRegion { primitive_va, .. }
        | ContinentStop::FindRegionCentroid { primitive_va, .. }
        | ContinentStop::FillCont { primitive_va, .. } => *primitive_va,
        ContinentStop::EastIndiesNonplayerIslands { next_rng_va } => *next_rng_va,
        ContinentStop::RetryGeneration { .. } => receipt.make_continents_va,
    }
}

fn mismatch(stage: ReplayWorldOwnerStage, field: &'static str) -> ReplayWorldOwnerTransitionError {
    ReplayWorldOwnerTransitionError::ReceiptMismatch { stage, field }
}

fn advance(
    map: &mut InitialWorld,
    stage: ReplayWorldOwnerStage,
    proof: ExactPortTransitionProof,
) -> Result<Option<ReplayWorldOwnerTransition>, ReplayWorldOwnerTransitionError> {
    let Some(ledger) = map.ownership.as_mut() else {
        map.checksum = map.world.checksum_sections();
        return Ok(None);
    };
    let coverage = ledger.coverage();
    if map.checksum != ledger.snapshot().checksum
        || map.sourced_walked_bytes != coverage.owned_bytes as u64
    {
        return Err(ReplayWorldOwnerTransitionError::CachedMetadataMismatch);
    }
    let receipt = ledger.advance_exact_port(&map.world, proof)?;
    let coverage = ledger.coverage();
    map.checksum = ledger.snapshot().checksum.clone();
    map.sourced_walked_bytes = coverage.owned_bytes as u64;
    Ok(Some(ReplayWorldOwnerTransition { stage, receipt }))
}

/// Advance through one executed map-style virtual. The virtual may grow the
/// four start arrays and may mutate WData; no other walked section is admitted.
pub fn advance_continent_world_ownership(
    map: &mut InitialWorld,
    receipt: &ContinentReceipt,
) -> Result<Option<ReplayWorldOwnerTransition>, ReplayWorldOwnerTransitionError> {
    let stage = ReplayWorldOwnerStage::Continent;
    if receipt.make_continents_va == 0 {
        return Err(mismatch(stage, "make_continents_va"));
    }
    if !receipt.world_wiped {
        return Err(mismatch(stage, "world_wiped"));
    }
    if matches!(&receipt.stop, ContinentStop::HookComplete { next_va } if *next_va != REGIONS_CLEAR_ALL_VA)
    {
        return Err(mismatch(stage, "stop.next_va"));
    }
    if map.world.start_x.items.len() != receipt.starts_added
        || map.world.start_y.items.len() != receipt.starts_added
        || map.world.start_city_x.items.len() != receipt.starts_added.saturating_mul(4)
        || map.world.start_city_y.items.len() != receipt.starts_added.saturating_mul(4)
    {
        return Err(mismatch(stage, "starts_added"));
    }
    let input_checksum = map
        .ownership
        .as_ref()
        .map_or(map.checksum.full, |ledger| ledger.snapshot().checksum.full);
    let output_checksum = map.world.checksum_sections().full;
    advance(
        map,
        stage,
        ExactPortTransitionProof {
            entry_va: receipt.make_continents_va,
            resume_va: continent_resume_va(receipt),
            implementation_sha256: continent_implementation_digest(),
            receipt_sha256: receipt_digest("MapStyle::make_continents", receipt),
            proof_document: PROOF_DOCUMENT,
            input_checksum,
            output_checksum,
            allowed_sections: WorldSectionMask::only(WorldSection::StartArrays)
                .with(WorldSection::WData),
        },
    )
}

/// Advance the common region/diagonal/coastline chain. Regions live outside
/// the World walker; the chain's only admitted walked output is WData.
pub fn advance_post_continent_world_ownership(
    map: &mut InitialWorld,
    receipt: &PostContinentReceipt,
) -> Result<Option<ReplayWorldOwnerTransition>, ReplayWorldOwnerTransitionError> {
    let stage = ReplayWorldOwnerStage::PostContinent;
    if receipt.first_clear_va != REGIONS_CLEAR_ALL_VA
        || receipt.first_find_va != REGIONS_FIND_ALL_VA
        || receipt.fix_diag_va != MAP_FIX_DIAG_LAND_VA
        || receipt.make_coastlines_va != MAP_MAKE_COASTLINES_VA
        || receipt.second_clear_va != REGIONS_CLEAR_ALL_VA
        || receipt.second_find_va != REGIONS_FIND_ALL_VA
        || receipt.next_va != TERRAIN_GROUPS_FILL_FERTILE_VA
    {
        return Err(mismatch(stage, "schedule_va"));
    }
    let final_land_cells = map
        .world
        .wdata
        .iter()
        .filter(|cell| {
            !(cell.flags & wflag::WATERHALF == 0
                && matches!(cell.land, land::COASTAL | land::OCEAN))
        })
        .count();
    let final_coast_cells = map
        .world
        .wdata
        .iter()
        .filter(|cell| cell.flags & wflag::COAST != 0)
        .count();
    let final_ocean_cells = map.world.wdata.len().saturating_sub(final_land_cells);
    let final_half_water_cells = map
        .world
        .wdata
        .iter()
        .filter(|cell| cell.flags & wflag::WATERHALF != 0)
        .count();
    if (
        final_land_cells,
        final_coast_cells,
        final_ocean_cells,
        final_half_water_cells,
    ) != (
        receipt.final_land_cells,
        receipt.final_coast_cells,
        receipt.final_ocean_cells,
        receipt.final_half_water_cells,
    ) {
        return Err(mismatch(stage, "final_cell_census"));
    }
    let input_checksum = map
        .ownership
        .as_ref()
        .map_or(map.checksum.full, |ledger| ledger.snapshot().checksum.full);
    let output_checksum = map.world.checksum_sections().full;
    advance(
        map,
        stage,
        ExactPortTransitionProof {
            entry_va: receipt.first_clear_va,
            resume_va: receipt.next_va,
            implementation_sha256: sha256(include_bytes!("post_continent.rs")),
            receipt_sha256: receipt_digest("Map::post_continent", receipt),
            proof_document: PROOF_DOCUMENT,
            input_checksum,
            output_checksum,
            allowed_sections: WorldSectionMask::only(WorldSection::WData),
        },
    )
}

/// Advance the instruction-complete fertility pass. It writes only the WData
/// `land_sub` byte (and redundantly rewrites the unchanged `land` byte).
pub fn advance_fill_fertile_world_ownership(
    map: &mut InitialWorld,
    receipt: &FillFertileReceipt,
) -> Result<Option<ReplayWorldOwnerTransition>, ReplayWorldOwnerTransitionError> {
    let stage = ReplayWorldOwnerStage::FillFertile;
    let fertile_cells = map
        .world
        .wdata
        .iter()
        .filter(|cell| cell.land == land::FERTILE)
        .count();
    if receipt.fertile_cells < 0 || receipt.fertile_cells as usize != fertile_cells {
        return Err(mismatch(stage, "fertile_cells"));
    }
    let input_checksum = map
        .ownership
        .as_ref()
        .map_or(map.checksum.full, |ledger| ledger.snapshot().checksum.full);
    let output_checksum = map.world.checksum_sections().full;
    advance(
        map,
        stage,
        ExactPortTransitionProof {
            entry_va: TERRAIN_GROUPS_FILL_FERTILE_VA,
            resume_va: crate::fractal_boundary::TERRAIN_GROUPS_PLACE_ALL_VA,
            implementation_sha256: sha256(include_bytes!(
                "../../don-sim/src/systems/terrain_groups.rs"
            )),
            receipt_sha256: receipt_digest("TerrainGroups::fill_fertile", receipt),
            proof_document: PROOF_DOCUMENT,
            input_checksum,
            output_checksum,
            allowed_sections: WorldSectionMask::only(WorldSection::WData),
        },
    )
}
