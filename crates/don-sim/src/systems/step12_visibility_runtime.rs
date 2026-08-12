// SPDX-License-Identifier: GPL-3.0-or-later
//! Live, revisioned authority for a bounded Build-plus-Unit portion of step-12 visibility
//! production.
//!
//! This owner joins the exact type, leader-count, HeroesData, Constants, projection, and
//! instance-detector facts required by [`super::step12_visibility_producer_frontier`].  It is
//! extended with canonical Build rows in exact retail Build-before-Unit order, including the
//! complete `Wall::update_local_seen` footprint body. The active cohort requires an empty
//! dedicated Wall band, disabled scenario reveal points, and the PE-proven no-effect branch of
//! every reached `World::reveal_fog`. Other active maps still stop before `World::clear_seen`.

use crate::systems::sparse_object_bands_authority_frontier::{
    RetailBand, RetailObjectAddress, SparseSlotLifecycle, BUILD_BAND_BASE, WALL_BAND_BASE,
};
use crate::world::{Handle, World, WorldObjectIdentity};

use super::borders_fog::{self, CircleTable, Fog, SeeingObject};
use super::items::WFLAG_ITEM;
use super::map_terrain::{self, tflag, wflag};
use super::production::{
    self,
    runtime::{LiveProductionRuntime, LiveTypeClass},
    BuildData,
};
use super::step12_visibility_producer_frontier::{
    object_init_flags, prepare_live_unit_pass, resolve_unit_los, DetectorInstanceProvenance,
    LiveStep12Preparation, LiveStep12PrepareFault, LiveStep12UnitBandSnapshot,
    LiveStep12UnitIdentity, LiveStep12UnitState, PreparedStep12UnitPass, SmallLosProjectionReceipt,
    Step12UnitAuthorityReceipt, Step12VisibilityAuthorityReceipt, Step12VisibilityCadence,
    Step12VisibilityTrigger, UnitLosFacts, UnitStampDecision, LEADER_SLOTS, OBJECT_DETECTOR,
    OBJECT_VALID, PTOLEMY_NUM_UNITS_INDEX, PTOLEMY_ROLE_MASK, SMALL_LOS_PROJECT_DISTANCE,
    SMALL_LOS_STANDARD_TYPE_FLAGS2, SMALL_LOS_STANDARD_UNIT_MASKS, THE_CEO_NUM_UNITS_INDEX,
};
use super::victory_score;

pub const UNIT_TYPE_BASE: i32 = 0x32;
pub const UNIT_COUNT_SLOTS: usize = 352;
pub const HERO_RECORD_ACTIVE: u8 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VisibilityConstants {
    pub ptolemy_los_bonus: i32,
    pub the_ceo_unit_los: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VisibilityTypeProjection {
    pub type_index: i32,
    pub object_masks: u32,
    pub domain: i32,
    pub unit_flags2: u32,
    pub role: u32,
    pub is_siege: bool,
    /// `ObjectTypeData +0x234`, used by the exact `Unit::update_local_seen` disc.
    pub local_seen_radius: i32,
}

/// Identity-bearing row of the owner-local `HeroesData` list. `radius_tiles` is the exact
/// result of `HeroData::get_radius` for this record at the authority revision; retaining the
/// registry row (rather than a pre-combined boolean) preserves order and missing-object faults.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VisibilityHeroRecord {
    pub slot: u16,
    pub handle: Handle,
    pub uid: u16,
    pub o: i16,
    pub who: i8,
    pub hero_flags: u8,
    pub type_index: i32,
    pub radius_tiles: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VisibilityLeaderAuthority {
    pub unit_counts: [u16; UNIT_COUNT_SLOTS],
    pub heroes: Vec<VisibilityHeroRecord>,
}

impl Default for VisibilityLeaderAuthority {
    fn default() -> Self {
        Self {
            unit_counts: [0; UNIT_COUNT_SLOTS],
            heroes: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VisibilityDetectorAuthority {
    ObjectInitMasks(u32),
    AuthoritativeInstanceFlags(u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VisibilityInstanceAuthority {
    pub handle: Handle,
    pub uid: u16,
    pub detector: VisibilityDetectorAuthority,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step12VisibilityAuthority {
    state_revision: u64,
    type_revision: u64,
    composition_digest: u64,
    constants: Option<VisibilityConstants>,
    types: Vec<VisibilityTypeProjection>,
    leaders: [VisibilityLeaderAuthority; LEADER_SLOTS],
    instances: Vec<VisibilityInstanceAuthority>,
}

impl Default for Step12VisibilityAuthority {
    fn default() -> Self {
        Self {
            state_revision: 0,
            type_revision: 0,
            composition_digest: 0,
            constants: None,
            types: Vec::new(),
            leaders: std::array::from_fn(|_| VisibilityLeaderAuthority::default()),
            instances: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VisibilityAuthorityInstallError {
    InvalidOwner(usize),
    DuplicateType(i32),
    HeroSlotsOutOfOrder {
        who: usize,
        previous: u16,
        current: u16,
    },
    HeroOwnerOutOfRange {
        who: i8,
    },
    NegativeHeroRadius {
        who: usize,
        slot: u16,
        radius: i32,
    },
    DuplicateInstance(Handle),
    MissingUnit(Handle),
    UnitSideStoreLength {
        live: usize,
        type_rows: usize,
    },
    MissingVisibilityType(i32),
    ObjectInitAlreadyPassed {
        flags: u8,
    },
    InvalidUnitOwner(u8),
    InvalidUnitType(i32),
    UnitNotTracked(Handle),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VisibilityAuthorityFault {
    UnitSideStoreLength {
        live: usize,
        type_rows: usize,
    },
    RegistryRowOutOfRange {
        who: usize,
        object_o: usize,
        row: u32,
    },
    MissingSparseSlot {
        who: usize,
        object_o: i32,
    },
    ReservedSparseSlot {
        who: usize,
        object_o: i32,
    },
    WrongSparseUnitIdentity {
        who: usize,
        object_o: i32,
    },
    RegistryIdentityMismatch {
        row: usize,
        expected_who: u8,
        expected_o: i16,
        actual_who: u8,
        actual_o: i16,
    },
    UnitCountMismatch {
        who: u8,
        index: usize,
        authority: u16,
        live: u16,
    },
    MissingTypeFact {
        row: usize,
        type_index: i32,
        fact: &'static str,
    },
    MissingConstants {
        row: usize,
    },
    MissingDetectorInstance {
        row: usize,
        handle: Handle,
    },
    MissingLiveHandle {
        row: usize,
    },
    DetectorUidMismatch {
        row: usize,
        live: u16,
        authority: u16,
    },
    MissingHeroObject {
        who: u8,
        slot: u16,
        handle: Handle,
    },
    HeroIdentityMismatch {
        who: u8,
        slot: u16,
    },
    InvalidHeroRadius {
        who: u8,
        slot: u16,
        radius: i32,
    },
    Los {
        row: usize,
    },
    Frontier(LiveStep12PrepareFault),
}

/// The reasons a prepared Unit pass is still not permission to clear the planes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Step12ProducerResiduals {
    pub build_and_wall_bands: bool,
    pub started_wonder_local_seen: bool,
    pub reveal_fog: bool,
    pub scenario_reveal_points: bool,
    pub frame_zero_explored_sharing: bool,
    pub direct_entry_routing: bool,
    pub incremental_update_seen: bool,
}

impl Step12ProducerResiduals {
    pub const MISSING: Self = Self {
        build_and_wall_bands: true,
        started_wonder_local_seen: true,
        reveal_fog: true,
        scenario_reveal_points: true,
        frame_zero_explored_sharing: true,
        direct_entry_routing: true,
        incremental_update_seen: true,
    };
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step12VisibilityPreflightError {
    Authority(VisibilityAuthorityFault),
    ActiveUnitCohort(ActiveUnitProducerFault),
    IncompleteProducer {
        prepared_unit_stamps: usize,
        residuals: Step12ProducerResiduals,
    },
}

#[derive(Clone, Copy, Debug)]
pub struct ScheduledActiveProducerContext<'a> {
    pub terrain: &'a map_terrain::World,
    pub circle: &'a CircleTable,
    pub builds: &'a [BuildData],
    pub production: &'a LiveProductionRuntime,
    pub scenario_reveal_points_enabled: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RevealFogNoEffectBlocker {
    ResourceTile,
    OilCell,
    ItemCell,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActiveUnitProducerFault {
    MissingScheduledContext,
    BuildMarkBeforeBase {
        who: usize,
        mark: i32,
    },
    MissingBuildSlot {
        who: usize,
        object_o: i32,
    },
    BuildSlotNotLive {
        who: usize,
        object_o: i32,
    },
    WrongBuildSlotIdentity {
        who: usize,
        object_o: i32,
    },
    BuildRowOutOfRange {
        who: usize,
        object_o: i32,
        row: u32,
    },
    BuildIdentityMismatch {
        row: usize,
        expected_who: u8,
        expected_o: i16,
        actual_who: u8,
        actual_o: i16,
    },
    MissingBuildType {
        row: usize,
        who: u8,
        object_o: i16,
    },
    InvalidBuildType {
        row: usize,
        type_index: i32,
    },
    BuildLocalSeenNeedsFootprintAuthority {
        row: usize,
        type_index: i32,
    },
    BuildWonderNeedsFortAuthority {
        row: usize,
        type_index: i32,
    },
    InvalidBuildLocalSeenFootprint {
        row: usize,
        x_size: i32,
        y_size: i32,
    },
    NegativeBuildLos {
        row: usize,
        los: i8,
    },
    InvalidBuildGrantSeen2 {
        row: usize,
        recipient: i8,
    },
    NonEmptyWallBand {
        who: usize,
        mark: i32,
    },
    ScenarioRevealPointsEnabled,
    FrameZeroExploredSharing,
    InvalidGrantSeen2 {
        row: usize,
        recipient: i8,
    },
    RevealFogEffectful {
        fog_x: i32,
        fog_y: i32,
        blocker: RevealFogNoEffectBlocker,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedStep12ActiveUnitClear {
    build_rows_visited: usize,
    build_actions: Vec<PreparedStep12BuildAction>,
    unit_pass: PreparedStep12UnitPass,
    unit_local_seen: Vec<Vec<PreparedStep12LocalSeen>>,
    expected_reveal_calls: usize,
}

impl PreparedStep12ActiveUnitClear {
    pub const fn expected_reveal_calls(&self) -> usize {
        self.expected_reveal_calls
    }

    pub const fn unit_stamps(&self) -> usize {
        self.unit_pass.stamps()
    }

    pub const fn build_rows_visited(&self) -> usize {
        self.build_rows_visited
    }

    pub fn build_stamps(&self) -> usize {
        self.build_actions
            .iter()
            .filter(|action| matches!(action, PreparedStep12BuildAction::UpdateSeen(_)))
            .count()
    }

    pub fn build_local_seen_cells(&self) -> usize {
        self.build_actions
            .iter()
            .map(|action| match action {
                PreparedStep12BuildAction::UpdateSeen(_) => 0,
                PreparedStep12BuildAction::UpdateLocalSeen(cells) => cells.len(),
            })
            .sum()
    }

    pub fn unit_local_seen_cells(&self) -> usize {
        self.unit_local_seen.iter().map(Vec::len).sum()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum PreparedStep12BuildAction {
    UpdateSeen(PreparedStep12BuildStamp),
    UpdateLocalSeen(Vec<PreparedStep12LocalSeen>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PreparedStep12LocalSeen {
    fog_x: i32,
    fog_y: i32,
    player_mask: u8,
    explored_only: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PreparedStep12BuildStamp {
    row: usize,
    object_o: i16,
    fine_x: i32,
    fine_y: i32,
    who: u8,
    los_tiles: i32,
    detector: bool,
    grant_seen2_to: i8,
}

impl PreparedStep12BuildStamp {
    fn seeing_object(self) -> SeeingObject {
        SeeingObject {
            fine_x: self.fine_x,
            fine_y: self.fine_y,
            owner: self.who,
            los_tiles: self.los_tiles,
            detector: self.detector,
            grant_seen2_to: self.grant_seen2_to,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActiveUnitClearTrace {
    pub rows_visited: usize,
    pub build_rows_visited: usize,
    pub build_stamps: usize,
    pub build_local_seen_cells: usize,
    pub unit_local_seen_cells: usize,
    pub unit_stamps: usize,
    pub reveal_no_effect_calls: usize,
}

fn resolve_build_type<'a>(
    row: usize,
    build: &BuildData,
    production: &'a LiveProductionRuntime,
) -> Result<(i32, &'a super::production::runtime::LiveProductionType), ActiveUnitProducerFault> {
    let type_index = production.build_types.get(row).copied().flatten().ok_or(
        ActiveUnitProducerFault::MissingBuildType {
            row,
            who: build.who,
            object_o: build.object_id(),
        },
    )?;
    let type_facts = usize::try_from(type_index)
        .ok()
        .and_then(|index| production.types.get(index))
        .and_then(Option::as_ref)
        .filter(|facts| facts.type_index == type_index && facts.class == LiveTypeClass::Building)
        .ok_or(ActiveUnitProducerFault::InvalidBuildType { row, type_index })?;
    Ok((type_index, type_facts))
}

fn build_implicit_local_seen(
    row: usize,
    build: &BuildData,
    type_index: i32,
    type_facts: &super::production::runtime::LiveProductionType,
) -> Result<bool, ActiveUnitProducerFault> {
    let is_wonder = (victory_score::WONDER_FIRST as i32..victory_score::WONDER_END as i32)
        .contains(&type_index);
    // Exact Object::update_seen 0x00651B8A..0x00651C3F. A non-Wonder or captured
    // Build returns before type-vtable +0xFC. Only an uncaptured Wonder reads is_fort,
    // and only a non-fort started Wonder sets the implicit local-seen flag.
    if !is_wonder || build.flags & production::flag::CAPTURED != 0 {
        return Ok(false);
    }
    let is_fort = type_facts
        .build_visibility
        .and_then(|facts| facts.is_fort)
        .ok_or(ActiveUnitProducerFault::BuildWonderNeedsFortAuthority { row, type_index })?;
    Ok(!is_fort && build.is_started())
}

fn prepare_wall_local_seen(
    row: usize,
    build: &BuildData,
    type_index: i32,
    type_facts: &super::production::runtime::LiveProductionType,
    terrain: &map_terrain::World,
) -> Result<Vec<PreparedStep12LocalSeen>, ActiveUnitProducerFault> {
    let visibility = type_facts.build_visibility;
    let is_wonder = (victory_score::WONDER_FIRST as i32..victory_score::WONDER_END as i32)
        .contains(&type_index);
    let mut player_mask =
        build.other[production::off::VISIBLE] | build.ever_seen | (1u8 << build.who);
    let mut explored_only = true;

    // Exact Wall::update_local_seen 0x0063ED62..0x0063EDCA gate order. An ordinary
    // Build returns before captured/is_fort. A captured Wonder returns before is_fort.
    // An uncaptured non-fort started Wonder uses 0xFF visible+explored; an unstarted
    // one retains the relation mask but still writes both planes.
    if is_wonder && build.flags & production::flag::CAPTURED == 0 {
        let is_fort = visibility
            .and_then(|facts| facts.is_fort)
            .ok_or(ActiveUnitProducerFault::BuildWonderNeedsFortAuthority { row, type_index })?;
        if !is_fort {
            explored_only = false;
            if build.is_started() {
                player_mask = u8::MAX;
            }
        }
    }
    let footprint = visibility.and_then(|facts| facts.footprint).ok_or(
        ActiveUnitProducerFault::BuildLocalSeenNeedsFootprintAuthority { row, type_index },
    )?;
    if footprint.x_size <= 0
        || footprint.y_size <= 0
        || footprint.x_size > terrain.tile_xs
        || footprint.y_size > terrain.tile_ys
    {
        return Err(ActiveUnitProducerFault::InvalidBuildLocalSeenFootprint {
            row,
            x_size: footprint.x_size,
            y_size: footprint.y_size,
        });
    }
    let (fine_x, fine_y) = build.position();
    let (corner_x, corner_y) = footprint.tile_corner(fine_x, fine_y);
    let (Some(start_x), Some(start_y), Some(end_x), Some(end_y), Some(span_x), Some(span_y)) = (
        corner_x.checked_sub(1),
        corner_y.checked_sub(1),
        corner_x.checked_add(footprint.x_size),
        corner_y.checked_add(footprint.y_size),
        footprint.x_size.checked_add(2),
        footprint.y_size.checked_add(2),
    ) else {
        return Err(ActiveUnitProducerFault::InvalidBuildLocalSeenFootprint {
            row,
            x_size: footprint.x_size,
            y_size: footprint.y_size,
        });
    };
    let mut prepared = Vec::with_capacity(
        usize::try_from(span_x).expect("positive validated Build local-seen x span")
            * usize::try_from(span_y).expect("positive validated Build local-seen y span"),
    );
    // Exact 0x0063EDDE..0x0063EE92: both counters start at -1 and run while
    // `<= x_size/y_size`, revealing a one-tile perimeter around the footprint.
    // Edge cells are tested and skipped rather than making the call fail.
    for tile_x in start_x..=end_x {
        for tile_y in start_y..=end_y {
            if terrain.valid_t(tile_x, tile_y) {
                prepared.push(PreparedStep12LocalSeen {
                    fog_x: tile_x >> 1,
                    fog_y: tile_y >> 1,
                    player_mask,
                    explored_only,
                });
            }
        }
    }
    Ok(prepared)
}

fn prepare_unit_local_seen(
    stamp: super::step12_visibility_producer_frontier::Step12UnitStamp,
    visible: i8,
    radius: i32,
    terrain: &map_terrain::World,
    circle: &CircleTable,
) -> Vec<PreparedStep12LocalSeen> {
    debug_assert!(
        (0..=super::step12_visibility_producer_frontier::MAX_FOG_RADIUS).contains(&radius)
    );
    let center_x = super::step12_visibility_producer_frontier::fine_to_fog(stamp.object_fine_x);
    let center_y = super::step12_visibility_producer_frontier::fine_to_fog(stamp.object_fine_y);
    let end = circle.radius[radius as usize] as usize;
    let mut prepared = Vec::with_capacity(end);
    for index in 0..end {
        let fog_x = center_x + i32::from(circle.x[index]);
        let fog_y = center_y + i32::from(circle.y[index]);
        if terrain.valid_f(fog_x, fog_y) {
            prepared.push(PreparedStep12LocalSeen {
                fog_x,
                fog_y,
                player_mask: visible as u8,
                explored_only: false,
            });
        }
    }
    prepared
}

/// The complete `GameDaemon::update_all_seen` paths that the live Sim can currently
/// authorize before the step-12 shell mutates anything.
///
/// `InactiveLeadersClear` is not a claim about the active-object producer.  In the retail PE,
/// every Build/Wall, Unit, scenario-point, and frame-zero alliance loop begins with the same
/// `LeaderData::flags & 1` gate.  With all eight gates clear, the whole reached body is exactly
/// `busy = 4`, `World::clear_seen`, and the `seen3` clear.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PreparedStep12FullProducer {
    NoMutation(Step12VisibilityCadence),
    InactiveLeadersClear,
    ActiveUnitClear(PreparedStep12ActiveUnitClear),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LiveJoinRow {
    state: LiveStep12UnitState,
    handle: Option<Handle>,
}

fn reveal_fog_no_effect(
    terrain: &map_terrain::World,
    fog_x: i32,
    fog_y: i32,
) -> Result<(), ActiveUnitProducerFault> {
    let tile_x = fog_x * 2 + 1;
    let tile_y = fog_y * 2 + 1;
    if terrain.tdata[terrain.t_index(tile_x, tile_y)] & tflag::RESOURCE != 0 {
        return Err(ActiveUnitProducerFault::RevealFogEffectful {
            fog_x,
            fog_y,
            blocker: RevealFogNoEffectBlocker::ResourceTile,
        });
    }
    let cell = &terrain.wdata[terrain.w_index(fog_x >> 1, fog_y >> 1)];
    if cell.flags & wflag::OIL != 0 {
        return Err(ActiveUnitProducerFault::RevealFogEffectful {
            fog_x,
            fog_y,
            blocker: RevealFogNoEffectBlocker::OilCell,
        });
    }
    if cell.flags & WFLAG_ITEM != 0 {
        return Err(ActiveUnitProducerFault::RevealFogEffectful {
            fog_x,
            fog_y,
            blocker: RevealFogNoEffectBlocker::ItemCell,
        });
    }
    Ok(())
}

fn preflight_seeing_cells(
    seeing: SeeingObject,
    context: ScheduledActiveProducerContext<'_>,
    explored: &mut [u8],
) -> Result<usize, ActiveUnitProducerFault> {
    let radius = borders_fog::los_to_fog_radius(seeing.los_tiles);
    let end = context.circle.radius[radius as usize] as usize;
    let fog_x = super::step12_visibility_producer_frontier::fine_to_fog(seeing.fine_x);
    let fog_y = super::step12_visibility_producer_frontier::fine_to_fog(seeing.fine_y);
    let fully_inside = context.terrain.valid_f(fog_x + radius, fog_y + radius)
        && context.terrain.valid_f(fog_x - radius, fog_y - radius);
    let owner_mask = 1u8 << seeing.owner;
    let extra_mask = if seeing.grant_seen2_to == 0 {
        0
    } else {
        1u8 << seeing.grant_seen2_to
    };
    let mut reveal_calls = 0usize;
    for index in 0..end {
        let cell_x = fog_x + i32::from(context.circle.x[index]);
        let cell_y = fog_y + i32::from(context.circle.y[index]);
        if !fully_inside && !context.terrain.valid_f(cell_x, cell_y) {
            continue;
        }
        let plane_index = context.terrain.f_index(cell_x, cell_y);
        if explored[plane_index] & owner_mask == 0 {
            reveal_fog_no_effect(context.terrain, cell_x, cell_y)?;
            reveal_calls += 1;
        }
        explored[plane_index] |= owner_mask | extra_mask;
    }
    Ok(reveal_calls)
}

fn prepare_active_unit_clear(
    pass: PreparedStep12UnitPass,
    world: &World,
    leader_active: [bool; LEADER_SLOTS],
    context: ScheduledActiveProducerContext<'_>,
) -> Result<PreparedStep12ActiveUnitClear, ActiveUnitProducerFault> {
    if pass.frame() == 0 {
        return Err(ActiveUnitProducerFault::FrameZeroExploredSharing);
    }
    if context.scenario_reveal_points_enabled {
        return Err(ActiveUnitProducerFault::ScenarioRevealPointsEnabled);
    }
    let mut explored = context.terrain.seen2.clone();
    let mut expected_reveal_calls = 0usize;
    let mut build_rows_visited = 0usize;
    let mut build_actions = Vec::new();
    for who in 0..LEADER_SLOTS {
        if !leader_active[who] {
            continue;
        }
        let build_mark = world
            .object_bands()
            .mark(who, RetailBand::Build)
            .expect("active step-12 owner is inside the canonical sparse registry");
        if build_mark < BUILD_BAND_BASE {
            return Err(ActiveUnitProducerFault::BuildMarkBeforeBase {
                who,
                mark: build_mark,
            });
        }
        let wall_mark = world
            .object_bands()
            .mark(who, RetailBand::Wall)
            .expect("active step-12 owner is inside the canonical sparse registry");
        if wall_mark != WALL_BAND_BASE {
            return Err(ActiveUnitProducerFault::NonEmptyWallBand {
                who,
                mark: wall_mark,
            });
        }

        for object_o in BUILD_BAND_BASE..build_mark {
            build_rows_visited += 1;
            let address = RetailObjectAddress::new(who as u8, RetailBand::Build, object_o);
            let slot = world
                .object_bands()
                .slot(address)
                .ok_or(ActiveUnitProducerFault::MissingBuildSlot { who, object_o })?;
            let SparseSlotLifecycle::Live(WorldObjectIdentity::BuildRow(row)) = slot.lifecycle
            else {
                return Err(match slot.lifecycle {
                    SparseSlotLifecycle::Live(_) => {
                        ActiveUnitProducerFault::WrongBuildSlotIdentity { who, object_o }
                    }
                    _ => ActiveUnitProducerFault::BuildSlotNotLive { who, object_o },
                });
            };
            let row = usize::try_from(row).expect("u32 Build row fits usize");
            let build =
                context
                    .builds
                    .get(row)
                    .ok_or(ActiveUnitProducerFault::BuildRowOutOfRange {
                        who,
                        object_o,
                        row: row as u32,
                    })?;
            let expected_o = i16::try_from(object_o).expect("Build band is inside i16");
            if build.who != who as u8 || build.object_id() != expected_o {
                return Err(ActiveUnitProducerFault::BuildIdentityMismatch {
                    row,
                    expected_who: who as u8,
                    expected_o,
                    actual_who: build.who,
                    actual_o: build.object_id(),
                });
            }

            // `is_valid_wall` is the first virtual at 0x00732907. Invalid retained rows
            // then fail `is_valid_build` too, so neither the Wonder special case nor a
            // visibility virtual is reached.
            if !build.is_valid() {
                continue;
            }
            if !build.is_active() {
                // Exact 0x00732938..0x00732992: valid incomplete buildings query
                // `BuildData::is_wonder`; ordinary incomplete buildings then skip with no
                // visibility effect. Only a started Wonder reaches
                // `Wall::update_local_seen`.
                let (type_index, type_facts) = resolve_build_type(row, build, context.production)?;
                if !(victory_score::WONDER_FIRST as i32..victory_score::WONDER_END as i32)
                    .contains(&type_index)
                    || !build.is_started()
                {
                    continue;
                }
                let cells =
                    prepare_wall_local_seen(row, build, type_index, type_facts, context.terrain)?;
                for cell in &cells {
                    explored[context.terrain.f_index(cell.fog_x, cell.fog_y)] |= cell.player_mask;
                }
                build_actions.push(PreparedStep12BuildAction::UpdateLocalSeen(cells));
                continue;
            }

            // Object::update_seen computes this type/Build preamble before its LOS
            // virtual. Even a zero-LOS active Build therefore requires current type
            // identity and the reached Wonder fort fact.
            let (type_index, type_facts) = resolve_build_type(row, build, context.production)?;
            let implicit_local_seen =
                build_implicit_local_seen(row, build, type_index, type_facts)?;
            let los = build.other[production::off::MYLOS] as i8;
            if los < 0 {
                return Err(ActiveUnitProducerFault::NegativeBuildLos { row, los });
            }
            if los == 0 {
                continue;
            }
            let visible = build.other[production::off::VISIBLE] as i8;
            if visible != 0 || implicit_local_seen {
                let cells =
                    prepare_wall_local_seen(row, build, type_index, type_facts, context.terrain)?;
                for cell in &cells {
                    explored[context.terrain.f_index(cell.fog_x, cell.fog_y)] |= cell.player_mask;
                }
                build_actions.push(PreparedStep12BuildAction::UpdateLocalSeen(cells));
            }
            let recipient = build.other[production::off::INFILTRATED] as i8;
            if !(0..=7).contains(&recipient) {
                return Err(ActiveUnitProducerFault::InvalidBuildGrantSeen2 { row, recipient });
            }
            let (fine_x, fine_y) = build.position();
            let stamp = PreparedStep12BuildStamp {
                row,
                object_o: expected_o,
                fine_x,
                fine_y,
                who: build.who,
                los_tiles: i32::from(los),
                detector: build.flags & OBJECT_DETECTOR != 0,
                grant_seen2_to: recipient,
            };
            expected_reveal_calls +=
                preflight_seeing_cells(stamp.seeing_object(), context, &mut explored)?;
            build_actions.push(PreparedStep12BuildAction::UpdateSeen(stamp));
        }
    }

    let mut unit_local_seen = Vec::with_capacity(pass.rows().len());
    for row in pass.rows() {
        let UnitStampDecision::Stamp(stamp) = row.decision() else {
            unit_local_seen.push(Vec::new());
            continue;
        };
        let local_cells = match row.local_seen_radius() {
            Some(radius) => prepare_unit_local_seen(
                stamp,
                row.visible(),
                radius,
                context.terrain,
                context.circle,
            ),
            None => Vec::new(),
        };
        for cell in &local_cells {
            explored[context.terrain.f_index(cell.fog_x, cell.fog_y)] |= cell.player_mask;
        }
        unit_local_seen.push(local_cells);

        if !(0..=7).contains(&stamp.grant_seen2_to) {
            return Err(ActiveUnitProducerFault::InvalidGrantSeen2 {
                row: row.identity().row,
                recipient: stamp.grant_seen2_to,
            });
        }
        expected_reveal_calls += preflight_seeing_cells(
            SeeingObject {
                fine_x: stamp.stamp_fine_x,
                fine_y: stamp.stamp_fine_y,
                owner: stamp.who,
                los_tiles: stamp.resolved_los_tiles,
                detector: stamp.detector,
                grant_seen2_to: stamp.grant_seen2_to,
            },
            context,
            &mut explored,
        )?;
    }
    Ok(PreparedStep12ActiveUnitClear {
        build_rows_visited,
        build_actions,
        unit_pass: pass,
        unit_local_seen,
        expected_reveal_calls,
    })
}

/// Commit the preflight-complete active Build-plus-Unit producer after the caller stores
/// `GameDaemon::busy = 4`. Every reached `World::reveal_fog` was proven to take its no-effect
/// branch, so the exact call is represented by the trace rather than a skipped mutation.
pub fn commit_active_unit_clear(
    prepared: PreparedStep12ActiveUnitClear,
    fog: &Fog,
    terrain: &mut map_terrain::World,
    circle: &CircleTable,
) -> ActiveUnitClearTrace {
    fog.begin_frame(terrain);
    let mut newly_explored = Vec::new();
    for action in &prepared.build_actions {
        match action {
            PreparedStep12BuildAction::UpdateSeen(stamp) => borders_fog::update_seen(
                fog,
                terrain,
                circle,
                &stamp.seeing_object(),
                &mut newly_explored,
            ),
            PreparedStep12BuildAction::UpdateLocalSeen(cells) => {
                for cell in cells {
                    fog.set_locally_seen_mask(
                        terrain,
                        cell.fog_x,
                        cell.fog_y,
                        cell.player_mask,
                        cell.explored_only,
                    );
                }
            }
        }
    }
    let rows_visited = prepared.build_rows_visited + prepared.unit_pass.rows().len();
    assert_eq!(
        prepared.unit_local_seen.len(),
        prepared.unit_pass.rows().len(),
        "prepared Unit local-seen rows drifted before visibility commit"
    );
    for (row, local_cells) in prepared
        .unit_pass
        .rows()
        .iter()
        .zip(&prepared.unit_local_seen)
    {
        let UnitStampDecision::Stamp(stamp) = row.decision() else {
            continue;
        };
        for cell in local_cells {
            fog.set_locally_seen_mask(
                terrain,
                cell.fog_x,
                cell.fog_y,
                cell.player_mask,
                cell.explored_only,
            );
        }
        borders_fog::update_seen(
            fog,
            terrain,
            circle,
            &SeeingObject {
                fine_x: stamp.stamp_fine_x,
                fine_y: stamp.stamp_fine_y,
                owner: stamp.who,
                los_tiles: stamp.resolved_los_tiles,
                detector: stamp.detector,
                grant_seen2_to: stamp.grant_seen2_to,
            },
            &mut newly_explored,
        );
    }
    assert_eq!(
        newly_explored.len(),
        prepared.expected_reveal_calls,
        "prepared reveal_fog chronology drifted before visibility commit"
    );
    ActiveUnitClearTrace {
        rows_visited,
        build_rows_visited: prepared.build_rows_visited,
        build_stamps: prepared.build_stamps(),
        build_local_seen_cells: prepared.build_local_seen_cells(),
        unit_local_seen_cells: prepared.unit_local_seen_cells(),
        unit_stamps: prepared.unit_pass.stamps(),
        reveal_no_effect_calls: newly_explored.len(),
    }
}

impl Step12VisibilityAuthority {
    pub const fn state_revision(&self) -> u64 {
        self.state_revision
    }

    pub const fn type_revision(&self) -> u64 {
        self.type_revision
    }

    pub const fn composition_digest(&self) -> u64 {
        self.composition_digest
    }

    pub const fn constants(&self) -> Option<VisibilityConstants> {
        self.constants
    }

    pub fn types(&self) -> &[VisibilityTypeProjection] {
        &self.types
    }

    pub fn leaders(&self) -> &[VisibilityLeaderAuthority; LEADER_SLOTS] {
        &self.leaders
    }

    pub fn instances(&self) -> &[VisibilityInstanceAuthority] {
        &self.instances
    }

    fn bump_state_revision(&mut self) {
        self.state_revision = self.state_revision.wrapping_add(1);
    }

    /// Replace the immutable visibility projection from one synchronized rules/mod
    /// composition. Rows are sorted here, making lookup, persistence, and digest order stable.
    pub fn replace_type_source(
        &mut self,
        type_revision: u64,
        composition_digest: u64,
        constants: VisibilityConstants,
        mut types: Vec<VisibilityTypeProjection>,
    ) -> Result<(), VisibilityAuthorityInstallError> {
        types.sort_by_key(|facts| facts.type_index);
        for pair in types.windows(2) {
            if pair[0].type_index == pair[1].type_index {
                return Err(VisibilityAuthorityInstallError::DuplicateType(
                    pair[0].type_index,
                ));
            }
        }
        self.type_revision = type_revision;
        self.composition_digest = composition_digest;
        self.constants = Some(constants);
        self.types = types;
        self.bump_state_revision();
        Ok(())
    }

    /// Replace one complete leader count table and ordered HeroesData registry atomically.
    pub fn replace_leader(
        &mut self,
        who: usize,
        leader: VisibilityLeaderAuthority,
    ) -> Result<(), VisibilityAuthorityInstallError> {
        if who >= LEADER_SLOTS {
            return Err(VisibilityAuthorityInstallError::InvalidOwner(who));
        }
        validate_hero_records(who, &leader.heroes)?;
        self.leaders[who] = leader;
        self.bump_state_revision();
        Ok(())
    }

    fn type_projection(&self, type_index: i32) -> Option<VisibilityTypeProjection> {
        self.types
            .binary_search_by_key(&type_index, |facts| facts.type_index)
            .ok()
            .map(|index| self.types[index])
    }

    fn instance(&self, handle: Handle) -> Option<VisibilityInstanceAuthority> {
        self.instances
            .binary_search_by_key(&(handle.id, handle.generation), |entry| {
                (entry.handle.id, entry.handle.generation)
            })
            .ok()
            .map(|index| self.instances[index])
    }

    /// Exact `Object::init` detector suffix for a freshly spawned Sim unit. The method refuses
    /// a byte that has already moved past `SubObject::init`'s `OBJECT_VALID` state, so it cannot
    /// erase legitimate later instance mutations. On success it also advances the exact
    /// owner-local unit count and retains the initialization mask decision.
    pub fn materialize_object_init(
        &mut self,
        world: &mut World,
        unit_types: &[i32],
        handle: Handle,
    ) -> Result<usize, VisibilityAuthorityInstallError> {
        let live = world.live_count() as usize;
        if unit_types.len() != live {
            return Err(VisibilityAuthorityInstallError::UnitSideStoreLength {
                live,
                type_rows: unit_types.len(),
            });
        }
        let row = world
            .row_of(handle)
            .ok_or(VisibilityAuthorityInstallError::MissingUnit(handle))?;
        let flags = world.units.get_flags(row);
        if flags != OBJECT_VALID {
            return Err(VisibilityAuthorityInstallError::ObjectInitAlreadyPassed { flags });
        }
        if self.instance(handle).is_some() {
            return Err(VisibilityAuthorityInstallError::DuplicateInstance(handle));
        }
        let type_index = unit_types[row];
        let facts = self.type_projection(type_index).ok_or(
            VisibilityAuthorityInstallError::MissingVisibilityType(type_index),
        )?;
        let who = world.units.get_who(row);
        if usize::from(who) >= LEADER_SLOTS {
            return Err(VisibilityAuthorityInstallError::InvalidUnitOwner(who));
        }
        let Some(count_index) = type_count_index(type_index) else {
            return Err(VisibilityAuthorityInstallError::InvalidUnitType(type_index));
        };

        world
            .units
            .set_flags(row, object_init_flags(facts.object_masks));
        self.leaders[usize::from(who)].unit_counts[count_index] =
            self.leaders[usize::from(who)].unit_counts[count_index].wrapping_add(1);
        self.instances.push(VisibilityInstanceAuthority {
            handle,
            uid: world.units.get_uid(row),
            detector: VisibilityDetectorAuthority::ObjectInitMasks(facts.object_masks),
        });
        self.instances
            .sort_by_key(|entry| (entry.handle.id, entry.handle.generation));
        self.bump_state_revision();
        Ok(row)
    }

    /// Remove the authority facts for a Unit immediately before its live object is despawned.
    /// The caller still owns the surrounding despawn transaction; failure leaves this owner
    /// unchanged.
    pub fn retire_unit(
        &mut self,
        world: &World,
        unit_types: &[i32],
        handle: Handle,
    ) -> Result<usize, VisibilityAuthorityInstallError> {
        let live = world.live_count() as usize;
        if unit_types.len() != live {
            return Err(VisibilityAuthorityInstallError::UnitSideStoreLength {
                live,
                type_rows: unit_types.len(),
            });
        }
        let row = world
            .row_of(handle)
            .ok_or(VisibilityAuthorityInstallError::MissingUnit(handle))?;
        let position = self
            .instances
            .binary_search_by_key(&(handle.id, handle.generation), |entry| {
                (entry.handle.id, entry.handle.generation)
            })
            .map_err(|_| VisibilityAuthorityInstallError::UnitNotTracked(handle))?;
        let who = world.units.get_who(row);
        if usize::from(who) >= LEADER_SLOTS {
            return Err(VisibilityAuthorityInstallError::InvalidUnitOwner(who));
        }
        let type_index = unit_types[row];
        let Some(count_index) = type_count_index(type_index) else {
            return Err(VisibilityAuthorityInstallError::InvalidUnitType(type_index));
        };
        self.instances.remove(position);
        self.leaders[usize::from(who)].unit_counts[count_index] =
            self.leaders[usize::from(who)].unit_counts[count_index].wrapping_sub(1);
        self.bump_state_revision();
        Ok(row)
    }

    /// Record an exact loaded or explicitly mutated instance byte. Unlike object-init
    /// provenance this does not consult the current type mask.
    pub fn record_authoritative_instance(
        &mut self,
        world: &World,
        handle: Handle,
    ) -> Result<usize, VisibilityAuthorityInstallError> {
        let row = world
            .row_of(handle)
            .ok_or(VisibilityAuthorityInstallError::MissingUnit(handle))?;
        let entry = VisibilityInstanceAuthority {
            handle,
            uid: world.units.get_uid(row),
            detector: VisibilityDetectorAuthority::AuthoritativeInstanceFlags(
                world.units.get_flags(row),
            ),
        };
        match self
            .instances
            .binary_search_by_key(&(handle.id, handle.generation), |candidate| {
                (candidate.handle.id, candidate.handle.generation)
            }) {
            Ok(position) => self.instances[position] = entry,
            Err(position) => self.instances.insert(position, entry),
        }
        self.bump_state_revision();
        Ok(row)
    }

    /// Prepare every reached canonical Unit row without mutating a fog plane.
    pub fn prepare_unit_pass(
        &self,
        world: &World,
        unit_types: &[i32],
        leader_active: [bool; LEADER_SLOTS],
        fog_option: u8,
        trigger: Step12VisibilityTrigger,
    ) -> Result<LiveStep12Preparation, VisibilityAuthorityFault> {
        let live = world.live_count() as usize;
        if unit_types.len() != live {
            return Err(VisibilityAuthorityFault::UnitSideStoreLength {
                live,
                type_rows: unit_types.len(),
            });
        }

        // Preserve the producer's lazy entry: malformed registry/type authority is not read on
        // unscheduled frames or when fog option three suppresses the body.
        let cadence = super::step12_visibility_producer_frontier::visibility_entry(
            world.frame,
            fog_option,
            trigger,
        );
        if cadence != Step12VisibilityCadence::FullRefresh {
            let empty = LiveStep12UnitBandSnapshot {
                frame: world.frame,
                state_revision: self.state_revision,
                type_revision: self.type_revision,
                leader_active,
                owner_band_lengths: [0; LEADER_SLOTS],
                rows: &[],
            };
            return prepare_live_unit_pass(fog_option, trigger, empty, None)
                .map_err(VisibilityAuthorityFault::Frontier);
        }

        let mut joined = Vec::new();
        let mut owner_band_lengths = [0usize; LEADER_SLOTS];
        for who in 0..LEADER_SLOTS {
            if !leader_active[who] {
                continue;
            }
            let mark = world
                .unit_mark(who)
                .ok_or(VisibilityAuthorityFault::MissingSparseSlot { who, object_o: 0 })?;
            let length =
                usize::try_from(mark).map_err(|_| VisibilityAuthorityFault::MissingSparseSlot {
                    who,
                    object_o: mark,
                })?;
            owner_band_lengths[who] = length;
            for object_o in 0..mark {
                let address = RetailObjectAddress::new(who as u8, RetailBand::Unit, object_o);
                let slot = world
                    .object_bands()
                    .slot(address)
                    .ok_or(VisibilityAuthorityFault::MissingSparseSlot { who, object_o })?;
                match slot.lifecycle {
                    SparseSlotLifecycle::Live(WorldObjectIdentity::Unit { id, generation }) => {
                        let handle = Handle { id, generation };
                        let row = world.row_of(handle).ok_or(
                            VisibilityAuthorityFault::RegistryRowOutOfRange {
                                who,
                                object_o: object_o as usize,
                                row: u32::MAX,
                            },
                        )?;
                        if row >= live {
                            return Err(VisibilityAuthorityFault::RegistryRowOutOfRange {
                                who,
                                object_o: object_o as usize,
                                row: row as u32,
                            });
                        }
                        let expected_o = i16::try_from(object_o).map_err(|_| {
                            VisibilityAuthorityFault::RegistryRowOutOfRange {
                                who,
                                object_o: object_o as usize,
                                row: row as u32,
                            }
                        })?;
                        let actual_who = world.units.get_who(row);
                        let actual_o = world.units.o()[row];
                        if actual_who != who as u8 || actual_o != expected_o {
                            return Err(VisibilityAuthorityFault::RegistryIdentityMismatch {
                                row,
                                expected_who: who as u8,
                                expected_o,
                                actual_who,
                                actual_o,
                            });
                        }
                        joined.push(LiveJoinRow {
                            handle: Some(handle),
                            state: LiveStep12UnitState {
                                identity: LiveStep12UnitIdentity {
                                    row,
                                    who: actual_who,
                                    object_o: actual_o,
                                    uid: world.units.get_uid(row),
                                    type_index: unit_types[row],
                                },
                                object_flags: world.units.get_flags(row),
                                inside_up: world.units.inside_up()[row],
                                fine_x: world.units.x_internal()[row],
                                fine_y: world.units.y_internal()[row],
                                unit_angle: world.units.angle()[row],
                                mylos: world.units.mylos()[row],
                                visible: world.units.visible()[row],
                                unit_masks: world.units.get_unit_masks(row),
                                infiltrated: world.units.infiltrated()[row],
                            },
                        });
                    }
                    SparseSlotLifecycle::Live(_) => {
                        return Err(VisibilityAuthorityFault::WrongSparseUnitIdentity {
                            who,
                            object_o,
                        });
                    }
                    SparseSlotLifecycle::Tombstone(facts) => {
                        // Retail still visits retained Unit storage below `mark`; the invalid
                        // flags gate stops before any other object/type fact is read.
                        joined.push(LiveJoinRow {
                            handle: None,
                            state: LiveStep12UnitState {
                                identity: LiveStep12UnitIdentity {
                                    row: live + joined.len(),
                                    who: who as u8,
                                    object_o: object_o as i16,
                                    uid: 0,
                                    type_index: 0,
                                },
                                object_flags: facts.flags,
                                inside_up: 0,
                                fine_x: 0,
                                fine_y: 0,
                                unit_angle: 0,
                                mylos: 0,
                                visible: 0,
                                unit_masks: 0,
                                infiltrated: 0,
                            },
                        });
                    }
                    SparseSlotLifecycle::Reserved { .. } => {
                        return Err(VisibilityAuthorityFault::ReservedSparseSlot { who, object_o });
                    }
                }
            }
        }

        self.validate_unit_counts(&joined, leader_active)?;
        let states: Vec<_> = joined.iter().map(|row| row.state).collect();
        let mut authority_rows = Vec::with_capacity(joined.len());
        for row in &joined {
            if row.state.object_flags & OBJECT_VALID == 0 || row.state.inside_up >= 0 {
                authority_rows.push(None);
                continue;
            }
            if row.handle.is_none() {
                return Err(VisibilityAuthorityFault::MissingLiveHandle {
                    row: row.state.identity.row,
                });
            }
            authority_rows.push(Some(self.authority_for_row(row, &joined)?));
        }
        let receipt = Step12VisibilityAuthorityReceipt {
            frame: world.frame,
            state_revision: self.state_revision,
            type_revision: self.type_revision,
            leader_active,
            owner_band_lengths,
            rows: authority_rows,
        };
        let snapshot = LiveStep12UnitBandSnapshot {
            frame: world.frame,
            state_revision: self.state_revision,
            type_revision: self.type_revision,
            leader_active,
            owner_band_lengths,
            rows: &states,
        };
        prepare_live_unit_pass(fog_option, trigger, snapshot, Some(&receipt))
            .map_err(VisibilityAuthorityFault::Frontier)
    }

    /// Preflight the live Unit subpass, then enforce the still-red whole-producer boundary.
    pub fn preflight_full_producer(
        &self,
        world: &World,
        unit_types: &[i32],
        leader_active: [bool; LEADER_SLOTS],
        fog_option: u8,
        trigger: Step12VisibilityTrigger,
        scheduled_active: Option<ScheduledActiveProducerContext<'_>>,
    ) -> Result<PreparedStep12FullProducer, Step12VisibilityPreflightError> {
        match self
            .prepare_unit_pass(world, unit_types, leader_active, fog_option, trigger)
            .map_err(Step12VisibilityPreflightError::Authority)?
        {
            LiveStep12Preparation::NoMutation(cadence) => {
                Ok(PreparedStep12FullProducer::NoMutation(cadence))
            }
            LiveStep12Preparation::UnitPass(pass)
                if !leader_active.iter().copied().any(|active| active) =>
            {
                debug_assert_eq!(pass.stamps(), 0);
                Ok(PreparedStep12FullProducer::InactiveLeadersClear)
            }
            LiveStep12Preparation::UnitPass(pass)
                if matches!(trigger, Step12VisibilityTrigger::ScheduledStep12) =>
            {
                let context =
                    scheduled_active.ok_or(Step12VisibilityPreflightError::ActiveUnitCohort(
                        ActiveUnitProducerFault::MissingScheduledContext,
                    ))?;
                prepare_active_unit_clear(pass, world, leader_active, context)
                    .map(PreparedStep12FullProducer::ActiveUnitClear)
                    .map_err(Step12VisibilityPreflightError::ActiveUnitCohort)
            }
            LiveStep12Preparation::UnitPass(pass) => {
                Err(Step12VisibilityPreflightError::IncompleteProducer {
                    prepared_unit_stamps: pass.stamps(),
                    residuals: Step12ProducerResiduals::MISSING,
                })
            }
        }
    }

    fn validate_unit_counts(
        &self,
        rows: &[LiveJoinRow],
        leader_active: [bool; LEADER_SLOTS],
    ) -> Result<(), VisibilityAuthorityFault> {
        let mut live_counts = [[0u16; UNIT_COUNT_SLOTS]; LEADER_SLOTS];
        for row in rows {
            if row.state.object_flags & OBJECT_VALID == 0 {
                continue;
            }
            if let Some(index) = type_count_index(row.state.identity.type_index) {
                let who = usize::from(row.state.identity.who);
                live_counts[who][index] = live_counts[who][index].wrapping_add(1);
            }
        }
        for who in 0..LEADER_SLOTS {
            if !leader_active[who] {
                continue;
            }
            for index in 0..UNIT_COUNT_SLOTS {
                if self.leaders[who].unit_counts[index] != live_counts[who][index] {
                    return Err(VisibilityAuthorityFault::UnitCountMismatch {
                        who: who as u8,
                        index,
                        authority: self.leaders[who].unit_counts[index],
                        live: live_counts[who][index],
                    });
                }
            }
        }
        Ok(())
    }

    fn authority_for_row(
        &self,
        row: &LiveJoinRow,
        rows: &[LiveJoinRow],
    ) -> Result<Step12UnitAuthorityReceipt, VisibilityAuthorityFault> {
        let identity = row.state.identity;
        let who = usize::from(identity.who);
        let ptolemy_count = self.leaders[who].unit_counts[PTOLEMY_NUM_UNITS_INDEX];
        let the_ceo_count = self.leaders[who].unit_counts[THE_CEO_NUM_UNITS_INDEX];
        let mut type_facts = None;
        let mut require_type = |fact: &'static str| {
            if type_facts.is_none() {
                type_facts = self.type_projection(identity.type_index);
            }
            type_facts.ok_or(VisibilityAuthorityFault::MissingTypeFact {
                row: identity.row,
                type_index: identity.type_index,
                fact,
            })
        };

        let unit_role = if ptolemy_count != 0 {
            require_type("role")?.role
        } else {
            0
        };
        let has_ptolemy_general = if ptolemy_count != 0 && unit_role & PTOLEMY_ROLE_MASK != 0 {
            Some(self.has_general(
                row,
                rows,
                super::step12_visibility_producer_frontier::PTOLEMY_TYPE_INDEX,
            )?)
        } else {
            None
        };
        let unit_is_siege = if the_ceo_count != 0 {
            Some(require_type("is_siege")?.is_siege)
        } else {
            None
        };
        let has_the_ceo_general = if the_ceo_count != 0 && unit_is_siege == Some(false) {
            Some(self.has_general(
                row,
                rows,
                super::step12_visibility_producer_frontier::THE_CEO_TYPE_INDEX,
            )?)
        } else {
            None
        };
        let constants_needed =
            has_ptolemy_general == Some(true) || has_the_ceo_general == Some(true);
        let constants = if constants_needed {
            self.constants
                .ok_or(VisibilityAuthorityFault::MissingConstants { row: identity.row })?
        } else {
            VisibilityConstants {
                ptolemy_los_bonus: 0,
                the_ceo_unit_los: 0,
            }
        };
        let los_facts = UnitLosFacts {
            mylos: row.state.mylos,
            ptolemy_count,
            unit_role,
            has_ptolemy_general,
            ptolemy_los_bonus: constants.ptolemy_los_bonus,
            the_ceo_count,
            unit_is_siege,
            has_the_ceo_general,
            the_ceo_unit_los: constants.the_ceo_unit_los,
        };
        let resolved_los = resolve_unit_los(los_facts)
            .map_err(|_| VisibilityAuthorityFault::Los { row: identity.row })?;
        let positive_los = resolved_los > 0;
        let radius = resolved_los.wrapping_mul(0xc0) / 0x180;

        let (unit_domain, type_unit_flags2, small_los_projection) = if positive_los && radius <= 3 {
            let facts = require_type("domain/unit_flags2")?;
            let projection = if facts.domain == 0
                && facts.unit_flags2 & SMALL_LOS_STANDARD_TYPE_FLAGS2 == 0
                && row.state.unit_masks & SMALL_LOS_STANDARD_UNIT_MASKS == 0
            {
                let (projected_fine_x, projected_fine_y) = super::collision::boat_project(
                    row.state.fine_x,
                    row.state.fine_y,
                    row.state.unit_angle,
                    SMALL_LOS_PROJECT_DISTANCE,
                );
                Some(SmallLosProjectionReceipt {
                    source_fine_x: row.state.fine_x,
                    source_fine_y: row.state.fine_y,
                    unit_angle: row.state.unit_angle,
                    distance: SMALL_LOS_PROJECT_DISTANCE,
                    projected_fine_x,
                    projected_fine_y,
                })
            } else {
                None
            };
            (Some(facts.domain), Some(facts.unit_flags2), projection)
        } else {
            (None, None, None)
        };

        let detector = if positive_los {
            let handle = row
                .handle
                .ok_or(VisibilityAuthorityFault::MissingLiveHandle { row: identity.row })?;
            let instance =
                self.instance(handle)
                    .ok_or(VisibilityAuthorityFault::MissingDetectorInstance {
                        row: identity.row,
                        handle,
                    })?;
            if instance.uid != identity.uid {
                return Err(VisibilityAuthorityFault::DetectorUidMismatch {
                    row: identity.row,
                    live: identity.uid,
                    authority: instance.uid,
                });
            }
            Some(match instance.detector {
                VisibilityDetectorAuthority::ObjectInitMasks(object_masks_at_init) => {
                    DetectorInstanceProvenance::ObjectInit {
                        object_masks_at_init,
                    }
                }
                VisibilityDetectorAuthority::AuthoritativeInstanceFlags(instance_flags) => {
                    DetectorInstanceProvenance::AuthoritativeInstance {
                        instance_flags,
                        state_revision: self.state_revision,
                    }
                }
            })
        } else {
            None
        };

        let local_seen_radius = if positive_los && row.state.visible != 0 {
            Some(require_type("local_seen_radius")?.local_seen_radius)
        } else {
            None
        };

        Ok(Step12UnitAuthorityReceipt {
            identity,
            detector,
            los_facts,
            unit_domain,
            type_unit_flags2,
            small_los_projection,
            local_seen_radius,
        })
    }

    fn has_general(
        &self,
        target: &LiveJoinRow,
        rows: &[LiveJoinRow],
        type_index: i32,
    ) -> Result<bool, VisibilityAuthorityFault> {
        if target.state.identity.type_index == type_index {
            return Ok(true);
        }
        let owner = usize::from(target.state.identity.who);
        for hero in &self.leaders[owner].heroes {
            if hero.hero_flags & HERO_RECORD_ACTIVE == 0 {
                continue;
            }
            if hero.radius_tiles < 0 {
                return Err(VisibilityAuthorityFault::InvalidHeroRadius {
                    who: target.state.identity.who,
                    slot: hero.slot,
                    radius: hero.radius_tiles,
                });
            }
            let Some(candidate) = rows.iter().find(|row| row.handle == Some(hero.handle)) else {
                return Err(VisibilityAuthorityFault::MissingHeroObject {
                    who: target.state.identity.who,
                    slot: hero.slot,
                    handle: hero.handle,
                });
            };
            let id = candidate.state.identity;
            if id.uid != hero.uid
                || id.who as i8 != hero.who
                || id.object_o != hero.o
                || id.type_index != hero.type_index
            {
                return Err(VisibilityAuthorityFault::HeroIdentityMismatch {
                    who: target.state.identity.who,
                    slot: hero.slot,
                });
            }
            if candidate.state.object_flags & OBJECT_VALID == 0 || id.type_index != type_index {
                continue;
            }
            let distance = super::movement::vector_dist(
                target.state.fine_x.wrapping_sub(candidate.state.fine_x),
                target.state.fine_y.wrapping_sub(candidate.state.fine_y),
            );
            let radius = hero.radius_tiles.wrapping_mul(0xc0);
            if distance <= radius {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Deterministic authority/staleness digest. It is deliberately separate from retail
    /// checksum channel 12, whose bytes are only `World` sections 1..13.
    pub fn digest(&self) -> u64 {
        fn mix(hash: &mut u64, value: u64) {
            *hash ^= value;
            *hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        let mut hash = 0xcbf2_9ce4_8422_2325;
        mix(&mut hash, self.state_revision);
        mix(&mut hash, self.type_revision);
        mix(&mut hash, self.composition_digest);
        match self.constants {
            Some(constants) => {
                mix(&mut hash, 1);
                mix(&mut hash, constants.ptolemy_los_bonus as u32 as u64);
                mix(&mut hash, constants.the_ceo_unit_los as u32 as u64);
            }
            None => mix(&mut hash, 0),
        }
        mix(&mut hash, self.types.len() as u64);
        for facts in &self.types {
            mix(&mut hash, facts.type_index as u32 as u64);
            mix(&mut hash, u64::from(facts.object_masks));
            mix(&mut hash, facts.domain as u32 as u64);
            mix(&mut hash, u64::from(facts.unit_flags2));
            mix(&mut hash, u64::from(facts.role));
            mix(&mut hash, facts.is_siege as u64);
            mix(&mut hash, facts.local_seen_radius as u32 as u64);
        }
        for leader in &self.leaders {
            for &count in &leader.unit_counts {
                mix(&mut hash, u64::from(count));
            }
            mix(&mut hash, leader.heroes.len() as u64);
            for hero in &leader.heroes {
                mix(&mut hash, u64::from(hero.slot));
                mix(&mut hash, u64::from(hero.handle.id));
                mix(&mut hash, u64::from(hero.handle.generation));
                mix(&mut hash, u64::from(hero.uid));
                mix(&mut hash, hero.o as u16 as u64);
                mix(&mut hash, hero.who as u8 as u64);
                mix(&mut hash, u64::from(hero.hero_flags));
                mix(&mut hash, hero.type_index as u32 as u64);
                mix(&mut hash, hero.radius_tiles as u32 as u64);
            }
        }
        mix(&mut hash, self.instances.len() as u64);
        for instance in &self.instances {
            mix(&mut hash, u64::from(instance.handle.id));
            mix(&mut hash, u64::from(instance.handle.generation));
            mix(&mut hash, u64::from(instance.uid));
            match instance.detector {
                VisibilityDetectorAuthority::ObjectInitMasks(masks) => {
                    mix(&mut hash, 0);
                    mix(&mut hash, u64::from(masks));
                }
                VisibilityDetectorAuthority::AuthoritativeInstanceFlags(flags) => {
                    mix(&mut hash, 1);
                    mix(&mut hash, u64::from(flags));
                }
            }
        }
        hash
    }
}

fn type_count_index(type_index: i32) -> Option<usize> {
    let index = type_index.checked_sub(UNIT_TYPE_BASE)?;
    usize::try_from(index)
        .ok()
        .filter(|&index| index < UNIT_COUNT_SLOTS)
}

fn validate_hero_records(
    who: usize,
    heroes: &[VisibilityHeroRecord],
) -> Result<(), VisibilityAuthorityInstallError> {
    let mut previous = None;
    for hero in heroes {
        if let Some(previous) = previous {
            if hero.slot <= previous {
                return Err(VisibilityAuthorityInstallError::HeroSlotsOutOfOrder {
                    who,
                    previous,
                    current: hero.slot,
                });
            }
        }
        if !(0..LEADER_SLOTS as i8).contains(&hero.who) {
            return Err(VisibilityAuthorityInstallError::HeroOwnerOutOfRange { who: hero.who });
        }
        if hero.radius_tiles < 0 {
            return Err(VisibilityAuthorityInstallError::NegativeHeroRadius {
                who,
                slot: hero.slot,
                radius: hero.radius_tiles,
            });
        }
        previous = Some(hero.slot);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::systems::step12_visibility_producer_frontier::{
        UnitStampCenter, UnitStampDecision, OBJMASK_DETECT,
    };

    #[test]
    fn exact_type_projection_object_init_counts_and_projection_join_one_revision() {
        let mut world = World::new(7);
        assert!(world.set_object_owner_active(0, true));
        let handle = world.spawn_typed(0, UNIT_TYPE_BASE).unwrap();
        let row = world.row_of(handle).unwrap();
        world.set_pos(row, 10 * 0x180, 12 * 0x180);
        world.units.angle_mut()[row] = 0x4000_0000;
        world.units.mylos_mut()[row] = 4;
        let unit_types = [UNIT_TYPE_BASE];

        let mut authority = Step12VisibilityAuthority::default();
        authority
            .replace_type_source(
                11,
                0x1234,
                VisibilityConstants {
                    ptolemy_los_bonus: 2,
                    the_ceo_unit_los: 2,
                },
                vec![VisibilityTypeProjection {
                    type_index: UNIT_TYPE_BASE,
                    object_masks: OBJMASK_DETECT,
                    domain: 0,
                    unit_flags2: 0,
                    role: 0,
                    is_siege: false,
                    local_seen_radius: 2,
                }],
            )
            .unwrap();
        authority
            .materialize_object_init(&mut world, &unit_types, handle)
            .unwrap();
        assert_ne!(
            world.units.get_flags(row)
                & super::super::step12_visibility_producer_frontier::OBJECT_DETECTOR,
            0
        );

        world.frame = 33;
        let LiveStep12Preparation::UnitPass(pass) = authority
            .prepare_unit_pass(
                &world,
                &unit_types,
                [true, false, false, false, false, false, false, false],
                0,
                Step12VisibilityTrigger::ScheduledStep12,
            )
            .unwrap()
        else {
            panic!("expected prepared Unit pass")
        };
        let UnitStampDecision::Stamp(stamp) = pass.rows()[0].decision() else {
            panic!("expected stamp")
        };
        assert!(stamp.detector);
        assert_eq!(stamp.center, UnitStampCenter::ProjectedSmallLandUnit);
        assert_eq!(stamp.stamp_fine_x, world.units.x_internal()[row] + 0x180);
        assert_eq!(pass.state_revision(), authority.state_revision());
        assert_eq!(pass.type_revision(), 11);
    }

    #[test]
    fn full_producer_admits_the_exact_all_inactive_clear_path() {
        let world = World::new(9);
        let authority = Step12VisibilityAuthority::default();
        let prepared = authority
            .preflight_full_producer(
                &world,
                &[],
                [false; LEADER_SLOTS],
                0,
                Step12VisibilityTrigger::GameRun,
                None,
            )
            .unwrap();
        assert_eq!(prepared, PreparedStep12FullProducer::InactiveLeadersClear);
    }

    #[test]
    fn active_leader_still_never_turns_a_unit_pass_into_clear_permission() {
        let mut world = World::new(9);
        assert!(world.set_object_owner_active(0, true));
        let authority = Step12VisibilityAuthority::default();
        let error = authority
            .preflight_full_producer(
                &world,
                &[],
                [true, false, false, false, false, false, false, false],
                0,
                Step12VisibilityTrigger::GameRun,
                None,
            )
            .unwrap_err();
        assert_eq!(
            error,
            Step12VisibilityPreflightError::IncompleteProducer {
                prepared_unit_stamps: 0,
                residuals: Step12ProducerResiduals::MISSING,
            }
        );
    }

    #[test]
    fn leader_counts_ordered_heroes_and_constants_resolve_the_ptolemy_los_arm() {
        let mut world = World::new(11);
        assert!(world.set_object_owner_active(0, true));
        let target_type = UNIT_TYPE_BASE;
        let target = world.spawn_typed(0, target_type).unwrap();
        let hero = world
            .spawn_typed(
                0,
                super::super::step12_visibility_producer_frontier::PTOLEMY_TYPE_INDEX,
            )
            .unwrap();
        let target_row = world.row_of(target).unwrap();
        let hero_row = world.row_of(hero).unwrap();
        world.set_pos(target_row, 10 * 0x180, 10 * 0x180);
        world.set_pos(hero_row, 10 * 0x180 + 0xc0, 10 * 0x180);
        world.units.mylos_mut()[target_row] = 4;
        world.units.mylos_mut()[hero_row] = 0;
        let unit_types = [
            target_type,
            super::super::step12_visibility_producer_frontier::PTOLEMY_TYPE_INDEX,
        ];

        let mut authority = Step12VisibilityAuthority::default();
        authority
            .replace_type_source(
                12,
                0x5566,
                VisibilityConstants {
                    ptolemy_los_bonus: 2,
                    the_ceo_unit_los: 7,
                },
                vec![
                    VisibilityTypeProjection {
                        type_index: target_type,
                        object_masks: 0,
                        domain: 0,
                        unit_flags2: SMALL_LOS_STANDARD_TYPE_FLAGS2,
                        role: PTOLEMY_ROLE_MASK,
                        is_siege: false,
                        local_seen_radius: 2,
                    },
                    VisibilityTypeProjection {
                        type_index:
                            super::super::step12_visibility_producer_frontier::PTOLEMY_TYPE_INDEX,
                        object_masks: 0,
                        domain: 0,
                        unit_flags2: SMALL_LOS_STANDARD_TYPE_FLAGS2,
                        role: 0,
                        is_siege: false,
                        local_seen_radius: 2,
                    },
                ],
            )
            .unwrap();
        authority
            .materialize_object_init(&mut world, &unit_types, target)
            .unwrap();
        authority
            .materialize_object_init(&mut world, &unit_types, hero)
            .unwrap();
        let mut leader = authority.leaders()[0].clone();
        leader.heroes.push(VisibilityHeroRecord {
            slot: 0,
            handle: hero,
            uid: world.units.get_uid(hero_row),
            o: world.units.o()[hero_row],
            who: 0,
            hero_flags: HERO_RECORD_ACTIVE,
            type_index: super::super::step12_visibility_producer_frontier::PTOLEMY_TYPE_INDEX,
            radius_tiles: 2,
        });
        authority.replace_leader(0, leader).unwrap();

        world.frame = 33;
        let LiveStep12Preparation::UnitPass(pass) = authority
            .prepare_unit_pass(
                &world,
                &unit_types,
                [true, false, false, false, false, false, false, false],
                0,
                Step12VisibilityTrigger::ScheduledStep12,
            )
            .unwrap()
        else {
            panic!("expected prepared Unit pass")
        };
        let target = pass
            .rows()
            .iter()
            .find(|row| row.identity().row == target_row)
            .unwrap();
        let UnitStampDecision::Stamp(stamp) = target.decision() else {
            panic!("target should receive its general LOS bonus")
        };
        assert_eq!(stamp.radius_fog_cells, 3, "(4 + 2) * 192 / 384");
        assert_eq!(stamp.center, UnitStampCenter::ObjectPosition);
        assert_eq!(
            authority.leaders()[0].unit_counts[PTOLEMY_NUM_UNITS_INDEX],
            1
        );
    }
}
