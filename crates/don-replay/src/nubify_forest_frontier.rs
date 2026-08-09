// SPDX-License-Identifier: GPL-3.0-or-later
//! Source-only replay frontier for `TerrainGroups::nubify_forest` (`0x006a93b0`).
//!
//! The scan, main-RNG draws, candidate gates, WData writes, temporary-array behaviour, and
//! checksum transaction are instruction-derived.  The first unresolved callee is deliberately
//! left external: every `World::is_edge_of_region` (`0x006b3180`) result must arrive in a typed
//! receipt bound to the exact staged World checksum and call request.

use crate::check_player_forest::{
    CheckPlayerForestReceipt, MAP_CHECK_PLAYER_FOREST_CALLER_RESUME_VA,
    MAP_CHECK_PLAYER_FOREST_RET_VA, MAP_CHECK_PLAYER_FOREST_VA, TERRAIN_GROUPS_NUBIFY_FOREST_VA,
};
use crate::initial::InitialWorld;
use don_sim::rng::Random;
use don_sim::systems::map_terrain::{land, wflag, World, WorldChecksum, WorldSection};

pub const MAP_NUBIFY_FOREST_CALL_VA: u32 = 0x0068_c08b;
pub const MAP_NUBIFY_FOREST_CALLER_RESUME_VA: u32 = 0x0068_c090;
pub const MAP_POST_NUBIFY_CHECKSUM_CALL_VA: u32 = 0x0068_c0b4;
pub const MAP_POST_NUBIFY_CHECKSUM_SOURCE_TOKEN: u32 = 0x1eb9;
pub const TERRAIN_GROUPS_NUBIFY_FOREST_RET_VA: u32 = 0x006a_99b9;
/// Half-open PDB extent, including cold direction-selection blocks after the hot return.
pub const TERRAIN_GROUPS_NUBIFY_FOREST_END_VA: u32 = 0x006a_9a51;
pub const WORLD_IS_EDGE_OF_REGION_VA: u32 = 0x006b_3180;
pub const COLUMN_OFFSET_RANDOM_VA: u32 = 0x006a_952e;
pub const ROW_OFFSET_RANDOM_VA: u32 = 0x006a_9783;
pub const ROW_DIRECTION_RANDOM_VA: u32 = 0x006a_9a0f;
pub const RANDOM_GET_VA: u32 = 0x00a3_9d70;
pub const TERRAIN_GROUPS_CHANGE_FOREST_BASE_VA: u32 = 0x006a_9a60;
pub const TERRAIN_GROUPS_CHANGE_MOUNTAIN_BASE_VA: u32 = 0x006a_9c10;
pub const TERRAIN_GROUPS_CHANGE_COAST_BASE_VA: u32 = 0x006a_9dc0;
pub const TERRAIN_GROUPS_FIX_TRANSITIONS_VA: u32 = 0x006a_9f70;

pub const COLUMN_EDGE_CALL_VA: u32 = 0x006a_95d0;
pub const ROW_EDGE_CALL_VA: u32 = 0x006a_982d;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NubifyPass {
    /// Outer X, inner Y; east/west neighbours select the side.
    Column,
    /// Outer Y, inner X; south/north neighbours select the side.
    Row,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NubifyRandomUse {
    OffsetWithinFourCellRun,
    RowDirectionTie,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NubifyRandomDraw {
    pub call_va: u32,
    pub random_va: u32,
    pub pass: NubifyPass,
    pub use_: NubifyRandomUse,
    pub scan_x: i32,
    pub scan_y: i32,
    pub state_before: i32,
    pub raw: i32,
    /// Offset `0..=3`, or direction `-1/+1`, according to `use_`.
    pub derived: i32,
    pub state_after: i32,
}

/// Exact opaque-callee request at `0x006a95d0` or `0x006a982d`.
///
/// `world_checksum` binds the answer to the evolving staged WData.  The non-walked start-city
/// mask is copied as well so a receipt cannot be replayed across a different candidate gate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EdgeOfRegionRequest {
    pub call_va: u32,
    pub callee_va: u32,
    pub ordinal: usize,
    pub pass: NubifyPass,
    pub scan_x: i32,
    pub scan_y: i32,
    pub candidate_x: i32,
    pub candidate_y: i32,
    pub arg_zero: i32,
    pub arg_region: i32,
    pub world_checksum: WorldChecksum,
    pub start_city_locs: Vec<u8>,
}

/// Typed answer from the deliberately unresolved World callee.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EdgeOfRegionReceipt {
    pub request: EdgeOfRegionRequest,
    /// Retail rejects the candidate on every nonzero result.
    pub result: i32,
    pub provenance: EdgeOfRegionProvenance,
}

/// Typed evidence class for an opaque-callee result.
///
/// Hash fields are evidence carried by the caller, not cryptographic verification performed by
/// this adapter. `SyntheticFixture` is admitted only while this source file is compiled with
/// `cfg(test)` and cannot cross a production integration boundary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EdgeOfRegionProvenance {
    RetailCapture {
        retail_executable_sha256: [u8; 32],
        capture_sha256: [u8; 32],
    },
    ExactPort {
        implementation_sha256: [u8; 32],
        proof_document: String,
    },
    SyntheticFixture {
        fixture: String,
    },
}

impl EdgeOfRegionProvenance {
    fn is_admissible(&self) -> bool {
        let nonzero = |digest: &[u8; 32]| digest.iter().any(|byte| *byte != 0);
        match self {
            Self::RetailCapture {
                retail_executable_sha256,
                capture_sha256,
            } => nonzero(retail_executable_sha256) && nonzero(capture_sha256),
            Self::ExactPort {
                implementation_sha256,
                proof_document,
            } => nonzero(implementation_sha256) && !proof_document.is_empty(),
            Self::SyntheticFixture { fixture } => cfg!(test) && !fixture.is_empty(),
        }
    }
}

/// Boundary implemented by shared convergence, not by this source pack.
pub trait EdgeOfRegionHost {
    fn is_edge_of_region(
        &mut self,
        world: &World,
        request: &EdgeOfRegionRequest,
    ) -> Option<EdgeOfRegionReceipt>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NubifyForestWrite {
    pub pass: NubifyPass,
    pub scan_x: i32,
    pub scan_y: i32,
    pub candidate_x: i32,
    pub candidate_y: i32,
    pub edge_receipt_ordinal: usize,
    pub old_flags: u16,
    pub old_land: i8,
    pub old_land_sub: u8,
    pub new_flags: u16,
    pub new_land: i8,
    pub new_land_sub: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NubifyForestReceipt {
    pub call_va: u32,
    pub entry_va: u32,
    pub return_va: u32,
    pub caller_resume_va: u32,
    /// The sole stack parameter is compiler-supplied but never read by the retail body.
    pub stack_argument_read: bool,
    pub random_state_before: i32,
    pub random_state_after: i32,
    pub random_draws: Vec<NubifyRandomDraw>,
    pub edge_receipts: Vec<EdgeOfRegionReceipt>,
    pub writes: Vec<NubifyForestWrite>,
    pub cells_changed: usize,
    /// Both parallel native `SimpleArray<int>` objects have this final logical capacity.
    pub temporary_array_capacity: i32,
    pub checksum_before: WorldChecksum,
    pub checksum_after: WorldChecksum,
    pub sourced_walked_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NubifyForestError {
    CheckPlayerForestReceiptMismatch,
    StoredChecksumMismatch,
    WorldShapeMismatch {
        xs: i32,
        ys: i32,
        size: i32,
        wdata: usize,
        start_city_locs: usize,
    },
    EdgeReceiptUnavailable {
        request: EdgeOfRegionRequest,
    },
    EdgeReceiptMismatch {
        expected: EdgeOfRegionRequest,
        observed: EdgeOfRegionRequest,
    },
    EdgeReceiptInvalidProvenance {
        request: EdgeOfRegionRequest,
    },
    UnexpectedChecksumSections {
        sections: Vec<WorldSection>,
    },
}

#[derive(Clone, Debug, Default)]
struct RetailCoordArrays {
    x: Vec<i32>,
    y: Vec<i32>,
    x_capacity: i32,
    y_capacity: i32,
}

impl RetailCoordArrays {
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

    fn capacity(&self) -> i32 {
        debug_assert_eq!(self.x.len(), self.y.len());
        debug_assert_eq!(self.x_capacity, self.y_capacity);
        self.x_capacity
    }
}

fn validate_world_shape(world: &World) -> Result<(), NubifyForestError> {
    let cells = world.xs.checked_mul(world.ys).filter(|cells| *cells >= 0);
    let expected_wdata = cells.map(|cells| cells as usize);
    let expected_mask = expected_wdata.map(|cells| cells.saturating_add(7) / 8);
    if world.xs < 0
        || world.ys < 0
        || cells != Some(world.size)
        || expected_wdata != Some(world.wdata.len())
        || expected_mask != Some(world.start_city_locs.len())
    {
        return Err(NubifyForestError::WorldShapeMismatch {
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
fn is_forest(world: &World, x: i32, y: i32) -> bool {
    world.wdata(x, y).flags & wflag::FOREST != 0
}

#[inline]
fn start_city_wcoord(world: &World, x: i32, y: i32) -> bool {
    let index = y * world.xs + x;
    world.start_city_locs[(index >> 3) as usize] & (1u8 << ((index & 7) as u32)) != 0
}

fn draw(
    random: &mut Random,
    draws: &mut Vec<NubifyRandomDraw>,
    call_va: u32,
    pass: NubifyPass,
    use_: NubifyRandomUse,
    scan_x: i32,
    scan_y: i32,
) -> i32 {
    let state_before = random.state();
    let raw = random.get(0, 0xffff);
    let derived = match use_ {
        NubifyRandomUse::OffsetWithinFourCellRun => raw & 3,
        NubifyRandomUse::RowDirectionTie => {
            if raw & 1 == 0 {
                -1
            } else {
                1
            }
        }
    };
    draws.push(NubifyRandomDraw {
        call_va,
        random_va: RANDOM_GET_VA,
        pass,
        use_,
        scan_x,
        scan_y,
        state_before,
        raw,
        derived,
        state_after: random.state(),
    });
    derived
}

#[allow(clippy::too_many_arguments)]
fn try_candidate<H: EdgeOfRegionHost>(
    staged: &mut World,
    host: &mut H,
    pass: NubifyPass,
    scan_x: i32,
    scan_y: i32,
    candidate_x: i32,
    candidate_y: i32,
    edge_receipts: &mut Vec<EdgeOfRegionReceipt>,
    writes: &mut Vec<NubifyForestWrite>,
    scratch: &mut RetailCoordArrays,
    cells_changed: &mut usize,
) -> Result<(), NubifyForestError> {
    if start_city_wcoord(staged, candidate_x, candidate_y) {
        return Ok(());
    }
    let candidate = staged.wdata(candidate_x, candidate_y);
    if candidate.flags & wflag::WATERHALF == 0
        && matches!(candidate.land, land::COASTAL | land::OCEAN)
    {
        return Ok(());
    }

    let request = EdgeOfRegionRequest {
        call_va: match pass {
            NubifyPass::Column => COLUMN_EDGE_CALL_VA,
            NubifyPass::Row => ROW_EDGE_CALL_VA,
        },
        callee_va: WORLD_IS_EDGE_OF_REGION_VA,
        ordinal: edge_receipts.len(),
        pass,
        scan_x,
        scan_y,
        candidate_x,
        candidate_y,
        arg_zero: 0,
        arg_region: 0xffff,
        world_checksum: staged.checksum_sections(),
        start_city_locs: staged.start_city_locs.clone(),
    };
    let Some(receipt) = host.is_edge_of_region(staged, &request) else {
        return Err(NubifyForestError::EdgeReceiptUnavailable { request });
    };
    if receipt.request != request {
        return Err(NubifyForestError::EdgeReceiptMismatch {
            expected: request,
            observed: receipt.request,
        });
    }
    if !receipt.provenance.is_admissible() {
        return Err(NubifyForestError::EdgeReceiptInvalidProvenance { request });
    }
    let edge_result = receipt.result;
    let edge_receipt_ordinal = edge_receipts.len();
    edge_receipts.push(receipt);
    if edge_result != 0 {
        return Ok(());
    }

    let old = staged.wdata(candidate_x, candidate_y).clone();
    let next_land = if old.flags & wflag::COAST != 0 {
        land::OCEAN as i32
    } else {
        old.land as i32
    };
    staged.set_land(
        candidate_x,
        candidate_y,
        next_land,
        -1,
        wflag::FOREST as i32,
        false,
    );
    let new = staged.wdata(candidate_x, candidate_y);
    if old.flags != new.flags || old.land != new.land || old.land_sub != new.land_sub {
        *cells_changed += 1;
    }
    writes.push(NubifyForestWrite {
        pass,
        scan_x,
        scan_y,
        candidate_x,
        candidate_y,
        edge_receipt_ordinal,
        old_flags: old.flags,
        old_land: old.land,
        old_land_sub: old.land_sub,
        new_flags: new.flags,
        new_land: new.land,
        new_land_sub: new.land_sub,
    });
    scratch.push(candidate_x, candidate_y);
    Ok(())
}

/// Execute the exact body around the unresolved `World::is_edge_of_region` boundary.
///
/// World writes occur on a clone.  A malformed/missing external receipt, structural error, or
/// checksum-scope violation leaves `map` and the upstream RNG handoff unchanged. Host observations
/// or side effects are outside that transaction; use a side-effect-free or two-phase host when
/// external atomicity is required. This adapter never asks the host to mutate the staged World.
pub fn execute_nubify_forest_frontier<H: EdgeOfRegionHost>(
    map: &mut InitialWorld,
    previous: &CheckPlayerForestReceipt,
    host: &mut H,
) -> Result<NubifyForestReceipt, NubifyForestError> {
    if previous.entry_va != MAP_CHECK_PLAYER_FOREST_VA
        || previous.return_va != MAP_CHECK_PLAYER_FOREST_RET_VA
        || previous.caller_resume_va != MAP_CHECK_PLAYER_FOREST_CALLER_RESUME_VA
        || previous.next_va != TERRAIN_GROUPS_NUBIFY_FOREST_VA
        || previous.random_draws != 0
        || previous.random_state_before != previous.random_state_after
        || previous.checksum_after != map.checksum
        || previous.sourced_walked_bytes != map.sourced_walked_bytes
        || previous
            .checksum_before
            .differing_sections(&previous.checksum_after)
            .iter()
            .any(|section| *section != WorldSection::WData)
    {
        return Err(NubifyForestError::CheckPlayerForestReceiptMismatch);
    }
    validate_world_shape(&map.world)?;
    if map.world.checksum_sections() != map.checksum {
        return Err(NubifyForestError::StoredChecksumMismatch);
    }

    let checksum_before = map.checksum.clone();
    let random_state_before = previous.random_state_after;
    let mut random = Random::new(random_state_before);
    let mut staged = map.world.clone();
    let mut draws = Vec::new();
    let mut edge_receipts = Vec::new();
    let mut writes = Vec::new();
    let mut scratch = RetailCoordArrays::default();
    let mut cells_changed = 0usize;

    // 0x006a9472..0x006a96bb: columns first, X outer and Y inner.
    for x in 1..staged.xs - 1 {
        let mut direction = 0i32;
        let mut run = 0i32;
        for y in 1..staged.ys - 1 {
            let east = is_forest(&staged, x + 1, y);
            let west = is_forest(&staged, x - 1, y);
            if !is_forest(&staged, x, y) {
                run = 0;
            } else if (east && direction == 1) || (west && direction == -1) {
                run += 1;
                if run >= 4 {
                    let offset = draw(
                        &mut random,
                        &mut draws,
                        COLUMN_OFFSET_RANDOM_VA,
                        NubifyPass::Column,
                        NubifyRandomUse::OffsetWithinFourCellRun,
                        x,
                        y,
                    );
                    try_candidate(
                        &mut staged,
                        host,
                        NubifyPass::Column,
                        x,
                        y,
                        x.wrapping_add(direction),
                        y.wrapping_sub(offset),
                        &mut edge_receipts,
                        &mut writes,
                        &mut scratch,
                        &mut cells_changed,
                    )?;
                    run = 0;
                }
            } else if east && west {
                run = 0;
            } else {
                run = 1;
                direction = if east {
                    1
                } else if west {
                    -1
                } else {
                    0
                };
            }
        }
    }

    // 0x006a96bb..0x006a993e: rows second, Y outer and X inner.
    for y in 1..staged.ys - 1 {
        let mut direction = 0i32;
        let mut run = 0i32;
        for x in 1..staged.xs - 1 {
            let south = is_forest(&staged, x, y + 1);
            let north = is_forest(&staged, x, y - 1);
            if !is_forest(&staged, x, y) {
                run = 0;
            } else if (south && direction == 1) || (north && direction == -1) {
                run += 1;
                if run >= 4 {
                    let offset = draw(
                        &mut random,
                        &mut draws,
                        ROW_OFFSET_RANDOM_VA,
                        NubifyPass::Row,
                        NubifyRandomUse::OffsetWithinFourCellRun,
                        x,
                        y,
                    );
                    try_candidate(
                        &mut staged,
                        host,
                        NubifyPass::Row,
                        x,
                        y,
                        x.wrapping_sub(offset),
                        y.wrapping_add(direction),
                        &mut edge_receipts,
                        &mut writes,
                        &mut scratch,
                        &mut cells_changed,
                    )?;
                    run = 0;
                }
            } else {
                run = 1;
                direction = if south {
                    if north {
                        draw(
                            &mut random,
                            &mut draws,
                            ROW_DIRECTION_RANDOM_VA,
                            NubifyPass::Row,
                            NubifyRandomUse::RowDirectionTie,
                            x,
                            y,
                        )
                    } else {
                        1
                    }
                } else if north {
                    -1
                } else {
                    0
                };
            }
        }
    }

    let checksum_after = staged.checksum_sections();
    let sections = checksum_before.differing_sections(&checksum_after);
    if sections
        .iter()
        .any(|section| *section != WorldSection::WData)
    {
        return Err(NubifyForestError::UnexpectedChecksumSections { sections });
    }

    map.world = staged;
    map.checksum = checksum_after.clone();
    Ok(NubifyForestReceipt {
        call_va: MAP_NUBIFY_FOREST_CALL_VA,
        entry_va: TERRAIN_GROUPS_NUBIFY_FOREST_VA,
        return_va: TERRAIN_GROUPS_NUBIFY_FOREST_RET_VA,
        caller_resume_va: MAP_NUBIFY_FOREST_CALLER_RESUME_VA,
        stack_argument_read: false,
        random_state_before,
        random_state_after: random.state(),
        random_draws: draws,
        edge_receipts,
        writes,
        cells_changed,
        temporary_array_capacity: scratch.capacity(),
        checksum_before,
        checksum_after,
        sourced_walked_bytes: map.sourced_walked_bytes,
    })
}
