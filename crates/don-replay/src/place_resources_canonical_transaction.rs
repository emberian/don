// SPDX-License-Identifier: GPL-3.0-or-later
//! Canonical source-only transaction for the supported `Map::place_resources` body.
//!
//! This owner composes the registered XML row owners, the category cleanup owner,
//! and the exact Player / concrete-World placement bodies.  It deliberately does
//! not install a Goods or World replay channel: allocation hosts remain
//! observational/two-phase until the complete caller schedule is source-owned.

use don_sim::rng::Random;

use crate::place_player_resource_body_frontier::{
    execute_player_resource_body, PlayerAllocationHost, PlayerAllocationKind,
    PlayerAllocationReceipt, PlayerAllocationRequest, PlayerBodyPlacementEvidence,
    PlayerResourceBodyError, PlayerResourceBodyFacts, PlayerResourceBodyParameters,
    PlayerResourceBodyReceipt, PlayerResourceBodyState,
};
use crate::place_region_resource_world_body_frontier::{
    execute_region_world_body, RegionBodyPlacementEvidence, RegionInitGoodHost,
    RegionInitGoodReceipt, RegionInitGoodRequest, RegionWorldBodyError, RegionWorldBodyFacts,
    RegionWorldBodyParameters, RegionWorldBodyReceipt, RegionWorldBodyState,
};
use crate::place_resources_bonus_mutation_frontier::{
    execute_first_bonus_mutation, AllocationKind, CalleeRandomDraw, FirstBonusMutationError,
    FirstBonusMutationFacts, FirstBonusMutationReceipt, PlaceResourcesBonusMutationState,
    PlacementEvidence, PlacementHost, PlacementPath, PlacementPattern, PlacementReceipt,
    PlacementRequest, ResourceAllocation, WorldOccupancyWrite, FIRST_BONUS_ROW_BODY_VA,
};
use crate::place_resources_bonus_rows_mutation_frontier::{
    execute_carried_category_first_mutation, execute_next_bonus_mutation,
    CarriedCategoryFirstMutationReceipt, LaterBonusMutationFacts, LaterBonusMutationReceipt,
    RemainingBonusRowsError, RemainingBonusRowsState,
};
use crate::place_resources_category_frontier::{
    advance_completed_category, execute_player_placement_prefix, execute_region_placement_prefix,
    CategoryAdvanceDisposition, CategoryAdvanceError, CategoryAdvanceFacts, CategoryAdvanceReceipt,
    CategoryLoopState, DeterministicResourceState, HostHandles, PlacementPrefixError,
    PlayerPlacementPrefixFacts, PlayerPlacementPrefixReceipt, PlayerPlacementPrefixRequest,
    RegionPlacementPrefixFacts, RegionPlacementPrefixReceipt, RegionPlacementPrefixRequest,
    ResourceCategory, SectionSource,
};
use crate::place_resources_pool_frontier::{resource_divvy_pool_digest, ResourceDivvyPoolState};
use crate::place_resources_xml_frontier::{
    BonusXmlRowFact, BonusesSectionSource, PlaceResourcesBonusRowsHandoff, XmlHostHandles,
    PLACE_RESOURCES_XML_RESIDUAL_VA,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalPlaceResourcesState {
    pub mutation: PlaceResourcesBonusMutationState,
    pub good_state_digest: u64,
    pub item_state_digest: u64,
    pub selected_document_handles: HostHandles,
    pub default_document_handles: HostHandles,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CanonicalRowMutationFacts {
    /// Legal only for BONUSES row zero.
    InitialBonus(FirstBonusMutationFacts),
    /// Legal for every recurrence row and for row zero of FISH / GOODIES.
    CarriedOrLater(LaterBonusMutationFacts),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CanonicalPlacementFacts {
    Player {
        prefix: PlayerPlacementPrefixFacts,
        body: PlayerResourceBodyFacts,
    },
    RegionWorld {
        prefix: RegionPlacementPrefixFacts,
        body: RegionWorldBodyFacts,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalRowFacts {
    pub mutation: CanonicalRowMutationFacts,
    /// Must be present exactly when the row reaches a placement call.
    pub placement: Option<CanonicalPlacementFacts>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalPlaceResourcesFacts {
    pub bonuses: Vec<CanonicalRowFacts>,
    pub bonuses_tail: CategoryAdvanceFacts,
    pub fish: Vec<CanonicalRowFacts>,
    pub fish_tail: CategoryAdvanceFacts,
    pub goodies: Vec<CanonicalRowFacts>,
    pub goodies_tail: CategoryAdvanceFacts,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CanonicalExactPlacementReceipt {
    Player {
        prefix: PlayerPlacementPrefixReceipt,
        body: PlayerResourceBodyReceipt,
    },
    RegionWorld {
        prefix: RegionPlacementPrefixReceipt,
        body: RegionWorldBodyReceipt,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CanonicalRowMutationReceipt {
    InitialBonus(FirstBonusMutationReceipt),
    CarriedFirst(CarriedCategoryFirstMutationReceipt),
    Later(LaterBonusMutationReceipt),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalRowReceipt {
    pub category: ResourceCategory,
    pub row_index: usize,
    pub mutation: CanonicalRowMutationReceipt,
    pub exact_placement: Option<CanonicalExactPlacementReceipt>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalCategoryReceipt {
    pub category: ResourceCategory,
    pub rows: Vec<CanonicalRowReceipt>,
    pub tail: CategoryAdvanceReceipt,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalPlaceResourcesReceipt {
    pub categories: Vec<CanonicalCategoryReceipt>,
    pub deterministic_before: DeterministicResourceState,
    pub deterministic_after: DeterministicResourceState,
    pub resource_pool_before: ResourceDivvyPoolState,
    pub resource_pool_after: ResourceDivvyPoolState,
    pub good_state_digest_before: u64,
    pub good_state_digest_after: u64,
    pub item_state_digest_before: u64,
    pub item_state_digest_after: u64,
    pub return_count: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CanonicalPlaceResourcesError {
    WrongHandoff,
    WrongCategoryPlan { category: ResourceCategory },
    MissingExactPlacement,
    UnexpectedExactPlacement,
    PlacementProjectionMismatch,
    ExactPlacementStateMismatch,
    FirstRow(FirstBonusMutationError),
    RemainingRow(RemainingBonusRowsError),
    Category(CategoryAdvanceError),
    Prefix(PlacementPrefixError),
    PlayerBody(PlayerResourceBodyError),
    RegionBody(RegionWorldBodyError),
    AllocationTranscriptMismatch,
}

pub trait CanonicalResourceAllocationHost: PlayerAllocationHost + RegionInitGoodHost {}

impl<T> CanonicalResourceAllocationHost for T where T: PlayerAllocationHost + RegionInitGoodHost {}

/// Exact two-phase host proposals accepted while executing one canonical row.
/// A later schedule continuation replays these proposals before trusting the
/// stored row receipt; no Good/Item digest transition is reverse-engineered.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CanonicalAllocationTranscript {
    pub player: Vec<PlayerAllocationReceipt>,
    pub region: Vec<RegionInitGoodReceipt>,
}

struct RecordingAllocationHost<'a, H> {
    inner: &'a mut H,
    transcript: CanonicalAllocationTranscript,
}

impl<H: PlayerAllocationHost> PlayerAllocationHost for RecordingAllocationHost<'_, H> {
    fn propose_allocation(
        &mut self,
        request: &PlayerAllocationRequest,
    ) -> Option<PlayerAllocationReceipt> {
        let receipt = self.inner.propose_allocation(request)?;
        self.transcript.player.push(receipt.clone());
        Some(receipt)
    }
}

impl<H: RegionInitGoodHost> RegionInitGoodHost for RecordingAllocationHost<'_, H> {
    fn propose_init_good(
        &mut self,
        request: &RegionInitGoodRequest,
    ) -> Option<RegionInitGoodReceipt> {
        let receipt = self.inner.propose_init_good(request)?;
        self.transcript.region.push(receipt.clone());
        Some(receipt)
    }
}

struct PlaybackAllocationHost<'a> {
    transcript: &'a CanonicalAllocationTranscript,
    player_index: usize,
    region_index: usize,
}

impl PlayerAllocationHost for PlaybackAllocationHost<'_> {
    fn propose_allocation(
        &mut self,
        request: &PlayerAllocationRequest,
    ) -> Option<PlayerAllocationReceipt> {
        let receipt = self.transcript.player.get(self.player_index)?;
        if receipt.request != *request {
            return None;
        }
        self.player_index += 1;
        Some(receipt.clone())
    }
}

impl RegionInitGoodHost for PlaybackAllocationHost<'_> {
    fn propose_init_good(
        &mut self,
        request: &RegionInitGoodRequest,
    ) -> Option<RegionInitGoodReceipt> {
        let receipt = self.transcript.region.get(self.region_index)?;
        if receipt.request != *request {
            return None;
        }
        self.region_index += 1;
        Some(receipt.clone())
    }
}

fn xml_handles(handles: HostHandles) -> XmlHostHandles {
    XmlHostHandles {
        head: handles.head,
        tail: handles.tail,
        inline_tail_word: handles.inline_tail_word,
    }
}

fn category_handles(handles: XmlHostHandles) -> HostHandles {
    HostHandles {
        head: handles.head,
        tail: handles.tail,
        inline_tail_word: handles.inline_tail_word,
    }
}

fn section_source(source: SectionSource) -> BonusesSectionSource {
    match source {
        SectionSource::Selected => BonusesSectionSource::SelectedStyle,
        SectionSource::Default => BonusesSectionSource::DefaultStyle,
        SectionSource::MissingDefault => BonusesSectionSource::MissingDefault,
    }
}

fn deterministic(
    mutation: &PlaceResourcesBonusMutationState,
    last_chance_group: i32,
    signed_chance_budget: i32,
    winner_seen: bool,
) -> DeterministicResourceState {
    DeterministicResourceState {
        random_state: mutation.random_state,
        world_checksum: mutation.world_checksum.clone(),
        sourced_walked_bytes: mutation.sourced_walked_bytes,
        resource_pool_digest: mutation.resource_pool_digest,
        allocated_resources: mutation.allocated_resources,
        requested_resources: mutation.requested_resources,
        last_chance_group,
        signed_chance_budget,
        winner_seen,
    }
}

fn mutation_matches_deterministic(
    mutation: &PlaceResourcesBonusMutationState,
    state: &DeterministicResourceState,
) -> bool {
    mutation.random_state == state.random_state
        && mutation.world_checksum == state.world_checksum
        && mutation.sourced_walked_bytes == state.sourced_walked_bytes
        && mutation.resource_pool_digest == state.resource_pool_digest
        && mutation.allocated_resources == state.allocated_resources
        && mutation.requested_resources == state.requested_resources
}

fn placement_state_initial(
    mutation: &PlaceResourcesBonusMutationState,
    facts: &FirstBonusMutationFacts,
) -> DeterministicResourceState {
    let mut random = Random::new(mutation.random_state);
    let mut budget = 0;
    if facts.chance_group != -1 {
        budget = random.get(0, 0xffff) % 100;
    }
    budget = budget.wrapping_sub(facts.chance);
    let mut out = deterministic(mutation, facts.chance_group, budget, true);
    out.random_state = random.state();
    out
}

fn placement_state_continued(
    state: &RemainingBonusRowsState,
    facts: &LaterBonusMutationFacts,
) -> DeterministicResourceState {
    let mut random = Random::new(state.mutation.random_state);
    let mut group = state.last_chance_group;
    let mut budget = state.signed_chance_budget;
    if facts.chance_group != group || facts.chance_group == 0 {
        budget = random.get(0, 0xffff) % 100;
        group = facts.chance_group;
    }
    budget = budget.wrapping_sub(facts.chance);
    let mut out = deterministic(&state.mutation, group, budget, true);
    out.random_state = random.state();
    out
}

fn callee_draw(
    call_va: u32,
    random_get_va: u32,
    low: i32,
    high: i32,
    state_before: i32,
    raw: i32,
    state_after: i32,
) -> CalleeRandomDraw {
    CalleeRandomDraw {
        call_va,
        random_get_va,
        low,
        high,
        state_before,
        raw,
        state_after,
    }
}

fn generic_player_evidence(evidence: &PlayerBodyPlacementEvidence) -> PlacementEvidence {
    match evidence {
        PlayerBodyPlacementEvidence::RetailCapture {
            executable_sha256,
            capture_sha256,
        } => PlacementEvidence::RetailCapture {
            executable_sha256: executable_sha256.clone(),
            capture_sha256: *capture_sha256,
        },
        PlayerBodyPlacementEvidence::ExactPort {
            implementation_sha256,
            proof_document,
        } => PlacementEvidence::ExactPort {
            implementation_sha256: *implementation_sha256,
            proof_document: proof_document.clone(),
        },
        PlayerBodyPlacementEvidence::SyntheticFixture { fixture } => {
            PlacementEvidence::SyntheticFixture {
                fixture: fixture.clone(),
            }
        }
    }
}

fn generic_region_evidence(evidence: &RegionBodyPlacementEvidence) -> PlacementEvidence {
    match evidence {
        RegionBodyPlacementEvidence::RetailCapture {
            executable_sha256,
            capture_sha256,
        } => PlacementEvidence::RetailCapture {
            executable_sha256: executable_sha256.clone(),
            capture_sha256: *capture_sha256,
        },
        RegionBodyPlacementEvidence::ExactPort {
            implementation_sha256,
            proof_document,
        } => PlacementEvidence::ExactPort {
            implementation_sha256: *implementation_sha256,
            proof_document: proof_document.clone(),
        },
        RegionBodyPlacementEvidence::SyntheticFixture { fixture } => {
            PlacementEvidence::SyntheticFixture {
                fixture: fixture.clone(),
            }
        }
    }
}

fn generic_player_allocation(
    allocation: &crate::place_player_resource_body_frontier::PlayerResourceAllocation,
) -> ResourceAllocation {
    let (kind, type_id) = match allocation.kind {
        PlayerAllocationKind::Good { good_id } => (AllocationKind::Good, good_id),
        PlayerAllocationKind::Item => (
            AllocationKind::Item,
            crate::place_player_resource_body_frontier::ITEM_GOOD_ID,
        ),
    };
    ResourceAllocation {
        call_va: allocation.call_va,
        callee_va: allocation.callee_va,
        kind,
        type_id,
        coord_x: allocation.coord_x,
        coord_y: allocation.coord_y,
        slot: allocation.slot,
        occupancy_write: allocation
            .occupancy_write
            .as_ref()
            .map(|write| WorldOccupancyWrite {
                world_x: write.world_x,
                world_y: write.world_y,
                down_write_va: write.down_write_va,
                down_who_write_va: write.down_who_write_va,
                old_down: write.old_down,
                old_down_who: write.old_down_who,
                new_down: write.new_down,
                new_down_who: write.new_down_who,
            }),
    }
}

fn generic_region_allocation(
    allocation: &crate::place_region_resource_world_body_frontier::RegionGoodAllocation,
) -> ResourceAllocation {
    ResourceAllocation {
        call_va: allocation.call_va,
        callee_va: allocation.callee_va,
        kind: AllocationKind::Good,
        type_id: allocation.good_id,
        coord_x: allocation.coord_x,
        coord_y: allocation.coord_y,
        slot: allocation.slot,
        occupancy_write: allocation
            .occupancy_write
            .as_ref()
            .map(|write| WorldOccupancyWrite {
                world_x: write.world_x,
                world_y: write.world_y,
                down_write_va: write.down_write_va,
                down_who_write_va: write.down_who_write_va,
                old_down: write.old_down,
                old_down_who: write.old_down_who,
                new_down: write.new_down,
                new_down_who: write.new_down_who,
            }),
    }
}

struct ExactPlacementAdapter<'a, H> {
    facts: Option<&'a CanonicalPlacementFacts>,
    deterministic: DeterministicResourceState,
    pool: ResourceDivvyPoolState,
    good_state_digest: u64,
    item_state_digest: u64,
    host: &'a mut H,
    failure: Option<CanonicalPlaceResourcesError>,
    exact: Option<CanonicalExactPlacementReceipt>,
    deterministic_after: Option<DeterministicResourceState>,
    pool_after: Option<ResourceDivvyPoolState>,
    good_state_digest_after: Option<u64>,
    item_state_digest_after: Option<u64>,
}

impl<'a, H> ExactPlacementAdapter<'a, H> {
    fn fail(&mut self, error: CanonicalPlaceResourcesError) -> Option<PlacementReceipt> {
        self.failure = Some(error);
        None
    }

    fn request_matches(&self, request: &PlacementRequest) -> bool {
        request.random_state_before == self.deterministic.random_state
            && request.world_checksum_before == self.deterministic.world_checksum
            && request.sourced_walked_bytes == self.deterministic.sourced_walked_bytes
            && request.resource_pool_digest_before == self.deterministic.resource_pool_digest
            && request.resource_pool_before.as_ref() == Some(&self.pool)
            && resource_divvy_pool_digest(&self.pool) == self.deterministic.resource_pool_digest
    }
}

impl<H: CanonicalResourceAllocationHost> PlacementHost for ExactPlacementAdapter<'_, H> {
    fn place(&mut self, request: &PlacementRequest) -> Option<PlacementReceipt> {
        if !self.request_matches(request) {
            return self.fail(CanonicalPlaceResourcesError::PlacementProjectionMismatch);
        }
        let Some(facts) = self.facts else {
            return self.fail(CanonicalPlaceResourcesError::MissingExactPlacement);
        };
        match facts {
            CanonicalPlacementFacts::Player { prefix, body } => {
                if request.path != PlacementPath::Player
                    || request.params.pattern != PlacementPattern::Player
                {
                    return self.fail(CanonicalPlaceResourcesError::PlacementProjectionMismatch);
                }
                let parameters = PlayerResourceBodyParameters {
                    player_count: request.params.player_count,
                    good_id: request.params.good_id,
                    num_rare: request.params.num_rare,
                    player_keep_away: request.params.player_keep_away,
                    player_stay_near: request.params.player_stay_near,
                    existing_resource_spacing: request.params.spacing,
                    placed_resource_spacing: request.params.group_spacing,
                    center_keep_away: request.params.center_keep_away,
                    center_stay_near: request.params.center_stay_near,
                    edge_keep_away: request.params.edge_keep_away,
                    edge_stay_near: request.params.edge_stay_near,
                    corner_keep_away: request.params.corner_keep_away,
                    corner_stay_near: request.params.corner_stay_near,
                    selector: request.params.selector,
                };
                let prefix_request = PlayerPlacementPrefixRequest {
                    player_count: parameters.player_count,
                    good_id: parameters.good_id,
                    num_rare: parameters.num_rare,
                    spacing: parameters.player_keep_away,
                    group_spacing: parameters.player_stay_near,
                    selector: parameters.selector,
                };
                let prefix_receipt = match execute_player_placement_prefix(
                    &self.deterministic,
                    prefix_request,
                    prefix,
                ) {
                    Ok(receipt) => receipt,
                    Err(error) => return self.fail(CanonicalPlaceResourcesError::Prefix(error)),
                };
                let mut body_state = PlayerResourceBodyState {
                    deterministic: self.deterministic.clone(),
                    resource_pool: self.pool.clone(),
                    good_state_digest: self.good_state_digest,
                    item_state_digest: self.item_state_digest,
                };
                let body_receipt = match execute_player_resource_body(
                    &mut body_state,
                    parameters,
                    prefix,
                    &prefix_receipt,
                    body,
                    self.host,
                ) {
                    Ok(receipt) => receipt,
                    Err(error) => {
                        return self.fail(CanonicalPlaceResourcesError::PlayerBody(error));
                    }
                };
                let mut random_draws = Vec::new();
                for attempt in &body_receipt.attempts {
                    if let Some(selection) = &attempt.pool_selection {
                        random_draws.extend(selection.random_draws.iter().map(|draw| {
                            callee_draw(
                                draw.call_va,
                                draw.random_get_va,
                                draw.low,
                                draw.high,
                                draw.state_before,
                                draw.raw,
                                draw.state_after,
                            )
                        }));
                    }
                    if let Some(draw) = attempt.point_draw {
                        random_draws.push(callee_draw(
                            draw.call_va,
                            draw.random_get_va,
                            draw.low,
                            draw.high,
                            draw.state_before,
                            draw.raw,
                            draw.state_after,
                        ));
                    }
                }
                let allocations = body_receipt
                    .allocations
                    .iter()
                    .map(generic_player_allocation)
                    .collect::<Vec<_>>();
                let placement = PlacementReceipt {
                    request: request.clone(),
                    random_draws,
                    random_state_after: body_receipt.deterministic_after.random_state,
                    allocated_count: body_receipt.return_count,
                    allocations,
                    world_checksum_after: body_receipt.deterministic_after.world_checksum.clone(),
                    sourced_walked_bytes_after: body_receipt
                        .deterministic_after
                        .sourced_walked_bytes,
                    resource_pool_digest_after: body_receipt
                        .deterministic_after
                        .resource_pool_digest,
                    resource_pool_selections: body_receipt.pool_selections.clone(),
                    resource_pool_after: Some(body_receipt.resource_pool_after.clone()),
                    evidence: generic_player_evidence(&body_receipt.placement_evidence),
                };
                self.deterministic_after = Some(body_receipt.deterministic_after.clone());
                self.pool_after = Some(body_receipt.resource_pool_after.clone());
                self.good_state_digest_after = Some(body_receipt.good_state_digest_after);
                self.item_state_digest_after = Some(body_receipt.item_state_digest_after);
                self.exact = Some(CanonicalExactPlacementReceipt::Player {
                    prefix: prefix_receipt,
                    body: body_receipt,
                });
                Some(placement)
            }
            CanonicalPlacementFacts::RegionWorld { prefix, body } => {
                if request.path != PlacementPath::Region
                    || request.params.pattern != PlacementPattern::World
                    || request.params.selector != 0
                {
                    return self.fail(CanonicalPlaceResourcesError::PlacementProjectionMismatch);
                }
                let parameters = RegionWorldBodyParameters {
                    good_id: request.params.good_id,
                    pattern: request.params.pattern as i32,
                    saturate: request.params.saturate,
                    num_rare: request.params.num_rare,
                    selector: request.params.selector,
                    player_keep_away: request.params.player_keep_away,
                    player_stay_near: request.params.player_stay_near,
                    existing_resource_spacing: request.params.spacing,
                    group_spacing: request.params.group_spacing,
                    center_keep_away: request.params.center_keep_away,
                    center_stay_near: request.params.center_stay_near,
                    edge_keep_away: request.params.edge_keep_away,
                    edge_stay_near: request.params.edge_stay_near,
                    corner_keep_away: request.params.corner_keep_away,
                    corner_stay_near: request.params.corner_stay_near,
                };
                let prefix_request = RegionPlacementPrefixRequest {
                    good_id: parameters.good_id,
                    pattern: parameters.pattern,
                    saturate: parameters.saturate,
                    num_rare: parameters.num_rare,
                    selector: parameters.selector,
                };
                let prefix_receipt = match execute_region_placement_prefix(
                    &self.deterministic,
                    prefix_request,
                    prefix,
                ) {
                    Ok(receipt) => receipt,
                    Err(error) => return self.fail(CanonicalPlaceResourcesError::Prefix(error)),
                };
                let mut body_state = RegionWorldBodyState {
                    deterministic: self.deterministic.clone(),
                    good_state_digest: self.good_state_digest,
                };
                let body_receipt = match execute_region_world_body(
                    &mut body_state,
                    parameters,
                    &prefix_receipt,
                    body,
                    self.host,
                ) {
                    Ok(receipt) => receipt,
                    Err(error) => {
                        return self.fail(CanonicalPlaceResourcesError::RegionBody(error));
                    }
                };
                let random_draws = body_receipt
                    .random_draws
                    .iter()
                    .map(|draw| {
                        callee_draw(
                            draw.call_va,
                            draw.random_get_va,
                            draw.low,
                            draw.high,
                            draw.state_before,
                            draw.raw,
                            draw.state_after,
                        )
                    })
                    .collect();
                let allocations = body_receipt
                    .allocations
                    .iter()
                    .map(generic_region_allocation)
                    .collect::<Vec<_>>();
                let placement = PlacementReceipt {
                    request: request.clone(),
                    random_draws,
                    random_state_after: body_receipt.deterministic_after.random_state,
                    allocated_count: body_receipt.return_count,
                    allocations,
                    world_checksum_after: body_receipt.deterministic_after.world_checksum.clone(),
                    sourced_walked_bytes_after: body_receipt
                        .deterministic_after
                        .sourced_walked_bytes,
                    resource_pool_digest_after: body_receipt
                        .deterministic_after
                        .resource_pool_digest,
                    resource_pool_selections: Vec::new(),
                    resource_pool_after: Some(self.pool.clone()),
                    evidence: generic_region_evidence(&body_receipt.placement_evidence),
                };
                self.deterministic_after = Some(body_receipt.deterministic_after.clone());
                self.pool_after = Some(self.pool.clone());
                self.good_state_digest_after = Some(body_receipt.good_state_digest_after);
                self.item_state_digest_after = Some(self.item_state_digest);
                self.exact = Some(CanonicalExactPlacementReceipt::RegionWorld {
                    prefix: prefix_receipt,
                    body: body_receipt,
                });
                Some(placement)
            }
        }
    }
}

fn finish_adapter<H>(
    state: &mut CanonicalPlaceResourcesState,
    carried: &RemainingBonusRowsState,
    facts: &CanonicalRowFacts,
    adapter: ExactPlacementAdapter<'_, H>,
) -> Result<Option<CanonicalExactPlacementReceipt>, CanonicalPlaceResourcesError> {
    if let Some(error) = adapter.failure {
        return Err(error);
    }
    if adapter.exact.is_none() != facts.placement.is_none() {
        return Err(if facts.placement.is_some() {
            CanonicalPlaceResourcesError::UnexpectedExactPlacement
        } else {
            CanonicalPlaceResourcesError::MissingExactPlacement
        });
    }
    if let Some(exact_after) = adapter.deterministic_after {
        if exact_after.random_state != carried.mutation.random_state
            || exact_after.world_checksum != carried.mutation.world_checksum
            || exact_after.sourced_walked_bytes != carried.mutation.sourced_walked_bytes
            || exact_after.resource_pool_digest != carried.mutation.resource_pool_digest
            || exact_after.allocated_resources != carried.mutation.allocated_resources
            || exact_after.last_chance_group != carried.last_chance_group
            || exact_after.signed_chance_budget != carried.signed_chance_budget
            || exact_after.winner_seen != carried.winner_seen
        {
            return Err(CanonicalPlaceResourcesError::ExactPlacementStateMismatch);
        }
        state.good_state_digest = adapter
            .good_state_digest_after
            .ok_or(CanonicalPlaceResourcesError::ExactPlacementStateMismatch)?;
        state.item_state_digest = adapter
            .item_state_digest_after
            .ok_or(CanonicalPlaceResourcesError::ExactPlacementStateMismatch)?;
        state.mutation.resource_pool = Some(
            adapter
                .pool_after
                .ok_or(CanonicalPlaceResourcesError::ExactPlacementStateMismatch)?,
        );
    }
    Ok(adapter.exact)
}

fn category_handoff(
    prior: &PlaceResourcesBonusRowsHandoff,
    loop_state: &CategoryLoopState,
    source: SectionSource,
    rows: &[crate::place_resources_category_frontier::CategoryRowFact],
) -> PlaceResourcesBonusRowsHandoff {
    PlaceResourcesBonusRowsHandoff {
        resume_va: prior.resume_va,
        first_row_body_va: prior.first_row_body_va,
        player_count_argument: prior.player_count_argument,
        section_source: section_source(source),
        current_category_handles: xml_handles(loop_state.category_handles),
        rows: rows
            .iter()
            .map(|row| BonusXmlRowFact {
                capture_ordinal: row.capture_ordinal,
                element_name: row.element_name.clone(),
                handles: xml_handles(row.handles),
            })
            .collect(),
        selected_document_live: loop_state.selected_document_handles.valid(),
        default_document_live: loop_state.default_document_handles.valid(),
        random_state: loop_state.deterministic.random_state,
        world_checksum: loop_state.deterministic.world_checksum.clone(),
        sourced_walked_bytes: loop_state.deterministic.sourced_walked_bytes,
        resource_pool_digest: loop_state.deterministic.resource_pool_digest,
    }
}

fn continued_state(
    entry: &PlaceResourcesBonusRowsHandoff,
    deterministic: &DeterministicResourceState,
    pool: &ResourceDivvyPoolState,
) -> RemainingBonusRowsState {
    RemainingBonusRowsState {
        next_row_index: 0,
        section_source: entry.section_source,
        rows: entry.rows.clone(),
        last_chance_group: deterministic.last_chance_group,
        signed_chance_budget: deterministic.signed_chance_budget,
        winner_seen: deterministic.winner_seen,
        mutation: PlaceResourcesBonusMutationState {
            random_state: deterministic.random_state,
            world_checksum: deterministic.world_checksum.clone(),
            sourced_walked_bytes: deterministic.sourced_walked_bytes,
            resource_pool_digest: deterministic.resource_pool_digest,
            resource_pool: Some(pool.clone()),
            allocated_resources: deterministic.allocated_resources,
            requested_resources: deterministic.requested_resources,
        },
    }
}

fn run_initial_row<H: CanonicalResourceAllocationHost>(
    state: &mut CanonicalPlaceResourcesState,
    entry: &PlaceResourcesBonusRowsHandoff,
    facts: &CanonicalRowFacts,
    host: &mut H,
) -> Result<(CanonicalRowReceipt, RemainingBonusRowsState), CanonicalPlaceResourcesError> {
    let CanonicalRowMutationFacts::InitialBonus(mutation_facts) = &facts.mutation else {
        return Err(CanonicalPlaceResourcesError::WrongCategoryPlan {
            category: ResourceCategory::Bonuses,
        });
    };
    let pool = state
        .mutation
        .resource_pool
        .clone()
        .ok_or(CanonicalPlaceResourcesError::WrongHandoff)?;
    let body_deterministic = placement_state_initial(&state.mutation, mutation_facts);
    let mut adapter = ExactPlacementAdapter {
        facts: facts.placement.as_ref(),
        deterministic: body_deterministic,
        pool,
        good_state_digest: state.good_state_digest,
        item_state_digest: state.item_state_digest,
        host,
        failure: None,
        exact: None,
        deterministic_after: None,
        pool_after: None,
        good_state_digest_after: None,
        item_state_digest_after: None,
    };
    let receipt =
        execute_first_bonus_mutation(&mut state.mutation, entry, mutation_facts, &mut adapter)
            .map_err(|error| {
                adapter
                    .failure
                    .take()
                    .unwrap_or(CanonicalPlaceResourcesError::FirstRow(error))
            })?;
    let carried =
        RemainingBonusRowsState::from_first_row(entry, mutation_facts, &receipt, &state.mutation)
            .map_err(CanonicalPlaceResourcesError::RemainingRow)?;
    let exact = finish_adapter(state, &carried, facts, adapter)?;
    Ok((
        CanonicalRowReceipt {
            category: ResourceCategory::Bonuses,
            row_index: 0,
            mutation: CanonicalRowMutationReceipt::InitialBonus(receipt),
            exact_placement: exact,
        },
        carried,
    ))
}

fn run_continued_row<H: CanonicalResourceAllocationHost>(
    state: &mut CanonicalPlaceResourcesState,
    entry: &PlaceResourcesBonusRowsHandoff,
    category: ResourceCategory,
    row_index: usize,
    carried: &mut RemainingBonusRowsState,
    facts: &CanonicalRowFacts,
    host: &mut H,
) -> Result<CanonicalRowReceipt, CanonicalPlaceResourcesError> {
    let CanonicalRowMutationFacts::CarriedOrLater(mutation_facts) = &facts.mutation else {
        return Err(CanonicalPlaceResourcesError::WrongCategoryPlan { category });
    };
    let pool = carried
        .mutation
        .resource_pool
        .clone()
        .ok_or(CanonicalPlaceResourcesError::WrongHandoff)?;
    let body_deterministic = placement_state_continued(carried, mutation_facts);
    let mut adapter = ExactPlacementAdapter {
        facts: facts.placement.as_ref(),
        deterministic: body_deterministic,
        pool,
        good_state_digest: state.good_state_digest,
        item_state_digest: state.item_state_digest,
        host,
        failure: None,
        exact: None,
        deterministic_after: None,
        pool_after: None,
        good_state_digest_after: None,
        item_state_digest_after: None,
    };
    let mutation_receipt = if category != ResourceCategory::Bonuses && row_index == 0 {
        CanonicalRowMutationReceipt::CarriedFirst(
            execute_carried_category_first_mutation(
                carried,
                entry,
                category,
                mutation_facts,
                &mut adapter,
            )
            .map_err(|error| {
                adapter
                    .failure
                    .take()
                    .unwrap_or(CanonicalPlaceResourcesError::RemainingRow(error))
            })?,
        )
    } else {
        CanonicalRowMutationReceipt::Later(
            execute_next_bonus_mutation(carried, entry, mutation_facts, &mut adapter).map_err(
                |error| {
                    adapter
                        .failure
                        .take()
                        .unwrap_or(CanonicalPlaceResourcesError::RemainingRow(error))
                },
            )?,
        )
    };
    state.mutation = carried.mutation.clone();
    let exact = finish_adapter(state, carried, facts, adapter)?;
    Ok(CanonicalRowReceipt {
        category,
        row_index,
        mutation: mutation_receipt,
        exact_placement: exact,
    })
}

/// Execute exactly row zero of a carried FISH or GOODIES category. The caller
/// supplies the category handoff and chance carry produced by the preceding
/// exact category cleanup. Both canonical state projections are staged and
/// commit together only after the generic row receipt agrees with the exact
/// Player/Region body receipt.
pub fn execute_canonical_carried_first_row<H: CanonicalResourceAllocationHost>(
    state: &mut CanonicalPlaceResourcesState,
    entry: &PlaceResourcesBonusRowsHandoff,
    category: ResourceCategory,
    carried: &mut RemainingBonusRowsState,
    facts: &CanonicalRowFacts,
    host: &mut H,
) -> Result<CanonicalRowReceipt, CanonicalPlaceResourcesError> {
    if category == ResourceCategory::Bonuses
        || carried.next_row_index != 0
        || entry.rows.is_empty()
        || carried.rows != entry.rows
        || state.mutation != carried.mutation
        || state.selected_document_handles.valid() != entry.selected_document_live
        || state.default_document_handles.valid() != entry.default_document_live
    {
        return Err(CanonicalPlaceResourcesError::WrongHandoff);
    }
    let mut staged_state = state.clone();
    let mut staged_carried = carried.clone();
    let receipt = run_continued_row(
        &mut staged_state,
        entry,
        category,
        0,
        &mut staged_carried,
        facts,
        host,
    )?;
    *state = staged_state;
    *carried = staged_carried;
    Ok(receipt)
}

/// Execute the same carried row while retaining every accepted two-phase
/// allocation proposal needed to replay an exact placed body later.
pub fn execute_canonical_carried_first_row_recorded<H: CanonicalResourceAllocationHost>(
    state: &mut CanonicalPlaceResourcesState,
    entry: &PlaceResourcesBonusRowsHandoff,
    category: ResourceCategory,
    carried: &mut RemainingBonusRowsState,
    facts: &CanonicalRowFacts,
    host: &mut H,
) -> Result<(CanonicalRowReceipt, CanonicalAllocationTranscript), CanonicalPlaceResourcesError> {
    let mut recording = RecordingAllocationHost {
        inner: host,
        transcript: CanonicalAllocationTranscript::default(),
    };
    let receipt = execute_canonical_carried_first_row(
        state,
        entry,
        category,
        carried,
        facts,
        &mut recording,
    )?;
    Ok((receipt, recording.transcript))
}

/// Re-execute one stored carried row from its exact before-image and allocation
/// transcript. State commits only if the complete receipt and transcript usage
/// match; extra, missing, or request-detached proposals fail closed.
pub fn replay_canonical_carried_first_row(
    state: &mut CanonicalPlaceResourcesState,
    entry: &PlaceResourcesBonusRowsHandoff,
    category: ResourceCategory,
    carried: &mut RemainingBonusRowsState,
    facts: &CanonicalRowFacts,
    expected: &CanonicalRowReceipt,
    transcript: &CanonicalAllocationTranscript,
) -> Result<(), CanonicalPlaceResourcesError> {
    let mut staged_state = state.clone();
    let mut staged_carried = carried.clone();
    let mut playback = PlaybackAllocationHost {
        transcript,
        player_index: 0,
        region_index: 0,
    };
    let actual = execute_canonical_carried_first_row(
        &mut staged_state,
        entry,
        category,
        &mut staged_carried,
        facts,
        &mut playback,
    )?;
    if actual != *expected
        || playback.player_index != transcript.player.len()
        || playback.region_index != transcript.region.len()
    {
        return Err(CanonicalPlaceResourcesError::AllocationTranscriptMismatch);
    }
    *state = staged_state;
    *carried = staged_carried;
    Ok(())
}

fn run_category_rows<H: CanonicalResourceAllocationHost>(
    state: &mut CanonicalPlaceResourcesState,
    entry: &PlaceResourcesBonusRowsHandoff,
    category: ResourceCategory,
    facts: &[CanonicalRowFacts],
    carried: &mut RemainingBonusRowsState,
    host: &mut H,
) -> Result<Vec<CanonicalRowReceipt>, CanonicalPlaceResourcesError> {
    if facts.len() != entry.rows.len() {
        return Err(CanonicalPlaceResourcesError::WrongCategoryPlan { category });
    }
    let mut receipts = Vec::with_capacity(facts.len());
    for (row_index, row_facts) in facts.iter().enumerate() {
        receipts.push(run_continued_row(
            state, entry, category, row_index, carried, row_facts, host,
        )?);
    }
    Ok(receipts)
}

fn apply_category_tail(
    loop_state: &mut CategoryLoopState,
    category: ResourceCategory,
    carried: &RemainingBonusRowsState,
    facts: &CategoryAdvanceFacts,
) -> Result<CategoryAdvanceReceipt, CanonicalPlaceResourcesError> {
    loop_state.category = category;
    loop_state.rows_remaining = 0;
    loop_state.row_handles = carried
        .rows
        .last()
        .map(|row| category_handles(row.handles))
        .unwrap_or_default();
    loop_state.deterministic = deterministic(
        &carried.mutation,
        carried.last_chance_group,
        carried.signed_chance_budget,
        carried.winner_seen,
    );
    advance_completed_category(loop_state, facts).map_err(CanonicalPlaceResourcesError::Category)
}

/// Execute BONUSES → FISH → GOODIES as one local transaction. External allocation
/// hosts are observational/two-phase, so a caller may commit their proposals only
/// after this complete receipt is accepted.
pub fn execute_canonical_place_resources<H: CanonicalResourceAllocationHost>(
    state: &mut CanonicalPlaceResourcesState,
    entry: &PlaceResourcesBonusRowsHandoff,
    facts: &CanonicalPlaceResourcesFacts,
    host: &mut H,
) -> Result<CanonicalPlaceResourcesReceipt, CanonicalPlaceResourcesError> {
    let Some(pool_before) = state.mutation.resource_pool.clone() else {
        return Err(CanonicalPlaceResourcesError::WrongHandoff);
    };
    if entry.resume_va != PLACE_RESOURCES_XML_RESIDUAL_VA
        || entry.first_row_body_va != FIRST_BONUS_ROW_BODY_VA
        || !entry.selected_document_live
        || !entry.default_document_live
        || entry.random_state != state.mutation.random_state
        || entry.world_checksum != state.mutation.world_checksum
        || entry.sourced_walked_bytes != state.mutation.sourced_walked_bytes
        || entry.resource_pool_digest != state.mutation.resource_pool_digest
        || resource_divvy_pool_digest(&pool_before) != state.mutation.resource_pool_digest
        || state.mutation.allocated_resources != 0
        || state.mutation.requested_resources != 0
        || entry.selected_document_live != state.selected_document_handles.valid()
        || entry.default_document_live != state.default_document_handles.valid()
        || facts.bonuses.len() != entry.rows.len()
    {
        return Err(CanonicalPlaceResourcesError::WrongHandoff);
    }

    let deterministic_before = deterministic(&state.mutation, -1, 0, false);
    let good_state_digest_before = state.good_state_digest;
    let item_state_digest_before = state.item_state_digest;
    let mut staged = state.clone();
    let mut categories = Vec::with_capacity(3);

    let (bonus_rows, mut carried) = if entry.rows.is_empty() {
        (
            Vec::new(),
            RemainingBonusRowsState {
                next_row_index: 0,
                section_source: entry.section_source,
                rows: Vec::new(),
                last_chance_group: -1,
                signed_chance_budget: 0,
                winner_seen: false,
                mutation: staged.mutation.clone(),
            },
        )
    } else {
        let (first, mut carried) = run_initial_row(&mut staged, entry, &facts.bonuses[0], host)?;
        let mut rows = vec![first];
        for (index, row_facts) in facts.bonuses.iter().enumerate().skip(1) {
            rows.push(run_continued_row(
                &mut staged,
                entry,
                ResourceCategory::Bonuses,
                index,
                &mut carried,
                row_facts,
                host,
            )?);
        }
        (rows, carried)
    };

    let mut loop_state = CategoryLoopState {
        category: ResourceCategory::Bonuses,
        rows_remaining: 0,
        category_handles: category_handles(entry.current_category_handles),
        row_handles: HostHandles::default(),
        selected_document_handles: staged.selected_document_handles,
        default_document_handles: staged.default_document_handles,
        deterministic: deterministic(
            &carried.mutation,
            carried.last_chance_group,
            carried.signed_chance_budget,
            carried.winner_seen,
        ),
    };
    let bonus_tail = apply_category_tail(
        &mut loop_state,
        ResourceCategory::Bonuses,
        &carried,
        &facts.bonuses_tail,
    )?;
    categories.push(CanonicalCategoryReceipt {
        category: ResourceCategory::Bonuses,
        rows: bonus_rows,
        tail: bonus_tail.clone(),
    });

    let (fish_source, fish_rows) = match &bonus_tail.disposition {
        CategoryAdvanceDisposition::NextCategory {
            category: ResourceCategory::Fish,
            source,
            rows,
            ..
        } => (*source, rows.clone()),
        _ => {
            return Err(CanonicalPlaceResourcesError::WrongCategoryPlan {
                category: ResourceCategory::Fish,
            });
        }
    };
    let fish_entry = category_handoff(entry, &loop_state, fish_source, &fish_rows);
    let fish_pool = staged
        .mutation
        .resource_pool
        .clone()
        .ok_or(CanonicalPlaceResourcesError::WrongHandoff)?;
    carried = continued_state(&fish_entry, &loop_state.deterministic, &fish_pool);
    let fish_receipts = run_category_rows(
        &mut staged,
        &fish_entry,
        ResourceCategory::Fish,
        &facts.fish,
        &mut carried,
        host,
    )?;
    let fish_tail = apply_category_tail(
        &mut loop_state,
        ResourceCategory::Fish,
        &carried,
        &facts.fish_tail,
    )?;
    categories.push(CanonicalCategoryReceipt {
        category: ResourceCategory::Fish,
        rows: fish_receipts,
        tail: fish_tail.clone(),
    });

    let (goodies_source, goodies_rows) = match &fish_tail.disposition {
        CategoryAdvanceDisposition::NextCategory {
            category: ResourceCategory::Goodies,
            source,
            rows,
            ..
        } => (*source, rows.clone()),
        _ => {
            return Err(CanonicalPlaceResourcesError::WrongCategoryPlan {
                category: ResourceCategory::Goodies,
            });
        }
    };
    let goodies_entry = category_handoff(&fish_entry, &loop_state, goodies_source, &goodies_rows);
    let goodies_pool = staged
        .mutation
        .resource_pool
        .clone()
        .ok_or(CanonicalPlaceResourcesError::WrongHandoff)?;
    carried = continued_state(&goodies_entry, &loop_state.deterministic, &goodies_pool);
    let goodies_receipts = run_category_rows(
        &mut staged,
        &goodies_entry,
        ResourceCategory::Goodies,
        &facts.goodies,
        &mut carried,
        host,
    )?;
    let goodies_tail = apply_category_tail(
        &mut loop_state,
        ResourceCategory::Goodies,
        &carried,
        &facts.goodies_tail,
    )?;
    let return_count = match goodies_tail.disposition {
        CategoryAdvanceDisposition::Returned { return_count, .. } => return_count,
        _ => {
            return Err(CanonicalPlaceResourcesError::WrongCategoryPlan {
                category: ResourceCategory::Goodies,
            });
        }
    };
    categories.push(CanonicalCategoryReceipt {
        category: ResourceCategory::Goodies,
        rows: goodies_receipts,
        tail: goodies_tail,
    });

    staged.mutation = carried.mutation.clone();
    staged.selected_document_handles = loop_state.selected_document_handles;
    staged.default_document_handles = loop_state.default_document_handles;
    if !mutation_matches_deterministic(&staged.mutation, &loop_state.deterministic) {
        return Err(CanonicalPlaceResourcesError::ExactPlacementStateMismatch);
    }
    let pool_after = staged
        .mutation
        .resource_pool
        .clone()
        .ok_or(CanonicalPlaceResourcesError::WrongHandoff)?;
    let receipt = CanonicalPlaceResourcesReceipt {
        categories,
        deterministic_before,
        deterministic_after: loop_state.deterministic,
        resource_pool_before: pool_before,
        resource_pool_after: pool_after,
        good_state_digest_before,
        good_state_digest_after: staged.good_state_digest,
        item_state_digest_before,
        item_state_digest_after: staged.item_state_digest,
        return_count,
    };
    *state = staged;
    Ok(receipt)
}
