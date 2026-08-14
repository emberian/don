// SPDX-License-Identifier: GPL-3.0-or-later
//! Detached exact early-return transaction for the golden replay's first attrition cadence.
//!
//! Retail reaches `Unit::process_attrition` `0x005E11A0` from `Unit::process`
//! `0x006115EA..0x00611626` when `(Game::frame + UnitData::o) % 32 == 0`.  In the
//! supported replay the first seven calls are owner 0's `o6..o0`, at frames `26..=32`.
//! This module owns only the two prefixes whose first dynamic territory answer returns before
//! diplomacy, type, supply, damage, or RNG work:
//!
//! * `WData::who == 0` -- friendly territory, return at `0x005E12C5`;
//! * signed `WData::who < 0` and both live owner-0 `neutral_attrition` mirrors are zero --
//!   return at `0x005E12A5`.
//!
//! Foreign territory remains a typed boundary.  So do populated ScenarioData attrition-free
//! point arrays and every later supply/damage path.  The transaction is deliberately detached:
//! it is not called by `Sim::do_frame`.

#![forbid(unsafe_code)]

use crate::systems::borders_fog::{deobf, COORD_XOR};
use crate::systems::map_terrain::{Coord, WCoord, WorldChecksum};
use crate::systems::save_load::{save_sim, SaveError};
use crate::tick::Sim;
use crate::world::{Handle, OBJ_FLAG_ACTIVE};

pub const UNIT_PROCESS_VA: u32 = 0x0061_0bc0;
pub const ATTRITION_GATE_FIRST_VA: u32 = 0x0061_15ea;
pub const ATTRITION_GATE_CALL_VA: u32 = 0x0061_1612;
pub const ATTRITION_GATE_LAST_VA: u32 = 0x0061_1626;
pub const UNIT_PROCESS_ATTRITION_VA: u32 = 0x005e_11a0;
pub const FRIENDLY_TERRITORY_RETURN_VA: u32 = 0x005e_12c5;
pub const UNOWNED_ZERO_NEUTRAL_RETURN_VA: u32 = 0x005e_12a5;

pub const GOLDEN_OWNER: u8 = 0;
pub const FIRST_GOLDEN_ATTRITION_FRAME: i32 = 26;
pub const LAST_GOLDEN_ATTRITION_FRAME: i32 = 32;
pub const ATTRITION_PERIOD: i32 = 32;
pub const RESUPPLIED_THIS_TICK: u32 = 0x0004_0000;
pub const PROCESS_ATTRITION_CLEAR_MASK: u32 = 0x0040_0080;
pub const POST_ATTRITION_CLEAR_MASK2: u32 = 0x0000_0003;

/// The only source admitted by this bounded authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GoldenPhase32AttritionSource {
    /// A stopped supported-retail-process capture at the exact Unit attrition call gate,
    /// reconciled to the canonical `Sim` snapshot carried by the authority.
    SupportedRetailProcessAtUnitAttritionGate,
}

/// Stable identity of one reached Unit.  Dense row is intentionally absent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GoldenPhase32UnitIdentity {
    pub handle: Handle,
    pub who: u8,
    pub o: i16,
    pub uid: u16,
    pub type_index: i32,
}

/// Complete capture authority for one detached early-return transaction.
///
/// `canonical_sim_image` is the exact `save_sim` before-image admitted by the replay-side
/// capture binder.  The full terrain checksum image is retained independently because it is
/// the source of `WData::who`; the object-world digest and exact Unit before-image protect the
/// stable Handle join.  A cell-local fact without these whole-owner guards is not authority.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GoldenPhase32AttritionAuthority {
    pub revision: u64,
    pub composition_digest: [u8; 32],
    pub source: GoldenPhase32AttritionSource,
    pub canonical_sim_image: Vec<u8>,
    pub frame: i32,
    pub unit: GoldenPhase32UnitIdentity,
    /// Stored/XOR-obfuscated ObjectData position words.
    pub stored_x: i32,
    pub stored_y: i32,
    /// Decoded Coord position used by the WCoord conversion.
    pub x: i32,
    pub y: i32,
    pub wx: i32,
    pub wy: i32,
    pub cell_index: usize,
    pub territory_owner: i8,
    pub terrain_checksum: WorldChecksum,
    pub terrain_checksum_image: Vec<u8>,
    pub object_world_digest: u64,
    /// Canonical persistent owner: `Sim::vic_leaders[0].neutral_attrition`.
    pub victory_neutral_attrition: i32,
    /// Live checksum/tick duplicate: `Sim::step8.leaders[0].neutral_attrition`.
    pub step8_neutral_attrition: i32,
    /// The supported capture must prove the owner-0 ScenarioData attrition-free list empty.
    /// `don-sim` does not otherwise materialize that retail registry.
    pub scenario_attrition_free_points_empty: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GoldenPhase32AttritionCone {
    FriendlyTerritory,
    UnownedZeroNeutralAttrition,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GoldenPhase32AttritionBefore {
    pub unit: GoldenPhase32UnitIdentity,
    pub frame: i32,
    pub stored_x: i32,
    pub stored_y: i32,
    pub x: i32,
    pub y: i32,
    pub wx: i32,
    pub wy: i32,
    pub cell_index: usize,
    pub territory_owner: i8,
    pub unit_masks: u32,
    pub unit_masks2: u32,
    pub attrition: i16,
}

/// Exact write chronology across the caller and callee.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GoldenPhase32AttritionWrites {
    /// `0x00611609`, before the child call.
    pub unit_masks2_after_call_gate: u32,
    /// `0x005E11A6`, first child write.
    pub unit_masks_after_child_entry: u32,
    /// `0x005E11B2`, second child write.
    pub attrition_after_child_entry: i16,
    /// `0x00611617..0x00611626`, after the child returns.
    pub unit_masks2_after_child_return: u32,
}

#[derive(Clone, Debug)]
pub struct PreparedGoldenPhase32Attrition {
    authority: GoldenPhase32AttritionAuthority,
    before: GoldenPhase32AttritionBefore,
    writes: GoldenPhase32AttritionWrites,
    cone: GoldenPhase32AttritionCone,
}

impl PreparedGoldenPhase32Attrition {
    pub const fn before(&self) -> GoldenPhase32AttritionBefore {
        self.before
    }

    pub const fn writes(&self) -> GoldenPhase32AttritionWrites {
        self.writes
    }

    pub const fn cone(&self) -> GoldenPhase32AttritionCone {
        self.cone
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GoldenPhase32AttritionReceipt {
    pub authority_revision: u64,
    pub authority_digest: [u8; 32],
    pub before: GoldenPhase32AttritionBefore,
    pub writes: GoldenPhase32AttritionWrites,
    pub cone: GoldenPhase32AttritionCone,
    pub return_va: u32,
    pub object_world_digest_after: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GoldenPhase32AttritionError {
    MissingAuthorityRevision,
    MissingAuthorityDigest,
    MissingCanonicalSimImage,
    Snapshot(SaveError),
    CanonicalSimImageMismatch,
    UnsupportedGoldenFrame(i32),
    FrameMismatch {
        world: i32,
        authority: i32,
    },
    GameFrameMirrorMismatch {
        world: i32,
        victory: i32,
    },
    WrongGoldenActor {
        frame: i32,
        expected_o: i16,
        actual_o: i16,
    },
    WrongOwner(u8),
    MissingUnit(Handle),
    InactiveUnit(Handle),
    UnitIdentityMismatch,
    UnitTypeMismatch,
    PositionMismatch,
    InvalidMapDimensions {
        xs: i32,
        ys: i32,
        cells: usize,
    },
    PositionOutsideWorld {
        x: i32,
        y: i32,
        wx: i32,
        wy: i32,
    },
    CellMismatch,
    TerrainChecksumMismatch,
    TerrainChecksumImageMismatch,
    ObjectWorldDigestMismatch,
    NeutralAttritionMirrorMismatch {
        victory: i32,
        step8: i32,
    },
    NeutralAttritionNonZero(i32),
    ScenarioAttritionFreePoints,
    TerritoryOwnerMismatch {
        authority: i8,
        world: i8,
    },
    /// First unowned dynamic continuation. Diplomacy/type/supply/damage are deliberately not
    /// collapsed into a guessed return.
    ForeignTerritoryRequiresSelectionAndSupplyAuthority(i8),
    StalePreparedBeforeImage,
}

/// Golden actor reached at a given first-cadence frame.
pub const fn expected_golden_actor_o(frame: i32) -> Option<i16> {
    if frame >= FIRST_GOLDEN_ATTRITION_FRAME && frame <= LAST_GOLDEN_ATTRITION_FRAME {
        Some((LAST_GOLDEN_ATTRITION_FRAME - frame) as i16)
    } else {
        None
    }
}

fn checked_cell_count(xs: i32, ys: i32) -> Option<usize> {
    if xs <= 0 || ys <= 0 {
        return None;
    }
    usize::try_from(xs)
        .ok()?
        .checked_mul(usize::try_from(ys).ok()?)
}

fn current_before(
    sim: &Sim,
    authority: &GoldenPhase32AttritionAuthority,
) -> Result<(GoldenPhase32AttritionBefore, GoldenPhase32AttritionCone), GoldenPhase32AttritionError>
{
    if authority.revision == 0 {
        return Err(GoldenPhase32AttritionError::MissingAuthorityRevision);
    }
    if authority.composition_digest == [0; 32] {
        return Err(GoldenPhase32AttritionError::MissingAuthorityDigest);
    }
    if authority.canonical_sim_image.is_empty() {
        return Err(GoldenPhase32AttritionError::MissingCanonicalSimImage);
    }
    let snapshot = save_sim(sim).map_err(GoldenPhase32AttritionError::Snapshot)?;
    if snapshot != authority.canonical_sim_image {
        return Err(GoldenPhase32AttritionError::CanonicalSimImageMismatch);
    }
    let Some(expected_o) = expected_golden_actor_o(authority.frame) else {
        return Err(GoldenPhase32AttritionError::UnsupportedGoldenFrame(
            authority.frame,
        ));
    };
    if sim.world.frame != authority.frame {
        return Err(GoldenPhase32AttritionError::FrameMismatch {
            world: sim.world.frame,
            authority: authority.frame,
        });
    }
    if sim.vic_match.frame != sim.world.frame {
        return Err(GoldenPhase32AttritionError::GameFrameMirrorMismatch {
            world: sim.world.frame,
            victory: sim.vic_match.frame,
        });
    }
    if authority.unit.who != GOLDEN_OWNER {
        return Err(GoldenPhase32AttritionError::WrongOwner(authority.unit.who));
    }
    if authority.unit.o != expected_o {
        return Err(GoldenPhase32AttritionError::WrongGoldenActor {
            frame: authority.frame,
            expected_o,
            actual_o: authority.unit.o,
        });
    }
    let row =
        sim.world
            .row_of(authority.unit.handle)
            .ok_or(GoldenPhase32AttritionError::MissingUnit(
                authority.unit.handle,
            ))?;
    if sim.world.units.get_flags(row) & OBJ_FLAG_ACTIVE == 0 {
        return Err(GoldenPhase32AttritionError::InactiveUnit(
            authority.unit.handle,
        ));
    }
    if sim.world.units.get_who(row) != authority.unit.who
        || sim.world.units.o()[row] != authority.unit.o
        || sim.world.units.get_uid(row) != authority.unit.uid
        || sim
            .world
            .unit_row_at(i32::from(authority.unit.who), i32::from(authority.unit.o))
            != Some(row)
    {
        return Err(GoldenPhase32AttritionError::UnitIdentityMismatch);
    }
    if sim.world.unit_type_id(row) != Some(authority.unit.type_index) {
        return Err(GoldenPhase32AttritionError::UnitTypeMismatch);
    }
    let stored_x = sim.world.units.x_internal()[row];
    let stored_y = sim.world.units.y_internal()[row];
    let x = deobf(stored_x as u32);
    let y = deobf(stored_y as u32);
    if (stored_x, stored_y, x, y)
        != (
            authority.stored_x,
            authority.stored_y,
            authority.x,
            authority.y,
        )
    {
        return Err(GoldenPhase32AttritionError::PositionMismatch);
    }

    let map = &sim.map.world;
    let Some(cell_count) = checked_cell_count(map.xs, map.ys) else {
        return Err(GoldenPhase32AttritionError::InvalidMapDimensions {
            xs: map.xs,
            ys: map.ys,
            cells: map.wdata.len(),
        });
    };
    if cell_count != map.wdata.len() {
        return Err(GoldenPhase32AttritionError::InvalidMapDimensions {
            xs: map.xs,
            ys: map.ys,
            cells: map.wdata.len(),
        });
    }
    let wx = WCoord::from_coord(Coord(x)).0;
    let wy = WCoord::from_coord(Coord(y)).0;
    if !map.valid_w(wx, wy) {
        return Err(GoldenPhase32AttritionError::PositionOutsideWorld { x, y, wx, wy });
    }
    let cell_index = map.w_index(wx, wy);
    if (wx, wy, cell_index) != (authority.wx, authority.wy, authority.cell_index) {
        return Err(GoldenPhase32AttritionError::CellMismatch);
    }
    if map.checksum_sections() != authority.terrain_checksum {
        return Err(GoldenPhase32AttritionError::TerrainChecksumMismatch);
    }
    if map.checksum_image().0 != authority.terrain_checksum_image {
        return Err(GoldenPhase32AttritionError::TerrainChecksumImageMismatch);
    }
    if sim.world.digest() != authority.object_world_digest {
        return Err(GoldenPhase32AttritionError::ObjectWorldDigestMismatch);
    }
    let victory_neutral = sim.vic_leaders.slots[GOLDEN_OWNER as usize].neutral_attrition;
    let step8_neutral = sim.step8.leaders[GOLDEN_OWNER as usize].neutral_attrition;
    if victory_neutral != authority.victory_neutral_attrition
        || step8_neutral != authority.step8_neutral_attrition
        || victory_neutral != step8_neutral
    {
        return Err(
            GoldenPhase32AttritionError::NeutralAttritionMirrorMismatch {
                victory: victory_neutral,
                step8: step8_neutral,
            },
        );
    }
    if victory_neutral != 0 {
        return Err(GoldenPhase32AttritionError::NeutralAttritionNonZero(
            victory_neutral,
        ));
    }
    if !authority.scenario_attrition_free_points_empty {
        return Err(GoldenPhase32AttritionError::ScenarioAttritionFreePoints);
    }
    let territory_owner = map.wdata[cell_index].who;
    if territory_owner != authority.territory_owner {
        return Err(GoldenPhase32AttritionError::TerritoryOwnerMismatch {
            authority: authority.territory_owner,
            world: territory_owner,
        });
    }
    let cone = if territory_owner == GOLDEN_OWNER as i8 {
        GoldenPhase32AttritionCone::FriendlyTerritory
    } else if territory_owner < 0 {
        GoldenPhase32AttritionCone::UnownedZeroNeutralAttrition
    } else {
        return Err(
            GoldenPhase32AttritionError::ForeignTerritoryRequiresSelectionAndSupplyAuthority(
                territory_owner,
            ),
        );
    };

    Ok((
        GoldenPhase32AttritionBefore {
            unit: authority.unit,
            frame: authority.frame,
            stored_x,
            stored_y,
            x,
            y,
            wx,
            wy,
            cell_index,
            territory_owner,
            unit_masks: sim.world.units.get_unit_masks(row),
            unit_masks2: sim.world.units.get_unit_masks2(row),
            attrition: sim.world.units.attrition()[row],
        },
        cone,
    ))
}

/// Plan the complete bounded write sequence without changing `sim`.
pub fn prepare_golden_phase32_attrition(
    sim: &Sim,
    authority: &GoldenPhase32AttritionAuthority,
) -> Result<PreparedGoldenPhase32Attrition, GoldenPhase32AttritionError> {
    let (before, cone) = current_before(sim, authority)?;
    let at_gate = before.unit_masks2 & !RESUPPLIED_THIS_TICK;
    let writes = GoldenPhase32AttritionWrites {
        unit_masks2_after_call_gate: at_gate,
        unit_masks_after_child_entry: before.unit_masks & !PROCESS_ATTRITION_CLEAR_MASK,
        attrition_after_child_entry: 0,
        unit_masks2_after_child_return: at_gate & !POST_ATTRITION_CLEAR_MASK2,
    };
    Ok(PreparedGoldenPhase32Attrition {
        authority: authority.clone(),
        before,
        writes,
        cone,
    })
}

/// Atomically publish a previously prepared early-return transaction.
///
/// Every fallible check, including equality with the complete canonical Sim before-image, runs
/// before the first write.  The four stores below are then infallible and retain retail's
/// caller/child/caller chronology.
pub fn commit_golden_phase32_attrition(
    sim: &mut Sim,
    prepared: PreparedGoldenPhase32Attrition,
) -> Result<GoldenPhase32AttritionReceipt, GoldenPhase32AttritionError> {
    let (current, cone) = current_before(sim, &prepared.authority)?;
    if current != prepared.before || cone != prepared.cone {
        return Err(GoldenPhase32AttritionError::StalePreparedBeforeImage);
    }
    let row = sim
        .world
        .row_of(prepared.before.unit.handle)
        .ok_or(GoldenPhase32AttritionError::StalePreparedBeforeImage)?;

    sim.world
        .units
        .set_unit_masks2(row, prepared.writes.unit_masks2_after_call_gate);
    sim.world
        .units
        .set_unit_masks(row, prepared.writes.unit_masks_after_child_entry);
    sim.world.units.attrition_mut()[row] = prepared.writes.attrition_after_child_entry;
    sim.world
        .units
        .set_unit_masks2(row, prepared.writes.unit_masks2_after_child_return);

    let return_va = match prepared.cone {
        GoldenPhase32AttritionCone::FriendlyTerritory => FRIENDLY_TERRITORY_RETURN_VA,
        GoldenPhase32AttritionCone::UnownedZeroNeutralAttrition => UNOWNED_ZERO_NEUTRAL_RETURN_VA,
    };
    Ok(GoldenPhase32AttritionReceipt {
        authority_revision: prepared.authority.revision,
        authority_digest: prepared.authority.composition_digest,
        before: prepared.before,
        writes: prepared.writes,
        cone: prepared.cone,
        return_va,
        object_world_digest_after: sim.world.digest(),
    })
}

/// Encode a decoded Coord the same way ObjectData stores it.  Public for capture/test adapters.
#[inline]
pub const fn store_object_coord(coord: i32) -> i32 {
    (coord as u32 ^ COORD_XOR) as i32
}
