//! Exact mount from the canonical 2024 setup Sim to its first Group -> Move command.
//!
//! The setup receipt owns frame zero. This module never edits that frame to 379. It requires an
//! explicit chronology authority for the canonical Sim after every intervening retail frame has
//! completed, rebinds all seven owner-0 setup identities at that immutable command-entry state,
//! installs complete live Group-Move authority, and executes the exact replay package.

#![forbid(unsafe_code)]

use std::fmt;

use don_sim::systems::map_terrain::WorldChecksum;
use don_sim::tick::Sim;

use crate::groups_sim_channel::{
    issue_replay_group_move, GroupChannelTransitionReceipt, GroupSimChannelError,
    ReplayGroupMoveSource,
};
use crate::replay::Replay;
use crate::replay_land_speed_content::{
    produce_replay_land_speed_content, ReplayLandSpeedContentError,
};
use crate::setup_2024_frame379::{
    Frame379SetupReceipt, GROUP_MOVE_FRAME, GROUP_MOVE_SERIAL, REPLAY_FILE_SHA256, SETUP_CALLS,
};
use crate::setup_group_move_authority::{
    produce_replay_fresh_setup_group_move_authority, SetupGroupMoveAuthorityError,
    SetupGroupMoveAuthorityReceipt,
};
use crate::setup_unit_member_authority::{
    bind_canonical_setup_members, CanonicalSetupMemberError, CanonicalSetupMemberSource,
    CanonicalSetupSnapshotAuthority,
};

pub const SETUP_FRAME: i32 = 0;
pub const PAIR_INDEX: usize = 1;
pub const PAIR_PLAY: usize = 0;
pub const SELECTED_CITIZENS: [i16; 4] = [3, 4, 5, 6];
pub const DESTINATION: (i32, i32) = (5_186, 72_095);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame379CommandEntrySource {
    /// Complete replay execution from the validated post-setup frame-zero Sim to the playback
    /// iteration whose live `Game::frame` is 379. Playback checks first, then processes serial 64.
    AuthoritativePlaybackChronologyFrameZeroThrough379,
}

/// Source-backed command-entry state. Recorded checksum words are not members of this authority.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame379CommandEntryAuthority {
    pub revision: u64,
    pub composition_digest: [u8; 32],
    pub source: Frame379CommandEntrySource,
    pub replay_file_sha256: [u8; 32],
    pub setup_composition_digest: [u8; 32],
    pub setup_frame: i32,
    pub command_frame: i32,
    pub world_checksum: WorldChecksum,
    pub random_state: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame379GroupMoveMountReceipt {
    pub chronology_revision: u64,
    pub chronology_digest: [u8; 32],
    pub source: Frame379CommandEntrySource,
    pub replay_file_sha256: [u8; 32],
    pub setup_composition_digest: [u8; 32],
    pub command_source: ReplayGroupMoveSource,
    pub canonical_setup_members: usize,
    pub group_authority: SetupGroupMoveAuthorityReceipt,
    pub transition: GroupChannelTransitionReceipt,
    /// The next required state transition. The delayed recorded checksum is observation only.
    pub next_exact_boundary: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame379GroupMoveMountError {
    MissingChronologyRevision,
    MissingChronologyDigest,
    ChronologyReplayMismatch,
    ChronologySetupMismatch,
    WrongSetupFrame { expected: i32, actual: i32 },
    WrongCommandFrame { expected: i32, actual: i32 },
    CommandEntryWorldMismatch,
    CommandEntryRandomMismatch,
    MissingSource,
    DuplicateSource,
    WrongSourceIdentity,
    WrongSourceShell,
    SetupMember(CanonicalSetupMemberError),
    LandSpeedContent(ReplayLandSpeedContentError),
    GroupAuthority(SetupGroupMoveAuthorityError),
    GroupTransition(GroupSimChannelError),
}

impl fmt::Display for Frame379GroupMoveMountError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "2024 frame-379 Group-Move mount refused: {self:?}")
    }
}

impl std::error::Error for Frame379GroupMoveMountError {}

impl From<CanonicalSetupMemberError> for Frame379GroupMoveMountError {
    fn from(value: CanonicalSetupMemberError) -> Self {
        Self::SetupMember(value)
    }
}

impl From<ReplayLandSpeedContentError> for Frame379GroupMoveMountError {
    fn from(value: ReplayLandSpeedContentError) -> Self {
        Self::LandSpeedContent(value)
    }
}

impl From<SetupGroupMoveAuthorityError> for Frame379GroupMoveMountError {
    fn from(value: SetupGroupMoveAuthorityError) -> Self {
        Self::GroupAuthority(value)
    }
}

impl From<GroupSimChannelError> for Frame379GroupMoveMountError {
    fn from(value: GroupSimChannelError) -> Self {
        Self::GroupTransition(value)
    }
}

/// Find the unique source package with the pinned serial/frame/play and exact inert shell.
pub fn discover_frame379_group_move_source(
    replay: &Replay,
) -> Result<ReplayGroupMoveSource, Frame379GroupMoveMountError> {
    let mut matches = Vec::new();
    for (turn_index, turn) in replay.turns.iter().enumerate() {
        for player_index in 0..turn.players.len() {
            let Ok(source) = ReplayGroupMoveSource::from_replay(replay, turn_index, player_index)
            else {
                continue;
            };
            if source.lockstep_serial == GROUP_MOVE_SERIAL
                && source.package_frame == GROUP_MOVE_FRAME
                && source.play == PAIR_PLAY
            {
                matches.push(source);
            }
        }
    }
    let source = match matches.len() {
        0 => return Err(Frame379GroupMoveMountError::MissingSource),
        1 => matches.pop().expect("one source"),
        _ => return Err(Frame379GroupMoveMountError::DuplicateSource),
    };
    if source.pair_command_index != PAIR_INDEX
        || !source.unowned_sim_prefix.is_empty()
        || !source.unowned_sim_suffix.is_empty()
        || source.inert_opcodes != [0x4f, 0x39, 0x4a, 0x48]
    {
        return Err(Frame379GroupMoveMountError::WrongSourceShell);
    }
    let wire = don_sim::systems::canonical_group_move_host::decode_group_move_package(
        &source.command_bytes(),
    )
    .map_err(|error| {
        Frame379GroupMoveMountError::GroupTransition(GroupSimChannelError::Host(error))
    })?;
    if wire.who != 0
        || wire.objects != SELECTED_CITIZENS
        || (wire.movement.x, wire.movement.y) != DESTINATION
    {
        return Err(Frame379GroupMoveMountError::WrongSourceIdentity);
    }
    Ok(source)
}

/// Bind, install, and issue serial 64 at live frame 379.
///
/// Ownership of `candidate` makes refusal externally atomic: callers receive the Sim back and
/// may discard it, while no already-published Sim is mutated. The command host itself stages and
/// commits Group/Unit/path state atomically.
pub fn mount_frame379_group_move(
    replay: &Replay,
    setup: &Frame379SetupReceipt,
    mut candidate: Sim,
    chronology: &Frame379CommandEntryAuthority,
) -> Result<(Sim, Frame379GroupMoveMountReceipt), (Frame379GroupMoveMountError, Sim)> {
    let validate = || -> Result<ReplayGroupMoveSource, Frame379GroupMoveMountError> {
        if chronology.revision == 0 {
            return Err(Frame379GroupMoveMountError::MissingChronologyRevision);
        }
        if chronology.composition_digest == [0; 32] {
            return Err(Frame379GroupMoveMountError::MissingChronologyDigest);
        }
        if chronology.replay_file_sha256 != REPLAY_FILE_SHA256
            || setup.replay_file_sha256 != REPLAY_FILE_SHA256
        {
            return Err(Frame379GroupMoveMountError::ChronologyReplayMismatch);
        }
        if chronology.setup_composition_digest != setup.canonical_composition_digest {
            return Err(Frame379GroupMoveMountError::ChronologySetupMismatch);
        }
        if chronology.setup_frame != SETUP_FRAME || setup.canonical_frame != SETUP_FRAME {
            return Err(Frame379GroupMoveMountError::WrongSetupFrame {
                expected: SETUP_FRAME,
                actual: chronology.setup_frame,
            });
        }
        if chronology.command_frame != GROUP_MOVE_FRAME || candidate.world.frame != GROUP_MOVE_FRAME
        {
            return Err(Frame379GroupMoveMountError::WrongCommandFrame {
                expected: GROUP_MOVE_FRAME,
                actual: candidate.world.frame,
            });
        }
        if candidate.map.world.checksum_sections() != chronology.world_checksum {
            return Err(Frame379GroupMoveMountError::CommandEntryWorldMismatch);
        }
        if candidate.world.random.state() != chronology.random_state {
            return Err(Frame379GroupMoveMountError::CommandEntryRandomMismatch);
        }
        discover_frame379_group_move_source(replay)
    };
    let source = match validate() {
        Ok(source) => source,
        Err(error) => return Err((error, candidate)),
    };

    let snapshot = CanonicalSetupSnapshotAuthority {
        revision: chronology.revision,
        composition_digest: chronology.composition_digest,
        source: CanonicalSetupMemberSource::ReplayRulesCompleteInitReceiptAndCanonicalSim,
        replay_file_sha256: chronology.replay_file_sha256,
        frame: chronology.command_frame,
        world_checksum: chronology.world_checksum.clone(),
        random_state: chronology.random_state,
    };
    let ordinals = (0..SETUP_CALLS).collect::<Vec<_>>();
    let members = match bind_canonical_setup_members(
        replay,
        &setup.plan,
        &setup.setup,
        &candidate,
        &ordinals,
        &snapshot,
    ) {
        Ok(members) => members,
        Err(error) => return Err((Frame379GroupMoveMountError::SetupMember(error), candidate)),
    };
    let content = match produce_replay_land_speed_content(replay) {
        Ok(content) => content,
        Err(error) => {
            return Err((
                Frame379GroupMoveMountError::LandSpeedContent(error),
                candidate,
            ));
        }
    };
    let group_authority = match produce_replay_fresh_setup_group_move_authority(
        &candidate,
        &members,
        &content,
        DESTINATION,
        false,
    ) {
        Ok(receipt) => receipt,
        Err(error) => {
            return Err((
                Frame379GroupMoveMountError::GroupAuthority(error),
                candidate,
            ));
        }
    };
    candidate.replace_group_move_authority(group_authority.authority.clone());
    let transition = match issue_replay_group_move(&mut candidate, &source) {
        Ok(receipt) => receipt,
        Err(error) => {
            return Err((
                Frame379GroupMoveMountError::GroupTransition(error),
                candidate,
            ));
        }
    };
    Ok((
        candidate,
        Frame379GroupMoveMountReceipt {
            chronology_revision: chronology.revision,
            chronology_digest: chronology.composition_digest,
            source: chronology.source,
            replay_file_sha256: chronology.replay_file_sha256,
            setup_composition_digest: chronology.setup_composition_digest,
            command_source: source,
            canonical_setup_members: members.len(),
            group_authority,
            transition,
            next_exact_boundary:
                "tick installed Group through frame-384 Groups::process normalization before frame-391 visibility",
        },
    ))
}
