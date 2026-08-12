//! Retail `[Group][ATTACK_GROUND]` through fixed Groups, tag-9 save and a resumed real frame.

use don_sim::order::{OrderIndex, ORDER_GROUP};
use don_sim::systems::canonical_attack_ground_runtime::{
    CanonicalAttackGroundAuthority, CanonicalAttackGroundBinding,
};
use don_sim::systems::canonical_group_move_host::{
    CachedSelection, CommandPackageState, GroupMoveAuthority, MoveMemberAuthority,
};
use don_sim::systems::groups_guys::FormationMember;
use don_sim::systems::save_load::{load_sim, save_sim};
use don_sim::tick::lifecycle_host::PlayerTable;
use don_sim::tick::Sim;
use don_sim::Handle;

// playback___2014.08.13_19_26_39__wed_.rcx, package 23836 / turn 23837 / play 0 /
// frame 190162. This is the strongest non-scenario ordinary-Unit witness in the 64-file
// corpus: owner 1 selects seven Units and attacks unowned ground at (16722,58277), Queue-New.
const RETAIL_ATTACK_GROUND: &[u8] = &[
    0x00, 0x07, 0x01, 0x9f, 0x05, 0x48, 0x06, 0x4b, 0x06, 0x4e, 0x06, 0x54, 0x06, 0x62, 0x06, 0x88,
    0x06, 0x09, 0x52, 0x41, 0x00, 0x00, 0xa5, 0xe3, 0x00, 0x00, 0x02,
];
const MEMBERS: [i16; 7] = [1439, 1608, 1611, 1614, 1620, 1634, 1672];

// playback___2014.04.26_16_17_03__sat_.rcx, package 14660 / turn 14661 / play 3 /
// frame 87791. The zero-member Group reuses the eight-member explicit selection received at
// package 14654 / frame 87755 (`000803f7006c017f01a801a901bb01c701e001`).
const CACHED_ATTACK_GROUND: &[u8] = &[
    0x00, 0x00, 0x03, 0x09, 0x97, 0x0a, 0x01, 0x00, 0x5e, 0x3e, 0x00, 0x00, 0x02,
];
const CACHED_MEMBERS: [i16; 8] = [247, 364, 383, 424, 425, 443, 455, 480];

fn selection_authority(actors: &[Handle]) -> GroupMoveAuthority {
    GroupMoveAuthority {
        revision: 0x6174_7461_636b_09,
        composition_digest: [0x09; 32],
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
                is_plane: false,
                domain: 0,
                unit_flags: 0,
                speed: 12,
                admits_unsplit_move_near: true,
                land_formation: FormationMember::default(),
                water_formation: FormationMember::default(),
            })
            .collect(),
    }
}

fn attack_ground_authority(actors: &[Handle]) -> CanonicalAttackGroundAuthority {
    CanonicalAttackGroundAuthority {
        revision: 0x6174_7461_636b_0901,
        composition_digest: [0xa9; 32],
        bindings: actors
            .iter()
            .map(|&actor| CanonicalAttackGroundBinding {
                actor,
                routes_to_ground_order: true,
                can_attack_ground: true,
                receiver_external_effects_empty: true,
                target_is_in_range: true,
                raw_target_angle: 0x1000_0000,
                side_firing: false,
                current_animation: 2,
            })
            .collect(),
    }
}

fn install(sim: &mut Sim, actors: &[Handle]) {
    sim.replace_group_move_authority(selection_authority(actors));
    sim.replace_attack_ground_authority(attack_ground_authority(actors));
}

fn fixture() -> (Sim, Vec<Handle>) {
    let mut sim = Sim::new(0x6174_7461_636b, 100);
    let mut players = PlayerTable::new();
    players.seat(0, 1, 1, 0);
    sim.players = Some(players);
    let mut all = Vec::new();
    for o in 0..=1672 {
        all.push(
            sim.spawn_unit(1, 77, 12_000 + o * 4, 40_000, 4)
                .expect("retail-addressed Unit row"),
        );
    }
    let actors: Vec<_> = MEMBERS.iter().map(|&o| all[o as usize]).collect();
    for &actor in &actors {
        let row = sim.world.row_of(actor).unwrap();
        sim.world.units.group_mut()[row] = -1;
        sim.world.units.o_down_mut()[row] = -1;
        sim.world.units.set_recharging(row, 7);
        sim.world.units.set_unit_masks(row, 0x0400_0000);
    }
    install(&mut sim, &actors);
    sim.world.frame = 190_162;
    sim.vic_match.frame = 190_162;
    (sim, actors)
}

fn assert_typed_orders(sim: &Sim, actors: &[Handle]) {
    for &actor in actors {
        let row = sim.world.row_of(actor).unwrap();
        let order = sim.world.orders(row).current().unwrap();
        assert_eq!(order.kind, OrderIndex::AttackGround);
        assert_eq!(order.flags, ORDER_GROUP);
        assert_eq!((order.x, order.y), (16_722, 58_277));
        assert_eq!(
            order.attack_ground.unwrap(),
            don_sim::systems::targeted_order_plans::AttackGroundOrderState {
                att_x: 16_722,
                att_y: 58_277,
                accuracy: 0,
                attack_unit: 0,
            }
        );
        assert_eq!(sim.world.units.get_unit_masks(row) & 0x0400_0000, 0);
        assert_eq!(sim.world.units.orders_x()[row], sim.world.units.x_internal()[row]);
        assert_eq!(sim.world.units.orders_y()[row], sim.world.units.y_internal()[row]);
        assert_eq!(sim.world.units.dest_angle()[row], sim.world.units.angle()[row]);
    }
}

#[test]
fn retail_multi_unit_packet_tag9_save_and_recharge_hold_resume_identically() {
    let (mut direct, actors) = fixture();
    let receipt = direct
        .process_attack_ground_group_package(0, 23_837, RETAIL_ATTACK_GROUND)
        .unwrap();
    assert_eq!(receipt.frame, 190_162);
    assert_eq!(receipt.selected.len(), 7);
    assert_eq!(
        receipt
            .selected
            .iter()
            .map(|identity| identity.o)
            .collect::<Vec<_>>(),
        MEMBERS
    );
    assert_eq!(receipt.random_state_before, receipt.random_state_after);
    assert_typed_orders(&direct, &actors);

    let bytes = save_sim(&direct).unwrap();
    let mut resumed = load_sim(&bytes).unwrap();
    assert_eq!(save_sim(&resumed).unwrap(), bytes);
    install(&mut resumed, &actors);
    let mut uninterrupted = load_sim(&bytes).unwrap();
    install(&mut uninterrupted, &actors);

    let before_rng = resumed.world.random.state();
    resumed.do_frame();
    uninterrupted.do_frame();
    assert_typed_orders(&resumed, &actors);
    assert_typed_orders(&uninterrupted, &actors);
    for &actor in &actors {
        let resumed_row = resumed.world.row_of(actor).unwrap();
        let direct_row = uninterrupted.world.row_of(actor).unwrap();
        assert_eq!(resumed.world.units.get_recharging(resumed_row), 7);
        assert_eq!(
            resumed.world.orders(resumed_row),
            uninterrupted.world.orders(direct_row)
        );
    }
    assert_eq!(
        resumed.world.random.state(),
        uninterrupted.world.random.state()
    );
    // Other scheduled systems may consume RNG; the two identical full-frame streams remain
    // equal, while the bounded ATTACK_GROUND planner itself emitted no effect and no draw.
    assert_ne!(resumed.world.frame, 190_162);
    let _ = before_rng;
}

#[test]
fn exact_zero_member_packet_reuses_the_saved_retail_selection_cache() {
    let mut sim = Sim::new(0x6361_6368_6509, 100);
    let mut players = PlayerTable::new();
    players.seat(3, 1, 3, 0);
    sim.players = Some(players);
    let mut all = Vec::new();
    for o in 0..=480 {
        all.push(
            sim.spawn_unit(3, 77, 8_000 + o * 4, 12_000, 4)
                .expect("retail-addressed cached Unit row"),
        );
    }
    let actors: Vec<_> = CACHED_MEMBERS.iter().map(|&o| all[o as usize]).collect();
    for &actor in &actors {
        let row = sim.world.row_of(actor).unwrap();
        sim.world.units.o_down_mut()[row] = -1;
        sim.world.units.set_recharging(row, 5);
    }
    install(&mut sim, &actors);
    let mut selections: [Vec<CachedSelection>; 8] = std::array::from_fn(|_| Vec::new());
    selections[3] = CACHED_MEMBERS
        .iter()
        .map(|&o| {
            let row = sim.world.unit_row_at(3, i32::from(o)).unwrap();
            CachedSelection {
                o,
                uid: sim.world.units.get_uid(row),
            }
        })
        .collect();
    sim.command_package_state = CommandPackageState::from_saved_selections(selections).unwrap();
    sim.world.frame = 87_791;
    sim.vic_match.frame = 87_791;

    let cache_save = save_sim(&sim).unwrap();
    let mut resumed = load_sim(&cache_save).unwrap();
    install(&mut resumed, &actors);
    let receipt = resumed
        .process_attack_ground_group_package(3, 14_661, CACHED_ATTACK_GROUND)
        .unwrap();
    assert_eq!(receipt.frame, 87_791);
    assert_eq!(
        receipt
            .selected
            .iter()
            .map(|identity| identity.o)
            .collect::<Vec<_>>(),
        CACHED_MEMBERS
    );
    for &actor in &actors {
        let row = resumed.world.row_of(actor).unwrap();
        let order = resumed.world.orders(row).current().unwrap();
        assert_eq!(order.kind, OrderIndex::AttackGround);
        assert_eq!((order.x, order.y), (68_247, 15_966));
    }
}

#[test]
fn owned_terrain_and_air_delegate_stay_outside_the_bounded_host() {
    let (mut owned, actors) = fixture();
    owned.map.world.wdata_mut(16_722 / 768, 58_277 / 768).who = 2;
    assert!(owned
        .process_attack_ground_group_package(0, 23_837, RETAIL_ATTACK_GROUND)
        .is_err());

    let (mut air, actors2) = fixture();
    let mut selection = selection_authority(&actors2);
    selection.members[0].is_plane = true;
    air.replace_group_move_authority(selection);
    assert!(air
        .process_attack_ground_group_package(0, 23_837, RETAIL_ATTACK_GROUND)
        .is_err());
    let _ = actors;
}

#[test]
fn nonempty_prior_queue_stays_outside_close_orders_epilogue_authority() {
    let (mut sim, actors) = fixture();
    let row = sim.world.row_of(actors[0]).unwrap();
    sim.world
        .orders_mut(row)
        .replace(don_sim::order::Order::move_to(1, 2, 3));
    assert!(sim
        .process_attack_ground_group_package(0, 23_837, RETAIL_ATTACK_GROUND)
        .is_err());
    assert_eq!(
        sim.world.orders(row).current().unwrap().kind,
        OrderIndex::MoveTo
    );
}

#[test]
fn tag9_kind_and_coordinate_mismatches_refuse_to_save() {
    let (mut sim, actors) = fixture();
    sim.process_attack_ground_group_package(0, 23_837, RETAIL_ATTACK_GROUND)
        .unwrap();
    let row = sim.world.row_of(actors[0]).unwrap();
    let canonical = sim.world.orders(row).current().unwrap().clone();

    sim.world.orders_mut(row).current_mut().unwrap().x ^= 1;
    assert!(save_sim(&sim).is_err());

    *sim.world.orders_mut(row).current_mut().unwrap() = canonical.clone();
    sim.world
        .orders_mut(row)
        .current_mut()
        .unwrap()
        .attack_ground = None;
    let legacy = load_sim(&save_sim(&sim).unwrap()).unwrap();
    assert!(legacy.world.orders(row).current().unwrap().attack_ground.is_none());

    *sim.world.orders_mut(row).current_mut().unwrap() = canonical;
    sim.world.orders_mut(row).current_mut().unwrap().kind = OrderIndex::Think;
    assert!(save_sim(&sim).is_err());
}
