//! Source-bound frame-one `Village` process prefix for the selected 2024 replay.
//!
//! Retail reaches `Build::process` at step 14 of the post-command frame-one tick, after the
//! rotated ten-owner Unit-band loop and in the fixed owner-zero Build band. For the Village
//! (`o=2000`), `Wall::process` has no reached dynamic child: frame one is
//! not this owner's eight-frame seen slot, `(frame + o) % 32 == 17`, and
//! `(frame + o) % 16 == 1`.  The only Wall writes are therefore the helper latch.  The exact
//! Build subclass then reaches a local healing countdown. Replay Rules give Village attack 80,
//! but captured negative `attack_ox`/`attack_whom`, phase 17, and negative `near_o` close both
//! attack children. The remaining gates close locally, `Build::do_queue(0)` returns from its
//! exact empty arm, and the first reached source-dependent child is
//! `BuildTypeData::is_gather_type`.
//!
//! This module does not accept those gates as booleans.  It binds the complete post-command
//! capture, the post-Market City receipt, the canonical Build-band rows, and the replay-carried
//! Village `BuildTypeData`.  Planning is detached and applying is atomic: the authority and
//! preimage are revalidated before the sole canonical Build row is published.

#![forbid(unsafe_code)]

use std::fmt;

use don_sim::objects::OWNER_SLOTS;
use don_sim::systems::map_terrain::WorldChecksum;
use don_sim::systems::production::{self, BuildData};
use don_sim::systems::save_load::{save_sim, SaveError};
use don_sim::systems::sparse_object_bands_authority_frontier::{RetailBand, RetailObjectAddress};
use don_sim::systems::tech_cities::{self, CityRecord};
use don_sim::systems::unit_inctime::SUPPORTED_RETAIL_EXE_SHA256;
use don_sim::tick::Sim;
use don_sim::world::WorldObjectIdentity;

use crate::cities_runtime::{check_sim_owned_cities, CitiesChannelValue, CitiesRuntimeError};
use crate::groups_pre_pair_unit_authority::{
    replay_build_type_facts, PrePairUnitAuthorityError, ReplayBuildTypeFacts,
};
use crate::replay::{load_payload, Replay};
use crate::setup_2024_frame379::Frame379SetupEntryReceipt;
use crate::setup_2024_golden_capture::{
    validate_frame1_post_command_authority, Frame1GoldenBindError, Frame1PostCommandAuthority,
};
use crate::setup_2024_starting_market::{
    derive_golden_starting_market_plan, GoldenStartingMarketCaptureSource,
    GoldenStartingMarketCityReceipt, GoldenStartingMarketPlanError, DUTCH_STARTING_MARKET_O,
    MARKET_CITY_FLAGS,
};
use crate::world_owner_frontier::sha256;

pub const BUILD_PROCESS_VA: u32 = 0x0061_edf0;
pub const WALL_PROCESS_VA: u32 = 0x0064_0450;
pub const BUILD_VTABLE_VA: u32 = 0x00b4_2174;
pub const BUILD_DO_QUEUE_VA: u32 = 0x0061_e410;
pub const BUILD_TYPE_IS_GATHER_TYPE_VA: u32 = 0x0047_2bb0;
pub const BUILD_PROCESS_NEXT_VA: u32 = 0x0061_f451;
pub const BUILD_PROCESS_ATTACK_CHILD_VA: u32 = 0x0062_28f0;
pub const OBJECT_CACHED_TARGET_CHILD_VA: u32 = 0x0064_8d70;

pub const WALL_HELPERS_ZERO_MASK_STORE_VA: u32 = 0x0064_084f;
pub const WALL_HELPERS_RESET_STORE_VA: u32 = 0x0064_085a;
pub const WALL_WORKED_MASK_STORE_VA: u32 = 0x0064_085e;
pub const WALL_HELPER_COUNTED_MASK_STORE_VA: u32 = 0x0064_086b;
pub const BUILD_HEALING_STORE_VA: u32 = 0x0061_ee41;
pub const BUILD_DO_QUEUE_CALL_VA: u32 = 0x0061_f3bf;
pub const BUILD_IS_GATHER_TYPE_CALL_VA: u32 = 0x0061_f3ca;

pub const GOLDEN_FRAME: i32 = 1;
pub const GOLDEN_STEP: u8 = 14;
pub const GOLDEN_OWNER: u8 = 0;
pub const GOLDEN_VILLAGE_O: i32 = 2_000;
pub const GOLDEN_VILLAGE_TYPE: i32 = tech_cities::ty::VILLAGE;
pub const GOLDEN_MARKET_TYPE: i32 = tech_cities::ty::MARKET;
pub const GOLDEN_CITY_SLOT: i16 = 0;
pub const GOLDEN_MARKET_SPACE_GRADE: i32 = 4;

const BUILD_LAUNCH_MASK: u16 = 0x0008;
const NEAR_O_OFFSET: usize = 0x34;
const NEAR_WHO_OFFSET: usize = 0x36;
const HEALING_OFFSET: usize = 0x38;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame1VillageStage {
    PostCommandAuthorityBound,
    PostMarketCityBound,
    OwnerRotatedObjectWalkBound,
    WallPeriodicSeenGateSkipped,
    WallSlowSlotSkipped,
    WallHelperLatchWritten,
    WallTerritorySlotSkipped,
    ExactBuildActiveGatePassed,
    HealingCountdownApplied,
    EjectionGateSkipped,
    LaunchGateSkipped,
    AttackChildrenSkipped,
    OwnershipLatchSkipped,
    DamageRecoveryGraphSkipped,
    EmptyQueueChildReturned,
    ReplayBuildTypeGatherChildReturned,
}

pub const FRAME1_VILLAGE_STAGE_ORDER: [Frame1VillageStage; 16] = [
    Frame1VillageStage::PostCommandAuthorityBound,
    Frame1VillageStage::PostMarketCityBound,
    Frame1VillageStage::OwnerRotatedObjectWalkBound,
    Frame1VillageStage::WallPeriodicSeenGateSkipped,
    Frame1VillageStage::WallSlowSlotSkipped,
    Frame1VillageStage::WallHelperLatchWritten,
    Frame1VillageStage::WallTerritorySlotSkipped,
    Frame1VillageStage::ExactBuildActiveGatePassed,
    Frame1VillageStage::HealingCountdownApplied,
    Frame1VillageStage::EjectionGateSkipped,
    Frame1VillageStage::LaunchGateSkipped,
    Frame1VillageStage::AttackChildrenSkipped,
    Frame1VillageStage::OwnershipLatchSkipped,
    Frame1VillageStage::DamageRecoveryGraphSkipped,
    Frame1VillageStage::EmptyQueueChildReturned,
    Frame1VillageStage::ReplayBuildTypeGatherChildReturned,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame1VillageWriteField {
    BuildMasks,
    Helpers,
    Healing,
}

/// One instruction-ordered scalar store. Same-value writes are retained because retail
/// executes them and their order defines the exact prefix even when the checksum is unchanged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame1VillageScalarWrite {
    pub instruction_va: u32,
    pub field: Frame1VillageWriteField,
    pub before: u32,
    pub after: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame1VillageBuildIdentity {
    pub row: usize,
    pub owner: u8,
    pub object_id: i32,
    pub uid: u16,
    pub type_index: i32,
    pub city: i16,
    pub city_down: i16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame1VillageCityFacts {
    pub slot: i16,
    pub flags: u16,
    pub owner: i8,
    pub center_o: i16,
    pub filled: u8,
    pub space_grade: i32,
    pub space: [u8; 3],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame1VillageTraversalReceipt {
    pub frame: i32,
    pub step: u8,
    /// The Unit-band loop starts with `(frame + 0) % 10` and completes before any Build.
    pub first_unit_owner: u8,
    /// Build bands then use fixed owner order `0..8`; Village is in the first Build owner.
    pub build_owner_ordinal: u8,
    pub owner: u8,
    pub object_id: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame1VillageWallReceipt {
    pub periodic_seen_due: bool,
    pub phase: i32,
    pub slow_slot_due: bool,
    pub territory_slot_due: bool,
    pub helpers_before: u8,
    pub helpers_after: u8,
    pub build_masks_before: u16,
    pub build_masks_after: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame1VillageBuildPrefixReceipt {
    pub native_vtable: u32,
    pub active: bool,
    pub healing_before: u16,
    pub healing_after: u16,
    pub queue_rows: usize,
    pub logical_queued: u8,
    pub attack: i32,
    pub attack_graph_entered: bool,
    pub attack_ox: i16,
    pub attack_whom: i8,
    pub near_o: i16,
    pub near_who: i16,
    pub attack_refresh_due: bool,
    pub attack_process_child_va: u32,
    pub cached_target_child_va: u32,
    pub ownership_latch: bool,
    pub damage: i32,
    pub damage_frac: i8,
    pub gather_build_flags: u32,
    pub is_gather_type: bool,
    pub queue_child_va: u32,
    pub gather_child_va: u32,
}

/// Detached, source-bound plan. Its digest includes the authority revision/digest, exact
/// Village/City/Market preimages, replay byte spans, and every ordered scalar write.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame1VillageProcessPlan {
    pub authority_revision: u64,
    pub authority_digest: [u8; 32],
    pub post_command_sim_sha256: [u8; 32],
    pub replay_payload_sha256: [u8; 32],
    pub setup_entry_sim_sha256: [u8; 32],
    pub market_native_trace_sha256: [u8; 32],
    pub village: Frame1VillageBuildIdentity,
    pub market: Frame1VillageBuildIdentity,
    pub city: Frame1VillageCityFacts,
    pub city_channel: CitiesChannelValue,
    pub world_checksum: WorldChecksum,
    pub random_state: i32,
    pub village_preimage_sha256: [u8; 32],
    pub market_preimage_sha256: [u8; 32],
    pub city_preimage_sha256: [u8; 32],
    pub village_postimage_sha256: [u8; 32],
    pub village_type: ReplayBuildTypeFacts,
    pub traversal: Frame1VillageTraversalReceipt,
    pub wall: Frame1VillageWallReceipt,
    pub build: Frame1VillageBuildPrefixReceipt,
    pub writes: Vec<Frame1VillageScalarWrite>,
    pub stage_order: [Frame1VillageStage; 16],
    pub next_exact_boundary_va: u32,
    pub next_exact_boundary: &'static str,
    pub composition_digest: [u8; 32],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame1VillageProcessReceipt {
    pub plan: Frame1VillageProcessPlan,
    /// Locally serialized result of this child prefix. No adjacent retail capture is claimed.
    pub derived_post_prefix_sim_sha256: [u8; 32],
    pub world_checksum_after: WorldChecksum,
    pub random_state_after: i32,
    pub city_channel_after: CitiesChannelValue,
    pub city_owner_unchanged: bool,
    pub adjacent_oracle_sim_sha256: Option<[u8; 32]>,
}

#[derive(Debug)]
pub enum Frame1VillageProcessError {
    Golden(Frame1GoldenBindError),
    MarketPlan(GoldenStartingMarketPlanError),
    MarketEntryMismatch,
    MarketReceiptMismatch,
    PayloadRead(String),
    PayloadMismatch,
    MissingRules,
    TypeFacts(PrePairUnitAuthorityError),
    Snapshot(SaveError),
    Cities(CitiesRuntimeError),
    PostMarketCityChannelMismatch,
    MissingVillage,
    VillageIdentityMismatch,
    MissingMarket,
    MarketIdentityMismatch,
    MissingCity,
    PostMarketCityMismatch,
    WrongWallSchedule,
    InactiveVillage,
    EjectionChildReached,
    LaunchChildReached,
    AttackProcessChildReached { attack_ox: i16, attack_whom: i8 },
    AttackTargetChildReached { near_o: i16, near_who: i16 },
    OwnershipChildReached,
    DamageRecoveryChildReached { damage: i32, damage_frac: i8 },
    NonEmptyQueue { queued: u8 },
    GatherTailReached,
    DetachedPlanMismatch,
    StaleVillagePreimage,
    StaleCityPreimage,
    PostWorldChanged,
    PostRandomChanged,
    PostCityChanged,
}

impl fmt::Display for Frame1VillageProcessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "2024 frame-1 Village process prefix refused: {self:?}")
    }
}

impl std::error::Error for Frame1VillageProcessError {}

impl From<Frame1GoldenBindError> for Frame1VillageProcessError {
    fn from(value: Frame1GoldenBindError) -> Self {
        Self::Golden(value)
    }
}

impl From<PrePairUnitAuthorityError> for Frame1VillageProcessError {
    fn from(value: PrePairUnitAuthorityError) -> Self {
        Self::TypeFacts(value)
    }
}

impl From<GoldenStartingMarketPlanError> for Frame1VillageProcessError {
    fn from(value: GoldenStartingMarketPlanError) -> Self {
        Self::MarketPlan(value)
    }
}

impl From<CitiesRuntimeError> for Frame1VillageProcessError {
    fn from(value: CitiesRuntimeError) -> Self {
        Self::Cities(value)
    }
}

fn healing(build: &BuildData) -> u16 {
    u16::from_le_bytes(
        build.other[HEALING_OFFSET..HEALING_OFFSET + 2]
            .try_into()
            .expect("fixed BuildData healing window"),
    )
}

fn set_healing(build: &mut BuildData, value: u16) {
    build.other[HEALING_OFFSET..HEALING_OFFSET + 2].copy_from_slice(&value.to_le_bytes());
}

fn object_i16(build: &BuildData, offset: usize) -> i16 {
    i16::from_le_bytes(
        build.other[offset..offset + 2]
            .try_into()
            .expect("fixed BuildData ObjectData short window"),
    )
}

fn build_preimage_sha256(build: &BuildData) -> [u8; 32] {
    let mut image = b"don-frame1-village-build-preimage-v1".to_vec();
    image.extend_from_slice(&build.image());
    image.extend_from_slice(&(build.queue.entries.len() as u64).to_le_bytes());
    for entry in &build.queue.entries {
        image.extend_from_slice(&entry.image());
    }
    image.extend_from_slice(&(build.gather_from.tiles.len() as u64).to_le_bytes());
    for tile in &build.gather_from.tiles {
        image.extend_from_slice(&tile.to_le_bytes());
    }
    image.extend_from_slice(&(build.gather.len() as u64).to_le_bytes());
    for point in &build.gather {
        image.extend_from_slice(&point.x.to_le_bytes());
        image.extend_from_slice(&point.y.to_le_bytes());
        image.push(point.action);
        image.push(point.node_tag);
    }
    sha256(&image)
}

fn city_preimage_sha256(city: &CityRecord) -> [u8; 32] {
    let mut image = b"don-frame1-village-city-preimage-v1".to_vec();
    image.extend_from_slice(&city.city_flags.to_le_bytes());
    image.extend_from_slice(&city.pod_bytes());
    image.extend_from_slice(&(city.vans.items.len() as u64).to_le_bytes());
    image.extend_from_slice(&city.vans.capacity.to_le_bytes());
    image.extend_from_slice(&city.vans.grow.to_le_bytes());
    image.push(city.vans.flags);
    for link in &city.vans.items {
        image.extend_from_slice(&link.cara.to_le_bytes());
        image.extend_from_slice(&link.who.to_le_bytes());
    }
    image.extend_from_slice(&(city.name.len() as u64).to_le_bytes());
    image.extend_from_slice(city.name.as_bytes());
    image.extend_from_slice(&(city.id.len() as u64).to_le_bytes());
    image.extend_from_slice(city.id.as_bytes());
    sha256(&image)
}

/// Recovered `BuildTypeData::is_gather_type` (`0x00472BB0`).
pub fn build_type_is_gather_type(facts: &ReplayBuildTypeFacts) -> bool {
    facts.build_flags & 0x40 != 0
}

fn derive_local_prefix(
    build: &BuildData,
    facts: &ReplayBuildTypeFacts,
    frame: i32,
) -> Result<
    (
        BuildData,
        Frame1VillageWallReceipt,
        Frame1VillageBuildPrefixReceipt,
        Vec<Frame1VillageScalarWrite>,
    ),
    Frame1VillageProcessError,
> {
    let object_id = i32::from(build.object_id());
    let periodic_seen_due = frame != 0 && (frame as u8 & 7) == build.who;
    let phase = frame.wrapping_add(object_id);
    let slow_slot_due = phase % 32 == 0;
    let territory_slot_due = phase % 16 == 0;
    if periodic_seen_due || slow_slot_due || territory_slot_due {
        return Err(Frame1VillageProcessError::WrongWallSchedule);
    }
    if !build.is_active() {
        return Err(Frame1VillageProcessError::InactiveVillage);
    }

    let mut after = build.clone();
    let mut writes = Vec::with_capacity(4);
    let masks_before = after.build_masks;
    let helpers_before = after.helpers;
    if after.helpers == 0 {
        let before = after.build_masks;
        after.build_masks &= !production::mask::WORKED_LAST_FRAME;
        writes.push(Frame1VillageScalarWrite {
            instruction_va: WALL_HELPERS_ZERO_MASK_STORE_VA,
            field: Frame1VillageWriteField::BuildMasks,
            before: u32::from(before),
            after: u32::from(after.build_masks),
        });
    } else {
        let before = after.helpers;
        after.helpers = 0;
        writes.push(Frame1VillageScalarWrite {
            instruction_va: WALL_HELPERS_RESET_STORE_VA,
            field: Frame1VillageWriteField::Helpers,
            before: u32::from(before),
            after: 0,
        });
        let before = after.build_masks;
        after.build_masks |= production::mask::WORKED_LAST_FRAME;
        writes.push(Frame1VillageScalarWrite {
            instruction_va: WALL_WORKED_MASK_STORE_VA,
            field: Frame1VillageWriteField::BuildMasks,
            before: u32::from(before),
            after: u32::from(after.build_masks),
        });
    }
    let before = after.build_masks;
    after.build_masks &= !production::mask::HELPER_COUNTED;
    writes.push(Frame1VillageScalarWrite {
        instruction_va: WALL_HELPER_COUNTED_MASK_STORE_VA,
        field: Frame1VillageWriteField::BuildMasks,
        before: u32::from(before),
        after: u32::from(after.build_masks),
    });
    let wall = Frame1VillageWallReceipt {
        periodic_seen_due,
        phase,
        slow_slot_due,
        territory_slot_due,
        helpers_before,
        helpers_after: after.helpers,
        build_masks_before: masks_before,
        build_masks_after: after.build_masks,
    };

    let healing_before = healing(&after);
    let healing_after = if healing_before == 0 {
        0
    } else {
        healing_before - 1
    };
    if healing_before != 0 {
        set_healing(&mut after, healing_after);
        writes.push(Frame1VillageScalarWrite {
            instruction_va: BUILD_HEALING_STORE_VA,
            field: Frame1VillageWriteField::Healing,
            before: u32::from(healing_before),
            after: u32::from(healing_after),
        });
    }
    if after.build_masks & production::mask::EJECTING != 0 {
        return Err(Frame1VillageProcessError::EjectionChildReached);
    }
    if after.build_masks & BUILD_LAUNCH_MASK != 0 {
        return Err(Frame1VillageProcessError::LaunchChildReached);
    }
    let attack_graph_entered = facts.attack != 0;
    let near_o = object_i16(&after, NEAR_O_OFFSET);
    let near_who = object_i16(&after, NEAR_WHO_OFFSET);
    let attack_refresh_due = phase % 32 == 0;
    if attack_graph_entered {
        if after.attack_ox >= 0 && after.attack_whom >= 0 {
            return Err(Frame1VillageProcessError::AttackProcessChildReached {
                attack_ox: after.attack_ox,
                attack_whom: after.attack_whom,
            });
        }
        if attack_refresh_due {
            return Err(Frame1VillageProcessError::AttackProcessChildReached {
                attack_ox: after.attack_ox,
                attack_whom: after.attack_whom,
            });
        }
        if near_o >= 0 {
            return Err(Frame1VillageProcessError::AttackTargetChildReached { near_o, near_who });
        }
    }
    if after.build_masks & production::mask::OWNERSHIP_LATCH != 0 {
        return Err(Frame1VillageProcessError::OwnershipChildReached);
    }
    // The three Leader shorts at 0x0061F282..0x0061F29E can only make retail inspect these
    // two Build fields. Both zero dominates either Leader outcome without inventing it.
    if after.damage != 0 || after.damage_frac != 0 {
        return Err(Frame1VillageProcessError::DamageRecoveryChildReached {
            damage: after.damage,
            damage_frac: after.damage_frac,
        });
    }
    if after.queue.queued != 0 {
        return Err(Frame1VillageProcessError::NonEmptyQueue {
            queued: after.queue.queued,
        });
    }
    let is_gather_type = build_type_is_gather_type(facts);
    if is_gather_type {
        return Err(Frame1VillageProcessError::GatherTailReached);
    }
    let build_receipt = Frame1VillageBuildPrefixReceipt {
        native_vtable: BUILD_VTABLE_VA,
        active: true,
        healing_before,
        healing_after,
        queue_rows: after.queue.num(),
        logical_queued: after.queue.queued,
        attack: facts.attack,
        attack_graph_entered,
        attack_ox: after.attack_ox,
        attack_whom: after.attack_whom,
        near_o,
        near_who,
        attack_refresh_due,
        attack_process_child_va: BUILD_PROCESS_ATTACK_CHILD_VA,
        cached_target_child_va: OBJECT_CACHED_TARGET_CHILD_VA,
        ownership_latch: false,
        damage: after.damage,
        damage_frac: after.damage_frac,
        gather_build_flags: facts.build_flags,
        is_gather_type,
        queue_child_va: BUILD_DO_QUEUE_VA,
        gather_child_va: BUILD_TYPE_IS_GATHER_TYPE_VA,
    };
    Ok((after, wall, build_receipt, writes))
}

fn build_identity(build: &BuildData, row: usize, type_index: i32) -> Frame1VillageBuildIdentity {
    Frame1VillageBuildIdentity {
        row,
        owner: build.who,
        object_id: i32::from(build.object_id()),
        uid: build.uid,
        type_index,
        city: build.city,
        city_down: build.city_down,
    }
}

fn append_span(image: &mut Vec<u8>, offset: usize, len: usize) {
    image.extend_from_slice(&(offset as u64).to_le_bytes());
    image.extend_from_slice(&(len as u64).to_le_bytes());
}

fn plan_digest(plan: &Frame1VillageProcessPlan) -> [u8; 32] {
    let mut image = b"don-frame1-village-process-plan-v1".to_vec();
    image.extend_from_slice(&plan.authority_revision.to_le_bytes());
    image.extend_from_slice(&plan.authority_digest);
    image.extend_from_slice(&plan.post_command_sim_sha256);
    image.extend_from_slice(&plan.replay_payload_sha256);
    image.extend_from_slice(&plan.setup_entry_sim_sha256);
    image.extend_from_slice(&plan.market_native_trace_sha256);
    image.extend_from_slice(&(plan.village.row as u64).to_le_bytes());
    image.extend_from_slice(&plan.village.object_id.to_le_bytes());
    image.extend_from_slice(&plan.village.uid.to_le_bytes());
    image.extend_from_slice(&(plan.market.row as u64).to_le_bytes());
    image.extend_from_slice(&plan.market.object_id.to_le_bytes());
    image.extend_from_slice(&plan.market.uid.to_le_bytes());
    image.extend_from_slice(&plan.city.slot.to_le_bytes());
    image.extend_from_slice(&plan.city.flags.to_le_bytes());
    image.extend_from_slice(&plan.city.owner.to_le_bytes());
    image.extend_from_slice(&plan.city.center_o.to_le_bytes());
    image.push(plan.city.filled);
    image.extend_from_slice(&plan.city.space_grade.to_le_bytes());
    image.extend_from_slice(&plan.city.space);
    image.extend_from_slice(&plan.city_channel.checksum.to_le_bytes());
    image.extend_from_slice(&plan.city_channel.bytes_walked.to_le_bytes());
    image.extend_from_slice(&plan.city_channel.cities_walked.to_le_bytes());
    image.extend_from_slice(&plan.city_channel.slots_scanned.to_le_bytes());
    image.extend_from_slice(&plan.world_checksum.full.to_le_bytes());
    image.extend_from_slice(&plan.world_checksum.bytes.to_le_bytes());
    for section in &plan.world_checksum.per_section {
        image.extend_from_slice(&section.adler.to_le_bytes());
        image.extend_from_slice(&section.bytes.to_le_bytes());
    }
    image.extend_from_slice(&plan.random_state.to_le_bytes());
    image.extend_from_slice(&plan.village_preimage_sha256);
    image.extend_from_slice(&plan.market_preimage_sha256);
    image.extend_from_slice(&plan.city_preimage_sha256);
    image.extend_from_slice(&plan.village_postimage_sha256);
    append_span(
        &mut image,
        plan.village_type.spans.type_base.offset,
        plan.village_type.spans.type_base.bytes,
    );
    append_span(
        &mut image,
        plan.village_type.spans.object.offset,
        plan.village_type.spans.object.bytes,
    );
    append_span(
        &mut image,
        plan.village_type.spans.build.offset,
        plan.village_type.spans.build.bytes,
    );
    image.extend_from_slice(&plan.village_type.attack.to_le_bytes());
    image.extend_from_slice(&plan.village_type.build_flags.to_le_bytes());
    image.extend_from_slice(&plan.traversal.frame.to_le_bytes());
    image.push(plan.traversal.step);
    image.push(plan.traversal.first_unit_owner);
    image.push(plan.traversal.build_owner_ordinal);
    image.push(plan.traversal.owner);
    image.extend_from_slice(&plan.traversal.object_id.to_le_bytes());
    image.push(u8::from(plan.wall.periodic_seen_due));
    image.extend_from_slice(&plan.wall.phase.to_le_bytes());
    image.push(u8::from(plan.wall.slow_slot_due));
    image.push(u8::from(plan.wall.territory_slot_due));
    image.push(plan.wall.helpers_before);
    image.push(plan.wall.helpers_after);
    image.extend_from_slice(&plan.wall.build_masks_before.to_le_bytes());
    image.extend_from_slice(&plan.wall.build_masks_after.to_le_bytes());
    image.extend_from_slice(&plan.build.attack.to_le_bytes());
    image.push(u8::from(plan.build.attack_graph_entered));
    image.extend_from_slice(&plan.build.attack_ox.to_le_bytes());
    image.extend_from_slice(&plan.build.attack_whom.to_le_bytes());
    image.extend_from_slice(&plan.build.near_o.to_le_bytes());
    image.extend_from_slice(&plan.build.near_who.to_le_bytes());
    image.push(u8::from(plan.build.attack_refresh_due));
    image.extend_from_slice(&plan.build.gather_build_flags.to_le_bytes());
    image.push(u8::from(plan.build.is_gather_type));
    for write in &plan.writes {
        image.extend_from_slice(&write.instruction_va.to_le_bytes());
        image.push(write.field as u8);
        image.extend_from_slice(&write.before.to_le_bytes());
        image.extend_from_slice(&write.after.to_le_bytes());
    }
    image.extend_from_slice(&plan.next_exact_boundary_va.to_le_bytes());
    sha256(&image)
}

/// Bind the exact post-command frame-one Sim and produce the detached Village prefix plan.
pub fn plan_frame1_village_process_prefix(
    replay: &Replay,
    setup_entry: &Frame379SetupEntryReceipt,
    market_receipt: &GoldenStartingMarketCityReceipt,
    authority: &Frame1PostCommandAuthority,
    candidate: &Sim,
) -> Result<Frame1VillageProcessPlan, Frame1VillageProcessError> {
    validate_frame1_post_command_authority(setup_entry, authority, candidate)?;
    let source_market_plan = derive_golden_starting_market_plan(replay)?;
    if setup_entry.entry_sim_sha256 != market_receipt.after_sim_sha256 {
        return Err(Frame1VillageProcessError::MarketEntryMismatch);
    }
    if market_receipt.placement.plan != source_market_plan
        || !market_receipt.placement.footprint.validates()
        || market_receipt.placement.candidate.continuation
            != Some(market_receipt.placement.footprint.entry.input)
        || !market_receipt.placement.blocked_location.validates()
        || market_receipt.placement.blocked_location.input != market_receipt.placement.footprint
        || market_receipt.placement.source_produced_city_bytes != 0
        || market_receipt.placement.installed_in_scoreboard
        || market_receipt.source
            != GoldenStartingMarketCaptureSource::CompleteRetailLeaderProduceBuildingReturn
        || market_receipt.executable_sha256 != SUPPORTED_RETAIL_EXE_SHA256
        || market_receipt.native_trace_sha256 == [0; 32]
        || market_receipt.market_build_o != DUTCH_STARTING_MARKET_O
        || market_receipt.market_city_slot != GOLDEN_CITY_SLOT
        || market_receipt.space_grade != GOLDEN_MARKET_SPACE_GRADE
        || market_receipt.source_produced_city_bytes != market_receipt.after_cities.bytes_walked
        || market_receipt.installed_in_scoreboard
    {
        return Err(Frame1VillageProcessError::MarketReceiptMismatch);
    }
    let candidate_sim_bytes = save_sim(candidate).map_err(Frame1VillageProcessError::Snapshot)?;
    if sha256(&candidate_sim_bytes) != authority.post_command_sim_sha256 {
        return Err(Frame1VillageProcessError::DetachedPlanMismatch);
    }
    let city_channel = check_sim_owned_cities(candidate)?;
    if city_channel != market_receipt.after_cities {
        return Err(Frame1VillageProcessError::PostMarketCityChannelMismatch);
    }

    let payload = load_payload(&replay.path)
        .map_err(|error| Frame1VillageProcessError::PayloadRead(error.to_string()))?;
    if sha256(&payload) != replay.initial.payload_sha256 {
        return Err(Frame1VillageProcessError::PayloadMismatch);
    }
    let rules = replay
        .initial
        .rules
        .ok_or(Frame1VillageProcessError::MissingRules)?;
    let village_type = replay_build_type_facts(&payload, &rules, GOLDEN_VILLAGE_TYPE)?;

    if !candidate.world.object_bands_are_dense_equivalent() {
        return Err(Frame1VillageProcessError::VillageIdentityMismatch);
    }
    let village_row = setup_entry.center_build_row;
    let village = candidate
        .builds
        .get(village_row)
        .ok_or(Frame1VillageProcessError::MissingVillage)?;
    let village_runtime_type = candidate
        .production_runtime
        .build_types
        .get(village_row)
        .copied()
        .flatten()
        .ok_or(Frame1VillageProcessError::VillageIdentityMismatch)?;
    let village_address =
        RetailObjectAddress::new(GOLDEN_OWNER, RetailBand::Build, GOLDEN_VILLAGE_O);
    if setup_entry.center_build_o != GOLDEN_VILLAGE_O
        || setup_entry.center_city_slot != GOLDEN_CITY_SLOT
        || village.who != GOLDEN_OWNER
        || i32::from(village.object_id()) != GOLDEN_VILLAGE_O
        || village_runtime_type != GOLDEN_VILLAGE_TYPE
        || village.city != GOLDEN_CITY_SLOT
        || village.city_down != DUTCH_STARTING_MARKET_O as i16
        || village.stance != 1
        || candidate
            .world
            .object_bands()
            .live_identity(village_address)
            != Some(WorldObjectIdentity::BuildRow(village_row as u32))
    {
        return Err(Frame1VillageProcessError::VillageIdentityMismatch);
    }

    let market_address =
        RetailObjectAddress::new(GOLDEN_OWNER, RetailBand::Build, DUTCH_STARTING_MARKET_O);
    let Some(WorldObjectIdentity::BuildRow(market_row)) =
        candidate.world.object_bands().live_identity(market_address)
    else {
        return Err(Frame1VillageProcessError::MissingMarket);
    };
    let market_row = market_row as usize;
    let market = candidate
        .builds
        .get(market_row)
        .ok_or(Frame1VillageProcessError::MissingMarket)?;
    let market_runtime_type = candidate
        .production_runtime
        .build_types
        .get(market_row)
        .copied()
        .flatten()
        .ok_or(Frame1VillageProcessError::MarketIdentityMismatch)?;
    if market.who != GOLDEN_OWNER
        || i32::from(market.object_id()) != DUTCH_STARTING_MARKET_O
        || market_runtime_type != GOLDEN_MARKET_TYPE
        || market.city != GOLDEN_CITY_SLOT
        || market.city_down != -1
        || market.flags & production::flag::VALID == 0
    {
        return Err(Frame1VillageProcessError::MarketIdentityMismatch);
    }

    let city = candidate
        .cities
        .slots
        .get(usize::from(GOLDEN_OWNER))
        .and_then(|owner| owner.get(GOLDEN_CITY_SLOT as usize))
        .ok_or(Frame1VillageProcessError::MissingCity)?;
    if !city.active()
        || city.city != GOLDEN_CITY_SLOT
        || city.o != GOLDEN_VILLAGE_O as i16
        || city.who != GOLDEN_OWNER as i8
        || city.city_flags != MARKET_CITY_FLAGS
        || city.filled != 2
    {
        return Err(Frame1VillageProcessError::PostMarketCityMismatch);
    }

    let (village_after, wall, build, writes) =
        derive_local_prefix(village, &village_type, GOLDEN_FRAME)?;
    let village_identity = build_identity(village, village_row, village_runtime_type);
    let market_identity = build_identity(market, market_row, market_runtime_type);
    let city_facts = Frame1VillageCityFacts {
        slot: city.city,
        flags: city.city_flags,
        owner: city.who,
        center_o: city.o,
        filled: city.filled,
        space_grade: market_receipt.space_grade,
        space: city.space,
    };
    let mut plan = Frame1VillageProcessPlan {
        authority_revision: authority.revision,
        authority_digest: authority.composition_digest,
        post_command_sim_sha256: authority.post_command_sim_sha256,
        replay_payload_sha256: replay.initial.payload_sha256,
        setup_entry_sim_sha256: setup_entry.entry_sim_sha256,
        market_native_trace_sha256: market_receipt.native_trace_sha256,
        village: village_identity,
        market: market_identity,
        city: city_facts,
        city_channel,
        world_checksum: candidate.map.world.checksum_sections(),
        random_state: candidate.world.random.state(),
        village_preimage_sha256: build_preimage_sha256(village),
        market_preimage_sha256: build_preimage_sha256(market),
        city_preimage_sha256: city_preimage_sha256(city),
        village_postimage_sha256: build_preimage_sha256(&village_after),
        village_type,
        traversal: Frame1VillageTraversalReceipt {
            frame: GOLDEN_FRAME,
            step: GOLDEN_STEP,
            first_unit_owner: GOLDEN_FRAME.rem_euclid(OWNER_SLOTS as i32) as u8,
            build_owner_ordinal: 0,
            owner: GOLDEN_OWNER,
            object_id: GOLDEN_VILLAGE_O,
        },
        wall,
        build,
        writes,
        stage_order: FRAME1_VILLAGE_STAGE_ORDER,
        next_exact_boundary_va: BUILD_PROCESS_NEXT_VA,
        next_exact_boundary:
            "Build::process type-specific tail beginning with the BuildType 0x21a test",
        composition_digest: [0; 32],
    };
    plan.composition_digest = plan_digest(&plan);
    Ok(plan)
}

/// Atomically publish a previously inspected detached plan.
///
/// The replay-derived plan is recomputed and compared before any write. On every refusal the
/// untouched owned Sim is returned to the caller.
pub fn mount_frame1_village_process_prefix(
    replay: &Replay,
    setup_entry: &Frame379SetupEntryReceipt,
    market_receipt: &GoldenStartingMarketCityReceipt,
    authority: &Frame1PostCommandAuthority,
    mut candidate: Sim,
    detached: &Frame1VillageProcessPlan,
) -> Result<(Sim, Frame1VillageProcessReceipt), (Frame1VillageProcessError, Sim)> {
    let plan = match plan_frame1_village_process_prefix(
        replay,
        setup_entry,
        market_receipt,
        authority,
        &candidate,
    ) {
        Ok(plan) => plan,
        Err(error) => return Err((error, candidate)),
    };
    if &plan != detached || plan.composition_digest != plan_digest(&plan) {
        return Err((Frame1VillageProcessError::DetachedPlanMismatch, candidate));
    }
    let village_before = candidate.builds[plan.village.row].clone();
    let city_before =
        candidate.cities.slots[usize::from(GOLDEN_OWNER)][GOLDEN_CITY_SLOT as usize].clone();
    if build_preimage_sha256(&village_before) != plan.village_preimage_sha256 {
        return Err((Frame1VillageProcessError::StaleVillagePreimage, candidate));
    }
    if city_preimage_sha256(&city_before) != plan.city_preimage_sha256 {
        return Err((Frame1VillageProcessError::StaleCityPreimage, candidate));
    }
    let (village_after, _, _, _) =
        match derive_local_prefix(&village_before, &plan.village_type, GOLDEN_FRAME) {
            Ok(result) => result,
            Err(error) => return Err((error, candidate)),
        };
    if build_preimage_sha256(&village_after) != plan.village_postimage_sha256 {
        return Err((Frame1VillageProcessError::DetachedPlanMismatch, candidate));
    }

    // Install only while serializing the result. A structural refusal restores the exact row.
    candidate.builds[plan.village.row] = village_after;
    let post_bytes = match save_sim(&candidate) {
        Ok(bytes) => bytes,
        Err(error) => {
            candidate.builds[plan.village.row] = village_before;
            return Err((Frame1VillageProcessError::Snapshot(error), candidate));
        }
    };
    let world_after = candidate.map.world.checksum_sections();
    if world_after != plan.world_checksum {
        candidate.builds[plan.village.row] = village_before;
        return Err((Frame1VillageProcessError::PostWorldChanged, candidate));
    }
    let random_after = candidate.world.random.state();
    if random_after != plan.random_state {
        candidate.builds[plan.village.row] = village_before;
        return Err((Frame1VillageProcessError::PostRandomChanged, candidate));
    }
    let city_after =
        candidate.cities.slots[usize::from(GOLDEN_OWNER)][GOLDEN_CITY_SLOT as usize].clone();
    let city_channel_after = match check_sim_owned_cities(&candidate) {
        Ok(value) => value,
        Err(error) => {
            candidate.builds[plan.village.row] = village_before;
            return Err((Frame1VillageProcessError::Cities(error), candidate));
        }
    };
    if city_after != city_before || city_channel_after != plan.city_channel {
        candidate.builds[plan.village.row] = village_before;
        return Err((Frame1VillageProcessError::PostCityChanged, candidate));
    }
    let receipt = Frame1VillageProcessReceipt {
        plan,
        derived_post_prefix_sim_sha256: sha256(&post_bytes),
        world_checksum_after: world_after,
        random_state_after: random_after,
        city_channel_after,
        city_owner_unchanged: true,
        adjacent_oracle_sim_sha256: None,
    };
    Ok((candidate, receipt))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::groups_pre_pair_unit_authority::ReplayBuildTypeSpans;
    use crate::initial::ReplayByteSpan;
    use std::path::Path;

    fn facts(attack: i32, build_flags: u32) -> ReplayBuildTypeFacts {
        ReplayBuildTypeFacts {
            spans: ReplayBuildTypeSpans {
                type_base: ReplayByteSpan {
                    offset: 1,
                    bytes: 90,
                },
                object: ReplayByteSpan {
                    offset: 91,
                    bytes: 152,
                },
                build: ReplayByteSpan {
                    offset: 243,
                    bytes: 49,
                },
            },
            type_index: GOLDEN_VILLAGE_TYPE,
            from: -1,
            job_time: 0,
            costs: [0; 6],
            upgrade: -1,
            jump: -1,
            obj_masks: 0,
            attack,
            hits: 0,
            domain: 0,
            x_size: 7,
            y_size: 7,
            graft: 0,
            age: 0,
            town_hits: 0,
            min_city_size: 0,
            misery_rate: 0,
            build_flags,
            most_shots: 0,
            garrison_max: 0,
            base_arrows: 0,
            wonder_val: 0,
            plunder_value: 0,
            plunder_good: 0,
            behind_height: 0,
            to: -1,
            civ_graph_mask: 0,
        }
    }

    fn village() -> BuildData {
        let mut build = BuildData::default();
        build.flags =
            production::flag::VALID | production::flag::STARTED | production::flag::ACTIVE | 0x20;
        build.who = GOLDEN_OWNER;
        build.other[production::off::OBJECT_ID..production::off::OBJECT_ID + 2]
            .copy_from_slice(&(GOLDEN_VILLAGE_O as i16).to_le_bytes());
        build.build_masks = 0x1000;
        build.city = GOLDEN_CITY_SLOT;
        build.city_down = DUTCH_STARTING_MARKET_O as i16;
        build.attack_ox = -1;
        build.attack_whom = -1;
        build.other[NEAR_O_OFFSET..NEAR_O_OFFSET + 2].copy_from_slice(&(-1_i16).to_le_bytes());
        build.other[NEAR_WHO_OFFSET..NEAR_WHO_OFFSET + 2].copy_from_slice(&(-1_i16).to_le_bytes());
        build
    }

    #[test]
    fn exact_frame_one_wall_schedule_and_empty_queue_reach_first_type_child() {
        let build = village();
        let (after, wall, prefix, writes) =
            derive_local_prefix(&build, &facts(80, 0), GOLDEN_FRAME).unwrap();
        assert_eq!(wall.phase, 2001);
        assert!(!wall.periodic_seen_due);
        assert!(!wall.slow_slot_due);
        assert!(!wall.territory_slot_due);
        assert_eq!((after.helpers, after.build_masks), (0, 0x1000));
        assert_eq!(writes.len(), 2);
        assert_eq!(writes[0].instruction_va, WALL_HELPERS_ZERO_MASK_STORE_VA);
        assert_eq!(writes[1].instruction_va, WALL_HELPER_COUNTED_MASK_STORE_VA);
        assert_eq!(prefix.logical_queued, 0);
        assert!(prefix.attack_graph_entered);
        assert_eq!((prefix.near_o, prefix.near_who), (-1, -1));
        assert!(!prefix.is_gather_type);
    }

    #[test]
    fn helper_latch_and_healing_writes_preserve_native_order() {
        let mut build = village();
        build.helpers = 3;
        build.build_masks |= production::mask::HELPER_COUNTED;
        set_healing(&mut build, 2);
        let (after, _, _, writes) =
            derive_local_prefix(&build, &facts(80, 0), GOLDEN_FRAME).unwrap();
        assert_eq!(
            (after.helpers, after.build_masks, healing(&after)),
            (0, 0x1400, 1)
        );
        assert_eq!(
            writes
                .iter()
                .map(|write| write.instruction_va)
                .collect::<Vec<_>>(),
            vec![
                WALL_HELPERS_RESET_STORE_VA,
                WALL_WORKED_MASK_STORE_VA,
                WALL_HELPER_COUNTED_MASK_STORE_VA,
                BUILD_HEALING_STORE_VA,
            ]
        );
    }

    #[test]
    fn every_unowned_dynamic_child_fails_closed() {
        let mut build = village();
        build.build_masks |= production::mask::EJECTING;
        assert!(matches!(
            derive_local_prefix(&build, &facts(80, 0), GOLDEN_FRAME),
            Err(Frame1VillageProcessError::EjectionChildReached)
        ));
        let mut build = village();
        build.build_masks |= BUILD_LAUNCH_MASK;
        assert!(matches!(
            derive_local_prefix(&build, &facts(80, 0), GOLDEN_FRAME),
            Err(Frame1VillageProcessError::LaunchChildReached)
        ));
        let mut build = village();
        build.attack_ox = 0;
        build.attack_whom = 0;
        assert!(matches!(
            derive_local_prefix(&build, &facts(80, 0), GOLDEN_FRAME),
            Err(Frame1VillageProcessError::AttackProcessChildReached { .. })
        ));
        let mut build = village();
        build.other[NEAR_O_OFFSET..NEAR_O_OFFSET + 2].copy_from_slice(&7_i16.to_le_bytes());
        assert!(matches!(
            derive_local_prefix(&build, &facts(80, 0), GOLDEN_FRAME),
            Err(Frame1VillageProcessError::AttackTargetChildReached { near_o: 7, .. })
        ));
        let mut build = village();
        build.build_masks |= production::mask::OWNERSHIP_LATCH;
        assert!(matches!(
            derive_local_prefix(&build, &facts(80, 0), GOLDEN_FRAME),
            Err(Frame1VillageProcessError::OwnershipChildReached)
        ));
        let mut build = village();
        build.damage_frac = 1;
        assert!(matches!(
            derive_local_prefix(&build, &facts(80, 0), GOLDEN_FRAME),
            Err(Frame1VillageProcessError::DamageRecoveryChildReached { .. })
        ));
        let mut build = village();
        build.queue.queued = 1;
        assert!(matches!(
            derive_local_prefix(&build, &facts(80, 0), GOLDEN_FRAME),
            Err(Frame1VillageProcessError::NonEmptyQueue { queued: 1 })
        ));
        assert!(matches!(
            derive_local_prefix(&village(), &facts(80, 0x40), GOLDEN_FRAME),
            Err(Frame1VillageProcessError::GatherTailReached)
        ));
    }

    #[test]
    fn installed_golden_rules_close_attack_and_gather_branches() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../ron-data/replays/multi/Playback___2024.02.23_20_49_35__Fri_.rcx");
        if !path.exists() {
            eprintln!("SKIPPED -- NOT A PASS: missing {}", path.display());
            return;
        }
        let replay = Replay::open(&path).expect("installed 2024 witness must decode");
        let payload = load_payload(&path).expect("installed 2024 witness must inflate");
        let rules = replay
            .initial
            .rules
            .expect("installed 2024 witness must carry admitted Rules");
        let village = replay_build_type_facts(&payload, &rules, GOLDEN_VILLAGE_TYPE).unwrap();
        assert_eq!(village.attack, 80);
        assert_eq!(village.build_flags & 0x40, 0);
    }
}
