#[allow(dead_code)]
#[path = "../src/setup_place_unit_deep_re.rs"]
mod setup_place_unit_deep_re;
#[path = "../src/unit_init_collision_tail_deep_re.rs"]
mod tail;
#[allow(dead_code)]
#[path = "../src/unit_init_location_deep_re.rs"]
mod unit_init_location_deep_re;

use don_sim::systems::casters_animals::ManaCapacityInput;
use don_sim::systems::collision::block_get;
use don_sim::systems::graphics_turret::{ExtractedGuyGraphics, GraphicsProvenance};
use don_sim::systems::groups_guys::UnitTypeStats;
use don_sim::systems::map_terrain::World;
use don_sim::systems::unit_inctime::{SUPPORTED_RETAIL_EXE_SHA256, SUPPORTED_UNIT_GRAPHICS_SHA256};
use setup_place_unit_deep_re::{
    produce_unit_guy_init_prefix, GuyGraphicsInitReceipt, GuyInitPredicateFacts,
    StableUnitIdentity, UnitGuyInitInputs,
};
use tail::*;
use unit_init_location_deep_re::{
    produce_unit_init_location_continuation, TerrainHeightReceipt, TerrainQueryKind,
    UnitInitLocationInputs, GUY_TERRAIN_Z_CALL_VA, TERRAIN_FIND_DATA_Z_VA,
    TERRAIN_FIND_TCOORD_Z_VA, UNIT_TERRAIN_Z_CALL_VA,
};

fn identity() -> StableUnitIdentity {
    StableUnitIdentity {
        id: 1_001,
        generation: 4,
        owner: 2,
        o: 31,
        type_index: 0x32,
    }
}

fn graphics(radius: i32) -> GuyGraphicsInitReceipt {
    GuyGraphicsInitReceipt {
        provenance: GraphicsProvenance {
            executable_sha256: SUPPORTED_RETAIL_EXE_SHA256,
            installed_unit_graphics_sha256: SUPPORTED_UNIT_GRAPHICS_SHA256,
            coherent_capture: true,
        },
        extracted: ExtractedGuyGraphics {
            guy_num: 0,
            gpiece: 700 + radius,
            pivot_graph_name: None,
            track_dx: 0,
            track_dy: 0,
            turret_angles: [0; 4],
            des_turret_angles: [0; 4],
            node_flags: 0,
            des_node_flags: 0,
        },
        restriction_count: 0,
    }
}

fn location(radius: i32) -> unit_init_location_deep_re::UnitInitLocationReceipt {
    let anchor = (10 * 48 + 24, 11 * 48 + 24);
    let graph = graphics(radius);
    let prefix = produce_unit_guy_init_prefix(
        UnitGuyInitInputs {
            identity: identity(),
            squad_size: 1,
            crew_size: 0,
            graphics: vec![graph.clone()],
            predicates: vec![GuyInitPredicateFacts {
                valid_animation: [true; 4],
                ..GuyInitPredicateFacts::default()
            }],
        },
        0x1234_5678,
    )
    .unwrap();
    produce_unit_init_location_continuation(UnitInitLocationInputs {
        prefix,
        graphics: vec![graph],
        unit_type: UnitTypeStats {
            domain: 0,
            guy_spacing: 192,
            new_block_radius: radius,
            squad_size: 1,
            crew_size: 0,
            ..UnitTypeStats::default()
        },
        formation: 0,
        unit_masks: 0,
        domain_two_tracks_ground: false,
        anchor_x: anchor.0,
        anchor_y: anchor.1,
        world_max_x: 16 * 0x300,
        world_max_y: 16 * 0x300,
        terrain: vec![
            TerrainHeightReceipt {
                ordinal: 0,
                call_va: UNIT_TERRAIN_Z_CALL_VA,
                body_va: TERRAIN_FIND_TCOORD_Z_VA,
                kind: TerrainQueryKind::UnitTcoord,
                x: anchor.0 / 192,
                y: anchor.1 / 192,
                final_arg: 1,
                returned_z: 80,
            },
            TerrainHeightReceipt {
                ordinal: 1,
                call_va: GUY_TERRAIN_Z_CALL_VA,
                body_va: TERRAIN_FIND_DATA_Z_VA,
                kind: TerrainQueryKind::GuyCoord,
                x: anchor.0,
                y: anchor.1,
                final_arg: 0,
                returned_z: 81,
            },
        ],
    })
    .unwrap()
}

fn scalar(field: UnitTailScalarField, returned: i32) -> UnitTailScalarReceipt {
    let (call_va, body_va) = match field {
        UnitTailScalarField::MyHits => (UNIT_UPDATE_HITS_CALL_VA, UNIT_UPDATE_HITS_VA),
        UnitTailScalarField::MyLos => (UNIT_UPDATE_LOS_CALL_VA, UNIT_UPDATE_LOS_VA),
        UnitTailScalarField::MySpeed => (UNIT_UPDATE_SPEED_CALL_VA, UNIT_UPDATE_SPEED_VA),
        UnitTailScalarField::MyArmor => (UNIT_UPDATE_ARMOR_CALL_VA, UNIT_UPDATE_ARMOR_VA),
    };
    UnitTailScalarReceipt {
        call_va,
        body_va,
        field,
        returned,
    }
}

fn inputs(radius: i32) -> UnitInitCollisionTailInputs {
    UnitInitCollisionTailInputs {
        location: location(radius),
        type_facts: UnitTailTypeFacts {
            domain: 0,
            type_id: 0x32,
            type_line: 0x1ab,
            is_15f_strict: true,
            is_3a: true,
            is_143: true,
            ..UnitTailTypeFacts::default()
        },
        leader: UnitTailLeaderFacts {
            leader_flags_before: 0,
            lakota: true,
            lakota_food: 4,
            ..UnitTailLeaderFacts::default()
        },
        stats: UnitTailStatReceipts {
            hits: scalar(UnitTailScalarField::MyHits, 125),
            los: scalar(UnitTailScalarField::MyLos, 7),
            speed: scalar(UnitTailScalarField::MySpeed, 31),
            armor: scalar(UnitTailScalarField::MyArmor, 4),
        },
        mana: ManaCapacityInput {
            base_mana: 501,
            is_air: false,
            has_space_program: false,
            space_air_percent: 0,
            is_supply: false,
            supply_upgrade: 0,
            has_special_craft_bonus: false,
            is_general: false,
            special_craft_percent: 0,
        },
        unit_masks2_before_tail: 0,
        stance_before_tail: 1,
        object_flags: 1,
    }
}

#[test]
fn native_extent_calls_and_store_bits_are_frozen() {
    assert_eq!(UNIT_INIT_POST_LOCATION_BEGIN_VA, 0x0061_2cd9);
    assert_eq!(UNIT_INIT_POST_LOCATION_RETURN_VA, 0x0061_2f7f);
    assert_eq!(OBJECT_DATA_CAN_CARRY_VA, 0x0064_6c40);
    assert_eq!(UNIT_UPDATE_HITS_VA, 0x0060_e930);
    assert_eq!(UNIT_UPDATE_LOS_VA, 0x0060_e4d0);
    assert_eq!(UNIT_UPDATE_SPEED_VA, 0x0060_55c0);
    assert_eq!(UNIT_UPDATE_ARMOR_VA, 0x0060_54c0);
    assert_eq!(OBJECT_UPDATE_SEEN_VA, 0x0065_1b80);
    assert_eq!(OBJECT_DATA_IS_FORWARDER_VA, 0x0065_3790);
    assert_eq!(UNIT_LAKOTA_IS_45_CALL_VA, 0x0061_2ee4);
    assert_eq!(UNIT_AMERICANS_IS_45_CALL_VA, 0x0061_2f3f);
    assert_eq!(
        (
            LEADER_ACTIVE_UNIT_FLAG,
            LEADER_STARTING_ECON_FLAG,
            UNIT_CAN_CARRY_AIR_FLAG,
            UNIT_DEFAULT_MAP_FLAG,
        ),
        (0x0080_0000, 0x0200_0000, 0x0020_0000, 0x0004_0000)
    );
}

#[test]
fn complete_normal_land_chain_commits_collision_and_common_tail() {
    let mut world = World::init_default_rules(16, 16);
    let receipt = produce_unit_init_collision_tail(&mut world, inputs(1)).unwrap();

    assert_eq!(receipt.identity, identity());
    assert_eq!(receipt.rng_before, receipt.rng_after);
    assert_eq!(receipt.collision_requests.len(), 1);
    assert_eq!(receipt.collision_deltas.len(), 1);
    assert!(receipt.collision_deltas[0].allocated);
    assert_eq!(receipt.collision_deltas[0].changed_bits.len(), 9);

    let request = receipt.collision_requests[0];
    let (bx, by) = (request.new_ucoord.0 >> 4, request.new_ucoord.1 >> 4);
    let block = world.wdata[world.w_index(bx, by)].block.as_deref().unwrap();
    for dx in -1..=1 {
        for dy in -1..=1 {
            assert!(block_get(
                block,
                request.new_ucoord.0 + dx,
                request.new_ucoord.1 + dy
            ));
        }
    }

    assert_eq!(
        (
            receipt.unit.collide_frame,
            receipt.unit.collide,
            receipt.unit.collide_o,
            receipt.unit.collide_guy,
            receipt.unit.collide_who,
        ),
        (-1, 0, -1, -1, -1)
    );
    assert_eq!(
        (
            receipt.unit.myhits,
            receipt.unit.mylos,
            receipt.unit.myspeed,
            receipt.unit.myarmor,
        ),
        (125, 7, 31, 4)
    );
    assert_eq!(receipt.unit.spell_time, 250);
    assert_eq!(receipt.unit.stance, 3);
    assert_eq!(
        receipt.unit.unit_masks,
        UNIT_CAN_CARRY_AIR_FLAG | UNIT_DEFAULT_MAP_FLAG
    );
    assert_eq!(receipt.unit.unit_masks2, UNIT_STANCE_CAPABLE_FLAG);
    assert_eq!(
        receipt.leader_flags_after,
        LEADER_ACTIVE_UNIT_FLAG | LEADER_STARTING_ECON_FLAG
    );
    assert_eq!(
        receipt.calls.first().unwrap().kind,
        UnitTailCallKind::CollisionMove
    );
    assert_eq!(
        receipt.calls.last().unwrap().kind,
        UnitTailCallKind::AmericansBonus
    );
    assert!(receipt
        .calls
        .iter()
        .enumerate()
        .all(|(i, c)| c.ordinal as usize == i));
    assert!(!receipt
        .calls
        .iter()
        .any(|c| c.kind == UnitTailCallKind::IsType45Lakota));
    assert_eq!(
        receipt.next_external_residual,
        UnitInitTailExternalResidual::ObjectUpdateSeen(receipt.visibility)
    );
    assert_eq!(receipt.visibility.source_mylos, receipt.unit.mylos);
    assert_eq!(receipt.visibility.type_domain, 0);
    assert_eq!(receipt.visibility.unit_masks, UNIT_CAN_CARRY_AIR_FLAG);
}

#[test]
fn zero_radius_preserves_the_exact_collision_noop_call() {
    let mut world = World::init_default_rules(16, 16);
    let receipt = produce_unit_init_collision_tail(&mut world, inputs(0)).unwrap();
    assert_eq!(receipt.collision_requests.len(), 1);
    assert!(receipt.collision_deltas.is_empty());
    assert_eq!(receipt.calls[0].kind, UnitTailCallKind::CollisionMove);
    assert!(world.wdata.iter().all(|w| w.block.is_none()));
}

#[test]
fn leader_flag_four_suppresses_default_map_bit_and_stance_write() {
    let mut world = World::init_default_rules(16, 16);
    let mut i = inputs(1);
    i.leader.leader_flags_before = 4;
    i.type_facts.is_3a = false;
    i.type_facts.is_143 = true;
    i.type_facts.is_15f_strict = false;
    let receipt = produce_unit_init_collision_tail(&mut world, i).unwrap();
    assert_eq!(receipt.unit.unit_masks, 0);
    assert_eq!(receipt.unit.spell_time, 0);
    assert_eq!(receipt.unit.stance, 1);
    assert!(!receipt
        .calls
        .iter()
        .any(|c| c.kind == UnitTailCallKind::Mana || c.kind == UnitTailCallKind::SetStance));
}

#[test]
fn americans_and_lakota_use_their_distinct_native_type_gates() {
    let mut world = World::init_default_rules(16, 16);
    let mut i = inputs(1);
    i.leader.lakota = false;
    i.leader.americans = true;
    i.leader.americans_barracks_gather = 2;
    i.type_facts.type_id = 0x99;
    i.type_facts.type_line = 0x1ab;
    let yes = produce_unit_init_collision_tail(&mut world, i.clone()).unwrap();
    assert_ne!(yes.leader_flags_after & LEADER_STARTING_ECON_FLAG, 0);
    assert!(yes
        .calls
        .iter()
        .any(|c| c.kind == UnitTailCallKind::IsType45Americans));

    let mut other_world = World::init_default_rules(16, 16);
    i.type_facts.is_45 = true;
    let no = produce_unit_init_collision_tail(&mut other_world, i).unwrap();
    assert_eq!(no.leader_flags_after & LEADER_STARTING_ECON_FLAG, 0);
    assert_eq!(
        no.calls
            .iter()
            .filter(|c| c.kind == UnitTailCallKind::IsType45Americans)
            .count(),
        1
    );
}

#[test]
fn lakota_type_query_is_emitted_only_after_native_id_short_circuit() {
    let mut world = World::init_default_rules(16, 16);
    let mut i = inputs(1);
    i.type_facts.type_id = 0x99;
    i.type_facts.type_line = 0x1ac;
    i.type_facts.is_45 = false;
    let receipt = produce_unit_init_collision_tail(&mut world, i).unwrap();
    assert_ne!(receipt.leader_flags_after & LEADER_STARTING_ECON_FLAG, 0);
    let bonus = receipt
        .calls
        .iter()
        .position(|c| c.kind == UnitTailCallKind::LakotaBonus)
        .unwrap();
    let query = receipt
        .calls
        .iter()
        .position(|c| c.kind == UnitTailCallKind::IsType45Lakota)
        .unwrap();
    assert_eq!(query, bonus + 1);
    assert_eq!(receipt.calls[query].call_va, UNIT_LAKOTA_IS_45_CALL_VA);
}

#[test]
fn bad_scalar_receipt_is_rejected_before_any_world_commit() {
    let mut world = World::init_default_rules(16, 16);
    let before = world.clone();
    let mut i = inputs(1);
    i.stats.los.body_va ^= 4;
    assert_eq!(
        produce_unit_init_collision_tail(&mut world, i).unwrap_err(),
        UnitInitCollisionTailError::InvalidStatReceipt {
            field: UnitTailScalarField::MyLos
        }
    );
    assert_eq!(world.checksum(), before.checksum());
    assert!(world.wdata.iter().all(|w| w.block.is_none()));
}

#[test]
fn non_land_receipts_are_not_silently_reinterpreted() {
    let mut world = World::init_default_rules(16, 16);
    let mut i = inputs(1);
    i.type_facts.domain = 1;
    assert_eq!(
        produce_unit_init_collision_tail(&mut world, i).unwrap_err(),
        UnitInitCollisionTailError::UnsupportedNonLandDomain
    );
}
