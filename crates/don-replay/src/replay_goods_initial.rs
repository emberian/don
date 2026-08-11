//! Bounded replay/mapgen owner for the initial `goods` checksum prefix.
//!
//! `Objects::init` (`0x0065ea80`) resets the logical `PtrArray<Good>` length,
//! increment, and flags before calling `Objects::clear` (`0x0065d740`), which
//! zeros `ObjectsData::good_mark`.  A nonnegative allocation capacity may be
//! retained from an earlier game, but no live logical row or inactive hole
//! survives the reset.  The supported PE's cold `.data` image has capacity
//! zero.
//!
//! This runtime then adapts the typed oil requests emitted by map generation to
//! the canonical `don-sim` `World::set_oil_at` owner.  It deliberately remains a
//! prefix: later `Map::place_resources` (`0x0068f4f0`) creates non-oil Goods, so
//! these bytes must not be installed as a complete first-checkpoint channel.

use don_sim::systems::map_terrain::World;
use don_sim::systems::terrain_drop_tile::{DropTileExternalRequest, DropTileExternalResolution};
use don_sim::systems::world_oil_goods::{
    apply_world_set_oil_at, OilGoodMutation, OilGoodMutationError, OilGoodMutationReceipt,
    OilGoodRuntime, OilGoodStateSummary,
};

/// `Objects::init()`.
pub const OBJECTS_INIT_VA: u32 = 0x0065_ea80;
/// `Objects::clear()`, called by `Objects::init` after container setup.
pub const OBJECTS_CLEAR_VA: u32 = 0x0065_d740;
/// Static `PtrArray<Good> goods` in the supported image.
pub const GOODS_SINGLETON_VA: u32 = 0x00c0_a0e0;
/// The later owner that makes an oil-only prefix insufficient for turn one.
pub const MAP_PLACE_RESOURCES_VA: u32 = 0x0068_f4f0;

/// Provenance for allocator fields retained across `Objects::init`.
///
/// Capacity and cursor are not visited by `CheckSums::check_goods`; this
/// distinction exists because they change allocation receipts and generic
/// save/DataWalk metadata.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InitialGoodsStorageSource {
    /// Capacity and cursor zero from the supported PE's cold `.data` image.
    ColdPeImage,
    /// Retained fields supplied by a separate observation. Ordinary `.rcx`
    /// bytes establish neither value.
    MeasuredRetainedStorage,
}

/// Retained `PtrArray<Good>` fields which `Objects::init` does not overwrite.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RetainedGoodsStorageFact {
    pub capacity: i32,
    pub cur_index: i32,
}

/// Exact Goods-owned projection after `Objects::init` and `Objects::clear`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectsInitGoodsReceipt {
    pub objects_init_va: u32,
    pub objects_clear_va: u32,
    pub goods_singleton_va: u32,
    pub storage_source: InitialGoodsStorageSource,
    pub state: OilGoodStateSummary,
    /// Number of inactive slots strictly below `good_mark`.
    pub holes_below_mark: usize,
}

/// One typed mapgen request resolved through the canonical Good owner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayOilPlacementReceipt {
    pub ordinal: usize,
    pub request: DropTileExternalRequest,
    pub resolution: DropTileExternalResolution,
    pub execution: OilGoodMutationReceipt,
    pub holes_before: usize,
    pub holes_after: usize,
}

/// Exact bytes currently owned by this bounded initial-Goods reconstruction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InitialGoodsPrefixSnapshot {
    pub state: OilGoodStateSummary,
    pub holes_below_mark: usize,
    pub oil_receipts: usize,
    /// Every walked byte in `state` comes from the exact Good initializer.
    pub sourced_walked_bytes: usize,
    /// Always false in this tranche. `Map::place_resources` remains a later
    /// checksum-visible Good producer even when all place-all oil calls ran.
    pub complete_for_first_checkpoint: bool,
    pub later_good_owner_va: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReplayInitialGoodsError {
    NegativeRetainedCapacity { capacity: i32 },
    UnsupportedExternalRequest { request: DropTileExternalRequest },
    OilGood(OilGoodMutationError),
}

impl std::fmt::Display for ReplayInitialGoodsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "initial Goods reconstruction refused: {self:?}")
    }
}

impl std::error::Error for ReplayInitialGoodsError {}

/// Initial Goods owner carried alongside replay map generation.
///
/// The runtime retains sparse slot identity, `good_mark`, engine capacity
/// history, and every oil transaction receipt.  It exposes only a prefix
/// snapshot, never an installable channel, because later rare-Good allocation
/// is outside this module's ownership.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayInitialGoodsRuntime {
    initialization: ObjectsInitGoodsReceipt,
    goods: OilGoodRuntime,
    oil_receipts: Vec<ReplayOilPlacementReceipt>,
}

impl ReplayInitialGoodsRuntime {
    /// Exact cold-process state after `Objects::init`/`Objects::clear`.
    pub fn cold_process() -> Self {
        // The supported PE carries zero length/capacity/list/cursor and -1
        // increment in `.data`; `Objects::init` rewrites length/increment/flags,
        // and `Objects::clear` rewrites good_mark. `OilGoodRuntime::default`
        // is exactly that cold post-init projection.
        Self::from_post_init_storage(0, 0, InitialGoodsStorageSource::ColdPeImage)
            .expect("zero is a valid cold Goods capacity")
    }

    /// Post-init state with separately measured retained allocator fields.
    ///
    /// Neither field is inferred from a replay or checksum. `cur_index` is not
    /// used by oil allocation, but retaining an invented zero would make the
    /// generic container state claim stronger than its evidence.
    pub fn from_measured_retained_storage(
        fact: RetainedGoodsStorageFact,
    ) -> Result<Self, ReplayInitialGoodsError> {
        Self::from_post_init_storage(
            fact.capacity,
            fact.cur_index,
            InitialGoodsStorageSource::MeasuredRetainedStorage,
        )
    }

    fn from_post_init_storage(
        capacity: i32,
        cur_index: i32,
        storage_source: InitialGoodsStorageSource,
    ) -> Result<Self, ReplayInitialGoodsError> {
        if capacity < 0 {
            return Err(ReplayInitialGoodsError::NegativeRetainedCapacity { capacity });
        }
        let goods = OilGoodRuntime {
            capacity,
            cur_index,
            ..OilGoodRuntime::default()
        };
        let state = goods.summary();
        let initialization = ObjectsInitGoodsReceipt {
            objects_init_va: OBJECTS_INIT_VA,
            objects_clear_va: OBJECTS_CLEAR_VA,
            goods_singleton_va: GOODS_SINGLETON_VA,
            storage_source,
            state,
            holes_below_mark: 0,
        };
        Ok(Self {
            initialization,
            goods,
            oil_receipts: Vec::new(),
        })
    }

    pub fn initialization(&self) -> &ObjectsInitGoodsReceipt {
        &self.initialization
    }

    pub fn goods(&self) -> &OilGoodRuntime {
        &self.goods
    }

    pub fn oil_receipts(&self) -> &[ReplayOilPlacementReceipt] {
        &self.oil_receipts
    }

    /// Current exact oil-only Goods prefix.
    pub fn prefix_snapshot(&self) -> InitialGoodsPrefixSnapshot {
        let state = self.goods.summary();
        InitialGoodsPrefixSnapshot {
            sourced_walked_bytes: state.goods_walked_bytes,
            state,
            holes_below_mark: holes_below_mark(&self.goods),
            oil_receipts: self.oil_receipts.len(),
            complete_for_first_checkpoint: false,
            later_good_owner_va: MAP_PLACE_RESOURCES_VA,
        }
    }

    /// Resolve one terrain request against the canonical oil/Good transaction.
    ///
    /// Non-oil requests are rejected without mutation.  The underlying owner
    /// stages both World and Goods, so a validation or object-band refusal also
    /// leaves this adapter and its receipt history unchanged.
    pub fn resolve_oil_request(
        &mut self,
        world: &mut World,
        request: DropTileExternalRequest,
    ) -> Result<ReplayOilPlacementReceipt, ReplayInitialGoodsError> {
        let DropTileExternalRequest::OilGoodMutation {
            world_x,
            world_y,
            enabled,
            good_type,
            coord_x,
            coord_y,
        } = request
        else {
            return Err(ReplayInitialGoodsError::UnsupportedExternalRequest { request });
        };

        let holes_before = holes_below_mark(&self.goods);
        let execution = apply_world_set_oil_at(
            world,
            &mut self.goods,
            OilGoodMutation {
                world_x,
                world_y,
                enabled,
                good_type,
                coord_x,
                coord_y,
            },
        )
        .map_err(ReplayInitialGoodsError::OilGood)?;
        let resolution = DropTileExternalResolution::OilGoodsApplied { request };
        let receipt = ReplayOilPlacementReceipt {
            ordinal: self.oil_receipts.len(),
            request,
            resolution,
            execution,
            holes_before,
            holes_after: holes_below_mark(&self.goods),
        };
        self.oil_receipts.push(receipt.clone());
        Ok(receipt)
    }
}

fn holes_below_mark(goods: &OilGoodRuntime) -> usize {
    let end = goods.good_mark.max(0).min(goods.slots.len() as i32) as usize;
    goods.slots[..end]
        .iter()
        .filter(|slot| !slot.active())
        .count()
}
