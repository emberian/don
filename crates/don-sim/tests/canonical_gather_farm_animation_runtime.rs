use don_sim::order::{Order, OrderIndex};
use don_sim::systems::canonical_gather_work::*;
use don_sim::systems::economy_order_payload_authority::{
    EconomyOrderHeader, EconomyOrderNode, EconomyOrderPayload, GatherOrderPayload,
    StableTargetIdentity,
};
use don_sim::systems::groups_guys::{GuyData, UnitGuys, GUY_WALK_LEN};
use don_sim::systems::{production, save_load};
use don_sim::tick::{Sim, UnitGuysInstallError};

const FRAME: i32 = 1_199;
const WHO: u8 = 2;
const ACTOR_O: i16 = 5;
const ACTOR_UID: u16 = 11;
const SITE_O: i16 = 2_004;
const SITE_UID: u16 = 4;
const FARM_INDEX: i16 = 8;
const CELL: usize = 2 * 4 + 1;
const PERCENT_BEFORE: u32 = 0x3da3_d70a;
const PERCENT_AFTER: u32 = 0x3dae_147b;

fn hex<const N: usize>(text: &str) -> [u8; N] {
    assert_eq!(text.len(), N * 2);
    let mut out = [0; N];
    for (index, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).unwrap();
    }
    out
}

fn retail_guy_image() -> [u8; GUY_WALK_LEN] {
    hex(
        "32000000b805000038820000830200000000a41d00004d20000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000b8050000388200000000a41db80500003882000083020000010000000f00000000000000000000000f000000c01800000500ffff000000000000000000001000080000ff000200",
    )
}

fn retail_farm_image() -> [u8; FarmStruct::WALKED_BYTES] {
    hex(
        "02000000d4070000000000000000803f0000803f000000000000803f000000000000000000000000000000000ad7a33d0000803f0000803f0000803f0000803f0000803f0000803f00002444000025440080224400001b4400001a440080224400c025440000234400801c4400001c44000024440080234400c01f4400801a4400001d44004026440000234400c01e4400001b4400401f4400c02544008023440040204400801d4400001d44000202000200000000010202020202020104",
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

fn farm_order() -> Order {
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

fn authority(sim: &Sim, actor_row: usize, site_row: usize) -> GatherWorkAuthority {
    let actor = sim.world.handle_at_row(actor_row).unwrap();
    let site = &sim.builds[site_row];
    GatherWorkAuthority {
        revision: 0x4138_2335,
        composition_digest: [0xa8; 32],
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
            move_runtime: None,
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
            farm_type: 4,
            covers_actor_tile: true,
            corner_tx: 5,
            corner_ty: 172,
            x_size: 4,
            y_size: 4,
            selected_cell_status: Some(1),
            lead_cur_time: 1,
            lead_end_time: 15,
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
    let mut sim = Sim::new(0x0148_10ac, 256);
    sim.map.world.seed = 0x0148_10ac;
    let mut actor = None;
    for _ in 0..=ACTOR_O {
        actor = Some(sim.spawn_unit(WHO as usize, 50, 1_464, 33_336, 4).unwrap());
    }
    let actor = actor.unwrap();
    let actor_row = sim.world.row_of(actor).unwrap();
    sim.world.units.set_uid(actor_row, ACTOR_UID);
    sim.world.units.set_unit_masks(actor_row, 0x0004_0008);
    sim.world.units.group_mut()[actor_row] = -1;
    sim.world.frame = FRAME;
    sim.vic_match.frame = FRAME;

    let guy = GuyData::from_walk_bytes(retail_guy_image());
    assert_eq!(guy.walk_bytes(), retail_guy_image());
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

    let mut rows = Vec::new();
    for index in 0..=4i16 {
        rows.push(sim.spawn_build(
            WHO as usize,
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
    sim.world.orders_mut(actor_row).replace(farm_order());
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
    let authority = authority(&sim, actor_row, site_row);
    (sim, actor_row, site_row, authority)
}

#[test]
fn exact_retail_owner2_o5_anim8_to35_and_farm_grow_is_atomic() {
    let (mut sim, actor_row, site_row, authority) = fixture();
    let literal = [
        0x00, 0xd4, 0x07, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x04, 0x00, 0xff, 0xff, 0xff, 0xff,
        0xff, 0xff, 0xff, 0xff, 0xa1, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00,
        0x01,
    ];
    let retail_order = GatherWorkOrder::from_retail_payload(0, literal);
    assert_eq!(retail_order.retail_payload_image(), literal);
    assert_eq!(
        (
            retail_order.target_who,
            retail_order.target_o,
            retail_order.target_uid
        ),
        (2, 2_004, 4)
    );
    let before_guy = retail_guy_image();
    let before_farm = retail_farm_image();
    let before_order = sim.world.orders(actor_row).clone();
    let before_build = sim.builds[site_row].image();
    let before_rng = sim.world.random.state();
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
    assert_eq!(prepared.plan.farm_before.unwrap().image(), before_farm);

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
    assert_eq!((receipt.changed_fields, receipt.guy_changed_fields), (5, 4));
    assert_eq!(
        (receipt.guy_animation_before, receipt.guy_animation_after),
        (Some(8), Some(35))
    );
    assert_eq!(
        (receipt.guy_cur_time_before, receipt.guy_cur_time_after),
        (Some(1), Some(0))
    );
    assert_eq!(
        (receipt.guy_end_time_before, receipt.guy_end_time_after),
        (Some(15), Some(47))
    );
    assert_eq!(
        (receipt.guy_last_time_before, receipt.guy_last_time_after),
        (Some(0), Some(-1))
    );
    assert_eq!(
        (receipt.farm_percent_before, receipt.farm_percent_after),
        (Some(PERCENT_BEFORE), Some(PERCENT_AFTER))
    );
    assert_eq!(receipt.rng_draws, 0);

    let after_guy = sim.unit_guys[actor_row].as_ref().unwrap().guys[0]
        .as_ref()
        .unwrap()
        .walk_bytes();
    let changed: Vec<_> = before_guy
        .iter()
        .zip(after_guy.iter())
        .enumerate()
        .filter_map(|(index, (before, after))| (before != after).then_some(index))
        .collect();
    assert_eq!(changed, vec![108, 112, 116, 117, 118, 119, 148]);
    assert_eq!(
        sim.farms.get(FARM_INDEX as usize).unwrap().percent[CELL],
        PERCENT_AFTER
    );
    assert_eq!(sim.world.orders(actor_row), &before_order);
    assert_eq!(sim.builds[site_row].image(), before_build);
    assert_eq!(sim.world.random.state(), before_rng);
}

#[test]
fn unit_guys_v19_save_reload_resume_is_byte_identical() {
    let (mut direct, actor_row, _, authority) = fixture();
    let saved = save_load::save_sim(&direct).unwrap();
    let mut resumed = save_load::load_sim(&saved).unwrap();
    assert_eq!(save_load::save_sim(&resumed).unwrap(), saved);
    assert_eq!(resumed.unit_guys, direct.unit_guys);
    direct.replace_gather_work_authority(authority.clone());
    resumed.replace_gather_work_authority(reinstall(&resumed, authority));

    direct.do_frame();
    resumed.do_frame();
    let receipt = direct
        .last_gather_work_receipt
        .as_ref()
        .expect("production do_frame reached the exact Farm/Guy transaction");
    assert_eq!(receipt.branch, GatherWorkBranch::FarmStatus1Grow);
    assert_eq!((receipt.changed_fields, receipt.guy_changed_fields), (5, 4));
    assert_eq!(
        (receipt.guy_animation_before, receipt.guy_animation_after),
        (Some(8), Some(35))
    );
    assert_eq!(
        direct.last_gather_work_receipt,
        resumed.last_gather_work_receipt
    );
    assert_eq!(direct.unit_guys, resumed.unit_guys);
    assert_eq!(direct.farms, resumed.farms);
    assert_eq!(
        save_load::save_sim(&direct).unwrap(),
        save_load::save_sim(&resumed).unwrap()
    );
    assert_eq!(
        direct.world.orders(actor_row).order_type(),
        OrderIndex::Gather
    );
    assert_eq!(
        direct.unit_guys[actor_row].as_ref().unwrap().guys[0]
            .as_ref()
            .unwrap()
            .cur_anim,
        35
    );
    assert_eq!(
        direct.farms.get(FARM_INDEX as usize).unwrap().percent[CELL],
        PERCENT_AFTER
    );
}

#[test]
fn missing_or_stale_complete_guy_owner_cannot_publish_farm_or_guy() {
    let (mut sim, actor_row, _, authority) = fixture();
    sim.unit_guys[actor_row] = None;
    let farm_before = sim.farms.clone();
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
    assert_eq!(sim.farms, farm_before);

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
    sim.unit_guys[actor_row].as_mut().unwrap().guys[0]
        .as_mut()
        .unwrap()
        .avg_speed ^= 1;
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
fn recycled_handle_cannot_inherit_or_install_the_dead_units_guy_owner() {
    let mut sim = Sim::new(7, 8);
    let dead = sim.spawn_unit(WHO as usize, 50, 64, 64, 1).unwrap();
    let row = sim.world.row_of(dead).unwrap();
    let guy = GuyData {
        ty: 50,
        who: WHO as i8,
        o: sim.world.units.o()[row],
        guy_num: 0,
        ..Default::default()
    };
    let owner = UnitGuys {
        guys: vec![Some(guy)],
        size: 1,
        increment: 1,
        flags: 0,
        guy_mark: 1,
    };
    sim.install_unit_guys(dead, owner.clone()).unwrap();

    assert!(sim.world.despawn(dead));
    let replacement = sim.spawn_unit(WHO as usize, 50, 64, 64, 1).unwrap();
    assert_eq!(sim.world.row_of(replacement), Some(row));
    assert_ne!(replacement.generation, dead.generation);
    assert!(sim.unit_guys[row].is_none());
    assert_eq!(
        sim.install_unit_guys(dead, owner),
        Err(UnitGuysInstallError::StaleHandle)
    );
}
