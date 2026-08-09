// SPDX-License-Identifier: GPL-3.0-or-later

#[path = "../src/systems/tail_command_transactions.rs"]
mod tail_command_transactions;

use tail_command_transactions::adjacent::{
    BitMask32State, LeaderOptionDataState, LeaderOptionRowReceipt, CONSOLE_COMMAND_WIRE_BYTES,
    LEADER_OPTIONS_WIRE_BYTES,
};
use tail_command_transactions::*;

fn resign_wire(play: i32) -> Vec<u8> {
    let mut wire = vec![RESIGN_OPCODE];
    wire.extend_from_slice(&play.to_le_bytes());
    wire
}

fn quit_wire(play: i32, replay: u8, system_quit: u8) -> Vec<u8> {
    let mut wire = resign_wire(play);
    wire[0] = QUIT_OPCODE;
    wire.extend_from_slice(&[replay, system_quit]);
    wire
}

fn leader_wire(who: i32, peasants: i32, buildings: i32, flags: u8) -> Vec<u8> {
    let mut wire = vec![0; LEADER_OPTIONS_WIRE_BYTES];
    wire[0] = adjacent::LEADER_OPTIONS_OPCODE;
    for (offset, value) in [
        (1, who),
        (5, peasants),
        (9, 3),
        (13, buildings),
        (17, 32),
        (21, 1),
        (25, 0),
    ] {
        wire[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    wire[29] = flags;
    wire
}

fn previous(who: i32) -> LeaderOptionRowReceipt {
    LeaderOptionRowReceipt {
        who,
        state: LeaderOptionDataState {
            who,
            peasants: 4,
            peasants_wait: 3,
            buildings: 5,
            flags: BitMask32State {
                bits: 32,
                size: 1,
                flags: 0,
                inline: [2, 0, 0, 0],
            },
        },
    }
}

#[test]
fn fixed_decoders_pin_all_five_rows() {
    assert_eq!(
        decode_tail_command(&resign_wire(-7)),
        Ok(TailCommandRequest::Resign(ResignRequest { play: -7 }))
    );
    assert_eq!(
        decode_tail_command(&quit_wire(3, 0x80, 2)),
        Ok(TailCommandRequest::Quit(QuitRequest {
            play: 3,
            replay: 0x80,
            system_quit: 2,
        }))
    );
    assert!(matches!(
        decode_tail_command(&leader_wire(2, 4, 5, 2)),
        Ok(TailCommandRequest::LeaderOptions(_))
    ));

    let mut console = vec![0; CONSOLE_COMMAND_WIRE_BYTES];
    console[0] = adjacent::CONSOLE_COMMAND_OPCODE;
    assert!(matches!(
        decode_tail_command(&console),
        Ok(TailCommandRequest::ConsoleCommand(_))
    ));
    assert_eq!(
        decode_tail_command(&[late::UNGRACEFUL_DROP_OPCODE, 4, 3]),
        Ok(TailCommandRequest::UngracefulDrop(
            late::UngracefulDropRequest { play: 4, state: 3 }
        ))
    );
}

#[test]
fn resign_and_quit_are_always_boundaries() {
    let resign = decode_tail_command(&resign_wire(2)).unwrap();
    assert!(matches!(
        plan_tail_command(&resign, &TailCommandFacts::NoExternalFacts),
        Ok(TailDecision::Boundary(TailBoundaryPlan {
            boundary: TailOpenBoundary::PlayerResign(ResignRequest { play: 2 }),
            ..
        }))
    ));

    let quit = decode_tail_command(&quit_wire(2, 1, 1)).unwrap();
    assert!(matches!(
        plan_tail_command(&quit, &TailCommandFacts::NoExternalFacts),
        Ok(TailDecision::Boundary(TailBoundaryPlan {
            boundary: TailOpenBoundary::PlayerQuit(QuitRequest { play: 2, .. }),
            ..
        }))
    ));
}

#[test]
fn leader_options_applies_only_when_no_cascade_or_local_mirror_is_reached() {
    let unchanged = decode_tail_command(&leader_wire(2, 4, 5, 2)).unwrap();
    let facts = TailCommandFacts::LeaderOptions {
        previous: previous(2),
        console_play: -1,
    };
    assert!(matches!(
        plan_tail_command(&unchanged, &facts),
        Ok(TailDecision::Apply(_))
    ));

    let changed = decode_tail_command(&leader_wire(2, 7, 5, 2)).unwrap();
    assert!(matches!(
        plan_tail_command(&changed, &facts),
        Ok(TailDecision::Boundary(TailBoundaryPlan {
            boundary: TailOpenBoundary::LeaderOptions(_),
            ..
        }))
    ));
}

#[test]
fn console_and_drop_null_gates_have_recomputable_no_tail_apply_branches() {
    let mut console_wire = vec![0; CONSOLE_COMMAND_WIRE_BYTES];
    console_wire[0] = adjacent::CONSOLE_COMMAND_OPCODE;
    let console = decode_tail_command(&console_wire).unwrap();
    let absent = TailCommandFacts::ConsoleCommand {
        console_present: false,
    };
    let TailDecision::Apply(console_plan) = plan_tail_command(&console, &absent).unwrap() else {
        panic!("absent console must be a whole-row no-tail plan");
    };
    let receipt = TailCommandReceipt {
        request: console.clone(),
        status: TailTransactionStatus::Applied,
        facts: Some(absent),
        plan: Some(console_plan),
    };
    assert!(receipt.validates(&console));

    let drop = decode_tail_command(&[late::UNGRACEFUL_DROP_OPCODE, 1, 2]).unwrap();
    let solo = TailCommandFacts::UngracefulDrop { game_semaphore: 0 };
    assert!(matches!(
        plan_tail_command(&drop, &solo),
        Ok(TailDecision::Apply(_))
    ));
    let network = TailCommandFacts::UngracefulDrop {
        game_semaphore: late::GAME_SEM_NETWORK_DROP,
    };
    assert!(matches!(
        plan_tail_command(&drop, &network),
        Ok(TailDecision::Boundary(TailBoundaryPlan {
            boundary: TailOpenBoundary::DropControlProcessDrop(_),
            ..
        }))
    ));
}

#[test]
fn applied_receipts_cannot_forge_a_boundary_decision() {
    let request = decode_tail_command(&resign_wire(5)).unwrap();
    let forged = TailCommandReceipt {
        request: request.clone(),
        status: TailTransactionStatus::Applied,
        facts: Some(TailCommandFacts::NoExternalFacts),
        plan: Some(TailCommandPlan {
            request: request.clone(),
            effects: Vec::new(),
        }),
    };
    assert!(!forged.validates(&request));
    assert!(TailCommandReceipt::unavailable(request.clone()).validates(&request));
}

#[test]
fn applied_receipts_are_bound_to_the_bridge_preflight_snapshot() {
    let request = decode_tail_command(&[late::UNGRACEFUL_DROP_OPCODE, 1, 2]).unwrap();
    let supplied = TailCommandFacts::UngracefulDrop { game_semaphore: 0 };
    let TailDecision::Apply(plan) = plan_tail_command(&request, &supplied).unwrap() else {
        panic!("non-network drop must be a no-tail apply plan");
    };
    let receipt = TailCommandReceipt {
        request: request.clone(),
        status: TailTransactionStatus::Applied,
        facts: Some(supplied.clone()),
        plan: Some(plan),
    };
    assert!(receipt.validates_for(&request, &supplied));
    assert!(!receipt.validates_for(
        &request,
        &TailCommandFacts::UngracefulDrop { game_semaphore: 1 },
    ));
}
