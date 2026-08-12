//! Exact retail `[Group][GUARD]` witness through canonical selection, tag-5 save and one frame.

use don_sim::order::OrderIndex;
use don_sim::systems::canonical_group_move_host::{GroupMoveAuthority, MoveMemberAuthority};
use don_sim::systems::canonical_guard_runtime::{CanonicalGuardAuthority, CanonicalGuardBinding};
use don_sim::systems::groups_guys::FormationMember;
use don_sim::systems::save_load::{load_sim, save_sim};
use don_sim::tick::lifecycle_host::PlayerTable;
use don_sim::tick::Sim;
use don_sim::Handle;

// playback___2014.08.08_22_23_01__fri_.rcx, package 1284 / turn 1285 / play 1 /
// frame 75135. Group owner 3 selects Unit 110; GUARD targets Unit 106, QueuePos::New.
const RETAIL_GUARD: &[u8] = &[
    0x00, 0x01, 0x03, 0x6e, 0x00, 0x1f, 0x6a, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x02, 0x00,
    0x00, 0x00,
];
const CACHED_GUARD: &[u8] = &[
    0x00, 0x00, 0x03, 0x1f, 0x6a, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00,
];

fn selection_authority(actor: Handle) -> GroupMoveAuthority {
    GroupMoveAuthority {
        revision: 0x6775_6172_64,
        composition_digest: [0x31; 32],
        destination_is_water: false,
        force_formation_facing_zero: false,
        members: vec![MoveMemberAuthority {
            handle: actor,
            role: 0,
            on_map: true,
            is_captain: true,
            can_move: true,
            can_install_order: true,
            is_plane: false,
            domain: 0,
            unit_flags: 0,
            speed: 16,
            admits_unsplit_move_near: true,
            land_formation: FormationMember::default(),
            water_formation: FormationMember::default(),
        }],
    }
}

fn guard_authority(actor: Handle, target: Handle) -> CanonicalGuardAuthority {
    CanonicalGuardAuthority {
        revision: 0x6775_6172_642d_3331,
        composition_digest: [0x5a; 32],
        bindings: vec![CanonicalGuardBinding {
            actor,
            target,
            dx: 0,
            dy: 0,
            target_is_on_map: true,
            target_is_valid_unit: true,
            target_is_moving: false,
            receiver_external_effects_empty: true,
        }],
    }
}

fn install(sim: &mut Sim, actor: Handle, target: Handle) {
    sim.replace_group_move_authority(selection_authority(actor));
    sim.replace_guard_authority(guard_authority(actor, target));
}

fn fixture() -> (Sim, Handle, Handle) {
    let mut sim = Sim::new(0x6775_6172, 128);
    let mut players = PlayerTable::new();
    players.seat(1, 1, 3, 0);
    sim.players = Some(players);
    let mut handles = Vec::new();
    for o in 0..=110 {
        handles.push(
            sim.spawn_unit(3, 77, 4_800 + o * 12, 9_600, 4)
                .expect("retail-addressed Unit row"),
        );
    }
    let target = handles[106];
    let actor = handles[110];
    let actor_row = sim.world.row_of(actor).unwrap();
    sim.world.units.group_mut()[actor_row] = -1;
    sim.world.units.o_down_mut()[actor_row] = -1;
    install(&mut sim, actor, target);
    sim.world.frame = 75_135;
    sim.vic_match.frame = 75_135;
    (sim, actor, target)
}

fn guard_idle(sim: &Sim, actor: Handle) -> i32 {
    let row = sim.world.row_of(actor).unwrap();
    let order = sim.world.orders(row).current().unwrap();
    assert_eq!(order.kind, OrderIndex::Guard);
    order.guard.unwrap().idle
}

#[test]
fn retail_guard_packet_cache_tag5_save_and_periodic_frame_resume_identically() {
    let (mut direct, actor, target) = fixture();
    let receipt = direct
        .process_guard_group_package(1, 1_285, RETAIL_GUARD)
        .unwrap();
    assert_eq!(receipt.frame, 75_135);
    assert_eq!((receipt.selected.who, receipt.selected.o), (3, 110));
    assert_eq!((receipt.target.who, receipt.target.o), (3, 106));
    assert_eq!(receipt.random_state_before, receipt.random_state_after);
    assert_eq!(guard_idle(&direct, actor), 0);

    let first_save = save_sim(&direct).unwrap();
    let mut cached = load_sim(&first_save).unwrap();
    assert_eq!(save_sim(&cached).unwrap(), first_save);
    install(&mut cached, actor, target);
    let cached_receipt = cached
        .process_guard_group_package(1, 1_286, CACHED_GUARD)
        .unwrap();
    assert_eq!(cached_receipt.selected, receipt.selected);
    assert_eq!(cached_receipt.target, receipt.target);

    let second_save = save_sim(&cached).unwrap();
    let mut resumed = load_sim(&second_save).unwrap();
    assert_eq!(save_sim(&resumed).unwrap(), second_save);
    install(&mut resumed, actor, target);
    let mut uninterrupted = load_sim(&second_save).unwrap();
    install(&mut uninterrupted, actor, target);

    // The exact executor phase is signed `(actor.o + frame) % 16 == 0`. The two branches
    // begin from the same saved state at the first such frame after the recorded package.
    uninterrupted.world.frame = 75_138;
    uninterrupted.vic_match.frame = 75_138;
    resumed.world.frame = 75_138;
    resumed.vic_match.frame = 75_138;
    uninterrupted.do_frame();
    resumed.do_frame();
    assert_eq!(guard_idle(&uninterrupted, actor), 1);
    assert_eq!(guard_idle(&resumed, actor), 1);
    assert_eq!(
        uninterrupted
            .world
            .orders(uninterrupted.world.row_of(actor).unwrap()),
        resumed.world.orders(resumed.world.row_of(actor).unwrap())
    );
    // Other scheduled retail systems consume the frame RNG; the GUARD path itself consumes
    // none, and the uninterrupted/resumed complete-frame streams remain identical.
    assert_eq!(
        uninterrupted.world.random.state(),
        resumed.world.random.state()
    );
}

#[test]
fn queue_last_and_multi_member_guard_stay_outside_the_bounded_host() {
    let (mut sim, _actor, _target) = fixture();
    let mut queue_last = RETAIL_GUARD.to_vec();
    queue_last[14..18].copy_from_slice(&1i32.to_le_bytes());
    assert!(sim
        .process_guard_group_package(1, 1_285, &queue_last)
        .is_err());
    assert!(sim
        .process_guard_group_package(
            1,
            1_285,
            &[0, 2, 3, 110, 0, 109, 0, 31, 106, 0, 0, 0, 3, 0, 0, 0, 2, 0, 0, 0]
        )
        .is_err());
}

#[test]
fn tag5_kind_and_duplicated_target_mismatches_refuse_to_save() {
    let (mut sim, actor, _target) = fixture();
    sim.process_guard_group_package(1, 1_285, RETAIL_GUARD)
        .unwrap();
    let row = sim.world.row_of(actor).unwrap();
    let canonical = sim.world.orders(row).current().unwrap().clone();

    sim.world.orders_mut(row).current_mut().unwrap().target_uid ^= 1;
    assert!(save_sim(&sim).is_err());

    *sim.world.orders_mut(row).current_mut().unwrap() = canonical;
    sim.world.orders_mut(row).current_mut().unwrap().kind = OrderIndex::Think;
    assert!(save_sim(&sim).is_err());
}
