//! Exact `ObjectsData::find_building_placed_at` `0x00658c80` transaction.
//!
//! Retail first overwrites `ObjectsData+0x200` with `-1`, rejects a tile unless its TData
//! carries a building blocker or `STARTED`, then walks the centre WData cell and its eight
//! neighbours. Every chain node's `ObjectData::{down,down_who}` is read before owner,
//! exclusion, validity, type, and footprint gates. A hit returns its object ordinal and stores
//! its owner in the scratch word.
//!
//! The scratch word is not part of [`crate::tick::Sim`]. Callers must install an explicit
//! [`ObjectsSelectedOwnerAuthority`] from a source; a Sim hash is not a substitute. Planning is
//! pure. Its journal may be committed only after a complete native return and can be rolled back
//! exactly. Unit and Build bands are read from their canonical owners. The current Wall adapter
//! stores a band-local `WallState::o` while retail spatial links use the banded ordinal, so a
//! reached Wall is preserved as a typed boundary instead of being normalized or guessed.
//!
//! Source ledger: supported PE SHA-256
//! `30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079`;
//! body `[0x00658c80,0x00658e60)` is 480 bytes with SHA-256
//! `125fca1a859ba30aa9cdf27e585b111918316817041875e0779d61b51aab56ed`.

#![forbid(unsafe_code)]

use super::air_patrol_building_search_frontier::WORLD_CELL_SEARCH_OFFSETS;
use super::build_type_find_friends::{
    ObjectsFindBuildingPlacedAtBoundary, OBJECTS_FIND_BUILDING_PLACED_AT_VA,
    OBJECTS_SELECTED_OWNER_OFFSET,
};
use super::map_terrain::{tflag, WorldChecksum};
use super::production::{
    flag,
    runtime::{LiveProductionRuntime, LiveTypeClass},
    Footprint,
};
use crate::objects::{Band, BUILD_BAND_BASE, OWNER_SLOTS, WALL_BAND_BASE};
use crate::tick::Sim;

pub const OBJECTS_FIND_BUILDING_PLACED_AT_END_VA: u32 = 0x0065_8e60;
pub const OBJECTS_FIND_BUILDING_PLACED_AT_BYTES: u32 =
    OBJECTS_FIND_BUILDING_PLACED_AT_END_VA - OBJECTS_FIND_BUILDING_PLACED_AT_VA;
pub const OBJECTS_FIND_BUILDING_PLACED_AT_FIRST_TYPE_CALL_VA: u32 = 0x0065_8db8;
pub const OBJECTS_FIND_BUILDING_PLACED_AT_TYPE_SLOT: u32 = 0x000c;
pub const WALL_BAND_IDENTITY_BOUNDARY_VA: u32 = 0x0065_8d7a;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObjectsSelectedOwnerAuthority {
    /// Revision of the installed source followed by exact committed writes in this owner.
    pub revision: u64,
    /// Digest of the external capture/authority which supplied the initial runtime word.
    pub source_sha256: [u8; 32],
    /// `ObjectsData+0x200`.
    pub selected_owner: i32,
}

impl ObjectsSelectedOwnerAuthority {
    pub fn validates(&self) -> bool {
        self.revision != 0 && self.source_sha256 != [0; 32]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObjectsSelectedOwnerJournal {
    pub offset: u32,
    pub before: ObjectsSelectedOwnerAuthority,
    /// Unconditional first write at `0x00658c94`.
    pub first_write: i32,
    /// Current staged word at this receipt's stop/return boundary.
    pub staged_selected_owner: i32,
    /// Present only after a complete native return.
    pub after: Option<ObjectsSelectedOwnerAuthority>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectsSelectedOwnerCommitError {
    ReceiptDidNotReturn,
    StaleAuthority,
}

impl ObjectsSelectedOwnerJournal {
    pub fn apply(
        self,
        authority: &mut ObjectsSelectedOwnerAuthority,
    ) -> Result<(), ObjectsSelectedOwnerCommitError> {
        let after = self
            .after
            .ok_or(ObjectsSelectedOwnerCommitError::ReceiptDidNotReturn)?;
        if *authority != self.before {
            return Err(ObjectsSelectedOwnerCommitError::StaleAuthority);
        }
        *authority = after;
        Ok(())
    }

    pub fn rollback(
        self,
        authority: &mut ObjectsSelectedOwnerAuthority,
    ) -> Result<(), ObjectsSelectedOwnerCommitError> {
        let after = self
            .after
            .ok_or(ObjectsSelectedOwnerCommitError::ReceiptDidNotReturn)?;
        if *authority != after {
            return Err(ObjectsSelectedOwnerCommitError::StaleAuthority);
        }
        *authority = self.before;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ObjectsSpatialKey {
    pub who: i16,
    pub o: i16,
}

impl ObjectsSpatialKey {
    pub const fn none() -> Self {
        Self { who: 0, o: -1 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectsSpatialBand {
    Unit,
    Build,
    Wall,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObjectsFindBuildingPlacedAtTerrainRead {
    pub tile: [i32; 2],
    pub mask: u16,
    pub building_blocker: bool,
    pub started: bool,
    pub spatial_walk_reached: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObjectsFindBuildingPlacedAtCellRead {
    pub offset_index: u8,
    pub offset: [i32; 2],
    pub world_cell: [i32; 2],
    pub in_bounds: bool,
    pub head: Option<ObjectsSpatialKey>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObjectsFindBuildingPlacedAtObjectRead {
    pub cell_offset_index: u8,
    pub key: ObjectsSpatialKey,
    /// Read before every later candidate gate.
    pub down: ObjectsSpatialKey,
    pub band: ObjectsSpatialBand,
    pub row: usize,
    pub playable_owner: bool,
    pub excluded: bool,
    pub owner_filter_matches: bool,
    /// Virtual slot `+0x0c`; absent when an earlier gate skipped the call.
    pub valid_build: Option<bool>,
    pub type_index: Option<i32>,
    pub footprint: Option<Footprint>,
    pub position: Option<[i32; 2]>,
    pub corner: Option<[i32; 2]>,
    pub contains_query_tile: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObjectsFindBuildingPlacedAtWallBoundary {
    pub instruction_va: u32,
    pub cell_offset_index: u8,
    pub key: ObjectsSpatialKey,
    pub row: usize,
    pub wall_stored_o: i16,
    pub required_banded_o: i16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectsFindBuildingPlacedAtStop {
    TerrainGateReturn,
    SpatialExhaustionReturn,
    ObjectHitReturn,
    WallBandIdentityBoundary,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectsFindBuildingPlacedAtReceipt {
    pub request: ObjectsFindBuildingPlacedAtBoundary,
    pub world_checksum: WorldChecksum,
    pub terrain: ObjectsFindBuildingPlacedAtTerrainRead,
    pub cells: Vec<ObjectsFindBuildingPlacedAtCellRead>,
    pub objects: Vec<ObjectsFindBuildingPlacedAtObjectRead>,
    pub scratch: ObjectsSelectedOwnerJournal,
    pub stop: ObjectsFindBuildingPlacedAtStop,
    /// `eax`; absent only at a typed child boundary.
    pub returned: Option<i32>,
    /// Exact owner paired with a nonnegative return.
    pub returned_owner: Option<i32>,
    pub wall_boundary: Option<ObjectsFindBuildingPlacedAtWallBoundary>,
}

impl ObjectsFindBuildingPlacedAtReceipt {
    pub fn validates(&self) -> bool {
        if self.request.callee_va != OBJECTS_FIND_BUILDING_PLACED_AT_VA
            || self.request.call_va == 0
            || self.request.native_push_order
                != [
                    self.request.excluded_owner,
                    self.request.excluded_object,
                    self.request.owner_filter,
                    self.request.tile[1],
                    self.request.tile[0],
                ]
            || self.scratch.offset != OBJECTS_SELECTED_OWNER_OFFSET
            || self.scratch.first_write != -1
            || !self.scratch.before.validates()
        {
            return false;
        }
        match self.stop {
            ObjectsFindBuildingPlacedAtStop::TerrainGateReturn
            | ObjectsFindBuildingPlacedAtStop::SpatialExhaustionReturn => {
                self.returned == Some(-1)
                    && self.returned_owner.is_none()
                    && self.wall_boundary.is_none()
                    && self.scratch.staged_selected_owner == -1
                    && self.scratch.after.is_some_and(|after| {
                        after.selected_owner == -1
                            && self.scratch.before.revision.checked_add(1) == Some(after.revision)
                            && after.source_sha256 == self.scratch.before.source_sha256
                    })
            }
            ObjectsFindBuildingPlacedAtStop::ObjectHitReturn => {
                self.returned.is_some_and(|value| value >= 0)
                    && self.returned_owner.is_some_and(|owner| owner >= 0)
                    && self.wall_boundary.is_none()
                    && self.returned_owner == Some(self.scratch.staged_selected_owner)
                    && self.scratch.after.is_some_and(|after| {
                        after.selected_owner == self.scratch.staged_selected_owner
                            && self.scratch.before.revision.checked_add(1) == Some(after.revision)
                            && after.source_sha256 == self.scratch.before.source_sha256
                    })
            }
            ObjectsFindBuildingPlacedAtStop::WallBandIdentityBoundary => {
                self.returned.is_none()
                    && self.returned_owner.is_none()
                    && self.wall_boundary.is_some()
                    && self.scratch.staged_selected_owner == -1
                    && self.scratch.after.is_none()
            }
        }
    }

    pub fn validates_against(&self, sim: &Sim, production: &LiveProductionRuntime) -> bool {
        plan_objects_find_building_placed_at(sim, production, &self.scratch.before, self.request)
            .is_ok_and(|expected| expected == *self)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ObjectsFindBuildingPlacedAtError {
    InvalidRequest,
    InvalidScratchAuthority,
    ScratchRevisionExhausted,
    InvalidWorldShape,
    QueryTileOutOfBounds,
    InvalidSpatialOwner {
        key: ObjectsSpatialKey,
    },
    MissingRegistryEntry {
        key: ObjectsSpatialKey,
    },
    MissingObjectRow {
        key: ObjectsSpatialKey,
        row: usize,
    },
    ObjectIdentityMismatch {
        key: ObjectsSpatialKey,
        row: usize,
    },
    ChainCycle {
        world_cell: [i32; 2],
        key: ObjectsSpatialKey,
    },
    MissingBuildType {
        key: ObjectsSpatialKey,
        row: usize,
    },
    MissingBuildFootprint {
        key: ObjectsSpatialKey,
        type_index: i32,
    },
    BuildTypeClassMismatch {
        key: ObjectsSpatialKey,
        type_index: i32,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ObjectsFindBuildingPlacedAtExecuteError {
    Plan(ObjectsFindBuildingPlacedAtError),
    Commit(ObjectsSelectedOwnerCommitError),
}

#[derive(Clone, Copy)]
struct ResolvedObject {
    band: ObjectsSpatialBand,
    row: usize,
    down: ObjectsSpatialKey,
    valid_build: bool,
    type_index: Option<i32>,
    position: Option<[i32; 2]>,
    wall_boundary: Option<ObjectsFindBuildingPlacedAtWallBoundary>,
}

fn i16_at(bytes: &[u8], offset: usize) -> i16 {
    i16::from_le_bytes(
        bytes[offset..offset + 2]
            .try_into()
            .expect("two-byte field"),
    )
}

fn registry_row(sim: &Sim, key: ObjectsSpatialKey) -> Option<(Band, usize)> {
    let owner = usize::try_from(key.who)
        .ok()
        .filter(|&who| who < OWNER_SLOTS)?;
    let o = i32::from(key.o);
    let (band, index) = if o < BUILD_BAND_BASE as i32 {
        (Band::Unit, usize::try_from(o).ok()?)
    } else if o < WALL_BAND_BASE as i32 {
        (
            Band::Build,
            usize::try_from(o - BUILD_BAND_BASE as i32).ok()?,
        )
    } else {
        (Band::Wall, usize::try_from(o - WALL_BAND_BASE as i32).ok()?)
    };
    let row = *sim.world.objects.slot(owner).band(band).get(index)? as usize;
    Some((band, row))
}

fn resolve_object(
    sim: &Sim,
    production: &LiveProductionRuntime,
    key: ObjectsSpatialKey,
    cell_offset_index: u8,
) -> Result<ResolvedObject, ObjectsFindBuildingPlacedAtError> {
    let (band, row) = registry_row(sim, key)
        .ok_or(ObjectsFindBuildingPlacedAtError::MissingRegistryEntry { key })?;
    match band {
        Band::Unit => {
            if row >= sim.world.units.len() {
                return Err(ObjectsFindBuildingPlacedAtError::MissingObjectRow { key, row });
            }
            if sim.world.units.get_who(row) != key.who as u8 || sim.world.units.o()[row] != key.o {
                return Err(ObjectsFindBuildingPlacedAtError::ObjectIdentityMismatch { key, row });
            }
            Ok(ResolvedObject {
                band: ObjectsSpatialBand::Unit,
                row,
                down: ObjectsSpatialKey {
                    who: sim.world.units.down_who()[row],
                    o: sim.world.units.down()[row],
                },
                // Virtual +0x0c is the live-Build/Wall predicate; ordinary Units return zero.
                valid_build: false,
                type_index: None,
                position: None,
                wall_boundary: None,
            })
        }
        Band::Build => {
            let build = sim
                .builds
                .get(row)
                .ok_or(ObjectsFindBuildingPlacedAtError::MissingObjectRow { key, row })?;
            if build.who != key.who as u8 || build.object_id() != key.o {
                return Err(ObjectsFindBuildingPlacedAtError::ObjectIdentityMismatch { key, row });
            }
            let image = build.image();
            Ok(ResolvedObject {
                band: ObjectsSpatialBand::Build,
                row,
                down: ObjectsSpatialKey {
                    who: i16_at(&image, 0x2e),
                    o: i16_at(&image, 0x2c),
                },
                valid_build: build.flags & flag::VALID != 0,
                type_index: production.build_types.get(row).copied().flatten(),
                position: Some([build.position().0, build.position().1]),
                wall_boundary: None,
            })
        }
        Band::Wall => {
            let wall = sim
                .walls
                .get(row)
                .ok_or(ObjectsFindBuildingPlacedAtError::MissingObjectRow { key, row })?;
            if wall.who != key.who as u8 {
                return Err(ObjectsFindBuildingPlacedAtError::ObjectIdentityMismatch { key, row });
            }
            Ok(ResolvedObject {
                band: ObjectsSpatialBand::Wall,
                row,
                down: ObjectsSpatialKey {
                    who: wall.down_who,
                    o: wall.down,
                },
                valid_build: wall.is_alive(),
                type_index: wall.ptype,
                position: Some([wall.x(), wall.y()]),
                wall_boundary: Some(ObjectsFindBuildingPlacedAtWallBoundary {
                    instruction_va: WALL_BAND_IDENTITY_BOUNDARY_VA,
                    cell_offset_index,
                    key,
                    row,
                    wall_stored_o: wall.o,
                    required_banded_o: key.o,
                }),
            })
        }
    }
}

fn completed_authority(
    before: ObjectsSelectedOwnerAuthority,
    selected_owner: i32,
) -> Result<ObjectsSelectedOwnerAuthority, ObjectsFindBuildingPlacedAtError> {
    Ok(ObjectsSelectedOwnerAuthority {
        revision: before
            .revision
            .checked_add(1)
            .ok_or(ObjectsFindBuildingPlacedAtError::ScratchRevisionExhausted)?,
        source_sha256: before.source_sha256,
        selected_owner,
    })
}

/// Plan the exact callee without publishing its scratch write.
pub fn plan_objects_find_building_placed_at(
    sim: &Sim,
    production: &LiveProductionRuntime,
    scratch_before: &ObjectsSelectedOwnerAuthority,
    request: ObjectsFindBuildingPlacedAtBoundary,
) -> Result<ObjectsFindBuildingPlacedAtReceipt, ObjectsFindBuildingPlacedAtError> {
    if request.call_va == 0
        || request.callee_va != OBJECTS_FIND_BUILDING_PLACED_AT_VA
        || request.native_push_order
            != [
                request.excluded_owner,
                request.excluded_object,
                request.owner_filter,
                request.tile[1],
                request.tile[0],
            ]
        || !(-1..8).contains(&request.owner_filter)
        || request.selected_owner_offset != OBJECTS_SELECTED_OWNER_OFFSET
        || request.selected_owner_first_write != -1
    {
        return Err(ObjectsFindBuildingPlacedAtError::InvalidRequest);
    }
    if !scratch_before.validates() {
        return Err(ObjectsFindBuildingPlacedAtError::InvalidScratchAuthority);
    }
    let world = &sim.map.world;
    let wlen = usize::try_from(world.xs).ok().and_then(|xs| {
        usize::try_from(world.ys)
            .ok()
            .and_then(|ys| xs.checked_mul(ys))
    });
    let tlen = usize::try_from(world.tile_xs).ok().and_then(|xs| {
        usize::try_from(world.tile_ys)
            .ok()
            .and_then(|ys| xs.checked_mul(ys))
    });
    if wlen != Some(world.wdata.len())
        || tlen != Some(world.tdata.len())
        || world.tile_xs != world.xs.saturating_mul(4)
        || world.tile_ys != world.ys.saturating_mul(4)
    {
        return Err(ObjectsFindBuildingPlacedAtError::InvalidWorldShape);
    }
    if !world.valid_t(request.tile[0], request.tile[1]) {
        return Err(ObjectsFindBuildingPlacedAtError::QueryTileOutOfBounds);
    }
    let mask = world.tmask(request.tile[0], request.tile[1]);
    let building_blocker = mask & tflag::BLOCKER_MASK == tflag::BLOCKER_BUILDING;
    let started = mask & tflag::STARTED != 0;
    let terrain = ObjectsFindBuildingPlacedAtTerrainRead {
        tile: request.tile,
        mask,
        building_blocker,
        started,
        spatial_walk_reached: building_blocker || started,
    };
    let world_checksum = world.checksum_sections();
    if !terrain.spatial_walk_reached {
        let after = completed_authority(*scratch_before, -1)?;
        let receipt = ObjectsFindBuildingPlacedAtReceipt {
            request,
            world_checksum,
            terrain,
            cells: Vec::new(),
            objects: Vec::new(),
            scratch: ObjectsSelectedOwnerJournal {
                offset: OBJECTS_SELECTED_OWNER_OFFSET,
                before: *scratch_before,
                first_write: -1,
                staged_selected_owner: -1,
                after: Some(after),
            },
            stop: ObjectsFindBuildingPlacedAtStop::TerrainGateReturn,
            returned: Some(-1),
            returned_owner: None,
            wall_boundary: None,
        };
        debug_assert!(receipt.validates());
        return Ok(receipt);
    }

    let base = [request.tile[0] >> 2, request.tile[1] >> 2];
    let mut cells = Vec::with_capacity(9);
    let mut objects = Vec::new();
    for (offset_index, &(dx, dy)) in WORLD_CELL_SEARCH_OFFSETS.iter().enumerate() {
        let world_cell = [base[0].wrapping_add(dx), base[1].wrapping_add(dy)];
        if !world.valid_w(world_cell[0], world_cell[1]) {
            cells.push(ObjectsFindBuildingPlacedAtCellRead {
                offset_index: offset_index as u8,
                offset: [dx, dy],
                world_cell,
                in_bounds: false,
                head: None,
            });
            continue;
        }
        let wdata = world.wdata(world_cell[0], world_cell[1]);
        let head = ObjectsSpatialKey {
            who: wdata.down_who,
            o: wdata.down,
        };
        cells.push(ObjectsFindBuildingPlacedAtCellRead {
            offset_index: offset_index as u8,
            offset: [dx, dy],
            world_cell,
            in_bounds: true,
            head: Some(head),
        });
        let cell_object_start = objects.len();
        let mut key = head;
        while key.o >= 0 {
            if key.who < 0 || usize::try_from(key.who).map_or(true, |who| who >= OWNER_SLOTS) {
                return Err(ObjectsFindBuildingPlacedAtError::InvalidSpatialOwner { key });
            }
            if objects[cell_object_start..]
                .iter()
                .any(|read: &ObjectsFindBuildingPlacedAtObjectRead| read.key == key)
            {
                return Err(ObjectsFindBuildingPlacedAtError::ChainCycle { world_cell, key });
            }
            let resolved = resolve_object(sim, production, key, offset_index as u8)?;
            if let Some(boundary) = resolved.wall_boundary {
                let receipt = ObjectsFindBuildingPlacedAtReceipt {
                    request,
                    world_checksum,
                    terrain,
                    cells,
                    objects,
                    scratch: ObjectsSelectedOwnerJournal {
                        offset: OBJECTS_SELECTED_OWNER_OFFSET,
                        before: *scratch_before,
                        first_write: -1,
                        staged_selected_owner: -1,
                        after: None,
                    },
                    stop: ObjectsFindBuildingPlacedAtStop::WallBandIdentityBoundary,
                    returned: None,
                    returned_owner: None,
                    wall_boundary: Some(boundary),
                };
                debug_assert!(receipt.validates());
                return Ok(receipt);
            }
            let playable_owner = key.who < 8;
            let excluded = playable_owner
                && i32::from(key.who) == request.excluded_owner
                && i32::from(key.o) == request.excluded_object;
            let owner_filter_matches = playable_owner
                && (request.owner_filter < 0 || i32::from(key.who) == request.owner_filter);
            let reaches_virtual = playable_owner && !excluded && owner_filter_matches;
            let valid_build = reaches_virtual.then_some(resolved.valid_build);
            let mut read = ObjectsFindBuildingPlacedAtObjectRead {
                cell_offset_index: offset_index as u8,
                key,
                down: resolved.down,
                band: resolved.band,
                row: resolved.row,
                playable_owner,
                excluded,
                owner_filter_matches,
                valid_build,
                type_index: None,
                footprint: None,
                position: None,
                corner: None,
                contains_query_tile: None,
            };
            if valid_build == Some(true) {
                let type_index = resolved.type_index.ok_or(
                    ObjectsFindBuildingPlacedAtError::MissingBuildType {
                        key,
                        row: resolved.row,
                    },
                )?;
                let target = usize::try_from(type_index)
                    .ok()
                    .and_then(|index| production.types.get(index))
                    .and_then(Option::as_ref)
                    .ok_or(ObjectsFindBuildingPlacedAtError::MissingBuildFootprint {
                        key,
                        type_index,
                    })?;
                if target.type_index != type_index || target.class != LiveTypeClass::Building {
                    return Err(ObjectsFindBuildingPlacedAtError::BuildTypeClassMismatch {
                        key,
                        type_index,
                    });
                }
                let footprint = target
                    .build_visibility
                    .and_then(|facts| facts.footprint)
                    .ok_or(ObjectsFindBuildingPlacedAtError::MissingBuildFootprint {
                        key,
                        type_index,
                    })?;
                let position = resolved.position.expect("live Build supplies position");
                let corner = footprint.tile_corner(position[0], position[1]);
                let contains = request.tile[0] >= corner.0
                    && request.tile[0] < corner.0.wrapping_add(footprint.x_size)
                    && request.tile[1] >= corner.1
                    && request.tile[1] < corner.1.wrapping_add(footprint.y_size);
                read.type_index = Some(type_index);
                read.footprint = Some(footprint);
                read.position = Some(position);
                read.corner = Some([corner.0, corner.1]);
                read.contains_query_tile = Some(contains);
                objects.push(read);
                if contains {
                    let selected_owner = i32::from(key.who);
                    let after = completed_authority(*scratch_before, selected_owner)?;
                    let receipt = ObjectsFindBuildingPlacedAtReceipt {
                        request,
                        world_checksum,
                        terrain,
                        cells,
                        objects,
                        scratch: ObjectsSelectedOwnerJournal {
                            offset: OBJECTS_SELECTED_OWNER_OFFSET,
                            before: *scratch_before,
                            first_write: -1,
                            staged_selected_owner: selected_owner,
                            after: Some(after),
                        },
                        stop: ObjectsFindBuildingPlacedAtStop::ObjectHitReturn,
                        returned: Some(i32::from(key.o)),
                        returned_owner: Some(selected_owner),
                        wall_boundary: None,
                    };
                    debug_assert!(receipt.validates());
                    return Ok(receipt);
                }
            } else {
                objects.push(read);
            }
            key = resolved.down;
        }
    }

    let after = completed_authority(*scratch_before, -1)?;
    let receipt = ObjectsFindBuildingPlacedAtReceipt {
        request,
        world_checksum,
        terrain,
        cells,
        objects,
        scratch: ObjectsSelectedOwnerJournal {
            offset: OBJECTS_SELECTED_OWNER_OFFSET,
            before: *scratch_before,
            first_write: -1,
            staged_selected_owner: -1,
            after: Some(after),
        },
        stop: ObjectsFindBuildingPlacedAtStop::SpatialExhaustionReturn,
        returned: Some(-1),
        returned_owner: None,
        wall_boundary: None,
    };
    debug_assert!(receipt.validates());
    Ok(receipt)
}

/// Plan one call and publish its scratch after-image only when the callee reached a native
/// return. A typed boundary leaves `authority` untouched. Callers composing several lookups may
/// roll completed receipts back in reverse order through [`ObjectsSelectedOwnerJournal::rollback`].
pub fn execute_objects_find_building_placed_at(
    sim: &Sim,
    production: &LiveProductionRuntime,
    authority: &mut ObjectsSelectedOwnerAuthority,
    request: ObjectsFindBuildingPlacedAtBoundary,
) -> Result<ObjectsFindBuildingPlacedAtReceipt, ObjectsFindBuildingPlacedAtExecuteError> {
    let receipt = plan_objects_find_building_placed_at(sim, production, authority, request)
        .map_err(ObjectsFindBuildingPlacedAtExecuteError::Plan)?;
    if receipt.returned.is_some() {
        receipt
            .scratch
            .apply(authority)
            .map_err(ObjectsFindBuildingPlacedAtExecuteError::Commit)?;
    }
    Ok(receipt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::systems::production::{runtime::LiveProductionType, BuildData};

    fn authority() -> ObjectsSelectedOwnerAuthority {
        ObjectsSelectedOwnerAuthority {
            revision: 7,
            source_sha256: [0x5a; 32],
            selected_owner: 6,
        }
    }

    fn request(tile: [i32; 2]) -> ObjectsFindBuildingPlacedAtBoundary {
        ObjectsFindBuildingPlacedAtBoundary {
            call_va: 0x0063_9335,
            callee_va: OBJECTS_FIND_BUILDING_PLACED_AT_VA,
            native_push_order: [-1, -1, 0, tile[1], tile[0]],
            circle_offset: 1,
            world_cell: [(tile[0] - 2) >> 2, (tile[1] - 2) >> 2],
            tile,
            owner_filter: 0,
            excluded_object: -1,
            excluded_owner: -1,
            selected_owner_offset: OBJECTS_SELECTED_OWNER_OFFSET,
            selected_owner_first_write: -1,
        }
    }

    #[test]
    fn empty_tdata_returns_minus_one_and_journals_scratch() {
        let sim = Sim::new(1, 8);
        let runtime = LiveProductionRuntime::default();
        let before = authority();
        let receipt =
            plan_objects_find_building_placed_at(&sim, &runtime, &before, request([14, 14]))
                .unwrap();
        assert!(receipt.validates_against(&sim, &runtime));
        assert_eq!(
            receipt.stop,
            ObjectsFindBuildingPlacedAtStop::TerrainGateReturn
        );
        assert_eq!(receipt.returned, Some(-1));
        assert!(receipt.cells.is_empty());
        assert_eq!(receipt.scratch.first_write, -1);
        assert_eq!(receipt.scratch.after.unwrap().selected_owner, -1);

        let mut live = before;
        receipt.scratch.apply(&mut live).unwrap();
        assert_eq!(live.selected_owner, -1);
        receipt.scratch.rollback(&mut live).unwrap();
        assert_eq!(live, before);
    }

    #[test]
    fn canonical_build_chain_returns_object_and_owner() {
        let mut sim = Sim::new(2, 8);
        sim.activate(0);
        let footprint = Footprint {
            x_size: 4,
            y_size: 4,
        };
        let (px, py) = footprint.corner_tile(5, 5);
        let mut build = BuildData::default();
        build.flags = flag::VALID;
        build.other[0x10..0x14].copy_from_slice(&(px ^ 0x63637).to_le_bytes());
        build.other[0x14..0x18].copy_from_slice(&(py ^ 0x63637).to_le_bytes());
        build.other[0x2c..0x2e].copy_from_slice(&(-1_i16).to_le_bytes());
        build.other[0x2e..0x30].copy_from_slice(&0_i16.to_le_bytes());
        let row = sim.spawn_build(0, build);
        let mut runtime = LiveProductionRuntime::default();
        runtime.register_build(row, 414);
        let mut facts = LiveProductionType::in_place_building(414, 1);
        facts.build_visibility = Some(
            super::super::production::runtime::LiveBuildVisibilityTypeFacts {
                domain: Some(0),
                footprint: Some(footprint),
                is_fort: Some(false),
            },
        );
        runtime.install_type(facts);
        sim.map.world.tdata[6 * sim.map.world.tile_xs as usize + 6] |= tflag::BLOCKER_BUILDING;
        let cell = sim.map.world.wdata_mut(1, 1);
        cell.down = BUILD_BAND_BASE as i16;
        cell.down_who = 0;

        let before = authority();
        let receipt =
            plan_objects_find_building_placed_at(&sim, &runtime, &before, request([6, 6])).unwrap();
        assert!(receipt.validates_against(&sim, &runtime));
        assert_eq!(
            receipt.stop,
            ObjectsFindBuildingPlacedAtStop::ObjectHitReturn
        );
        assert_eq!(receipt.returned, Some(BUILD_BAND_BASE as i32));
        assert_eq!(receipt.returned_owner, Some(0));
        assert_eq!(receipt.objects.len(), 1);
        assert_eq!(receipt.objects[0].type_index, Some(414));
        assert_eq!(receipt.objects[0].contains_query_tile, Some(true));
        assert_eq!(receipt.scratch.after.unwrap().selected_owner, 0);
    }
}
