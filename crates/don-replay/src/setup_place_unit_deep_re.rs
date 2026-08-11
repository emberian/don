//! Exact source-only producer for the `Setup::place_unit` probe loop and the
//! `Unit::init` Guy-allocation / `Guy::init_real` prefix.
//!
//! This module deliberately stops at native call boundaries that still require a shared
//! mutable owner.  It does not install either replay checksum channel.  Its purpose is to
//! remove two former "runtime BSS" unknowns, preserve game-RNG chronology, and return
//! receipts that a future `Objects::init_unit` host can consume without reconstructing
//! candidate order or synthesising Guy bytes.

use don_sim::rng::Random;
use don_sim::systems::graphics_turret::{ExtractedGuyGraphics, GraphicsProvenance};
use don_sim::systems::groups_guys::{GuyData, UnitGuys};
use don_sim::systems::unit_inctime::{SUPPORTED_RETAIL_EXE_SHA256, SUPPORTED_UNIT_GRAPHICS_SHA256};
use std::fmt;

pub const SETUP_PLACE_UNIT_VA: u32 = 0x005a_bca0;
pub const SETUP_PLACE_UNIT_BYTES: u32 = 749;
pub const PLACE_UNIT_RANDOM_CALL_VA: u32 = 0x005a_bd76;
pub const CIRCLE_INIT_VA: u32 = 0x0068_17f0;
pub const CIRCLE_INIT_BYTES: u32 = 295;
pub const CIRCLE_X_VA: u32 = 0x00cb_7e90;
pub const CIRCLE_Y_VA: u32 = 0x00cb_b0e0;
pub const CIRCLE_END_VA: u32 = 0x00cb_e330;
pub const CIRCLE_MAX_RADIUS: usize = 64;
pub const CIRCLE_STORAGE_LIMIT: usize = 0x3248 + 1;

pub const OBJECTS_INIT_UNIT_THUNK_VA: u32 = 0x0046_1310;
pub const OBJECTS_INIT_UNIT_VA: u32 = 0x0065_e0c0;
pub const UNIT_COME_OUT_VA: u32 = 0x0061_7c10;
pub const BUILD_VTABLE_VA: u32 = 0x00b4_2174;
pub const BUILD_GET_BUILD_SLOT: u32 = 0x00ac;
pub const BUILD_GET_BUILD_CALL_VA: u32 = 0x005a_bf2f;
pub const BUILD_GET_BUILD_TARGET_VA: u32 = 0x0041_c000;
pub const BUILD_TRAIN_CALL_VA: u32 = 0x005a_bf3a;
pub const BUILD_TRAIN_VA: u32 = 0x0062_f9b0;

pub const UNIT_INIT_VA: u32 = 0x0061_2100;
pub const UNIT_INIT_BYTES: u32 = 3_732;
pub const UNIT_GUY_ALLOCATION_BEGIN_VA: u32 = 0x0061_2a7a;
pub const UNIT_GUY_INIT_LOOP_BEGIN_VA: u32 = 0x0061_2c2f;
pub const UNIT_GUY_INIT_LOOP_END_VA: u32 = 0x0061_2cc1;
pub const GUY_CLEAR_VA: u32 = 0x005d_b590;
pub const GUY_CLEAR_BYTES: u32 = 276;
pub const GUY_INIT_REAL_VA: u32 = 0x005d_b6b0;
pub const GUY_INIT_REAL_BYTES: u32 = 1_442;
pub const GUY_INIT_RANDOM_CALL_VA: u32 = 0x005d_b6fd;
pub const UNIT_UPDATE_GPIECE_VA: u32 = 0x005e_2920;
pub const UNIT_SET_NEW_LOCATION_VA: u32 = 0x005f_8d20;

pub const PLAYABLE_OWNER_SLOTS: i32 = 8;
pub const UNIT_BAND_LIMIT: i32 = 2_000;
pub const MAX_GUYS_PER_UNIT: i32 = 128;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CircleTables {
    offsets: Vec<(i8, i8)>,
    ring_end: [usize; CIRCLE_MAX_RADIUS + 1],
}

impl CircleTables {
    pub fn offsets(&self) -> &[(i8, i8)] {
        &self.offsets
    }

    pub fn end(&self, radius: usize) -> Option<usize> {
        self.ring_end.get(radius).copied()
    }
}

fn approximate_circle_distance(x: i32, y: i32) -> i32 {
    let ax = x.wrapping_abs();
    let ay = y.wrapping_abs();
    if ay < ax {
        if ax == 0 {
            0
        } else if ay < 60_000 {
            ay.wrapping_mul(ay) / ax.wrapping_mul(2) + ax
        } else {
            ay.wrapping_add(ax.wrapping_mul(2)) >> 1
        }
    } else if ay == 0 {
        0
    } else if ax < 60_000 {
        ax.wrapping_mul(ax) / ay.wrapping_mul(2) + ay
    } else {
        ax.wrapping_add(ay.wrapping_mul(2)) >> 1
    }
}

/// Port the complete runtime initializer of `circle_x`, `circle_y`, and `circle_radius`.
///
/// The storage cap is checked after a coordinate is appended, so the last admitted row is
/// entry 12,872 (length 12,873).  Retail then copies that one end value through radius 64.
pub fn build_circle_tables() -> CircleTables {
    let mut offsets = Vec::with_capacity(CIRCLE_STORAGE_LIMIT);
    let mut ring_end = [0usize; CIRCLE_MAX_RADIUS + 1];

    for radius in 0..=CIRCLE_MAX_RADIUS {
        let r = radius as i32;
        for x in -r..=r {
            for y in -r..=r {
                if approximate_circle_distance(x, y) != r {
                    continue;
                }
                offsets.push((x as i8, y as i8));
                if offsets.len() > 0x3248 {
                    for end in &mut ring_end[radius..] {
                        *end = offsets.len();
                    }
                    return CircleTables { offsets, ring_end };
                }
            }
        }
        ring_end[radius] = offsets.len();
    }
    CircleTables { offsets, ring_end }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlacementTileFacts {
    /// `WData +0x04`; the candidate cell must equal the anchor cell.
    pub continent: u16,
    /// `WData +0x00`.
    pub flags: u16,
    /// Signed byte at `WData +0x02`.
    pub land: i8,
    /// Signed short at `WData +0x08`; a nonnegative value blocks placement.
    pub occupied_o: i16,
    /// Collision word in `World::tdata` (`WorldData +0x138`) at the center TCoord of this
    /// WCoord cell: `(4*y + 2) * tile_xs + (4*x + 2)`.
    pub collision: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlacementMapSnapshot {
    pub xs: i32,
    pub ys: i32,
    pub tiles: Vec<PlacementTileFacts>,
}

impl PlacementMapSnapshot {
    fn validate(&self) -> bool {
        self.xs > 0
            && self.ys > 0
            && self
                .xs
                .checked_mul(self.ys)
                .and_then(|n| usize::try_from(n).ok())
                == Some(self.tiles.len())
    }

    fn tile(&self, x: i32, y: i32) -> Option<PlacementTileFacts> {
        if x < 0 || y < 0 || x >= self.xs || y >= self.ys {
            return None;
        }
        let index = y.checked_mul(self.xs)?.checked_add(x)?;
        self.tiles.get(usize::try_from(index).ok()?).copied()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CenterBuildFacts {
    pub owner: i32,
    pub o: i32,
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlaceUnitInputs {
    pub owner: i32,
    /// Exact output of the second `LeaderData::current_upgrade` call at `0x005ABCB8`.
    pub upgraded_type: i32,
    pub requested_x: i32,
    pub requested_y: i32,
    pub center: Option<CenterBuildFacts>,
    /// `GameInfo::starting_town` (`Game +0x2C`).
    pub starting_town: u8,
    /// `LeaderData::active` (`Leader +0x93C`).
    pub leader_active: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RandomDrawReceipt {
    pub call_va: u32,
    pub state_before: i32,
    pub returned: i32,
    pub state_after: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProbeRejection {
    XBelowZero,
    YBelowZero,
    XAtOrAboveWorld,
    YAtOrAboveWorld,
    DifferentContinent,
    WDataFlags30,
    LandClassOneOrTwo,
    WDataFlag100,
    Occupied,
    CollisionClass,
    NegativeWDataFlags,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProbeDisposition {
    Rejected(ProbeRejection),
    Accepted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlacementProbeReceipt {
    pub ordinal: u16,
    pub draw: RandomDrawReceipt,
    pub offset_index: usize,
    pub offset: (i8, i8),
    pub candidate_tile: (i32, i32),
    pub disposition: ProbeDisposition,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObjectsInitUnitRequest {
    pub call_va: u32,
    pub body_va: u32,
    pub owner: i32,
    pub type_index: i32,
    pub x: i32,
    pub y: i32,
    pub exact_o: i32,
    pub external_previous: i32,
    pub external_next: i32,
    /// Exhausted city-less placement calls `Unit::come_out(0)` only after a successful
    /// allocation.  An accepted candidate returns directly from the thin thunk.
    pub come_out_zero_after_success: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildTrainRequest {
    /// The exhausted-center path first dispatches `Build::get_build` through slot `+0xAC`.
    /// The supported retail `Build` vtable resolves that getter to the three-byte identity
    /// return at `0x0041C000`; its returned `Build *` is the receiver of `train`.
    pub receiver_vtable_va: u32,
    pub receiver_get_build_slot: u32,
    pub receiver_get_build_call_va: u32,
    pub receiver_get_build_target_va: u32,
    pub train_call_va: u32,
    pub body_va: u32,
    pub center: CenterBuildFacts,
    pub type_index: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaceUnitExternalResidual {
    ObjectsInitUnit(ObjectsInitUnitRequest),
    BuildTrain(BuildTrainRequest),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaceUnitProducerReceipt {
    pub inputs: PlaceUnitInputs,
    pub anchor_coord: (i32, i32),
    pub anchor_tile: (i32, i32),
    pub radius: usize,
    pub circle_count: usize,
    pub attempt_limit: u16,
    pub rng_initial: i32,
    pub rng_after_probes: i32,
    pub probes: Vec<PlacementProbeReceipt>,
    /// This is the first unexecuted native mutator.  Returning it rather than a guessed
    /// object identity keeps the placement producer useful without claiming allocation.
    pub first_external_residual: PlaceUnitExternalResidual,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlaceUnitProducerError {
    OwnerOutOfRange { owner: i32 },
    InvalidType { type_index: i32 },
    InvalidCenter,
    InvalidMapShape,
    CoordinateOutsideTableDomain { x: i32, y: i32 },
    AnchorOutsideWorld { x: i32, y: i32 },
}

impl fmt::Display for PlaceUnitProducerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Setup::place_unit producer refused: {self:?}")
    }
}

impl std::error::Error for PlaceUnitProducerError {}

fn reject_candidate(
    map: &PlacementMapSnapshot,
    anchor: PlacementTileFacts,
    x: i32,
    y: i32,
) -> Option<ProbeRejection> {
    if x < 0 {
        return Some(ProbeRejection::XBelowZero);
    }
    if y < 0 {
        return Some(ProbeRejection::YBelowZero);
    }
    if x >= map.xs {
        return Some(ProbeRejection::XAtOrAboveWorld);
    }
    if y >= map.ys {
        return Some(ProbeRejection::YAtOrAboveWorld);
    }
    let tile = map.tile(x, y).expect("bounds checked above");
    if tile.continent != anchor.continent {
        return Some(ProbeRejection::DifferentContinent);
    }
    if tile.flags & 0x30 != 0 {
        return Some(ProbeRejection::WDataFlags30);
    }
    if tile.flags & 0x100 == 0 && matches!(tile.land, 1 | 2) {
        return Some(ProbeRejection::LandClassOneOrTwo);
    }
    if tile.flags & 0x100 != 0 {
        return Some(ProbeRejection::WDataFlag100);
    }
    if tile.occupied_o >= 0 {
        return Some(ProbeRejection::Occupied);
    }
    if tile.collision & 0x4000 != 0 || tile.collision & 3 == 3 {
        return Some(ProbeRejection::CollisionClass);
    }
    if tile.flags & 0x8000 != 0 {
        return Some(ProbeRejection::NegativeWDataFlags);
    }
    None
}

fn objects_init_request(
    call_va: u32,
    inputs: PlaceUnitInputs,
    x: i32,
    y: i32,
    come_out_zero_after_success: bool,
) -> ObjectsInitUnitRequest {
    ObjectsInitUnitRequest {
        call_va,
        body_va: OBJECTS_INIT_UNIT_VA,
        owner: inputs.owner,
        type_index: inputs.upgraded_type,
        x,
        y,
        exact_o: -1,
        external_previous: -1,
        external_next: -1,
        come_out_zero_after_success,
    }
}

/// Execute every deterministic action through the first still-external mutation.
///
/// All reachable setup radii have more than one circle entry, so every probe consumes one
/// `Random::get(0,0xffff)` draw before any world gate.  Rejected candidates retain that
/// draw.  The accepted call or exhaustion fallback is returned but is not executed.
pub fn produce_place_unit_probe_prefix(
    inputs: PlaceUnitInputs,
    map: &PlacementMapSnapshot,
    rng_state: i32,
) -> Result<PlaceUnitProducerReceipt, PlaceUnitProducerError> {
    if !(0..PLAYABLE_OWNER_SLOTS).contains(&inputs.owner) {
        return Err(PlaceUnitProducerError::OwnerOutOfRange {
            owner: inputs.owner,
        });
    }
    if inputs.upgraded_type < 0 {
        return Err(PlaceUnitProducerError::InvalidType {
            type_index: inputs.upgraded_type,
        });
    }
    if inputs
        .center
        .is_some_and(|center| center.owner != inputs.owner || !(2_000..3_000).contains(&center.o))
    {
        return Err(PlaceUnitProducerError::InvalidCenter);
    }
    if !map.validate() {
        return Err(PlaceUnitProducerError::InvalidMapShape);
    }
    let anchor_coord = inputs
        .center
        .map(|center| (center.x, center.y))
        .unwrap_or((inputs.requested_x, inputs.requested_y));
    if anchor_coord.0 < 0 || anchor_coord.1 < 0 {
        return Err(PlaceUnitProducerError::CoordinateOutsideTableDomain {
            x: anchor_coord.0,
            y: anchor_coord.1,
        });
    }
    // For generated-map coordinates the runtime `div_3_table[coord >> 8]` is exactly
    // floor(coord / 768).  Negative indices are outside the admitted setup domain above.
    let anchor_tile = (anchor_coord.0 / 0x300, anchor_coord.1 / 0x300);
    let anchor = map.tile(anchor_tile.0, anchor_tile.1).ok_or(
        PlaceUnitProducerError::AnchorOutsideWorld {
            x: anchor_tile.0,
            y: anchor_tile.1,
        },
    )?;

    let (attempt_limit, radius) = if inputs.starting_town != 0 {
        (60u16, 2usize)
    } else if inputs.leader_active != 0 {
        (500u16, 24usize)
    } else {
        (500u16, 8usize)
    };
    let circles = build_circle_tables();
    let circle_count = circles.end(radius).expect("setup radii are bounded");
    debug_assert!(circle_count > 1);
    let mut rng = Random::new(rng_state);
    let mut probes = Vec::with_capacity(attempt_limit as usize);

    for ordinal in 0..attempt_limit {
        let state_before = rng.state();
        let returned = rng.get(0, 0xffff);
        let state_after = rng.state();
        let offset_index = usize::try_from(returned % circle_count as i32)
            .expect("retail Random::get(0,0xffff) is nonnegative");
        let offset = circles.offsets[offset_index];
        let candidate_tile = (
            anchor_tile.0.wrapping_add(i32::from(offset.0)),
            anchor_tile.1.wrapping_add(i32::from(offset.1)),
        );
        let rejection = reject_candidate(map, anchor, candidate_tile.0, candidate_tile.1);
        let disposition = rejection
            .map(ProbeDisposition::Rejected)
            .unwrap_or(ProbeDisposition::Accepted);
        probes.push(PlacementProbeReceipt {
            ordinal,
            draw: RandomDrawReceipt {
                call_va: PLACE_UNIT_RANDOM_CALL_VA,
                state_before,
                returned,
                state_after,
            },
            offset_index,
            offset,
            candidate_tile,
            disposition,
        });
        if rejection.is_none() {
            let x = candidate_tile.0.wrapping_mul(0x300).wrapping_add(0x180);
            let y = candidate_tile.1.wrapping_mul(0x300).wrapping_add(0x180);
            return Ok(PlaceUnitProducerReceipt {
                inputs,
                anchor_coord,
                anchor_tile,
                radius,
                circle_count,
                attempt_limit,
                rng_initial: rng_state,
                rng_after_probes: rng.state(),
                probes,
                first_external_residual: PlaceUnitExternalResidual::ObjectsInitUnit(
                    objects_init_request(OBJECTS_INIT_UNIT_THUNK_VA, inputs, x, y, false),
                ),
            });
        }
    }

    let first_external_residual = match inputs.center {
        Some(center) => PlaceUnitExternalResidual::BuildTrain(BuildTrainRequest {
            receiver_vtable_va: BUILD_VTABLE_VA,
            receiver_get_build_slot: BUILD_GET_BUILD_SLOT,
            receiver_get_build_call_va: BUILD_GET_BUILD_CALL_VA,
            receiver_get_build_target_va: BUILD_GET_BUILD_TARGET_VA,
            train_call_va: BUILD_TRAIN_CALL_VA,
            body_va: BUILD_TRAIN_VA,
            center,
            type_index: inputs.upgraded_type,
        }),
        None => PlaceUnitExternalResidual::ObjectsInitUnit(objects_init_request(
            OBJECTS_INIT_UNIT_VA,
            inputs,
            anchor_coord.0,
            anchor_coord.1,
            true,
        )),
    };
    Ok(PlaceUnitProducerReceipt {
        inputs,
        anchor_coord,
        anchor_tile,
        radius,
        circle_count,
        attempt_limit,
        rng_initial: rng_state,
        rng_after_probes: rng.state(),
        probes,
        first_external_residual,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct StableUnitIdentity {
    pub id: u32,
    pub generation: u32,
    pub owner: i32,
    pub o: i32,
    pub type_index: i32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GuyInitPredicateFacts {
    /// The selected initial animation has an in-range packet and a loaded RData body.
    pub valid_animation: [bool; 4],
    /// `GraphicPieces::pivot_restrictions[get_type(gpiece)-50]` is non-empty.
    pub has_pivot_restrictions: bool,
    /// Animation packet slot 22 resolves to loaded graphics.
    pub animation_22_loaded: bool,
    pub unit_flags2_bit_4: bool,
    pub type_is_0x20: bool,
    pub type_is_0x1000: bool,
    pub unit_flags_bit_0x10: bool,
    pub unit_flags_bit_0x2: bool,
    pub air_predicate: bool,
    pub base_type_is_52_or_53: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GuyGraphicsInitReceipt {
    pub provenance: GraphicsProvenance,
    /// Exact hierarchy output from the existing `graphics_turret` extractor boundary.
    pub extracted: ExtractedGuyGraphics,
    /// Number returned by `GraphicPieces::get_restrictions`; 0 or at most four.
    pub restriction_count: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnitGuyInitInputs {
    pub identity: StableUnitIdentity,
    pub squad_size: i32,
    pub crew_size: i32,
    pub graphics: Vec<GuyGraphicsInitReceipt>,
    pub predicates: Vec<GuyInitPredicateFacts>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GuyInitRandomReceipt {
    pub slot: i32,
    pub call_va: u32,
    pub state_before: i32,
    pub returned: i32,
    pub remainder: i32,
    pub selected_before_validation: i8,
    pub selected_after_validation: i8,
    pub state_after: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StableGuyIdentity {
    pub unit_id: u32,
    pub unit_generation: u32,
    pub owner: i8,
    pub o: i16,
    pub guy_num: i8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnitGuyExternalResidual {
    /// `Unit::init` next calls `Unit::update_gpiece`, then the complete
    /// `Unit::set_new_location(anchor,1,1)` body.  The latter owns squad formation and
    /// recursively derives crew positions from the exact graphics hierarchy.
    UpdateGpieceThenSetNewLocation {
        update_gpiece_va: u32,
        set_new_location_va: u32,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct UnitGuyInitPrefixReceipt {
    pub identity: StableUnitIdentity,
    pub rng_initial: i32,
    pub rng_after_guys: i32,
    pub array_length: i32,
    pub array_capacity: i32,
    pub array_increment: i16,
    pub array_flags: u8,
    pub guy_mark: i8,
    pub stable_guys: Vec<StableGuyIdentity>,
    pub random: Vec<GuyInitRandomReceipt>,
    /// Runnable synchronized state at `0x00612CC1`, immediately after every
    /// `Guy::init_real(0)` and the per-slot variation store.
    pub guys: UnitGuys,
    pub first_external_residual: UnitGuyExternalResidual,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnitGuyInitError {
    InvalidIdentity,
    InvalidGuyCounts,
    ReceiptCountMismatch,
    UnsupportedExecutable { slot: usize },
    UnsupportedGraphicsData { slot: usize },
    IncoherentGraphicsCapture { slot: usize },
    WrongGraphicsSlot { slot: usize },
    MissingGpiece { slot: usize },
    InvalidRestrictionCount { slot: usize },
    GraphicsPredicateMismatch { slot: usize },
    InvalidPivotFlags { slot: usize },
}

impl fmt::Display for UnitGuyInitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Unit::init Guy prefix refused: {self:?}")
    }
}

impl std::error::Error for UnitGuyInitError {}

fn guy_clear_image() -> GuyData {
    GuyData {
        ty: 50,
        x: -1_536,
        y: -1_536,
        last_time: -1,
        gpiece: -1,
        ox: -1,
        whom: -1,
        ..GuyData::default()
    }
}

fn selected_animation(remainder: i32) -> i8 {
    match remainder {
        70..=79 => 1,
        80..=89 => 2,
        90..=99 => 3,
        _ => 0,
    }
}

fn guy_init_flags(facts: GuyInitPredicateFacts) -> u16 {
    let mut flags = 0u16;
    if facts.has_pivot_restrictions {
        flags |= 0x100;
    }
    if facts.animation_22_loaded || facts.unit_flags2_bit_4 {
        flags |= 0x8;
    } else if (facts.type_is_0x20 || facts.type_is_0x1000 || facts.unit_flags_bit_0x10)
        && !facts.unit_flags_bit_0x2
    {
        flags |= 0x10;
    }
    if facts.air_predicate {
        flags |= 0x40;
    }
    if facts.base_type_is_52_or_53 {
        flags |= 0x80;
    }
    flags
}

/// Produce the complete new-Unit pointer-array allocation and synchronized
/// `Guy::clear -> Guy::init_real(0)` prefix.
///
/// `Unit::Unit` changes the inherited `PtrArray<Guy>` increment from `-1` to `1`.
/// Starting from length/capacity zero, the pre-growth at `0x00612A7A` therefore reserves
/// exactly `squad_size + crew_size`, and all slots are non-null.  Each Guy consumes one
/// game-RNG draw in slot order.  Exact hierarchy receipts are mandatory because gpiece and
/// pivot fields lie inside the 155-byte Guy checksum image.
pub fn produce_unit_guy_init_prefix(
    inputs: UnitGuyInitInputs,
    rng_state: i32,
) -> Result<UnitGuyInitPrefixReceipt, UnitGuyInitError> {
    let identity = inputs.identity;
    if !(0..PLAYABLE_OWNER_SLOTS).contains(&identity.owner)
        || !(0..UNIT_BAND_LIMIT).contains(&identity.o)
        || identity.type_index < 0
        || identity.o > i16::MAX as i32
    {
        return Err(UnitGuyInitError::InvalidIdentity);
    }
    let total = inputs
        .squad_size
        .checked_add(inputs.crew_size)
        .ok_or(UnitGuyInitError::InvalidGuyCounts)?;
    if inputs.squad_size < 0
        || inputs.squad_size > i8::MAX as i32
        || inputs.crew_size < 0
        || !(0..=MAX_GUYS_PER_UNIT).contains(&total)
    {
        return Err(UnitGuyInitError::InvalidGuyCounts);
    }
    let total_usize = usize::try_from(total).map_err(|_| UnitGuyInitError::InvalidGuyCounts)?;
    if inputs.graphics.len() != total_usize || inputs.predicates.len() != total_usize {
        return Err(UnitGuyInitError::ReceiptCountMismatch);
    }

    for (slot, graphics) in inputs.graphics.iter().enumerate() {
        if graphics.provenance.executable_sha256 != SUPPORTED_RETAIL_EXE_SHA256 {
            return Err(UnitGuyInitError::UnsupportedExecutable { slot });
        }
        if graphics.provenance.installed_unit_graphics_sha256 != SUPPORTED_UNIT_GRAPHICS_SHA256 {
            return Err(UnitGuyInitError::UnsupportedGraphicsData { slot });
        }
        if !graphics.provenance.coherent_capture {
            return Err(UnitGuyInitError::IncoherentGraphicsCapture { slot });
        }
        if graphics.extracted.guy_num != slot as i8 {
            return Err(UnitGuyInitError::WrongGraphicsSlot { slot });
        }
        if graphics.extracted.gpiece < 0 {
            return Err(UnitGuyInitError::MissingGpiece { slot });
        }
        if graphics.restriction_count > 4
            || (graphics.restriction_count == 0) != graphics.extracted.pivot_graph_name.is_none()
        {
            return Err(UnitGuyInitError::InvalidRestrictionCount { slot });
        }
        if inputs.predicates[slot].has_pivot_restrictions != (graphics.restriction_count != 0) {
            return Err(UnitGuyInitError::GraphicsPredicateMismatch { slot });
        }
        let allowed = (1u16 << graphics.restriction_count) - 1;
        let pivot_flags =
            graphics.extracted.node_flags as u16 | graphics.extracted.des_node_flags as u16;
        if pivot_flags & !allowed != 0 {
            return Err(UnitGuyInitError::InvalidPivotFlags { slot });
        }
        if graphics.restriction_count == 0
            && (graphics.extracted.turret_angles != [0; 4]
                || graphics.extracted.des_turret_angles != [0; 4])
        {
            return Err(UnitGuyInitError::InvalidPivotFlags { slot });
        }
    }

    let mut rng = Random::new(rng_state);
    let mut random = Vec::with_capacity(total_usize);
    let mut stable_guys = Vec::with_capacity(total_usize);
    let mut slots = Vec::with_capacity(total_usize);
    for slot in 0..total_usize {
        let graphics = &inputs.graphics[slot];
        let predicates = inputs.predicates[slot];
        let mut guy = guy_clear_image();
        guy.ty = identity.type_index;
        guy.o = identity.o as i16;
        guy.who = identity.owner as i8;
        guy.guy_num = slot as i8;

        // `Guy::update_gpiece` is first in `init_real`; these are the synchronized outputs
        // of the already hash-gated hierarchy receipt.  The function later clears track
        // offsets before returning, so only gpiece and reset-pivot state survive this seam.
        guy.gpiece = graphics.extracted.gpiece;
        guy.turret_angles = graphics.extracted.turret_angles;
        guy.des_turret_angles = graphics.extracted.des_turret_angles;
        guy.node_flags = graphics.extracted.node_flags;
        guy.des_node_flags = graphics.extracted.des_node_flags;

        let state_before = rng.state();
        let returned = rng.get(0, 0xffff);
        let state_after = rng.state();
        let remainder = returned % 100;
        let selected_before_validation = selected_animation(remainder);
        let selected_after_validation =
            if predicates.valid_animation[selected_before_validation as usize] {
                selected_before_validation
            } else {
                0
            };

        guy.cur_anim = selected_after_validation;
        guy.cur_time = 0;
        guy.end_time = 0;
        guy.last_time = -1;
        guy.track_dx = 0;
        guy.track_dy = 0;
        guy.last_speed = 0;
        guy.avg_speed = 0;
        guy.stopped = 1;
        guy.hold_attack = 0;
        guy.queued_attack = 0;
        guy.guy_flags = guy_init_flags(predicates);
        // Unit::init writes this immediately after `init_real` returns.
        guy.variation = 0;

        random.push(GuyInitRandomReceipt {
            slot: slot as i32,
            call_va: GUY_INIT_RANDOM_CALL_VA,
            state_before,
            returned,
            remainder,
            selected_before_validation,
            selected_after_validation,
            state_after,
        });
        stable_guys.push(StableGuyIdentity {
            unit_id: identity.id,
            unit_generation: identity.generation,
            owner: identity.owner as i8,
            o: identity.o as i16,
            guy_num: slot as i8,
        });
        slots.push(Some(guy));
    }

    let guys = UnitGuys {
        guys: slots,
        size: total,
        increment: 1,
        flags: 0,
        guy_mark: inputs.squad_size as i8,
    };
    Ok(UnitGuyInitPrefixReceipt {
        identity,
        rng_initial: rng_state,
        rng_after_guys: rng.state(),
        array_length: total,
        array_capacity: total,
        array_increment: 1,
        array_flags: 0,
        guy_mark: inputs.squad_size as i8,
        stable_guys,
        random,
        guys,
        first_external_residual: UnitGuyExternalResidual::UpdateGpieceThenSetNewLocation {
            update_gpiece_va: UNIT_UPDATE_GPIECE_VA,
            set_new_location_va: UNIT_SET_NEW_LOCATION_VA,
        },
    })
}
