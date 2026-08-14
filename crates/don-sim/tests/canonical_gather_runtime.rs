use don_sim::order::{Order, OrderIndex};
use don_sim::rng::Random;
use don_sim::systems::canonical_gather_work::*;
use don_sim::systems::economy_order_payload_authority::{
    EconomyOrderHeader, EconomyOrderNode, EconomyOrderPayload, GatherOrderPayload,
    StableTargetIdentity,
};
use don_sim::systems::{production, save_load};
use don_sim::tick::Sim;

fn build(uid: u16, o: i16) -> production::BuildData {
    let mut build = production::BuildData {
        flags: production::flag::VALID | production::flag::STARTED | production::flag::ACTIVE,
        who: 0,
        uid,
        orig_type: CAMP_PROPERTY,
        gather_down: -1,
        city: -1,
        city_down: -1,
        wonder: -1,
        dock: -1,
        attack_ox: -1,
        attack_whom: -1,
        ..Default::default()
    };
    build.other[0x0a..0x0c].copy_from_slice(&o.to_le_bytes());
    build.other[0x28..0x2a].copy_from_slice(&(-1i16).to_le_bytes());
    build
}

fn gather_order(target_o: i32, target_uid: u16, wait: i32, goto_build: u8) -> Order {
    Order::economy(EconomyOrderNode {
        metric: 0,
        header: EconomyOrderHeader {
            kind: OrderIndex::Gather,
            flags: 0,
            x: 0,
            y: 0,
            primary: StableTargetIdentity::banded(target_o, 0, target_uid),
        },
        payload: EconomyOrderPayload::Gather(GatherOrderPayload {
            tx: 61,
            ty: 72,
            build_type: CAMP_PROPERTY,
            wait,
            goto_build,
            non_flat_gather: 1,
            dist_mod: 4,
            been_there: 1,
        }),
    })
    .unwrap()
}

fn wait(order: &Order) -> i32 {
    let EconomyOrderPayload::Gather(payload) = order.economy.unwrap() else {
        panic!("expected Gather payload")
    };
    payload.wait
}

fn authority(sim: &Sim, actor_row: usize, build_row: usize) -> GatherWorkAuthority {
    let build = &sim.builds[build_row];
    GatherWorkAuthority {
        revision: 41,
        composition_digest: [0xa7; 32],
        actors: vec![GatherActorRuntimeFacts {
            actor: sim.world.handle_at_row(actor_row).unwrap(),
            who: sim.world.units.get_who(actor_row),
            o: sim.world.units.o()[actor_row],
            uid: sim.world.units.get_uid(actor_row),
            type_index: sim.unit_type[actor_row],
            guys_length: 1,
            guys_capacity: 1,
            guys_increment: 1,
            guys_flags: 0,
            slot_zero_present: true,
            lead_animation: GATHER_ANIMATION_19,
            move_runtime: None,
        }],
        sites: vec![GatherSiteRuntimeFacts {
            who: build.who,
            o: build.object_id(),
            uid: build.uid,
            property: build.orig_type,
            resolves_build: true,
            valid_wall_projection: true,
        }],
        farms: vec![],
    }
}

fn reinstall_authority(sim: &Sim, authority: &GatherWorkAuthority) -> GatherWorkAuthority {
    let mut authority = authority.clone();
    for facts in &mut authority.actors {
        let row = (0..sim.world.live_count() as usize)
            .find(|&row| {
                sim.world.units.get_who(row) == facts.who
                    && sim.world.units.o()[row] == facts.o
                    && sim.world.units.get_uid(row) == facts.uid
            })
            .unwrap();
        facts.actor = sim.world.handle_at_row(row).unwrap();
    }
    authority
}

fn fixture(order_wait: i32) -> (Sim, usize, usize, GatherWorkAuthority) {
    let mut sim = Sim::new(0x1234_5678, 4);
    sim.map.world.seed = 0x1234_5678;
    let mut actor_row = 0;
    for x in 0..=3 {
        let actor = sim.spawn_unit(0, 0x32, 1000 + x * 10, 1000, 4).unwrap();
        actor_row = sim.world.row_of(actor).unwrap();
    }
    assert_eq!(sim.world.units.o()[actor_row], 3);
    sim.world.units.group_mut()[actor_row] = 9;
    sim.world.units.set_unit_masks(actor_row, 0x7f00_0042);
    sim.world.frame = 1;
    sim.vic_match.frame = 1;
    sim.spawn_build(0, build(98, 2000));
    let site_row = sim.spawn_build(0, build(17, 2001));
    sim.world
        .orders_mut(actor_row)
        .replace(gather_order(2001, 17, order_wait, 0));
    let authority = authority(&sim, actor_row, site_row);
    (sim, actor_row, site_row, authority)
}

#[test]
fn production_do_frame_matches_direct_save_reload_and_resume() {
    let (mut direct, actor_row, site_row, authority) = fixture(84);
    let bytes = save_load::save_sim(&direct).unwrap();
    let mut resumed = save_load::load_sim(&bytes).unwrap();
    assert_eq!(
        resumed.gather_work_authority,
        GatherWorkAuthority::default()
    );
    direct.replace_gather_work_authority(authority.clone());
    resumed.replace_gather_work_authority(reinstall_authority(&resumed, &authority));

    direct.do_frame();
    resumed.do_frame();

    let direct_receipt = direct.last_gather_work_receipt.unwrap();
    assert_eq!(direct_receipt, resumed.last_gather_work_receipt.unwrap());
    assert_eq!(
        direct_receipt.branch,
        GatherWorkBranch::CampAnimation19Waiting
    );
    assert_eq!(
        (direct_receipt.wait_before, direct_receipt.wait_after),
        (84, 83)
    );
    assert_eq!(direct.world.units.group()[actor_row], -1);
    assert_eq!(direct.world.units.get_unit_masks(actor_row), 0x0700_0042);
    // The Unit pass sets the retail 0x800 latch; the later Build pass in the same object
    // traversal consumes/clears it exactly as `Wall::process` does.
    assert_eq!(direct.builds[site_row].build_masks, 0);
    assert_eq!(direct.builds[site_row].recharging, 1);
    assert_eq!(wait(direct.world.orders(actor_row).current().unwrap()), 83);
    assert_eq!(
        save_load::save_sim(&direct).unwrap(),
        save_load::save_sim(&resumed).unwrap()
    );
}

#[test]
fn wait_zero_empty_chain_is_all_gathering_and_save_resume_exact() {
    let (mut direct, actor_row, _site_row, authority) = fixture(1);
    let saved = save_load::save_sim(&direct).unwrap();
    let mut resumed = save_load::load_sim(&saved).unwrap();
    direct.replace_gather_work_authority(authority.clone());
    resumed.replace_gather_work_authority(reinstall_authority(&resumed, &authority));
    direct.do_frame();
    resumed.do_frame();

    let receipt = direct.last_gather_work_receipt.unwrap();
    assert_eq!(receipt, resumed.last_gather_work_receipt.unwrap());
    assert_eq!(
        receipt.branch,
        GatherWorkBranch::CampAnimation19AllGathering
    );
    assert_eq!(receipt.wait_after, -1);
    assert_eq!(receipt.rng_draws, 0);
    assert_eq!(receipt.random_state_before, receipt.random_state_after);
    assert_eq!(wait(direct.world.orders(actor_row).current().unwrap()), -1);
    assert_eq!(
        save_load::save_sim(&direct).unwrap(),
        save_load::save_sim(&resumed).unwrap()
    );
}

#[test]
fn wait_zero_false_chain_consumes_one_exact_rng_draw() {
    let (mut sim, actor_row, site_row, authority) = fixture(1);
    let blocker = sim.spawn_unit(0, 0x32, 1100, 1000, 4).unwrap();
    let blocker_row = sim.world.row_of(blocker).unwrap();
    let blocker_o = sim.world.units.o()[blocker_row];
    sim.world.units.gather_down_mut()[blocker_row] = -1;
    sim.world
        .orders_mut(blocker_row)
        .replace(gather_order(2001, 17, 9, 1));
    sim.builds[site_row].gather_down = blocker_o;
    let before_rng = sim.world.random.state();
    let mut expected_rng = Random::new(before_rng);
    let draw = expected_rng.get(0, 0xffff);

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
    assert_eq!(prepared.chain.as_ref().unwrap().all_gathering, false);
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

    assert_eq!(receipt.branch, GatherWorkBranch::CampAnimation19Rescheduled);
    assert_eq!(receipt.rng_draws, 1);
    assert_eq!(receipt.wait_after, draw % 100 + 300);
    assert_eq!(sim.world.random.state(), expected_rng.state());
    assert_eq!(
        wait(sim.world.orders(actor_row).current().unwrap()),
        draw % 100 + 300
    );
}

#[test]
fn pure_wait_zero_planner_matches_random_and_mutations_kill_branches() {
    let (sim, actor_row, site_row, _authority) = fixture(1);
    let build = &sim.builds[site_row];
    let current = sim.world.orders(actor_row).current().unwrap();
    let EconomyOrderPayload::Gather(payload) = current.economy.unwrap() else {
        unreachable!()
    };
    let snapshot = GatherWorkSnapshot {
        revision: 7,
        frame: sim.world.frame,
        rng_state: sim.world.random.state(),
        actor: GatherActorImage {
            who: sim.world.units.get_who(actor_row),
            o: sim.world.units.o()[actor_row],
            uid: sim.world.units.get_uid(actor_row),
            group: sim.world.units.group()[actor_row],
            unit_masks: sim.world.units.get_unit_masks(actor_row),
            lead_animation: Some(GATHER_ANIMATION_19),
        },
        order: GatherWorkOrder {
            node_metric: current.node_metric,
            flags: current.flags,
            target_o: i32::from(current.target_o),
            target_who: i32::from(current.target_who),
            target_uid: current.target_uid,
            tx: payload.tx,
            ty: payload.ty,
            build_type: payload.build_type,
            wait: payload.wait,
            goto_build: payload.goto_build,
            non_flat_gather: payload.non_flat_gather,
            dist_mod: payload.dist_mod,
            been_there: payload.been_there,
        },
        site: GatherSiteImage {
            who: i32::from(build.who),
            o: i32::from(build.object_id()),
            uid: build.uid,
            resolved_build: true,
            valid_wall: true,
            active: true,
            property: build.orig_type,
            build_masks: build.build_masks,
            recharging: build.recharging,
        },
    };
    let all = plan_fresh_gather_tick_at_wait_zero(snapshot, true).unwrap();
    assert_eq!(all.branch, GatherWorkBranch::CampAnimation19AllGathering);
    assert_eq!((all.after.order.wait, all.rng_draws), (-1, 0));
    let rescheduled = plan_fresh_gather_tick_at_wait_zero(snapshot, false).unwrap();
    let mut expected = Random::new(snapshot.rng_state);
    let draw = expected.get(0, 0xffff);
    assert_eq!(
        rescheduled.branch,
        GatherWorkBranch::CampAnimation19Rescheduled
    );
    assert_eq!(rescheduled.after.order.wait, draw % 100 + 300);
    assert_eq!(rescheduled.after.rng_state, expected.state());

    let mutations = [
        GatherWorkSnapshot {
            actor: GatherActorImage {
                lead_animation: Some(0x1d),
                ..snapshot.actor
            },
            ..snapshot
        },
        GatherWorkSnapshot {
            order: GatherWorkOrder {
                wait: 0,
                ..snapshot.order
            },
            ..snapshot
        },
        GatherWorkSnapshot {
            frame: (-i32::from(snapshot.actor.o) * 4) & CAPACITY_PHASE_MASK,
            ..snapshot
        },
    ];
    assert!(matches!(
        plan_fresh_gather_tick_at_wait_zero(mutations[0], false),
        Err(GatherWorkPlanError::Unowned(
            UnownedGatherArm::LeadAnimation(0x1d)
        ))
    ));
    assert!(matches!(
        plan_fresh_gather_tick_at_wait_zero(mutations[1], false),
        Err(GatherWorkPlanError::Binding(
            FreshGatherBindingError::CampStateMismatch
        ))
    ));
    assert!(matches!(
        plan_fresh_gather_tick_at_wait_zero(mutations[2], false),
        Err(GatherWorkPlanError::Unowned(
            UnownedGatherArm::CapacityAndGathererCount
        ))
    ));
}

#[test]
fn check_gatherers_unlink_and_stale_compare_are_atomic() {
    let (mut sim, actor_row, site_row, authority) = fixture(1);
    let stale = sim.spawn_unit(0, 0x32, 1100, 1000, 4).unwrap();
    let stale_row = sim.world.row_of(stale).unwrap();
    let stale_o = sim.world.units.o()[stale_row];
    sim.world.units.gather_down_mut()[stale_row] = -1;
    sim.builds[site_row].gather_down = stale_o;

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
    let chain = prepared.chain.as_ref().unwrap();
    assert_eq!(
        (chain.removed, chain.site_head_before, chain.site_head_after),
        (1, stale_o, -1)
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
    assert_eq!(receipt.chain_unlinks, 1);
    assert_eq!(sim.builds[site_row].gather_down, -1);
    assert_eq!(sim.world.units.gather_down()[stale_row], -1);

    // A newly prepared wait tick cannot overwrite a later Build latch image.
    let (mut stale_sim, actor_row, site_row, authority) = fixture(84);
    let prepared = prepare_gather_work_activation(
        &stale_sim.world,
        &stale_sim.builds,
        &stale_sim.farms,
        &stale_sim.unit_guys,
        &stale_sim.unit_type,
        &authority,
        actor_row,
    )
    .unwrap();
    stale_sim.builds[site_row].build_masks = 0x20;
    let order_before = stale_sim.world.orders(actor_row).clone();
    let group_before = stale_sim.world.units.group()[actor_row];
    assert_eq!(
        commit_gather_work_activation(
            &mut stale_sim.world,
            &mut stale_sim.builds,
            &mut stale_sim.farms,
            &mut stale_sim.unit_guys,
            &stale_sim.unit_type,
            &authority,
            prepared,
        ),
        Err(GatherWorkRuntimeError::StaleState)
    );
    assert_eq!(stale_sim.builds[site_row].build_masks, 0x20);
    assert_eq!(stale_sim.world.units.group()[actor_row], group_before);
    assert_eq!(stale_sim.world.orders(actor_row), &order_before);
}

#[test]
fn missing_or_normalized_guy_authority_is_zero_write() {
    let (sim, actor_row, _site_row, mut authority) = fixture(84);
    let missing = GatherWorkAuthority::default();
    assert_eq!(
        prepare_gather_work_activation(
            &sim.world,
            &sim.builds,
            &sim.farms,
            &sim.unit_guys,
            &sim.unit_type,
            &missing,
            actor_row,
        ),
        Err(GatherWorkRuntimeError::MissingCompositionDigest)
    );
    authority.actors[0].guys_capacity = 2;
    assert_eq!(
        prepare_gather_work_activation(
            &sim.world,
            &sim.builds,
            &sim.farms,
            &sim.unit_guys,
            &sim.unit_type,
            &authority,
            actor_row,
        ),
        Err(GatherWorkRuntimeError::InvalidGuyArrayFacts)
    );
    assert_eq!(sim.world.units.group()[actor_row], 9);
    assert_eq!(wait(sim.world.orders(actor_row).current().unwrap()), 84);
}
