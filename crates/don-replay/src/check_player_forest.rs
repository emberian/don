// SPDX-License-Identifier: GPL-3.0-or-later
//! Replay adapter for `Map::check_player_forest` (`0x0068e8f0`).
//!
//! This is the first World-mutating call after `TerrainGroups::place_all` in
//! the ordinary `Map::make` path. It consumes no RNG. Its only persistent
//! writes are land-class updates in checksum section 5 (`WData`).

use crate::fractal_boundary::TERRAIN_GROUPS_PLACE_ALL_VA;
use crate::initial::{InitialItemBoundary, InitialItemReconstruction, InitialWorld};
use crate::place_all_boundary::{ReplayPlaceAllReceipt, TERRAIN_GROUPS_PLACE_ALL_RETURN_VA};
use crate::rules_channel::RETAIL_AFTER_CONSTANTS;
use don_sim::systems::combat::{circle_table, CIRCLE_MAX_RING};
use don_sim::systems::map_terrain::{wflag, World, WorldChecksum, WorldSection};
use don_sim::systems::tech_cities::CityRules;

/// `Map::make` call instruction.
pub const MAP_CHECK_PLAYER_FOREST_CALL_VA: u32 = 0x0068_c04d;
/// `Map::check_player_forest` entry.
pub const MAP_CHECK_PLAYER_FOREST_VA: u32 = 0x0068_e8f0;
/// Final instruction of the callee.
pub const MAP_CHECK_PLAYER_FOREST_RET_VA: u32 = 0x0068_eeff;
/// Half-open callee end.
pub const MAP_CHECK_PLAYER_FOREST_END_VA: u32 = 0x0068_ef00;
/// `Map::make` resumes here after the call.
pub const MAP_CHECK_PLAYER_FOREST_CALLER_RESUME_VA: u32 = 0x0068_c052;
/// The next World mutator and the next consumer of the main RNG.
pub const TERRAIN_GROUPS_NUBIFY_FOREST_VA: u32 = 0x006a_93b0;

/// Shipped `Constants::city_center_radius` at PDB offset `+0x12c`.
pub const SHIPPED_CITY_CENTER_RADIUS: i32 = 20;

const CARDINAL_DX: [i32; 4] = [0, 1, 0, -1];
const CARDINAL_DY: [i32; 4] = [-1, 0, 1, 0];

/// Static and caller-branch facts which an ordinary `.rcx` does not serialize
/// next to the generated World.
///
/// The constructor admits the radius only behind the replay's independently
/// reproduced shipped-Constants checkpoint. `semaphore_bit9_set` is retained
/// because `Map::make` skips both `place_all` and this call when that bit is set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CheckPlayerForestFacts {
    pub city_center_radius: i32,
    pub rules_after_constants: u32,
    pub semaphore_bit9_set: bool,
}

impl CheckPlayerForestFacts {
    pub fn from_admitted_replay_rules(
        plan: &InitialItemReconstruction,
        semaphore_bit9_set: bool,
    ) -> Result<Self, CheckPlayerForestError> {
        let rules = plan
            .rules
            .ok_or(CheckPlayerForestError::StaticRulesUnavailable)?;
        if rules.after_constants != RETAIL_AFTER_CONSTANTS {
            return Err(CheckPlayerForestError::RulesConstantsCheckpointMismatch {
                expected: RETAIL_AFTER_CONSTANTS,
                actual: rules.after_constants,
            });
        }
        Ok(Self {
            city_center_radius: CityRules::RETAIL.city_center_radius,
            rules_after_constants: rules.after_constants,
            semaphore_bit9_set,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CheckPlayerForestError {
    Blocked {
        boundary: InitialItemBoundary,
    },
    StaticRulesUnavailable,
    RulesConstantsCheckpointMismatch {
        expected: u32,
        actual: u32,
    },
    CityCenterRadiusMismatch {
        expected: i32,
        actual: i32,
    },
    CallerBranchSkipped,
    PlaceAllReceiptMismatch,
    MapStyleMismatch {
        replay_map_style: u8,
        receipt_map_style: u8,
    },
    GeneratedStartCountMismatch {
        receipt_starts: usize,
        start_x: usize,
        start_y: usize,
    },
    WorldShapeMismatch {
        xs: i32,
        ys: i32,
        size: i32,
        wdata: usize,
        start_city_locs: usize,
    },
    StoredChecksumMismatch,
    InvalidCircleRing {
        city_center_radius: i32,
        ring: i32,
    },
    UnexpectedChecksumSections {
        sections: Vec<WorldSection>,
    },
}

/// One successful exact-size patch in native append order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForestPatch {
    pub start_index: usize,
    pub anchor: (i32, i32),
    pub requested_cells: u8,
    /// Anchor, followed by the earliest admissible N/E/S/W cells.
    pub cells: Vec<(i32, i32)>,
}

/// Per-start decision in start-array order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlayerForestResult {
    AlreadySufficient {
        start_index: usize,
        existing_forests: usize,
    },
    Patched(ForestPatch),
    NoPatch {
        start_index: usize,
        existing_forests: usize,
    },
}

/// Typed handoff from the deterministic repair into `nubify_forest`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckPlayerForestReceipt {
    pub entry_va: u32,
    pub return_va: u32,
    pub caller_resume_va: u32,
    pub next_va: u32,
    pub city_center_radius: i32,
    pub circle_ring: usize,
    /// Half-open canonical-circle range used to count existing forest.
    pub existing_forest_scan: (usize, usize),
    /// Half-open canonical-circle range used for candidate anchors.
    pub candidate_anchor_scan: (usize, usize),
    pub starts: Vec<PlayerForestResult>,
    pub cells_changed: usize,
    pub random_state_before: i32,
    pub random_state_after: i32,
    pub random_draws: u32,
    /// Logical final capacity of each native temporary `Array<WCoord>`.
    /// Native growth is `0 -> 4 -> 8` and lengths are cleared between anchors.
    pub temporary_array_capacity: i32,
    pub checksum_before: WorldChecksum,
    pub checksum_after: WorldChecksum,
    /// No caller-supplied or inferred byte is promoted to replay-sourced data.
    pub sourced_walked_bytes: u64,
}

/// Parallel native `Array<WCoord>` scratch objects.
///
/// Allocation addresses and freed storage are not checksummed, but length
/// clearing, retained capacity, growth, and append order determine the native
/// control flow. Track those semantics explicitly instead of relying on
/// `Vec`'s host-specific capacity policy.
#[derive(Clone, Debug, Default)]
struct RetailCoordArrays {
    x: Vec<i32>,
    y: Vec<i32>,
    x_capacity: i32,
    y_capacity: i32,
}

impl RetailCoordArrays {
    fn clear(&mut self) {
        self.x.clear();
        self.y.clear();
    }

    fn grow(capacity: &mut i32, length: usize) {
        if length as i32 >= *capacity {
            *capacity = if *capacity == 0 {
                4
            } else {
                (*capacity).wrapping_add(*capacity)
            };
        }
    }

    fn push(&mut self, x: i32, y: i32) {
        Self::grow(&mut self.x_capacity, self.x.len());
        self.x.push(x);
        Self::grow(&mut self.y_capacity, self.y.len());
        self.y.push(y);
    }

    fn len(&self) -> usize {
        debug_assert_eq!(self.x.len(), self.y.len());
        self.x.len()
    }

    fn coords(&self) -> Vec<(i32, i32)> {
        self.x.iter().copied().zip(self.y.iter().copied()).collect()
    }

    fn capacity(&self) -> i32 {
        debug_assert_eq!(self.x_capacity, self.y_capacity);
        self.x_capacity
    }
}

fn validate_world_shape(world: &World) -> Result<(), CheckPlayerForestError> {
    let cells = world.xs.checked_mul(world.ys).filter(|n| *n >= 0);
    let expected_wdata = cells.map(|n| n as usize);
    let expected_mask = cells.map(|n| (n as usize).saturating_add(7) / 8);
    if world.xs < 0
        || world.ys < 0
        || cells != Some(world.size)
        || expected_wdata != Some(world.wdata.len())
        || expected_mask != Some(world.start_city_locs.len())
    {
        return Err(CheckPlayerForestError::WorldShapeMismatch {
            xs: world.xs,
            ys: world.ys,
            size: world.size,
            wdata: world.wdata.len(),
            start_city_locs: world.start_city_locs.len(),
        });
    }
    Ok(())
}

#[inline]
fn start_city_wcoord(world: &World, x: i32, y: i32) -> bool {
    let index = y * world.xs + x;
    world.start_city_locs[(index >> 3) as usize] & (1u8 << ((index & 7) as u32)) != 0
}

#[inline]
fn admissible(world: &World, x: i32, y: i32) -> bool {
    world.valid_w(x, y)
        && world.is_flat(x, y)
        && !start_city_wcoord(world, x, y)
        && !world.is_coast(x, y)
}

fn set_forest(world: &mut World, x: i32, y: i32) {
    let old = world.wdata(x, y);
    let land = if old.flags & wflag::COAST != 0 {
        2
    } else {
        old.land as i32
    };
    let land_sub = old.land_sub as i32;
    world.set_land(x, y, land, land_sub, wflag::FOREST as i32, false);
}

/// Execute the native forest repair transaction after a successful matching
/// `TerrainGroups::place_all` receipt.
///
/// All validation and mutation occur against a staged World. Any error leaves
/// `map.world`, its stored checksum, RNG handoff, and sourced-byte accounting
/// unchanged.
pub fn execute_check_player_forest(
    plan: &InitialItemReconstruction,
    map: &mut InitialWorld,
    place_all: &ReplayPlaceAllReceipt,
    facts: CheckPlayerForestFacts,
) -> Result<CheckPlayerForestReceipt, CheckPlayerForestError> {
    match plan.boundary {
        InitialItemBoundary::MapTerrainGroupsPlaceAllUnavailable { next_va }
            if next_va == TERRAIN_GROUPS_PLACE_ALL_VA => {}
        boundary => return Err(CheckPlayerForestError::Blocked { boundary }),
    }
    let rules = plan
        .rules
        .ok_or(CheckPlayerForestError::StaticRulesUnavailable)?;
    if rules.after_constants != RETAIL_AFTER_CONSTANTS {
        return Err(CheckPlayerForestError::RulesConstantsCheckpointMismatch {
            expected: RETAIL_AFTER_CONSTANTS,
            actual: rules.after_constants,
        });
    }
    if facts.semaphore_bit9_set {
        return Err(CheckPlayerForestError::CallerBranchSkipped);
    }
    if facts.rules_after_constants != RETAIL_AFTER_CONSTANTS {
        return Err(CheckPlayerForestError::RulesConstantsCheckpointMismatch {
            expected: RETAIL_AFTER_CONSTANTS,
            actual: facts.rules_after_constants,
        });
    }
    if facts.city_center_radius != SHIPPED_CITY_CENTER_RADIUS
        || facts.city_center_radius != CityRules::RETAIL.city_center_radius
    {
        return Err(CheckPlayerForestError::CityCenterRadiusMismatch {
            expected: SHIPPED_CITY_CENTER_RADIUS,
            actual: facts.city_center_radius,
        });
    }
    if place_all.entry_va != TERRAIN_GROUPS_PLACE_ALL_VA
        || place_all.return_va != TERRAIN_GROUPS_PLACE_ALL_RETURN_VA
        || place_all.return_value != 1
        || place_all.tdata_cells != map.world.tdata.len()
        || place_all.sourced_walked_bytes != map.sourced_walked_bytes
    {
        return Err(CheckPlayerForestError::PlaceAllReceiptMismatch);
    }
    if place_all.map_style != plan.inputs.map_style {
        return Err(CheckPlayerForestError::MapStyleMismatch {
            replay_map_style: plan.inputs.map_style,
            receipt_map_style: place_all.map_style,
        });
    }

    validate_world_shape(&map.world)?;
    let start_x = map.world.start_x.items.len();
    let start_y = map.world.start_y.items.len();
    if start_x != start_y || start_x != place_all.generated_starts {
        return Err(CheckPlayerForestError::GeneratedStartCountMismatch {
            receipt_starts: place_all.generated_starts,
            start_x,
            start_y,
        });
    }
    let recomputed_before = map.world.checksum_sections();
    if map.checksum != recomputed_before || map.checksum != place_all.checksum_after {
        return Err(CheckPlayerForestError::StoredChecksumMismatch);
    }

    // The machine sequence implements signed truncation toward zero before
    // subtracting one. Rust's signed integer division has the same rule.
    let ring = facts.city_center_radius / 4 - 1;
    if !(0..=CIRCLE_MAX_RING as i32).contains(&ring) {
        return Err(CheckPlayerForestError::InvalidCircleRing {
            city_center_radius: facts.city_center_radius,
            ring,
        });
    }
    let ring = ring as usize;
    let circle = circle_table();
    let existing_forest_scan = (circle.ring_end[0] as usize, circle.ring_end[ring] as usize);
    let candidate_anchor_scan = (circle.ring_end[2] as usize, circle.ring_end[ring] as usize);

    let checksum_before = map.checksum.clone();
    let mut staged = map.world.clone();
    let mut scratch = RetailCoordArrays::default();
    let mut starts = Vec::with_capacity(start_x);
    let mut cells_changed = 0usize;

    for start_index in 0..start_x {
        let sx = staged.start_x.items[start_index];
        let sy = staged.start_y.items[start_index];
        let existing_forests = (existing_forest_scan.0..existing_forest_scan.1)
            .filter(|&circle_index| {
                let x = sx.wrapping_add(circle.x[circle_index] as i32);
                let y = sy.wrapping_add(circle.y[circle_index] as i32);
                staged.valid_w(x, y) && staged.wdata(x, y).flags & wflag::FOREST != 0
            })
            .count();
        if existing_forests >= 3 {
            starts.push(PlayerForestResult::AlreadySufficient {
                start_index,
                existing_forests,
            });
            continue;
        }

        let mut selected = None;
        'requested: for requested_cells in (1usize..=3).rev() {
            for circle_index in candidate_anchor_scan.0..candidate_anchor_scan.1 {
                let anchor_x = sx.wrapping_add(circle.x[circle_index] as i32);
                let anchor_y = sy.wrapping_add(circle.y[circle_index] as i32);
                if !admissible(&staged, anchor_x, anchor_y) {
                    continue;
                }

                scratch.clear();
                scratch.push(anchor_x, anchor_y);
                for direction in 0..4 {
                    let x = anchor_x.wrapping_add(CARDINAL_DX[direction]);
                    let y = anchor_y.wrapping_add(CARDINAL_DY[direction]);
                    if admissible(&staged, x, y) {
                        scratch.push(x, y);
                    }

                    // Retail checks after every attempted direction, including
                    // an inadmissible one. There is no pre-neighbour test and
                    // no subset selection after all four directions.
                    if scratch.len() == requested_cells {
                        let cells = scratch.coords();
                        for &(forest_x, forest_y) in &cells {
                            set_forest(&mut staged, forest_x, forest_y);
                        }
                        cells_changed += cells.len();
                        selected = Some(ForestPatch {
                            start_index,
                            anchor: (anchor_x, anchor_y),
                            requested_cells: requested_cells as u8,
                            cells,
                        });
                        break 'requested;
                    }
                }
            }
        }

        starts.push(match selected {
            Some(patch) => PlayerForestResult::Patched(patch),
            None => PlayerForestResult::NoPatch {
                start_index,
                existing_forests,
            },
        });
    }

    let checksum_after = staged.checksum_sections();
    let sections = checksum_before.differing_sections(&checksum_after);
    if sections
        .iter()
        .any(|section| *section != WorldSection::WData)
    {
        return Err(CheckPlayerForestError::UnexpectedChecksumSections { sections });
    }

    map.world = staged;
    map.checksum = checksum_after.clone();
    Ok(CheckPlayerForestReceipt {
        entry_va: MAP_CHECK_PLAYER_FOREST_VA,
        return_va: MAP_CHECK_PLAYER_FOREST_RET_VA,
        caller_resume_va: MAP_CHECK_PLAYER_FOREST_CALLER_RESUME_VA,
        next_va: TERRAIN_GROUPS_NUBIFY_FOREST_VA,
        city_center_radius: facts.city_center_radius,
        circle_ring: ring,
        existing_forest_scan,
        candidate_anchor_scan,
        starts,
        cells_changed,
        random_state_before: place_all.random_state_after,
        random_state_after: place_all.random_state_after,
        random_draws: 0,
        temporary_array_capacity: scratch.capacity(),
        checksum_before,
        checksum_after,
        sourced_walked_bytes: map.sourced_walked_bytes,
    })
}
