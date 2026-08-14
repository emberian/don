//! Exact replay source and synchronized mutation plan for the two frame-1 LeaderOptions rows.
//!
//! The selected 2024 replay has only two simulation commands before its first Group mutation,
//! both opcode 73 at live frame 1.  This module validates that complete pre-pair inventory,
//! executes the recovered row-store/change-gate prefix from retail's exact constructor image,
//! and resolves every synchronized object cascade reached by the seven-unit Dutch setup.
//!
//! It deliberately does not advance a [`don_sim::tick::Sim`] from frame zero to frame one and
//! does not publish a frame-379 chronology authority.  The returned stance writes are the exact
//! handoff for that future transaction, not permission to relabel a setup snapshot's frame.

#![forbid(unsafe_code)]

use std::fmt;

use don_sim::command::tail_command_transactions::adjacent::{
    decode_leader_options, plan_leader_options_prefix, AdjacentOpenTailRequest, BitMask32State,
    LeaderOptionChangeSet, LeaderOptionDataState, LeaderOptionRowReceipt, LeaderOptionsCommand,
    LeaderOptionsPrefixError, LEADER_OPTIONS_OPCODE, LEADER_OPTION_ROWS,
};

use crate::groups_pre_pair_unit_authority::{
    replay_unit_type_facts, PrePairUnitAuthorityError, ReplayUnitTypeFacts,
};
use crate::replay::{load_payload, Replay};
use crate::setup_2024_frame379::{GROUP_MOVE_FRAME, GROUP_MOVE_SERIAL, REPLAY_FILE_SHA256};
use crate::setup_2024_frame379_group_move::discover_frame379_group_move_source;
use crate::wire::{classify, CommandClass};
use crate::world_owner_frontier::sha256;

pub const LEADER_OPTIONS_SERIAL: i32 = 1;
pub const LEADER_OPTIONS_FRAME: u32 = 1;
pub const LEADER_OPTIONS_SHELL: [u8; 4] = [0x49, 0x4a, 0x4a, 0x48];
pub const SETUP_SCOUT_TYPE: i32 = 69;
pub const SETUP_MERCHANT_TYPE: i32 = 62;
pub const SETUP_CITIZEN_TYPE: i32 = 50;
pub const STARTING_VILLAGE_TYPE: i32 = 414;

pub const PLAYER_ZERO_WIRE: [u8; 33] = [
    0x49, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00,
    0x00, 0x20, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x0b, 0x00, 0x00,
    0x00,
];
pub const PLAYER_ONE_WIRE: [u8; 33] = [
    0x49, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x20, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x2a, 0x00, 0x00,
    0x00,
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame1LeaderOptionsCommandSource {
    pub turn_index: usize,
    pub player_index: usize,
    pub command_index: usize,
    pub lockstep_serial: i32,
    pub frame: u32,
    pub play: i32,
    pub command: LeaderOptionsCommand,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame1LeaderOptionsSourceReceipt {
    pub replay_file_sha256: [u8; 32],
    pub replay_payload_sha256: [u8; 32],
    pub commands: [Frame1LeaderOptionsCommandSource; 2],
    pub next_sim_serial: i32,
    pub next_sim_frame: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame1StanceTarget {
    SetupUnit { setup_ordinal: usize },
    StartingVillage,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame1StanceWrite {
    pub who: i32,
    pub o: i32,
    pub type_index: i32,
    pub stance_type: i32,
    pub value: i8,
    pub target: Frame1StanceTarget,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame1SetupLeaderOptionsPlan {
    pub source: Frame1LeaderOptionsSourceReceipt,
    pub rows_before: [LeaderOptionDataState; LEADER_OPTION_ROWS],
    pub rows_after: [LeaderOptionDataState; LEADER_OPTION_ROWS],
    pub command_changes: [LeaderOptionChangeSet; 2],
    pub setup_type_stance: [(i32, i32); 3],
    pub writes: Vec<Frame1StanceWrite>,
    /// `MiscAccess::my_leader_option` is local presentation state and is not synchronized.
    pub local_mirror_excluded: bool,
    pub next_exact_boundary: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame1LeaderOptionsError {
    ReplayRead(String),
    WrongReplayFile,
    MissingRules,
    PayloadRead(String),
    WrongPrePairSimInventory,
    WrongSourceCoordinates,
    WrongSourceShell,
    WrongSourceWire,
    MissingFrame379Pair,
    Prefix(LeaderOptionsPrefixError),
    PrefixDidNotReachCascade,
    WrongChangeSet,
    TypeFacts(PrePairUnitAuthorityError),
    WrongSetupTypeFacts,
}

impl fmt::Display for Frame1LeaderOptionsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "2024 frame-1 LeaderOptions plan refused: {self:?}")
    }
}

impl std::error::Error for Frame1LeaderOptionsError {}

impl From<LeaderOptionsPrefixError> for Frame1LeaderOptionsError {
    fn from(value: LeaderOptionsPrefixError) -> Self {
        Self::Prefix(value)
    }
}

impl From<PrePairUnitAuthorityError> for Frame1LeaderOptionsError {
    fn from(value: PrePairUnitAuthorityError) -> Self {
        Self::TypeFacts(value)
    }
}

/// Retail `LeaderOptions::init` `0x006F1D40` after all ten `LeaderOption` constructors.
pub fn retail_initial_leader_options() -> [LeaderOptionDataState; LEADER_OPTION_ROWS] {
    std::array::from_fn(|who| LeaderOptionDataState {
        who: who as i32,
        peasants: 0,
        peasants_wait: 2,
        buildings: 0,
        flags: BitMask32State {
            bits: 32,
            size: 4,
            flags: 0,
            inline: [0x0a, 0, 0, 0],
        },
    })
}

/// `UnitTypeData::get_stance_type` `0x0061D350`, over replay-carried Rules fields.
pub fn unit_type_stance_type(facts: &ReplayUnitTypeFacts) -> i32 {
    if facts.role & 0x1_0000 != 0 {
        return if facts.unit_flags2 & 4 != 0 { 3 } else { 0 };
    }
    if (50..=53).contains(&facts.type_index) {
        return 1;
    }
    if facts.unit_flags2 & 6 == 2 {
        2
    } else {
        -1
    }
}

/// Pin the complete Rust-classified simulation inventory before serial 64's Group+Move pair.
pub fn discover_frame1_leader_options_source(
    replay: &Replay,
) -> Result<Frame1LeaderOptionsSourceReceipt, Frame1LeaderOptionsError> {
    let raw = std::fs::read(&replay.path)
        .map_err(|error| Frame1LeaderOptionsError::ReplayRead(error.to_string()))?;
    let replay_file_sha256 = sha256(&raw);
    if replay_file_sha256 != REPLAY_FILE_SHA256 {
        return Err(Frame1LeaderOptionsError::WrongReplayFile);
    }
    let group_move = discover_frame379_group_move_source(replay)
        .map_err(|_| Frame1LeaderOptionsError::MissingFrame379Pair)?;
    if group_move.lockstep_serial != GROUP_MOVE_SERIAL
        || group_move.package_frame != GROUP_MOVE_FRAME
    {
        return Err(Frame1LeaderOptionsError::MissingFrame379Pair);
    }

    let mut sources = Vec::new();
    for (turn_index, turn) in replay.turns.iter().enumerate() {
        for (player_index, player) in turn.players.iter().enumerate() {
            if i32::try_from(player.stamp)
                .ok()
                .is_some_and(|frame| frame >= GROUP_MOVE_FRAME)
            {
                continue;
            }
            for (command_index, command) in player.commands.iter().enumerate() {
                if classify(command.opcode) != CommandClass::Sim {
                    continue;
                }
                if command.opcode != LEADER_OPTIONS_OPCODE {
                    return Err(Frame1LeaderOptionsError::WrongPrePairSimInventory);
                }
                let decoded = decode_leader_options(&command.bytes)
                    .map_err(|_| Frame1LeaderOptionsError::WrongSourceWire)?;
                sources.push(Frame1LeaderOptionsCommandSource {
                    turn_index,
                    player_index,
                    command_index,
                    lockstep_serial: turn.turn,
                    frame: player.stamp,
                    play: player.play,
                    command: decoded,
                });
            }
        }
    }
    if sources.len() != 2 {
        return Err(Frame1LeaderOptionsError::WrongPrePairSimInventory);
    }
    sources.sort_by_key(|source| source.play);
    for (index, source) in sources.iter().enumerate() {
        if source.lockstep_serial != LEADER_OPTIONS_SERIAL
            || source.frame != LEADER_OPTIONS_FRAME
            || source.play != index as i32
            || source.command_index != 0
        {
            return Err(Frame1LeaderOptionsError::WrongSourceCoordinates);
        }
        let player = &replay.turns[source.turn_index].players[source.player_index];
        if player
            .commands
            .iter()
            .map(|command| command.opcode)
            .ne(LEADER_OPTIONS_SHELL)
        {
            return Err(Frame1LeaderOptionsError::WrongSourceShell);
        }
        let expected = if index == 0 {
            PLAYER_ZERO_WIRE.as_slice()
        } else {
            PLAYER_ONE_WIRE.as_slice()
        };
        if player.commands[source.command_index].bytes != expected {
            return Err(Frame1LeaderOptionsError::WrongSourceWire);
        }
    }
    let commands: [Frame1LeaderOptionsCommandSource; 2] = sources
        .try_into()
        .expect("the exact two-source count was checked");
    Ok(Frame1LeaderOptionsSourceReceipt {
        replay_file_sha256,
        replay_payload_sha256: replay.initial.payload_sha256,
        commands,
        next_sim_serial: group_move.lockstep_serial,
        next_sim_frame: group_move.package_frame,
    })
}

fn cascade_changes(
    command: &LeaderOptionsCommand,
    rows: &mut [LeaderOptionDataState; LEADER_OPTION_ROWS],
) -> Result<LeaderOptionChangeSet, Frame1LeaderOptionsError> {
    let who = usize::try_from(command.data.who)
        .ok()
        .filter(|&who| who < LEADER_OPTION_ROWS)
        .ok_or(Frame1LeaderOptionsError::WrongSourceWire)?;
    // The local mirror is intentionally outside synchronized state. Passing -1 preserves all
    // simulation branch gates while preventing that presentation-only write from masquerading
    // as a Sim mutation.
    let plan = plan_leader_options_prefix(
        command,
        LeaderOptionRowReceipt {
            who: who as i32,
            state: rows[who],
        },
        -1,
    )?;
    let Some(AdjacentOpenTailRequest::LeaderOptionsCascade(cascade)) = plan.open_tail else {
        return Err(Frame1LeaderOptionsError::PrefixDidNotReachCascade);
    };
    rows[who] = plan.stored;
    Ok(cascade.changes)
}

/// Produce the exact synchronized writes reached by the selected setup cohort at frame 1.
pub fn plan_frame1_setup_leader_options(
    replay: &Replay,
) -> Result<Frame1SetupLeaderOptionsPlan, Frame1LeaderOptionsError> {
    let source = discover_frame1_leader_options_source(replay)?;
    let payload = load_payload(&replay.path)
        .map_err(|error| Frame1LeaderOptionsError::PayloadRead(error.to_string()))?;
    if sha256(&payload) != replay.initial.payload_sha256 {
        return Err(Frame1LeaderOptionsError::WrongReplayFile);
    }
    let rules = replay
        .initial
        .rules
        .ok_or(Frame1LeaderOptionsError::MissingRules)?;
    let scout = replay_unit_type_facts(&payload, &rules, SETUP_SCOUT_TYPE)?;
    let merchant = replay_unit_type_facts(&payload, &rules, SETUP_MERCHANT_TYPE)?;
    let citizen = replay_unit_type_facts(&payload, &rules, SETUP_CITIZEN_TYPE)?;
    let setup_type_stance = [
        (scout.type_index, unit_type_stance_type(&scout)),
        (merchant.type_index, unit_type_stance_type(&merchant)),
        (citizen.type_index, unit_type_stance_type(&citizen)),
    ];
    if setup_type_stance != [(69, 2), (62, -1), (50, 1)] {
        return Err(Frame1LeaderOptionsError::WrongSetupTypeFacts);
    }

    let rows_before = retail_initial_leader_options();
    let mut rows_after = rows_before;
    let changes_zero = cascade_changes(&source.commands[0].command, &mut rows_after)?;
    let changes_one = cascade_changes(&source.commands[1].command, &mut rows_after)?;
    if changes_zero
        != (LeaderOptionChangeSet {
            peasants_changed: true,
            buildings_changed: true,
            flag_bit_1_changed: false,
            flag_bit_3_changed: false,
            flag_bit_4_changed: false,
            mirror_to_local_option: false,
        })
        || changes_one
            != (LeaderOptionChangeSet {
                peasants_changed: true,
                buildings_changed: false,
                flag_bit_1_changed: false,
                flag_bit_3_changed: false,
                flag_bit_4_changed: false,
                mirror_to_local_option: false,
            })
    {
        return Err(Frame1LeaderOptionsError::WrongChangeSet);
    }

    // `process_leader_options` scans the Unit band first. Peasant changes directly select
    // concrete type 50/51, hence exactly the four Citizens. The building-change scan asks
    // `get_stance_type()==0`; none of the seven setup Units has type zero (2/-1/1).
    let mut writes = (3..=6)
        .map(|setup_ordinal| Frame1StanceWrite {
            who: 0,
            o: setup_ordinal as i32,
            type_index: SETUP_CITIZEN_TYPE,
            stance_type: 1,
            value: 1,
            target: Frame1StanceTarget::SetupUnit { setup_ordinal },
        })
        .collect::<Vec<_>>();
    // The Build-band peasant scan selects `BuildTypeData::get_stance_type()==1`. Village 414
    // takes that function's first exact `is(VILLAGE)` arm. The later building-type-zero scan
    // therefore does not overwrite it.
    writes.push(Frame1StanceWrite {
        who: 0,
        o: 2_000,
        type_index: STARTING_VILLAGE_TYPE,
        stance_type: 1,
        value: 1,
        target: Frame1StanceTarget::StartingVillage,
    });

    Ok(Frame1SetupLeaderOptionsPlan {
        source,
        rows_before,
        rows_after,
        command_changes: [changes_zero, changes_one],
        setup_type_stance,
        writes,
        local_mirror_excluded: true,
        next_exact_boundary:
            "bind these five writes to the canonical frame-1 Sim, then execute frames 2..379",
    })
}
