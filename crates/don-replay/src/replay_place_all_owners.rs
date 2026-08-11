//! Exact entry-owner state for the replay `TerrainGroups::place_all` survey.
//!
//! `Objects::init`/`Objects::clear` supplies an executable oil/Good owner. The
//! mountain owner is installed only when a caller explicitly binds a user-owned
//! content provider containing `effects_graphics.xml` and all 16 referenced
//! displacement TGAs. Missing, incomplete, or malformed content therefore
//! remains a typed boundary; this module contains no shipped art or precomputed
//! displacement rows.

use crate::replay_goods_initial::{ObjectsInitGoodsReceipt, ReplayInitialGoodsRuntime};
use don_sim::systems::mountain_add_runtime::MountainAddRuntime;
use don_sim::systems::mountain_template_producer::{
    load_mountain_template_catalog, MountainTemplateCatalog, MountainTemplateProducerError,
    MOUNTAIN_TEMPLATE_CAPACITY,
};
use don_sim::systems::terrain_region_continuation::PlaceRegionGroupOwners;
use std::fmt;
use std::path::{Path, PathBuf};

/// `MountainRange::init`, the installed-content producer of the displacement
/// coordinate triples consumed by `Mountains::add_mountain`.
pub const MOUNTAIN_TEMPLATE_PRODUCER_VA: u32 = 0x0089_98b0;

/// Explicit user-owned source binding for the mountain template catalog.
///
/// [`Self::from_content_root`] models the normal installed layout. [`Self::new`]
/// also permits an independently mounted XML file while keeping every TGA
/// reference confined beneath `content_root` by the producer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayMountainContentProvider {
    content_root: PathBuf,
    effects_graphics_xml: PathBuf,
}

impl ReplayMountainContentProvider {
    pub fn from_content_root(content_root: impl Into<PathBuf>) -> Self {
        let content_root = content_root.into();
        let effects_graphics_xml = content_root.join("effects_graphics.xml");
        Self {
            content_root,
            effects_graphics_xml,
        }
    }

    pub fn new(content_root: impl Into<PathBuf>, effects_graphics_xml: impl Into<PathBuf>) -> Self {
        Self {
            content_root: content_root.into(),
            effects_graphics_xml: effects_graphics_xml.into(),
        }
    }

    pub fn content_root(&self) -> &Path {
        &self.content_root
    }

    pub fn effects_graphics_xml(&self) -> &Path {
        &self.effects_graphics_xml
    }
}

/// Installed-content boundary retained beside the executable Goods owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplayPlaceAllMountainOwnerBoundary {
    MissingInstalledDisplacementTemplates {
        producer_va: u32,
        required_source: &'static str,
    },
    InstalledDisplacementTemplates {
        producer_va: u32,
        template_count: usize,
    },
}

impl ReplayPlaceAllMountainOwnerBoundary {
    pub const MISSING: Self = Self::MissingInstalledDisplacementTemplates {
        producer_va: MOUNTAIN_TEMPLATE_PRODUCER_VA,
        required_source: "user-owned effects_graphics.xml and referenced displacement TGAs",
    };
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReplayPlaceAllOwnerInitializationError {
    MountainTemplateProducer(MountainTemplateProducerError),
    IncompleteMountainTemplateCatalog {
        expected: usize,
        sources: usize,
        displacement_tgas: usize,
        templates: usize,
    },
}

impl fmt::Display for ReplayPlaceAllOwnerInitializationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MountainTemplateProducer(error) => error.fmt(f),
            Self::IncompleteMountainTemplateCatalog {
                expected,
                sources,
                displacement_tgas,
                templates,
            } => write!(
                f,
                "installed mountain catalog has {sources} source rows, {displacement_tgas} displacement-file receipts, and {templates} derived templates; exactly {expected} of each are required"
            ),
        }
    }
}

impl std::error::Error for ReplayPlaceAllOwnerInitializationError {}

impl From<MountainTemplateProducerError> for ReplayPlaceAllOwnerInitializationError {
    fn from(error: MountainTemplateProducerError) -> Self {
        Self::MountainTemplateProducer(error)
    }
}

/// Atomic installed-source/derived-template evidence retained by the owner.
///
/// The catalog keeps all source rows and all derived geometry in the same
/// document order. It exposes immutable evidence; each place-all survey gets a
/// fresh `MountainAddRuntime` with world-sized verification storage.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayInstalledMountainOwnerReceipt {
    provider: ReplayMountainContentProvider,
    catalog: MountainTemplateCatalog,
}

impl ReplayInstalledMountainOwnerReceipt {
    pub fn provider(&self) -> &ReplayMountainContentProvider {
        &self.provider
    }

    pub fn catalog(&self) -> &MountainTemplateCatalog {
        &self.catalog
    }

    fn runtime(&self, world_cells: usize) -> MountainAddRuntime {
        MountainAddRuntime::new(
            world_cells,
            self.catalog.templates.iter().cloned().map(Some).collect(),
        )
    }
}

/// Owner initialization retained by the executable initial-item plan.
///
/// The Goods runtime is exact for a fresh supported process after
/// `Objects::init` and `Objects::clear`. Installed mountain evidence is joined
/// atomically: no runtime is returned unless the producer derived the complete
/// 16-row catalog from the explicit provider.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayPlaceAllOwnerInitialization {
    goods: ReplayInitialGoodsRuntime,
    mountains: Option<ReplayInstalledMountainOwnerReceipt>,
    mountain_boundary: ReplayPlaceAllMountainOwnerBoundary,
}

impl ReplayPlaceAllOwnerInitialization {
    pub fn cold_process() -> Self {
        Self {
            goods: ReplayInitialGoodsRuntime::cold_process(),
            mountains: None,
            mountain_boundary: ReplayPlaceAllMountainOwnerBoundary::MISSING,
        }
    }

    /// Bind the exact cold-process owners to an explicit user-owned content
    /// provider. The result is fail-closed: partial catalogs return an error and
    /// cannot yield a `PlaceRegionGroupOwners` value.
    pub fn from_installed_content(
        provider: ReplayMountainContentProvider,
    ) -> Result<Self, ReplayPlaceAllOwnerInitializationError> {
        let catalog = load_mountain_template_catalog(
            provider.effects_graphics_xml(),
            provider.content_root(),
        )?;
        if catalog.sources.len() != MOUNTAIN_TEMPLATE_CAPACITY
            || catalog.displacement_tgas.len() != MOUNTAIN_TEMPLATE_CAPACITY
            || catalog.templates.len() != MOUNTAIN_TEMPLATE_CAPACITY
        {
            return Err(
                ReplayPlaceAllOwnerInitializationError::IncompleteMountainTemplateCatalog {
                    expected: MOUNTAIN_TEMPLATE_CAPACITY,
                    sources: catalog.sources.len(),
                    displacement_tgas: catalog.displacement_tgas.len(),
                    templates: catalog.templates.len(),
                },
            );
        }
        Ok(Self {
            goods: ReplayInitialGoodsRuntime::cold_process(),
            mountains: Some(ReplayInstalledMountainOwnerReceipt { provider, catalog }),
            mountain_boundary:
                ReplayPlaceAllMountainOwnerBoundary::InstalledDisplacementTemplates {
                    producer_va: MOUNTAIN_TEMPLATE_PRODUCER_VA,
                    template_count: MOUNTAIN_TEMPLATE_CAPACITY,
                },
        })
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

    pub fn installed_mountain_receipt(&self) -> Option<&ReplayInstalledMountainOwnerReceipt> {
        self.mountains.as_ref()
    }

    /// Build the exact owner snapshot accepted by the canonical owned
    /// place-all adapter. The caller supplies the actual World plane length so
    /// `MountainsData::verify_bits` cannot be silently under- or over-sized.
    pub fn entry_owners(&self, world_cells: usize) -> PlaceRegionGroupOwners {
        PlaceRegionGroupOwners {
            mountains: self
                .mountains
                .as_ref()
                .map(|mountains| mountains.runtime(world_cells)),
            oil_goods: Some(self.goods.goods().clone()),
        }
    }
}
