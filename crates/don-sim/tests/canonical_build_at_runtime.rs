use don_sim::order::{Order, OrderIndex, ORDER_GROUP};
use don_sim::systems::canonical_build_at_work::*;
use don_sim::systems::{production, save_load};
use don_sim::tick::Sim;

fn put_position(build: &mut production::BuildData, x: i32, y: i32) {
    build.other[0x10..0x14].copy_from_slice(&(x ^ 0x63637).to_le_bytes());
    build.other[0x14..0x18].copy_from_slice(&(y ^ 0x63637).to_le_bytes());
}

fn build(o: i16, uid: u16, x: i32, y: i32, witness: bool) -> production::BuildData {
    let mut build = production::BuildData {
        flags: if witness {
            FRESH_BUILD_FLAGS
        } else {
            production::flag::VALID | production::flag::STARTED | production::flag::ACTIVE
        },
        uid,
        orig_type: if witness {
            FRESH_BUILD_AT_TARGET_TYPE
        } else {
            400
        },
        build_masks: if witness { FRESH_BUILD_MASKS } else { 0 },
        recharging: if witness { FRESH_BUILD_RECHARGING } else { 0 },
        helpers: FRESH_BUILD_HELPERS,
        job_counter: if witness { FRESH_BUILD_JOB_COUNTER } else { 0 },
        job_counter_2: if witness { FRESH_BUILD_JOB_COUNTER } else { 0 },
        constr_time: FRESH_BUILD_CONSTRUCT_TIME,
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
    put_position(&mut build, x, y);
    build
}

fn build_at_order() -> Order {
    Order {
        node_metric: 0,
        kind: OrderIndex::BuildAt,
        flags: ORDER_GROUP,
        target_who: FRESH_BUILD_AT_TARGET_WHO as i8,
        target_o: FRESH_BUILD_AT_TARGET_O,
        target_uid: FRESH_BUILD_AT_TARGET_UID,
        ..Order::default()
    }
}

fn authority(sim: &Sim, actor_row: usize, build_row: usize) -> BuildAtWorkAuthority {
    let build = &sim.builds[build_row];
    BuildAtWorkAuthority {
        revision: 61,
        composition_digest: [0xb6; 32],
        actors: vec![BuildAtActorRuntimeFacts {
            actor: sim.world.handle_at_row(actor_row).unwrap(),
            who: sim.world.units.get_who(actor_row),
            o: sim.world.units.o()[actor_row],
            uid: sim.world.units.get_uid(actor_row),
            type_index: sim.unit_type[actor_row],
            branch: BuildAtBranchFacts::FRESH,
        }],
        sites: vec![BuildAtSiteRuntimeFacts {
            who: build.who,
            o: build.object_id(),
            uid: build.uid,
            type_index: build.orig_type,
        }],
    }
}

fn reinstall_authority(sim: &mut Sim, old: &BuildAtWorkAuthority) -> BuildAtWorkAuthority {
    let mut authority = old.clone();
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
    for site in &authority.sites {
        let row = sim
            .builds
            .iter()
            .position(|build| {
                (build.who, build.object_id(), build.uid) == (site.who, site.o, site.uid)
            })
            .unwrap();
        sim.production_runtime.register_build(row, site.type_index);
    }
    authority
}

fn fixture() -> (Sim, usize, usize, BuildAtWorkAuthority) {
    let mut sim = Sim::new(0x1234_5678, 64);
    // Frame 1 is a save-supported scheduler phase; the executable witness's saved frame
    // (1199) is frozen independently because do_build does not read it on this branch.
    sim.world.frame = 1;
    sim.vic_match.frame = 1;
    let actor_x = FRESH_BUILD_AT_ACTOR_X;
    let actor_y = FRESH_BUILD_AT_ACTOR_Y;
    let site_x = FRESH_BUILD_AT_TARGET_X;
    let site_y = FRESH_BUILD_AT_TARGET_Y;
    let mut actor_row = 0;
    for o in 0..=FRESH_BUILD_AT_ACTOR_O {
        let actor = sim
            .spawn_unit(
                FRESH_BUILD_AT_ACTOR_WHO as usize,
                if o == FRESH_BUILD_AT_ACTOR_O {
                    FRESH_BUILD_AT_ACTOR_TYPE
                } else {
                    1
                },
                actor_x - i32::from(FRESH_BUILD_AT_ACTOR_O - o) * 96,
                actor_y,
                1,
            )
            .unwrap();
        actor_row = sim.world.row_of(actor).unwrap();
    }
    assert_eq!(sim.world.units.o()[actor_row], FRESH_BUILD_AT_ACTOR_O);
    sim.world.units.set_uid(actor_row, FRESH_BUILD_AT_ACTOR_UID);
    sim.world
        .units
        .set_unit_masks(actor_row, FRESH_BUILD_AT_ACTOR_MASKS);

    let mut site_row = 0;
    for offset in 0..=(FRESH_BUILD_AT_TARGET_O - production::BUILD_BAND_BASE as i16) {
        let o = production::BUILD_BAND_BASE as i16 + offset;
        let witness = o == FRESH_BUILD_AT_TARGET_O;
        site_row = sim.spawn_build(
            FRESH_BUILD_AT_TARGET_WHO as usize,
            build(
                o,
                if witness {
                    FRESH_BUILD_AT_TARGET_UID
                } else {
                    offset as u16 + 1
                },
                if witness {
                    site_x
                } else {
                    site_x + (i32::from(offset) + 1) * 192
                },
                site_y,
                witness,
            ),
        );
        sim.production_runtime.register_build(
            site_row,
            if witness {
                FRESH_BUILD_AT_TARGET_TYPE
            } else {
                400
            },
        );
    }
    assert_eq!(sim.builds[site_row].object_id(), FRESH_BUILD_AT_TARGET_O);
    sim.world.units.angle_mut()[actor_row] =
        don_sim::trig::find_angle(site_x - actor_x, site_y - actor_y);
    sim.world.orders_mut(actor_row).replace(build_at_order());
    let authority = authority(&sim, actor_row, site_row);
    (sim, actor_row, site_row, authority)
}

fn pure_snapshot() -> BuildAtWorkSnapshot {
    BuildAtWorkSnapshot {
        authority_revision: 61,
        actor: BuildAtActorImage {
            who: FRESH_BUILD_AT_ACTOR_WHO,
            o: FRESH_BUILD_AT_ACTOR_O,
            uid: FRESH_BUILD_AT_ACTOR_UID,
            type_index: FRESH_BUILD_AT_ACTOR_TYPE,
            active: true,
            x: FRESH_BUILD_AT_ACTOR_X,
            y: FRESH_BUILD_AT_ACTOR_Y,
            angle: FRESH_BUILD_AT_ACTOR_ANGLE,
            unit_masks: FRESH_BUILD_AT_ACTOR_MASKS,
        },
        order: BuildAtWorkOrder::FRESH,
        site: BuildAtSiteImage {
            who: FRESH_BUILD_AT_TARGET_WHO,
            o: FRESH_BUILD_AT_TARGET_O,
            uid: FRESH_BUILD_AT_TARGET_UID,
            type_index: FRESH_BUILD_AT_TARGET_TYPE,
            flags: FRESH_BUILD_FLAGS,
            x: FRESH_BUILD_AT_TARGET_X,
            y: FRESH_BUILD_AT_TARGET_Y,
            build_masks: FRESH_BUILD_MASKS,
            recharging: FRESH_BUILD_RECHARGING,
            helpers: FRESH_BUILD_HELPERS,
            job_counter: FRESH_BUILD_JOB_COUNTER,
            job_counter_2: FRESH_BUILD_JOB_COUNTER,
            construct_time: FRESH_BUILD_CONSTRUCT_TIME,
        },
        branch_facts: BuildAtBranchFacts::FRESH,
    }
}

#[test]
fn fresh_payload_and_binary_evidence_are_frozen() {
    assert_eq!(UNIT_DO_BUILD_VA, 0x005e_ebf0);
    assert_eq!(UNIT_DO_BUILD_BYTES, 1_711);
    assert_eq!(
        UNIT_DO_BUILD_SHA256,
        "0ddd82c78a65cd13956f0d33b2ebdfaaa5942000339102b40f33b9a6d102587e"
    );
    assert_eq!(WALL_DO_CONSTRUCT_VA, 0x0064_34d0);
    assert_eq!(WALL_DO_CONSTRUCT_BYTES, 1_245);
    assert_eq!(
        WALL_DO_CONSTRUCT_SHA256,
        "b92b6180e910b076a4de2458f94ed2ccb530e6d0db292ae70eb201561fd59589"
    );
    assert_eq!(FRESH_BUILD_AT_PAYLOAD_SHA256.len(), 64);
    assert_eq!(FRESH_BUILD_AT_NODE_SHA256.len(), 64);
    assert_eq!(FRESH_BUILD_AT_UNIT_SHA256.len(), 64);
    assert_eq!(FRESH_BUILD_AT_GUY_SHA256.len(), 64);
    assert_eq!(FRESH_BUILD_AT_BUILD_SHA256.len(), 64);
    assert_eq!(
        don_sim::trig::find_angle(
            FRESH_BUILD_AT_TARGET_X - FRESH_BUILD_AT_ACTOR_X,
            FRESH_BUILD_AT_TARGET_Y - FRESH_BUILD_AT_ACTOR_Y,
        ),
        FRESH_BUILD_AT_ACTOR_ANGLE
    );
    assert_eq!(
        BuildAtWorkOrder::FRESH.retail_payload_image(),
        [0x04, 0xd7, 0x07, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x10, 0x00]
    );
}

#[test]
fn pure_plan_is_the_exact_five_field_noncompletion_write() {
    let before = pure_snapshot();
    let plan = plan_fresh_build_at_tick(before).unwrap();
    assert_eq!(plan.branch, BuildAtWorkBranch::PeasantVillageNonCompleting);
    assert_eq!(plan.after.site.recharging, 252);
    assert_eq!(plan.after.site.build_masks, 0x0c00);
    assert_eq!(plan.after.site.helpers, 1);
    assert_eq!(plan.after.site.job_counter, 25_200);
    assert_eq!(plan.after.site.job_counter_2, 25_200);
    assert_eq!(plan.after.actor, before.actor);
    assert_eq!(plan.after.order, before.order);
    assert_eq!(plan.after.branch_facts, before.branch_facts);
}

#[test]
fn every_adjacent_unowned_cone_fails_closed() {
    let base = pure_snapshot();
    let mutations = [
        BuildAtWorkSnapshot {
            order: BuildAtWorkOrder {
                flags: 0,
                ..base.order
            },
            ..base
        },
        BuildAtWorkSnapshot {
            actor: BuildAtActorImage {
                who: 1,
                ..base.actor
            },
            ..base
        },
        BuildAtWorkSnapshot {
            actor: BuildAtActorImage {
                type_index: 51,
                ..base.actor
            },
            ..base
        },
        BuildAtWorkSnapshot {
            actor: BuildAtActorImage {
                active: false,
                ..base.actor
            },
            ..base
        },
        BuildAtWorkSnapshot {
            actor: BuildAtActorImage {
                angle: base.actor.angle.wrapping_add(1),
                ..base.actor
            },
            ..base
        },
        BuildAtWorkSnapshot {
            actor: BuildAtActorImage {
                unit_masks: base.actor.unit_masks | 1,
                ..base.actor
            },
            ..base
        },
        BuildAtWorkSnapshot {
            site: BuildAtSiteImage {
                uid: 17,
                ..base.site
            },
            ..base
        },
        BuildAtWorkSnapshot {
            site: BuildAtSiteImage {
                type_index: 415,
                ..base.site
            },
            ..base
        },
        BuildAtWorkSnapshot {
            site: BuildAtSiteImage {
                flags: production::flag::VALID,
                ..base.site
            },
            ..base
        },
        BuildAtWorkSnapshot {
            site: BuildAtSiteImage {
                build_masks: FRESH_BUILD_MASKS | production::mask::UNDER_ATTACK,
                ..base.site
            },
            ..base
        },
        BuildAtWorkSnapshot {
            site: BuildAtSiteImage {
                helpers: 1,
                ..base.site
            },
            ..base
        },
        BuildAtWorkSnapshot {
            site: BuildAtSiteImage {
                job_counter: FRESH_BUILD_JOB_COUNTER + 1,
                ..base.site
            },
            ..base
        },
        BuildAtWorkSnapshot {
            branch_facts: BuildAtBranchFacts {
                target_is_valid_wall: false,
                ..base.branch_facts
            },
            ..base
        },
        BuildAtWorkSnapshot {
            branch_facts: BuildAtBranchFacts {
                adjacent: false,
                ..base.branch_facts
            },
            ..base
        },
        BuildAtWorkSnapshot {
            branch_facts: BuildAtBranchFacts {
                builder_tile_is_covered: true,
                ..base.branch_facts
            },
            ..base
        },
        BuildAtWorkSnapshot {
            branch_facts: BuildAtBranchFacts {
                target_is_farm: true,
                ..base.branch_facts
            },
            ..base
        },
        BuildAtWorkSnapshot {
            branch_facts: BuildAtBranchFacts {
                korean_bonus: true,
                ..base.branch_facts
            },
            ..base
        },
        BuildAtWorkSnapshot {
            branch_facts: BuildAtBranchFacts {
                ai_speed: 2,
                ..base.branch_facts
            },
            ..base
        },
        BuildAtWorkSnapshot {
            branch_facts: BuildAtBranchFacts {
                lead_animation_time: 14,
                ..base.branch_facts
            },
            ..base
        },
        BuildAtWorkSnapshot {
            branch_facts: BuildAtBranchFacts {
                lead_hold_attack: 1,
                ..base.branch_facts
            },
            ..base
        },
    ];
    for mutation in mutations {
        assert!(plan_fresh_build_at_tick(mutation).is_err());
    }
}

#[test]
fn commit_revalidates_the_authority_before_any_build_write() {
    let (mut sim, actor_row, site_row, authority) = fixture();
    let prepared = prepare_build_at_work_activation(
        &sim.world,
        &sim.builds,
        &sim.unit_type,
        &sim.production_runtime.build_types,
        sim.step8_env.leaders[0].payout.ai_speed,
        false,
        &sim.prod_rules,
        &authority,
        actor_row,
    )
    .unwrap();
    let before = sim.builds[site_row].image();
    let mut changed_authority = authority.clone();
    changed_authority.composition_digest[0] ^= 1;
    let error = commit_build_at_work_activation(
        &sim.world,
        &mut sim.builds,
        &sim.unit_type,
        &sim.production_runtime.build_types,
        sim.step8_env.leaders[0].payout.ai_speed,
        false,
        &sim.prod_rules,
        &changed_authority,
        prepared,
    )
    .unwrap_err();
    assert_eq!(error, BuildAtWorkRuntimeError::StaleState);
    assert_eq!(sim.builds[site_row].image(), before);
}

#[test]
fn stale_build_order_type_rules_and_globals_are_all_nonmutating() {
    let (mut sim, actor_row, site_row, authority) = fixture();
    let prepared = prepare_build_at_work_activation(
        &sim.world,
        &sim.builds,
        &sim.unit_type,
        &sim.production_runtime.build_types,
        sim.step8_env.leaders[0].payout.ai_speed,
        false,
        &sim.prod_rules,
        &authority,
        actor_row,
    )
    .unwrap();
    sim.builds[site_row].job_counter += 1;
    let before = sim.builds[site_row].image();
    assert!(commit_build_at_work_activation(
        &sim.world,
        &mut sim.builds,
        &sim.unit_type,
        &sim.production_runtime.build_types,
        sim.step8_env.leaders[0].payout.ai_speed,
        false,
        &sim.prod_rules,
        &authority,
        prepared,
    )
    .is_err());
    assert_eq!(sim.builds[site_row].image(), before);

    let (mut sim, actor_row, site_row, authority) = fixture();
    let prepared = prepare_build_at_work_activation(
        &sim.world,
        &sim.builds,
        &sim.unit_type,
        &sim.production_runtime.build_types,
        sim.step8_env.leaders[0].payout.ai_speed,
        false,
        &sim.prod_rules,
        &authority,
        actor_row,
    )
    .unwrap();
    sim.world
        .orders_mut(actor_row)
        .current_mut()
        .unwrap()
        .target_uid += 1;
    let before = sim.builds[site_row].image();
    assert!(commit_build_at_work_activation(
        &sim.world,
        &mut sim.builds,
        &sim.unit_type,
        &sim.production_runtime.build_types,
        sim.step8_env.leaders[0].payout.ai_speed,
        false,
        &sim.prod_rules,
        &authority,
        prepared,
    )
    .is_err());
    assert_eq!(sim.builds[site_row].image(), before);

    let (mut sim, actor_row, site_row, authority) = fixture();
    let prepared = prepare_build_at_work_activation(
        &sim.world,
        &sim.builds,
        &sim.unit_type,
        &sim.production_runtime.build_types,
        sim.step8_env.leaders[0].payout.ai_speed,
        false,
        &sim.prod_rules,
        &authority,
        actor_row,
    )
    .unwrap();
    sim.production_runtime.build_types[site_row] = Some(FRESH_BUILD_AT_TARGET_TYPE + 1);
    let before = sim.builds[site_row].image();
    assert!(commit_build_at_work_activation(
        &sim.world,
        &mut sim.builds,
        &sim.unit_type,
        &sim.production_runtime.build_types,
        sim.step8_env.leaders[0].payout.ai_speed,
        false,
        &sim.prod_rules,
        &authority,
        prepared,
    )
    .is_err());
    assert_eq!(sim.builds[site_row].image(), before);

    let (mut sim, actor_row, site_row, authority) = fixture();
    let prepared = prepare_build_at_work_activation(
        &sim.world,
        &sim.builds,
        &sim.unit_type,
        &sim.production_runtime.build_types,
        sim.step8_env.leaders[0].payout.ai_speed,
        false,
        &sim.prod_rules,
        &authority,
        actor_row,
    )
    .unwrap();
    sim.prod_rules
        .set(production::rule::ACCEL_CONSTRUCT, FRESH_BUILD_RATE + 1);
    let before = sim.builds[site_row].image();
    assert!(commit_build_at_work_activation(
        &sim.world,
        &mut sim.builds,
        &sim.unit_type,
        &sim.production_runtime.build_types,
        sim.step8_env.leaders[0].payout.ai_speed,
        false,
        &sim.prod_rules,
        &authority,
        prepared,
    )
    .is_err());
    assert_eq!(sim.builds[site_row].image(), before);

    let (mut sim, actor_row, site_row, authority) = fixture();
    let prepared = prepare_build_at_work_activation(
        &sim.world,
        &sim.builds,
        &sim.unit_type,
        &sim.production_runtime.build_types,
        sim.step8_env.leaders[0].payout.ai_speed,
        false,
        &sim.prod_rules,
        &authority,
        actor_row,
    )
    .unwrap();
    let before = sim.builds[site_row].image();
    assert!(commit_build_at_work_activation(
        &sim.world,
        &mut sim.builds,
        &sim.unit_type,
        &sim.production_runtime.build_types,
        2,
        false,
        &sim.prod_rules,
        &authority,
        prepared,
    )
    .is_err());
    assert_eq!(sim.builds[site_row].image(), before);

    let (mut sim, actor_row, site_row, authority) = fixture();
    let prepared = prepare_build_at_work_activation(
        &sim.world,
        &sim.builds,
        &sim.unit_type,
        &sim.production_runtime.build_types,
        sim.step8_env.leaders[0].payout.ai_speed,
        false,
        &sim.prod_rules,
        &authority,
        actor_row,
    )
    .unwrap();
    let before = sim.builds[site_row].image();
    assert!(commit_build_at_work_activation(
        &sim.world,
        &mut sim.builds,
        &sim.unit_type,
        &sim.production_runtime.build_types,
        sim.step8_env.leaders[0].payout.ai_speed,
        true,
        &sim.prod_rules,
        &authority,
        prepared,
    )
    .is_err());
    assert_eq!(sim.builds[site_row].image(), before);
}

#[test]
fn production_do_frame_matches_direct_save_reload_and_resume() {
    let (mut direct, actor_row, site_row, authority) = fixture();
    let saved = save_load::save_sim(&direct).unwrap();
    let mut resumed = save_load::load_sim(&saved).unwrap();
    assert_eq!(
        resumed.build_at_work_authority,
        BuildAtWorkAuthority::default()
    );
    direct.replace_build_at_work_authority(authority.clone());
    let resumed_authority = reinstall_authority(&mut resumed, &authority);
    resumed.replace_build_at_work_authority(resumed_authority);

    direct.do_frame();
    resumed.do_frame();

    let receipt = direct.last_build_at_work_receipt.unwrap();
    assert_eq!(
        receipt,
        resumed.last_build_at_work_receipt.unwrap_or_else(|| {
            panic!(
                "resumed BUILD_AT refusal: {:?}",
                resumed.last_build_at_work_error
            )
        })
    );
    assert_eq!(
        receipt.branch,
        BuildAtWorkBranch::PeasantVillageNonCompleting
    );
    assert_eq!(
        (receipt.progress_before, receipt.progress_after),
        (25_100, 25_200)
    );
    assert_eq!(
        (receipt.recharge_before, receipt.recharge_after),
        (251, 252)
    );
    assert!(receipt.helper_latch_set);
    assert!(receipt.order_unchanged);
    assert!(receipt.guy_hold_attack_zero_write);
    assert_eq!((receipt.rng_draws, receipt.external_effects), (0, 0));
    assert_eq!(direct.builds[site_row].job_counter, 25_200);
    assert_eq!(direct.builds[site_row].job_counter_2, 25_200);
    assert_eq!(direct.builds[site_row].recharging, 252);
    // The target's later Build::process consumes the per-frame helper latch.
    assert_eq!(direct.builds[site_row].helpers, 0);
    assert_eq!(direct.builds[site_row].build_masks, FRESH_BUILD_MASKS);
    assert_eq!(
        direct.world.orders(actor_row).current(),
        Some(&build_at_order())
    );
    assert_eq!(direct.world.frame, resumed.world.frame);
    assert_eq!(direct.world.random.state(), resumed.world.random.state());
    assert_eq!(
        direct.builds[site_row].image(),
        resumed.builds[site_row].image()
    );
    assert_eq!(
        direct.world.orders(actor_row),
        resumed.world.orders(actor_row)
    );
}

#[test]
fn missing_or_mutated_authority_cannot_contribute_to_the_fresh_site() {
    let (mut missing, actor_row, site_row, _) = fixture();
    let before_order = missing.world.orders(actor_row).clone();
    missing.do_frame();
    assert_eq!(
        missing.builds[site_row].job_counter,
        FRESH_BUILD_JOB_COUNTER
    );
    assert_eq!(missing.builds[site_row].recharging, FRESH_BUILD_RECHARGING);
    assert_eq!(missing.world.orders(actor_row), &before_order);
    assert_eq!(
        missing.last_build_at_work_error,
        Some(BuildAtWorkRuntimeError::MissingCompositionDigest)
    );

    let (mut mutated, _actor_row, site_row, mut authority) = fixture();
    authority.actors[0].branch.adjacent = false;
    mutated.replace_build_at_work_authority(authority);
    mutated.do_frame();
    assert_eq!(
        mutated.builds[site_row].job_counter,
        FRESH_BUILD_JOB_COUNTER
    );
    assert_eq!(mutated.builds[site_row].recharging, FRESH_BUILD_RECHARGING);
    assert!(matches!(
        mutated.last_build_at_work_error,
        Some(BuildAtWorkRuntimeError::Plan(
            BuildAtWorkPlanError::Unowned(UnownedBuildAtArm::Reswarm)
        ))
    ));
}
