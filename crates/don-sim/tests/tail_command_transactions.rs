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
fn resign_and_quit_are_whole_row_boundaries_without_a_lifecycle_image() {
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

// ---------------------------------------------------------------------------
// The `TailCommandFacts::PlayerLifecycle` arms added by the recovered
// `Player::resign` / `Player::quit` / `DropControl::process_drop` bodies.
// ---------------------------------------------------------------------------

use tail_command_transactions::lifecycle::{
    LifecycleBoundary, LifecycleCall, LifecycleImage, LifecycleRequest, LEADER_HUMAN,
    LEADER_VALID_ACTIVE, PLAYER_DROP_GATE, PLAYER_LEAVE_SCAN_REQUIRED, PLAYER_PRESENT,
    SEM_DROP_CONTROL, SEM_LOCAL_LEFT, SEM_NET_OR_RECORDING, SEM_QUIT_PREFIX,
};

fn lifecycle_image() -> LifecycleImage {
    let mut img = LifecycleImage::default();
    for slot in 0..8 {
        img.players[slot].play = slot as u8;
        img.players[slot].who = slot as u8;
        img.leaders[slot].who = slot as i32;
    }
    for slot in 0..2 {
        img.players[slot].flags = PLAYER_PRESENT | PLAYER_LEAVE_SCAN_REQUIRED;
        img.leaders[slot].leader_flags = LEADER_VALID_ACTIVE | LEADER_HUMAN;
    }
    img.console_play = 1;
    img.console_who = 1;
    img
}

fn lifecycle_facts(image: LifecycleImage) -> TailCommandFacts {
    let game_semaphore = image.semaphore[0];
    TailCommandFacts::PlayerLifecycle {
        image: Box::new(image),
        game_semaphore,
    }
}

#[test]
fn a_lifecycle_image_turns_row_70_into_a_named_leader_defeat_boundary() {
    let resign = decode_tail_command(&resign_wire(0)).unwrap();
    let facts = lifecycle_facts(lifecycle_image());
    let Ok(TailDecision::Boundary(plan)) = plan_tail_command(&resign, &facts) else {
        panic!("a resign that defeats a leader must stay a boundary");
    };
    let TailOpenBoundary::PlayerLifecycle {
        request,
        plan: lifecycle,
    } = plan.boundary
    else {
        panic!("expected the recovered lifecycle boundary");
    };
    assert_eq!(
        request,
        LifecycleRequest::Resign {
            play: 0,
            from_quit: 0
        }
    );
    assert!(lifecycle.effects.iter().any(|e| matches!(
        e,
        tail_command_transactions::lifecycle::LifecycleEffect::Call(LifecycleCall::LeaderDefeat {
            defeat_type: 6,
            ..
        })
    )));
}

#[test]
fn row_70_applies_atomically_when_a_co_tenant_keeps_the_leader_alive() {
    let mut image = lifecycle_image();
    image.set_semaphore_bit(SEM_NET_OR_RECORDING, true);
    image.players[1].who = 0; // player 1 still holds leader 0
    let resign = decode_tail_command(&resign_wire(0)).unwrap();
    let facts = lifecycle_facts(image);
    let Ok(TailDecision::Apply(plan)) = plan_tail_command(&resign, &facts) else {
        panic!("a resign with no authoritative call must be applicable");
    };
    let receipt = TailCommandReceipt {
        request: resign.clone(),
        status: TailTransactionStatus::Applied,
        facts: Some(facts.clone()),
        plan: Some(plan),
    };
    assert!(receipt.validates_for(&resign, &facts));
}

#[test]
fn the_row_71_handler_prefix_commits_inside_the_same_transaction() {
    let mut image = lifecycle_image();
    image.set_semaphore_bit(SEM_NET_OR_RECORDING, true);
    image.players[1].who = 0;
    image.console_play = 0;
    image.console_who = 0;

    let quit = decode_tail_command(&quit_wire(0, 1, 0)).unwrap();
    let facts = lifecycle_facts(image);
    let Ok(TailDecision::Apply(plan)) = plan_tail_command(&quit, &facts) else {
        panic!("a quit with no authoritative call must be applicable");
    };
    let after = plan
        .effects
        .iter()
        .find_map(|e| match e {
            TailEffect::PlayerLifecycle { plan, .. } => Some(plan.image.clone()),
            _ => None,
        })
        .expect("the lifecycle transaction is part of the applied plan");
    // `0x00943A95` sets semaphore bit 18 before `Player::quit` runs.
    assert!(after.sem(SEM_QUIT_PREFIX));
    // `Player::resign(1)` then sets bit 15 for the local player and `Player::quit` restores
    // the sampled value, which the handler prefix had just cleared.
    assert!(!after.sem(SEM_LOCAL_LEFT));
    assert_eq!(after.playing, 0);
}

#[test]
fn the_row_71_system_quit_callback_follows_the_lifecycle_transaction() {
    let mut image = lifecycle_image();
    image.set_semaphore_bit(SEM_NET_OR_RECORDING, true);
    image.players[1].who = 0;
    image.console_play = 0;
    image.console_who = 0;
    let quit = decode_tail_command(&quit_wire(0, 0, 1)).unwrap();
    let Ok(TailDecision::Apply(plan)) = plan_tail_command(&quit, &lifecycle_facts(image.clone()))
    else {
        panic!("expected an applicable quit");
    };
    assert!(matches!(
        plan.effects.last(),
        Some(TailEffect::Presentation(
            TailPresentationReceipt::SystemQuitCallback
        ))
    ));

    // The drop-control semaphore bit suppresses it.
    let mut gated = image;
    gated.set_semaphore_bit(SEM_DROP_CONTROL, true);
    let Ok(TailDecision::Apply(plan)) = plan_tail_command(&quit, &lifecycle_facts(gated)) else {
        panic!("expected an applicable quit");
    };
    assert!(!plan.effects.iter().any(|e| matches!(
        e,
        TailEffect::Presentation(TailPresentationReceipt::SystemQuitCallback)
    )));
}

#[test]
fn row_80_reaches_the_recovered_drop_control_body_only_under_semaphore_bit_four() {
    let mut image = lifecycle_image();
    image.set_semaphore_bit(SEM_DROP_CONTROL, true);
    let drop = decode_tail_command(&[late::UNGRACEFUL_DROP_OPCODE, 0, 1]).unwrap();

    // State 1 declares war between every remaining leader pair: still a boundary, but now a
    // named `Leader::action_declare` one rather than "the drop-control body is unrecovered".
    let Ok(TailDecision::Boundary(plan)) =
        plan_tail_command(&drop, &lifecycle_facts(image.clone()))
    else {
        panic!("state 1 reaches Leader::action_declare");
    };
    let TailOpenBoundary::PlayerLifecycle {
        plan: lifecycle, ..
    } = plan.boundary
    else {
        panic!("expected the recovered lifecycle boundary");
    };
    assert!(lifecycle.effects.iter().any(|e| matches!(
        e,
        tail_command_transactions::lifecycle::LifecycleEffect::Call(
            LifecycleCall::LeaderActionDeclare { .. }
        )
    )));

    // A state-3 drop for a player already marked dropped mutates nothing and applies.
    let mut gated = image;
    gated.players[0].flags |= PLAYER_DROP_GATE;
    let drop3 = decode_tail_command(&[late::UNGRACEFUL_DROP_OPCODE, 0, 3]).unwrap();
    assert!(matches!(
        plan_tail_command(&drop3, &lifecycle_facts(gated)),
        Ok(TailDecision::Apply(_))
    ));
}

#[test]
fn a_lifecycle_image_that_disagrees_with_the_handler_semaphore_byte_is_refused() {
    let image = lifecycle_image();
    let facts = TailCommandFacts::PlayerLifecycle {
        image: Box::new(image),
        game_semaphore: 0x10,
    };
    let drop = decode_tail_command(&[late::UNGRACEFUL_DROP_OPCODE, 0, 1]).unwrap();
    assert_eq!(
        plan_tail_command(&drop, &facts),
        Err(TailPlanError::SemaphoreImageMismatch {
            supplied: 0x10,
            image: 0,
        })
    );
}

#[test]
fn a_malformed_lifecycle_image_refuses_the_whole_row() {
    let mut image = lifecycle_image();
    image.leaders[3].who = 7;
    let resign = decode_tail_command(&resign_wire(0)).unwrap();
    assert_eq!(
        plan_tail_command(&resign, &lifecycle_facts(image)),
        Err(TailPlanError::Lifecycle(
            tail_command_transactions::lifecycle::LifecycleError::LeaderSlotMismatch {
                slot: 3,
                who: 7
            }
        ))
    );
}

#[test]
fn the_capital_elimination_arm_is_still_an_unrecovered_callee() {
    let mut image = lifecycle_image();
    image.elimination = 1;
    image.leaders[0].lost_capital_timer = 12;
    let resign = decode_tail_command(&resign_wire(0)).unwrap();
    let Ok(TailDecision::Boundary(plan)) = plan_tail_command(&resign, &lifecycle_facts(image))
    else {
        panic!("expected a boundary");
    };
    let TailOpenBoundary::PlayerLifecycle {
        plan: lifecycle, ..
    } = plan.boundary
    else {
        panic!("expected the recovered lifecycle boundary");
    };
    assert_eq!(
        lifecycle.boundary,
        Some(LifecycleBoundary::FindCapitalForDefeat { who: 0 })
    );
}
