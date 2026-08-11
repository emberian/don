//! Exact map-generation owner for `World::set_oil_at` and its base-`Good` storage.
//!
//! The shipped `World::set_oil_at` (`0x006b2a10`) is not a scalar WData setter.  It
//! closes every active type-5 `Good` whose decoded position belongs to the requested
//! WCoord, toggles `WData::OIL`, and, when enabled, calls `Objects::init_good`
//! (`0x00653f30`) at the cell centre.  This module owns that complete transaction for the
//! `TerrainGroups::place_all` chronology, where `TerrainOut` has not yet been initialized
//! and `Good::init` therefore records logical height zero.
//!
//! This is deliberately separate from [`crate::systems::map_terrain`]: the `goods`
//! checksum channel, sparse slot identity, `ObjectsData::good_mark`, and the engine array
//! growth schedule are object-system state, not World state.

use crate::container::increase_by;
use crate::systems::economy::{goods_channel, GoodNode};
use crate::systems::map_terrain::{tflag, Coord, TCoord, WCoord, World};

/// `World::set_oil_at(WCoord const&, WCoord const&, int)`.
pub const WORLD_SET_OIL_AT_VA: u32 = 0x006b_2a10;
/// `Objects::init_good(TypeIndex, Coord, Coord)`.
pub const OBJECTS_INIT_GOOD_VA: u32 = 0x0065_3f30;
/// `Good::close()`.
pub const GOOD_CLOSE_VA: u32 = 0x0066_d860;
/// `Good::init(TypeIndex, int, Coord, Coord, int)`.
pub const GOOD_INIT_VA: u32 = 0x0066_da20;

/// TypeIndex 5, the shipped base-Good type named `Oil`.
pub const OIL_GOOD_TYPE: i32 = 5;
/// `SubObject`'s coordinate XOR key.
pub const SUBOBJECT_COORD_XOR: i32 = 0x0006_3637;
/// Constructor/close sentinel stored in `SubObject::{z,x,y}`.
pub const CLOSED_COORD_INTERNAL: i32 = -0x0006_3638;
/// `Good::close` clears this otherwise-unattributed WData footprint bit.
pub const WDATA_GOOD_FOOTPRINT: u16 = 0x0001;
/// `Good::walk_data` plus `SubObject::walk_data` contributes 21 bytes per active node.
pub const GOOD_WALKED_BYTES: usize = 21;

/// The redundant values carried by the terrain placement request.
///
/// `good_type` and the centre coordinates are validated before mutation.  Keeping them
/// here prevents a caller from acknowledging the right WCoord with an alien object
/// allocation fact.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct OilGoodMutation {
    pub world_x: i32,
    pub world_y: i32,
    pub enabled: bool,
    pub good_type: i32,
    pub coord_x: i32,
    pub coord_y: i32,
}

impl OilGoodMutation {
    /// Build the request exactly as the style-14 terrain callers do.
    pub const fn at(world_x: i32, world_y: i32, enabled: bool) -> Self {
        Self {
            world_x,
            world_y,
            enabled,
            good_type: OIL_GOOD_TYPE,
            coord_x: world_x.wrapping_mul(0x300).wrapping_add(0x180),
            coord_y: world_y.wrapping_mul(0x300).wrapping_add(0x180),
        }
    }
}

/// One allocated `Good` object.
///
/// `node.{z,x,y}` are the XOR-encoded in-memory words walked by channel 11, not decoded
/// display coordinates. `ptype_present` models the pointer separately because a closed
/// Good stores a null pointer while its skipped checksum row has no TypeIndex to walk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OilGoodSlot {
    pub node: GoodNode,
    pub ptype_present: bool,
    /// `GoodData::cur_time`; not checksummed, but reset by type-5 `Good::init` and retained
    /// by `Good::close`.
    pub cur_time: u32,
}

impl Default for OilGoodSlot {
    fn default() -> Self {
        Self {
            node: GoodNode {
                flags: 0,
                who: u8::MAX,
                o: -1,
                z: CLOSED_COORD_INTERNAL,
                x: CLOSED_COORD_INTERNAL,
                y: CLOSED_COORD_INTERNAL,
                type_index: 0,
                ever_seen: 0,
            },
            ptype_present: false,
            cur_time: 0,
        }
    }
}

impl OilGoodSlot {
    #[inline]
    pub const fn active(&self) -> bool {
        self.node.flags & 1 != 0
    }

    #[inline]
    pub const fn coord_x(&self) -> i32 {
        self.node.x ^ SUBOBJECT_COORD_XOR
    }

    #[inline]
    pub const fn coord_y(&self) -> i32 {
        self.node.y ^ SUBOBJECT_COORD_XOR
    }

    #[inline]
    pub const fn coord_z(&self) -> i32 {
        self.node.z ^ SUBOBJECT_COORD_XOR
    }
}

/// The base-`Good` pointer array plus `ObjectsData::good_mark`.
///
/// Rust `Vec::capacity` is intentionally irrelevant. `capacity`, `increment`, and
/// `array_flags` are the retail `PtrArray<Good>` fields. `cur_index` is retained for
/// container fidelity but is not read or written by this transaction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OilGoodRuntime {
    pub slots: Vec<OilGoodSlot>,
    pub capacity: i32,
    pub increment: i16,
    pub array_flags: u8,
    pub cur_index: i32,
    pub good_mark: i32,
}

impl Default for OilGoodRuntime {
    /// Cold-start mapgen state: empty pointer array, doubling hint, mark 0.
    fn default() -> Self {
        Self {
            slots: Vec::new(),
            capacity: 0,
            increment: -1,
            array_flags: 0,
            cur_index: 0,
            good_mark: 0,
        }
    }
}

impl OilGoodRuntime {
    /// Active walked nodes in exact channel-11 slot order.
    ///
    /// `CheckSums::check_goods` scans the full PtrArray logical length. This is
    /// intentionally independent of `good_mark`, which bounds `set_oil_at`
    /// and scenario-save scans instead.
    pub fn active_nodes(&self) -> Vec<GoodNode> {
        self.slots
            .iter()
            .filter(|slot| slot.active())
            .map(|slot| slot.node)
            .collect()
    }

    /// Exact `CheckSums::check_goods` scalar digest for the represented base Goods.
    pub fn goods_checksum(&self) -> u32 {
        goods_channel(&self.active_nodes())
    }

    /// Semantic rows written by `ScenarioWrite::save_goods_chunk`, in retail slot order.
    ///
    /// The real chunk stores the type name rather than TypeIndex.  Type 5 resolves to
    /// `Oil`; other names require the caller's synchronized catalog and are therefore not
    /// invented here.
    pub fn scenario_rows(&self) -> Vec<ScenarioGoodRow> {
        let end = self.good_mark.max(0).min(self.slots.len() as i32) as usize;
        self.slots[..end]
            .iter()
            .enumerate()
            .filter(|(_, slot)| slot.active())
            .map(|(slot, good)| ScenarioGoodRow {
                slot,
                type_index: good.node.type_index,
                coord_x: good.coord_x(),
                coord_y: good.coord_y(),
            })
            .collect()
    }

    pub fn summary(&self) -> OilGoodStateSummary {
        let active_count = self.active_nodes().len();
        OilGoodStateSummary {
            array: PtrArrayGoodHeader {
                length: self.slots.len() as i32,
                capacity: self.capacity,
                increment: self.increment,
                flags: self.array_flags,
            },
            cur_index: self.cur_index,
            good_mark: self.good_mark,
            active_count,
            goods_walked_bytes: active_count * GOOD_WALKED_BYTES,
            goods_checksum: self.goods_checksum(),
            scenario_rows: self.scenario_rows(),
        }
    }
}

/// Save/DataWalk-visible `PtrArray<Good>` metadata.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PtrArrayGoodHeader {
    pub length: i32,
    pub capacity: i32,
    pub increment: i16,
    pub flags: u8,
}

/// One active semantic `.scx` goods row.  On disk the `type_index` is resolved to a name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScenarioGoodRow {
    pub slot: usize,
    pub type_index: i32,
    pub coord_x: i32,
    pub coord_y: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OilGoodStateSummary {
    pub array: PtrArrayGoodHeader,
    pub cur_index: i32,
    pub good_mark: i32,
    pub active_count: usize,
    pub goods_walked_bytes: usize,
    pub goods_checksum: u32,
    pub scenario_rows: Vec<ScenarioGoodRow>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GoodAllocationKind {
    ReusedInactive,
    Appended,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GoodAllocationReceipt {
    pub kind: GoodAllocationKind,
    pub slot: usize,
    pub capacity_before: i32,
    pub capacity_after: i32,
}

/// Auditable result of one atomic oil/Good mutation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OilGoodMutationReceipt {
    pub primitive_va: u32,
    pub request: OilGoodMutation,
    /// Matching active oil objects closed in ascending slot order.
    pub closed_slots: Vec<usize>,
    pub allocation: Option<GoodAllocationReceipt>,
    pub before: OilGoodStateSummary,
    pub after: OilGoodStateSummary,
    pub world_checksum_before: u32,
    pub world_checksum_after: u32,
    /// Physical WData indices whose complete records changed.
    pub changed_wdata: Vec<usize>,
    /// Physical TData indices whose masks changed.
    pub changed_tdata: Vec<usize>,
    /// World walk sections changed by this transaction: 5 for WData, 6 for TData.
    pub changed_world_sections: Vec<i32>,
    /// Pinned to zero: none of the four retail functions in this transaction draws RNG.
    pub rng_draws: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OilGoodMutationError {
    InvalidWorldShape {
        field: &'static str,
        expected: i64,
        actual: i64,
    },
    WorldCoordinateOutOfBounds {
        world_x: i32,
        world_y: i32,
    },
    WrongGoodType {
        expected: i32,
        actual: i32,
    },
    WrongCentre {
        expected_x: i32,
        expected_y: i32,
        actual_x: i32,
        actual_y: i32,
    },
    InvalidCapacity {
        capacity: i32,
        length: usize,
    },
    UnsupportedArrayFlags {
        flags: u8,
    },
    InvalidGoodMark {
        good_mark: i32,
        length: usize,
    },
    ActiveSlotPastGoodMark {
        slot: usize,
        good_mark: i32,
    },
    ActiveSlotWithoutType {
        slot: usize,
    },
    ActiveSlotWrongOwner {
        slot: usize,
        who: u8,
    },
    ActiveSlotWrongIdentity {
        slot: usize,
        object_index: i16,
    },
    OilFootprintOutOfBounds {
        slot: usize,
        tile_x: i32,
        tile_y: i32,
    },
    DownChainNeedsObjectBand {
        slot: usize,
        world_x: i32,
        world_y: i32,
        down: i16,
        down_who: i16,
    },
    ArrayCannotGrow {
        capacity: i32,
        increment: i16,
    },
    CapacityOverflow {
        capacity: i32,
        increment: i16,
    },
}

impl std::fmt::Display for OilGoodMutationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "oil/Good transaction refused: {self:?}")
    }
}

impl std::error::Error for OilGoodMutationError {}

/// Execute the exact style-14 map-generation `World::set_oil_at` transaction.
///
/// All validation and the complete close/toggle/allocation sequence run on clones.  A
/// refusal therefore leaves both owners byte-for-byte unchanged.  The only deliberately
/// bounded retail edge is `World::clear_down`: a nonnegative chain head requires the
/// general object bands, so this owner refuses it instead of unlinking a guessed object.
pub fn apply_world_set_oil_at(
    world: &mut World,
    goods: &mut OilGoodRuntime,
    request: OilGoodMutation,
) -> Result<OilGoodMutationReceipt, OilGoodMutationError> {
    validate_request(world, goods, request)?;

    let original_world = world.clone();
    let mut next_world = original_world.clone();
    let mut next_goods = goods.clone();
    let before = goods.summary();
    let world_checksum_before = world.checksum();

    let matching = matching_oil_slots(&next_goods, request.world_x, request.world_y);
    preflight_oil_closes(&next_world, &next_goods, &matching)?;

    for &slot_index in &matching {
        close_oil(&mut next_world, &mut next_goods.slots[slot_index]);
    }

    next_world.set_oil_at(request.world_x, request.world_y, request.enabled);

    let allocation = if request.enabled {
        let allocated = allocate_good_slot(&mut next_goods)?;
        init_mapgen_oil(
            &mut next_goods,
            allocated.slot,
            request.coord_x,
            request.coord_y,
        );
        Some(allocated)
    } else {
        None
    };

    validate_runtime(&next_goods)?;

    let changed_wdata = changed_indices(&original_world.wdata, &next_world.wdata);
    let changed_tdata = changed_indices(&original_world.tdata, &next_world.tdata);
    let mut changed_world_sections = Vec::with_capacity(2);
    if !changed_wdata.is_empty() {
        changed_world_sections.push(5);
    }
    if !changed_tdata.is_empty() {
        changed_world_sections.push(6);
    }

    let receipt = OilGoodMutationReceipt {
        primitive_va: WORLD_SET_OIL_AT_VA,
        request,
        closed_slots: matching,
        allocation,
        before,
        after: next_goods.summary(),
        world_checksum_before,
        world_checksum_after: next_world.checksum(),
        changed_wdata,
        changed_tdata,
        changed_world_sections,
        rng_draws: 0,
    };

    *world = next_world;
    *goods = next_goods;
    Ok(receipt)
}

fn validate_request(
    world: &World,
    goods: &OilGoodRuntime,
    request: OilGoodMutation,
) -> Result<(), OilGoodMutationError> {
    validate_world_shape(world)?;
    validate_runtime(goods)?;
    if !world.valid_w(request.world_x, request.world_y) {
        return Err(OilGoodMutationError::WorldCoordinateOutOfBounds {
            world_x: request.world_x,
            world_y: request.world_y,
        });
    }
    if request.good_type != OIL_GOOD_TYPE {
        return Err(OilGoodMutationError::WrongGoodType {
            expected: OIL_GOOD_TYPE,
            actual: request.good_type,
        });
    }
    let expected = OilGoodMutation::at(request.world_x, request.world_y, request.enabled);
    if request.coord_x != expected.coord_x || request.coord_y != expected.coord_y {
        return Err(OilGoodMutationError::WrongCentre {
            expected_x: expected.coord_x,
            expected_y: expected.coord_y,
            actual_x: request.coord_x,
            actual_y: request.coord_y,
        });
    }
    Ok(())
}

fn validate_world_shape(world: &World) -> Result<(), OilGoodMutationError> {
    let expected_size = i64::from(world.xs) * i64::from(world.ys);
    if world.xs < 0 || world.ys < 0 || expected_size != i64::from(world.size) {
        return Err(OilGoodMutationError::InvalidWorldShape {
            field: "size",
            expected: expected_size,
            actual: i64::from(world.size),
        });
    }
    if world.wdata.len() as i64 != expected_size {
        return Err(OilGoodMutationError::InvalidWorldShape {
            field: "wdata.length",
            expected: expected_size,
            actual: world.wdata.len() as i64,
        });
    }
    let expected_tile_xs = i64::from(world.xs) * 4;
    let expected_tile_ys = i64::from(world.ys) * 4;
    if i64::from(world.tile_xs) != expected_tile_xs {
        return Err(OilGoodMutationError::InvalidWorldShape {
            field: "tile_xs",
            expected: expected_tile_xs,
            actual: i64::from(world.tile_xs),
        });
    }
    if i64::from(world.tile_ys) != expected_tile_ys {
        return Err(OilGoodMutationError::InvalidWorldShape {
            field: "tile_ys",
            expected: expected_tile_ys,
            actual: i64::from(world.tile_ys),
        });
    }
    let expected_tile_size = expected_tile_xs * expected_tile_ys;
    if i64::from(world.tile_size) != expected_tile_size {
        return Err(OilGoodMutationError::InvalidWorldShape {
            field: "tile_size",
            expected: expected_tile_size,
            actual: i64::from(world.tile_size),
        });
    }
    if world.tdata.len() as i64 != expected_tile_size {
        return Err(OilGoodMutationError::InvalidWorldShape {
            field: "tdata.length",
            expected: expected_tile_size,
            actual: world.tdata.len() as i64,
        });
    }
    Ok(())
}

fn validate_runtime(goods: &OilGoodRuntime) -> Result<(), OilGoodMutationError> {
    if goods.capacity < 0 || goods.slots.len() as i64 > i64::from(goods.capacity) {
        return Err(OilGoodMutationError::InvalidCapacity {
            capacity: goods.capacity,
            length: goods.slots.len(),
        });
    }
    // Objects::init establishes zero. Bit 7 changes length during growth and the other
    // bits' owners are not established for the goods singleton, so the mapgen owner is
    // intentionally fail-closed outside that state.
    if goods.array_flags != 0 {
        return Err(OilGoodMutationError::UnsupportedArrayFlags {
            flags: goods.array_flags,
        });
    }
    if goods.good_mark < 0 || goods.good_mark as usize > goods.slots.len() {
        return Err(OilGoodMutationError::InvalidGoodMark {
            good_mark: goods.good_mark,
            length: goods.slots.len(),
        });
    }
    for (slot_index, slot) in goods.slots.iter().enumerate() {
        if slot.active() && slot_index >= goods.good_mark as usize {
            return Err(OilGoodMutationError::ActiveSlotPastGoodMark {
                slot: slot_index,
                good_mark: goods.good_mark,
            });
        }
        if slot.active() && !slot.ptype_present {
            return Err(OilGoodMutationError::ActiveSlotWithoutType { slot: slot_index });
        }
        if slot.active() && slot.node.who != u8::MAX {
            return Err(OilGoodMutationError::ActiveSlotWrongOwner {
                slot: slot_index,
                who: slot.node.who,
            });
        }
        if slot.active() && slot.node.o != slot_index as i16 {
            return Err(OilGoodMutationError::ActiveSlotWrongIdentity {
                slot: slot_index,
                object_index: slot.node.o,
            });
        }
    }
    Ok(())
}

fn matching_oil_slots(goods: &OilGoodRuntime, world_x: i32, world_y: i32) -> Vec<usize> {
    goods.slots[..goods.good_mark as usize]
        .iter()
        .enumerate()
        .filter(|(_, slot)| {
            slot.active()
                && slot.node.type_index == OIL_GOOD_TYPE
                && WCoord::from_coord(Coord(slot.coord_x())).0 == world_x
                && WCoord::from_coord(Coord(slot.coord_y())).0 == world_y
        })
        .map(|(slot, _)| slot)
        .collect()
}

fn preflight_oil_closes(
    world: &World,
    goods: &OilGoodRuntime,
    matching: &[usize],
) -> Result<(), OilGoodMutationError> {
    for &slot_index in matching {
        let slot = &goods.slots[slot_index];
        let tx = TCoord::from_coord(Coord(slot.coord_x())).0;
        let ty = TCoord::from_coord(Coord(slot.coord_y())).0;
        if !world.valid_t(tx, ty) {
            return Err(OilGoodMutationError::OilFootprintOutOfBounds {
                slot: slot_index,
                tile_x: tx,
                tile_y: ty,
            });
        }
        let wx = WCoord::from_coord(Coord(slot.coord_x())).0;
        let wy = WCoord::from_coord(Coord(slot.coord_y())).0;
        let w = world.wdata(wx, wy);
        if w.down >= 0 {
            return Err(OilGoodMutationError::DownChainNeedsObjectBand {
                slot: slot_index,
                world_x: wx,
                world_y: wy,
                down: w.down,
                down_who: w.down_who,
            });
        }
    }
    Ok(())
}

fn close_oil(world: &mut World, slot: &mut OilGoodSlot) {
    let coord_x = slot.coord_x();
    let coord_y = slot.coord_y();
    let tx = TCoord::from_coord(Coord(coord_x)).0;
    let ty = TCoord::from_coord(Coord(coord_y)).0;
    let wx = WCoord::from_coord(Coord(coord_x)).0;
    let wy = WCoord::from_coord(Coord(coord_y)).0;

    *world.tmask_mut(tx, ty) &= !tflag::RESOURCE;
    let w = world.wdata_mut(wx, wy);
    w.flags &= !WDATA_GOOD_FOOTPRINT;
    // This is the negative-head arm of World::clear_down (0x006b3ad0).
    w.down = -1;
    w.down_who = -1;

    slot.node.flags = 0;
    slot.node.z = CLOSED_COORD_INTERNAL;
    slot.node.x = CLOSED_COORD_INTERNAL;
    slot.node.y = CLOSED_COORD_INTERNAL;
    slot.node.type_index = 0;
    slot.ptype_present = false;
    // who, o, ever_seen, and cur_time survive Good::close.
}

fn allocate_good_slot(
    goods: &mut OilGoodRuntime,
) -> Result<GoodAllocationReceipt, OilGoodMutationError> {
    let capacity_before = goods.capacity;
    if let Some(slot) = goods.slots.iter().position(|slot| !slot.active()) {
        return Ok(GoodAllocationReceipt {
            kind: GoodAllocationKind::ReusedInactive,
            slot,
            capacity_before,
            capacity_after: goods.capacity,
        });
    }

    if goods.slots.len() as i32 >= goods.capacity {
        let growth = increase_by(goods.increment, goods.capacity);
        if growth == 0 {
            return Err(OilGoodMutationError::ArrayCannotGrow {
                capacity: goods.capacity,
                increment: goods.increment,
            });
        }
        let Some(next_capacity) = goods.capacity.checked_add(growth) else {
            return Err(OilGoodMutationError::CapacityOverflow {
                capacity: goods.capacity,
                increment: goods.increment,
            });
        };
        if next_capacity <= goods.capacity || next_capacity <= goods.slots.len() as i32 {
            return Err(OilGoodMutationError::CapacityOverflow {
                capacity: goods.capacity,
                increment: goods.increment,
            });
        }
        goods.capacity = next_capacity;
    }

    let slot = goods.slots.len();
    goods.slots.push(OilGoodSlot::default());
    Ok(GoodAllocationReceipt {
        kind: GoodAllocationKind::Appended,
        slot,
        capacity_before,
        capacity_after: goods.capacity,
    })
}

fn init_mapgen_oil(goods: &mut OilGoodRuntime, slot_index: usize, coord_x: i32, coord_y: i32) {
    let slot = &mut goods.slots[slot_index];
    slot.node = GoodNode {
        flags: 1,
        who: u8::MAX,
        o: slot_index as i16,
        // place_all runs before TerrainOut::init. find_tcoord_z therefore returns the
        // zeroed land_height fallback, then SubObject::init XOR-encodes it.
        z: SUBOBJECT_COORD_XOR,
        x: coord_x ^ SUBOBJECT_COORD_XOR,
        y: coord_y ^ SUBOBJECT_COORD_XOR,
        type_index: OIL_GOOD_TYPE,
        ever_seen: 0,
    };
    slot.ptype_present = true;
    slot.cur_time = 0;
    goods.good_mark = goods.good_mark.max(slot_index as i32 + 1);
}

fn changed_indices<T: PartialEq>(before: &[T], after: &[T]) -> Vec<usize> {
    before
        .iter()
        .zip(after)
        .enumerate()
        .filter_map(|(index, (a, b))| (a != b).then_some(index))
        .collect()
}
