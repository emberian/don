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
    GoodyAward, GoodyRules, Items, LeaderGoody, DOWN_ITEM, DOWN_NONE, LAND_REJECT_A, LAND_REJECT_B,
    TYPE_GOODY, WFLAG_ITEM, WFLAG_OVERRIDE_LAND,
};
use crate::systems::map_terrain::World as TerrainWorld;

/// The executable producer behind retail `CheckSums::check_items` (`0x00937790`).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ItemRuntime {
    items: Items,
    xs: i32,
    ys: i32,
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
