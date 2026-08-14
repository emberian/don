use don_sim::order::{
    MoveOrderState, Order, OrderIndex, ORDER_PATHED, RETAIL_MOVE_ORDER_NODE_BYTES,
};
use don_sim::systems::canonical_gather_work::*;
use don_sim::systems::economy_order_payload_authority::{
    EconomyOrderHeader, EconomyOrderNode, EconomyOrderPayload, GatherOrderPayload,
    StableTargetIdentity,
};
use don_sim::systems::groups_guys::{GuyData, UnitGuys, GUY_WALK_LEN};
use don_sim::systems::{movement, movement_live, production, save_load};
use don_sim::tick::Sim;

const FRAME: i32 = 1_199;
const WHO: u8 = 2;
const ACTOR_O: i16 = 9;
const ACTOR_UID: u16 = 18;
const SITE_O: i16 = 2_002;
const SITE_UID: u16 = 2;
const FARM_INDEX: i16 = 6;

fn hex<const N: usize>(text: &str) -> [u8; N] {
    assert_eq!(text.len(), N * 2);
    let mut out = [0; N];
    for (index, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).unwrap();
    }
    out
}

fn retail_guy_image() -> [u8; GUY_WALK_LEN] {
    // Fresh SVX 0x59980..0x59a1b; SHA-256
    // b74f23ab73abd60305923d6659f7cdf8229c3262006a059a733da83585d86e15.
    hex(
        "32000000dd070000b07f00006f02000000001c4d00001c4d000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000dd070000b07f000000001c4dc6070000a87f00006f020000030000000f00000002000000180000000d000000c01800000900ffff000000000000000000001000080000ff000200",
    )
}

fn retail_farm_image() -> [u8; FarmStruct::WALKED_BYTES] {
    // Fresh SVX Farm index 6 at 0xe3a49..0xe3b07.
    hex(
        "02000000d2070000000000000000803f0000803f0000803f000000000000803f0000803f0000803f0000803f00000000000000000000803f000000000000803f0000803f0000000000401d4400401c440080224400002544004026440000144400c0184400c01e440040254400c02544000014440040154400001b4400802344004026440000144400c0104400401844000022440000244400001444004010440000164400401e4400c01f44000202020002020202000002000202000100",
    )
}

fn farm_from_retail_image() -> FarmStruct {
    let raw = retail_farm_image();
    let mut farm = FarmStruct {
        who: i32::from_le_bytes(raw[0..4].try_into().unwrap()),
        o: i32::from_le_bytes(raw[4..8].try_into().unwrap()),
        valid: raw[188],
        farm_type: raw[189],
        ..Default::default()
    };
    for index in 0..16 {
        farm.percent[index] =
            u32::from_le_bytes(raw[8 + index * 4..12 + index * 4].try_into().unwrap());
    }
    for index in 0..25 {
        farm.terrain_height[index] =
            u32::from_le_bytes(raw[72 + index * 4..76 + index * 4].try_into().unwrap());
    }
    farm.status.copy_from_slice(&raw[172..188]);
    assert_eq!(farm.image(), raw);
    farm
}

fn move_order() -> Order {
    Order {
        node_metric: 0,
        kind: OrderIndex::MoveTo,
        flags: ORDER_PATHED,
        x: 2_232,
        y: 32_760,
        tolerance: 0,
        move_state: Some(MoveOrderState {
            angle: 0x4ad3_0000,
            dest: 1,
            facing: -1,
            dest_x: 2_232,
            dest_y: 32_760,
            last_x: -1,
            last_y: -1,
            orig_x: -1,
            orig_y: -1,
            off_x: 696,
            off_y: 504,
            ..MoveOrderState::default()
        }),
        ..Order::default()
    }
}

fn gather_node() -> EconomyOrderNode {
    EconomyOrderNode {
        metric: 0,
        header: EconomyOrderHeader {
            kind: OrderIndex::Gather,
            flags: 0,
            x: 0,
            y: 0,
            primary: StableTargetIdentity::banded(i32::from(SITE_O), i32::from(WHO), SITE_UID),
        },
        payload: EconomyOrderPayload::Gather(GatherOrderPayload {
            tx: -1,
            ty: -1,
            build_type: FARM_PROPERTY,
            wait: 0,
            goto_build: 1,
            non_flat_gather: 0,
            dist_mod: 0,
            been_there: 1,
        }),
    }
}

fn build(o: i16, uid: u16, farm_index: i16) -> production::BuildData {
    let mut build = production::BuildData {
        flags: production::flag::VALID | production::flag::STARTED | production::flag::ACTIVE,
        who: WHO,
        uid,
        orig_type: if farm_index >= 0 { FARM_PROPERTY } else { 0 },
        gather_down: -1,
        city: 0,
        city_down: -1,
        wonder: -1,
        dock: farm_index,
        attack_ox: -1,
        attack_whom: -1,
        ..Default::default()
    };
    build.other[0x0a..0x0c].copy_from_slice(&o.to_le_bytes());
    build.other[0x28..0x2a].copy_from_slice(&(-1i16).to_le_bytes());
    build
}

fn unit_guys_walk_image(guys: &UnitGuys) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(guys.guys.len() as i32).to_le_bytes());
    if guys.guys.is_empty() {
        return out;
    }
    out.extend_from_slice(&guys.size.to_le_bytes());
    out.extend_from_slice(&guys.increment.to_le_bytes());
    out.push(guys.flags & !0x40);
    out.extend(guys.guys.iter().map(|guy| u8::from(guy.is_some())));
    out.extend_from_slice(&guys.size.to_le_bytes());
    out.extend_from_slice(&guys.increment.to_le_bytes());
    for guy in guys.guys.iter().flatten() {
        out.extend_from_slice(&guy.walk_bytes());
    }
    out
}

fn retail_order_list_image(move_order: &Order, gather: EconomyOrderNode) -> Vec<u8> {
    let mut out = 2i32.to_le_bytes().to_vec();
    out.extend_from_slice(&move_order.retail_move_node_image().unwrap());
    out.extend_from_slice(&gather.retail_node_image().unwrap());
    out
}

fn fixture() -> (Sim, usize) {
    let mut sim = Sim::new(0x0148_10ac, 512);
    sim.map.world.seed = 0x0148_10ac;
    let mut actor = None;
    for _ in 0..=ACTOR_O {
        actor = Some(sim.spawn_unit(WHO as usize, 50, 2_013, 32_688, 4).unwrap());
    }
    let actor = actor.unwrap();
    let actor_row = sim.world.row_of(actor).unwrap();
    sim.world.units.set_uid(actor_row, ACTOR_UID);
    sim.world.units.x_internal_mut()[actor_row] = 2_013;
    sim.world.units.y_internal_mut()[actor_row] = 32_688;
    sim.world.units.angle_mut()[actor_row] = 0x4d1c_0000;
    sim.world.units.myspeed_mut()[actor_row] = 25;
    sim.world.units.set_unit_masks(actor_row, 0x0004_0008);
    sim.world.units.group_mut()[actor_row] = -1;
    sim.world.frame = FRAME;
    sim.vic_match.frame = FRAME;

    let guy = GuyData::from_walk_bytes(retail_guy_image());
    sim.install_unit_guys(
        actor,
        UnitGuys {
            guys: vec![Some(guy)],
            size: 1,
            increment: 1,
            flags: 0,
            guy_mark: 1,
        },
    )
    .unwrap();

    sim.paths[actor_row] = movement::PathStack::with_header(10, 10).unwrap();
    sim.paths[actor_row].push(movement::PathData {
        to_x: 2_232,
        to_y: 32_760,
        tolerance: 0,
        flags: movement::PathData::FLAG_MORE,
    });
    sim.world.orders_mut(actor_row).replace(move_order());
    sim.world
        .orders_mut(actor_row)
        .push(Order::economy(gather_node()).unwrap());

    let mut build_rows = Vec::new();
    for index in 0..=2i16 {
        build_rows.push(sim.spawn_build(
            WHO as usize,
            build(
                2_000 + index,
                index as u16,
                if index == 2 { FARM_INDEX } else { -1 },
            ),
        ));
    }
    for pair in build_rows.windows(2) {
        sim.builds[pair[0]].city_down = sim.builds[pair[1]].object_id();
    }
    let city = &mut sim.cities.slots[WHO as usize][0];
    city.city_flags = 1;
    city.city = 0;
    city.o = 2_000;
    city.who = WHO as i8;
    sim.cities.city_mark[WHO as usize] = 1;

    sim.farms = Farms::with_header(32, -1, 0);
    for index in 0..=FARM_INDEX {
        sim.farms
            .push(if index == FARM_INDEX {
                farm_from_retail_image()
            } else {
                FarmStruct::default()
            })
            .unwrap();
    }
    (sim, actor_row)
}

fn authority(sim: &Sim, actor_row: usize) -> GatherWorkAuthority {
    let actor = sim.world.handle_at_row(actor_row).unwrap();
    GatherWorkAuthority {
        revision: 0x005f_7b30,
        composition_digest: [0x9d; 32],
        actors: vec![GatherActorRuntimeFacts {
            actor,
            who: WHO,
            o: ACTOR_O,
            uid: ACTOR_UID,
            type_index: 50,
            guys_length: 1,
            guys_capacity: 1,
            guys_increment: 1,
            guys_flags: 0,
            slot_zero_present: true,
            lead_animation: 8,
            move_runtime: Some(GatherMoveRuntimeFacts {
                expected_unit_masks: 0x0004_0008,
                expected_myspeed: 25,
                type_unit_flags: 0x0000_1881, // `<FLAGS>lmah</FLAGS>`
                type_special_wide_turner: false,
                unit_type: don_sim::systems::groups_guys::UnitTypeStats {
                    domain: 0,
                    guy_spacing: 12,
                    x_spacing: 12,
                    y_spacing: 12,
                    guy_radius: 1,
                    new_block_radius: 1,
                    turn_speed: 0x2000_0000,
                    squad_size: 1,
                    uber_size: 1,
                    ..Default::default()
                },
                turn_scale: 256,
                turn_scale2: 2,
                ai_speed: 1,
                lead_gpiece: 6_336,
            }),
        }],
        sites: vec![GatherSiteRuntimeFacts {
            who: WHO,
            o: SITE_O,
            uid: SITE_UID,
            property: FARM_PROPERTY,
            resolves_build: true,
            valid_wall_projection: true,
        }],
        farms: vec![GatherFarmRuntimeFacts {
            actor,
            site_who: WHO,
            site_o: SITE_O,
            site_uid: SITE_UID,
            farm_index: FARM_INDEX,
            farm_record_who: WHO,
            farm_record_o: SITE_O,
            farm_record_valid: true,
            farm_type: 0,
            covers_actor_tile: true,
            corner_tx: 8,
            corner_ty: 168,
            x_size: 4,
            y_size: 4,
            selected_cell_status: Some(2),
            lead_cur_time: 14,
            lead_end_time: 15,
            lead_hold_attack: 0,
        }],
    }
}

fn source(sim: &Sim, row: usize) -> movement_live::LiveCollisionSource {
    movement_live::LiveCollisionSource {
        domain: 0,
        block_radius: 1,
        big_radius: 48,
        push_size: 1,
        push_circles: 1,
        unit_flags: 0x0000_1881,
        unit_flags2: 0,
        attack_value: 4,
        spell_id: -1,
        unpacking: false,
        captain: false,
        moving: row == ACTOR_O as usize,
        searching: false,
        action: if row == ACTOR_O as usize {
            OrderIndex::Gather as i32
        } else {
            OrderIndex::None as i32
        },
        invalid_tiles: Vec::new(),
        guys: vec![movement_live::LiveCollisionGuy {
            x: sim.world.units.x_internal()[row],
            y: sim.world.units.y_internal()[row],
            angle: sim.world.units.angle()[row],
            block_radius: 1,
        }],
    }
}

fn arm(mut sim: Sim, actor_row: usize, rehydrate: bool) -> Sim {
    // The reduced fixture's first nine placeholder rows do not represent the fresh save's
    // earlier active citizens; they exist only to preserve owner-local o=9 identity.
    for row in 0..actor_row {
        sim.world.units.set_flags(row, 0);
    }
    sim.replace_gather_work_authority(authority(&sim, actor_row));
    let actor = sim.world.handle_at_row(actor_row).unwrap();
    let installed = source(&sim, actor_row);
    if rehydrate {
        sim.rehydrate_movement_collision_sources(vec![(actor, installed)])
            .unwrap();
    } else {
        sim.install_movement_collision_source(actor, installed)
            .unwrap();
    }
    sim
}

#[test]
fn owner2_o9_retail_path_queue_guy_and_farm_images_are_exact_and_resumable() {
    let (sim, actor_row) = fixture();
    let move_order = sim.world.orders(actor_row).current().unwrap();
    let move_payload = hex::<77>(
        "01b8080000f87f00000000d34a010000000000000000000000000000000000000000000000ffffffffb8080000f87f0000ffffffffffffffff0000000000000000ffffffffffffffffb802f801",
    );
    assert_eq!(move_order.retail_move_walk_image().unwrap(), move_payload);
    assert_eq!(RETAIL_MOVE_ORDER_NODE_BYTES, 82);

    let gather = gather_node();
    let gather_payload =
        hex::<31>("00d2070000020000000200ffffffffffffffffa10100000000000001000001");
    assert_eq!(gather.retail_walk_image().unwrap(), gather_payload);
    let order_list = retail_order_list_image(move_order, gather);
    assert_eq!(order_list.len(), 122);
    assert_eq!(
        order_list,
        hex::<122>(
            "02000000010000000001b8080000f87f00000000d34a010000000000000000000000000000000000000000000000ffffffffb8080000f87f0000ffffffffffffffff0000000000000000ffffffffffffffffb802f801070000000000d2070000020000000200ffffffffffffffffa10100000000000001000001",
        )
    );

    assert_eq!(
        sim.paths[actor_row].walk_bytes(),
        hex::<25>("0a000000010000000ab8080000f87f00000000000001000000")
    );
    let guys = sim.unit_guys[actor_row].as_ref().unwrap();
    assert_eq!(
        guys.guys[0].as_ref().unwrap().walk_bytes(),
        retail_guy_image()
    );
    assert_eq!(
        unit_guys_walk_image(guys),
        hex::<173>(
            "01000000010000000100000101000000010032000000dd070000b07f00006f02000000001c4d00001c4d000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000dd070000b07f000000001c4dc6070000a87f00006f020000030000000f00000002000000180000000d000000c01800000900ffff000000000000000000001000080000ff000200",
        )
    );
    assert_eq!(
        sim.farms.get(FARM_INDEX as usize).unwrap().image(),
        retail_farm_image()
    );

    let saved = save_load::save_sim(&sim).unwrap();
    let resumed = save_load::load_sim(&saved).unwrap();
    assert_eq!(save_load::save_sim(&resumed).unwrap(), saved);
    assert_eq!(resumed.world.orders(actor_row), sim.world.orders(actor_row));
    assert_eq!(resumed.paths[actor_row], sim.paths[actor_row]);
    assert_eq!(resumed.paths[actor_row].checksum_header(), (10, 1, 10));
    assert_eq!(resumed.unit_guys[actor_row], sim.unit_guys[actor_row]);
    assert_eq!(resumed.farms, sim.farms);
}

#[test]
fn production_resume_keeps_gather_queued_until_exact_locomotion_and_guy_tick_exist() {
    let (mut direct, actor_row) = fixture();
    let saved = save_load::save_sim(&direct).unwrap();
    let mut resumed = save_load::load_sim(&saved).unwrap();
    let orders_before = direct.world.orders(actor_row).clone();
    let path_before = direct.paths[actor_row].clone();
    let guys_before = direct.unit_guys[actor_row].clone();
    let farms_before = direct.farms.clone();

    direct.do_frame();
    resumed.do_frame();

    assert_eq!(direct.world.orders(actor_row), &orders_before);
    assert_eq!(
        direct.world.orders(actor_row).order_type(),
        OrderIndex::MoveTo
    );
    assert_eq!(direct.world.orders(actor_row).len(), 2);
    assert_eq!(direct.paths[actor_row], path_before);
    assert_eq!(direct.unit_guys[actor_row], guys_before);
    assert_eq!(direct.farms, farms_before);
    assert!(direct.last_gather_work_receipt.is_none());
    assert_eq!(
        direct.world.orders(actor_row),
        resumed.world.orders(actor_row)
    );
    assert_eq!(direct.paths[actor_row], resumed.paths[actor_row]);
    assert_eq!(direct.unit_guys[actor_row], resumed.unit_guys[actor_row]);
    assert_eq!(direct.farms, resumed.farms);
    assert_eq!(
        save_load::save_sim(&direct).unwrap(),
        save_load::save_sim(&resumed).unwrap()
    );
}

#[test]
fn exact_owner2_o9_move_arrival_then_animation36_farm_snip_is_resumable() {
    // The queued node's saved angle is `0x4ad30000`, but `Unit::move_step` recomputes the
    // heading from the live `(219,72)` delta and snaps this sub-IGNORE residual exactly.
    assert_eq!(movement::find_angle(219, 72), 0x4d0b_0000);
    let (fixture, actor_row) = fixture();
    let mut direct = arm(fixture, actor_row, false);
    let saved = save_load::save_sim(&direct).unwrap();
    let mut resumed = save_load::load_sim(&saved).unwrap();
    resumed = arm(resumed, actor_row, true);

    direct.do_frame();
    resumed.do_frame();
    assert_eq!(save_load::save_sim(&direct), save_load::save_sim(&resumed));
    assert_eq!(
        (
            direct.world.units.x_internal()[actor_row],
            direct.world.units.y_internal()[actor_row],
            direct.world.units.angle()[actor_row],
        ),
        (2_036, 32_696, 0x4d0b_0000)
    );
    let first_guy = direct.unit_guys[actor_row].as_ref().unwrap().guys[0]
        .as_ref()
        .unwrap();
    assert_eq!(
        (
            first_guy.last_x,
            first_guy.last_y,
            first_guy.x,
            first_guy.y,
            first_guy.last_speed,
            first_guy.avg_speed,
            first_guy.last_time,
            first_guy.cur_time,
            first_guy.angle,
            first_guy.des_angle,
            first_guy.guy_flags & don_sim::systems::groups_guys::GUY_FLAG_NO_IDLE_TURN,
        ),
        (
            2_013,
            32_688,
            2_036,
            32_696,
            24,
            15,
            3,
            4,
            0x4d0b_0000,
            0x4d0b_0000,
            0,
        )
    );

    let mut frames = 1usize;
    while direct.world.orders(actor_row).order_type() == OrderIndex::MoveTo && frames < 32 {
        direct.do_frame();
        resumed.do_frame();
        frames += 1;
        assert_eq!(save_load::save_sim(&direct), save_load::save_sim(&resumed));
    }
    assert!(frames < 32, "exact MOVE_TO did not arrive");
    assert_eq!(
        direct.world.orders(actor_row).order_type(),
        OrderIndex::Gather
    );
    assert_eq!(direct.world.orders(actor_row).len(), 1);
    assert_eq!(direct.paths[actor_row].checksum_header(), (10, 0, 10));
    assert_eq!(
        (
            direct.world.units.x_internal()[actor_row],
            direct.world.units.y_internal()[actor_row],
        ),
        (2_232, 32_760)
    );
    assert!(direct.last_gather_work_receipt.is_none());

    let arrived_guy = direct.unit_guys[actor_row].as_ref().unwrap().guys[0]
        .as_ref()
        .unwrap();
    assert_eq!(
        (
            frames,
            arrived_guy.cur_time,
            arrived_guy.end_time,
            arrived_guy.last_time,
            arrived_guy.cur_anim,
            arrived_guy.x,
            arrived_guy.y,
        ),
        (11, 14, 15, 13, 8, 2_232, 32_760)
    );

    let before = direct.farms.get(FARM_INDEX as usize).unwrap().image();
    assert_eq!(before[172 + 3 * 4 + 2], 2);
    direct.do_frame();
    resumed.do_frame();
    assert_eq!(save_load::save_sim(&direct), save_load::save_sim(&resumed));
    let receipt = direct.last_gather_work_receipt.as_ref().unwrap_or_else(|| {
        panic!(
            "Gather refused after exact arrival: {:?}",
            direct.last_gather_work_error
        )
    });
    assert_eq!(receipt.branch, GatherWorkBranch::FarmStatus2Snip);
    assert_eq!((receipt.changed_fields, receipt.guy_changed_fields), (5, 4));
    assert_eq!(
        (receipt.guy_animation_before, receipt.guy_animation_after),
        (Some(8), Some(36))
    );
    assert_eq!(receipt.rng_draws, 0);
    let after = direct.farms.get(FARM_INDEX as usize).unwrap().image();
    let changed: Vec<_> = before
        .iter()
        .zip(after.iter())
        .enumerate()
        .filter_map(|(index, (before, after))| (before != after).then_some(index))
        .collect();
    assert_eq!(changed, vec![172 + 3 * 4 + 2]);
    assert_eq!(after[172 + 3 * 4 + 2], 3);
    assert_eq!(
        direct.last_gather_work_receipt,
        resumed.last_gather_work_receipt
    );
    assert_eq!(direct.unit_guys, resumed.unit_guys);
    assert_eq!(direct.farms, resumed.farms);
}

#[test]
fn stale_exact_move_composition_is_zero_write_across_every_canonical_owner() {
    let (fixture, actor_row) = fixture();
    let mut sim = arm(fixture, actor_row, false);
    let mut stale = authority(&sim, actor_row);
    stale.actors[0]
        .move_runtime
        .as_mut()
        .unwrap()
        .expected_myspeed = 24;
    sim.replace_gather_work_authority(stale);

    let orders_before = sim.world.orders(actor_row).clone();
    let path_before = sim.paths[actor_row].clone();
    let guys_before = sim.unit_guys[actor_row].clone();
    let farms_before = sim.farms.clone();
    let unit_before = (
        sim.world.units.x_internal()[actor_row],
        sim.world.units.y_internal()[actor_row],
        sim.world.units.angle()[actor_row],
        sim.world.units.get_unit_masks(actor_row),
    );

    sim.do_frame();

    assert_eq!(sim.world.orders(actor_row), &orders_before);
    assert_eq!(sim.paths[actor_row], path_before);
    assert_eq!(sim.unit_guys[actor_row], guys_before);
    assert_eq!(sim.farms, farms_before);
    assert_eq!(
        (
            sim.world.units.x_internal()[actor_row],
            sim.world.units.y_internal()[actor_row],
            sim.world.units.angle()[actor_row],
            sim.world.units.get_unit_masks(actor_row),
        ),
        unit_before
    );
    assert!(sim.last_gather_work_receipt.is_none());
}
