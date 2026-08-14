// SPDX-License-Identifier: GPL-3.0-or-later
//! DoNSave v12 owns the mutable `Leaders`/`Match`, lifecycle-player, and CityPool state
//! needed to resume a configured match after frame zero.

use don_sim::command::tail_command_transactions::{
    decode_tail_command,
    lifecycle::{PLAYER_LEAVE_SCAN_REQUIRED, PLAYER_PRESENT},
};
use don_sim::command::{Bridge, ObjectTable, Package};
use don_sim::systems::player_setup::ManualPlayerSetup;
use don_sim::systems::save_load::{load_sim, save_sim, SaveError};
use don_sim::systems::tech_cities::{CaravanLink, CaravanLinkArray, CityRecord};
use don_sim::systems::victory_score::{game_sem, leader_flag, DefeatType};
use don_sim::tick::lifecycle_host::PlayerTable;
use don_sim::tick::lifecycle_opcode_cohort::LifecycleFleet;
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
    let bytes = save_sim(&original).expect("current format owns the configured mid-match pair");
    assert_eq!(u32::from_le_bytes(bytes[24..28].try_into().unwrap()), 21);
    assert!(!section_range(&bytes, LEADER_MATCH).is_empty());

    let loaded = load_sim(&bytes).expect("current leader/match state loads");
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
fn v21_round_trips_the_checksum_owned_army_muster_strategy_array() {
    let mut original = configured_sim();
    for (region, value) in original.vic_leaders.slots[1]
        .strategy
        .iter_mut()
        .enumerate()
    {
        *value = (region as u16).wrapping_mul(0x101).wrapping_add(7);
    }
    let before_digest = original.channel_digest();
    let bytes = save_sim(&original).expect("v21 owns LeaderData::strategy");
    assert_eq!(u32::from_le_bytes(bytes[24..28].try_into().unwrap()), 21);

    let mut loaded = load_sim(&bytes).expect("v21 strategy array reloads");
    assert_eq!(
        loaded.vic_leaders.slots[1].strategy,
        original.vic_leaders.slots[1].strategy
    );
    assert_eq!(loaded.channel_digest(), before_digest);
    assert_eq!(save_sim(&loaded).unwrap(), bytes);

    loaded.vic_leaders.slots[1].strategy[63] ^= 1;
    assert_ne!(loaded.channel_digest(), before_digest);
}

#[test]
fn corrupt_lifecycle_player_identity_is_rejected() {
    let bytes = save_sim(&configured_sim()).unwrap();
    let range = section_range(&bytes, LEADER_MATCH);
    // PlayerTable is followed by the v12 CityPool tail. Locate its full canonical 58-byte
    // image rather than coupling this check to the size of the preceding Leader rows or
    // following City capacity.
    let mut table = Vec::new();
    table.push(1); // Some(PlayerTable)
    for slot in 0..8u8 {
        let flags = if slot < 2 {
            PLAYER_PRESENT | PLAYER_LEAVE_SCAN_REQUIRED
        } else {
            0
        };
        table.extend_from_slice(&flags.to_le_bytes());
        table.push(u8::from(slot == 1));
        table.push(if slot == 1 { 1 } else { 0 });
        table.push(slot);
    }
    table.extend_from_slice(&1i32.to_le_bytes());
    table.extend_from_slice(&0i32.to_le_bytes());
    table.extend_from_slice(&0i32.to_le_bytes());
    table.extend_from_slice(&0i32.to_le_bytes());
    table.push(0);
    assert_eq!(table.len(), 58);
    let payload = &bytes[range.clone()];
    let starts: Vec<_> = payload
        .windows(table.len())
        .enumerate()
        .filter_map(|(at, window)| (window == table).then_some(at))
        .collect();
    assert_eq!(starts.len(), 1);
    let mut corrupt = bytes.clone();
    let play0 = range.start + starts[0] + 5;
    corrupt[play0] = 7;
    assert_eq!(
        load_sim(&corrupt).err(),
        Some(SaveError::Invalid("lifecycle player identity"))
    );
}

#[test]
fn capital_resign_city_lookup_and_counters_round_trip_after_the_real_bridge() {
    let mut original = configured_sim();
    original.world.frame = 1;
    original.vic_match.frame = 1;
    original.vic_match.options.elimination = 1;
    original.vic_leaders.slots[1].lost_capital_timer = 19;
    original.vic_leaders.slots[0].cities_captured = 5;
    original.vic_leaders.slots[1].cities_lost = 7;
    original.cities.city_mark[0] = 1;
    original.cities.slots[0][0] = CityRecord {
        city_flags: 1,
        city: 0,
        o: 17,
        reg: 3,
        x: 0x1234,
        y: -0x2345,
        attack_stamp: 11,
        raid_stamp: 12,
        reduce_stamp: 13,
        capture_stamp: 14,
        assimilation_timer: 15,
        capture_strength: 16,
        traded_with: [1, 2, 3, 4, 5, 6, 7, 8],
        scouted: 2,
        in_port: 3,
        peasant_dist: 4,
        trade_val: 5,
        conquest_node: 6,
        granary: 7,
        lumber_mill: 8,
        smelter: 9,
        refinery: 10,
        free: 11,
        busy: 12,
        gatherers: 13,
        pop: 14,
        who: 0,
        race: 1,
        founder: 1,
        plundered: 15,
        ocean: 16,
        land: 17,
        filled: 18,
        bordering: 19,
        ocean_filled: 20,
        dock_tile: 21,
        was_capital_flags: 1 << 1,
        space: [22, 23, 24],
        ter: [25, 26, 27, 28, 29, 30],
        vans: CaravanLinkArray {
            items: vec![CaravanLink { cara: 31, who: 1 }],
            capacity: 5,
            grow: -1,
            flags: 0x41,
        },
        name: "Former Capital".into(),
        id: "city-id-0".into(),
    };

    let wire = {
        let mut wire = vec![70];
        wire.extend_from_slice(&1i32.to_le_bytes());
        wire
    };
    let mut bridge = Bridge::new();
    let mut package = Package::new(1, 1);
    let mut objects = ObjectTable::new(8);
    {
        let mut fleet = LifecycleFleet::new(&mut objects, &mut original);
        bridge.process_all(&mut package, &wire, &mut fleet).unwrap();
    }
    let receipts = bridge.take_discharged_tail_command_receipts();
    assert_eq!(receipts.len(), 1);
    assert!(receipts[0].valid);
    assert_eq!(original.vic_leaders.slots[1].defeated_by, 0);
    assert_eq!(
        original.vic_leaders.slots[1].defeat_type,
        DefeatType::Capital as i32
    );

    let city_mark = original.cities.city_mark;
    let cities = original.cities.slots.clone();
    let before_walk = walked(&original);
    let before_digest = original.channel_digest();
    let bytes = save_sim(&original).expect("v12 owns the live CityPool and counters");
    let loaded = load_sim(&bytes).expect("v12 restores the live CityPool and counters");
    assert_eq!(loaded.cities.city_mark, city_mark);
    assert_eq!(loaded.cities.slots, cities);
    assert_eq!(loaded.vic_leaders.slots[0].cities_captured, 5);
    assert_eq!(loaded.vic_leaders.slots[1].cities_lost, 7);
    assert_eq!(walked(&loaded), before_walk);
    assert_eq!(loaded.channel_digest(), before_digest);
    assert_eq!(save_sim(&loaded).unwrap(), bytes);
}
