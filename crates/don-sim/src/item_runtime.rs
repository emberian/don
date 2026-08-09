//! Runtime ownership for checksum channel 10 (`items`).
//!
//! [`crate::systems::items`] contains the recovered stable-slot registry, retail
//! transactions, and exact `Item::walk_data` byte stream, but those objects previously
//! lived only in isolated tests. This module installs that registry in [`crate::World`]
//! and deliberately uses [`crate::systems::map_terrain::World`]'s existing `WData` as
//! the only occupancy/terrain owner. There is no shadow item grid: placement and
//! collection change the same cells walked by checksum channel 12.
//!
//! An unavailable registry is not treated as an empty channel. Retail Adler-32 of an
//! empty item list is `1`, but returning that value before map setup would make an absent
//! model indistinguishable from a genuinely empty, modelled registry. The APIs therefore
//! fail closed with [`ItemRuntimeError::Unavailable`] until a map is attached.

use crate::systems::items::{
    channel_size, checksum_items, goody_amount, pick_goody_resource, snap_center, wcoord_of,
    GoodyAward, GoodyRules, Item, Items, LeaderGoody, DOWN_ITEM, DOWN_NONE, LAND_REJECT_A,
    LAND_REJECT_B, TYPE_GOODY, WFLAG_ITEM, WFLAG_OVERRIDE_LAND,
};
use crate::systems::map_terrain::World as TerrainWorld;

/// The executable producer behind retail `CheckSums::check_items` (`0x00937790`).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ItemRuntime {
    items: Items,
    xs: i32,
    ys: i32,
}

/// Pointer-free stable-slot image owned by deterministic save/load.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct ItemRuntimeSaveState {
    pub map_xs: i32,
    pub map_ys: i32,
    pub slots: Vec<Item>,
}

/// A save/load invariant failure across checksum channels 10 (items) and 12 (world).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ItemRuntimeSaveError {
    MapShape,
    SlotLimit,
    SlotIdentity(usize),
    SlotRecord(usize),
    ItemCoordinate(usize),
    HeterogeneousOccupancy { wx: i32, wy: i32 },
    MapCoupling { wx: i32, wy: i32 },
    DuplicateMapReference(usize),
    UnmappedLiveSlot(usize),
}

impl std::fmt::Display for ItemRuntimeSaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MapShape => f.write_str("item runtime/map shape mismatch"),
            Self::SlotLimit => f.write_str("item stable-slot count exceeds signed object index"),
            Self::SlotIdentity(slot) => write!(f, "item slot {slot} has mismatched identity"),
            Self::SlotRecord(slot) => write!(f, "item slot {slot} is not a runtime record"),
            Self::ItemCoordinate(slot) => {
                write!(f, "item slot {slot} has an invalid snapped map coordinate")
            }
            Self::HeterogeneousOccupancy { wx, wy } => {
                write!(
                    f,
                    "item at ({wx},{wy}) is behind a heterogeneous object head"
                )
            }
            Self::MapCoupling { wx, wy } => {
                write!(f, "item registry and WData disagree at ({wx},{wy})")
            }
            Self::DuplicateMapReference(slot) => {
                write!(f, "item slot {slot} is referenced by multiple WData cells")
            }
            Self::UnmappedLiveSlot(slot) => {
                write!(f, "live item slot {slot} has no WData sentinel")
            }
        }
    }
}

/// Evidence emitted alongside the channel word so an empty/absent producer cannot be
/// confused with a substantive one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ItemChannelReport {
    /// Adler-32 over live `Item::walk_data` records in stable slot order.
    pub checksum: u32,
    /// Number of live records visited by `CheckSums::check_items`.
    pub elements: u32,
    /// Exact bytes handed to the checksum visitor.
    pub bytes_walked: u32,
}

/// Fail-closed runtime setup/transaction errors.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ItemRuntimeError {
    /// Map setup has not installed an item registry, so channel 10 has no producer.
    Unavailable,
    /// A transaction was handed a different terrain plane from the one attached at setup.
    MapMismatch {
        expected_xs: i32,
        expected_ys: i32,
        actual_xs: i32,
        actual_ys: i32,
    },
    /// A map producer attempted to place an item outside the attached plane.
    CellOutOfBounds { wx: i32, wy: i32 },
    /// The cell head is a live object. Retail follows its heterogeneous object chain;
    /// core `ObjectRegistry` does not expose those `next` links yet, so mutation stops.
    ObjectChainUnavailable { down: i16, down_who: i16 },
}

impl ItemRuntime {
    /// Attach a new empty item registry to this exact terrain shape.
    pub fn for_map(map: &TerrainWorld) -> Self {
        Self {
            items: Items::new(),
            xs: map.xs,
            ys: map.ys,
        }
    }

    #[inline]
    pub fn items(&self) -> &Items {
        &self.items
    }

    /// Dimensions of the terrain plane this registry was attached to. Checksum bridges
    /// use this to refuse pairing channel 10 with a different channel-12 owner.
    #[inline]
    pub fn map_shape(&self) -> (i32, i32) {
        (self.xs, self.ys)
    }

    pub(crate) fn export_save_state(
        &self,
        map: &TerrainWorld,
    ) -> Result<ItemRuntimeSaveState, ItemRuntimeSaveError> {
        if self.items.len() > i16::MAX as usize + 1 {
            return Err(ItemRuntimeSaveError::SlotLimit);
        }
        let state = ItemRuntimeSaveState {
            map_xs: self.xs,
            map_ys: self.ys,
            slots: (0..self.items.len())
                .map(|slot| *self.items.get(slot).expect("slot is below Items::len"))
                .collect(),
        };
        validate_item_state(&state, map)?;
        Ok(state)
    }

    pub(crate) fn import_save_state(
        state: ItemRuntimeSaveState,
        map: &TerrainWorld,
    ) -> Result<Self, ItemRuntimeSaveError> {
        validate_item_state(&state, map)?;

        // Keep every placeholder live while growing: `Items::init_record` reuses the
        // first dead slot, so overwriting dead records before the final length exists
        // would collapse stable identities.
        let mut items = Items::new();
        let (x, y) = snap_center(0, 0);
        for _ in 0..state.slots.len() {
            items.init_record(TYPE_GOODY, x, y, 0);
        }
        for (slot, record) in state.slots.iter().copied().enumerate() {
            *items
                .get_mut(slot)
                .expect("all stable slots were provisioned above") = record;
        }
        Ok(Self {
            items,
            xs: state.map_xs,
            ys: state.map_ys,
        })
    }

    /// Exact current channel evidence. Unlike a bare checksum word, `bytes_walked`
    /// exposes whether the result is substantive.
    pub fn channel_report(&self) -> ItemChannelReport {
        ItemChannelReport {
            checksum: checksum_items(&self.items),
            elements: self.items.count_valid() as u32,
            bytes_walked: channel_size(&self.items),
        }
    }

    fn check_map(&self, map: &TerrainWorld) -> Result<(), ItemRuntimeError> {
        if (map.xs, map.ys) == (self.xs, self.ys) {
            Ok(())
        } else {
            Err(ItemRuntimeError::MapMismatch {
                expected_xs: self.xs,
                expected_ys: self.ys,
                actual_xs: map.xs,
                actual_ys: map.ys,
            })
        }
    }

    fn place_goody(
        &mut self,
        map: &mut TerrainWorld,
        wx: i32,
        wy: i32,
        z: i32,
    ) -> Result<usize, ItemRuntimeError> {
        self.check_map(map)?;
        if !in_w_bounds(map, wx, wy) {
            return Err(ItemRuntimeError::CellOutOfBounds { wx, wy });
        }
        let (x, y) = snap_center(wx, wy);
        let slot = self.items.init_record(TYPE_GOODY, x, y, z);
        let cell = map.wdata_mut(wx, wy);
        cell.flags |= WFLAG_ITEM;
        cell.down = DOWN_ITEM;
        cell.down_who = slot as i16;
        Ok(slot)
    }

    fn reveal(
        &mut self,
        map: &TerrainWorld,
        wx: i32,
        wy: i32,
        who: u8,
    ) -> Result<(), ItemRuntimeError> {
        self.check_map(map)?;
        if !in_w_bounds(map, wx, wy) {
            return Ok(());
        }
        let cell = map.wdata(wx, wy);
        if !lookup_allowed(cell.flags, cell.land) || cell.down != DOWN_ITEM || cell.down_who < 0 {
            return Ok(());
        }
        if let Some(item) = self.items.get_mut(cell.down_who as usize) {
            if item.is_valid() {
                item.mark_seen(who);
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn collect(
        &mut self,
        map: &mut TerrainWorld,
        leader: &mut LeaderGoody,
        rules: &GoodyRules,
        rng: &mut crate::rng::Random,
        unit_x: i32,
        unit_y: i32,
        game_frame: u32,
    ) -> Result<Option<GoodyAward>, ItemRuntimeError> {
        self.check_map(map)?;
        let wx = wcoord_of(unit_x);
        let wy = wcoord_of(unit_y);
        if !in_w_bounds(map, wx, wy) || map.wdata(wx, wy).flags & WFLAG_ITEM == 0 {
            return Ok(None);
        }

        let (flags, land, down, down_who) = {
            let cell = map.wdata(wx, wy);
            (cell.flags, cell.land, cell.down, cell.down_who)
        };
        if down >= 0 {
            return Err(ItemRuntimeError::ObjectChainUnavailable { down, down_who });
        }
        let slot = if lookup_allowed(flags, land)
            && down == DOWN_ITEM
            && down_who >= 0
            && self
                .items
                .get(down_who as usize)
                .is_some_and(|item| item.is_valid())
        {
            i32::from(down_who)
        } else {
            -1
        };

        if slot >= 0 {
            self.items.close_record(slot as usize);
        }
        let cell = map.wdata_mut(wx, wy);
        cell.flags &= !WFLAG_ITEM;
        cell.down = DOWN_NONE;
        cell.down_who = DOWN_NONE;

        // `Unit::explore_goody` removes the item during setup but returns before RNG or
        // resource mutation (`cmp Game::frame,0` at 0x005F9975).
        if game_frame == 0 {
            return Ok(None);
        }

        let (resource, draws) = pick_goody_resource(leader, rng);
        let amount = goody_amount(rules, leader.epoch_science, leader.spanish_ruins_bonus);
        leader.bucket[resource] = leader.bucket[resource].wrapping_add(amount);
        leader.goody_box_resources = leader.goody_box_resources.wrapping_add(amount);
        Ok(Some(GoodyAward {
            slot,
            resource,
            amount,
            draws,
        }))
    }
}

/// Prove that an absent producer is not paired with channel-12 item sentinels.
pub(crate) fn validate_absent_items_map(map: &TerrainWorld) -> Result<(), ItemRuntimeSaveError> {
    validate_map_geometry(map)?;
    for (index, cell) in map.wdata.iter().enumerate() {
        if cell.flags & WFLAG_ITEM == 0 && cell.down != DOWN_ITEM {
            continue;
        }
        let wx = index as i32 % map.xs;
        let wy = index as i32 / map.xs;
        if cell.down >= 0 {
            return Err(ItemRuntimeSaveError::HeterogeneousOccupancy { wx, wy });
        }
        return Err(ItemRuntimeSaveError::MapCoupling { wx, wy });
    }
    Ok(())
}

fn validate_map_geometry(map: &TerrainWorld) -> Result<(), ItemRuntimeSaveError> {
    let size = map
        .xs
        .checked_mul(map.ys)
        .and_then(|size| usize::try_from(size).ok())
        .ok_or(ItemRuntimeSaveError::MapShape)?;
    if map.xs <= 0 || map.ys <= 0 || map.wdata.len() != size {
        return Err(ItemRuntimeSaveError::MapShape);
    }
    Ok(())
}

fn validate_item_state(
    state: &ItemRuntimeSaveState,
    map: &TerrainWorld,
) -> Result<(), ItemRuntimeSaveError> {
    validate_map_geometry(map)?;
    if (state.map_xs, state.map_ys) != (map.xs, map.ys) {
        return Err(ItemRuntimeSaveError::MapShape);
    }
    if state.slots.len() > i16::MAX as usize + 1 {
        return Err(ItemRuntimeSaveError::SlotLimit);
    }

    let mut expected_cell = vec![None; state.slots.len()];
    for (slot, item) in state.slots.iter().enumerate() {
        if item.o != slot as i16 {
            return Err(ItemRuntimeSaveError::SlotIdentity(slot));
        }
        if !matches!(item.flags, 0 | 1)
            || item.who != 0xff
            || !item.has_type
            || item.type_index != TYPE_GOODY
        {
            return Err(ItemRuntimeSaveError::SlotRecord(slot));
        }
        let (wx, wy) = (wcoord_of(item.x()), wcoord_of(item.y()));
        if !in_w_bounds(map, wx, wy) || snap_center(wx, wy) != (item.x(), item.y()) {
            return Err(ItemRuntimeSaveError::ItemCoordinate(slot));
        }
        if item.is_valid() {
            let cell = map.wdata(wx, wy);
            if cell.down >= 0 {
                return Err(ItemRuntimeSaveError::HeterogeneousOccupancy { wx, wy });
            }
            expected_cell[slot] = Some((wx, wy));
        }
    }

    let mut mapped = vec![false; state.slots.len()];
    for (index, cell) in map.wdata.iter().enumerate() {
        let has_flag = cell.flags & WFLAG_ITEM != 0;
        if !has_flag && cell.down != DOWN_ITEM {
            continue;
        }
        let wx = index as i32 % map.xs;
        let wy = index as i32 / map.xs;
        if cell.down >= 0 {
            return Err(ItemRuntimeSaveError::HeterogeneousOccupancy { wx, wy });
        }
        if !has_flag || cell.down != DOWN_ITEM || cell.down_who < 0 {
            return Err(ItemRuntimeSaveError::MapCoupling { wx, wy });
        }
        let slot = cell.down_who as usize;
        let Some(item) = state.slots.get(slot) else {
            return Err(ItemRuntimeSaveError::MapCoupling { wx, wy });
        };
        if !item.is_valid() || expected_cell[slot] != Some((wx, wy)) {
            return Err(ItemRuntimeSaveError::MapCoupling { wx, wy });
        }
        if std::mem::replace(&mut mapped[slot], true) {
            return Err(ItemRuntimeSaveError::DuplicateMapReference(slot));
        }
    }
    for (slot, item) in state.slots.iter().enumerate() {
        if item.is_valid() && !mapped[slot] {
            return Err(ItemRuntimeSaveError::UnmappedLiveSlot(slot));
        }
    }
    Ok(())
}

#[inline]
fn lookup_allowed(flags: u16, land: i8) -> bool {
    flags & WFLAG_OVERRIDE_LAND != 0 || (land != LAND_REJECT_A && land != LAND_REJECT_B)
}

#[inline]
fn in_w_bounds(map: &TerrainWorld, wx: i32, wy: i32) -> bool {
    wx >= 0 && wy >= 0 && wx < map.xs && wy < map.ys
}

impl crate::World {
    /// Install checksum channel 10's runtime producer for the authoritative terrain map.
    pub fn configure_items(&mut self, map: &TerrainWorld) {
        self.item_runtime = Some(ItemRuntime::for_map(map));
    }

    /// Return current channel evidence, or fail closed when map setup never installed
    /// the producer.
    pub fn items_channel(&self) -> Result<ItemChannelReport, ItemRuntimeError> {
        self.item_runtime
            .as_ref()
            .map(ItemRuntime::channel_report)
            .ok_or(ItemRuntimeError::Unavailable)
    }

    /// Place a generated goody box in the stable registry and the checksum-visible map.
    pub fn place_goody(
        &mut self,
        map: &mut TerrainWorld,
        wx: i32,
        wy: i32,
        z: i32,
    ) -> Result<usize, ItemRuntimeError> {
        self.item_runtime
            .as_mut()
            .ok_or(ItemRuntimeError::Unavailable)?
            .place_goody(map, wx, wy, z)
    }

    /// Reveal a goody to one player, changing the byte retail walks as `ever_seen`.
    pub fn reveal_goody(
        &mut self,
        map: &TerrainWorld,
        wx: i32,
        wy: i32,
        who: u8,
    ) -> Result<(), ItemRuntimeError> {
        self.item_runtime
            .as_mut()
            .ok_or(ItemRuntimeError::Unavailable)?
            .reveal(map, wx, wy, who)
    }

    /// Execute `Unit::explore_goody` (`0x005F9780`) against this world's frame and main
    /// simulation RNG, mutating the same terrain cells channel 12 walks.
    pub fn collect_goody(
        &mut self,
        map: &mut TerrainWorld,
        leader: &mut LeaderGoody,
        rules: &GoodyRules,
        unit_x: i32,
        unit_y: i32,
    ) -> Result<Option<GoodyAward>, ItemRuntimeError> {
        let mut runtime = self
            .item_runtime
            .take()
            .ok_or(ItemRuntimeError::Unavailable)?;
        let award = runtime.collect(
            map,
            leader,
            rules,
            &mut self.random,
            unit_x,
            unit_y,
            self.frame as u32,
        );
        self.item_runtime = Some(runtime);
        award
    }
}
