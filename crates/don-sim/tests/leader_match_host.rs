// SPDX-License-Identifier: GPL-3.0-or-later
//! Canonical `Leaders`/`Match` terminal transaction, including the command-owned defeat
//! caller and the live object cleanup adapter.

use don_sim::command::tail_command_transactions::{
    decode_tail_command,
    lifecycle::{PLAYER_LEAVE_SCAN_REQUIRED, PLAYER_PRESENT},
};
use don_sim::systems::production;
use don_sim::systems::victory_score::{game_sem, leader_flag, DefeatType, VictoryType};
use don_sim::tick::leader_match_host::{LeaderMatchError, LeaderMatchRequest};
use don_sim::tick::lifecycle_host::PlayerTable;
use don_sim::tick::Sim;

fn two_player_sim(seed: u64) -> Sim {
    let mut sim = Sim::new(seed, 8);
    sim.activate(0);
    sim.activate(1);
    sim.vic_match.set_sem(game_sem::CHECK_VICTORY_MODE);
    sim
}

#[test]
fn victory_publishes_both_checksum_owners_and_drains_every_winner_queue() {
    let mut sim = two_player_sim(0x71a0);
    let ty = 600usize;
    for who in 0..2usize {
        sim.spawn_build(
            who,
            production::BuildData {
                flags: production::flag::VALID | production::flag::ACTIVE,
                queue: production::BuildQueue {
                    queued: 1,
                    entries: vec![production::BuildQueueEntry {
                        type_index: ty as i16,
                        elapsed: 17,
                        ..Default::default()
                    }],
                },
                ..Default::default()
            },
        );
        sim.production_runtime.leaders[who].queued_counts[ty] = 1;
    }

    let before = sim.channel_digest();
    let receipt = sim
        .apply_leader_match_transaction(LeaderMatchRequest::Victory {
            who: 0,
            victory_type: VictoryType::Generic,
            instant: 0,
        })
        .expect("valid leader slot");

    assert_eq!(receipt.pair.terminal_queue_cleanup, 0b11);
    assert_eq!(receipt.pair.defeat_unit_cleanup, 0b10);
    assert!(receipt.cleanup_error.is_none());
    assert_ne!(receipt.pair.leaders_before, receipt.pair.leaders_after);
    assert_ne!(receipt.pair.match_before, receipt.pair.match_after);
    assert_ne!(sim.channel_digest(), before);
    assert!(sim.vic_leaders.slots[0].flag(leader_flag::WON));
    assert!(sim.vic_leaders.slots[1].flag(leader_flag::DEFEATED));
    assert!(sim.vic_match.sem(game_sem::GAME_OVER));
    assert!(sim.vic_match.sem(game_sem::VICTORY_RESOLVED));
    assert!(sim.builds.iter().all(|build| build.queue.queued == 0));
    assert_eq!(sim.production_runtime.leaders[0].queued_counts[ty], 0);
    assert_eq!(sim.production_runtime.leaders[1].queued_counts[ty], 0);
}

#[test]
fn invalid_target_is_atomic_across_both_checksum_owners() {
    let mut sim = two_player_sim(0x71a1);
    let before = sim.channel_digest();
    let err = sim
        .apply_leader_match_transaction(LeaderMatchRequest::Defeat {
            who: 8,
            defeat_type: DefeatType::Resign,
            by: -1,
            instant: 0,
        })
        .expect_err("retail has exactly eight leaders");
    assert_eq!(err, LeaderMatchError::LeaderOutOfRange { who: 8 });
    assert_eq!(sim.channel_digest(), before);
}

#[test]
fn decoded_resign_uses_the_same_pair_owner_and_reaches_game_over() {
    let mut sim = two_player_sim(0x71a2);
    let mut players = PlayerTable::new();
    let seated = PLAYER_PRESENT | PLAYER_LEAVE_SCAN_REQUIRED;
    players.seat(0, seated, 0, 0);
    players.seat(1, seated, 1, 1);
    players.console_play = 0;
    players.console_who = 0;
    sim.players = Some(players);

    let mut wire = [0u8; 5];
    wire[0] = 70;
    wire[1..].copy_from_slice(&1i32.to_le_bytes());
    let request = decode_tail_command(&wire).expect("row 70");
    let before = sim.channel_digest();
    let receipt = sim.apply_tail_command_transaction(&request);

    assert!(receipt.committed());
    assert!(receipt.validates());
    assert_ne!(sim.channel_digest(), before);
    assert_eq!(
        sim.vic_leaders.slots[1].defeat_type,
        DefeatType::Resign as i32
    );
    assert!(sim.vic_leaders.slots[1].flag(leader_flag::DEFEATED));
    assert!(sim.vic_leaders.slots[0].flag(leader_flag::WON));
    assert!(sim.vic_match.sem(game_sem::GAME_OVER));
}
