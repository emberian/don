use std::path::{Path, PathBuf};

use don_replay::groups_pre_pair_unit_authority::replay_unit_type_facts;
use don_replay::replay::load_payload;
use don_replay::replay::Replay;
use don_replay::setup_2024_frame1_leader_options::{
    discover_frame1_leader_options_source, plan_frame1_setup_leader_options,
    retail_initial_leader_options, unit_type_stance_type, Frame1LeaderOptionsError,
    Frame1StanceTarget, LEADER_OPTIONS_FRAME, LEADER_OPTIONS_SERIAL,
};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn installed_replay() -> Option<Replay> {
    let path = repo_root().join("ron-data/replays/multi/Playback___2024.02.23_20_49_35__Fri_.rcx");
    if !path.exists() {
        eprintln!("SKIPPED -- NOT A PASS: missing {}", path.display());
        return None;
    }
    Some(Replay::open(&path).unwrap())
}

#[test]
fn target_pre_frame379_sim_schedule_and_reached_tail_are_exact() {
    let Some(replay) = installed_replay() else {
        return;
    };
    let source = discover_frame1_leader_options_source(&replay).unwrap();
    assert_eq!(
        source
            .commands
            .iter()
            .map(|command| (command.lockstep_serial, command.frame, command.play))
            .collect::<Vec<_>>(),
        [
            (LEADER_OPTIONS_SERIAL, LEADER_OPTIONS_FRAME, 0),
            (LEADER_OPTIONS_SERIAL, LEADER_OPTIONS_FRAME, 1),
        ]
    );
    assert_eq!((source.next_sim_serial, source.next_sim_frame), (64, 379));

    let plan = plan_frame1_setup_leader_options(&replay).unwrap();
    assert_eq!(plan.rows_before, retail_initial_leader_options());
    assert_eq!(plan.setup_type_stance, [(69, 2), (62, -1), (50, 1)]);
    assert_eq!(plan.writes.len(), 5);
    assert_eq!(
        plan.writes
            .iter()
            .map(|write| (write.who, write.o, write.type_index, write.value))
            .collect::<Vec<_>>(),
        [
            (0, 3, 50, 1),
            (0, 4, 50, 1),
            (0, 5, 50, 1),
            (0, 6, 50, 1),
            (0, 2_000, 414, 1),
        ]
    );
    assert!(matches!(
        plan.writes.last().unwrap().target,
        Frame1StanceTarget::StartingVillage
    ));
    assert!(plan.local_mirror_excluded);
    assert_eq!(plan.rows_after[0].peasants, 1);
    assert_eq!(plan.rows_after[0].buildings, 2);
    assert_eq!(plan.rows_after[0].flags.inline, [0x0b, 0, 0, 0]);
    assert_eq!(plan.rows_after[1].peasants, 1);
    assert_eq!(plan.rows_after[1].buildings, 0);
    assert_eq!(plan.rows_after[1].flags.inline, [0x2a, 0, 0, 0]);
}

#[test]
fn source_gate_bites_wire_and_inventory_mutations() {
    let Some(replay) = installed_replay() else {
        return;
    };
    let source = discover_frame1_leader_options_source(&replay).unwrap();

    let mut damaged = replay.clone();
    damaged.turns[source.commands[0].turn_index].players[source.commands[0].player_index]
        .commands[source.commands[0].command_index]
        .bytes[29] ^= 1;
    assert_eq!(
        discover_frame1_leader_options_source(&damaged),
        Err(Frame1LeaderOptionsError::WrongSourceWire)
    );

    let mut duplicated = replay.clone();
    let command = duplicated.turns[source.commands[0].turn_index].players
        [source.commands[0].player_index]
        .commands[0]
        .clone();
    duplicated.turns[source.commands[0].turn_index].players[source.commands[0].player_index]
        .commands
        .insert(1, command);
    assert_eq!(
        discover_frame1_leader_options_source(&duplicated),
        Err(Frame1LeaderOptionsError::WrongPrePairSimInventory)
    );
}

#[test]
fn stance_type_projection_bites_each_reached_rules_gate() {
    let Some(replay) = installed_replay() else {
        return;
    };
    let payload = load_payload(&replay.path).unwrap();
    let rules = replay.initial.rules.as_ref().unwrap();
    let scout = replay_unit_type_facts(&payload, rules, 69).unwrap();
    let merchant = replay_unit_type_facts(&payload, rules, 62).unwrap();
    let citizen = replay_unit_type_facts(&payload, rules, 50).unwrap();
    assert_eq!(
        [
            unit_type_stance_type(&scout),
            unit_type_stance_type(&merchant),
            unit_type_stance_type(&citizen),
        ],
        [2, -1, 1]
    );

    let mut role = citizen;
    role.role |= 0x1_0000;
    assert_eq!(unit_type_stance_type(&role), 0);
    role.unit_flags2 |= 4;
    assert_eq!(unit_type_stance_type(&role), 3);

    let mut flags = scout;
    flags.unit_flags2 &= !6;
    assert_eq!(unit_type_stance_type(&flags), -1);
}
