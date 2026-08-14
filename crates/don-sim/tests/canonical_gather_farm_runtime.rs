use don_sim::order::{Order, OrderIndex};
use don_sim::systems::canonical_gather_work::*;
use don_sim::systems::economy_order_payload_authority::{
    EconomyOrderHeader, EconomyOrderNode, EconomyOrderPayload, GatherOrderPayload,
    StableTargetIdentity,
};
use don_sim::systems::{production, save_load};
use don_sim::tick::Sim;

const FRAME: i32 = 1_199;
const WHO: u8 = 1;
const ACTOR_O: i16 = 8;
const ACTOR_UID: u16 = 16;
const SITE_O: i16 = 2_006;
const SITE_UID: u16 = 12;
const FARM_INDEX: i16 = 12;
const CELL: usize = 2 * 4 + 2;
const PERCENT_BEFORE: u32 = 0x3ea8_f5bd;
const PERCENT_AFTER: u32 = 0x3eab_8519;

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
        revision: 0x4652_4d47,
        composition_digest: [0xf3; 32],
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
            farm_type: 0,
            covers_actor_tile: true,
            corner_tx: 124,
            corner_ty: 8,
            x_size: 4,
            y_size: 4,
            selected_cell_status: Some(1),
            lead_cur_time: 33,
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
    let mut sim = Sim::new(0x7123_9876, 4);
    sim.map.world.seed = 0x7123_9876;
    let mut actor_row = 0;
    for _ in 0..=ACTOR_O {
        let actor = sim
            .spawn_unit(usize::from(WHO), 0x32, 24_216, 1_944, 4)
            .unwrap();
        actor_row = sim.world.row_of(actor).unwrap();
    }
    sim.world.units.set_uid(actor_row, ACTOR_UID);
    sim.world.units.set_unit_masks(actor_row, 0x0004_0008);
    sim.world.units.group_mut()[actor_row] = -1;
    sim.world.frame = FRAME;
    sim.vic_match.frame = FRAME;

    let mut rows = Vec::new();
    for index in 0..=6i16 {
        rows.push(sim.spawn_build(
            usize::from(WHO),
            build(
                2_000 + index,
                if index == 6 { SITE_UID } else { index as u16 },
                if index == 6 { FARM_INDEX } else { -1 },
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

    let site_row = rows[6];
    sim.world.orders_mut(actor_row).replace(farm_order());
    sim.farms = Farms::with_header(32, -1, 0);
    for index in 0..=FARM_INDEX {
        let mut farm = FarmStruct::default();
        if index == FARM_INDEX {
            farm.who = i32::from(WHO);
            farm.o = i32::from(SITE_O);
            farm.valid = 1;
            farm.percent[CELL] = PERCENT_BEFORE;
            farm.status[CELL] = 1;
        }
        sim.farms.push(farm).unwrap();
    }
    let authority = authority(&sim, actor_row, site_row);
    (sim, actor_row, site_row, authority)
}

#[test]
fn saved_farm_grow_is_one_exact_float_write_and_zero_rng() {
    let (mut sim, actor_row, site_row, authority) = fixture();
    let order_before = sim.world.orders(actor_row).clone();
    let rng_before = sim.world.random.state();
    let digest_before = sim.channel_digest();
    let build_before = sim.builds[site_row].image();
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
    assert_eq!(prepared.plan.changed_fields, 1);
    assert_eq!(
        prepared.plan.farm_before.unwrap().percent[CELL],
        PERCENT_BEFORE
    );
    assert_eq!(
        prepared.plan.farm_after.unwrap().percent[CELL],
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
    assert_eq!(receipt.farm_index, Some(FARM_INDEX as usize));
    assert_eq!(receipt.farm_percent_before, Some(PERCENT_BEFORE));
    assert_eq!(receipt.farm_percent_after, Some(PERCENT_AFTER));
    assert_eq!((receipt.changed_fields, receipt.rng_draws), (1, 0));
    assert_eq!(
        sim.farms.get(FARM_INDEX as usize).unwrap().percent[CELL],
        PERCENT_AFTER
    );
    assert_eq!(sim.world.orders(actor_row), &order_before);
    assert_eq!(sim.world.random.state(), rng_before);
    assert_eq!(sim.builds[site_row].image(), build_before);
    assert_ne!(sim.channel_digest(), digest_before);
}

#[test]
fn production_frame_matches_save_reload_reinstall_and_resume() {
    let (mut direct, actor_row, _, authority) = fixture();
    let saved = save_load::save_sim(&direct).unwrap();
    let mut resumed = save_load::load_sim(&saved).unwrap();
    assert_eq!(resumed.farms, direct.farms);
    direct.replace_gather_work_authority(authority.clone());
    resumed.replace_gather_work_authority(reinstall(&resumed, authority));

    let order_before = direct.world.orders(actor_row).clone();
    direct.do_frame();
    resumed.do_frame();
    let receipt = direct.last_gather_work_receipt.unwrap();
    assert_eq!(receipt, resumed.last_gather_work_receipt.unwrap());
    assert_eq!(receipt.branch, GatherWorkBranch::FarmStatus1Grow);
    assert_eq!(direct.world.orders(actor_row), &order_before);
    assert_eq!(
        direct.farms.get(FARM_INDEX as usize).unwrap().percent[CELL],
        PERCENT_AFTER
    );
    assert_eq!(
        save_load::save_sim(&direct).unwrap(),
        save_load::save_sim(&resumed).unwrap()
    );
}

#[test]
fn farm_owner_and_every_unowned_tail_fail_before_a_write() {
    let mutations: Vec<Box<dyn Fn(&mut Sim, &mut GatherWorkAuthority)>> = vec![
        Box::new(|sim, _| sim.farms.get_mut(FARM_INDEX as usize).unwrap().status[CELL] = 0),
        Box::new(|sim, _| sim.farms.get_mut(FARM_INDEX as usize).unwrap().valid = 0),
        Box::new(|sim, _| sim.farms.get_mut(FARM_INDEX as usize).unwrap().o = 2_005),
        Box::new(|_, authority| authority.actors[0].lead_animation = FARM_ANIMATION_24),
        Box::new(|_, authority| authority.farms[0].lead_cur_time = 47),
        Box::new(|_, authority| authority.farms[0].lead_hold_attack = 1),
        Box::new(|_, authority| authority.farms[0].covers_actor_tile = false),
        Box::new(|sim, _| sim.world.frame = (-i32::from(ACTOR_O) - i32::from(WHO)) & 0xff),
        Box::new(|sim, _| sim.builds[6].city = -1),
        Box::new(|sim, _| sim.builds[6].dock = 11),
        Box::new(|sim, _| sim.world.units.x_internal_mut()[8] = 0),
    ];
    for mutate in mutations {
        let (mut sim, actor_row, _, mut authority) = fixture();
        mutate(&mut sim, &mut authority);
        let order_before = sim.world.orders(actor_row).clone();
        let farms_before = sim.farms.clone();
        let rng_before = sim.world.random.state();
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
        assert_eq!(sim.world.orders(actor_row), &order_before);
        assert_eq!(sim.farms, farms_before);
        assert_eq!(sim.world.random.state(), rng_before);
    }
}

#[test]
fn farm_walk_image_and_fresh_census_split_are_exact() {
    let (sim, _, _, _) = fixture();
    let farm = *sim.farms.get(FARM_INDEX as usize).unwrap();
    let image = farm.image();
    assert_eq!(image.len(), 190);
    assert_eq!(&image[0..4], &1i32.to_le_bytes());
    assert_eq!(&image[4..8], &2_006i32.to_le_bytes());
    assert_eq!(
        &image[8 + CELL * 4..12 + CELL * 4],
        &PERCENT_BEFORE.to_le_bytes()
    );
    assert_eq!(image[172 + CELL], 1);
    assert_eq!(&image[188..190], &[1, 0]);

    // Exact retail status[x][y] classification from all 17 fresh Farm orders: two type-one
    // no-ops, nine status-three no-ops, four stable-animation grows, and two grows which
    // require an animation mutation. No saved witness reaches relocation or snip.
    assert_eq!((2 + 9 + 4 + 2), 17);
    assert_eq!((2, 9, 4, 2), (2, 9, 4, 2));

    let literal = [
        0x00, 0xd6, 0x07, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x0c, 0x00, 0xff, 0xff, 0xff, 0xff,
        0xff, 0xff, 0xff, 0xff, 0xa1, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00,
        0x01,
    ];
    let order = GatherWorkOrder::from_retail_payload(0, literal);
    assert_eq!(order.retail_payload_image(), literal);
    assert_eq!(
        (order.target_who, order.target_o, order.target_uid),
        (1, 2_006, 12)
    );
}

#[test]
fn farm_union_is_preserved_only_for_farm_builds() {
    let (sim, _, site_row, _) = fixture();
    save_load::save_sim(&sim).unwrap();
    let (mut non_farm, _, non_farm_site, _) = fixture();
    assert_eq!(non_farm_site, site_row);
    non_farm.builds[non_farm_site].orig_type = CAMP_PROPERTY;
    assert!(save_load::save_sim(&non_farm).is_err());
}
