// SPDX-License-Identifier: GPL-3.0-or-later

use don_ai::arena::match_run::{load_world, MatchConfig};
use don_ai::arena::retail_systems::ArenaPlacementVerdict;
use don_ai::arena::world::{ConstructionMode, ConstructionRefusal, Job, World};
use don_ai::arena::{Cmd, EntId};
use don_ai::OrderResult;
use don_sim::objects::{BANDED_SLOTS, BUILD_BAND_BASE, OWNER_SLOTS, WALL_BAND_BASE};
use don_sim::systems::construction::{BuildOutcome, ChecksumEffects, ObjectKey};
use don_sim::systems::construction_lifecycle::LEADER_ACTIVATION_DIRTY;
use don_sim::systems::production::{flag, mask};

fn world(mode: ConstructionMode) -> Option<don_ai::arena::World> {
    let mut cfg = MatchConfig::default();
    cfg.arena.construction_mode = mode;
    load_world(&cfg).ok()
}

fn citizen(w: &don_ai::arena::World) -> EntId {
    w.own_ents(0)
        .find(|e| e.type_id == w.ids.citizen)
        .expect("the shipped Small Town start has citizens")
        .id
}

fn legal_farm_site(w: &don_ai::arena::World, worker: EntId) -> (i32, i32) {
    let (cx, cy) = w.ent(worker).unwrap().tile();
    for radius in 1i32..=18 {
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                if dx.abs().max(dy.abs()) != radius {
                    continue;
                }
                let (tx, ty) = (cx + dx, cy + dy);
                if w.placement_ok(0, w.ids.farm, tx, ty).is_ok() {
                    return (tx, ty);
                }
            }
        }
    }
    panic!("generated start has no legal Farm site within its city radius")
}

fn place_farm(w: &mut don_ai::arena::World, worker: EntId) -> EntId {
    let (tx, ty) = legal_farm_site(w, worker);
    match w.submit(
        0,
        Cmd::Build {
            worker,
            type_id: w.ids.farm,
            tx,
            ty,
        },
    ) {
        OrderResult::Ok(raw) => EntId(raw as u32),
        other => panic!("legal paid Farm command was not accepted: {other:?}"),
    }
}

fn legal_site(w: &don_ai::arena::World, worker: EntId, type_id: i32) -> (i32, i32) {
    let (cx, cy) = w.ent(worker).unwrap().tile();
    for radius in 1i32..=18 {
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                if dx.abs().max(dy.abs()) != radius {
                    continue;
                }
                let (tx, ty) = (cx + dx, cy + dy);
                if w.placement_ok(0, type_id, tx, ty).is_ok() {
                    return (tx, ty);
                }
            }
        }
    }
    panic!("generated start has no legal site for type {type_id}")
}

fn place_building(w: &mut don_ai::arena::World, worker: EntId, type_id: i32) -> EntId {
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
        other => panic!("legal paid building command was not accepted: {other:?}"),
    }
}

#[test]
fn site_and_order_keep_builddata_and_the_full_generational_target() {
    let Some(mut w) = world(ConstructionMode::ResearchModel) else {
        return;
    };
    let worker = citizen(&w);
    let site = place_farm(&mut w, worker);
    let target = w.ent(worker).unwrap().build_order.unwrap().target;
    let building = w.ent(site).unwrap();
    let build = building.build.as_ref().unwrap();

    assert_eq!(target.who, i32::from(building.who));
    assert_eq!(target.o, i32::from(building.object_o));
    assert_eq!(target.uid, building.object_uid);
    assert_eq!(build.uid, target.uid);
    assert_eq!(build.who, building.who);
    assert_eq!(build.flags, flag::VALID);
    assert_eq!(build.job_counter, 0);
    assert_eq!(build.job_counter_2, 0);
    assert!(w
        .ent(worker)
        .unwrap()
        .motion
        .as_ref()
        .unwrap()
        .orders
        .iter()
        .any(|order| order.kind == don_sim::order::OrderIndex::BuildAt
            && order.target_who == target.who
            && order.target_o == target.o
            && order.target_uid == target.uid));
}

#[test]
fn owner_local_bands_and_uid_stream_reach_every_installed_object_view() {
    let Some(mut w) = world(ConstructionMode::ResearchModel) else {
        return;
    };
    let worker = citizen(&w);
    let site = place_farm(&mut w, worker);

    for who in 0..w.players.len() {
        let objects: Vec<_> = w
            .ents
            .iter()
            .filter(|e| usize::from(e.who) == who)
            .collect();
        assert!(!objects.is_empty());
        for (expected_uid, e) in objects.iter().enumerate() {
            assert_eq!(e.object_uid, expected_uid as u16);
            if let Some(motion) = &e.motion {
                assert_eq!(motion.o, e.object_o);
                assert_eq!(motion.uid, e.object_uid);
            }
            for guy in e.guys.guys.iter().flatten() {
                assert_eq!(guy.o, e.object_o);
            }
            if let Some(build) = &e.build {
                assert_eq!(build.uid, e.object_uid);
                assert_eq!(build.who, e.who);
                assert_eq!(build.city, w.ent(e.city).map_or(-1, |city| city.object_o));
            }
        }

        let mut units: Vec<_> = objects.iter().filter(|e| !e.building).collect();
        units.sort_unstable_by_key(|e| e.object_o);
        assert_eq!(
            units.iter().map(|e| e.object_o).collect::<Vec<_>>(),
            (0..units.len()).map(|o| o as i16).collect::<Vec<_>>()
        );

        let mut buildings: Vec<_> = objects.iter().filter(|e| e.building).collect();
        buildings.sort_unstable_by_key(|e| e.object_o);
        assert_eq!(
            buildings.iter().map(|e| e.object_o).collect::<Vec<_>>(),
            (BUILD_BAND_BASE..BUILD_BAND_BASE + buildings.len() as u32)
                .map(|o| o as i16)
                .collect::<Vec<_>>()
        );
        assert!(buildings
            .iter()
            .all(|e| u32::try_from(e.object_o).unwrap() < WALL_BAND_BASE));
    }

    let site = w.ent(site).unwrap();
    let target = w.ent(worker).unwrap().build_order.unwrap().target;
    assert_eq!(target.who, i32::from(site.who));
    assert_eq!(target.o, i32::from(site.object_o));
    assert_eq!(target.uid, site.object_uid);
}

fn band_ids(w: &World, who: usize, buildings: bool) -> Vec<EntId> {
    let mut rows: Vec<_> = w
        .ents
        .iter()
        .filter(|e| e.alive && usize::from(e.who) == who && e.building == buildings)
        .collect();
    rows.sort_unstable_by_key(|e| e.object_o);
    rows.into_iter().map(|e| e.id).collect()
}

#[test]
fn object_pass_uses_preincrement_ten_slot_unit_rotation_then_fixed_build_bands() {
    let Some(mut w) = world(ConstructionMode::ResearchModel) else {
        return;
    };
    assert_eq!(w.players.len(), 2, "default arena fixture has two leaders");

    for object_frame in 0i64..12 {
        let unit_owners: Vec<_> = (0..OWNER_SLOTS)
            .map(|i| (object_frame.rem_euclid(OWNER_SLOTS as i64) as usize + i) % OWNER_SLOTS)
            .filter(|&who| who < w.players.len() && w.players[who].alive)
            .collect();
        let mut expected = Vec::new();
        for who in unit_owners {
            expected.extend(band_ids(&w, who, false));
        }
        for who in 0..w.players.len().min(BANDED_SLOTS) {
            if w.players[who].alive {
                expected.extend(band_ids(&w, who, true));
            }
        }

        w.step();
        assert_eq!(w.frame, object_frame + 1);
        assert_eq!(w.last_object_process_order, expected);
    }
}

#[test]
fn site_frame_consumes_live_unit_contribution_in_the_same_object_pass() {
    let Some(mut w) = world(ConstructionMode::ResearchModel) else {
        return;
    };
    let worker = citizen(&w);
    let site = place_farm(&mut w, worker);

    for _ in 0..600 {
        w.step();
        let build = w.ent(site).unwrap().build.as_ref().unwrap();
        if build.job_counter > 0 {
            assert_eq!(build.helpers, 0);
            assert_ne!(build.build_masks & mask::WORKED_LAST_FRAME, 0);
            assert_eq!(build.build_masks & mask::HELPER_COUNTED, 0);
            assert!(matches!(
                w.ent(worker).unwrap().job,
                Job::Work { target } if target == site
            ));
            return;
        }
    }
    panic!("builder never contributed to the paid construction site");
}

#[test]
fn site_frame_reset_and_cancel_keep_already_credited_work() {
    let Some(mut w) = world(ConstructionMode::ResearchModel) else {
        return;
    };
    let worker = citizen(&w);
    let site = place_farm(&mut w, worker);
    {
        let build = w.ents[site.index().unwrap()].build.as_mut().unwrap();
        build.job_counter = 77;
        build.job_counter_2 = 77;
        build.helpers = 3;
        build.build_masks |= mask::HELPER_COUNTED;
    }

    assert!(matches!(
        w.submit(0, Cmd::Halt { unit: worker }),
        OrderResult::Ok(_)
    ));
    assert!(w.ent(worker).unwrap().build_order.is_none());
    w.step();

    let build = w.ent(site).unwrap().build.as_ref().unwrap();
    assert_eq!(build.helpers, 0);
    assert_ne!(build.build_masks & mask::WORKED_LAST_FRAME, 0);
    assert_eq!(build.build_masks & mask::HELPER_COUNTED, 0);
    assert_eq!((build.job_counter, build.job_counter_2), (77, 77));
    assert!(!w.ent(site).unwrap().complete);
}

#[test]
fn claim_bearing_mode_stops_before_the_missing_world_transaction() {
    let Some(mut w) = world(ConstructionMode::FailClosedRetail) else {
        return;
    };
    let worker = citizen(&w);
    let site = place_farm(&mut w, worker);

    for _ in 0..600 {
        if w.ent(site).unwrap().construction_refusal.is_some() {
            break;
        }
        w.step();
    }

    let building = w.ent(site).unwrap();
    assert!(matches!(
        building.construction_refusal,
        Some(
            ConstructionRefusal::MissingBuilderAnimationTransaction
                | ConstructionRefusal::MissingReswarmTransaction
        )
    ));
    let build = building.build.as_ref().unwrap();
    assert_eq!(build.flags, flag::VALID);
    assert_eq!((build.job_counter, build.job_counter_2), (0, 0));
    assert!(!building.complete);
    assert!(matches!(w.ent(worker).unwrap().job, Job::Work { target } if target == site));
}

#[test]
fn playable_research_mode_uses_builddata_progress_and_local_completion_core() {
    let Some(mut w) = world(ConstructionMode::ResearchModel) else {
        return;
    };
    let worker = citizen(&w);
    let site = place_farm(&mut w, worker);

    for _ in 0..600 {
        if w.ent(site).unwrap().complete {
            break;
        }
        w.step();
    }

    let building = w.ent(site).unwrap();
    assert!(building.complete);
    assert_eq!(building.build_left, 0);
    assert!(building.construction_refusal.is_none());
    let build = building.build.as_ref().unwrap();
    assert_eq!(
        build.flags & (flag::VALID | flag::STARTED | flag::ACTIVE),
        flag::VALID | flag::STARTED | flag::ACTIVE
    );
    assert_eq!((build.job_counter, build.job_counter_2), (0, 0));
    assert_eq!(build.recharging, 0);
    assert_ne!(build.build_masks & 0x1000, 0);
}

#[test]
fn plain_site_start_reject_and_completion_keep_live_identities_and_effects() {
    let Some(mut completed_world) = world(ConstructionMode::ResearchModel) else {
        return;
    };
    completed_world.players[0].techs.insert(572);
    let completed_builder = citizen(&completed_world);
    let completed_type = completed_world.ids.barracks;
    let completed_site = place_building(&mut completed_world, completed_builder, completed_type);
    let site_key = {
        let site = completed_world.ent(completed_site).unwrap();
        ObjectKey {
            who: i32::from(site.who),
            o: i32::from(site.object_o),
            uid: site.object_uid,
        }
    };
    let builder_key = {
        let builder = completed_world.ent(completed_builder).unwrap();
        ObjectKey {
            who: i32::from(builder.who),
            o: i32::from(builder.object_o),
            uid: builder.object_uid,
        }
    };

    let mut first_progress = None;
    for _ in 0..600 {
        completed_world.step();
        let site = completed_world.ent(completed_site).unwrap();
        if first_progress.is_none() {
            first_progress = site.last_construction_receipt;
        }
        if site.complete {
            break;
        }
    }
    let first = first_progress.expect("plain site never entered the recovered transaction");
    assert_eq!(
        (first.site, first.builder, first.order_target),
        (site_key, builder_key, site_key)
    );
    assert!(matches!(
        first.receipt.outcome,
        BuildOutcome::Progressed {
            credited: 1..,
            started_this_call: true,
        }
    ));
    assert_eq!(first.receipt.rng_draws, 0);
    assert_eq!(
        first.receipt.checksums,
        ChecksumEffects {
            builds: true,
            units: true,
            guys: true,
            leaders: false,
            cities: false,
            groups: false,
            world: true,
            objects_other: false,
        }
    );

    let completed = completed_world.ent(completed_site).unwrap();
    assert!(completed.complete);
    let placement = completed
        .last_construction_placement_receipt
        .as_ref()
        .expect("Barracks lifecycle did not retain its blocked_site claims");
    assert_eq!(placement.site, site_key);
    assert_eq!(placement.type_id, completed_type);
    assert_eq!((placement.x_size, placement.y_size), (4, 4));
    assert_eq!(placement.claims.len(), 16);
    assert_eq!(placement.raw_code, 0);
    assert!(placement.accepted);
    assert_eq!(placement.verdict, ArenaPlacementVerdict::Admitted);
    for (index, claim) in placement.claims.iter().enumerate() {
        assert_eq!(
            (claim.tile.x, claim.tile.y),
            (
                placement.corner.x + (index / 4) as i32,
                placement.corner.y + (index % 4) as i32,
            ),
            "blocked_site did not walk x-outer/y-inner"
        );
        assert!(claim.in_bounds);
        assert!(claim.explored);
        assert!(claim.occupant.is_none());
    }
    let final_receipt = completed.last_construction_receipt.unwrap();
    assert_eq!(
        (final_receipt.site, final_receipt.builder),
        (site_key, builder_key)
    );
    assert!(matches!(
        final_receipt.receipt.outcome,
        BuildOutcome::Completed {
            credited: 1..,
            started_this_call: false,
        }
    ));
    assert_eq!(final_receipt.receipt.rng_draws, 0);
    assert_eq!(
        final_receipt.receipt.checksums,
        ChecksumEffects {
            builds: true,
            units: true,
            guys: true,
            leaders: true,
            cities: true,
            groups: false,
            world: true,
            objects_other: false,
        }
    );
    let build = completed.build.as_ref().unwrap();
    assert_eq!(
        build.flags & (flag::VALID | flag::STARTED | flag::ACTIVE),
        flag::VALID | flag::STARTED | flag::ACTIVE
    );
    assert_eq!(
        (build.job_counter, build.job_counter_2, build.recharging),
        (0, 0, 0)
    );
    assert_ne!(build.build_masks & 0x1000, 0);
    assert_eq!(
        completed_world.ent(completed_builder).unwrap().build_order,
        None
    );
    assert_eq!(
        completed_world.ent(completed_builder).unwrap().job,
        Job::Idle
    );
    assert_eq!(
        completed_world.players[0].construction_lifecycle_flags & LEADER_ACTIVATION_DIRTY,
        LEADER_ACTIVATION_DIRTY
    );

    let Some(mut rejected_world) = world(ConstructionMode::ResearchModel) else {
        return;
    };
    rejected_world.players[0].techs.insert(572);
    let rejected_builder = citizen(&rejected_world);
    let type_id = rejected_world.ids.barracks;
    let cost = rejected_world.types.get(type_id).unwrap().cost;
    let rejected_site = place_building(&mut rejected_world, rejected_builder, type_id);
    let rejected_index = rejected_site.index().unwrap();
    let rejected_builder_key = {
        let builder = rejected_world.ent(rejected_builder).unwrap();
        ObjectKey {
            who: i32::from(builder.who),
            o: i32::from(builder.object_o),
            uid: builder.object_uid,
        }
    };
    // Mutate a real completed building into the paid site's footprint *after* the
    // command-time model accepted it. The construction call must ignore that stale
    // decision, rescan live object/type footprints, and reject with the blocker's exact
    // `(who,o,uid)` identity.
    let (site_x, site_y) = {
        let site = &rejected_world.ents[rejected_index];
        (site.x, site.y)
    };
    let blocker_index = rejected_world
        .ents
        .iter()
        .position(|ent| ent.alive && ent.complete && ent.building && ent.id != rejected_site)
        .expect("Small Town start has a completed placement blocker");
    let blocker_key = {
        let blocker = &rejected_world.ents[blocker_index];
        ObjectKey {
            who: i32::from(blocker.who),
            o: i32::from(blocker.object_o),
            uid: blocker.object_uid,
        }
    };
    rejected_world.ents[blocker_index].x = site_x;
    rejected_world.ents[blocker_index].y = site_y;

    // Put the builder exactly one tile from the target so the next object pass enters
    // do_build; this mutates every Arena position view consumed by that pass.
    let builder_index = rejected_builder.index().unwrap();
    rejected_world.ents[builder_index].x = site_x + 192;
    rejected_world.ents[builder_index].y = site_y;
    if let Some(motion) = rejected_world.ents[builder_index].motion.as_mut() {
        motion.body.x = site_x + 192;
        motion.body.y = site_y;
    }
    for guy in rejected_world.ents[builder_index]
        .guys
        .guys
        .iter_mut()
        .flatten()
    {
        guy.x = site_x + 192;
        guy.y = site_y;
        guy.des_x = site_x + 192;
        guy.des_y = site_y;
    }
    rejected_world.players[0].stock = [1_000; 6];
    let stock_before = rejected_world.players[0].stock;
    rejected_world.step();

    assert!(rejected_world.ent(rejected_site).is_none());
    let rejected = &rejected_world.ents[rejected_index];
    let placement = rejected
        .last_construction_placement_receipt
        .as_ref()
        .expect("blocked placement did not retain its authoritative claims");
    assert_eq!(placement.raw_code, 1);
    assert!(!placement.accepted);
    assert!(matches!(
        placement.verdict,
        ArenaPlacementVerdict::Occupied { object, .. } if object == blocker_key
    ));
    assert_eq!(placement.claims.len(), 16);
    let receipt = rejected
        .last_construction_receipt
        .expect("blocked admission did not execute rejected Object::disband");
    assert_eq!(receipt.site, receipt.order_target);
    assert_eq!(receipt.builder, rejected_builder_key);
    assert_eq!(receipt.receipt.outcome, BuildOutcome::SiteRejected);
    assert_eq!(receipt.receipt.rng_draws, 0);
    assert_eq!(
        receipt.receipt.checksums,
        ChecksumEffects {
            builds: true,
            units: true,
            guys: true,
            leaders: true,
            cities: false,
            groups: false,
            world: true,
            objects_other: true,
        }
    );
    assert_eq!(rejected.build.as_ref().unwrap().flags & flag::VALID, 0);
    for good in 0..6 {
        assert_eq!(
            rejected_world.players[0].stock[good],
            stock_before[good].wrapping_add(cost[good]),
            "good {good} did not receive the exact full-cost rejection refund"
        );
    }
}
