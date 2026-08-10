// SPDX-License-Identifier: GPL-3.0-or-later
//! Snapshot owner for AIR_PATROL's mod-32 building search.
//!
//! This freezes the complete read-only transaction beginning at `Unit::do_air_patrol`
//! `0x005EA9AA` and ending at its `WallData::ever_seen +0x62` viewer-bit test.  It is not
//! registered but not wired into `Sim`: the live bridge must prove that its world-cell chains, building rows,
//! footprints, diplomacy, and post-waypoint-advance call ordering are one coherent frame.

#![allow(dead_code)]

pub const UNIT_DO_AIR_PATROL_VA: u32 = 0x005e_a620;
pub const AIR_PATROL_BUILDING_BRANCH_VA: u32 = 0x005e_a9aa;
pub const FIND_BUILDING_AT_CALL_VA: u32 = 0x005e_aa84;
pub const OBJECTS_FIND_BUILDING_AT_VA: u32 = 0x0065_ab40;
pub const SEARCH_VALID_SEARCH_VA: u32 = 0x0067_daa0;
pub const LEADER_IS_ENEMY_VA: u32 = 0x006e_baa0;
pub const WALL_TILE_CORNER_VA: u32 = 0x0064_3440;

pub const SEARCH_ENEMY: i32 = 3;
pub const FILTER_ALL: i32 = 0;
pub const PLAYER_SLOTS: usize = 8;
pub const TILES_PER_WORLD_CELL: i32 = 4;
pub const COORD_PER_TILE: i32 = 0xc0;
pub const COORD_PER_WORLD_CELL: i32 = 0x300;
pub const BUILDING_BLOCKER_MASK: u16 = 3;

pub const WORLD_XS_OFFSET: usize = 0x00;
pub const WORLD_YS_OFFSET: usize = 0x04;
pub const WORLD_TILE_XS_OFFSET: usize = 0x18;
pub const WORLD_TILE_YS_OFFSET: usize = 0x1c;
pub const WORLD_WDATA_OFFSET: usize = 0x134;
pub const WORLD_TDATA_OFFSET: usize = 0x138;
pub const WDATA_DOWN_OFFSET: usize = 0x08;
pub const WDATA_DOWN_WHO_OFFSET: usize = 0x0a;
pub const OBJECT_DOWN_OFFSET: usize = 0x2c;
pub const OBJECT_DOWN_WHO_OFFSET: usize = 0x2e;
pub const OBJECT_UID_OFFSET: usize = 0x30;
pub const WALL_EVER_SEEN_OFFSET: usize = 0x62;
pub const OBJECT_TYPE_X_SIZE_OFFSET: usize = 0x234;
pub const OBJECT_TYPE_Y_SIZE_OFFSET: usize = 0x238;

/// The two shipped offset arrays at `0x00ADCAF0` and `0x00ADC400`, zipped in the order
/// `ObjectsData::find_building_at` consumes them: centre, then clockwise from north-west.
pub const WORLD_CELL_SEARCH_OFFSETS: [(i32, i32); 9] = [
    (0, 0),
    (-1, -1),
    (0, -1),
    (1, -1),
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
];

/// Retail `(who,o)` address. `o < 0` terminates a `WData::down` chain; the companion
/// `who` word is ignored in that case.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ObjectKey {
    pub who: i16,
    pub o: i16,
}

impl ObjectKey {
    pub const NONE: Self = Self { who: 0, o: -1 };
}

/// Every candidate fact read by `find_building_at`, plus the post-return visibility byte.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildingSearchRecord {
    pub key: ObjectKey,
    /// `ObjectData::down/down_who +0x2C/+0x2E`; retail reads this before candidate gates.
    pub down: ObjectKey,
    /// Virtual `BuildData::is_valid_build`, vtable `+0x0C`.
    pub valid_build: bool,
    /// Virtual `WallData::is_active`, vtable `+0x4C`.
    pub active: bool,
    /// `WallData::tile_corner` result, in TCoord tiles.
    pub corner_x: i32,
    pub corner_y: i32,
    /// `ObjectTypeData::x_size/y_size +0x234/+0x238`.
    pub x_size: i32,
    pub y_size: i32,
    /// `ObjectData::uid +0x30`, retained for the eventual checksum-complete STRAFE target.
    pub uid: u16,
    /// `WallData::ever_seen +0x62`. This is not an ObjectTypeData field.
    pub ever_seen: u8,
}

/// One coherent read-only image of all state reached by the retail query.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildingSearchFrame {
    pub tile_xs: i32,
    pub tile_ys: i32,
    /// `TData::mask`, tile-row-major.
    pub tile_masks: Vec<u16>,
    pub world_cell_xs: i32,
    pub world_cell_ys: i32,
    /// `WData::down/down_who`, world-cell-row-major.
    pub world_cell_heads: Vec<ObjectKey>,
    pub buildings: Vec<BuildingSearchRecord>,
    /// Directional `LeaderData::diplos[8]`: `[from][toward]`.
    pub diplomacy: [[i32; PLAYER_SLOTS]; PLAYER_SLOTS],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildingSearchInstallFault {
    InvalidDimensions,
    TilePlaneLength {
        expected: usize,
        observed: usize,
    },
    WorldCellPlaneLength {
        expected: usize,
        observed: usize,
    },
    DuplicateObject {
        first: usize,
        second: usize,
        key: ObjectKey,
    },
    RevisionExhausted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildingSearchFault {
    Uninstalled,
    InvalidActorOwner(u8),
    MissingObject(ObjectKey),
    ChainCycle {
        cell_x: i32,
        cell_y: i32,
        at: ObjectKey,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AirPatrolBuildingGate {
    AirPhysicsStopped,
    AnimalOverride,
    PriorBranchReturned,
    PhaseNotDue,
    Due,
}

/// Inputs that decide whether retail reaches the mod-32 call. `prior_branch_returned` covers
/// last-waypoint retirement and an accepted mod-16 unit/bomber target, both of which return
/// before the building query.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AirPatrolBuildingGateInput {
    pub air_physics_succeeded: bool,
    pub actor_is_animal: bool,
    pub prior_branch_returned: bool,
    pub actor_o: i16,
    pub frame: i32,
}

#[inline]
pub const fn air_patrol_building_gate(input: AirPatrolBuildingGateInput) -> AirPatrolBuildingGate {
    if !input.air_physics_succeeded {
        AirPatrolBuildingGate::AirPhysicsStopped
    } else if input.actor_is_animal {
        AirPatrolBuildingGate::AnimalOverride
    } else if input.prior_branch_returned {
        AirPatrolBuildingGate::PriorBranchReturned
    } else if (input.actor_o as i32).wrapping_add(input.frame) % 32 != 0 {
        AirPatrolBuildingGate::PhaseNotDue
    } else {
        AirPatrolBuildingGate::Due
    }
}

/// `div_3_table[value] == floor(value/3)`, including the negative side of the table.
#[inline]
pub const fn div3_floor(value: i32) -> i32 {
    let q = value / 3;
    let r = value % 3;
    if value < 0 && r != 0 {
        q - 1
    } else {
        q
    }
}

/// Exact `Coord::operator TCoord`: `div_3_table[coord >> 6]`.
#[inline]
pub const fn coord_to_tile(coord: i32) -> i32 {
    div3_floor(coord >> 6)
}

/// The building branch starts at the current, already-advanced waypoint. Fighter-bombers
/// (type `0x134`, non-strict) add a live patrol-home position and call `WorldData::restrict`.
#[inline]
pub fn building_search_coord(
    waypoint: (i32, i32),
    actor_is_fighter_bomber: bool,
    live_home: Option<(i32, i32)>,
    world_cell_xs: i32,
    world_cell_ys: i32,
) -> (i32, i32) {
    if !actor_is_fighter_bomber {
        return waypoint;
    }
    let Some(home) = live_home else {
        return waypoint;
    };
    let max_x = world_cell_xs.wrapping_mul(COORD_PER_WORLD_CELL);
    let max_y = world_cell_ys.wrapping_mul(COORD_PER_WORLD_CELL);
    let restrict = |v: i32, max: i32| {
        let v = v.max(0);
        if max <= v {
            max.wrapping_sub(1)
        } else {
            v
        }
    };
    (
        restrict(waypoint.0.wrapping_add(home.0), max_x),
        restrict(waypoint.1.wrapping_add(home.1), max_y),
    )
}

/// Exact `LeaderData::is_enemy` predicate reached by `SearchIndexBH(3)`: owners must differ,
/// and hostility in either direction is sufficient.
#[inline]
pub const fn leader_is_enemy(
    diplomacy: &[[i32; PLAYER_SLOTS]; PLAYER_SLOTS],
    candidate_owner: usize,
    actor_owner: usize,
) -> bool {
    candidate_owner != actor_owner
        && (diplomacy[candidate_owner][actor_owner] == 0
            || diplomacy[actor_owner][candidate_owner] == 0)
}

/// The post-return gate at `0x005EAAB2..0x005EAAC2`. x86 masks the shift count to five
/// bits, but the source byte means only owner slots 0..7 can ever be admitted.
#[inline]
pub const fn building_was_seen_by(ever_seen: u8, actor_owner: u8) -> bool {
    let bit = 1u32.wrapping_shl((actor_owner as u32) & 31);
    (ever_seen as u32) & bit != 0
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildingSearchHit {
    pub key: ObjectKey,
    pub uid: u16,
    pub ever_seen: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AirPatrolBuildingDecision {
    NotDue(AirPatrolBuildingGate),
    Miss,
    /// The first `find_building_at` hit failed its later `ever_seen` bit. Retail does not
    /// resume the spatial walk to look for a second visible building.
    FirstHitNotSeen(BuildingSearchHit),
    Target(BuildingSearchHit),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildingSearchExecution {
    pub decision: AirPatrolBuildingDecision,
    pub visited_cells: Vec<(i32, i32)>,
    pub visited_candidates: Vec<ObjectKey>,
}

impl BuildingSearchExecution {
    fn not_due(gate: AirPatrolBuildingGate) -> Self {
        Self {
            decision: AirPatrolBuildingDecision::NotDue(gate),
            visited_cells: Vec::new(),
            visited_candidates: Vec::new(),
        }
    }
}

#[derive(Clone, Debug)]
struct InstalledFrame {
    frame: BuildingSearchFrame,
}

/// Atomic, revisioned snapshot owner. A failed install leaves the previous frame usable.
#[derive(Clone, Debug, Default)]
pub struct AirPatrolBuildingSearchOwner {
    revision: u64,
    installed: Option<InstalledFrame>,
}

impl AirPatrolBuildingSearchOwner {
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    pub fn install_frame(
        &mut self,
        frame: BuildingSearchFrame,
    ) -> Result<u64, BuildingSearchInstallFault> {
        validate_frame(&frame)?;
        let next = self
            .revision
            .checked_add(1)
            .ok_or(BuildingSearchInstallFault::RevisionExhausted)?;
        self.installed = Some(InstalledFrame { frame });
        self.revision = next;
        Ok(next)
    }

    /// Execute the fixed AIR_PATROL call chain. `search_coord` must be the current waypoint
    /// after retail's arrival/cursor transition, transformed by [`building_search_coord`].
    pub fn probe(
        &self,
        gate_input: AirPatrolBuildingGateInput,
        actor_owner: u8,
        search_coord: (i32, i32),
    ) -> Result<BuildingSearchExecution, BuildingSearchFault> {
        let gate = air_patrol_building_gate(gate_input);
        if gate != AirPatrolBuildingGate::Due {
            return Ok(BuildingSearchExecution::not_due(gate));
        }
        if actor_owner as usize >= PLAYER_SLOTS {
            return Err(BuildingSearchFault::InvalidActorOwner(actor_owner));
        }
        let installed = self
            .installed
            .as_ref()
            .ok_or(BuildingSearchFault::Uninstalled)?;
        let mut execution = find_building_at(
            &installed.frame,
            actor_owner,
            coord_to_tile(search_coord.0),
            coord_to_tile(search_coord.1),
        )?;
        if let AirPatrolBuildingDecision::Target(hit) = execution.decision {
            if !building_was_seen_by(hit.ever_seen, actor_owner) {
                execution.decision = AirPatrolBuildingDecision::FirstHitNotSeen(hit);
            }
        }
        Ok(execution)
    }
}

fn checked_plane_len(x: i32, y: i32) -> Option<usize> {
    if x <= 0 || y <= 0 {
        return None;
    }
    usize::try_from(x)
        .ok()?
        .checked_mul(usize::try_from(y).ok()?)
}

fn validate_frame(frame: &BuildingSearchFrame) -> Result<(), BuildingSearchInstallFault> {
    let Some(tile_len) = checked_plane_len(frame.tile_xs, frame.tile_ys) else {
        return Err(BuildingSearchInstallFault::InvalidDimensions);
    };
    let Some(cell_len) = checked_plane_len(frame.world_cell_xs, frame.world_cell_ys) else {
        return Err(BuildingSearchInstallFault::InvalidDimensions);
    };
    if frame.world_cell_xs.checked_mul(TILES_PER_WORLD_CELL) != Some(frame.tile_xs)
        || frame.world_cell_ys.checked_mul(TILES_PER_WORLD_CELL) != Some(frame.tile_ys)
    {
        return Err(BuildingSearchInstallFault::InvalidDimensions);
    }
    if frame.tile_masks.len() != tile_len {
        return Err(BuildingSearchInstallFault::TilePlaneLength {
            expected: tile_len,
            observed: frame.tile_masks.len(),
        });
    }
    if frame.world_cell_heads.len() != cell_len {
        return Err(BuildingSearchInstallFault::WorldCellPlaneLength {
            expected: cell_len,
            observed: frame.world_cell_heads.len(),
        });
    }
    for (second, record) in frame.buildings.iter().enumerate() {
        if let Some(first) = frame.buildings[..second]
            .iter()
            .position(|prior| prior.key == record.key)
        {
            return Err(BuildingSearchInstallFault::DuplicateObject {
                first,
                second,
                key: record.key,
            });
        }
    }
    Ok(())
}

fn find_building_at(
    frame: &BuildingSearchFrame,
    actor_owner: u8,
    tile_x: i32,
    tile_y: i32,
) -> Result<BuildingSearchExecution, BuildingSearchFault> {
    let mut execution = BuildingSearchExecution {
        decision: AirPatrolBuildingDecision::Miss,
        visited_cells: Vec::new(),
        visited_candidates: Vec::new(),
    };

    if tile_x < 0 || tile_y < 0 || tile_x >= frame.tile_xs || tile_y >= frame.tile_ys {
        return Ok(execution);
    }
    let tile_index = tile_y as usize * frame.tile_xs as usize + tile_x as usize;
    if frame.tile_masks[tile_index] & BUILDING_BLOCKER_MASK != BUILDING_BLOCKER_MASK {
        return Ok(execution);
    }

    let base_x = tile_x >> 2;
    let base_y = tile_y >> 2;
    for (dx, dy) in WORLD_CELL_SEARCH_OFFSETS {
        let cell_x = base_x.wrapping_add(dx);
        let cell_y = base_y.wrapping_add(dy);
        if cell_x < 0
            || cell_y < 0
            || cell_x >= frame.world_cell_xs
            || cell_y >= frame.world_cell_ys
        {
            continue;
        }
        execution.visited_cells.push((cell_x, cell_y));
        let cell_index = cell_y as usize * frame.world_cell_xs as usize + cell_x as usize;
        let mut key = frame.world_cell_heads[cell_index];
        let chain_start = execution.visited_candidates.len();
        while key.o >= 0 {
            if execution.visited_candidates[chain_start..].contains(&key) {
                return Err(BuildingSearchFault::ChainCycle {
                    cell_x,
                    cell_y,
                    at: key,
                });
            }
            execution.visited_candidates.push(key);
            let record = frame
                .buildings
                .iter()
                .find(|record| record.key == key)
                .ok_or(BuildingSearchFault::MissingObject(key))?;
            let next = record.down;

            let owner_matches = if key.who >= PLAYER_SLOTS as i16 {
                false
            } else if key.who < 0 {
                true
            } else {
                leader_is_enemy(&frame.diplomacy, key.who as usize, actor_owner as usize)
            };
            let inside = record.corner_x <= tile_x
                && tile_x < record.corner_x.wrapping_add(record.x_size)
                && record.corner_y <= tile_y
                && tile_y < record.corner_y.wrapping_add(record.y_size);
            if owner_matches && record.valid_build && record.active && inside {
                execution.decision = AirPatrolBuildingDecision::Target(BuildingSearchHit {
                    key,
                    uid: record.uid,
                    ever_seen: record.ever_seen,
                });
                return Ok(execution);
            }
            key = next;
        }
    }
    Ok(execution)
}
