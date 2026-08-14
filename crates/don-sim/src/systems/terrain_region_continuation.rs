//! Exact post-`drop_tile` continuation of `TerrainGroup::place_region_group`.
//!
//! PDB/disassembly provenance: `place_region_group` `0x006a2f60`,
//! `randomize_orthogs` `0x006a2320`, `clear_group` `0x006a28e0`, and
//! `place_oil_deposits` `0x006a2d90`. The owned audited entry executes the
//! exact Mountains and oil/Good owners locally; recorded resolutions and the
//! remaining Cliff request stay explicit host-evidence boundaries.

use super::ammo::vector_dist;
use super::map_terrain::{
    tflag, wflag, WCoord, World, WorldChecksum, WorldSection, NEIGHBOUR_DX, NEIGHBOUR_DY,
};
use super::mountain_add_runtime::{
    AddMountainCall, MountainAddReceipt, MountainAddRuntime, MountainAddRuntimeError,
    MountainWorld, MountainWorldCell,
};
use super::regions::Regions;
use super::terrain_drop_tile::{
    has_mountain_tcoords, DropTileError, DropTileExternalRequest, DropTileExternalResolution,
    DropTileReceipt,
};
use super::terrain_groups::TerrainGroup;
use super::terrain_region_placement::{
    next_region_cursor, reject_candidate, validate_inputs, PlaceRegionGroupCall,
    PlaceRegionGroupPrefixError, PlaceRegionGroupPrefixOutcome, PlaceRegionGroupPrefixReceipt,
    RegionCandidateAttempt, RegionDropTileInvocation, RegionHelpingState,
};
use super::world_oil_goods::{
    apply_world_set_oil_at, OilGoodMutation, OilGoodMutationError, OilGoodMutationReceipt,
    OilGoodRuntime,
};
use crate::rng::Random;

const CARDINAL: [(i32, i32); 4] = [(-1, 0), (0, 1), (1, 0), (0, -1)];
const TYPE_FOUR_DX: [i32; 9] = [0, -1, 0, 1, 1, 1, 0, -1, -1];
const TYPE_FOUR_DY: [i32; 9] = [0, -1, -1, -1, 0, 1, 1, 1, 0];
const OIL_GOOD_TYPE: i32 = 5;

impl MountainWorld for World {
    fn world_xs(&self) -> i32 {
        self.xs
    }

    fn world_ys(&self) -> i32 {
        self.ys
    }

    fn tile_xs(&self) -> i32 {
        self.tile_xs
    }

    fn tile_ys(&self) -> i32 {
        self.tile_ys
    }

    fn world_cell(&self, wx: i32, wy: i32) -> MountainWorldCell {
        let cell = self.wdata(wx, wy);
        MountainWorldCell {
            flags: cell.flags,
            land: cell.land,
        }
    }

    fn write_world_flags(&mut self, wx: i32, wy: i32, flags: u16) {
        self.wdata_mut(wx, wy).flags = flags;
    }

    fn tile_mask(&self, tx: i32, ty: i32) -> u16 {
        self.tmask(tx, ty)
    }

    fn set_mountain_tile(&mut self, tx: i32, ty: i32) {
        self.set_mountain_at(tx, ty, true);
    }

    fn set_behind_b(&mut self, tx: i32, ty: i32) {
        self.set_behind(tx, ty, true, true);
    }

    fn start_x_count(&self) -> usize {
        self.start_x.items.len()
    }

    fn start_y_count(&self) -> usize {
        self.start_y.items.len()
    }

    fn start_at(&self, index: usize) -> (i32, i32) {
        (self.start_x.items[index], self.start_y.items[index])
    }

    fn start_city_reserved(&self, wx: i32, wy: i32) -> bool {
        self.start_city_wcoord(WCoord(wx), WCoord(wy))
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct RandomizeOrthogsReceipt {
    pub cardinal_rotation: usize,
    /// The diagonal cursor is not read by this caller, but retail still draws
    /// and advances it.
    pub diagonal_rotation: usize,
    pub rng_state_after: i32,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RegionGrowthRejection {
    AlreadyInGroup,
    OutOfBounds,
    FixedOccupiedProbe,
    PlayerStartMinimum { player: usize, distance: i32 },
    PlayerStartMaximum { player: usize, distance: i32 },
    GroupMinimum { distance: i32 },
    GroupMaximum { distance: i32 },
    TypeFourImpassableNeighbor { offset_index: usize },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegionGrowthAttempt {
    pub base_index: usize,
    pub direction_index: usize,
    pub world_x: i32,
    pub world_y: i32,
    pub rejection: Option<RegionGrowthRejection>,
    pub drop: Option<DropTileReceipt>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegionGrowthPassReceipt {
    pub selected_index: usize,
    /// `0x006a3cee`: one draw when the tile list held more than one base,
    /// including when the first visited base succeeds and `base_order` is a
    /// strict prefix of that list.
    pub base_index_draws: u32,
    pub base_order: Vec<usize>,
    pub orthogs: Vec<RandomizeOrthogsReceipt>,
    pub attempts: Vec<RegionGrowthAttempt>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClearRegionGroupReceipt {
    pub cleared_tiles: Vec<(i32, i32)>,
    pub external_requests: Vec<DropTileExternalRequest>,
    pub tdata_unblock_calls: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PlaceOilDepositsReceipt {
    pub requested: i32,
    pub capped_requested: i32,
    pub placed: i32,
    pub external_requests: Vec<DropTileExternalRequest>,
    pub completed: bool,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PlaceRegionGroupOutcome {
    Returned(i32),
    ExternalResolutionRequired { request: DropTileExternalRequest },
}

/// One exact byte mutation in the stream walked by
/// `World::walk_data(CheckSum *, -1)` (`0x006b5cf0`).
///
/// `offset` is in channel-12 walk order, not a Rust-struct byte offset.  That
/// distinction matters because the walk omits WData padding and interleaves
/// container metadata with elements.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct RegionWorldByteMutation {
    pub offset: usize,
    pub before: u8,
    pub after: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaceRegionGroupWorldReceipt {
    /// Channel 12 before and after this preview transaction.
    pub checksum_before: WorldChecksum,
    pub checksum_after: WorldChecksum,
    /// Isolated retail walk sections whose digest or byte count changed.
    pub changed_sections: Vec<WorldSection>,
    /// Every changed channel-12 byte in exact walk order.
    pub byte_mutations: Vec<RegionWorldByteMutation>,
    /// Physical plane indices changed by the whole transaction.  These are a
    /// useful bridge back from walk offsets to map coordinates.
    pub changed_wdata_indices: Vec<usize>,
    pub changed_tdata_indices: Vec<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaceRegionGroupReceipt {
    pub prefix: PlaceRegionGroupPrefixReceipt,
    pub drops: Vec<DropTileReceipt>,
    pub helping_after: Option<RegionHelpingState>,
    pub growth_passes: Vec<RegionGrowthPassReceipt>,
    pub clear_passes: Vec<ClearRegionGroupReceipt>,
    pub oil_deposits: Option<PlaceOilDepositsReceipt>,
    pub external_resolutions_consumed: usize,
    pub outcome: PlaceRegionGroupOutcome,
    /// Main-stream state at function entry, before the optional region-cursor
    /// draw at `0x006a2fe1`.
    pub rng_state_before: i32,
    /// Exact number of main-stream words consumed by the prefix, growth loop,
    /// orthogonal randomization, and resolved cliff calls.
    pub rng_draws: u32,
    pub rng_state_after: i32,
    /// Present only for the explicit audited entry point. Ordinary map
    /// generation avoids cloning and walking the whole World per clump.
    pub world: Option<PlaceRegionGroupWorldReceipt>,
}

/// External simulation owners that can execute typed `drop_tile` requests
/// rather than accepting asserted result rows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PlaceRegionGroupOwners {
    /// `None` keeps the displacement-template producer as an explicit red
    /// boundary. Production must never substitute synthetic geometry.
    pub mountains: Option<MountainAddRuntime>,
    /// Base-Good object pool plus `ObjectsData::good_mark`.
    pub oil_goods: Option<OilGoodRuntime>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MountainOwnerExecutionReceipt {
    pub execution: MountainAddReceipt,
    /// Exact channel-12 delta of this leaf alone.
    pub world: PlaceRegionGroupWorldReceipt,
    /// Adler over `Mountains::walk_data`'s retained-array byte stream. This is
    /// a walk receipt, not a claim that Mountains is a separate check_all
    /// channel.
    pub mountain_walk_adler_before: u32,
    pub mountain_walk_adler_after: u32,
    pub mountain_walk_bytes_before: usize,
    pub mountain_walk_bytes_after: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlaceRegionGroupOwnerReceipt {
    Mountain(MountainOwnerExecutionReceipt),
    OilGood(OilGoodMutationReceipt),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaceRegionGroupOwnedReceipt {
    pub placement: PlaceRegionGroupReceipt,
    pub owners: Vec<PlaceRegionGroupOwnerReceipt>,
    /// False means the receipt describes a staged preview stopped at the next
    /// typed owner. Group, World, RNG, and owner state were all rolled back.
    pub committed: bool,
}

impl PlaceRegionGroupReceipt {
    /// Recompute the final main-stream word from the receipt's entry word and
    /// draw count.  This is an invariant check, not a second source of RNG.
    pub fn rng_receipt_is_coherent(&self) -> bool {
        let mut random = Random::new(self.rng_state_before);
        for _ in 0..self.rng_draws {
            random.advance();
        }
        random.state() == self.rng_state_after
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlaceRegionGroupError {
    InvalidPrefix(PlaceRegionGroupPrefixError),
    InvalidDropTile(DropTileError),
    InvalidGrowthFixedProbe {
        region_id: usize,
        initial_x: i32,
    },
    InvalidCursorCycle {
        region_id: usize,
    },
    ExternalResolutionMismatch {
        expected: DropTileExternalRequest,
        actual: DropTileExternalRequest,
    },
    ExternalResolutionKindMismatch {
        request: DropTileExternalRequest,
    },
    UnsupportedClearTile {
        world_x: i32,
        world_y: i32,
    },
    MissingMountainRuntime {
        request: DropTileExternalRequest,
    },
    InvalidMountainRuntime(MountainAddRuntimeError),
    MissingOilGoodRuntime {
        request: DropTileExternalRequest,
    },
    InvalidOilGoodRuntime(OilGoodMutationError),
}

impl TerrainGroup {
    /// Execute the complete deterministic continuation on caller-owned preview
    /// state. Missing object-system effects are returned as ordered boundaries;
    /// callers resume transactionally by replaying on clones with the emitted
    /// resolutions appended.
    pub fn apply_place_region_group(
        &mut self,
        world: &mut World,
        regions: &Regions,
        random: &mut Random,
        call: PlaceRegionGroupCall,
        helping: Option<RegionHelpingState>,
        externals: &[DropTileExternalResolution],
    ) -> Result<PlaceRegionGroupReceipt, PlaceRegionGroupError> {
        self.apply_place_region_group_recorded(
            world, regions, random, call, helping, externals, false,
        )
    }

    /// The same retail transaction with an exact channel-12 before/after walk
    /// and byte delta. Replay localization should use this entry point; ordinary
    /// map generation should use [`Self::apply_place_region_group`] so it does
    /// not clone and hash the whole World for every clump.
    pub fn apply_place_region_group_audited(
        &mut self,
        world: &mut World,
        regions: &Regions,
        random: &mut Random,
        call: PlaceRegionGroupCall,
        helping: Option<RegionHelpingState>,
        externals: &[DropTileExternalResolution],
    ) -> Result<PlaceRegionGroupReceipt, PlaceRegionGroupError> {
        self.apply_place_region_group_recorded(
            world, regions, random, call, helping, externals, true,
        )
    }

    /// Execute the region transaction against exact local subsystem owners.
    ///
    /// The whole call is staged. A native return (including `Liberr == 0` from
    /// an exhausted placement) commits Group, World, RNG, and owner state
    /// together. A typed external boundary or any owner error commits none of
    /// them. The returned placement and each executed owner carry exact World
    /// walk receipts.
    pub fn apply_place_region_group_owned_audited(
        &mut self,
        world: &mut World,
        regions: &Regions,
        random: &mut Random,
        call: PlaceRegionGroupCall,
        helping: Option<RegionHelpingState>,
        owners: &mut PlaceRegionGroupOwners,
    ) -> Result<PlaceRegionGroupOwnedReceipt, PlaceRegionGroupError> {
        let mut staged_group = self.clone();
        let mut staged_world = world.clone();
        let mut staged_random = *random;
        let mut staged_owners = owners.clone();
        let mut resolver = OwnedRegionExternalResolver {
            owners: &mut staged_owners,
            receipts: Vec::new(),
        };
        let mut placement = staged_group.apply_place_region_group_with_resolver(
            &mut staged_world,
            regions,
            &mut staged_random,
            call,
            helping,
            &mut resolver,
            true,
        )?;
        placement.external_resolutions_consumed = resolver.receipts.len();
        let owner_receipts = std::mem::take(&mut resolver.receipts);
        drop(resolver);
        let committed = matches!(placement.outcome, PlaceRegionGroupOutcome::Returned(_));
        if committed {
            *self = staged_group;
            *world = staged_world;
            *random = staged_random;
            *owners = staged_owners;
        }
        Ok(PlaceRegionGroupOwnedReceipt {
            placement,
            owners: owner_receipts,
            committed,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn apply_place_region_group_recorded(
        &mut self,
        world: &mut World,
        regions: &Regions,
        random: &mut Random,
        call: PlaceRegionGroupCall,
        helping: Option<RegionHelpingState>,
        externals: &[DropTileExternalResolution],
        audit_world: bool,
    ) -> Result<PlaceRegionGroupReceipt, PlaceRegionGroupError> {
        let mut resolver = RecordedRegionExternalResolver {
            externals,
            consumed: 0,
        };
        let mut receipt = self.apply_place_region_group_with_resolver(
            world,
            regions,
            random,
            call,
            helping,
            &mut resolver,
            audit_world,
        )?;
        receipt.external_resolutions_consumed = resolver.consumed;
        Ok(receipt)
    }

    fn apply_place_region_group_with_resolver(
        &mut self,
        world: &mut World,
        regions: &Regions,
        random: &mut Random,
        call: PlaceRegionGroupCall,
        helping: Option<RegionHelpingState>,
        resolver: &mut dyn RegionExternalResolver,
        audit_world: bool,
    ) -> Result<PlaceRegionGroupReceipt, PlaceRegionGroupError> {
        validate_inputs(self, world, regions, call, helping)
            .map_err(PlaceRegionGroupError::InvalidPrefix)?;
        if matches!(self.group_type, 4 | 6) && call.target_tiles > 1 {
            validate_growth_probes(world, regions, call)?;
        }

        let world_before = audit_world.then(|| world.clone());
        let rng_state_before = random.state();
        let mut prefix = self
            .plan_place_region_group_prefix(world, regions, random, call, helping)
            .map_err(PlaceRegionGroupError::InvalidPrefix)?;
        // Native clears length before its region draw. The planner records the
        // old length; moving this state-only write here preserves the same final
        // state while retaining fail-before-draw validation.
        self.tiles.items.clear();
        let mut receipt = PlaceRegionGroupReceipt {
            prefix: prefix.clone(),
            drops: Vec::new(),
            helping_after: helping,
            growth_passes: Vec::new(),
            clear_passes: Vec::new(),
            oil_deposits: None,
            external_resolutions_consumed: 0,
            outcome: PlaceRegionGroupOutcome::Returned(0),
            rng_state_before,
            rng_draws: 0,
            rng_state_after: random.state(),
            world: None,
        };

        let coords = &regions.list[call.region_id].coords.items;
        let initial_cursor = prefix.initial_cursor;
        let mut cursor = match prefix.outcome {
            PlaceRegionGroupPrefixOutcome::Exhausted => {
                return Ok(finalize_place_region_group_receipt(
                    receipt,
                    world_before.as_ref(),
                    world,
                    random,
                ));
            }
            PlaceRegionGroupPrefixOutcome::DropTile(_) => prefix.attempts.last().unwrap().cursor,
        };
        let mut pending = match prefix.outcome {
            PlaceRegionGroupPrefixOutcome::DropTile(invocation) => Some(invocation),
            PlaceRegionGroupPrefixOutcome::Exhausted => None,
        };
        // Native `local_30` holds the most recent entry-search drop result. A
        // stalled growth cleanup does not reset it before resuming the LFSR.
        let mut last_drop_result = 0;

        loop {
            let invocation = if let Some(invocation) = pending.take() {
                invocation
            } else {
                match next_accepted_candidate(
                    self,
                    world,
                    coords,
                    &mut cursor,
                    initial_cursor,
                    call,
                    receipt.helping_after,
                    &mut prefix.attempts,
                )? {
                    Some(invocation) => invocation,
                    None => {
                        receipt.outcome = PlaceRegionGroupOutcome::Returned(last_drop_result);
                        receipt.prefix = prefix;
                        return Ok(finalize_place_region_group_receipt(
                            receipt,
                            world_before.as_ref(),
                            world,
                            random,
                        ));
                    }
                }
            };

            let Some(drop) = apply_drop_with_ordered_external(
                self,
                world,
                random,
                invocation,
                resolver,
                &mut receipt.outcome,
            )?
            else {
                receipt.prefix = prefix;
                return Ok(finalize_place_region_group_receipt(
                    receipt,
                    world_before.as_ref(),
                    world,
                    random,
                ));
            };
            let placed = drop.placed;
            last_drop_result = i32::from(placed);
            receipt.drops.push(drop);
            if !placed {
                continue;
            }

            let initial_x = invocation.world_x;
            let initial_y = invocation.world_y;
            update_helping_scores(
                world,
                self.group_type,
                initial_x,
                initial_y,
                &mut receipt.helping_after,
            );

            if matches!(self.group_type, 5 | 7 | 8) {
                receipt.outcome = PlaceRegionGroupOutcome::Returned(1);
                receipt.prefix = prefix;
                return Ok(finalize_place_region_group_receipt(
                    receipt,
                    world_before.as_ref(),
                    world,
                    random,
                ));
            }

            loop {
                if call.target_tiles <= self.tiles.items.len() as i32 {
                    if self.group_type == 6 {
                        let Some(oil) = place_oil_deposits(
                            self,
                            world,
                            call.oil_deposits,
                            resolver,
                            &mut receipt.outcome,
                        )?
                        else {
                            receipt.prefix = prefix;
                            return Ok(finalize_place_region_group_receipt(
                                receipt,
                                world_before.as_ref(),
                                world,
                                random,
                            ));
                        };
                        receipt.oil_deposits = Some(oil);
                    }
                    receipt.outcome = PlaceRegionGroupOutcome::Returned(1);
                    receipt.prefix = prefix;
                    return Ok(finalize_place_region_group_receipt(
                        receipt,
                        world_before.as_ref(),
                        world,
                        random,
                    ));
                }

                let pass = grow_once(
                    self,
                    world,
                    random,
                    call,
                    initial_x,
                    initial_y,
                    resolver,
                    &mut receipt.outcome,
                )?;
                let placed = pass
                    .attempts
                    .iter()
                    .any(|attempt| attempt.drop.as_ref().is_some_and(|drop| drop.placed));
                receipt.growth_passes.push(pass);
                if matches!(
                    receipt.outcome,
                    PlaceRegionGroupOutcome::ExternalResolutionRequired { .. }
                ) {
                    receipt.prefix = prefix;
                    return Ok(finalize_place_region_group_receipt(
                        receipt,
                        world_before.as_ref(),
                        world,
                        random,
                    ));
                }
                if placed {
                    continue;
                }

                let Some(clear) = clear_group(self, world, resolver, &mut receipt.outcome)? else {
                    receipt.prefix = prefix;
                    return Ok(finalize_place_region_group_receipt(
                        receipt,
                        world_before.as_ref(),
                        world,
                        random,
                    ));
                };
                receipt.clear_passes.push(clear);
                break;
            }
        }
    }
}

fn finalize_place_region_group_receipt(
    mut receipt: PlaceRegionGroupReceipt,
    world_before: Option<&World>,
    world_after: &World,
    random: &Random,
) -> PlaceRegionGroupReceipt {
    receipt.rng_draws = place_region_group_rng_draws(&receipt);
    receipt.rng_state_after = random.state();
    let Some(world_before) = world_before else {
        return receipt;
    };
    receipt.world = Some(build_world_receipt(world_before, world_after));
    receipt
}

pub(crate) fn build_world_receipt(
    world_before: &World,
    world_after: &World,
) -> PlaceRegionGroupWorldReceipt {
    let checksum_before = world_before.checksum_sections();
    let checksum_after = world_after.checksum_sections();
    let changed_sections = checksum_before.differing_sections(&checksum_after);
    let before_image = world_before.checksum_image();
    let after_image = world_after.checksum_image();
    let byte_mutations = before_image
        .0
        .iter()
        .zip(&after_image.0)
        .enumerate()
        .filter_map(|(offset, (&before, &after))| {
            (before != after).then_some(RegionWorldByteMutation {
                offset,
                before,
                after,
            })
        })
        .collect();
    let changed_wdata_indices = world_before
        .wdata
        .iter()
        .zip(&world_after.wdata)
        .enumerate()
        .filter_map(|(index, (before, after))| (before != after).then_some(index))
        .collect();
    let changed_tdata_indices = world_before
        .tdata
        .iter()
        .zip(&world_after.tdata)
        .enumerate()
        .filter_map(|(index, (before, after))| (before != after).then_some(index))
        .collect();
    PlaceRegionGroupWorldReceipt {
        checksum_before,
        checksum_after,
        changed_sections,
        byte_mutations,
        changed_wdata_indices,
        changed_tdata_indices,
    }
}

fn place_region_group_rng_draws(receipt: &PlaceRegionGroupReceipt) -> u32 {
    let entry_drops = receipt.drops.iter().map(|drop| drop.rng_draws).sum::<u32>();
    let growth_draws = receipt
        .growth_passes
        .iter()
        .map(|pass| {
            pass.base_index_draws
                + pass.orthogs.len() as u32 * 2
                + pass
                    .attempts
                    .iter()
                    .filter_map(|attempt| attempt.drop.as_ref())
                    .map(|drop| drop.rng_draws)
                    .sum::<u32>()
        })
        .sum::<u32>();
    receipt
        .prefix
        .region_cursor_draws
        .wrapping_add(entry_drops)
        .wrapping_add(growth_draws)
}

trait RegionExternalResolver {
    fn resolve(
        &mut self,
        request: DropTileExternalRequest,
        world: &mut World,
    ) -> Result<Option<DropTileExternalResolution>, PlaceRegionGroupError>;
}

struct RecordedRegionExternalResolver<'a> {
    externals: &'a [DropTileExternalResolution],
    consumed: usize,
}

impl RegionExternalResolver for RecordedRegionExternalResolver<'_> {
    fn resolve(
        &mut self,
        request: DropTileExternalRequest,
        _world: &mut World,
    ) -> Result<Option<DropTileExternalResolution>, PlaceRegionGroupError> {
        let Some(actual) = self.externals.get(self.consumed).copied() else {
            return Ok(None);
        };
        ensure_external_request(request, actual)?;
        self.consumed += 1;
        Ok(Some(actual))
    }
}

struct OwnedRegionExternalResolver<'a> {
    owners: &'a mut PlaceRegionGroupOwners,
    receipts: Vec<PlaceRegionGroupOwnerReceipt>,
}

impl RegionExternalResolver for OwnedRegionExternalResolver<'_> {
    fn resolve(
        &mut self,
        request: DropTileExternalRequest,
        world: &mut World,
    ) -> Result<Option<DropTileExternalResolution>, PlaceRegionGroupError> {
        match request {
            DropTileExternalRequest::MountainsAddMountain {
                template,
                world_x,
                world_y,
                pattern,
                mountain_space,
                forest_space,
                rock_space,
                coast_space,
                start_min,
            } => {
                let runtime = self
                    .owners
                    .mountains
                    .as_mut()
                    .ok_or(PlaceRegionGroupError::MissingMountainRuntime { request })?;
                let world_before = world.clone();
                let walked_before = runtime.walked_bytes();
                let execution = runtime
                    .apply_add_mountain(
                        world,
                        AddMountainCall {
                            template,
                            world_x,
                            world_y,
                            verification_mode: pattern,
                            mountain_space,
                            forest_space,
                            rock_space,
                            coast_space,
                            start_min,
                        },
                    )
                    .map_err(PlaceRegionGroupError::InvalidMountainRuntime)?;
                let walked_after = runtime.walked_bytes();
                let liberr = execution.liberr;
                self.receipts.push(PlaceRegionGroupOwnerReceipt::Mountain(
                    MountainOwnerExecutionReceipt {
                        execution,
                        world: build_world_receipt(&world_before, world),
                        mountain_walk_adler_before: crate::checksum::adler32(1, &walked_before),
                        mountain_walk_adler_after: crate::checksum::adler32(1, &walked_after),
                        mountain_walk_bytes_before: walked_before.len(),
                        mountain_walk_bytes_after: walked_after.len(),
                    },
                ));
                Ok(Some(DropTileExternalResolution::Mountains {
                    request,
                    liberr,
                }))
            }
            DropTileExternalRequest::OilGoodMutation {
                world_x,
                world_y,
                enabled,
                good_type,
                coord_x,
                coord_y,
            } => {
                let goods = self
                    .owners
                    .oil_goods
                    .as_mut()
                    .ok_or(PlaceRegionGroupError::MissingOilGoodRuntime { request })?;
                let execution = apply_world_set_oil_at(
                    world,
                    goods,
                    OilGoodMutation {
                        world_x,
                        world_y,
                        enabled,
                        good_type,
                        coord_x,
                        coord_y,
                    },
                )
                .map_err(PlaceRegionGroupError::InvalidOilGoodRuntime)?;
                self.receipts
                    .push(PlaceRegionGroupOwnerReceipt::OilGood(execution));
                Ok(Some(DropTileExternalResolution::OilGoodsApplied {
                    request,
                }))
            }
            // Cliff eligibility owns an optional main-stream draw and remains
            // an explicit typed boundary.
            DropTileExternalRequest::CliffsPositionCliff { .. } => Ok(None),
        }
    }
}

fn validate_growth_probes(
    world: &World,
    regions: &Regions,
    call: PlaceRegionGroupCall,
) -> Result<(), PlaceRegionGroupError> {
    let region = i32::try_from(call.region_id).ok();
    for &(initial_x, _) in &regions.list[call.region_id].coords.items {
        let stride = if call.place_players == 0 {
            world.tile_xs
        } else {
            world.xs
        };
        let index = region
            .and_then(|region| stride.checked_mul(region))
            .and_then(|base| base.checked_add(initial_x))
            .and_then(|index| usize::try_from(index).ok());
        let valid = if call.place_players == 0 {
            index.is_some_and(|index| index < world.tdata.len())
        } else {
            index.is_some_and(|index| index / 8 < world.start_city_locs.len())
        };
        if !valid {
            return Err(PlaceRegionGroupError::InvalidGrowthFixedProbe {
                region_id: call.region_id,
                initial_x,
            });
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn next_accepted_candidate(
    group: &TerrainGroup,
    world: &World,
    coords: &[(i32, i32)],
    cursor: &mut u32,
    initial_cursor: u32,
    call: PlaceRegionGroupCall,
    helping: Option<RegionHelpingState>,
    attempts: &mut Vec<RegionCandidateAttempt>,
) -> Result<Option<RegionDropTileInvocation>, PlaceRegionGroupError> {
    loop {
        *cursor = next_region_cursor(*cursor, coords.len() as u32, initial_cursor).ok_or(
            PlaceRegionGroupError::InvalidCursorCycle {
                region_id: call.region_id,
            },
        )?;
        if *cursor == initial_cursor {
            return Ok(None);
        }
        let coord_index = *cursor as usize - 1;
        let (world_x, world_y) = coords[coord_index];
        let rejection = reject_candidate(group, world, world_x, world_y, call, helping);
        attempts.push(RegionCandidateAttempt {
            cursor: *cursor,
            coord_index,
            world_x,
            world_y,
            rejection,
        });
        if rejection.is_none() {
            return Ok(Some(RegionDropTileInvocation {
                world_x,
                world_y,
                group_type: group.group_type,
                group_radius: call.target_tiles / 4 + 1,
                land_subtype: call.land_subtype,
                target_tiles: call.target_tiles,
                oil_deposits: call.oil_deposits,
                group_index: call.group_index,
            }));
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_drop_with_ordered_external(
    group: &mut TerrainGroup,
    world: &mut World,
    random: &mut Random,
    invocation: RegionDropTileInvocation,
    resolver: &mut dyn RegionExternalResolver,
    outcome: &mut PlaceRegionGroupOutcome,
) -> Result<Option<DropTileReceipt>, PlaceRegionGroupError> {
    let external = if let Some(request) = group.drop_tile_external_request(invocation) {
        let Some(actual) = resolver.resolve(request, world)? else {
            *outcome = PlaceRegionGroupOutcome::ExternalResolutionRequired { request };
            return Ok(None);
        };
        Some(actual)
    } else {
        None
    };
    group
        .apply_drop_tile(world, random, invocation, external)
        .map(Some)
        .map_err(PlaceRegionGroupError::InvalidDropTile)
}

fn update_helping_scores(
    world: &World,
    group_type: i32,
    x: i32,
    y: i32,
    helping: &mut Option<RegionHelpingState>,
) {
    let Some(state) = helping else { return };
    let slot = (group_type - 4) as usize;
    for player in 0..state.num_players {
        let distance = vector_dist(
            world.start_x.items[player].wrapping_sub(x),
            world.start_y.items[player].wrapping_sub(y),
        );
        state.scores[player][slot] = state.scores[player][slot].wrapping_add(distance);
    }
    let mut best_player = 0;
    let mut best_score = 0;
    for player in 0..state.num_players {
        if best_score < state.scores[player][slot] {
            best_player = player as i32;
            best_score = state.scores[player][slot];
        }
    }
    state.lowest_player[slot] = best_player;
}

#[allow(clippy::too_many_arguments)]
fn grow_once(
    group: &mut TerrainGroup,
    world: &mut World,
    random: &mut Random,
    call: PlaceRegionGroupCall,
    initial_x: i32,
    initial_y: i32,
    resolver: &mut dyn RegionExternalResolver,
    outcome: &mut PlaceRegionGroupOutcome,
) -> Result<RegionGrowthPassReceipt, PlaceRegionGroupError> {
    let len = group.tiles.items.len();
    let selected_index = if len > 1 {
        (random.get(0, 0xffff) % len as i32) as usize
    } else {
        0
    };
    let base_index_draws = u32::from(len > 1);
    let mut base_order = Vec::with_capacity(len);
    let mut orthogs = Vec::with_capacity(len);
    let mut attempts = Vec::new();
    for offset in 1..=len {
        let base_index = (selected_index + offset) % len;
        base_order.push(base_index);
        let (base_x, base_y) = group.tiles.items[base_index];
        let orthog = randomize_orthogs(random);
        orthogs.push(orthog);
        for step in 0..4 {
            let direction_index = (orthog.cardinal_rotation + step) % 4;
            let (dx, dy) = CARDINAL[direction_index];
            let world_x = base_x.wrapping_add(dx);
            let world_y = base_y.wrapping_add(dy);
            let rejection =
                reject_growth_candidate(group, world, world_x, world_y, call, initial_x, initial_y);
            let mut attempt = RegionGrowthAttempt {
                base_index,
                direction_index,
                world_x,
                world_y,
                rejection,
                drop: None,
            };
            if rejection.is_none() {
                let invocation = RegionDropTileInvocation {
                    world_x,
                    world_y,
                    group_type: group.group_type,
                    group_radius: call.target_tiles / 4 + 1,
                    land_subtype: -1,
                    target_tiles: call.target_tiles,
                    oil_deposits: call.oil_deposits,
                    group_index: call.group_index,
                };
                let Some(drop) = apply_drop_with_ordered_external(
                    group, world, random, invocation, resolver, outcome,
                )?
                else {
                    attempts.push(attempt);
                    return Ok(RegionGrowthPassReceipt {
                        selected_index,
                        base_index_draws,
                        base_order,
                        orthogs,
                        attempts,
                    });
                };
                let placed = drop.placed;
                attempt.drop = Some(drop);
                attempts.push(attempt);
                if placed {
                    return Ok(RegionGrowthPassReceipt {
                        selected_index,
                        base_index_draws,
                        base_order,
                        orthogs,
                        attempts,
                    });
                }
            } else {
                attempts.push(attempt);
            }
        }
    }
    Ok(RegionGrowthPassReceipt {
        selected_index,
        base_index_draws,
        base_order,
        orthogs,
        attempts,
    })
}

fn randomize_orthogs(random: &mut Random) -> RandomizeOrthogsReceipt {
    let cardinal_rotation = (random.get(0, 0xffff) % 4) as usize;
    let diagonal_rotation = (random.get(0, 0xffff) % 4) as usize;
    RandomizeOrthogsReceipt {
        cardinal_rotation,
        diagonal_rotation,
        rng_state_after: random.state(),
    }
}

fn reject_growth_candidate(
    group: &TerrainGroup,
    world: &World,
    x: i32,
    y: i32,
    call: PlaceRegionGroupCall,
    initial_x: i32,
    initial_y: i32,
) -> Option<RegionGrowthRejection> {
    if group.tiles.items.contains(&(x, y)) {
        return Some(RegionGrowthRejection::AlreadyInGroup);
    }
    if !world.valid_w(x, y) {
        return Some(RegionGrowthRejection::OutOfBounds);
    }
    let fixed_index = if call.place_players == 0 {
        (world.tile_xs * call.region_id as i32 + initial_x) as usize
    } else {
        (world.xs * call.region_id as i32 + initial_x) as usize
    };
    let occupied = if call.place_players == 0 {
        let tile = world.tdata[fixed_index];
        tile & tflag::STARTED != 0 || tile & tflag::BLOCKER_MASK == tflag::BLOCKER_BUILDING
    } else {
        world.start_city_locs[fixed_index >> 3] & (1 << (fixed_index & 7)) != 0
    };
    if occupied {
        return Some(RegionGrowthRejection::FixedOccupiedProbe);
    }
    for (player, (&start_x, &start_y)) in world
        .start_x
        .items
        .iter()
        .zip(&world.start_y.items)
        .enumerate()
    {
        let distance = vector_dist(x.wrapping_sub(start_x), y.wrapping_sub(start_y));
        if group.start_min > 0 && distance < group.start_min {
            return Some(RegionGrowthRejection::PlayerStartMinimum { player, distance });
        }
        if group.start_max > 0 && group.start_max < distance {
            return Some(RegionGrowthRejection::PlayerStartMaximum { player, distance });
        }
    }
    let distance = vector_dist(x.wrapping_sub(initial_x), y.wrapping_sub(initial_y));
    if group.cent_min > 0 && distance < group.cent_min {
        return Some(RegionGrowthRejection::GroupMinimum { distance });
    }
    if group.cent_max > 0 && group.cent_max < distance {
        return Some(RegionGrowthRejection::GroupMaximum { distance });
    }
    if group.group_type == 4 {
        for offset_index in 0..9 {
            let nx = x.wrapping_add(TYPE_FOUR_DX[offset_index]);
            let ny = y.wrapping_add(TYPE_FOUR_DY[offset_index]);
            if world.valid_w(nx, ny) && world.wdata(nx, ny).flags & wflag::IMPASSABLE_X != 0 {
                return Some(RegionGrowthRejection::TypeFourImpassableNeighbor { offset_index });
            }
        }
    }
    None
}

fn clear_group(
    group: &mut TerrainGroup,
    world: &mut World,
    resolver: &mut dyn RegionExternalResolver,
    outcome: &mut PlaceRegionGroupOutcome,
) -> Result<Option<ClearRegionGroupReceipt>, PlaceRegionGroupError> {
    let mut receipt = ClearRegionGroupReceipt::default();
    for &(x, y) in group.tiles.items.clone().iter() {
        let cell = world.wdata(x, y);
        if cell.flags & (wflag::ROCKS | wflag::FOREST) == 0 {
            return Err(PlaceRegionGroupError::UnsupportedClearTile {
                world_x: x,
                world_y: y,
            });
        }
        world.wdata_mut(x, y).flags &= !(wflag::ROCKS | wflag::FOREST);
        let request = oil_request(x, y, false);
        let Some(()) = consume_oil_external(request, world, resolver, outcome)? else {
            return Ok(None);
        };
        receipt.external_requests.push(request);
        world.set_oil_at(x, y, false);
        for index in 0..16 {
            let tx = x * 4 + index % 4;
            let ty = y * 4 + index / 4;
            if world.tmask(tx, ty) & tflag::BLOCKER_MASK != tflag::BLOCKER_MOUNTAIN {
                world.set_blocked_at(tx, ty, false);
                receipt.tdata_unblock_calls += 1;
            }
        }
        receipt.cleared_tiles.push((x, y));
    }
    group.tiles.items.clear();
    Ok(Some(receipt))
}

fn place_oil_deposits(
    group: &TerrainGroup,
    world: &mut World,
    requested: i32,
    resolver: &mut dyn RegionExternalResolver,
    outcome: &mut PlaceRegionGroupOutcome,
) -> Result<Option<PlaceOilDepositsReceipt>, PlaceRegionGroupError> {
    let mut receipt = PlaceOilDepositsReceipt {
        requested,
        capped_requested: requested.min(group.tiles.items.len() as i32),
        ..PlaceOilDepositsReceipt::default()
    };
    if group.group_type != 6 || requested == 0 {
        return Ok(Some(receipt));
    }
    for &(x, y) in &group.tiles.items {
        if world.wdata(x, y).flags & wflag::OIL != 0 {
            continue;
        }
        let mut qualifies = false;
        for index in 0..8 {
            let nx = x + NEIGHBOUR_DX[index];
            let ny = y + NEIGHBOUR_DY[index];
            if world.valid_w(nx, ny)
                && world.wdata(nx, ny).flags & (wflag::ROCKS | wflag::FOREST) == 0
                && !has_mountain_tcoords(world, nx, ny)
            {
                qualifies = true;
                break;
            }
        }
        if qualifies {
            let request = oil_request(x, y, true);
            let Some(()) = consume_oil_external(request, world, resolver, outcome)? else {
                return Ok(None);
            };
            receipt.external_requests.push(request);
            world.set_oil_at(x, y, true);
            receipt.placed += 1;
            if receipt.placed == receipt.capped_requested {
                break;
            }
        }
    }
    receipt.completed = receipt.placed == receipt.capped_requested;
    Ok(Some(receipt))
}

fn oil_request(x: i32, y: i32, enabled: bool) -> DropTileExternalRequest {
    DropTileExternalRequest::OilGoodMutation {
        world_x: x,
        world_y: y,
        enabled,
        good_type: OIL_GOOD_TYPE,
        coord_x: x.wrapping_mul(0x300).wrapping_add(0x180),
        coord_y: y.wrapping_mul(0x300).wrapping_add(0x180),
    }
}

fn consume_oil_external(
    request: DropTileExternalRequest,
    world: &mut World,
    resolver: &mut dyn RegionExternalResolver,
    outcome: &mut PlaceRegionGroupOutcome,
) -> Result<Option<()>, PlaceRegionGroupError> {
    // The resolver has no RNG handle: `World::set_oil_at` consumes no main
    // stream word, and the type boundary makes an accidental draw impossible.
    let Some(actual) = resolver.resolve(request, world)? else {
        *outcome = PlaceRegionGroupOutcome::ExternalResolutionRequired { request };
        return Ok(None);
    };
    ensure_external_request(request, actual)?;
    Ok(Some(()))
}

fn ensure_external_request(
    expected: DropTileExternalRequest,
    resolution: DropTileExternalResolution,
) -> Result<(), PlaceRegionGroupError> {
    let kind_matches = matches!(
        (expected, resolution),
        (
            DropTileExternalRequest::MountainsAddMountain { .. },
            DropTileExternalResolution::Mountains { .. }
        ) | (
            DropTileExternalRequest::OilGoodMutation { .. },
            DropTileExternalResolution::OilGoodsApplied { .. }
        ) | (
            DropTileExternalRequest::CliffsPositionCliff { .. },
            DropTileExternalResolution::Cliffs { .. }
        )
    );
    let actual = match resolution {
        DropTileExternalResolution::Mountains { request, .. }
        | DropTileExternalResolution::OilGoodsApplied { request }
        | DropTileExternalResolution::Cliffs { request, .. } => request,
    };
    if !kind_matches {
        Err(PlaceRegionGroupError::ExternalResolutionKindMismatch { request: expected })
    } else if expected == actual {
        Ok(())
    } else {
        Err(PlaceRegionGroupError::ExternalResolutionMismatch { expected, actual })
    }
}
