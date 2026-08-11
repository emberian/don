//! Dynamic replay Groups producer integration.

use don_replay::checksum::Channel;
use don_replay::groups_channel::{CORPUS_INITIAL_GROUPS_CHANNEL, INITIAL_WALKED_BYTES};
use don_replay::groups_dynamic::{
    DynamicGroupsError, DynamicGroupsProducer, GroupMemberFact,
    HUMAN_CORPUS_FIRST_GROUP_DELAY_TURNS,
};
use don_replay::replay::{corpus, OwnedCommand, Replay};
use don_sim::command::build;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn human_only_replays() -> Vec<Replay> {
    corpus(&repo_root())
        .into_iter()
        .filter_map(|path| Replay::open(&path).ok())
        .filter(|replay| {
            replay
                .turns
                .iter()
                .find_map(|turn| turn.any_checksums())
                .is_some_and(|(_, sums)| sums.get(Channel::Groups) == CORPUS_INITIAL_GROUPS_CHANNEL)
        })
        .collect()
}

#[test]
fn the_authoritative_command_pool_images_to_the_exact_retail_clear_channel() {
    let producer = DynamicGroupsProducer::new(16);
    let checksum = producer.checksum().unwrap();
    assert_eq!(producer.bridge().groups.slots().len(), 512);
    assert_eq!(checksum.groups_walked, 512);
    assert_eq!(checksum.bytes_walked, INITIAL_WALKED_BYTES);
    assert_eq!(checksum.checksum, CORPUS_INITIAL_GROUPS_CHANNEL);
}

#[test]
fn recorded_group_wire_reaches_one_exact_slot_in_member_order() {
    let mut producer = DynamicGroupsProducer::new(16);
    let command = build::group(1, &[3, 5, 4]);
    let facts = [
        GroupMemberFact::unit(1, 3, 103, 0x100),
        GroupMemberFact::unit(1, 5, 105, 0x40000),
        GroupMemberFact::unit(1, 4, 104, 0x20),
    ];
    let receipt = producer
        .apply_group_command(14, 91, 1, &command, &facts)
        .unwrap();
    assert_eq!(
        receipt.group_slot, 65,
        "owner 1's base+1 is the first open slot"
    );
    assert_eq!(receipt.requested, vec![3, 5, 4]);
    assert_eq!(receipt.selected, receipt.requested);
    assert_eq!(receipt.before.checksum, CORPUS_INITIAL_GROUPS_CHANNEL);
    assert_ne!(receipt.after.checksum, receipt.before.checksum);
    // One populated three-member slot adds list 3*2, four i32 planes 4*(3*4), and
    // angles 3 to the fixed image.
    assert_eq!(receipt.after.bytes_walked, INITIAL_WALKED_BYTES + 57);

    let group = producer.bridge().groups.get(65).unwrap();
    assert_eq!(group.id, 65);
    assert_eq!(group.army, -1);
    assert_eq!(group.form, -1);
    assert_eq!(group.stamp, 91);
    assert_eq!(group.role, 0x4_0120);
    assert_eq!(group.who, 1);
    assert_eq!(&group.list[..3], &[3, 5, 4]);
    assert_eq!(&group.off_x[..3], &[0, 0, 0]);

    let mut reversed = DynamicGroupsProducer::new(16);
    let reversed_receipt = reversed
        .apply_group_command(14, 91, 1, &build::group(1, &[4, 5, 3]), &facts)
        .unwrap();
    assert_ne!(
        receipt.after.checksum, reversed_receipt.after.checksum,
        "Group::walk_data hashes the wire member order; a set projection would be wrong"
    );
}

#[test]
fn a_missing_object_fact_and_an_unsupported_package_tail_roll_back() {
    let mut producer = DynamicGroupsProducer::new(16);
    let initial = producer.checksum().unwrap();
    let command = build::group(0, &[1, 2]);
    let one_fact = [GroupMemberFact::unit(0, 1, 11, 0x40300)];
    assert_eq!(
        producer.apply_group_command(8, 55, 0, &command, &one_fact),
        Err(DynamicGroupsError::MissingMemberFact { who: 0, o: 2 })
    );
    assert_eq!(producer.checksum().unwrap(), initial);

    let commands = [
        OwnedCommand {
            opcode: 0,
            bytes: command,
        },
        OwnedCommand {
            opcode: 25,
            bytes: vec![25; 25],
        },
    ];
    let facts = [
        GroupMemberFact::unit(0, 1, 11, 0x40300),
        GroupMemberFact::unit(0, 2, 12, 0x40300),
    ];
    assert_eq!(
        producer.apply_recorded_package(8, 55, 0, &commands, &facts),
        Err(DynamicGroupsError::UnsupportedSimTail { opcode: 25 })
    );
    assert_eq!(producer.checksum().unwrap(), initial);
}

#[test]
fn empty_reselection_is_uid_guarded_and_preserves_old_group_order() {
    let mut producer = DynamicGroupsProducer::new(8);
    let first_facts = [
        GroupMemberFact::unit(0, 1, 10, 0x40300),
        GroupMemberFact::unit(0, 2, 20, 0x40300),
    ];
    let first = producer
        .apply_group_command(1, 10, 0, &build::group(0, &[1, 2]), &first_facts)
        .unwrap();
    assert_eq!(first.group_slot, 1);

    let recycled = [
        GroupMemberFact::unit(0, 1, 10, 0x40300),
        GroupMemberFact::unit(0, 2, 99, 0x40300),
    ];
    let second = producer
        .apply_group_command(2, 20, 0, &build::group(0, &[]), &recycled)
        .unwrap();
    assert_eq!(second.selected, vec![1]);
    assert_eq!(second.group_slot, 0);
    assert_eq!(producer.bridge().groups.get(0).unwrap().list[0], 1);
    assert_eq!(producer.bridge().groups.get(1).unwrap().list[0], 2);
}

/// Corpus boundary, not a fitted producer: each first mutation is two turns after an
/// opcode-0 package, and each of those packages has a Sim action tail. Until that tail
/// and the starter-object facts have owners, the whole-package API must refuse all seven
/// before mutation.
#[test]
fn all_seven_human_recordings_reach_the_same_typed_dynamic_frontier() {
    let replays = human_only_replays();
    if replays.is_empty() {
        eprintln!("\n  SKIPPED — NOT A PASS. No local checksum corpus.\n");
        return;
    }
    assert_eq!(replays.len(), 7);
    for replay in replays {
        let first_changed = replay
            .turns
            .iter()
            .find_map(|turn| {
                turn.any_checksums().and_then(|(_, sums)| {
                    (sums.get(Channel::Groups) != CORPUS_INITIAL_GROUPS_CHANNEL)
                        .then_some((turn.turn, sums.get(Channel::Groups)))
                })
            })
            .unwrap();
        let (issued_turn, player) = replay
            .turns
            .iter()
            .flat_map(|turn| turn.players.iter().map(move |player| (turn.turn, player)))
            .find(|(turn, player)| {
                *turn < first_changed.0 && player.commands.iter().any(|command| command.opcode == 0)
            })
            .unwrap();
        assert_eq!(
            first_changed.0 - issued_turn,
            HUMAN_CORPUS_FIRST_GROUP_DELAY_TURNS,
            "{}",
            replay.path.display()
        );
        let tail = player
            .commands
            .iter()
            .find(|command| {
                command.opcode != 0
                    && don_replay::wire::classify(command.opcode)
                        == don_replay::wire::CommandClass::Sim
            })
            .expect("the first selection has an action tail");
        let mut producer = DynamicGroupsProducer::new(4096);
        let before = producer.checksum().unwrap();
        assert_eq!(
            producer.apply_recorded_package(
                issued_turn,
                player.stamp as i32,
                player.play,
                &player.commands,
                &[],
            ),
            Err(DynamicGroupsError::UnsupportedSimTail {
                opcode: tail.opcode
            }),
            "{}",
            replay.path.display()
        );
        assert_eq!(producer.checksum().unwrap(), before);
    }
}
