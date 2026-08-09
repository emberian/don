use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use don_sim::systems::building_gather::{
    calc_shipped_farm_gather, FarmGatherError, FarmGatherRequest, FarmGatherRules,
};
use don_sim::systems::gathering::{
    AuthoritativeGatherTerrain, GatherTile, GatherWorldCell, LandGatherData, LandGatherSlot,
    MiningObjectCandidate, MiningObjectKind,
};
use don_sim::systems::map_terrain::{tflag, Coord};

#[derive(Default)]
struct FarmTerrain {
    tiles: (i32, i32),
    territory: BTreeMap<(i32, i32), i32>,
    land: BTreeMap<(i32, i32), LandGatherData>,
    rivers: BTreeSet<(i32, i32)>,
    alliances: BTreeMap<(i32, i32), bool>,
    trace: RefCell<Vec<String>>,
}

impl FarmTerrain {
    fn shipped_plain() -> Self {
        let land = LandGatherData {
            slots: [
                LandGatherSlot {
                    make: 3,
                    num_make: 1,
                },
                LandGatherSlot {
                    make: 0,
                    num_make: 1,
                },
                LandGatherSlot {
                    make: -1,
                    num_make: 0,
                },
                LandGatherSlot {
                    make: -1,
                    num_make: 0,
                },
            ],
        };
        let mut host = Self {
            tiles: (32, 32),
            ..Self::default()
        };
        for wx in 0..8 {
            for wy in 0..8 {
                host.territory.insert((wx, wy), -1);
                host.land.insert((wx, wy), land);
            }
        }
        host
    }
}

impl AuthoritativeGatherTerrain for FarmTerrain {
    fn world_cell_dimensions(&self) -> (i32, i32) {
        (8, 8)
    }
    fn tile_dimensions(&self) -> (i32, i32) {
        self.tiles
    }

    fn tile_mask(&self, tile: GatherTile) -> Option<u16> {
        self.trace
            .borrow_mut()
            .push(format!("mask:{},{}", tile.tx, tile.ty));
        (tile.tx >= 0 && tile.ty >= 0 && tile.tx < self.tiles.0 && tile.ty < self.tiles.1)
            .then_some(if self.rivers.contains(&(tile.tx, tile.ty)) {
                tflag::RIVER
            } else {
                0
            })
    }

    fn territory_owner(&self, wx: i32, wy: i32) -> Option<i32> {
        self.trace.borrow_mut().push(format!("who:{wx},{wy}"));
        self.territory.get(&(wx, wy)).copied()
    }

    fn world_cell_flags(&self, _wx: i32, _wy: i32) -> Option<u16> {
        Some(0)
    }

    fn is_allied(&self, site_owner: i32, territory_owner: i32) -> Option<bool> {
        self.trace
            .borrow_mut()
            .push(format!("ally:{site_owner},{territory_owner}"));
        self.alliances.get(&(site_owner, territory_owner)).copied()
    }

    fn nearest_mining_object(
        &self,
        _kind: MiningObjectKind,
        _site_x: Coord,
        _site_y: Coord,
        _site_region: i16,
    ) -> Option<MiningObjectCandidate> {
        None
    }

    fn mining_object_tiles(&self, _kind: MiningObjectKind, _index: i32) -> Option<&[GatherTile]> {
        None
    }

    fn mountain_solid_world_cells(&self, _index: i32) -> Option<&[GatherWorldCell]> {
        None
    }

    fn land_gather_data(&self, wx: i32, wy: i32) -> Option<LandGatherData> {
        self.trace.borrow_mut().push(format!("land:{wx},{wy}"));
        self.land.get(&(wx, wy)).copied()
    }
}

#[test]
fn shipped_farm_walks_six_slot_land_data_and_filters_to_food() {
    let host = FarmTerrain::shipped_plain();
    let request = FarmGatherRequest::shipped_completed(GatherTile { tx: 0, ty: 0 }, 2);
    let result = calc_shipped_farm_gather(&host, &FarmGatherRules::shipped(), request).unwrap();

    assert_eq!(result.footprint_resources, [16, 0, 0, 16, 0, 0]);
    assert_eq!(result.per_worker, 160);
    assert_eq!(result.gross, [160, 0, 0, 0, 0, 0]);
    assert_eq!(result.max_gatherers, 1);

    // Retail gets LandData before querying the fine-tile river flag and walks y inside x.
    assert_eq!(
        &host.trace.borrow()[..9],
        [
            "who:0,0", "land:0,0", "mask:0,0", "who:0,0", "land:0,0", "mask:0,1", "who:0,0",
            "land:0,0", "mask:0,2",
        ]
    );
}

#[test]
fn river_rule_city_and_tribe_inputs_are_mutation_sensitive() {
    let mut host = FarmTerrain::shipped_plain();
    host.rivers.insert((0, 0));
    let mut request = FarmGatherRequest::shipped_completed(GatherTile { tx: 0, ty: 0 }, 2);
    let baseline = calc_shipped_farm_gather(&host, &FarmGatherRules::shipped(), request).unwrap();
    assert_eq!(baseline.footprint_resources[0], 17);
    assert_eq!(baseline.per_worker, 170);

    request.city_enhancer_percent[0] = 120;
    request.has_japanese_fishing_bonus = true;
    request.has_egyptian_farm_bonus = true;
    let boosted = calc_shipped_farm_gather(&host, &FarmGatherRules::shipped(), request).unwrap();
    assert_eq!(boosted.per_worker, 255); // 170 * 120 / 100, then * 125 / 100
    assert_eq!(boosted.gross, [255, 0, 32, 0, 0, 0]);

    let mut mutated_rules = FarmGatherRules::shipped();
    mutated_rules.river_resource_value = 3;
    let mutated = calc_shipped_farm_gather(&host, &mutated_rules, request).unwrap();
    assert_eq!(mutated.footprint_resources[0], 18);
    assert_eq!(mutated.per_worker, 270);
    assert_eq!(mutated.gross, [270, 0, 32, 0, 0, 0]);
}

#[test]
fn hostile_cells_short_circuit_before_land_and_river_reads() {
    let mut host = FarmTerrain::shipped_plain();
    host.territory.insert((0, 0), 5);
    host.alliances.insert((2, 5), false);
    let request = FarmGatherRequest::shipped_completed(GatherTile { tx: 0, ty: 0 }, 2);
    let result = calc_shipped_farm_gather(&host, &FarmGatherRules::shipped(), request).unwrap();
    assert_eq!(result.gross, [0; 6]);
    let trace = host.trace.borrow();
    assert_eq!(&trace[..2], ["who:0,0", "ally:2,5"]);
    assert!(!trace.iter().any(|entry| entry.starts_with("land:")));
    assert!(!trace.iter().any(|entry| entry.starts_with("mask:")));
}

#[test]
fn missing_or_unsupported_inputs_fail_closed() {
    let host = FarmTerrain::shipped_plain();
    let mut request = FarmGatherRequest::shipped_completed(GatherTile { tx: 0, ty: 0 }, 2);
    request.type_index = 419;
    assert_eq!(
        calc_shipped_farm_gather(&host, &FarmGatherRules::shipped(), request),
        Err(FarmGatherError::UnsupportedType(419))
    );

    request = FarmGatherRequest::shipped_completed(GatherTile { tx: 0, ty: 0 }, 2);
    request.x_size = 2;
    assert_eq!(
        calc_shipped_farm_gather(&host, &FarmGatherRules::shipped(), request),
        Err(FarmGatherError::UnsupportedFootprint {
            x_size: 2,
            y_size: 4
        })
    );

    request = FarmGatherRequest::shipped_completed(GatherTile { tx: 0, ty: 0 }, 2);
    request.active_gatherers = 2;
    assert_eq!(
        calc_shipped_farm_gather(&host, &FarmGatherRules::shipped(), request),
        Err(FarmGatherError::ActiveGatherersOutOfRange(2))
    );

    let mut missing_diplomacy = FarmTerrain::shipped_plain();
    missing_diplomacy.territory.insert((0, 0), 5);
    request.active_gatherers = 1;
    assert_eq!(
        calc_shipped_farm_gather(&missing_diplomacy, &FarmGatherRules::shipped(), request),
        Err(FarmGatherError::MissingDiplomacy {
            owner: 2,
            territory_owner: 5
        })
    );

    request = FarmGatherRequest::shipped_completed(
        GatherTile {
            tx: i32::MAX,
            ty: 0,
        },
        2,
    );
    assert_eq!(
        calc_shipped_farm_gather(&host, &FarmGatherRules::shipped(), request),
        Err(FarmGatherError::FootprintCoordinateOverflow(GatherTile {
            tx: i32::MAX,
            ty: 0,
        }))
    );
}

#[test]
fn rules_block_uses_the_four_disassembled_byte_offsets() {
    let mut block = vec![0_i32; 0xC40 / 4 + 1];
    block[0x280 / 4] = 111;
    block[0x658 / 4] = 222;
    block[0x7AC / 4] = 333;
    block[0xC40 / 4] = 444;
    assert_eq!(
        FarmGatherRules::from_rules_block(&block).unwrap(),
        FarmGatherRules {
            peasant_rate_8_8: 111,
            egyptian_farm_wealth: 222,
            japanese_fishing_boats_percent: 333,
            river_resource_value: 444,
        }
    );
    assert!(matches!(
        FarmGatherRules::from_rules_block(&block[..block.len() - 1]),
        Err(FarmGatherError::RulesBlockTooShort { .. })
    ));
}

#[test]
fn shipped_constructor_matches_generated_rules_asset() {
    let generated = don_rules::Rules::shipped();
    assert_eq!(
        FarmGatherRules::from_rules_block(&generated.raw).unwrap(),
        FarmGatherRules::shipped()
    );
}
