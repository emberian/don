use don_sim::order::{Order, OrderIndex};
use don_sim::systems::canonical_gather_work::*;
use don_sim::systems::economy_order_payload_authority::{
    EconomyOrderHeader, EconomyOrderNode, EconomyOrderPayload, GatherOrderPayload,
    StableTargetIdentity,
};
use don_sim::systems::{production, save_load};
use don_sim::tick::Sim;

const FRAME: i32 = 1_199;
const WHO: u8 = 0;
const ACTOR_O: i16 = 7;
const ACTOR_UID: u16 = 14;
const SITE_O: i16 = 2_004;
const SITE_UID: u16 = 4;
const FARM_INDEX: i16 = 2;
const CELL_XY: usize = 2 * 4 + 1;
const TRANSPOSED_YX: usize = 1 * 4 + 2;
const PERCENT_BEFORE: u32 = 0x3e61_47a9;
const PERCENT_AFTER: u32 = 0x3e66_6661;

fn build(o: i16, uid: u16, farm_index: i16) -> production::BuildData {
    let mut build = production::BuildData {
        flags: production::flag::VALID | production::flag::STARTED | production::flag::ACTIVE,
        who: WHO,
        uid,
        orig_type: FARM_PROPERTY,
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

fn gather_order() -> Order {
    Order::economy(EconomyOrderNode {
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
    })
    .unwrap()
}

fn farm_two() -> FarmStruct {
    FarmStruct {
        who: i32::from(WHO),
        o: i32::from(SITE_O),
        percent: [
            0x3f80_0000,
            0x3f80_0000,
            0x3f80_0000,
            0,
            0,
            0x3f74_7ad5,
            0x3f80_0000,
            0x3f80_0000,
            0,
            PERCENT_BEFORE,
            0x3f80_0000,
            0x3f80_0000,
            0x3f80_0000,
            0,
            0,
            0x3f80_0000,
        ],
        terrain_height: [
            0x4346_0000,
            0x4353_0000,
            0x4366_0000,
            0x4380_0000,
            0x438a_8000,
            0x4346_0000,
            0x4344_0000,
            0x4348_0000,
            0x4359_0000,
            0x4371_0000,
            0x4343_0000,
            0x4334_0000,
            0x4334_0000,
            0x4341_0000,
            0x434c_0000,
            0x4339_0000,
            0x432a_0000,
            0x431f_0000,
            0x4323_0000,
            0x4332_0000,
            0x432e_0000,
            0x431b_0000,
            0x4314_0000,
            0x4314_0000,
            0x4321_0000,
        ],
        status: [2, 2, 2, 0, 0, 1, 2, 2, 0, 1, 2, 2, 2, 0, 0, 2],
        valid: 1,
        farm_type: 0,
    }
}

fn authority(sim: &Sim, actor_row: usize, site_row: usize) -> GatherWorkAuthority {
    let actor = sim.world.handle_at_row(actor_row).unwrap();
    let site = &sim.builds[site_row];
    GatherWorkAuthority {
        revision: 0x4652_4d58,
        composition_digest: [0x78; 32],
        actors: vec![GatherActorRuntimeFacts {
            actor,
            who: WHO,
            o: ACTOR_O,
            uid: ACTOR_UID,
            type_index: 0x32,
            guys_length: 1,
            guys_capacity: 1,
            guys_increment: 1,
            guys_flags: 0,
            slot_zero_present: true,
            lead_animation: FARM_ANIMATION_23,
        }],
        sites: vec![GatherSiteRuntimeFacts {
            who: site.who,
            o: site.object_id(),
            uid: site.uid,
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
            corner_tx: 249,
            corner_ty: 116,
            x_size: 4,
            y_size: 4,
            selected_cell_status: Some(1),
            lead_cur_time: 22,
            lead_end_time: 47,
            lead_hold_attack: 0,
        }],
    }
}

fn reinstall(sim: &Sim, mut authority: GatherWorkAuthority) -> GatherWorkAuthority {
    let row = (0..sim.world.live_count() as usize)
        .find(|&row| {
            (sim.world.units.get_who(row), sim.world.units.o()[row]) == (WHO, ACTOR_O)
                && sim.world.units.get_uid(row) == ACTOR_UID
        })
        .unwrap();
    let actor = sim.world.handle_at_row(row).unwrap();
    authority.actors[0].actor = actor;
    authority.farms[0].actor = actor;
    authority
}

fn fixture() -> (Sim, usize, usize, GatherWorkAuthority) {
    let mut sim = Sim::new(0x7133_9876, 4);
    sim.map.world.seed = 0x7133_9876;
    let mut actor_row = 0;
    for _ in 0..=ACTOR_O {
        let actor = sim
            .spawn_unit(usize::from(WHO), 0x32, 48_312, 22_584, 4)
            .unwrap();
        actor_row = sim.world.row_of(actor).unwrap();
    }
    sim.world.units.set_uid(actor_row, ACTOR_UID);
    sim.world.units.set_unit_masks(actor_row, 8);
    sim.world.units.angle_mut()[actor_row] = 0x5c32_0000;
    sim.world.units.dest_angle_mut()[actor_row] = 0x5c32_0000;
    sim.world.units.orders_x_mut()[actor_row] = 48_312;
    sim.world.units.orders_y_mut()[actor_row] = 22_584;
    sim.world.units.group_mut()[actor_row] = -1;
    sim.world.frame = FRAME;
    sim.vic_match.frame = FRAME;

    let mut rows = Vec::new();
    for index in 0..=4i16 {
        rows.push(sim.spawn_build(
            usize::from(WHO),
            build(
                2_000 + index,
                index as u16,
                if index == 4 { FARM_INDEX } else { -1 },
            ),
        ));
    }
    for pair in rows.windows(2) {
        sim.builds[pair[0]].city_down = sim.builds[pair[1]].object_id();
    }
    let city = &mut sim.cities.slots[WHO as usize][0];
    city.city_flags = 1;
    city.city = 0;
    city.o = 2_000;
    city.who = WHO as i8;
    sim.cities.city_mark[WHO as usize] = 1;

    let site_row = rows[4];
    sim.world.orders_mut(actor_row).replace(gather_order());
    sim.farms = Farms::with_header(32, -1, 0);
    sim.farms.push(FarmStruct::default()).unwrap();
    sim.farms.push(FarmStruct::default()).unwrap();
    sim.farms.push(farm_two()).unwrap();
    let authority = authority(&sim, actor_row, site_row);
    (sim, actor_row, site_row, authority)
}

#[test]
fn retail_x_then_y_cell_selects_the_second_exact_saved_grow() {
    let (mut sim, actor_row, site_row, authority) = fixture();
    assert_eq!(
        sim.farms.get(FARM_INDEX as usize).unwrap().status[CELL_XY],
        1
    );
    assert_eq!(
        sim.farms.get(FARM_INDEX as usize).unwrap().status[TRANSPOSED_YX],
        2
    );
    let order_before = sim.world.orders(actor_row).clone();
    let build_before = sim.builds[site_row].image();
    let rng_before = sim.world.random.state();
    let prepared = prepare_gather_work_activation(
        &sim.world,
        &sim.builds,
        &sim.farms,
        &sim.unit_guys,
        &sim.unit_type,
        &authority,
        actor_row,
    )
    .unwrap();
    assert_eq!(prepared.plan.branch, GatherWorkBranch::FarmStatus1Grow);
    assert_eq!(
        prepared.plan.farm_before.unwrap().percent[CELL_XY],
        PERCENT_BEFORE
    );
    assert_eq!(
        prepared.plan.farm_after.unwrap().percent[CELL_XY],
        PERCENT_AFTER
    );

    let receipt = commit_gather_work_activation(
        &mut sim.world,
        &mut sim.builds,
        &mut sim.farms,
        &mut sim.unit_guys,
        &sim.unit_type,
        &authority,
        prepared,
    )
    .unwrap();
    assert_eq!(receipt.branch, GatherWorkBranch::FarmStatus1Grow);
    assert_eq!(
        (receipt.farm_percent_before, receipt.farm_percent_after),
        (Some(PERCENT_BEFORE), Some(PERCENT_AFTER))
    );
    assert_eq!(
        sim.farms.get(FARM_INDEX as usize).unwrap().percent[CELL_XY],
        PERCENT_AFTER
    );
    assert_eq!(
        sim.farms.get(FARM_INDEX as usize).unwrap().percent[TRANSPOSED_YX],
        0x3f80_0000
    );
    assert_eq!(sim.world.orders(actor_row), &order_before);
    assert_eq!(sim.builds[site_row].image(), build_before);
    assert_eq!(sim.world.random.state(), rng_before);
}

#[test]
fn x_then_y_saved_grow_matches_v16_save_reload_resume() {
    let (mut direct, actor_row, _, authority) = fixture();
    let saved = save_load::save_sim(&direct).unwrap();
    let mut resumed = save_load::load_sim(&saved).unwrap();
    direct.replace_gather_work_authority(authority.clone());
    resumed.replace_gather_work_authority(reinstall(&resumed, authority));
    direct.do_frame();
    resumed.do_frame();
    assert_eq!(
        direct.last_gather_work_receipt,
        resumed.last_gather_work_receipt
    );
    assert_eq!(
        direct.last_gather_work_receipt.unwrap().branch,
        GatherWorkBranch::FarmStatus1Grow
    );
    assert_eq!(
        direct.farms.get(FARM_INDEX as usize).unwrap().percent[CELL_XY],
        PERCENT_AFTER
    );
    assert_eq!(
        direct.world.orders(actor_row).order_type(),
        OrderIndex::Gather
    );
    assert_eq!(
        save_load::save_sim(&direct).unwrap(),
        save_load::save_sim(&resumed).unwrap()
    );
}

#[test]
fn transposed_or_stale_cell_images_cannot_publish() {
    let (mut sim, actor_row, _, authority) = fixture();
    // Mutating only the transposed byte does not affect the executable's x-then-y lookup.
    sim.farms.get_mut(FARM_INDEX as usize).unwrap().status[TRANSPOSED_YX] = 0;
    assert!(prepare_gather_work_activation(
        &sim.world,
        &sim.builds,
        &sim.farms,
        &sim.unit_guys,
        &sim.unit_type,
        &authority,
        actor_row,
    )
    .is_ok());

    let (mut sim, actor_row, _, authority) = fixture();
    sim.farms.get_mut(FARM_INDEX as usize).unwrap().status[CELL_XY] = 2;
    let before = save_load::save_sim(&sim).unwrap();
    assert!(prepare_gather_work_activation(
        &sim.world,
        &sim.builds,
        &sim.farms,
        &sim.unit_guys,
        &sim.unit_type,
        &authority,
        actor_row,
    )
    .is_err());
    assert_eq!(save_load::save_sim(&sim).unwrap(), before);

    let (mut sim, actor_row, _, authority) = fixture();
    let prepared = prepare_gather_work_activation(
        &sim.world,
        &sim.builds,
        &sim.farms,
        &sim.unit_guys,
        &sim.unit_type,
        &authority,
        actor_row,
    )
    .unwrap();
    sim.farms.get_mut(FARM_INDEX as usize).unwrap().percent[CELL_XY] ^= 1;
    let before_commit = save_load::save_sim(&sim).unwrap();
    assert_eq!(
        commit_gather_work_activation(
            &mut sim.world,
            &mut sim.builds,
            &mut sim.farms,
            &mut sim.unit_guys,
            &sim.unit_type,
            &authority,
            prepared,
        ),
        Err(GatherWorkRuntimeError::StaleState)
    );
    assert_eq!(save_load::save_sim(&sim).unwrap(), before_commit);
}

#[test]
fn second_grow_witness_binds_literal_retail_images() {
    let literal = [
        0x00, 0xd4, 0x07, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x04, 0x00, 0xff, 0xff, 0xff, 0xff,
        0xff, 0xff, 0xff, 0xff, 0xa1, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00,
        0x01,
    ];
    let order = GatherWorkOrder::from_retail_payload(0, literal);
    assert_eq!(order.retail_payload_image(), literal);
    assert_eq!(
        (order.target_who, order.target_o, order.target_uid),
        (0, 2_004, 4)
    );
    let farm = farm_two().image();
    assert_eq!(farm.len(), 190);
    assert_eq!(farm[172 + CELL_XY], 1);
    assert_eq!(farm[172 + TRANSPOSED_YX], 2);
    assert_eq!(&farm[188..190], &[1, 0]);
}
