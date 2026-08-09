//! Wire-level transaction pins for opcode 67 `CheatInitUnitCommand`.

use std::collections::VecDeque;

use don_sim::command::{
    Bridge, CheatInitUnitAllocationRequest, CheatInitUnitNearbyOutcome, CheatInitUnitNearbyRequest,
    CheatInitUnitRequest, CheatInitUnitStepReceipt, CheatInitUnitTransactionReceipt,
    CheatInitUnitTransactionStatus, CheatWarningReceipt, Fleet, InlineDef, InlinePort, ObjectTable,
    Package, CHEAT_INIT_NEARBY_TAIL,
};
use don_sim::systems::order_dispatch::OrderQueue;

fn wire(who: i32, type_index: i32, x: i32, y: i32) -> Vec<u8> {
    let mut bytes = vec![67];
    for value in [who, type_index, x, y] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

struct TransactionFleet {
    objects: ObjectTable,
    replies: VecDeque<CheatInitUnitTransactionReceipt>,
    requests: Vec<CheatInitUnitRequest>,
}

impl TransactionFleet {
    fn with_reply(reply: CheatInitUnitTransactionReceipt) -> Self {
        Self {
            objects: ObjectTable::new(0),
            replies: VecDeque::from([reply]),
            requests: Vec::new(),
        }
    }
}

impl Fleet for TransactionFleet {
    fn alive(&self, who: u8, o: i16) -> bool {
        Fleet::alive(&self.objects, who, o)
    }

    fn is_unit(&self, who: u8, o: i16) -> bool {
        Fleet::is_unit(&self.objects, who, o)
    }

    fn is_building(&self, who: u8, o: i16) -> bool {
        Fleet::is_building(&self.objects, who, o)
    }

    fn group_of(&self, who: u8, o: i16) -> i16 {
        Fleet::group_of(&self.objects, who, o)
    }

    fn set_group_of(&mut self, who: u8, o: i16, slot: i16) {
        Fleet::set_group_of(&mut self.objects, who, o, slot);
    }

    fn uid(&self, who: u8, o: i16) -> u16 {
        Fleet::uid(&self.objects, who, o)
    }

    fn pos(&self, who: u8, o: i16) -> (i32, i32) {
        Fleet::pos(&self.objects, who, o)
    }

    fn orders(&self, who: u8, o: i16) -> Option<&OrderQueue> {
        Fleet::orders(&self.objects, who, o)
    }

    fn orders_mut(&mut self, who: u8, o: i16) -> Option<&mut OrderQueue> {
        Fleet::orders_mut(&mut self.objects, who, o)
    }

    fn set_stance(&mut self, who: u8, o: i16, stance: i8) {
        Fleet::set_stance(&mut self.objects, who, o, stance);
    }

    fn disband(&mut self, who: u8, o: i16) {
        Fleet::disband(&mut self.objects, who, o);
    }

    fn apply_cheat_init_unit_transaction(
        &mut self,
        request: CheatInitUnitRequest,
    ) -> CheatInitUnitTransactionReceipt {
        self.requests.push(request);
        self.replies
            .pop_front()
            .expect("one configured transaction")
    }
}

fn applied(
    request: CheatInitUnitRequest,
    steps: Vec<CheatInitUnitStepReceipt>,
) -> CheatInitUnitTransactionReceipt {
    CheatInitUnitTransactionReceipt {
        request,
        status: CheatInitUnitTransactionStatus::Applied,
        steps,
    }
}

fn nearby(
    owner: u8,
    request: CheatInitUnitRequest,
    outcome: CheatInitUnitNearbyOutcome,
) -> CheatInitUnitStepReceipt {
    CheatInitUnitStepReceipt::Nearby {
        request: CheatInitUnitNearbyRequest {
            owner,
            type_index: request.type_index,
            origin_x: request.x,
            origin_y: request.y,
            tail: CHEAT_INIT_NEARBY_TAIL,
        },
        outcome,
    }
}

fn allocation(
    owner: i32,
    type_index: i32,
    x: i32,
    y: i32,
    object_id: i32,
) -> CheatInitUnitStepReceipt {
    CheatInitUnitStepReceipt::Allocation {
        request: CheatInitUnitAllocationRequest {
            owner,
            type_index,
            x,
            y,
            tail: [-1; 3],
        },
        object_id,
    }
}

#[test]
fn direct_owner_preserves_raw_wire_identity_allocation_failure_and_warning_tail() {
    assert_eq!(InlineDef::find(67).unwrap().port, InlinePort::Complete);

    let mut bridge = Bridge::new();
    bridge.inline.network = true;
    bridge.inline.accum_cheated[6] = u8::MAX;
    bridge.inline.player_valid = [true, false, true, false, false, false, false, true];
    let expected = CheatInitUnitRequest {
        who: 6,
        type_index: 0x1234_5678,
        x: i32::MIN,
        y: i32::MAX,
        valid_players: bridge.inline.player_valid,
    };
    let mut world = TransactionFleet::with_reply(applied(
        expected,
        vec![allocation(
            expected.who,
            expected.type_index,
            expected.x,
            expected.y,
            -7,
        )],
    ));
    bridge
        .process_all(
            &mut Package::new(0, 0),
            &wire(expected.who, expected.type_index, expected.x, expected.y),
            &mut world,
        )
        .unwrap();

    assert_eq!(world.requests, vec![expected]);
    let records = bridge.take_cheat_init_unit_receipts();
    assert_eq!(records.len(), 1);
    assert!(records[0].valid);
    assert_eq!(records[0].observed.steps.len(), 1);
    assert_eq!(bridge.inline.accum_cheated[6], 0);
    assert_eq!(
        bridge.take_cheat_warning_receipts(),
        vec![CheatWarningReceipt {
            who: 6,
            sound_category: 99,
        }]
    );
    assert_eq!(bridge.stats.inline_state, 1);
    assert_eq!(bridge.stats.inert, 0);
}

#[test]
fn negative_owner_scans_only_valid_players_and_receipt_validation_fails_closed() {
    let mut bridge = Bridge::new();
    bridge.inline.network = true;
    bridge.inline.accum_cheated[8] = u8::MAX;
    bridge.inline.player_valid = [true, false, true, false, false, false, false, true];
    let expected = CheatInitUnitRequest {
        who: -99,
        type_index: 77,
        x: -800,
        y: 900,
        valid_players: bridge.inline.player_valid,
    };
    let steps = vec![
        nearby(
            0,
            expected,
            CheatInitUnitNearbyOutcome::Found { x: -790, y: 910 },
        ),
        allocation(0, 77, -790, 910, 4),
        nearby(2, expected, CheatInitUnitNearbyOutcome::NotFound),
        nearby(
            7,
            expected,
            CheatInitUnitNearbyOutcome::Found { x: -780, y: 920 },
        ),
        allocation(7, 77, -780, 920, -1),
    ];
    let mut world = TransactionFleet::with_reply(applied(expected, steps.clone()));
    bridge
        .process_all(
            &mut Package::new(0, 0),
            &wire(expected.who, expected.type_index, expected.x, expected.y),
            &mut world,
        )
        .unwrap();

    let records = bridge.take_cheat_init_unit_receipts();
    assert!(records[0].valid);
    assert_eq!(records[0].observed.steps, steps);
    assert_eq!(bridge.inline.accum_cheated[8], 0);
    assert_eq!(
        bridge.take_cheat_warning_receipts(),
        vec![CheatWarningReceipt {
            who: 8,
            sound_category: 99,
        }],
        "the completed loop counter, not the wire's negative owner, reaches warning"
    );

    let malformed = applied(
        expected,
        vec![nearby(0, expected, CheatInitUnitNearbyOutcome::NotFound)],
    );
    let mut malformed_world = TransactionFleet::with_reply(malformed);
    bridge
        .process_all(
            &mut Package::new(0, 0),
            &wire(expected.who, expected.type_index, expected.x, expected.y),
            &mut malformed_world,
        )
        .unwrap();
    assert!(!bridge.take_cheat_init_unit_receipts()[0].valid);

    let mut unavailable_bridge = Bridge::new();
    unavailable_bridge.inline.player_valid = expected.valid_players;
    unavailable_bridge
        .process_all(
            &mut Package::new(0, 0),
            &wire(expected.who, expected.type_index, expected.x, expected.y),
            &mut ObjectTable::new(0),
        )
        .unwrap();
    let unavailable = unavailable_bridge.take_cheat_init_unit_receipts();
    assert!(unavailable[0].valid);
    assert_eq!(
        unavailable[0].observed.status,
        CheatInitUnitTransactionStatus::Unavailable
    );
    assert!(unavailable[0].observed.steps.is_empty());
}
