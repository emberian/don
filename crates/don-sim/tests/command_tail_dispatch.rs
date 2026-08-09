// SPDX-License-Identifier: GPL-3.0-or-later

use don_sim::command::tail_command_transactions::adjacent::{
    BitMask32State, LeaderOptionDataState, LeaderOptionRowReceipt, CONSOLE_COMMAND_WIRE_BYTES,
    LEADER_OPTIONS_WIRE_BYTES,
};
use don_sim::command::tail_command_transactions::{
    plan_tail_command, TailCommandFacts, TailCommandReceipt, TailCommandRequest, TailDecision,
    TailTransactionStatus,
};
use don_sim::command::{Bridge, Fleet, InlineDef, InlinePort, ObjectTable, Package};
use don_sim::systems::order_dispatch::OrderQueue;

struct TailHost {
    objects: ObjectTable,
    calls: Vec<u8>,
    applied: Vec<u8>,
}

impl TailHost {
    fn new() -> Self {
        Self {
            objects: ObjectTable::new(1),
            calls: Vec::new(),
            applied: Vec::new(),
        }
    }
}

impl Fleet for TailHost {
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

    fn set_stance(&mut self, who: u8, o: i16, stance: i8) {
        self.objects.set_stance(who, o, stance);
    }

    fn disband(&mut self, who: u8, o: i16) {
        self.objects.disband(who, o);
    }

    fn tail_command_facts(&self, request: &TailCommandRequest) -> Option<TailCommandFacts> {
        Some(match request {
            TailCommandRequest::Resign(_) | TailCommandRequest::Quit(_) => {
                TailCommandFacts::NoExternalFacts
            }
            TailCommandRequest::LeaderOptions(command) => TailCommandFacts::LeaderOptions {
                previous: LeaderOptionRowReceipt {
                    who: command.data.who,
                    state: LeaderOptionDataState {
                        who: command.data.who,
                        peasants: command.data.peasants,
                        peasants_wait: command.data.peasants_wait,
                        buildings: command.data.buildings,
                        flags: BitMask32State {
                            bits: command.data.flags.bits,
                            size: command.data.flags.size,
                            flags: command.data.flags.flags,
                            inline: command.data.flags.inline,
                        },
                    },
                },
                console_play: -1,
            },
            TailCommandRequest::ConsoleCommand(_) => TailCommandFacts::ConsoleCommand {
                console_present: false,
            },
            TailCommandRequest::UngracefulDrop(_) => {
                TailCommandFacts::UngracefulDrop { game_semaphore: 0 }
            }
        })
    }

    fn apply_tail_command_transaction(
        &mut self,
        request: TailCommandRequest,
        facts: TailCommandFacts,
    ) -> TailCommandReceipt {
        self.calls.push(request.opcode());
        let Ok(TailDecision::Apply(plan)) = plan_tail_command(&request, &facts) else {
            return TailCommandReceipt::unavailable(request);
        };
        self.applied.push(request.opcode());
        TailCommandReceipt {
            request,
            status: TailTransactionStatus::Applied,
            facts: Some(facts),
            plan: Some(plan),
        }
    }
}

fn resign_wire() -> Vec<u8> {
    let mut wire = vec![70];
    wire.extend_from_slice(&2i32.to_le_bytes());
    wire
}

fn quit_wire() -> Vec<u8> {
    let mut wire = vec![71];
    wire.extend_from_slice(&2i32.to_le_bytes());
    wire.extend_from_slice(&[1, 0]);
    wire
}

fn leader_options_wire() -> Vec<u8> {
    let mut wire = vec![0; LEADER_OPTIONS_WIRE_BYTES];
    wire[0] = 73;
    for (offset, value) in [
        (1, 2i32),
        (5, 4),
        (9, 3),
        (13, 5),
        (17, 32),
        (21, 1),
        (25, 0),
    ] {
        wire[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    wire[29] = 2;
    wire
}

fn console_wire() -> Vec<u8> {
    let mut wire = vec![0; CONSOLE_COMMAND_WIRE_BYTES];
    wire[0] = 78;
    wire
}

#[test]
fn all_five_rows_reach_the_transaction_seam_without_becoming_inert() {
    let mut payload = resign_wire();
    payload.extend_from_slice(&quit_wire());
    payload.extend_from_slice(&leader_options_wire());
    payload.extend_from_slice(&console_wire());
    payload.extend_from_slice(&[80, 4, 3]);

    let mut bridge = Bridge::new();
    let mut package = Package::new(0, 0);
    let mut host = TailHost::new();
    bridge
        .process_all(&mut package, &payload, &mut host)
        .unwrap();

    assert_eq!(host.calls, [70, 71, 73, 78, 80]);
    assert_eq!(host.applied, [73, 78, 80]);
    assert_eq!(bridge.stats.inline_state, 5);
    assert_eq!(bridge.stats.inert, 0);
    let receipts = bridge.take_tail_command_receipts();
    assert_eq!(receipts.len(), 5);
    assert!(receipts.iter().all(|receipt| receipt.valid));
}

#[test]
fn row_level_closure_remains_red_for_every_dynamic_tail() {
    for opcode in [70, 71, 73, 78, 80] {
        assert_eq!(
            InlineDef::find(opcode).unwrap().port,
            InlinePort::StateWired
        );
    }
}
