//! Real-corpus and mutation-kill gates for the same-frame Groups Sim adapter.

use don_replay::groups_channel::{CORPUS_INITIAL_GROUPS_CHANNEL, INITIAL_WALKED_BYTES};
use don_replay::groups_sim_channel::{
    issue_replay_group_move, sim_groups_checksum, GroupSimChannelError, ReplayGroupMoveSource,
};
use don_replay::harness::{Simulation, WorldSim};
use don_replay::replay::{corpus, Replay};
use don_replay::Channel;
use don_sim::systems::canonical_group_move_host::{GroupMoveAuthority, MoveMemberAuthority};
use don_sim::systems::groups_guys::FormationMember;
use don_sim::systems::movement::PathStack;
use don_sim::tick::lifecycle_host::PlayerTable;
use don_sim::tick::Sim;
use don_sim::Handle;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const CHECKSUM_CORPUS_FILES: [&str; 21] = [
    "Playback___2018.11.17_13_21_42__Sat_.rcx",
    "Playback___2018.12.01_18_33_16__Sat_.rcx",
    "Playback___2019.03.24_11_56_19__Sun_.rcx",
    "Playback___2020.02.08_10_49_15__Sat_.rcx",
    "Playback___2020.02.21_09_48_48__Fri_.rcx",
    "Playback___2020.07.25_19_30_12__Sat_.rcx",
    "Playback___2020.07.25_19_32_40__Sat_.rcx",
    "Playback___2020.07.25_19_42_43__Sat_.rcx",
    "Playback___2024.02.23_20_49_35__Fri_.rcx",
    "Playback___2024.02.23_21_38_38__Fri_.rcx",
    "Playback___2024.02.24_21_25_53__Sat_.rcx",
    "Playback___2024.03.10_20_54_34__Sun_.rcx",
    "Playback___2024.03.10_20_56_31__Sun_.rcx",
    "Playback___2024.03.17_19_58_17__Sun_.rcx",
    "Playback___2024.03.18_18_18_49__Mon_.rcx",
    "Playback___2024.03.20_17_28_53__Wed_.rcx",
    "Playback___2024.03.23_21_16_13__Sat_.rcx",
    "Playback___2024.03.29_21_52_57__Fri_.rcx",
    "Playback___2024.03.29_22_00_58__Fri_.rcx",
    "Playback___2024.04.10_17_05_19__Wed_.rcx",
    "Playback___2025.02.10_21_26_50__Mon_.rcx",
];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn checksum_replays() -> &'static [Replay] {
    static REPLAYS: OnceLock<Vec<Replay>> = OnceLock::new();
    REPLAYS
        .get_or_init(|| {
            corpus(&repo_root())
                .into_iter()
                .filter(|path| {
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| CHECKSUM_CORPUS_FILES.contains(&name))
                })
                .filter_map(|path| Replay::open(&path).ok())
                .filter(|replay| replay.checksum_packets > 0)
                .collect()
        })
        .as_slice()
}

fn sources(replay: &Replay) -> Vec<ReplayGroupMoveSource> {
    replay
        .turns
        .iter()
        .enumerate()
        .flat_map(|(turn_index, turn)| {
            turn.players
                .iter()
                .enumerate()
                .filter_map(move |(player_index, _)| {
                    ReplayGroupMoveSource::from_replay(replay, turn_index, player_index).ok()
                })
        })
        .collect()
}

fn decode_selected(source: &ReplayGroupMoveSource) -> (u8, Vec<i16>) {
    let bytes = source.group_bytes();
    let count = bytes[1] as usize;
    let who = bytes[2];
    let objects = bytes[3..3 + count * 2]
        .chunks_exact(2)
        .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    (who, objects)
}

fn sim_for(source: &ReplayGroupMoveSource) -> (Sim, Vec<Handle>) {
    let (who, objects) = decode_selected(source);
    let mut sim = Sim::new(0x51a7, 128);
    let mut players = PlayerTable::new();
    players.seat(source.play, 1, who, 0);
    sim.players = Some(players);
    sim.world.frame = source.package_frame;

    let mut handles = Vec::new();
    for (index, o) in objects.iter().copied().enumerate() {
        if o < 0
            || sim
                .world
                .unit_row_at(i32::from(who), i32::from(o))
                .is_some()
        {
            continue;
        }
        let handle = sim
            .world
            .allocate_typed_at(who, i32::from(o), 2_000 + index as i32 * 120, 3_000)
            .expect("real Group member must fit the canonical object band");
        let row = sim.world.row_of(handle).unwrap();
        sim.world.units.group_mut()[row] = -1;
        sim.world.units.o_down_mut()[row] = -1;
        sim.world.units.form_mut()[row] = 0;
        sim.world.units.form_mod_mut()[row] = 50;
        sim.world.units.angle_mut()[row] = 0x1100_0000 + index as i32 * 0x0100_0000;
        sim.world.units.set_unit_masks(row, 0x0400_0400);
        handles.push(handle);
    }
    sim.paths
        .resize(sim.world.live_count() as usize, PathStack::default());
    sim.replace_group_move_authority(GroupMoveAuthority {
        revision: 11,
        composition_digest: [0x6d; 32],
        destination_is_water: false,
        force_formation_facing_zero: false,
        members: handles
            .iter()
            .enumerate()
            .map(|(index, &handle)| MoveMemberAuthority {
                handle,
                role: 0x100 << (index % 8),
                on_map: true,
                is_captain: true,
                can_move: true,
                can_install_order: true,
                is_plane: false,
                domain: 0,
                unit_flags: 0,
                speed: 20 + index as i32,
                admits_unsplit_move_near: true,
                land_formation: FormationMember {
                    category: 0,
                    x_spacing: 48,
                    y_spacing: 48,
                    formation_size: 1,
                    guy_spacing: 48,
                    modern_infantry: false,
                    width: 50,
                    angle: 0,
                },
                water_formation: FormationMember::default(),
            })
            .collect(),
    });
    (sim, handles)
}

fn executable_source() -> Result<&'static ReplayGroupMoveSource, &'static str> {
    static SOURCE: OnceLock<Result<ReplayGroupMoveSource, String>> = OnceLock::new();
    SOURCE
        .get_or_init(|| {
            let mut attempted = 0usize;
            let mut errors = Vec::new();
            for replay in checksum_replays() {
                for source in sources(replay) {
                    attempted += 1;
                    let (mut sim, _) = sim_for(&source);
                    match issue_replay_group_move(&mut sim, &source) {
                        Ok(_) => return Ok(source),
                        Err(error) if errors.len() < 12 => errors.push(format!(
                            "{} turn {} play {}: {error:?}",
                            replay.path.display(),
                            source.lockstep_serial,
                            source.play
                        )),
                        Err(_) => {}
                    }
                }
            }
            Err(format!(
                "no executable Group -> Move source among {attempted} exact corpus pairs; first refusals: {errors:?}"
            ))
        })
        .as_ref()
        .map_err(String::as_str)
}

#[test]
fn checksum_corpus_contains_a_real_strict_group_move_cohort() {
    let replays = checksum_replays();
    if replays.is_empty() {
        eprintln!("\n  SKIPPED — NOT A PASS. No checksum-bearing retail corpus.\n");
        return;
    }
    assert_eq!(replays.len(), 21, "the tracked checksum corpus changed");
    let admitted: Vec<_> = replays.iter().flat_map(sources).collect();
    assert!(
        !admitted.is_empty(),
        "no checksum-bearing replay contains the exact Group -> Move cohort"
    );
    assert!(admitted.iter().all(|source| {
        source.package_had_checksum
            && source.group_bytes()[0] == 0
            && source.move_bytes()[0] == 7
            && source.checksum_command_index > source.pair_command_index + 1
            && source.replay_payload_len > source.replay_stream_start
    }));
    assert!(admitted.iter().any(|source| {
        !source.inert_opcodes.is_empty()
            || !source.unowned_sim_prefix.is_empty()
            || !source.unowned_sim_suffix.is_empty()
    }));
}

#[test]
fn package_checksum_provenance_is_not_inferred_from_its_recording() {
    let Some(replay) = checksum_replays()
        .iter()
        .find(|replay| !sources(replay).is_empty())
    else {
        eprintln!("\n  SKIPPED — NOT A PASS. No checksum-bearing retail Group -> Move cohort.\n");
        return;
    };
    let source = sources(replay).remove(0);
    let mut stripped = replay.clone();
    stripped.turns[source.turn_index].players[source.player_index].checksums = None;
    assert!(stripped.checksum_packets > 0);
    assert_eq!(
        ReplayGroupMoveSource::from_replay(&stripped, source.turn_index, source.player_index,),
        Err(GroupSimChannelError::PackageHasNoChecksum)
    );

    let mut wrong_phase = replay.clone();
    let commands = &mut wrong_phase.turns[source.turn_index].players[source.player_index].commands;
    let checksum = commands.remove(source.checksum_command_index);
    commands.insert(source.pair_command_index, checksum);
    assert_eq!(
        ReplayGroupMoveSource::from_replay(&wrong_phase, source.turn_index, source.player_index,),
        Err(GroupSimChannelError::ChecksumDoesNotFollowPair)
    );

    let suffix_opcode = *source
        .unowned_sim_suffix
        .first()
        .expect("the pinned corpus witness has an unowned Sim suffix");
    let mut intervening = replay.clone();
    let commands = &mut intervening.turns[source.turn_index].players[source.player_index].commands;
    let suffix_index = commands
        .iter()
        .enumerate()
        .skip(source.checksum_command_index + 1)
        .find_map(|(index, command)| (command.opcode == suffix_opcode).then_some(index))
        .expect("the retained suffix opcode comes from this exact package");
    let injected = commands.remove(suffix_index);
    commands.insert(source.pair_command_index + 2, injected);
    assert_eq!(
        ReplayGroupMoveSource::from_replay(&intervening, source.turn_index, source.player_index,),
        Err(GroupSimChannelError::UnownedSimBeforeChecksum {
            index: source.pair_command_index + 2,
            opcode: suffix_opcode,
        })
    );
}

#[test]
fn real_packet_executes_at_its_package_frame_and_cross_checks_two_walkers() {
    let replays = checksum_replays();
    if replays.is_empty() {
        eprintln!("\n  SKIPPED — NOT A PASS. No checksum-bearing retail corpus.\n");
        return;
    }
    let source = executable_source().expect("a real checksum-corpus pair must reach the host");
    eprintln!(
        "Groups checksum witness: {} turn {} play {} frame {} pair-index {} checksum-index {} inert={:02x?} sim-prefix={:02x?} sim-suffix={:02x?}",
        source.replay_path.display(),
        source.lockstep_serial,
        source.play,
        source.package_frame,
        source.pair_command_index,
        source.checksum_command_index,
        source.inert_opcodes,
        source.unowned_sim_prefix,
        source.unowned_sim_suffix,
    );
    let mut sim = sim_for(source).0;
    let receipt = issue_replay_group_move(&mut sim, source).unwrap();
    assert!(
        source.package_had_checksum,
        "the pinned transition must come from a package carrying opcode 0x39"
    );
    assert_eq!(receipt.before.checksum, CORPUS_INITIAL_GROUPS_CHANNEL);
    assert_eq!(receipt.before.bytes_walked, INITIAL_WALKED_BYTES);
    assert!(receipt.channel_changed());
    assert_eq!(receipt.host.frame, source.package_frame);
    assert_eq!(receipt.host.lockstep_serial, source.lockstep_serial);
    assert_eq!(receipt.host.play, source.play);
    assert_eq!(receipt.after.checksum, receipt.host.groups_checksum);
    assert_eq!(receipt.after.groups_walked, 512);
    assert!(receipt.after.bytes_walked > receipt.before.bytes_walked);
    assert_eq!(
        receipt.host.random_state_before,
        receipt.host.random_state_after
    );
    assert!(!receipt.installed_in_scoreboard());
}

#[test]
fn real_packet_commits_into_the_same_sim_owner_channel_five_walks() {
    let replays = checksum_replays();
    if replays.is_empty() {
        eprintln!("\n  SKIPPED — NOT A PASS. No checksum-bearing retail corpus.\n");
        return;
    }
    let source = executable_source().expect("a real checksum-corpus pair must reach the host");
    let (prepared, _) = sim_for(source);
    let mut harness = WorldSim::new();
    *harness.groups_sim_mut() = prepared;

    let receipt = issue_replay_group_move(harness.groups_sim_mut(), source).unwrap();
    harness.step_turn(0);

    let (channels, evidence) = harness.check_all_with_evidence();
    let group_evidence = evidence[Channel::Groups as usize];
    assert_eq!(channels.get(Channel::Groups), receipt.after.checksum);
    assert_eq!(
        sim_groups_checksum(harness.groups()).unwrap(),
        receipt.after
    );
    assert!(group_evidence.substantive());
    assert_eq!(group_evidence.bytes_walked, receipt.after.bytes_walked);
    assert_eq!(group_evidence.unsourced_walked, 0);
}

#[test]
fn live_owner_reprojects_the_scheduled_groups_process_tail() {
    let mut harness = WorldSim::new();
    harness.groups_sim_mut().leaders[0].active = true;
    harness.groups_sim_mut().groups.proc_group = 0;
    let group = &mut harness.groups_mut().list[0];
    group.who = 0;
    group.num = 1;
    group.list[0] = 3;
    group.speed = 123;
    group.new_speed = 123;
    harness.step_turn(0);
    let armed = sim_groups_checksum(harness.groups()).unwrap();
    assert_eq!(harness.groups().list[0].speed, 123);
    let frame = harness.groups_sim().world.frame;

    harness.step_turn(1);

    let scheduled = sim_groups_checksum(harness.groups()).unwrap();
    assert_eq!(harness.groups_sim().world.frame, frame.wrapping_add(1));
    assert_eq!(harness.groups().list[0].speed, 0);
    assert_eq!(harness.groups().list[0].new_speed, 0);
    assert_ne!(scheduled.checksum, armed.checksum);
    assert_eq!(
        harness.check_all().0.get(Channel::Groups),
        scheduled.checksum
    );
}

#[test]
fn frame_and_member_byte_mutations_fail_before_channel_mutation() {
    let replays = checksum_replays();
    if replays.is_empty() {
        eprintln!("\n  SKIPPED — NOT A PASS. No checksum-bearing retail corpus.\n");
        return;
    }
    let source = executable_source().unwrap();
    let mut wrong_frame = sim_for(source).0;
    wrong_frame.world.frame = wrong_frame.world.frame.wrapping_add(1);
    let baseline = sim_groups_checksum(&wrong_frame.groups).unwrap();
    assert_eq!(
        issue_replay_group_move(&mut wrong_frame, source),
        Err(GroupSimChannelError::FrameMismatch {
            sim_frame: source.package_frame.wrapping_add(1),
            package_frame: source.package_frame,
        })
    );
    assert_eq!(sim_groups_checksum(&wrong_frame.groups).unwrap(), baseline);

    let mutated_source = source.clone();
    let mut group = mutated_source.group_bytes().to_vec();
    assert!(group[1] > 0, "executable source has a selected member");
    group[3..5].copy_from_slice(&i16::MAX.to_le_bytes());
    // Rebuild through an exact package-shaped Replay source is intentionally impossible:
    // source bytes are private. Feed the mutation to the canonical host and prove its typed
    // object resolution refuses without moving the Groups image.
    let (mut sim, _) = sim_for(source);
    let mut bytes = group;
    bytes.extend_from_slice(mutated_source.move_bytes());
    let before = sim_groups_checksum(&sim.groups).unwrap();
    let refused = sim.process_command_package(source.play, source.lockstep_serial, &bytes);
    assert!(
        refused.is_err(),
        "mutating a replay-selected object to an absent slot must fail closed: {refused:?}"
    );
    assert_eq!(sim_groups_checksum(&sim.groups).unwrap(), before);
}

#[test]
fn every_walked_group_field_is_live_in_the_independent_post_image() {
    let replays = checksum_replays();
    if replays.is_empty() {
        eprintln!("\n  SKIPPED — NOT A PASS. No checksum-bearing retail corpus.\n");
        return;
    }
    let source = executable_source().unwrap();
    let mut sim = sim_for(source).0;
    let receipt = issue_replay_group_move(&mut sim, source).unwrap();
    let slot = receipt.host.group_slot;
    let baseline = receipt.after;
    assert!(sim.groups.list[slot].num > 0);

    macro_rules! kill_i32 {
        ($field:ident) => {{
            let mut groups = sim.groups.clone();
            groups.list[slot].$field = groups.list[slot].$field.wrapping_add(1);
            assert_ne!(
                sim_groups_checksum(&groups).unwrap().checksum,
                baseline.checksum,
                stringify!($field)
            );
        }};
    }
    for field in [
        "id",
        "army",
        "num",
        "form",
        "stamp",
        "ox",
        "oy",
        "o_dist",
        "o_angle",
        "disband",
        "order_num",
        "priority",
        "role",
        "think_frame",
        "new_speed",
        "speed",
        "form_num",
    ] {
        match field {
            "id" => kill_i32!(id),
            "army" => kill_i32!(army),
            // `num` controls dynamic lengths; keep it valid and supply a distinct complete
            // after-image by testing the existing member arrays at the shorter prefix.
            "num" => {
                let mut groups = sim.groups.clone();
                groups.list[slot].num -= 1;
                assert_ne!(
                    sim_groups_checksum(&groups).unwrap().checksum,
                    baseline.checksum,
                    "num"
                );
            }
            "form" => kill_i32!(form),
            "stamp" => kill_i32!(stamp),
            "ox" => kill_i32!(ox),
            "oy" => kill_i32!(oy),
            "o_dist" => kill_i32!(o_dist),
            "o_angle" => kill_i32!(o_angle),
            "disband" => kill_i32!(disband),
            "order_num" => kill_i32!(order_num),
            "priority" => kill_i32!(priority),
            "role" => kill_i32!(role),
            "think_frame" => kill_i32!(think_frame),
            "new_speed" => kill_i32!(new_speed),
            "speed" => kill_i32!(speed),
            "form_num" => kill_i32!(form_num),
            _ => unreachable!(),
        }
    }
    macro_rules! kill_u8 {
        ($field:ident) => {{
            let mut groups = sim.groups.clone();
            groups.list[slot].$field ^= 1;
            assert_ne!(
                sim_groups_checksum(&groups).unwrap().checksum,
                baseline.checksum,
                stringify!($field)
            );
        }};
    }
    kill_u8!(facing);
    kill_u8!(buildings);
    kill_u8!(who);
    kill_u8!(march);

    macro_rules! kill_member {
        ($field:ident) => {{
            let mut groups = sim.groups.clone();
            groups.list[slot].$field[0] = groups.list[slot].$field[0].wrapping_add(1);
            assert_ne!(
                sim_groups_checksum(&groups).unwrap().checksum,
                baseline.checksum,
                stringify!($field)
            );
        }};
    }
    kill_member!(list);
    kill_member!(off_x);
    kill_member!(off_y);
    kill_member!(curr_x);
    kill_member!(curr_y);
    kill_member!(angles);
    for index in 0..8 {
        let mut groups = sim.groups.clone();
        groups.last_group[index] = groups.last_group[index].wrapping_add(1);
        assert_ne!(
            sim_groups_checksum(&groups).unwrap().checksum,
            baseline.checksum,
            "last_group[{index}]"
        );
    }
}
