use don_sim::order::{Order, OrderIndex};
use don_sim::systems::canonical_cast_work::*;
use don_sim::systems::economy_order_payload_authority::{
    CastOrderPayload, EconomyOrderHeader, EconomyOrderNode, EconomyOrderPayload,
    StableTargetIdentity,
};
use don_sim::systems::save_load;
use don_sim::tick::Sim;

fn cast_order() -> Order {
    Order::economy(EconomyOrderNode {
        metric: 0,
        header: EconomyOrderHeader {
            kind: OrderIndex::CastSpell,
            flags: 0,
            x: -1,
            y: -1,
            primary: StableTargetIdentity::NONE,
        },
        payload: EconomyOrderPayload::CastSpell(CastOrderPayload {
            paid: 1,
            spell: FRESH_CAST_SPELL,
        }),
    })
    .unwrap()
}

fn authority(sim: &Sim, row: usize) -> CastWorkAuthority {
    CastWorkAuthority {
        revision: 73,
        composition_digest: [0x92; 32],
        actors: vec![CastActorRuntimeFacts {
            actor: sim.world.handle_at_row(row).unwrap(),
            who: sim.world.units.get_who(row),
            o: sim.world.units.o()[row],
            uid: sim.world.units.get_uid(row),
            type_index: sim.unit_type[row],
            type_facts: CastTypeFacts::FRESH_FISHERMEN_DEPLOY,
        }],
    }
}

fn reinstall_authority(sim: &Sim, old: &CastWorkAuthority) -> CastWorkAuthority {
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
    authority
}

fn fixture() -> (Sim, usize, CastWorkAuthority) {
    let mut sim = Sim::new(0x1234_5678, 4);
    let actor = sim
        .spawn_unit(0, FRESH_CAST_ACTOR_TYPE, 37_848, 24_792, 4)
        .unwrap();
    let row = sim.world.row_of(actor).unwrap();
    sim.world.units.spell_time_mut()[row] = FRESH_CAST_SPELL_TIME;
    sim.world.orders_mut(row).replace(cast_order());
    let authority = authority(&sim, row);
    (sim, row, authority)
}

#[test]
fn fresh_payload_and_binary_evidence_are_frozen() {
    assert_eq!(UNIT_DO_CAST_VA, 0x005e_bfe0);
    assert_eq!(UNIT_DO_CAST_BYTES, 4_191);
    assert_eq!(
        UNIT_DO_CAST_SHA256,
        "e6ddf5fe099b840ec1241f9b8d7aba81884646f2cf8ab45992ccdfce4602da7a"
    );
    assert_eq!(FRESH_CAST_PAYLOAD_SHA256.len(), 64);
    assert_eq!(FRESH_CAST_UNIT_SHA256.len(), 64);
    assert_eq!(FRESH_CAST_GUY_SHA256.len(), 64);
    assert_eq!(
        CastWorkOrder::FRESH.retail_payload_image(),
        [
            0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
            0xff, 0xff, 0xff, 0xff, 0xff, 0x01, 0x00, 0x00, 0x00, 0x92, 0x02, 0x00, 0x00,
        ]
    );
}

#[test]
fn pure_plan_is_exact_single_write_waiting_branch() {
    let before = CastWorkSnapshot {
        authority_revision: 73,
        actor: CastActorImage {
            who: 0,
            o: 16,
            uid: 9,
            type_index: FRESH_CAST_ACTOR_TYPE,
            active: true,
            spell_time: FRESH_CAST_SPELL_TIME,
        },
        order: CastWorkOrder::FRESH,
        type_facts: CastTypeFacts::FRESH_FISHERMEN_DEPLOY,
    };
    let plan = plan_fresh_cast_tick(before).unwrap();
    assert_eq!(plan.branch, CastWorkBranch::FishermenDeployWaiting);
    assert_eq!(plan.after.actor.spell_time, 13);
    assert_eq!(plan.after.order, before.order);
    assert_eq!(plan.after.type_facts, before.type_facts);
    assert_eq!(plan.after.authority_revision, before.authority_revision);
}

#[test]
fn every_adjacent_unowned_cone_fails_closed() {
    let base = CastWorkSnapshot {
        authority_revision: 1,
        actor: CastActorImage {
            who: 0,
            o: 16,
            uid: 9,
            type_index: FRESH_CAST_ACTOR_TYPE,
            active: true,
            spell_time: FRESH_CAST_SPELL_TIME,
        },
        order: CastWorkOrder::FRESH,
        type_facts: CastTypeFacts::FRESH_FISHERMEN_DEPLOY,
    };
    let mutations = [
        CastWorkSnapshot {
            order: CastWorkOrder {
                paid: 0,
                ..base.order
            },
            ..base
        },
        CastWorkSnapshot {
            actor: CastActorImage {
                type_index: 318,
                ..base.actor
            },
            ..base
        },
        CastWorkSnapshot {
            actor: CastActorImage {
                spell_time: 0,
                ..base.actor
            },
            ..base
        },
        CastWorkSnapshot {
            type_facts: CastTypeFacts {
                spell_flags: 2,
                ..base.type_facts
            },
            ..base
        },
        CastWorkSnapshot {
            type_facts: CastTypeFacts {
                pack_predicate: true,
                ..base.type_facts
            },
            ..base
        },
        CastWorkSnapshot {
            type_facts: CastTypeFacts {
                deploy_predicate: false,
                ..base.type_facts
            },
            ..base
        },
        CastWorkSnapshot {
            type_facts: CastTypeFacts {
                actor_is_siege: true,
                ..base.type_facts
            },
            ..base
        },
        CastWorkSnapshot {
            actor: CastActorImage {
                spell_time: 39,
                ..base.actor
            },
            ..base
        },
    ];
    for mutation in mutations {
        assert!(plan_fresh_cast_tick(mutation).is_err());
    }
}

#[test]
fn runtime_commit_revalidates_every_owner_before_the_write() {
    let (mut sim, row, authority) = fixture();
    let prepared = prepare_cast_work_activation(&sim.world, &sim.unit_type, &authority, row)
        .expect("fresh exact witness is admitted");
    sim.world.units.spell_time_mut()[row] = 14;
    let order_before = sim.world.orders(row).clone();
    let err = commit_cast_work_activation(&mut sim.world, &sim.unit_type, &authority, prepared)
        .unwrap_err();
    assert_eq!(err, CastWorkRuntimeError::StaleState);
    assert_eq!(sim.world.units.spell_time()[row], 14);
    assert_eq!(sim.world.orders(row), &order_before);
}

#[test]
fn production_do_frame_matches_direct_save_reload_and_resume() {
    let (mut direct, actor_row, authority) = fixture();
    let bytes = save_load::save_sim(&direct).unwrap();
    let mut resumed = save_load::load_sim(&bytes).unwrap();
    assert_eq!(resumed.cast_work_authority, CastWorkAuthority::default());

    direct.replace_cast_work_authority(authority.clone());
    resumed.replace_cast_work_authority(reinstall_authority(&resumed, &authority));
    direct.do_frame();
    resumed.do_frame();

    let receipt = direct.last_cast_work_receipt.unwrap();
    assert_eq!(receipt, resumed.last_cast_work_receipt.unwrap());
    assert_eq!(receipt.branch, CastWorkBranch::FishermenDeployWaiting);
    assert_eq!(
        (receipt.spell_time_before, receipt.spell_time_after),
        (12, 13)
    );
    assert_eq!(receipt.job_time, 40);
    assert!(receipt.order_unchanged);
    assert_eq!((receipt.rng_draws, receipt.external_effects), (0, 0));
    assert_eq!(direct.world.units.spell_time()[actor_row], 13);
    assert_eq!(
        direct.world.orders(actor_row).current(),
        Some(&cast_order())
    );
    assert_eq!(
        save_load::save_sim(&direct).unwrap(),
        save_load::save_sim(&resumed).unwrap()
    );
}

#[test]
fn missing_or_mutated_installed_authority_is_non_mutating() {
    let (mut sim, row, _authority) = fixture();
    let before_order = sim.world.orders(row).clone();
    let before_timer = sim.world.units.spell_time()[row];
    sim.do_frame();
    assert_eq!(sim.world.units.spell_time()[row], before_timer);
    assert_eq!(sim.world.orders(row), &before_order);
    assert_eq!(
        sim.last_cast_work_error,
        Some(CastWorkRuntimeError::MissingCompositionDigest)
    );

    let (mut sim, row, mut authority) = fixture();
    authority.actors[0].type_facts.actor_is_siege = true;
    sim.replace_cast_work_authority(authority);
    sim.do_frame();
    assert_eq!(sim.world.units.spell_time()[row], FRESH_CAST_SPELL_TIME);
    assert!(matches!(
        sim.last_cast_work_error,
        Some(CastWorkRuntimeError::Plan(CastWorkPlanError::Unowned(
            UnownedCastArm::SiegeGeneralAndContainedTimers
        )))
    ));
}
