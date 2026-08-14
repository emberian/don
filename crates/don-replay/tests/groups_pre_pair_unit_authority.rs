//! Corpus census for the smallest clear-pool Group -> Move checksum transition.

use don_replay::checksum::Channel;
use don_replay::groups_channel::CORPUS_INITIAL_GROUPS_CHANNEL;
use don_replay::groups_sim_channel::ReplayGroupMoveSource;
use don_replay::initial::SHIPPED_TYPES_SERIALIZED_BYTES;
use don_replay::replay::{load_payload, Replay};
use don_replay::wire::{classify, CommandClass};
use don_sim::command::tail_command_transactions::adjacent::{
    decode_leader_options, LEADER_OPTIONS_OPCODE,
};
use don_sim::systems::canonical_group_move_host::{
    decode_group_move_package, GROUP_OPCODE, MOVE_TO_OPCODE,
};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

// Compile the exclusive source path directly as well as through its eventual lib hook.  These
// aliases preserve its `crate::...` imports when a clean remote baseline predates that hook.
mod initial {
    pub use don_replay::initial::*;
}
mod rules_channel {
    pub use don_replay::rules_channel::*;
}
mod world_owner_frontier {
    pub use don_replay::world_owner_frontier::*;
}
#[path = "../src/groups_pre_pair_unit_authority.rs"]
mod groups_pre_pair_unit_authority;
use groups_pre_pair_unit_authority::{
    replay_golden_counterintel_rules, replay_spell_range_constants, replay_spell_type_facts,
    replay_tribe_type_facts, replay_unit_type_facts, PrePairUnitAuthorityError, FORM_TYPE_CAT_VA,
    LEADER_HAS_TRIBE_BONUS_VA, SETUP_BUILD_UNITS_VA, UNIT_IS_MODERN_INFANTRY_VA,
};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

#[test]
fn census_first_clear_pool_group_move_witness() {
    assert_eq!(
        (
            SETUP_BUILD_UNITS_VA,
            LEADER_HAS_TRIBE_BONUS_VA,
            FORM_TYPE_CAT_VA,
            UNIT_IS_MODERN_INFANTRY_VA,
        ),
        (0x005a_afc0, 0x006e_1370, 0x0072_dfc0, 0x0060_7b40)
    );
    let path = repo_root().join("ron-data/replays/multi/Playback___2024.02.23_20_49_35__Fri_.rcx");
    let Ok(replay) = Replay::open(&path) else {
        eprintln!("SKIPPED -- NOT A PASS: missing {}", path.display());
        return;
    };
    eprintln!(
        "settings seed={:#010x} map={} size={} town={} resources={} reveal={} players={:?}",
        replay.initial.info.seed,
        replay.initial.info.settings.map_style,
        replay.initial.info.settings.map_size,
        replay.initial.info.settings.starting_town,
        replay.initial.info.settings.starting_resources,
        replay.initial.info.settings.reveal_map,
        replay
            .initial
            .info
            .players
            .iter()
            .filter(|player| player.present)
            .map(|player| (
                player.slot,
                player.play,
                player.who,
                player.tribe,
                player.team,
                player.difficulty,
                player.name.as_str(),
            ))
            .collect::<Vec<_>>()
    );

    let rules = replay
        .initial
        .rules
        .expect("the witness carries the exact shipped Rules section");
    let payload = load_payload(&path).unwrap();
    let constants_offset = rules.serialized_offset + 1 + SHIPPED_TYPES_SERIALIZED_BYTES;
    let bonus_scouts = i32::from_le_bytes(
        payload[constants_offset + 0x680..constants_offset + 0x684]
            .try_into()
            .unwrap(),
    );
    let tribe_index = replay
        .initial
        .info
        .players
        .iter()
        .find(|player| player.present && player.who == 0)
        .unwrap()
        .tribe as usize;
    let citizen_tribe = replay_tribe_type_facts(&payload, &rules, tribe_index, 50).unwrap();
    let merchant_tribe = replay_tribe_type_facts(&payload, &rules, tribe_index, 62).unwrap();
    let scout_tribe = replay_tribe_type_facts(&payload, &rules, tribe_index, 69).unwrap();
    let citizen = replay_unit_type_facts(&payload, &rules, 50).unwrap();
    let scout = replay_unit_type_facts(&payload, &rules, 69).unwrap();
    let counterintel = replay_spell_type_facts(&payload, &rules, 631).unwrap();
    let spell_range = replay_spell_range_constants(&payload, &rules).unwrap();
    let golden_counterintel = replay_golden_counterintel_rules(
        &payload,
        &rules,
        don_sim::systems::frame0_scout_spellcaster::GOLDEN_REPLAY_FILE_SHA256,
    )
    .unwrap();
    eprintln!(
        "setup sources tribe-index={tribe_index} tribe-id={} bonus-scouts={bonus_scouts} citizen-graft={} merchant-graft={} scout-graft={} citizen={citizen:#?}",
        citizen_tribe.tribe_id,
        citizen_tribe.nation_variant,
        merchant_tribe.nation_variant,
        scout_tribe.nation_variant,
    );
    assert_eq!(
        (
            citizen_tribe.tribe_id,
            bonus_scouts,
            citizen_tribe.nation_variant,
            merchant_tribe.nation_variant,
            scout_tribe.nation_variant,
        ),
        (22, 1, 50, 62, 69)
    );
    assert_eq!(
        (
            citizen.type_index,
            citizen.obj_masks,
            citizen.attack,
            citizen.max_range,
            citizen.domain,
            citizen.guy_spacing,
            citizen.x_spacing,
            citizen.y_spacing,
            citizen.unit_flags,
            citizen.unit_flags2,
            citizen.moves,
            citizen.role,
        ),
        (50, 4_227_108, 40, 0, 0, 144, 144, 144, 6_273, 2, 25, 262_912,)
    );
    assert_eq!(citizen.control_cost, 1);
    assert_eq!(scout.mana, 500);
    assert_eq!(
        (
            counterintel.type_index,
            counterintel.from_type,
            counterintel.from2,
            counterintel.mana,
            counterintel.spell_range,
            counterintel.spell_flags,
            spell_range.spy_bribe_upgrade_range,
            spell_range.terra_cotta_range,
        ),
        (631, 58, 69, 500, 1_920, 0x10B6, 2, 0)
    );
    assert_eq!(golden_counterintel.unit_type_mana, scout.mana);
    assert_eq!(golden_counterintel.spell_type, counterintel.type_index);
    assert_eq!(
        golden_counterintel.serialized_rules_sha256,
        rules.serialized_sha256
    );
    assert_eq!(
        replay_golden_counterintel_rules(&payload, &rules, [0; 32]),
        Err(PrePairUnitAuthorityError::WrongGoldenReplayFile)
    );
    assert_eq!(
        (
            citizen.squad_size,
            citizen.uber_size,
            citizen.crew_size,
            citizen.base_form,
        ),
        (1, 1, 0, 0)
    );

    let mut mutated_payload = payload.clone();
    mutated_payload[citizen.spans.unit.offset + 20] ^= 1; // UnitTypeData::role +0x2c8.
    assert_eq!(
        replay_unit_type_facts(&mutated_payload, &rules, 50),
        Err(PrePairUnitAuthorityError::RulesSha256Mismatch),
        "a content mutation must fail at the admitted Rules provenance boundary"
    );

    let mut prior_group_commands = 0usize;
    let mut prior_sim_commands = BTreeMap::<u8, usize>::new();
    let mut prior_leader_options = Vec::new();
    let mut witness = None;
    for (turn_index, turn) in replay.turns.iter().enumerate() {
        for (player_index, player) in turn.players.iter().enumerate() {
            let pair = player.commands.windows(2).position(|commands| {
                commands[0].opcode == GROUP_OPCODE && commands[1].opcode == MOVE_TO_OPCODE
            });
            if let Some(pair_index) = pair {
                for command in player.commands.iter().take(pair_index) {
                    if classify(command.opcode) == CommandClass::Sim {
                        *prior_sim_commands.entry(command.opcode).or_default() += 1;
                    }
                    if command.opcode == LEADER_OPTIONS_OPCODE {
                        prior_leader_options.push((
                            turn.turn,
                            player.stamp,
                            player.play,
                            decode_leader_options(&command.bytes).unwrap(),
                        ));
                    }
                }
                witness = Some((turn_index, player_index, pair_index));
                break;
            }
            prior_group_commands += player
                .commands
                .iter()
                .filter(|command| command.opcode == GROUP_OPCODE)
                .count();
            for command in &player.commands {
                if classify(command.opcode) == CommandClass::Sim {
                    *prior_sim_commands.entry(command.opcode).or_default() += 1;
                }
                if command.opcode == LEADER_OPTIONS_OPCODE {
                    prior_leader_options.push((
                        turn.turn,
                        player.stamp,
                        player.play,
                        decode_leader_options(&command.bytes).unwrap(),
                    ));
                }
            }
        }
        if witness.is_some() {
            break;
        }
    }
    let (turn_index, player_index, pair_index) = witness.expect("real Group -> Move witness");
    assert_eq!(
        prior_group_commands, 0,
        "pre-pair Groups must still be clear"
    );
    eprintln!("conservatively classified prior Sim commands={prior_sim_commands:02x?}");
    eprintln!("prior leader options={prior_leader_options:#?}");
    let turn = &replay.turns[turn_index];
    let player = &turn.players[player_index];
    let mut bytes = player.commands[pair_index].bytes.clone();
    bytes.extend_from_slice(&player.commands[pair_index + 1].bytes);
    let wire = decode_group_move_package(&bytes).unwrap();
    let sim_shell: Vec<_> = player
        .commands
        .iter()
        .filter(|command| classify(command.opcode) == CommandClass::Sim)
        .map(|command| command.opcode)
        .collect();
    eprintln!(
        "witness turn={} stamp={} play={} who={} objects={:?} movement={:?} shell={:02x?}",
        turn.turn, player.stamp, player.play, wire.who, wire.objects, wire.movement, sim_shell,
    );
    assert_eq!(wire.objects, [3, 4, 5, 6]);

    let checksums: Vec<_> = replay
        .turns
        .iter()
        .filter_map(|candidate| {
            candidate.any_checksums().map(|(_, sums)| {
                (
                    candidate.turn,
                    sums.get(Channel::Groups),
                    sums.get(Channel::Units),
                )
            })
        })
        .filter(|(serial, _, _)| *serial >= turn.turn - 2 && *serial <= turn.turn + 4)
        .collect();
    eprintln!("nearby checksums={checksums:08x?}");
    let first_changed = replay
        .turns
        .iter()
        .filter_map(|candidate| {
            candidate.any_checksums().map(|(_, sums)| {
                (
                    candidate.turn,
                    sums.get(Channel::Groups),
                    sums.get(Channel::Units),
                )
            })
        })
        .find(|(_, groups, _)| *groups != CORPUS_INITIAL_GROUPS_CHANNEL)
        .unwrap();
    assert_eq!(first_changed.0, turn.turn + 2);
    eprintln!("first changed checksum={first_changed:08x?}");
}

#[test]
fn census_same_package_checksum_adapter_witness() {
    let path = repo_root().join("ron-data/replays/multi/Playback___2018.11.17_13_21_42__Sat_.rcx");
    let Ok(replay) = Replay::open(&path) else {
        eprintln!("SKIPPED -- NOT A PASS: missing {}", path.display());
        return;
    };
    let rules = replay
        .initial
        .rules
        .expect("same-package witness carries shipped Rules");
    let payload = load_payload(&path).unwrap();
    let who_zero = replay
        .initial
        .info
        .players
        .iter()
        .find(|player| player.present && player.who == 0)
        .unwrap();
    let scout_tribe =
        replay_tribe_type_facts(&payload, &rules, who_zero.tribe as usize, 69).unwrap();
    let scout = replay_unit_type_facts(&payload, &rules, scout_tribe.nation_variant).unwrap();
    let mut prior_groups = Vec::new();
    let mut source = None;
    'turns: for (turn_index, turn) in replay.turns.iter().enumerate() {
        for (player_index, player) in turn.players.iter().enumerate() {
            if let Ok(candidate) =
                ReplayGroupMoveSource::from_replay(&replay, turn_index, player_index)
            {
                source = Some(candidate);
                break 'turns;
            }
            for (command_index, command) in player.commands.iter().enumerate() {
                if command.opcode == GROUP_OPCODE {
                    prior_groups.push((
                        turn.turn,
                        player.stamp,
                        player.play,
                        command_index,
                        command.bytes.clone(),
                    ));
                }
            }
        }
    }
    let source = source.expect("pinned same-package checksum witness");
    let wire = decode_group_move_package(&source.command_bytes()).unwrap();
    assert_eq!(
        (
            source.lockstep_serial,
            source.package_frame,
            source.play,
            source.pair_command_index,
            source.checksum_command_index,
            wire.who,
        ),
        (48, 259, 1, 0, 2, 0)
    );
    assert_eq!(wire.objects, [0]);
    assert_eq!(prior_groups.len(), 8);
    assert_eq!(
        (
            scout_tribe.tribe_id,
            scout_tribe.nation_variant,
            scout.type_index,
            scout.upgrade,
            scout.jump,
            scout.squad_size,
            scout.uber_size,
            scout.crew_size,
        ),
        (14, 69, 69, 71, 71, 1, 1, 1)
    );
    eprintln!(
        "same-package settings seed={:#010x} map={} size={} town={} resources={} reveal={} players={:?}",
        replay.initial.info.seed,
        replay.initial.info.settings.map_style,
        replay.initial.info.settings.map_size,
        replay.initial.info.settings.starting_town,
        replay.initial.info.settings.starting_resources,
        replay.initial.info.settings.reveal_map,
        replay
            .initial
            .info
            .players
            .iter()
            .filter(|player| player.present)
            .map(|player| (player.slot, player.play, player.who, player.tribe, player.team, player.difficulty, player.name.as_str()))
            .collect::<Vec<_>>(),
    );
    eprintln!("same-package base-scout tribe={scout_tribe:#?} content={scout:#?}");
    eprintln!(
        "same-package turn={} frame={} play={} pair={} checksum={} who={} objects={:?} movement={:?} prefix={:02x?} suffix={:02x?}",
        source.lockstep_serial,
        source.package_frame,
        source.play,
        source.pair_command_index,
        source.checksum_command_index,
        wire.who,
        wire.objects,
        wire.movement,
        source.unowned_sim_prefix,
        source.unowned_sim_suffix,
    );
    eprintln!("prior Group commands={prior_groups:02x?}");
}
