//! Exact replay source and synchronized mutation plan for the two frame-1 LeaderOptions rows.
//!
//! The selected 2024 replay has only two simulation commands before its first Group mutation,
//! both opcode 73 at live frame 1.  This module validates that complete pre-pair inventory,
//! executes the recovered row-store/change-gate prefix from retail's exact constructor image,
//! and resolves every synchronized object cascade reached by the seven-unit Dutch setup.
//!
//! It deliberately does not advance a [`don_sim::tick::Sim`] from frame zero to frame one and
//! does not publish a frame-379 chronology authority. The mount consumes a separately attested
//! frame-one command-entry Sim; it never permits relabelling a setup snapshot's frame.

#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::fmt;

use don_sim::command::tail_command_transactions::adjacent::{
    decode_leader_options, plan_leader_options_prefix, AdjacentOpenTailRequest, BitMask32State,
    LeaderOptionChangeSet, LeaderOptionDataState, LeaderOptionRowReceipt, LeaderOptionsCommand,
    LeaderOptionsPrefixError, LEADER_OPTIONS_OPCODE, LEADER_OPTION_ROWS,
};
use don_sim::systems::map_terrain::WorldChecksum;
use don_sim::systems::production;
use don_sim::systems::save_load::{save_sim, SaveError};
use don_sim::systems::sparse_object_bands_authority_frontier::{RetailBand, RetailObjectAddress};
use don_sim::systems::unit_inctime::SUPPORTED_RETAIL_EXE_SHA256;
use don_sim::tick::Sim;
use don_sim::world::{Handle, WorldObjectIdentity};

use crate::groups_pre_pair_unit_authority::{
    replay_unit_type_facts, PrePairUnitAuthorityError, ReplayUnitTypeFacts,
};
use crate::replay::{load_payload, Replay};
use crate::setup_2024_frame379::{
    Frame379SetupEntryReceipt, Frame379SetupEntrySource, Frame379SetupReceipt, GROUP_MOVE_FRAME,
    GROUP_MOVE_SERIAL, REPLAY_FILE_SHA256,
};
use crate::setup_2024_frame379_group_move::discover_frame379_group_move_source;
use crate::setup_unit_member_authority::{
    bind_canonical_setup_citizens, bind_canonical_setup_members, CanonicalSetupMemberError,
    CanonicalSetupMemberSource, CanonicalSetupSnapshotAuthority,
};
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame1CommandEntrySource {
    /// A complete retail playback chronology advanced the canonical post-setup Sim from
    /// frame zero to the command-processing boundary at live frame one.
    AuthoritativePlaybackChronologyFrameZeroThroughOne,
}

/// Whole-Sim command-entry attestation for serial 1.
///
/// The replay proves the command wire, not the state which receives it.  This authority is
/// therefore intentionally separate from [`Frame1LeaderOptionsSourceReceipt`].  In particular,
/// callers may not relabel [`Frame379SetupReceipt::canonical_frame`] from zero to one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame1CommandEntryAuthority {
    pub revision: u64,
    pub composition_digest: [u8; 32],
    pub source: Frame1CommandEntrySource,
    pub replay_file_sha256: [u8; 32],
    pub setup_composition_digest: [u8; 32],
    pub setup_frame: i32,
    pub command_frame: i32,
    pub command_entry_sim_sha256: [u8; 32],
    pub world_checksum: WorldChecksum,
    pub random_state: i32,
}

/// Retail-capture attestation for the one tick between the completed setup image and serial 1.
/// The snapshots are canonical DoNSave bytes; no replay checksum word is an input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame1CommandEntryCapture {
    pub revision: u64,
    pub source: Frame1CommandEntrySource,
    pub replay_file_sha256: [u8; 32],
    pub executable_sha256: [u8; 32],
    pub setup_sim_sha256: [u8; 32],
    pub command_entry_sim_sha256: [u8; 32],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame1CommandEntryBindError {
    Plan(Frame1LeaderOptionsError),
    MissingCaptureRevision,
    ReplayMismatch,
    UnsupportedExecutable,
    WrongSetupFrame { expected: i32, actual: i32 },
    WrongCommandFrame { expected: i32, actual: i32 },
    Snapshot(SaveError),
    SetupSnapshotMismatch,
    CommandEntrySnapshotMismatch,
    SetupWorldMismatch,
    SetupRandomMismatch,
    SetupMember(CanonicalSetupMemberError),
    SetupIdentityChanged,
}

impl fmt::Display for Frame1CommandEntryBindError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "2024 frame-1 command entry refused: {self:?}")
    }
}

impl std::error::Error for Frame1CommandEntryBindError {}

impl From<Frame1LeaderOptionsError> for Frame1CommandEntryBindError {
    fn from(value: Frame1LeaderOptionsError) -> Self {
        Self::Plan(value)
    }
}

impl From<CanonicalSetupMemberError> for Frame1CommandEntryBindError {
    fn from(value: CanonicalSetupMemberError) -> Self {
        Self::SetupMember(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame1StanceMutationReceipt {
    pub target: Frame1StanceTarget,
    pub row: usize,
    pub handle: Option<Handle>,
    pub who: i32,
    pub o: i32,
    pub type_index: i32,
    pub before: i8,
    pub after: i8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame1LeaderOptionsMountReceipt {
    pub chronology_revision: u64,
    pub chronology_digest: [u8; 32],
    pub source: Frame1CommandEntrySource,
    pub replay_file_sha256: [u8; 32],
    pub setup_composition_digest: [u8; 32],
    pub command_entry_sim_sha256: [u8; 32],
    pub commands: [Frame1LeaderOptionsCommandSource; 2],
    pub rows_after: [LeaderOptionDataState; LEADER_OPTION_ROWS],
    pub mutations: Vec<Frame1StanceMutationReceipt>,
    pub next_exact_boundary: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame1LeaderOptionsMountError {
    Plan(Frame1LeaderOptionsError),
    MissingChronologyRevision,
    MissingChronologyDigest,
    ChronologyReplayMismatch,
    ChronologySetupMismatch,
    SetupEntryMismatch,
    WrongSetupFrame { expected: i32, actual: i32 },
    WrongCommandFrame { expected: i32, actual: i32 },
    Snapshot(SaveError),
    CommandEntrySnapshotMismatch,
    CommandEntryWorldMismatch,
    CommandEntryRandomMismatch,
    SetupMember(CanonicalSetupMemberError),
    WrongCitizenCohort,
    StaleCitizen,
    CitizenStanceMismatch { o: i32, actual: i8 },
    RegistryNotDenseEquivalent,
    MissingStartingVillage,
    StartingVillageMismatch,
    StartingVillageStanceMismatch { actual: i8 },
}

impl fmt::Display for Frame1LeaderOptionsMountError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "2024 frame-1 LeaderOptions mount refused: {self:?}")
    }
}

impl std::error::Error for Frame1LeaderOptionsMountError {}

impl From<Frame1LeaderOptionsError> for Frame1LeaderOptionsMountError {
    fn from(value: Frame1LeaderOptionsError) -> Self {
        Self::Plan(value)
    }
}

impl From<CanonicalSetupMemberError> for Frame1LeaderOptionsMountError {
    fn from(value: CanonicalSetupMemberError) -> Self {
        Self::SetupMember(value)
    }
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
        next_exact_boundary: "bind these five writes to the canonical frame-1 command-entry Sim",
    })
}

/// Bind a supported retail frame-zero-to-one capture to the canonical setup receipt.
///
/// This is a capture admission boundary, not an offline reconstruction of the tick. It proves
/// that both snapshots are exact DoNSave images from the supported executable, that the first is
/// the completed seven-call setup state, and that all seven stable Unit identities survive into
/// the frame-one command entry. Recorded checksum packets are never consulted.
pub fn bind_captured_frame1_command_entry(
    replay: &Replay,
    setup: &Frame379SetupReceipt,
    setup_sim: &Sim,
    command_entry: &Sim,
    capture: &Frame1CommandEntryCapture,
) -> Result<Frame1CommandEntryAuthority, Frame1CommandEntryBindError> {
    let plan = plan_frame1_setup_leader_options(replay)?;
    if capture.revision == 0 {
        return Err(Frame1CommandEntryBindError::MissingCaptureRevision);
    }
    if capture.replay_file_sha256 != REPLAY_FILE_SHA256
        || setup.replay_file_sha256 != REPLAY_FILE_SHA256
        || plan.source.replay_file_sha256 != REPLAY_FILE_SHA256
    {
        return Err(Frame1CommandEntryBindError::ReplayMismatch);
    }
    if capture.executable_sha256 != SUPPORTED_RETAIL_EXE_SHA256 {
        return Err(Frame1CommandEntryBindError::UnsupportedExecutable);
    }
    if setup.canonical_frame != 0 || setup_sim.world.frame != 0 {
        return Err(Frame1CommandEntryBindError::WrongSetupFrame {
            expected: 0,
            actual: setup_sim.world.frame,
        });
    }
    if command_entry.world.frame != LEADER_OPTIONS_FRAME as i32 {
        return Err(Frame1CommandEntryBindError::WrongCommandFrame {
            expected: LEADER_OPTIONS_FRAME as i32,
            actual: command_entry.world.frame,
        });
    }
    let setup_bytes = save_sim(setup_sim).map_err(Frame1CommandEntryBindError::Snapshot)?;
    if sha256(&setup_bytes) != capture.setup_sim_sha256 {
        return Err(Frame1CommandEntryBindError::SetupSnapshotMismatch);
    }
    let entry_bytes = save_sim(command_entry).map_err(Frame1CommandEntryBindError::Snapshot)?;
    if sha256(&entry_bytes) != capture.command_entry_sim_sha256 {
        return Err(Frame1CommandEntryBindError::CommandEntrySnapshotMismatch);
    }
    if setup_sim.map.world.checksum_sections() != setup.canonical_world_checksum {
        return Err(Frame1CommandEntryBindError::SetupWorldMismatch);
    }
    if setup_sim.world.random.state() != setup.canonical_random_state {
        return Err(Frame1CommandEntryBindError::SetupRandomMismatch);
    }

    let ordinals = (0..setup.plan.calls.len()).collect::<Vec<_>>();
    let setup_members = bind_canonical_setup_members(
        replay,
        &setup.plan,
        &setup.setup,
        setup_sim,
        &ordinals,
        &setup.canonical_snapshot_authority(),
    )?;
    let entry_snapshot = CanonicalSetupSnapshotAuthority {
        revision: capture.revision,
        composition_digest: capture.command_entry_sim_sha256,
        source: CanonicalSetupMemberSource::ReplayRulesCompleteInitReceiptAndCanonicalSim,
        replay_file_sha256: capture.replay_file_sha256,
        frame: command_entry.world.frame,
        world_checksum: command_entry.map.world.checksum_sections(),
        random_state: command_entry.world.random.state(),
    };
    let entry_members = bind_canonical_setup_members(
        replay,
        &setup.plan,
        &setup.setup,
        command_entry,
        &ordinals,
        &entry_snapshot,
    )?;
    if setup_members.len() != entry_members.len()
        || setup_members.len() != setup.plan.calls.len()
        || setup_members
            .iter()
            .zip(&entry_members)
            .any(|(before, after)| {
                before.setup_ordinal != after.setup_ordinal
                    || before.member_ordinal != after.member_ordinal
                    || before.stable_identity() != after.stable_identity()
                    || before.current_type != after.current_type
            })
    {
        return Err(Frame1CommandEntryBindError::SetupIdentityChanged);
    }

    let world_checksum = command_entry.map.world.checksum_sections();
    let random_state = command_entry.world.random.state();
    let mut image = b"don-frame1-command-entry-v1".to_vec();
    image.extend_from_slice(&capture.revision.to_le_bytes());
    image.extend_from_slice(&capture.replay_file_sha256);
    image.extend_from_slice(&capture.executable_sha256);
    image.extend_from_slice(&setup.canonical_composition_digest);
    image.extend_from_slice(&capture.setup_sim_sha256);
    image.extend_from_slice(&capture.command_entry_sim_sha256);
    image.extend_from_slice(&world_checksum.full.to_le_bytes());
    image.extend_from_slice(&world_checksum.bytes.to_le_bytes());
    for section in &world_checksum.per_section {
        image.extend_from_slice(&section.adler.to_le_bytes());
        image.extend_from_slice(&section.bytes.to_le_bytes());
    }
    image.extend_from_slice(&random_state.to_le_bytes());
    for member in &entry_members {
        let identity = member.stable_identity();
        image.extend_from_slice(&(member.setup_ordinal as u64).to_le_bytes());
        image.extend_from_slice(&(member.member_ordinal as u64).to_le_bytes());
        image.extend_from_slice(&identity.id.to_le_bytes());
        image.extend_from_slice(&identity.generation.to_le_bytes());
        image.extend_from_slice(&identity.owner.to_le_bytes());
        image.extend_from_slice(&identity.o.to_le_bytes());
        image.extend_from_slice(&member.current_type.to_le_bytes());
    }
    Ok(Frame1CommandEntryAuthority {
        revision: capture.revision,
        composition_digest: sha256(&image),
        source: capture.source,
        replay_file_sha256: capture.replay_file_sha256,
        setup_composition_digest: setup.canonical_composition_digest,
        setup_frame: setup.canonical_frame,
        command_frame: command_entry.world.frame,
        command_entry_sim_sha256: capture.command_entry_sim_sha256,
        world_checksum,
        random_state,
    })
}

fn prepare_frame1_stance_mutations(
    replay: &Replay,
    setup_entry: &Frame379SetupEntryReceipt,
    setup: &Frame379SetupReceipt,
    candidate: &Sim,
    chronology: &Frame1CommandEntryAuthority,
    plan: &Frame1SetupLeaderOptionsPlan,
) -> Result<Vec<Frame1StanceMutationReceipt>, Frame1LeaderOptionsMountError> {
    if chronology.revision == 0 {
        return Err(Frame1LeaderOptionsMountError::MissingChronologyRevision);
    }
    if chronology.composition_digest == [0; 32] {
        return Err(Frame1LeaderOptionsMountError::MissingChronologyDigest);
    }
    if chronology.replay_file_sha256 != REPLAY_FILE_SHA256
        || setup.replay_file_sha256 != REPLAY_FILE_SHA256
        || setup_entry.worldgen.replay_file_sha256 != REPLAY_FILE_SHA256
    {
        return Err(Frame1LeaderOptionsMountError::ChronologyReplayMismatch);
    }
    if chronology.setup_composition_digest != setup.canonical_composition_digest {
        return Err(Frame1LeaderOptionsMountError::ChronologySetupMismatch);
    }
    if setup.world_checksum_before != setup_entry.worldgen.world_checksum
        || setup.rng_before != setup_entry.worldgen.random_state
        || setup.source != setup_entry.worldgen.source
        || setup_entry.source != Frame379SetupEntrySource::CompleteRetailBuildUnitsEntry
        || setup_entry.executable_sha256 != SUPPORTED_RETAIL_EXE_SHA256
        || setup_entry.entry_sim_sha256 == [0; 32]
    {
        return Err(Frame1LeaderOptionsMountError::SetupEntryMismatch);
    }
    if chronology.setup_frame != 0 || setup.canonical_frame != 0 {
        return Err(Frame1LeaderOptionsMountError::WrongSetupFrame {
            expected: 0,
            actual: chronology.setup_frame,
        });
    }
    if chronology.command_frame != LEADER_OPTIONS_FRAME as i32
        || candidate.world.frame != LEADER_OPTIONS_FRAME as i32
    {
        return Err(Frame1LeaderOptionsMountError::WrongCommandFrame {
            expected: LEADER_OPTIONS_FRAME as i32,
            actual: candidate.world.frame,
        });
    }
    let snapshot = save_sim(candidate).map_err(Frame1LeaderOptionsMountError::Snapshot)?;
    if sha256(&snapshot) != chronology.command_entry_sim_sha256 {
        return Err(Frame1LeaderOptionsMountError::CommandEntrySnapshotMismatch);
    }
    if candidate.map.world.checksum_sections() != chronology.world_checksum {
        return Err(Frame1LeaderOptionsMountError::CommandEntryWorldMismatch);
    }
    if candidate.world.random.state() != chronology.random_state {
        return Err(Frame1LeaderOptionsMountError::CommandEntryRandomMismatch);
    }

    let snapshot_authority = CanonicalSetupSnapshotAuthority {
        revision: chronology.revision,
        composition_digest: chronology.composition_digest,
        source: CanonicalSetupMemberSource::ReplayRulesCompleteInitReceiptAndCanonicalSim,
        replay_file_sha256: chronology.replay_file_sha256,
        frame: chronology.command_frame,
        world_checksum: chronology.world_checksum.clone(),
        random_state: chronology.random_state,
    };
    let citizens = bind_canonical_setup_citizens(
        replay,
        &setup.plan,
        &setup.setup,
        candidate,
        &snapshot_authority,
    )?;
    if citizens.len() != 4 {
        return Err(Frame1LeaderOptionsMountError::WrongCitizenCohort);
    }

    let mut mutations = Vec::with_capacity(plan.writes.len());
    let mut seen_rows = BTreeSet::new();
    for write in plan
        .writes
        .iter()
        .filter(|write| matches!(write.target, Frame1StanceTarget::SetupUnit { .. }))
    {
        let Frame1StanceTarget::SetupUnit { setup_ordinal } = write.target else {
            unreachable!("the filter retained only setup Units")
        };
        let member = citizens
            .iter()
            .find(|member| member.setup_ordinal == setup_ordinal)
            .ok_or(Frame1LeaderOptionsMountError::WrongCitizenCohort)?;
        if member.member_ordinal != 0
            || member.current_type != write.type_index
            || member.unit.identity.who != write.who as u8
            || i32::from(member.unit.identity.o) != write.o
            || (
                member.unit.identity.handle.id,
                member.unit.identity.handle.generation,
            ) != (
                member.allocation.identity.id,
                member.allocation.identity.generation,
            )
            || !seen_rows.insert(member.row)
        {
            return Err(Frame1LeaderOptionsMountError::StaleCitizen);
        }
        let actual = candidate.world.units.stance()[member.row];
        if actual != 0 {
            return Err(Frame1LeaderOptionsMountError::CitizenStanceMismatch {
                o: write.o,
                actual,
            });
        }
        mutations.push(Frame1StanceMutationReceipt {
            target: write.target,
            row: member.row,
            handle: Some(member.unit.identity.handle),
            who: write.who,
            o: write.o,
            type_index: write.type_index,
            before: actual,
            after: write.value,
        });
    }
    if mutations.len() != 4 {
        return Err(Frame1LeaderOptionsMountError::WrongCitizenCohort);
    }

    if !candidate.world.object_bands_are_dense_equivalent() {
        return Err(Frame1LeaderOptionsMountError::RegistryNotDenseEquivalent);
    }
    let village_write = plan
        .writes
        .iter()
        .find(|write| matches!(write.target, Frame1StanceTarget::StartingVillage))
        .ok_or(Frame1LeaderOptionsMountError::MissingStartingVillage)?;
    let village_row = setup_entry.center_build_row;
    let village = candidate
        .builds
        .get(village_row)
        .ok_or(Frame1LeaderOptionsMountError::MissingStartingVillage)?;
    let address = RetailObjectAddress::new(0, RetailBand::Build, village_write.o);
    if setup_entry.center_build_o != village_write.o
        || village_write.who != 0
        || village_write.type_index != STARTING_VILLAGE_TYPE
        || village.who != 0
        || i32::from(village.object_id()) != village_write.o
        || village.flags & production::flag::VALID == 0
        || candidate
            .production_runtime
            .build_types
            .get(village_row)
            .and_then(|value| *value)
            != Some(STARTING_VILLAGE_TYPE)
        || candidate.world.object_bands().live_identity(address)
            != Some(WorldObjectIdentity::BuildRow(village_row as u32))
    {
        return Err(Frame1LeaderOptionsMountError::StartingVillageMismatch);
    }
    if village.stance != 0 {
        return Err(
            Frame1LeaderOptionsMountError::StartingVillageStanceMismatch {
                actual: village.stance,
            },
        );
    }
    mutations.push(Frame1StanceMutationReceipt {
        target: village_write.target,
        row: village_row,
        handle: None,
        who: village_write.who,
        o: village_write.o,
        type_index: village_write.type_index,
        before: village.stance,
        after: village_write.value,
    });
    Ok(mutations)
}

fn validate_prepared_frame1_stance_mutations(
    candidate: &Sim,
    mutations: &[Frame1StanceMutationReceipt],
) -> Result<(), Frame1LeaderOptionsMountError> {
    if mutations.len() != 5 || !candidate.world.object_bands_are_dense_equivalent() {
        return Err(Frame1LeaderOptionsMountError::WrongCitizenCohort);
    }
    let mut citizen_ordinals = BTreeSet::new();
    let mut village_count = 0usize;
    for mutation in mutations {
        if mutation.before != 0 || mutation.after != 1 {
            return Err(Frame1LeaderOptionsMountError::WrongCitizenCohort);
        }
        match mutation.target {
            Frame1StanceTarget::SetupUnit { setup_ordinal } => {
                let handle = mutation
                    .handle
                    .ok_or(Frame1LeaderOptionsMountError::StaleCitizen)?;
                if setup_ordinal != mutation.o as usize
                    || !(3..=6).contains(&setup_ordinal)
                    || mutation.who != 0
                    || mutation.type_index != SETUP_CITIZEN_TYPE
                    || !citizen_ordinals.insert(setup_ordinal)
                    || candidate.world.row_of(handle) != Some(mutation.row)
                    || candidate.world.units.get_who(mutation.row) != 0
                    || i32::from(candidate.world.units.o()[mutation.row]) != mutation.o
                    || candidate.world.unit_type_id(mutation.row) != Some(SETUP_CITIZEN_TYPE)
                    || candidate.unit_type.get(mutation.row).copied() != Some(SETUP_CITIZEN_TYPE)
                {
                    return Err(Frame1LeaderOptionsMountError::StaleCitizen);
                }
                let actual = candidate.world.units.stance()[mutation.row];
                if actual != mutation.before {
                    return Err(Frame1LeaderOptionsMountError::CitizenStanceMismatch {
                        o: mutation.o,
                        actual,
                    });
                }
            }
            Frame1StanceTarget::StartingVillage => {
                village_count += 1;
                let village = candidate
                    .builds
                    .get(mutation.row)
                    .ok_or(Frame1LeaderOptionsMountError::MissingStartingVillage)?;
                let address =
                    RetailObjectAddress::new(mutation.who as u8, RetailBand::Build, mutation.o);
                if mutation.handle.is_some()
                    || mutation.who != 0
                    || mutation.o != 2_000
                    || mutation.type_index != STARTING_VILLAGE_TYPE
                    || village.who != 0
                    || i32::from(village.object_id()) != mutation.o
                    || candidate
                        .production_runtime
                        .build_types
                        .get(mutation.row)
                        .and_then(|value| *value)
                        != Some(STARTING_VILLAGE_TYPE)
                    || candidate.world.object_bands().live_identity(address)
                        != Some(WorldObjectIdentity::BuildRow(mutation.row as u32))
                {
                    return Err(Frame1LeaderOptionsMountError::StartingVillageMismatch);
                }
                if village.stance != mutation.before {
                    return Err(
                        Frame1LeaderOptionsMountError::StartingVillageStanceMismatch {
                            actual: village.stance,
                        },
                    );
                }
            }
        }
    }
    if citizen_ordinals != BTreeSet::from([3, 4, 5, 6]) || village_count != 1 {
        return Err(Frame1LeaderOptionsMountError::WrongCitizenCohort);
    }
    Ok(())
}

fn commit_prepared_frame1_stance_mutations(
    candidate: &mut Sim,
    mutations: &[Frame1StanceMutationReceipt],
) {
    for mutation in mutations {
        match mutation.target {
            Frame1StanceTarget::SetupUnit { .. } => {
                candidate.world.units.stance_mut()[mutation.row] = mutation.after;
            }
            Frame1StanceTarget::StartingVillage => {
                candidate.builds[mutation.row].stance = mutation.after;
            }
        }
    }
}

/// Atomically apply serial 1's exact synchronized tail to a canonical frame-one Sim.
///
/// Every source, snapshot, stable Unit identity, Build-band identity, type, and before-stance is
/// checked before the owned `candidate` is mutated. On refusal the untouched Sim is returned to
/// the caller. The next chronology step is the post-command `Game::do_frame` whose entry frame is
/// still one; this function does not skip directly to frame two or frame 379.
pub fn mount_frame1_setup_leader_options(
    replay: &Replay,
    setup_entry: &Frame379SetupEntryReceipt,
    setup: &Frame379SetupReceipt,
    mut candidate: Sim,
    chronology: &Frame1CommandEntryAuthority,
) -> Result<(Sim, Frame1LeaderOptionsMountReceipt), (Frame1LeaderOptionsMountError, Sim)> {
    let plan = match plan_frame1_setup_leader_options(replay) {
        Ok(plan) => plan,
        Err(error) => return Err((error.into(), candidate)),
    };
    let mutations = match prepare_frame1_stance_mutations(
        replay,
        setup_entry,
        setup,
        &candidate,
        chronology,
        &plan,
    ) {
        Ok(mutations) => mutations,
        Err(error) => return Err((error, candidate)),
    };
    if let Err(error) = validate_prepared_frame1_stance_mutations(&candidate, &mutations) {
        return Err((error, candidate));
    }

    commit_prepared_frame1_stance_mutations(&mut candidate, &mutations);
    let receipt = Frame1LeaderOptionsMountReceipt {
        chronology_revision: chronology.revision,
        chronology_digest: chronology.composition_digest,
        source: chronology.source,
        replay_file_sha256: chronology.replay_file_sha256,
        setup_composition_digest: chronology.setup_composition_digest,
        command_entry_sim_sha256: chronology.command_entry_sim_sha256,
        commands: plan.source.commands,
        rows_after: plan.rows_after,
        mutations,
        next_exact_boundary:
            "execute the post-command frame-1 tick, then frames 2..378 to frame-379 entry",
    };
    Ok((candidate, receipt))
}

#[cfg(test)]
mod mount_tests {
    use super::*;
    use crate::build_spawn_runtime::{spawn_canonical_build, CanonicalBuildSpawnRequest};

    fn candidate_and_mutations() -> (Sim, Vec<Frame1StanceMutationReceipt>) {
        let mut candidate = Sim::new(0x00bb_97d3, 16);
        let mut village = production::BuildData::default();
        village.flags = production::flag::VALID;
        village.city = -1;
        village.gather_from.mtn = -1;
        village.gather_from.cliff = -1;
        let build = spawn_canonical_build(
            &mut candidate,
            CanonicalBuildSpawnRequest {
                owner: 0,
                type_index: STARTING_VILLAGE_TYPE,
                snapped_x: 0x1800,
                snapped_y: 0x2400,
                build: village,
            },
        )
        .unwrap();
        assert_eq!((build.row, i32::from(build.object_id)), (0, 2_000));

        let mut handles = Vec::new();
        for (ordinal, type_index) in [69, 62, 62, 50, 50, 50, 50].into_iter().enumerate() {
            let handle = candidate
                .spawn_unit(0, type_index, 0x3000 + ordinal as i32, 0x4000, 1)
                .unwrap();
            handles.push(handle);
        }
        candidate.world.frame = LEADER_OPTIONS_FRAME as i32;

        let mut mutations = (3..=6)
            .map(|setup_ordinal| Frame1StanceMutationReceipt {
                target: Frame1StanceTarget::SetupUnit { setup_ordinal },
                row: candidate.world.row_of(handles[setup_ordinal]).unwrap(),
                handle: Some(handles[setup_ordinal]),
                who: 0,
                o: setup_ordinal as i32,
                type_index: SETUP_CITIZEN_TYPE,
                before: 0,
                after: 1,
            })
            .collect::<Vec<_>>();
        mutations.push(Frame1StanceMutationReceipt {
            target: Frame1StanceTarget::StartingVillage,
            row: build.row,
            handle: None,
            who: 0,
            o: i32::from(build.object_id),
            type_index: STARTING_VILLAGE_TYPE,
            before: 0,
            after: 1,
        });
        (candidate, mutations)
    }

    #[test]
    fn prepared_commit_changes_only_the_four_citizens_and_village() {
        let (mut candidate, mutations) = candidate_and_mutations();
        validate_prepared_frame1_stance_mutations(&candidate, &mutations).unwrap();
        commit_prepared_frame1_stance_mutations(&mut candidate, &mutations);

        assert_eq!(&candidate.world.units.stance()[..7], &[0, 0, 0, 1, 1, 1, 1]);
        assert_eq!(candidate.builds[0].stance, 1);
    }

    #[test]
    fn prepared_commit_bites_generation_cohort_and_before_image_mutations() {
        let (candidate, mut mutations) = candidate_and_mutations();
        let Some(handle) = mutations[0].handle.as_mut() else {
            panic!("Citizen mutation must carry a stable Handle")
        };
        handle.generation = handle.generation.wrapping_add(1);
        assert_eq!(
            validate_prepared_frame1_stance_mutations(&candidate, &mutations),
            Err(Frame1LeaderOptionsMountError::StaleCitizen)
        );

        let (mut candidate, mutations) = candidate_and_mutations();
        candidate.world.units.stance_mut()[mutations[1].row] = 2;
        assert_eq!(
            validate_prepared_frame1_stance_mutations(&candidate, &mutations),
            Err(Frame1LeaderOptionsMountError::CitizenStanceMismatch { o: 4, actual: 2 })
        );

        let (mut candidate, mutations) = candidate_and_mutations();
        candidate.builds[0].stance = 3;
        assert_eq!(
            validate_prepared_frame1_stance_mutations(&candidate, &mutations),
            Err(Frame1LeaderOptionsMountError::StartingVillageStanceMismatch { actual: 3 })
        );

        let (candidate, mut mutations) = candidate_and_mutations();
        mutations[2].target = Frame1StanceTarget::SetupUnit { setup_ordinal: 3 };
        assert_eq!(
            validate_prepared_frame1_stance_mutations(&candidate, &mutations),
            Err(Frame1LeaderOptionsMountError::StaleCitizen)
        );
    }
}
