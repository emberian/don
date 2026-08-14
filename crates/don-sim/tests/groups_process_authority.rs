//! Mutation-sensitive gates for the detached fixed-pool `Groups::process` transaction.

use don_sim::systems::canonical_group_move_host::{
    GroupMoveAuthority, MoveMemberAuthority, PackageError,
};
use don_sim::systems::groups_guys::{FormationMember, Groups};
use don_sim::systems::groups_process_authority::{
    commit_groups_process, prepare_groups_process, GroupsProcessAuthorityError,
    GroupsProcessAuthoritySource,
};
use don_sim::tick::Sim;
use don_sim::world::{Handle, OBJ_FLAG_ACTIVE};

fn movement_member(handle: Handle, role: i32, category: i32, speed: i32) -> MoveMemberAuthority {
    MoveMemberAuthority {
        handle,
        role,
        on_map: true,
        is_captain: true,
        can_move: true,
        can_install_order: true,
        is_plane: false,
        domain: 0,
        unit_flags: 6_273,
        speed,
        admits_unsplit_move_near: true,
        land_formation: FormationMember {
            category,
            x_spacing: 144,
            y_spacing: 144,
            formation_size: 1,
            guy_spacing: 144,
            modern_infantry: false,
            width: 50,
            angle: 0,
        },
        water_formation: FormationMember {
            category: category + 5,
            ..FormationMember::default()
        },
    }
}

fn fixture() -> (Sim, GroupMoveAuthority, Vec<Handle>) {
    let mut sim = Sim::new(0x9137, 100);
    sim.activate(0);
    sim.groups.proc_group = 0;
    let handles: Vec<_> = (0..4)
        .map(|index| {
            sim.spawn_unit(0, 50, 10_000 + index * 192, 10_000, 1)
                .unwrap()
        })
        .collect();
    let group = &mut sim.groups.list[Groups::index(0, 0)];
    group.who = 0;
    group.num = 4;
    group.role = -1;
    group.speed = 999;
    group.new_speed = 998;
    for (index, &handle) in handles.iter().enumerate() {
        let row = sim.world.row_of(handle).unwrap();
        let o = sim.world.units.o()[row];
        sim.world.units.group_mut()[row] = 0;
        group.list[index] = o;
        group.angles[index] = 10 + index as i8;
        group.off_x[index] = 20 + index as i32;
        group.off_y[index] = 30 + index as i32;
        group.curr_x[index] = 40 + index as i32;
        group.curr_y[index] = 50 + index as i32;
    }
    let authority = GroupMoveAuthority {
        revision: 19,
        composition_digest: [0x5a; 32],
        destination_is_water: false,
        force_formation_facing_zero: false,
        members: handles
            .iter()
            .enumerate()
            .map(|(index, &handle)| {
                movement_member(
                    handle,
                    1 << (index + 8),
                    [4, 1, 3, 2][index],
                    25 + index as i32,
                )
            })
            .collect(),
    };
    (sim, authority, handles)
}

#[test]
fn live_citizen_group_keeps_members_and_recomputes_role_and_dynamic_leader_speed() {
    let (mut sim, authority, _) = fixture();
    let active = std::array::from_fn(|who| sim.leaders[who].active);
    let prepared = prepare_groups_process(&sim.world, &sim.groups, &active, &authority).unwrap();
    let receipt = prepared.receipt().clone();

    let group = &prepared.after().list[0];
    assert_eq!(group.num, 4);
    assert_eq!(group.role, 0x0f00);
    assert_eq!(group.speed, 26, "lowest formation category is member one");
    assert_eq!(group.new_speed, 26);
    assert_eq!(prepared.after().proc_group, 1);
    assert_eq!(receipt.slot, 0);
    assert_eq!(receipt.groups_processed, 1);
    assert_eq!(receipt.members_removed, 0);
    assert_eq!(receipt.authority_revision, 19);
    assert_eq!(
        receipt.source,
        GroupsProcessAuthoritySource::ExecutableFixedPoolAndHandleBoundUnitAuthority
    );

    let committed = commit_groups_process(&mut sim.groups, prepared).unwrap();
    assert_eq!(committed, receipt);
    assert_eq!(sim.groups.list[0].speed, 26);
}

#[test]
fn dead_and_wrong_backlink_members_compact_every_parallel_plane_before_recompute() {
    let (mut sim, authority, handles) = fixture();
    let dead_row = sim.world.row_of(handles[1]).unwrap();
    sim.world.units.set_flags(dead_row, 0);
    let wrong_group_row = sim.world.row_of(handles[2]).unwrap();
    sim.world.units.group_mut()[wrong_group_row] = -1;
    let active = std::array::from_fn(|who| sim.leaders[who].active);

    let prepared = prepare_groups_process(&sim.world, &sim.groups, &active, &authority).unwrap();
    let group = &prepared.after().list[0];
    assert_eq!(group.num, 2);
    assert_eq!(group.list[..2], [0, 3]);
    assert_eq!(group.angles[..2], [10, 13]);
    assert_eq!(group.off_x[..2], [20, 23]);
    assert_eq!(group.off_y[..2], [30, 33]);
    assert_eq!(group.curr_x[..2], [40, 43]);
    assert_eq!(group.curr_y[..2], [50, 53]);
    assert_eq!(group.role, 0x0900);
    assert_eq!(group.speed, 28, "member three now has the lowest category");
    assert_eq!(prepared.receipt().members_removed, 2);
}

#[test]
fn build_vtable_member_is_removed_without_inventing_unit_authority() {
    let (sim, _, _) = fixture();
    let mut groups = sim.groups.clone();
    let group = &mut groups.list[0];
    group.num = 1;
    group.list[0] = 2_000;
    group.buildings = 1;
    group.role = 0x1234;
    group.speed = 55;
    group.new_speed = 54;
    let active = std::array::from_fn(|who| sim.leaders[who].active);

    let prepared =
        prepare_groups_process(&sim.world, &groups, &active, &GroupMoveAuthority::default())
            .unwrap();
    let group = &prepared.after().list[0];
    assert_eq!(group.num, 0);
    assert_eq!(group.role, 0);
    assert_eq!(group.speed, 0);
    assert_eq!(group.new_speed, 0);
    assert_eq!(prepared.receipt().members_removed, 1);
}

#[test]
fn building_flag_takes_find_role_and_compute_speed_zero_arms() {
    let (sim, _, _) = fixture();
    let mut groups = sim.groups.clone();
    groups.list[0].buildings = 1;
    let active = std::array::from_fn(|who| sim.leaders[who].active);

    let prepared =
        prepare_groups_process(&sim.world, &groups, &active, &GroupMoveAuthority::default())
            .unwrap();
    let group = &prepared.after().list[0];
    assert_eq!(
        group.num, 4,
        "Unit vtable predicates still retain the members"
    );
    assert_eq!(
        group.role, 0,
        "find_role returns after zeroing building Groups"
    );
    assert_eq!(group.speed, 0);
    assert_eq!(group.new_speed, 0);
}

#[test]
fn missing_current_authority_and_wall_members_fail_without_mutation() {
    let (sim, mut authority, handles) = fixture();
    let active = std::array::from_fn(|who| sim.leaders[who].active);
    authority
        .members
        .retain(|member| member.handle != handles[1]);
    assert_eq!(
        prepare_groups_process(&sim.world, &sim.groups, &active, &authority).unwrap_err(),
        GroupsProcessAuthorityError::UnitAuthority(PackageError::MissingAuthority {
            handle: handles[1]
        })
    );

    let mut wall = sim.groups.clone();
    wall.list[0].num = 1;
    wall.list[0].list[0] = 3_000;
    assert_eq!(
        prepare_groups_process(&sim.world, &wall, &active, &authority).unwrap_err(),
        GroupsProcessAuthorityError::UnsupportedWallMember { who: 0, o: 3_000 }
    );
    assert_eq!(
        sim.world.units.get_flags(0) & OBJ_FLAG_ACTIVE,
        OBJ_FLAG_ACTIVE
    );
}

#[test]
fn commit_rejects_any_fixed_pool_change_after_preparation() {
    let (mut sim, authority, _) = fixture();
    let active = std::array::from_fn(|who| sim.leaders[who].active);
    let prepared = prepare_groups_process(&sim.world, &sim.groups, &active, &authority).unwrap();
    let before = sim.groups.clone();
    sim.groups.list[1].stamp = 1;
    assert_eq!(
        commit_groups_process(&mut sim.groups, prepared),
        Err(GroupsProcessAuthorityError::StaleGroups)
    );
    assert_eq!(sim.groups.list[0], before.list[0]);
    assert_eq!(sim.groups.list[1].stamp, 1);
}
