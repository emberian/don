//! Exact entry-owner state for the replay `TerrainGroups::place_all` survey.
//!
//! `Objects::init`/`Objects::clear` supplies an executable oil/Good owner.
//! The shipped mountain displacement-template catalog does not: its
//! proprietary `.tga` producer remains an explicit boundary, so production
//! replay state must never install a synthetic `MountainAddRuntime` here.

use crate::replay_goods_initial::{ObjectsInitGoodsReceipt, ReplayInitialGoodsRuntime};
use don_sim::systems::terrain_region_continuation::PlaceRegionGroupOwners;

/// `MountainRange::init`, the unported producer of the displacement-template
/// coordinate triples consumed by `Mountains::add_mountain`.
pub const MOUNTAIN_TEMPLATE_PRODUCER_VA: u32 = 0x0089_98b0;

/// Installed-content boundary deliberately retained beside the executable
/// Goods owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplayPlaceAllMountainOwnerBoundary {
    MissingProprietaryDisplacementTemplates {
        producer_va: u32,
        required_source: &'static str,
    },
}

impl ReplayPlaceAllMountainOwnerBoundary {
    pub const MISSING: Self = Self::MissingProprietaryDisplacementTemplates {
        producer_va: MOUNTAIN_TEMPLATE_PRODUCER_VA,
        required_source: "shipped effects_graphics.xml .tga displacement art",
    };
}

/// Owner initialization retained by the executable initial-item plan.
///
/// The Goods runtime is exact for a fresh supported process after
/// `Objects::init` and `Objects::clear`. The mountain boundary is data, not an
/// empty/synthetic runtime. `entry_owners` clones this state because the
/// place-all advance is a read-only reconstruction survey.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayPlaceAllOwnerInitialization {
    goods: ReplayInitialGoodsRuntime,
    mountain_boundary: ReplayPlaceAllMountainOwnerBoundary,
}

impl ReplayPlaceAllOwnerInitialization {
    pub fn cold_process() -> Self {
        Self {
            goods: ReplayInitialGoodsRuntime::cold_process(),
            mountain_boundary: ReplayPlaceAllMountainOwnerBoundary::MISSING,
        }
    }

    pub fn goods_initialization(&self) -> &ObjectsInitGoodsReceipt {
        self.goods.initialization()
    }

    pub fn goods_runtime(&self) -> &ReplayInitialGoodsRuntime {
        &self.goods
    }

    pub const fn mountain_boundary(&self) -> ReplayPlaceAllMountainOwnerBoundary {
        self.mountain_boundary
    }

    /// Build the exact owner snapshot accepted by the canonical owned
    /// place-all adapter. `mountains: None` is the typed missing-owner stop;
    /// it must not be replaced by an empty template vector.
    pub fn entry_owners(&self) -> PlaceRegionGroupOwners {
        PlaceRegionGroupOwners {
            mountains: None,
            oil_goods: Some(self.goods.goods().clone()),
        }
    }
}
