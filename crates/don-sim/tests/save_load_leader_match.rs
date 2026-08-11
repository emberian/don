// SPDX-License-Identifier: GPL-3.0-or-later
//! DoNSave v11 owns the mutable `Leaders`/`Match` and lifecycle-player state needed to
//! resume a configured match after frame zero.

use don_sim::command::tail_command_transactions::{
    decode_tail_command,
    lifecycle::{PLAYER_LEAVE_SCAN_REQUIRED, PLAYER_PRESENT},
};
use don_sim::systems::player_setup::ManualPlayerSetup;
use don_sim::systems::save_load::{load_sim, save_sim, SaveError};
use don_sim::systems::victory_score::{game_sem, leader_flag, DefeatType};
use don_sim::tick::lifecycle_host::PlayerTable;
use don_sim::tick::Sim;

const LEADER_MATCH: u16 = 0x000a;

fn section_range(bytes: &[u8], want: u16) -> std::ops::Range<usize> {
    let mut at = 16usize; // magic + root header
    let children = u16::from_le_bytes(bytes[14..16].try_into().unwrap());
    for _ in 0..children {
        let size = u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
        let id = u16::from_le_bytes(bytes[at + 4..at + 6].try_into().unwrap());
        if id == want {
            return at + 8..at + size;
        }
        at += size;
    }
    panic!("section {want:#06x} absent")
}

fn configured_sim() -> Sim {
    let mut sim = Sim::new(0x11ea_de55, 8);
    let mut setup = ManualPlayerSetup {
        active_mask: 0x03,
        team_style: 1,
        local_player_setup_slot: 0,
        ..ManualPlayerSetup::default()
    };
    setup.teams[0] = 0;
    setup.teams[1] = 1;
    sim.vic_match.set_sem(game_sem::CHECK_VICTORY_MODE);
    sim.start_manual_player_setup(setup).unwrap();

    let mut players = PlayerTable::new();
    let seated = PLAYER_PRESENT | PLAYER_LEAVE_SCAN_REQUIRED;
    players.seat(0, seated, 0, 0);
    players.seat(1, seated, 1, 1);
    players.console_play = 0;
    players.console_who = 0;
    sim.players = Some(players);
    sim
}

fn resign(play: i32) -> don_sim::command::tail_command_transactions::TailCommandRequest {
    let mut wire = [0u8; 5];
    wire[0] = 70;
    wire[1..].copy_from_slice(&play.to_le_bytes());
    decode_tail_command(&wire).unwrap()
}

fn walked(sim: &Sim) -> (Vec<u8>, Vec<u8>) {
    let mut leaders = Vec::new();
    sim.vic_leaders.walk_bytes(&mut leaders);
    let mut game = Vec::new();
    sim.vic_match.walk_bytes(&mut game);
    (leaders, game)
}

#[test]
fn a_commanded_match_round_trips_after_frame_zero_and_resaves_identically() {
    let mut original = configured_sim();
    // Cross the PlayerSetup boundary while leaving unrelated step-8 AI inputs at their
    // independently supported snapshot. This test owns Leaders/Match, not that subsystem.
    original.world.frame = 1;
    original.vic_match.frame = 1;
    assert_eq!(original.world.frame, 1);

    let receipt = original.apply_tail_command_transaction(&resign(1));
    assert!(receipt.committed());
    assert!(receipt.validates());
    assert!(original.vic_leaders.slots[1].flag(leader_flag::DEFEATED));
    assert_eq!(
        original.vic_leaders.slots[1].defeat_type,
        DefeatType::Resign as i32
    );
    assert!(original.vic_match.sem(game_sem::GAME_OVER));

    let before_walk = walked(&original);
    let before_digest = original.channel_digest();
    let before_players = original.players.clone();
    let before_setup = original.vic_leaders.setup_owner.applied().cloned();
    let bytes = save_sim(&original).expect("v11 owns the configured mid-match pair");
    assert_eq!(u32::from_le_bytes(bytes[24..28].try_into().unwrap()), 11);
    assert!(!section_range(&bytes, LEADER_MATCH).is_empty());

    let loaded = load_sim(&bytes).expect("v11 leader/match state loads");
    assert_eq!(walked(&loaded), before_walk);
    assert_eq!(loaded.channel_digest(), before_digest);
    assert_eq!(loaded.players, before_players);
    assert_eq!(
        loaded.vic_leaders.setup_owner.applied(),
        before_setup.as_ref()
    );
    assert_eq!(save_sim(&loaded).unwrap(), bytes);
}

#[test]
fn corrupt_lifecycle_player_identity_is_rejected() {
    let bytes = save_sim(&configured_sim()).unwrap();
    let range = section_range(&bytes, LEADER_MATCH);
    // The optional PlayerTable is the final 58 bytes. Its first row's `play` byte is
    // payload offset +5; retail requires players[i].play == i.
    let mut corrupt = bytes.clone();
    let play0 = range.end - 58 + 5;
    corrupt[play0] = 7;
    assert_eq!(
        load_sim(&corrupt).err(),
        Some(SaveError::Invalid("lifecycle player identity"))
    );
}
