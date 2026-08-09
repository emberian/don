//! Source retention for the Arena gathering data plane.
//!
//! A final [`Terrain`](super::Terrain) bitmap cannot reconstruct the engine's installed
//! nine-row `LandData` table or the pointer-array identity and traversal order of generated
//! Mountain/Cliff objects. This module therefore stores only source records which the
//! shared retail materializer has validated. It deliberately does not implement
//! `AuthoritativeGatherTerrain`: execution also requires the synchronized live WData/TData
//! world and a complete diplomacy provider.

use std::sync::Arc;

use don_sim::systems::gather_terrain::{
    GatherTerrainMaterialization, GatherTerrainMaterializationError, GatherTerrainSourceStamp,
    GatherTerrainWorldIdentity, MaterializedCliffObject, MaterializedMountainObject,
};
use don_sim::systems::map_terrain::TILES_PER_WCELL;

use super::Map;

/// Why a map has no executable retail gathering source plane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GatherTerrainUnavailable {
    /// [`Map::generate`] is explicitly not retail `RandomMap`; its circle stamps carry no
    /// `MountainRangeData`, `CliffMiningData`, or WData land-source identity.
    NonRetailGenerator,
}

/// The Arena map's source-retention state.
///
/// `Unavailable` is distinct from a retained source with empty Mountain/Cliff vectors:
/// the latter authoritatively means that the source pointer arrays contained no objects.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GatherTerrainSourceState {
    Unavailable(GatherTerrainUnavailable),
    Retained(RetainedGatherTerrainSources),
}

/// Original installed bytes plus their parsed and validated retail source records.
///
/// Keeping the bytes allows a later installed-data provider to re-check its producer
/// identity. The materialization retains all nine ordered four-slot `LandData` rows and
/// sparse Mountain/Cliff vectors without compacting null pointer-array slots.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetainedGatherTerrainSources {
    rules_xml: Arc<[u8]>,
    materialization: GatherTerrainMaterialization,
}

impl RetainedGatherTerrainSources {
    pub fn rules_xml(&self) -> &[u8] {
        &self.rules_xml
    }

    pub fn materialization(&self) -> &GatherTerrainMaterialization {
        &self.materialization
    }
}

/// Fail-closed map/source mismatches. Materializer failures preserve their exact reason.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GatherTerrainRetentionError {
    /// Retail WData owns exactly four fine tiles per axis. A partial trailing W cell would
    /// not have a corresponding live `World` identity, so retention is refused.
    IncompatibleMapDimensions {
        tile_xs: i32,
        tile_ys: i32,
    },
    Materialization(GatherTerrainMaterializationError),
}

impl From<GatherTerrainMaterializationError> for GatherTerrainRetentionError {
    fn from(error: GatherTerrainMaterializationError) -> Self {
        Self::Materialization(error)
    }
}

impl Map {
    /// Return the validated source plane, or the precise reason it is unavailable.
    pub fn gather_terrain_sources(
        &self,
    ) -> Result<&RetainedGatherTerrainSources, GatherTerrainUnavailable> {
        match &self.gather_terrain {
            GatherTerrainSourceState::Unavailable(reason) => Err(*reason),
            GatherTerrainSourceState::Retained(sources) => Ok(sources),
        }
    }

    /// Retain one coherent installed/generated gathering transaction.
    ///
    /// The source vectors are passed directly to the shared retail validator: `None`
    /// entries remain pointer-array holes; search WCoords, mining TCoords, mountain solid
    /// WCoords, and the explicit `mountain_size` scalar are never inferred from Arena
    /// terrain. Validation completes before `self` is mutated, so a failed refresh cannot
    /// replace a previously valid source plane.
    pub fn retain_gather_terrain_sources(
        &mut self,
        rules_xml: Vec<u8>,
        stamp: GatherTerrainSourceStamp,
        mountains: Vec<Option<MaterializedMountainObject>>,
        cliffs: Vec<Option<MaterializedCliffObject>>,
    ) -> Result<(), GatherTerrainRetentionError> {
        if self.w <= 0
            || self.h <= 0
            || self.w % TILES_PER_WCELL != 0
            || self.h % TILES_PER_WCELL != 0
        {
            return Err(GatherTerrainRetentionError::IncompatibleMapDimensions {
                tile_xs: self.w,
                tile_ys: self.h,
            });
        }

        let identity = GatherTerrainWorldIdentity {
            world_xs: self.w / TILES_PER_WCELL,
            world_ys: self.h / TILES_PER_WCELL,
            tile_xs: self.w,
            tile_ys: self.h,
            world_seed: self.seed as i32,
        };
        let materialization = GatherTerrainMaterialization::from_supported_sources(
            &rules_xml, stamp, identity, mountains, cliffs,
        )?;
        self.gather_terrain = GatherTerrainSourceState::Retained(RetainedGatherTerrainSources {
            rules_xml: Arc::from(rules_xml),
            materialization,
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use don_sim::systems::gather_terrain::{
        GatherTerrainMaterializationError, GatherTerrainSourceStamp, MaterializedMountainObject,
        SUPPORTED_RULES_XML_SHA256,
    };
    use don_sim::systems::gathering::{GatherTile, GatherWorldCell};

    use super::*;
    use crate::arena::map::{MapParams, Spatial};

    const RULES_XML: &[u8] = include_bytes!("../../../../ron-data/rules.xml");

    fn spatial() -> Spatial {
        Spatial {
            city_center_radius: 20,
            city_center_pop_radius: 4,
            city_capture_radius: 10,
            woodcutter_radius: 8,
            mine_radius: 6,
        }
    }

    fn map() -> Map {
        Map::generate(MapParams::default(), spatial())
    }

    fn stamp(map: &Map) -> GatherTerrainSourceStamp {
        GatherTerrainSourceStamp {
            installed_rules_sha256: SUPPORTED_RULES_XML_SHA256,
            world_seed: map.seed as i32,
            coherent_generation: true,
        }
    }

    fn mountain(wx: i32, wy: i32) -> MaterializedMountainObject {
        MaterializedMountainObject {
            search_wcoords: vec![GatherWorldCell { wx, wy }],
            mining_tcoords: vec![GatherTile {
                tx: wx * TILES_PER_WCELL,
                ty: wy * TILES_PER_WCELL,
            }],
            solid_wcoords: vec![GatherWorldCell { wx, wy }],
            mountain_size: 7,
        }
    }

    #[test]
    fn arena_generator_is_explicitly_not_a_retail_gather_source() {
        let map = map();
        assert_eq!(
            map.gather_terrain_sources().unwrap_err(),
            GatherTerrainUnavailable::NonRetailGenerator
        );
        assert!(
            map.count_within(
                map.starts[0].0,
                map.starts[0].1,
                20,
                crate::arena::map::Terrain::Mountain
            ) > 0
        );
    }

    #[test]
    fn missing_or_mismatched_installed_sources_fail_without_mutating_the_map() {
        let mut map = map();
        let mut wrong_identity = stamp(&map);
        wrong_identity.installed_rules_sha256 = [0; 32];
        assert_eq!(
            map.retain_gather_terrain_sources(
                RULES_XML.to_vec(),
                wrong_identity,
                Vec::new(),
                Vec::new(),
            ),
            Err(GatherTerrainRetentionError::Materialization(
                GatherTerrainMaterializationError::UnsupportedRulesXml
            ))
        );
        assert_eq!(
            map.gather_terrain_sources().unwrap_err(),
            GatherTerrainUnavailable::NonRetailGenerator
        );

        let mut wrong_world = stamp(&map);
        wrong_world.world_seed = wrong_world.world_seed.wrapping_add(1);
        assert!(matches!(
            map.retain_gather_terrain_sources(
                RULES_XML.to_vec(),
                wrong_world,
                Vec::new(),
                Vec::new(),
            ),
            Err(GatherTerrainRetentionError::Materialization(
                GatherTerrainMaterializationError::StaleWorldSeed { .. }
            ))
        ));
        assert_eq!(
            map.gather_terrain_sources().unwrap_err(),
            GatherTerrainUnavailable::NonRetailGenerator
        );

        assert!(matches!(
            map.retain_gather_terrain_sources(Vec::new(), stamp(&map), Vec::new(), Vec::new(),),
            Err(GatherTerrainRetentionError::Materialization(
                GatherTerrainMaterializationError::WrongRulesXmlLength { .. }
            ))
        ));
        assert_eq!(
            map.gather_terrain_sources().unwrap_err(),
            GatherTerrainUnavailable::NonRetailGenerator
        );
    }

    #[test]
    fn exact_sparse_object_sources_and_rules_bytes_are_retained() {
        let mut map = map();
        map.retain_gather_terrain_sources(
            RULES_XML.to_vec(),
            stamp(&map),
            vec![None, Some(mountain(2, 3))],
            vec![None],
        )
        .unwrap();

        let retained = map.gather_terrain_sources().unwrap();
        assert_eq!(retained.rules_xml(), RULES_XML);
        assert_eq!(retained.materialization().lands().len(), 9);
        assert_eq!(retained.materialization().mountains().len(), 2);
        assert!(retained.materialization().mountains()[0].is_none());
        assert_eq!(
            retained.materialization().mountains()[1]
                .as_ref()
                .unwrap()
                .mountain_size,
            7
        );
        assert_eq!(retained.materialization().cliffs(), &[None]);
    }

    #[test]
    fn a_failed_refresh_cannot_replace_a_valid_source_plane() {
        let mut map = map();
        map.retain_gather_terrain_sources(RULES_XML.to_vec(), stamp(&map), Vec::new(), Vec::new())
            .unwrap();

        let invalid = mountain(map.w / TILES_PER_WCELL, 0);
        assert!(matches!(
            map.retain_gather_terrain_sources(
                RULES_XML.to_vec(),
                stamp(&map),
                vec![Some(invalid)],
                Vec::new(),
            ),
            Err(GatherTerrainRetentionError::Materialization(
                GatherTerrainMaterializationError::InvalidObjectCoordinate { .. }
            ))
        ));
        let retained = map.gather_terrain_sources().unwrap();
        assert!(retained.materialization().mountains().is_empty());
        assert!(retained.materialization().cliffs().is_empty());
    }

    #[test]
    fn map_shape_must_match_the_retail_four_tiles_per_world_cell() {
        let mut params = MapParams::default();
        params.size = 95;
        let mut map = Map::generate(params, spatial());
        assert_eq!(
            map.retain_gather_terrain_sources(
                RULES_XML.to_vec(),
                stamp(&map),
                Vec::new(),
                Vec::new(),
            ),
            Err(GatherTerrainRetentionError::IncompatibleMapDimensions {
                tile_xs: 95,
                tile_ys: 95,
            })
        );
        assert_eq!(
            map.gather_terrain_sources().unwrap_err(),
            GatherTerrainUnavailable::NonRetailGenerator
        );
    }
}
