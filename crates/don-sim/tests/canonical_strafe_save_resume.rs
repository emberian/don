use don_sim::order::Order;
use don_sim::systems::air::AirOrderWalk;
use don_sim::systems::canonical_strafe_runtime::{
    StrafeFireMode, StrafeRuntimeAuthority, StrafeSearchObservation, StrafeTypeFacts,
};
use don_sim::systems::patrol::StrafeOrder;
use don_sim::systems::save_load;
use don_sim::systems::strafe_order_frontier::{AirTargetSearchKind, ObjectIdentity};
use don_sim::tick::Sim;

fn install_authority(sim: &mut Sim, actor: ObjectIdentity) {
    let mut authority = StrafeRuntimeAuthority {
        revision: 0x5354_5241_4645,
        ..Default::default()
    };
    authority.insert_type(
        77,
        StrafeTypeFacts {
            animal: false,
            missile: false,
            helicopter: false,
            bomber: false,
            strafes: false,
            speed: 16,
            min_range: 0,
            max_range: 0x400,
            fire_mode: StrafeFireMode::FireAmmo,
            attack_call_al: 2,
            bomber_spell_delta: None,
        },
    );
    // Actor o=0/frame=0 reaches the exact mod-16 scan. A revision-bound observed miss keeps
    // the current target, and the live target lookup below remains canonical World state.
    authority.searches.push(StrafeSearchObservation {
        actor,
        frame: 0,
        kind: AirTargetSearchKind::AirFirst,
        result: None,
    });
    sim.replace_strafe_runtime_authority(authority);
}

fn fixture() -> (Sim, usize, ObjectIdentity) {
    let mut sim = Sim::new(0x1234_5678, 4);
    let actor_handle = sim.spawn_unit(0, 77, 1000, 1000, 4).unwrap();
    let target_handle = sim.spawn_unit(1, 88, 1000, 900, 4).unwrap();
    let actor_row = sim.world.row_of(actor_handle).unwrap();
    let target_row = sim.world.row_of(target_handle).unwrap();
    let actor = ObjectIdentity {
        o: i32::from(sim.world.units.o()[actor_row]),
        who: i32::from(sim.world.units.get_who(actor_row)),
        uid: sim.world.units.get_uid(actor_row),
    };
    let target = ObjectIdentity {
        o: i32::from(sim.world.units.o()[target_row]),
        who: i32::from(sim.world.units.get_who(target_row)),
        uid: sim.world.units.get_uid(target_row),
    };
    let order = StrafeOrder {
        target_o: target.o,
        target_who: target.who,
        target_uid: target.uid,
        def_x: 1000,
        def_y: 900,
        air: AirOrderWalk {
            oxx: -1,
            whose: -1,
            cruising_alt: 0x640,
            ..Default::default()
        },
        xx: 1000,
        yy: 900,
        ..Default::default()
    };
    sim.world
        .orders_mut(actor_row)
        .replace(Order::strafe(order, false).unwrap());
    install_authority(&mut sim, actor);
    (sim, actor_row, actor)
}

fn assert_same_strafe_frame(control: &Sim, resumed: &Sim, row: usize) {
    assert_eq!(control.world.frame, resumed.world.frame);
    assert_eq!(control.world.random.state(), resumed.world.random.state());
    assert_eq!(control.world.orders(row), resumed.world.orders(row));
    assert_eq!(control.paths[row], resumed.paths[row]);
    assert_eq!(
        control.world.units.x_internal()[row],
        resumed.world.units.x_internal()[row]
    );
    assert_eq!(
        control.world.units.y_internal()[row],
        resumed.world.units.y_internal()[row]
    );
    assert_eq!(
        control.world.units.angle()[row],
        resumed.world.units.angle()[row]
    );
    assert_eq!(
        control.world.units.get_recharging(row),
        resumed.world.units.get_recharging(row)
    );
    assert_eq!(
        control.world.units.get_idle(row),
        resumed.world.units.get_idle(row)
    );
}

#[test]
fn v13_tag8_reload_resumes_the_same_effect_and_rng_tick() {
    let (mut control, actor_row, actor) = fixture();
    let saved = save_load::save_sim(&control).unwrap();
    let mut resumed = save_load::load_sim(&saved).unwrap();
    install_authority(&mut resumed, actor);
    control.shooter_rules.push((77, Default::default()));
    resumed.shooter_rules.push((77, Default::default()));

    let before_rng = control.world.random.state();
    let before_y = control.world.units.y_internal()[actor_row];
    control.do_frame();
    resumed.do_frame();

    assert_same_strafe_frame(&control, &resumed, actor_row);
    assert_ne!(
        control.world.random.state(),
        before_rng,
        "air physics drew once"
    );
    assert_eq!(control.world.units.y_internal()[actor_row], before_y - 16);
    assert_eq!(control.world.units.get_recharging(actor_row), 3);
    assert_eq!(control.world.units.get_idle(actor_row), 1);
    assert_eq!(control.ammo.ammo_index, 1);
    assert_eq!(control.ammo.ammo_index, resumed.ammo.ammo_index);
    assert_eq!(control.ammo.slots, resumed.ammo.slots);
    assert!(control.ammo.slots[0].occupied());
    assert!(control.last_strafe_error.is_none());
    let receipt = control.last_strafe_receipt.as_ref().unwrap();
    assert_eq!(
        receipt.transaction.rng_epoch_after - receipt.transaction.rng_epoch_before,
        3
    );
    let mut expected_after_physics = don_sim::rng::Random::new(receipt.rng_state_before);
    expected_after_physics.get(0, 0xffff);
    assert_eq!(
        receipt.rng_state_after_physics,
        expected_after_physics.state()
    );

    let typed = control.world.orders(actor_row).current().unwrap();
    let payload = typed
        .strafe
        .as_ref()
        .expect("tag-8 payload remains canonical");
    assert_eq!((payload.target_o, payload.target_who), (0, 1));
    assert_eq!((payload.xx, payload.yy), (1000, 900));
    assert!(
        (1300..=1900).contains(&payload.air.cruising_alt) && payload.air.cruising_alt % 100 == 0,
        "mod-8 cruise draw stored a retail altitude"
    );
}

#[test]
fn malformed_tag8_header_or_foreign_payload_fails_before_save_mutation() {
    let (mut sim, actor_row, _) = fixture();
    let before_rng = sim.world.random.state();
    let before_order = sim.world.orders(actor_row).current().unwrap().clone();
    let malformed = sim.world.orders_mut(actor_row).current_mut().unwrap();
    malformed.target_uid = malformed.target_uid.wrapping_add(1);

    assert_eq!(
        save_load::save_sim(&sim),
        Err(save_load::SaveError::Invalid("invalid STRAFE payload"))
    );
    assert_eq!(sim.world.random.state(), before_rng);
    assert_ne!(sim.world.orders(actor_row).current(), Some(&before_order));
}
