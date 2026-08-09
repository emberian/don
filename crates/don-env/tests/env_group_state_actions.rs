//! Product-host coverage for the adjacent STANCE / UNITMASK / BUILDMASK /
//! SET_TRANSPORT cohort.

use don_env::action::{apply_unit, ApplyStats, UnitAction};
use don_env::generated as g;
use don_env::mask::MaskWriter;
use don_env::spec::{get_bit, EnvConfig};
use don_env::state::{EnvWorld, Rules};
use don_env::typecaps::{F_BUILDING, F_UNIT};
use don_sim::command::{build, Bridge, Package, QueuePos};
use don_sim::systems::groups_guys::{
    GroupBuildMaskRequest, GroupData, GroupSetTransportRequest, GroupStanceRequest,
    GroupStateTransactionStatus, GroupUnitMaskRequest,
};
use don_sim::systems::order_dispatch::OrderRec;
use don_sim::world::SUBTILE;

fn rules() -> Option<std::sync::Arc<Rules>> {
    let (rules, caps_real, _) = Rules::load(None, None);
    if !caps_real {
        eprintln!("SKIP: schema/live/env-typecaps.bin absent");
        return None;
    }
    Some(rules)
}

fn stance_type(rules: &Rules, type_index: u16) -> Option<i32> {
    rules
        .formation_cap(type_index)
        .map(|cap| cap.stance_type(type_index))
}

fn ordinary_type(rules: &Rules) -> u16 {
    (g::UNIT_TYPE_BASE..g::GAIA_TYPE_BASE)
        .map(|value| value as u16)
        .find(|&type_index| {
            let cap = rules.caps.get(type_index);
            cap.has(F_UNIT) && !cap.has(F_BUILDING) && !cap.is_plane && cap.domain == 0
        })
        .expect("shipped table has an ordinary land unit")
}

fn exact_stance_type(rules: &Rules) -> Option<(u16, i32)> {
    (g::UNIT_TYPE_BASE..g::GAIA_TYPE_BASE)
        .map(|value| value as u16)
        .find_map(|type_index| {
            let cap = rules.caps.get(type_index);
            let stance_type = stance_type(rules, type_index)?;
            (!cap.has(F_BUILDING) && !cap.is_plane && matches!(stance_type, 1..=3))
                .then_some((type_index, stance_type))
        })
}

fn transient_group(world: &EnvWorld, row: usize, is_building: bool) -> GroupData {
    let mut group = GroupData::default();
    group.add(
        world.sim.units.o()[row],
        world.sim.owner()[row] as u8,
        is_building,
        0,
        0,
    );
    group
}

fn fixed_i32_command(op: u8, args: &[i32]) -> Vec<u8> {
    let mut command = vec![op];
    for arg in args {
        command.extend_from_slice(&arg.to_le_bytes());
    }
    command
}

#[test]
fn exact_noncombat_stance_is_advertised_and_applied() {
    let Some(rules) = rules() else { return };
    let Some((type_index, stance_type)) = exact_stance_type(&rules) else {
        eprintln!("SKIP: captured formation table absent");
        return;
    };
    let cfg = EnvConfig {
        grid_w: 16,
        grid_h: 16,
        max_entities: 4,
        max_controlled: 2,
        ..Default::default()
    };
    let mut world = EnvWorld::new(rules, 4, 0x51A, cfg.grid_w, cfg.grid_h);
    let unit = world.spawn(0, type_index, SUBTILE, SUBTILE).unwrap();
    let row = world.sim.row_of(unit).unwrap();
    world.obs_ents[0] = vec![unit];

    let mut writer = MaskWriter::new(&cfg);
    let mut mask = vec![0; writer.unit.record_bytes * cfg.max_controlled];
    writer.write_unit_masks(&world, &cfg, 0, &[row], &[row], &mut mask);
    let verb = &mask[writer.unit.offsets[g::UnitHead::Verb as usize]..];
    assert!(get_bit(verb, g::uv::STANCE + 1));
    let stance = &mask[writer.unit.offsets[g::UnitHead::Stance as usize]..];
    let cycle = if stance_type == 1 { 4 } else { 2 };
    assert!((0..cycle).all(|option| get_bit(stance, option)));
    assert!((cycle..4).all(|option| !get_bit(stance, option)));

    let requested = cycle - 1;
    let mut stats = ApplyStats::default();
    apply_unit(
        &mut world,
        &cfg,
        0,
        unit,
        UnitAction {
            verb: (g::uv::STANCE + 1) as u16,
            stance: requested as u16,
            ..Default::default()
        },
        &mut stats,
    );
    assert_eq!(stats.applied, 1);
    assert_eq!(world.sim.units.stance()[row], requested as i8);
    assert_eq!(world.sim.units.get_flags(row) & 0x10, 0x10);
}

#[test]
fn retail_command_receivers_delegate_the_state_cohort_to_envworld() {
    let Some(rules) = rules() else { return };
    let Some((type_index, stance_type)) = exact_stance_type(&rules) else {
        eprintln!("SKIP: captured formation table absent");
        return;
    };
    let cfg = EnvConfig::default();
    let mut world = EnvWorld::new(rules, 4, 0xC04, cfg.grid_w, cfg.grid_h);
    let unit = world.spawn(0, type_index, SUBTILE, SUBTILE).unwrap();
    let row = world.sim.row_of(unit).unwrap();
    let object = world.sim.units.o()[row];
    world.players[0].leader_flags = 0x400;

    let mut bridge = Bridge::new();
    let mut package = Package::new(0, 0);
    bridge
        .process_all(&mut package, &build::group(0, &[object]), &mut world)
        .unwrap();

    let stance = if stance_type == 1 { 3 } else { 1 };
    bridge
        .process_all(&mut package, &build::stance(stance), &mut world)
        .unwrap();
    assert_eq!(world.sim.units.stance()[row], stance as i8);
    assert_eq!(world.sim.units.get_flags(row) & 0x10, 0x10);

    bridge
        .process_all(
            &mut package,
            &fixed_i32_command(32, &[0x20, 0x55AA]),
            &mut world,
        )
        .unwrap();
    assert_eq!(world.sim.units.get_unit_masks(row) & 0x20, 0x20);

    bridge
        .process_all(&mut package, &fixed_i32_command(14, &[1]), &mut world)
        .unwrap();
    assert_eq!(world.sim.units.get_unit_masks(row) & 0x800000, 0x800000);
}

#[test]
fn unitmask_100_commits_toggle_dirty_flag_and_order_retirement_atomically() {
    let Some(rules) = rules() else { return };
    let cfg = EnvConfig::default();
    let type_index = ordinary_type(&rules);
    let mut world = EnvWorld::new(rules, 4, 0x100, cfg.grid_w, cfg.grid_h);
    let unit = world.spawn(0, type_index, SUBTILE, SUBTILE).unwrap();
    let row = world.sim.row_of(unit).unwrap();
    world
        .install_order(row, OrderRec::move_to(40, 50, 0), QueuePos::New)
        .unwrap();
    world.sim.units.set_unit_masks(row, 0x0400_0000);
    let request = GroupUnitMaskRequest {
        group: transient_group(&world, row, false),
        mask: 0x100,
        set: 0x1234,
    };
    let receipt = world.apply_group_unitmask_transaction(request.clone());
    assert_eq!(receipt.status, GroupStateTransactionStatus::Applied);
    assert!(receipt.validates(&request));
    assert_eq!(world.sim.units.get_unit_masks(row), 0x100);
    assert_eq!(world.sim.units.get_flags(row) & 0x10, 0x10);
    assert!(world.orders[row].is_empty());
}

#[test]
fn set_transport_uses_exact_leader_flag_ladder_for_land_units() {
    let Some(rules) = rules() else { return };
    let cfg = EnvConfig::default();
    let type_index = ordinary_type(&rules);
    let mut world = EnvWorld::new(rules, 4, 0x714, cfg.grid_w, cfg.grid_h);
    let unit = world.spawn(0, type_index, SUBTILE, SUBTILE).unwrap();
    let row = world.sim.row_of(unit).unwrap();
    world.players[0].leader_flags = 0x400;
    let group = transient_group(&world, row, false);
    let set_request = GroupSetTransportRequest {
        group: group.clone(),
        flag: 1,
    };
    let set = world.apply_group_set_transport_transaction(set_request.clone());
    assert_eq!(set.status, GroupStateTransactionStatus::Applied);
    assert!(set.validates(&set_request));
    assert_eq!(world.sim.units.get_unit_masks(row) & 0x800000, 0x800000);

    let clear_request = GroupSetTransportRequest { group, flag: 0 };
    let clear = world.apply_group_set_transport_transaction(clear_request.clone());
    assert_eq!(clear.status, GroupStateTransactionStatus::Applied);
    assert!(clear.validates(&clear_request));
    assert_eq!(world.sim.units.get_unit_masks(row) & 0x800000, 0);
}

#[test]
fn buildmask_is_exactly_gated_and_reached_build_state_remains_unavailable() {
    let Some(rules) = rules() else { return };
    let cfg = EnvConfig::default();
    let ordinary = ordinary_type(&rules);
    let building_type = (g::BUILD_TYPE_BASE..g::BUILD_TYPE_BASE + g::NUM_BUILDTYPES)
        .map(|value| value as u16)
        .find(|&type_index| rules.caps.get(type_index).has(F_BUILDING))
        .unwrap();
    let mut world = EnvWorld::new(rules, 4, 0xB17, cfg.grid_w, cfg.grid_h);
    let unit = world.spawn(0, ordinary, SUBTILE, SUBTILE).unwrap();
    let building = world.spawn(0, building_type, 2 * SUBTILE, SUBTILE).unwrap();
    let unit_row = world.sim.row_of(unit).unwrap();
    let building_row = world.sim.row_of(building).unwrap();

    let gated_request = GroupBuildMaskRequest {
        group: transient_group(&world, unit_row, false),
        mask: 0x40,
        set: 0,
    };
    let gated = world.apply_group_buildmask_transaction(gated_request.clone());
    assert_eq!(gated.status, GroupStateTransactionStatus::Applied);
    assert!(gated.validates(&gated_request));

    let reached_request = GroupBuildMaskRequest {
        group: transient_group(&world, building_row, true),
        mask: 0x40,
        set: 0,
    };
    let before = world.clone();
    let reached = world.apply_group_buildmask_transaction(reached_request.clone());
    assert_eq!(reached.status, GroupStateTransactionStatus::Unavailable);
    assert!(reached.validates(&reached_request));
    assert_eq!(world.sim.digest(), before.sim.digest());
}

#[test]
fn unsupported_type_zero_stance_is_unavailable_without_mutation() {
    let Some(rules) = rules() else { return };
    let type_zero = (g::UNIT_TYPE_BASE..g::GAIA_TYPE_BASE)
        .map(|value| value as u16)
        .find(|&type_index| stance_type(&rules, type_index) == Some(0));
    let Some(type_zero) = type_zero else { return };
    let cfg = EnvConfig::default();
    let mut world = EnvWorld::new(rules, 4, 0x570, cfg.grid_w, cfg.grid_h);
    let unit = world.spawn(0, type_zero, SUBTILE, SUBTILE).unwrap();
    let row = world.sim.row_of(unit).unwrap();
    let request = GroupStanceRequest {
        group: transient_group(&world, row, false),
        stance: 1,
    };
    let before = world.clone();
    let receipt = world.apply_group_stance_transaction(request.clone());
    assert_eq!(receipt.status, GroupStateTransactionStatus::Unavailable);
    assert!(receipt.validates(&request));
    assert_eq!(world.sim.digest(), before.sim.digest());
    assert_eq!(world.stance, before.stance);
}
