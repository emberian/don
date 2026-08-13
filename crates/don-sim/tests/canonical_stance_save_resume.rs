//! Retail `[Group][STANCE]` through fixed Groups, core save, and saved selection-cache reuse.

use don_sim::systems::canonical_group_move_host::{
    groups_equal, GroupMoveAuthority, MoveMemberAuthority,
};
use don_sim::systems::canonical_stance_runtime::{
    prepare_stance_package, CanonicalStanceAuthority, CanonicalStanceBinding, CanonicalStanceError,
};
use don_sim::systems::groups_guys::FormationMember;
use don_sim::systems::save_load::{load_sim, save_sim};
use don_sim::tick::lifecycle_host::PlayerTable;
use don_sim::tick::Sim;
use don_sim::Handle;

// Playback___2024.03.18_18_18_49__Mon_.rcx, package 2818 / turn 2819 / play 0 /
// frame 11281: owner 1 explicitly selects five Units and requests the next stance option.
const RETAIL_EXPLICIT_STANCE: &[u8] = &[
    0x00, 0x05, 0x01, 0x1b, 0x00, 0x20, 0x00, 0x27, 0x00, 0x2a, 0x00, 0x2d, 0x00, 0x02, 0xff, 0xff,
    0xff, 0xff,
];
const MEMBERS: [i16; 5] = [27, 32, 39, 42, 45];

// The same replay, package 2820 / turn 2821 / play 0 / frame 11289. The empty Group reuses
// the preceding explicit `(o,uid)` selection cache and requests the next option again.
const RETAIL_CACHED_STANCE: &[u8] = &[0x00, 0x00, 0x01, 0x02, 0xff, 0xff, 0xff, 0xff];

fn selection_authority(actors: &[Handle], plane: bool) -> GroupMoveAuthority {
    GroupMoveAuthority {
        revision: 0x7374_616e_6365_02,
        composition_digest: [0x52; 32],
        destination_is_water: false,
        force_formation_facing_zero: false,
        members: actors
            .iter()
            .enumerate()
            .map(|(index, &handle)| MoveMemberAuthority {
                handle,
                role: 0,
                on_map: true,
                is_captain: index == 0,
                can_move: true,
                can_install_order: true,
                is_plane: plane,
                domain: 0,
                unit_flags: 0,
                speed: 12,
                admits_unsplit_move_near: true,
                land_formation: FormationMember {
                    category: index as i32,
                    ..FormationMember::default()
                },
                water_formation: FormationMember::default(),
            })
            .collect(),
    }
}

fn stance_authority(actors: &[Handle], stance_type: i32) -> CanonicalStanceAuthority {
    CanonicalStanceAuthority {
        revision: 0x7374_616e_6365_0201,
        composition_digest: [0x02; 32],
        bindings: actors
            .iter()
            .map(|&actor| CanonicalStanceBinding {
                actor,
                object_stance_type: stance_type,
                unit_stance_type: stance_type,
            })
            .collect(),
    }
}

fn install(sim: &mut Sim, actors: &[Handle], stance_type: i32) {
    sim.replace_group_move_authority(selection_authority(actors, false));
    sim.replace_stance_authority(stance_authority(actors, stance_type));
}

fn fixture() -> (Sim, Vec<Handle>) {
    let mut sim = Sim::new(0x7374_616e_6365, 100);
    let mut players = PlayerTable::new();
    players.seat(0, 1, 1, 0);
    sim.players = Some(players);
    let mut all = Vec::new();
    for o in 0..=45 {
        all.push(
            sim.spawn_unit(1, 77, 10_000 + o * 4, 20_000, 4)
                .expect("retail-addressed Unit row"),
        );
    }
    let actors = MEMBERS.iter().map(|&o| all[o as usize]).collect::<Vec<_>>();
    for &actor in &actors {
        let row = sim.world.row_of(actor).unwrap();
        sim.world.units.group_mut()[row] = -1;
        sim.world.units.o_down_mut()[row] = -1;
        sim.world.units.stance_mut()[row] = 0;
    }
    install(&mut sim, &actors, 1);
    sim.world.frame = 11_281;
    sim.vic_match.frame = 11_281;
    (sim, actors)
}

fn assert_stance(sim: &Sim, actors: &[Handle], expected: i8) {
    for &actor in actors {
        let row = sim.world.row_of(actor).unwrap();
        assert_eq!(sim.world.units.stance()[row], expected);
        assert_ne!(sim.world.units.get_flags(row) & 0x10, 0);
    }
}

#[test]
fn exact_explicit_then_cached_retail_packets_survive_core_save_and_resume() {
    let (mut direct, actors) = fixture();
    let explicit = direct
        .process_stance_group_package(0, 2_819, RETAIL_EXPLICIT_STANCE)
        .unwrap();
    assert_eq!(explicit.frame, 11_281);
    assert_eq!(explicit.stance_type, 1);
    assert_eq!(explicit.current_option, 0);
    assert_eq!(explicit.resolved_stance, 1);
    assert_eq!(
        explicit
            .selected
            .iter()
            .map(|identity| identity.o)
            .collect::<Vec<_>>(),
        MEMBERS
    );
    assert_eq!(explicit.random_state_before, explicit.random_state_after);
    assert_stance(&direct, &actors, 1);

    let bytes = save_sim(&direct).unwrap();
    let mut resumed = load_sim(&bytes).unwrap();
    assert_eq!(save_sim(&resumed).unwrap(), bytes);
    install(&mut resumed, &actors, 1);

    // The real recording contains one intervening turn. This bounded witness advances only the
    // package clock, preserving the exact cache-bearing state whose next command is under test.
    direct.world.frame = 11_289;
    direct.vic_match.frame = 11_289;
    resumed.world.frame = 11_289;
    resumed.vic_match.frame = 11_289;
    let direct_receipt = direct
        .process_stance_group_package(0, 2_821, RETAIL_CACHED_STANCE)
        .unwrap();
    let resumed_receipt = resumed
        .process_stance_group_package(0, 2_821, RETAIL_CACHED_STANCE)
        .unwrap();
    assert_eq!(direct_receipt, resumed_receipt);
    assert!(resumed_receipt.selected.iter().map(|id| id.o).eq(MEMBERS));
    assert_eq!(resumed_receipt.current_option, 1);
    assert_eq!(resumed_receipt.resolved_stance, 2);
    assert_stance(&direct, &actors, 2);
    assert_stance(&resumed, &actors, 2);
    assert_eq!(save_sim(&direct).unwrap(), save_sim(&resumed).unwrap());
}

#[test]
fn reverse_cycle_covers_every_admitted_nonzero_stance_type() {
    let mut reverse_wire = RETAIL_EXPLICIT_STANCE.to_vec();
    reverse_wire[14..18].copy_from_slice(&(-2i32).to_le_bytes());
    for stance_type in 1..=3 {
        let (mut sim, actors) = fixture();
        sim.replace_stance_authority(stance_authority(&actors, stance_type));
        let receipt = sim
            .process_stance_group_package(0, 2_819, &reverse_wire)
            .unwrap();
        let expected = if stance_type == 1 { 3 } else { 1 };
        assert_eq!(receipt.stance_type, stance_type);
        assert_eq!(receipt.current_option, 0);
        assert_eq!(receipt.resolved_stance, expected);
        assert_stance(&sim, &actors, expected as i8);
    }
}

#[test]
fn type_zero_aircraft_and_non_corpus_requests_remain_explicit_boundaries() {
    let (mut type_zero, actors) = fixture();
    type_zero.replace_stance_authority(stance_authority(&actors, 0));
    assert!(matches!(
        type_zero.process_stance_group_package(0, 2_819, RETAIL_EXPLICIT_STANCE),
        Err(CanonicalStanceError::UnsupportedCone(_))
    ));
    assert_stance_unset(&type_zero, &actors);

    let (mut air, air_actors) = fixture();
    air.replace_group_move_authority(selection_authority(&air_actors, true));
    assert!(matches!(
        air.process_stance_group_package(0, 2_819, RETAIL_EXPLICIT_STANCE),
        Err(CanonicalStanceError::UnsupportedCone(_))
    ));
    assert_stance_unset(&air, &air_actors);

    let (mut positive, positive_actors) = fixture();
    let mut positive_wire = RETAIL_EXPLICIT_STANCE.to_vec();
    positive_wire[14..18].copy_from_slice(&1i32.to_le_bytes());
    assert!(matches!(
        positive.process_stance_group_package(0, 2_819, &positive_wire),
        Err(CanonicalStanceError::UnsupportedCone(_))
    ));
    assert_stance_unset(&positive, &positive_actors);
}

fn assert_stance_unset(sim: &Sim, actors: &[Handle]) {
    for &actor in actors {
        let row = sim.world.row_of(actor).unwrap();
        assert_eq!(sim.world.units.stance()[row], 0);
        assert_eq!(sim.world.units.get_flags(row) & 0x10, 0);
        assert_eq!(sim.world.units.group()[row], -1);
    }
}

#[test]
fn detached_prepare_rejects_stale_stance_without_partial_publication() {
    let (mut sim, actors) = fixture();
    let player_who = [Some(1), None, None, None, None, None, None, None];
    let prepared = prepare_stance_package(
        &sim.world,
        &sim.groups,
        &sim.paths,
        &sim.command_package_state,
        &sim.group_move_authority,
        &sim.stance_authority,
        &player_who,
        sim.world.frame,
        0,
        2_819,
        RETAIL_EXPLICIT_STANCE,
    )
    .unwrap();
    let row = sim.world.row_of(actors[0]).unwrap();
    sim.world.units.stance_mut()[row] = 3;
    let groups_before = sim.groups.clone();
    let command_before = sim.command_package_state.clone();
    let error = don_sim::systems::canonical_stance_runtime::commit_stance_package(
        &mut sim.world,
        &mut sim.groups,
        &mut sim.paths,
        &mut sim.command_package_state,
        &sim.group_move_authority,
        &sim.stance_authority,
        &player_who,
        prepared,
    )
    .unwrap_err();
    assert_eq!(error, CanonicalStanceError::StaleUnitStance(actors[0]));
    assert!(groups_equal(&sim.groups, &groups_before));
    assert_eq!(sim.command_package_state, command_before);
    for &actor in &actors[1..] {
        let other_row = sim.world.row_of(actor).unwrap();
        assert_eq!(sim.world.units.stance()[other_row], 0);
        assert_eq!(sim.world.units.group()[other_row], -1);
    }
}
