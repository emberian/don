//! Transaction and mutation pins for the completed state-action cohort.

use don_sim::command::{
    plan_pause, ActionDef, Bridge, Fleet, InlineDef, InlinePort, ObjectTable, Package,
    PauseHostFacts, PauseState, PauseStep, PauseTransactionRequest, Port, QueuePos, Slot,
    NUM_OWNER_SLOTS,
};
use don_sim::systems::order_dispatch::{OrderQueue, OrderRec};

#[derive(Default)]
struct RefusingFleet {
    objects: ObjectTable,
}

impl RefusingFleet {
    fn new(per_owner: usize) -> Self {
        Self {
            objects: ObjectTable::new(per_owner),
        }
    }
}

impl Fleet for RefusingFleet {
    fn alive(&self, who: u8, o: i16) -> bool {
        self.objects.alive(who, o)
    }

    fn is_unit(&self, who: u8, o: i16) -> bool {
        self.objects.is_unit(who, o)
    }

    fn is_building(&self, who: u8, o: i16) -> bool {
        self.objects.is_building(who, o)
    }

    fn group_of(&self, who: u8, o: i16) -> i16 {
        self.objects.group_of(who, o)
    }

    fn set_group_of(&mut self, who: u8, o: i16, slot: i16) {
        self.objects.set_group_of(who, o, slot);
    }

    fn uid(&self, who: u8, o: i16) -> u16 {
        self.objects.uid(who, o)
    }

    fn pos(&self, who: u8, o: i16) -> (i32, i32) {
        self.objects.pos(who, o)
    }

    fn orders(&self, who: u8, o: i16) -> Option<&OrderQueue> {
        self.objects.orders(who, o)
    }

    fn orders_mut(&mut self, who: u8, o: i16) -> Option<&mut OrderQueue> {
        self.objects.orders_mut(who, o)
    }

    fn install_order_rec(&mut self, who: u8, o: i16, order: OrderRec, q: QueuePos) -> bool {
        self.objects.install_order_rec(who, o, order, q)
    }

    fn set_stance(&mut self, who: u8, o: i16, stance: i8) {
        self.objects.set_stance(who, o, stance);
    }

    fn disband(&mut self, who: u8, o: i16) {
        self.objects.disband(who, o);
    }
}

fn fixed(op: u8, words: &[i32]) -> Vec<u8> {
    let mut bytes = vec![op];
    for word in words {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    bytes
}

fn select(bridge: &mut Bridge, package: &mut Package, fleet: &mut dyn Fleet, list: &[i16]) {
    let mut bytes = vec![0, list.len() as u8, 1];
    for object in list {
        bytes.extend_from_slice(&object.to_le_bytes());
    }
    bridge.process_all(package, &bytes, fleet).unwrap();
}

fn pause_request(state: PauseState, play: i32, requested: u8) -> PauseTransactionRequest {
    PauseTransactionRequest {
        play,
        requested,
        local_play: 2,
        state,
    }
}

fn base_pause_state() -> PauseState {
    PauseState {
        paused: false,
        pause_delay: 7,
        network: false,
        immediate_process: false,
        pause_override: false,
        pauses: [0; NUM_OWNER_SLOTS],
        restart_delay: 0,
        restart_gate_2: false,
        restart_gate_4: false,
        chat_filter_bypass: false,
    }
}

#[test]
fn unavailable_group_receipts_leave_bridge_and_world_unchanged() {
    for action in ["stance", "set_transport", "unitmask", "buildmask"] {
        assert_eq!(ActionDef::find(action).unwrap().port, Port::Complete);
    }

    let mut bridge = Bridge::new();
    let mut package = Package::new(1, 0);
    let mut fleet = RefusingFleet::new(6);
    let mut first = Slot::unit(10, 0, 0);
    first.unit_masks = 0x0400_0020;
    first.stance = 3;
    first.orders.push_back(OrderRec::default());
    fleet.objects.put(1, 0, first.clone());
    fleet.objects.put(1, 1, Slot::unit(11, 0, 0));
    let mut building = Slot::building(12, 0, 0);
    building.build_masks = 0x40;
    building.build_mask_capabilities = 0x40;
    fleet.objects.put(1, 2, building.clone());

    select(&mut bridge, &mut package, &mut fleet, &[0, 1]);
    let first_after_select = fleet.objects.get(1, 0).unwrap().clone();
    bridge.groups.get_mut(package.group).unwrap().disband = 77;
    for bytes in [fixed(2, &[1]), fixed(14, &[1]), fixed(32, &[0x100, -99])] {
        bridge
            .process_all(&mut package, &bytes, &mut fleet)
            .unwrap();
    }
    assert_eq!(bridge.groups.get(package.group).unwrap().disband, 77);
    assert_eq!(fleet.objects.get(1, 0).unwrap(), &first_after_select);

    select(&mut bridge, &mut package, &mut fleet, &[2]);
    let building_after_select = fleet.objects.get(1, 2).unwrap().clone();
    bridge
        .process_all(&mut package, &fixed(33, &[0x40, 123]), &mut fleet)
        .unwrap();
    assert_eq!(fleet.objects.get(1, 2).unwrap(), &building_after_select);
}

#[test]
fn pause_planner_pins_duplicate_denial_and_override_ordering() {
    assert_eq!(InlineDef::find(76).unwrap().port, InlinePort::Complete);

    let duplicate = pause_request(base_pause_state(), 2, 0);
    assert_eq!(
        plan_pause(&duplicate, &PauseHostFacts::default())
            .unwrap()
            .steps,
        vec![PauseStep::DuplicateDiagnostic { requested: 0 }]
    );

    let mut denied_state = base_pause_state();
    denied_state.network = true;
    denied_state.pauses[2] = 10;
    let denied = plan_pause(
        &pause_request(denied_state, 2, 1),
        &PauseHostFacts {
            interface_present: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        denied.steps,
        vec![
            PauseStep::PauseLimitMessage { play: 2 },
            PauseStep::GlobalSound { category: 64 },
            PauseStep::PauseNotice,
        ]
    );
    assert!(!denied.state.paused);

    let mut override_state = base_pause_state();
    override_state.network = true;
    override_state.pause_override = true;
    override_state.pauses[2] = 10;
    let allowed = plan_pause(
        &pause_request(override_state, 2, 1),
        &PauseHostFacts {
            sound_system_present: true,
            interface_present: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        allowed.steps,
        vec![
            PauseStep::GlobalSound { category: 87 },
            PauseStep::SetPaused { value: true },
            PauseStep::SetPauseDelay { value: 0 },
            PauseStep::SoundSystemPause,
            PauseStep::IncrementPauseCount { play: 2, value: 11 },
            PauseStep::PauseMessage {
                play: 2,
                override_template: true,
                pauses_remaining: -1,
            },
            PauseStep::PauseNotice,
        ]
    );
    assert!(allowed.state.paused);
    assert_eq!(allowed.state.pauses[2], 11);
}

#[test]
fn pause_restart_branch_requires_exact_callback_and_leader_facts() {
    let mut state = base_pause_state();
    state.paused = true;
    state.pause_delay = 0;
    state.network = true;
    state.restart_gate_2 = true;
    state.restart_gate_4 = true;
    let request = pause_request(state, 2, 0);
    assert!(
        plan_pause(&request, &PauseHostFacts::default()).is_none(),
        "the bridge must not guess the side-effecting UnitBalance::next result"
    );

    let mut leaders = [0u32; 8];
    leaders[0] = 3;
    leaders[1] = 0x23;
    leaders[2] = 0x43;
    leaders[3] = 3;
    let plan = plan_pause(
        &request,
        &PauseHostFacts {
            sound_system_present: true,
            interface_present: true,
            unit_balance_next_result: Some(0),
            leader_status_words: Some(leaders),
        },
    )
    .unwrap();
    assert_eq!(
        plan.steps,
        vec![
            PauseStep::SetPaused { value: false },
            PauseStep::SetPauseDelay { value: 2 },
            PauseStep::SoundSystemResume,
            PauseStep::GlobalSound { category: 87 },
            PauseStep::ClearRestartGate4,
            PauseStep::SetRestartDelay { value: 2 },
            PauseStep::UnitBalanceNext { result: 0 },
            PauseStep::SetChatFilterBypass,
            PauseStep::SetRestartDelay { value: 0 },
            PauseStep::LeaderVictory {
                who: 0,
                victory_type: 0,
                instant: 0,
            },
            PauseStep::LeaderVictory {
                who: 3,
                victory_type: 0,
                instant: 0,
            },
            PauseStep::PauseNotice,
        ]
    );
    assert!(!plan.state.paused);
    assert!(!plan.state.restart_gate_4);
    assert!(plan.state.chat_filter_bypass);
    assert_eq!(plan.state.restart_delay, 0);
}
