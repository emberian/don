// SPDX-License-Identifier: GPL-3.0-or-later

use don_ai::arena::match_run::{load_world, MatchConfig};
use don_ai::arena::world::{ConstructionMode, Job, World};
use don_ai::arena::{Cmd, EntId};
use don_ai::OrderResult;
use don_sim::systems::construction::{BuildOutcome, BuilderFinish, ChecksumEffects};

fn world() -> Option<World> {
    let mut cfg = MatchConfig::default();
    cfg.arena.construction_mode = ConstructionMode::ResearchModel;
    load_world(&cfg).ok()
}

fn citizen(w: &World) -> EntId {
    w.own_ents(0)
        .find(|ent| ent.type_id == w.ids.citizen)
        .expect("the shipped Small Town start has citizens")
        .id
}

fn legal_site(w: &World, worker: EntId, type_id: i32) -> (i32, i32) {
    let (cx, cy) = w.ent(worker).expect("worker is live").tile();
    for radius in 1i32..=18 {
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                if dx.abs().max(dy.abs()) == radius
                    && w.placement_ok(0, type_id, cx + dx, cy + dy).is_ok()
                {
                    return (cx + dx, cy + dy);
                }
            }
        }
    }
    panic!("generated start has no legal site for type {type_id}")
}

fn place_site(w: &mut World, worker: EntId) -> EntId {
    w.players[0].stock = [10_000; 6];
    let type_id = w.ids.farm;
    let (tx, ty) = legal_site(w, worker, type_id);
    match w.submit(
        0,
        Cmd::Build {
            worker,
            type_id,
            tx,
            ty,
        },
    ) {
        OrderResult::Ok(raw) => EntId(raw as u32),
        other => panic!("legal Farm command was not accepted: {other:?}"),
    }
}

#[test]
fn explicit_halt_records_the_shared_order_cancelled_transaction() {
    let Some(mut w) = world() else { return };
    let worker = citizen(&w);
    let site = place_site(&mut w, worker);
    let site_before = w
        .ent(site)
        .expect("construction site is live")
        .build
        .clone()
        .expect("construction site owns BuildData");

    assert_eq!(w.submit(0, Cmd::Halt { unit: worker }), OrderResult::Ok(1));

    let builder = w.ent(worker).expect("cancelled builder remains live");
    let receipt = builder
        .last_construction_interruption
        .expect("explicit cancellation records the shared receipt");
    assert_eq!(
        receipt.outcome,
        BuildOutcome::OrderRetired(BuilderFinish::OrderCancelled)
    );
    assert_eq!(receipt.rng_draws, 0);
    assert_eq!(
        receipt.checksums,
        ChecksumEffects {
            units: true,
            ..ChecksumEffects::NONE
        }
    );
    assert_eq!(builder.job, Job::Idle);
    assert!(builder.build_order.is_none());
    assert!(builder
        .motion
        .as_ref()
        .expect("citizen has motion state")
        .orders
        .is_empty());
    assert_eq!(
        w.ent(site)
            .expect("site survives cancellation")
            .build
            .as_ref()
            .expect("site retains BuildData")
            .image(),
        site_before.image(),
        "builder interruption owns no site-side membership or progress mutation"
    );
}

#[test]
fn reap_records_builder_died_before_unlinking_the_unit() {
    let Some(mut w) = world() else { return };
    let worker = citizen(&w);
    let site = place_site(&mut w, worker);
    let worker_index = worker.index().expect("Arena handle has a dense slot");
    w.ents[worker_index].hp.damage = w.ents[worker_index].hp.myhits;
    // Isolate `reap` from the preceding healing band. Dead leaders are excluded from the
    // object pass, while `reap` still walks every live object slot.
    w.players[0].alive = false;

    w.step();

    let builder = &w.ents[worker_index];
    assert!(!builder.alive, "builder survived with hp {:?}", builder.hp);
    assert!(
        w.ent(site).is_some(),
        "builder death does not disband its site"
    );
    let receipt = builder
        .last_construction_interruption
        .expect("death records the shared interruption receipt before unlink");
    assert_eq!(
        receipt.outcome,
        BuildOutcome::OrderRetired(BuilderFinish::BuilderDied)
    );
    assert_eq!(receipt.rng_draws, 0);
    assert_eq!(
        receipt.checksums,
        ChecksumEffects {
            units: true,
            ..ChecksumEffects::NONE
        }
    );
    assert!(builder.build_order.is_none());
}
