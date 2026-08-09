//! Product wiring for the exact ordinary HALT/DISBAND group-action subdomain.

use don_env::action::{apply_unit, ApplyStats, UnitAction};
use don_env::generated as g;
use don_env::spec::EnvConfig;
use don_env::state::{EnvWorld, Rules};
use don_env::typecaps::{F_BUILDING, F_UNIT};
use don_sim::command::{build, Bridge, Package, QueuePos};
use don_sim::systems::order_dispatch::OrderRec;
use don_sim::world::SUBTILE;

fn fixture() -> Option<(EnvConfig, EnvWorld, don_sim::Handle, don_sim::Handle)> {
    let (rules, caps_real, _) = Rules::load(None, None);
    if !caps_real {
        eprintln!("SKIP: schema/live/env-typecaps.bin absent");
        return None;
    }
    let cfg = EnvConfig {
        grid_w: 16,
        grid_h: 16,
        max_entities: 8,
        max_controlled: 4,
        ..Default::default()
    };
    let mut world = EnvWorld::new(rules, 8, 0x71FE, cfg.grid_w, cfg.grid_h);
    let ground_type = (g::UNIT_TYPE_BASE..g::GAIA_TYPE_BASE)
        .map(|value| value as u16)
        .find(|&ty| {
            let cap = world.rules.caps.get(ty);
            cap.has(F_UNIT) && !cap.has(F_BUILDING) && !cap.is_plane
        })
        .expect("shipped table has an ordinary ground unit");
    let building_type = (g::BUILD_TYPE_BASE..g::BUILD_TYPE_BASE + g::NUM_BUILDTYPES)
        .map(|value| value as u16)
        .find(|&ty| world.rules.caps.get(ty).has(F_BUILDING))
        .expect("shipped table has a building");
    let unit = world
        .spawn(0, ground_type, 4 * SUBTILE, 4 * SUBTILE)
        .unwrap();
    let building = world
        .spawn(0, building_type, 6 * SUBTILE, 4 * SUBTILE)
        .unwrap();
    world.obs_ents[0] = vec![unit, building];
    Some((cfg, world, unit, building))
}

#[test]
fn ordinary_halt_clears_orders_and_both_retail_unit_mask_bits() {
    let Some((cfg, mut world, unit, _)) = fixture() else {
        return;
    };
    let row = world.sim.row_of(unit).unwrap();
    world
        .install_order(row, OrderRec::move_to(10, 20, 0), QueuePos::New)
        .unwrap();
    world.sim.units.set_unit_masks(row, 0x8400_0100 | 0x40);
    let mut stats = ApplyStats::default();
    apply_unit(
        &mut world,
        &cfg,
        0,
        unit,
        UnitAction {
            verb: (g::uv::HALT + 1) as u16,
            ..Default::default()
        },
        &mut stats,
    );

    assert_eq!(stats.applied, 1);
    assert!(world.orders[row].is_empty());
    assert_eq!(world.order[row], g::OrderIndex::None as u8);
    assert_eq!(world.sim.units.get_unit_masks(row), 0x8000_0040);
    assert_eq!(
        (world.dest_x[row], world.dest_y[row]),
        (world.sim.pos_x()[row], world.sim.pos_y()[row])
    );
}

#[test]
fn ordinary_disband_uses_the_transactional_object_retirement_path() {
    let Some((cfg, mut world, unit, _)) = fixture() else {
        return;
    };
    let before = world.sim.live_count();
    let mut stats = ApplyStats::default();
    apply_unit(
        &mut world,
        &cfg,
        0,
        unit,
        UnitAction {
            verb: (g::uv::DISBAND + 1) as u16,
            ..Default::default()
        },
        &mut stats,
    );
    assert_eq!(stats.applied, 1);
    assert_eq!(world.sim.live_count(), before - 1);
    assert!(world.sim.row_of(unit).is_none());
}

#[test]
fn command_bridge_halt_uses_the_atomic_envworld_host() {
    let Some((_cfg, mut world, unit, _)) = fixture() else {
        return;
    };
    let row = world.sim.row_of(unit).unwrap();
    let object = world.sim.units.o()[row];
    world
        .install_order(row, OrderRec::move_to(70, 80, 0), QueuePos::New)
        .unwrap();
    world.sim.units.set_unit_masks(row, 0x8400_0100 | 0x80);
    let mut bridge = Bridge::new();
    let mut package = Package::new(0, 0);
    bridge
        .process_all(&mut package, &build::group(0, &[object]), &mut world)
        .unwrap();
    bridge
        .process_all(&mut package, &build::halt(), &mut world)
        .unwrap();

    assert!(world.orders[row].is_empty());
    assert_eq!(world.sim.units.get_unit_masks(row), 0x8000_0080);
    assert_eq!(bridge.stats.orders_cleared, 1);
}

#[test]
fn command_bridge_disband_uses_the_atomic_envworld_host() {
    let Some((_cfg, mut world, unit, _)) = fixture() else {
        return;
    };
    let row = world.sim.row_of(unit).unwrap();
    let object = world.sim.units.o()[row];
    let before = world.sim.live_count();
    let mut bridge = Bridge::new();
    let mut package = Package::new(0, 0);
    bridge
        .process_all(&mut package, &build::group(0, &[object]), &mut world)
        .unwrap();
    bridge
        .process_all(&mut package, &build::disband(0), &mut world)
        .unwrap();

    assert_eq!(world.sim.live_count(), before - 1);
    assert!(world.sim.row_of(unit).is_none());
}

#[test]
fn building_lifecycle_and_unrecovered_stance_requests_fail_closed() {
    let Some((cfg, mut world, unit, building)) = fixture() else {
        return;
    };
    for (actor, verb) in [
        (building, g::uv::HALT),
        (building, g::uv::DISBAND),
        (unit, g::uv::STANCE),
    ] {
        let before_live = world.sim.live_count();
        let mut stats = ApplyStats::default();
        apply_unit(
            &mut world,
            &cfg,
            0,
            actor,
            UnitAction {
                verb: (verb + 1) as u16,
                stance: 2,
                ..Default::default()
            },
            &mut stats,
        );
        assert_eq!(stats.illegal, 1);
        assert_eq!(world.sim.live_count(), before_live);
    }
}

#[test]
fn true_airborne_plane_halt_is_not_executed_as_ground_halt() {
    let (rules, caps_real, _) = Rules::load(None, None);
    if !caps_real {
        return;
    }
    let plane_type = (g::UNIT_TYPE_BASE..g::GAIA_TYPE_BASE)
        .map(|value| value as u16)
        .find(|&ty| rules.caps.get(ty).is_plane)
        .expect("shipped table has a true plane");
    let cfg = EnvConfig::default();
    let mut world = EnvWorld::new(rules, 4, 0xA1A, cfg.grid_w, cfg.grid_h);
    let plane = world.spawn(0, plane_type, SUBTILE, SUBTILE).unwrap();
    let row = world.sim.row_of(plane).unwrap();
    world
        .install_order(row, OrderRec::move_to(10, 20, 0), QueuePos::New)
        .unwrap();
    let before = world.orders[row].clone();
    let mut stats = ApplyStats::default();
    apply_unit(
        &mut world,
        &cfg,
        0,
        plane,
        UnitAction {
            verb: (g::uv::HALT + 1) as u16,
            ..Default::default()
        },
        &mut stats,
    );
    assert_eq!(stats.illegal, 1);
    assert_eq!(world.orders[row], before);
}
