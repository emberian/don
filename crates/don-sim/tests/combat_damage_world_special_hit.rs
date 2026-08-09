use don_sim::systems::combat::damage_world::{
    apply_capture_attempt, apply_special_hit, plan_capture_attempt, plan_special_hit,
    BurningCitizenRequest, CaptureAttemptFacts, CaptureAttemptPlan, CaptureAttemptReceipt,
    CaptureAttemptWorld, CaptureCheckReceipt, CaptureCheckRequest, DeathVisual, DeathVisualScan,
    MissingSpecialHitFact, ObjectKey, SpecialHitFacts, SpecialHitMutation, SpecialHitPlan,
    SpecialHitWorld, BASE_CITIZEN_TYPE,
};
use don_sim::systems::combat::{UnitCombatState, UNIT_WALK_BEGIN, UNIT_WALK_LEN};

const ATTACKER: ObjectKey = ObjectKey { who: 1, o: 17 };
const VICTIM: ObjectKey = ObjectKey { who: 3, o: 41 };

#[derive(Default)]
struct Facts {
    city: bool,
    unit: bool,
    unit_masks: u32,
    unit_masks2: u32,
    land_inside: i32,
    x_size: i32,
    tribe_citizen: bool,
}

impl CaptureAttemptFacts for Facts {
    fn victim_is_city(&self, _: ObjectKey) -> Option<bool> {
        Some(self.city)
    }
}

impl SpecialHitFacts for Facts {
    fn attacker_is_flamethrower(&self, _: ObjectKey) -> Option<bool> {
        Some(true)
    }
    fn victim_is_unit(&self, _: ObjectKey) -> Option<bool> {
        Some(self.unit)
    }
    fn victim_unit_masks(&self, _: ObjectKey) -> Option<u32> {
        Some(self.unit_masks)
    }
    fn victim_unit_masks2(&self, _: ObjectKey) -> Option<u32> {
        Some(self.unit_masks2)
    }
    fn victim_can_carry_air(&self, _: ObjectKey) -> Option<bool> {
        Some(false)
    }
    fn victim_land_inside(&self, _: ObjectKey) -> Option<i32> {
        Some(self.land_inside)
    }
    fn victim_type_x_size(&self, _: ObjectKey) -> Option<i32> {
        Some(self.x_size)
    }
    fn death_visual_scan(&self) -> Option<DeathVisualScan> {
        Some(DeathVisualScan {
            records: vec![
                DeathVisual {
                    valid: 1,
                    gpiece: 40,
                },
                DeathVisual {
                    valid: 0,
                    gpiece: 99,
                },
            ],
            gpiece_threshold: 40,
        })
    }
    fn victim_xy(&self, _: ObjectKey) -> Option<(i32, i32)> {
        Some((123, -456))
    }
    fn victim_tribe_can_base_citizen(&self, _: u8) -> Option<bool> {
        Some(self.tribe_citizen)
    }
    fn victim_nation_citizen_type(&self, _: u8) -> Option<i32> {
        Some(912)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Event {
    RemoveEntrench,
    Eject,
    Init(BurningCitizenRequest, i32),
    GoInside(i32),
    ComeOut(i32),
    Close(i32),
}

#[derive(Default)]
struct World {
    events: Vec<Event>,
    init_results: Vec<i32>,
    next_init: usize,
}

impl SpecialHitWorld for World {
    fn remove_entrench(&mut self, _: ObjectKey) {
        self.events.push(Event::RemoveEntrench);
    }
    fn eject_contents(&mut self, _: ObjectKey) {
        self.events.push(Event::Eject);
    }
    fn init_burning_citizen(&mut self, request: BurningCitizenRequest) -> i32 {
        let result = self.init_results[self.next_init];
        self.next_init += 1;
        self.events.push(Event::Init(request, result));
        result
    }
    fn burning_citizen_go_inside(&mut self, spawned_o: i32, _: ObjectKey) {
        self.events.push(Event::GoInside(spawned_o));
    }
    fn burning_citizen_come_out(&mut self, spawned_o: i32, _: u8) {
        self.events.push(Event::ComeOut(spawned_o));
    }
    fn close_burning_citizen(&mut self, spawned_o: i32, _: u8) {
        self.events.push(Event::Close(spawned_o));
    }
}

#[test]
fn public_entrench_transaction_changes_only_retail_walked_mask_bytes() {
    let before_masks = 0xA200_0005;
    let before_masks2 = 0xF00A_1F0F;
    let facts = Facts {
        unit: true,
        unit_masks: before_masks,
        unit_masks2: before_masks2,
        ..Facts::default()
    };
    let plan = plan_special_hit(&facts, ATTACKER, VICTIM).unwrap();
    let mut unit = UnitCombatState {
        unit_masks: before_masks,
        unit_masks2: before_masks2,
        ..UnitCombatState::default()
    };
    let mut before = [0; UNIT_WALK_LEN];
    unit.patch_unit_range(&mut before);
    let mut world = World::default();
    let receipt = apply_special_hit(plan, Some(&mut unit), &mut world).unwrap();
    let mut after = [0; UNIT_WALK_LEN];
    unit.patch_unit_range(&mut after);

    assert_eq!(
        receipt.mutations,
        vec![
            SpecialHitMutation::ClearEntrenchMasks,
            SpecialHitMutation::RemoveEntrenchGraphics,
        ]
    );
    assert_eq!(world.events, vec![Event::RemoveEntrench]);
    assert_eq!(unit.unit_masks, before_masks & !0x0200_0000);
    assert_eq!(
        unit.unit_masks2,
        before_masks2 & !0x0000_1000 & !0x0002_0000
    );
    let first = 0x68 - UNIT_WALK_BEGIN;
    let second = 0x6C - UNIT_WALK_BEGIN;
    for i in 0..UNIT_WALK_LEN {
        if !(first..first + 4).contains(&i) && !(second..second + 4).contains(&i) {
            assert_eq!(after[i], before[i], "walk byte {i:#x}");
        }
    }
}

#[test]
fn public_eject_transaction_uses_signed_bound_and_deferred_close_order() {
    let facts = Facts {
        land_inside: 5,
        x_size: 7,
        tribe_citizen: true,
        ..Facts::default()
    };
    let SpecialHitPlan::EjectBuilding(plan) = plan_special_hit(&facts, ATTACKER, VICTIM).unwrap()
    else {
        panic!("eject plan");
    };
    let spawn = plan.burn_spawns.expect("death ring is below 7 >> 1");
    assert_eq!(
        (spawn.spawn_count, spawn.citizen_type),
        (3, BASE_CITIZEN_TYPE)
    );
    let request = BurningCitizenRequest {
        who: VICTIM.who,
        type_index: BASE_CITIZEN_TYPE,
        x: 123,
        y: -456,
    };
    let mut world = World {
        init_results: vec![12, -1, 7],
        ..World::default()
    };
    apply_special_hit(SpecialHitPlan::EjectBuilding(plan), None, &mut world).unwrap();
    assert_eq!(
        world.events,
        vec![
            Event::Eject,
            Event::Init(request, 12),
            Event::GoInside(12),
            Event::ComeOut(12),
            Event::Init(request, -1),
            Event::Init(request, 7),
            Event::GoInside(7),
            Event::ComeOut(7),
            Event::Close(12),
            Event::Close(7),
        ]
    );
}

#[test]
fn missing_second_mask_is_a_typed_fail_closed_entrench_boundary() {
    struct MissingMasks2;
    impl SpecialHitFacts for MissingMasks2 {
        fn attacker_is_flamethrower(&self, _: ObjectKey) -> Option<bool> {
            Some(true)
        }
        fn victim_is_unit(&self, _: ObjectKey) -> Option<bool> {
            Some(true)
        }
        fn victim_unit_masks(&self, _: ObjectKey) -> Option<u32> {
            Some(0x0200_0000)
        }
        fn victim_unit_masks2(&self, _: ObjectKey) -> Option<u32> {
            None
        }
        fn victim_can_carry_air(&self, _: ObjectKey) -> Option<bool> {
            unreachable!()
        }
        fn victim_land_inside(&self, _: ObjectKey) -> Option<i32> {
            unreachable!()
        }
        fn victim_type_x_size(&self, _: ObjectKey) -> Option<i32> {
            unreachable!()
        }
        fn death_visual_scan(&self) -> Option<DeathVisualScan> {
            unreachable!()
        }
        fn victim_xy(&self, _: ObjectKey) -> Option<(i32, i32)> {
            unreachable!()
        }
        fn victim_tribe_can_base_citizen(&self, _: u8) -> Option<bool> {
            unreachable!()
        }
        fn victim_nation_citizen_type(&self, _: u8) -> Option<i32> {
            unreachable!()
        }
    }
    assert_eq!(
        plan_special_hit(&MissingMasks2, ATTACKER, VICTIM),
        Err(MissingSpecialHitFact::VictimUnitMasks2)
    );
}

#[derive(Default)]
struct CaptureWorld {
    calls: Vec<CaptureCheckRequest>,
    returned_nonzero: bool,
}

impl CaptureAttemptWorld for CaptureWorld {
    fn check_capture(&mut self, request: CaptureCheckRequest) -> Option<CaptureCheckReceipt> {
        self.calls.push(request);
        Some(CaptureCheckReceipt {
            request,
            returned_nonzero: self.returned_nonzero,
        })
    }
}

#[test]
fn public_capture_attempt_calls_enemy_city_once_and_propagates_the_exit_edge() {
    let facts = Facts {
        city: true,
        ..Facts::default()
    };
    let request = CaptureCheckRequest {
        victim: VICTIM,
        attacker: ATTACKER,
    };
    let plan = plan_capture_attempt(&facts, ATTACKER, VICTIM).unwrap();
    assert_eq!(plan, CaptureAttemptPlan::CheckCapture(request));
    let mut world = CaptureWorld {
        returned_nonzero: true,
        ..CaptureWorld::default()
    };
    assert_eq!(
        apply_capture_attempt(plan, &mut world),
        Ok(CaptureAttemptReceipt::Checked {
            request,
            stop_post_damage: true,
        })
    );
    assert_eq!(world.calls, vec![request]);
}
