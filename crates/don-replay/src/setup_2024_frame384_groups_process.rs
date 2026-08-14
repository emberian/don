//! Exact continuation of the 2024 frame-379 Group+Move mount at frame 384.
//!
//! Playback applies lockstep serial 64 at live frame 379 after the local checksum. Five
//! scheduler frames later, slot zero reaches `Groups::process`. This adapter accepts only a
//! source-backed canonical Sim at the instant of that frame-384 callback, proves its Groups
//! image is still the serial-64 after-image, regenerates dynamic Unit speed authority from the
//! same state, and commits the detached exact normalization transaction.

#![forbid(unsafe_code)]

use std::fmt;

use don_sim::systems::groups_guys::NUM_LEADERS;
use don_sim::systems::groups_process_authority::{
    commit_groups_process, prepare_groups_process, GroupsProcessAuthorityError,
    GroupsProcessReceipt,
};
use don_sim::systems::map_terrain::WorldChecksum;
use don_sim::tick::Sim;

use crate::groups_channel::GroupsChecksum;
use crate::groups_sim_channel::{sim_groups_checksum, GroupSimChannelError};
use crate::replay::Replay;
use crate::replay_land_speed_content::{
    produce_replay_land_speed_content, ReplayLandSpeedContentError,
};
use crate::setup_2024_frame379::{
    Frame379SetupReceipt, GROUP_MOVE_FRAME, GROUP_MOVE_SERIAL, REPLAY_FILE_SHA256, SETUP_CALLS,
};
use crate::setup_2024_frame379_group_move::{
    Frame379GroupMoveMountReceipt, DESTINATION, PAIR_PLAY, SELECTED_CITIZENS,
};
use crate::setup_group_move_authority::{
    produce_replay_current_type_cohort_group_move_authority, SetupGroupMoveAuthorityError,
    SetupGroupMoveAuthorityReceipt,
};
use crate::setup_unit_member_authority::{
    bind_canonical_setup_members, CanonicalSetupMemberError, CanonicalSetupMemberSource,
    CanonicalSetupSnapshotAuthority,
};

pub const GROUPS_NORMALIZE_FRAME: i32 = 384;
pub const GROUPS_NORMALIZE_SLOT: i32 = 0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame384GroupsCallSource {
    /// Complete retail playback through frame 384 step 12, immediately before
    /// `Groups::process` `0x006FA210` visits slot zero.
    AuthoritativePlaybackThroughFrame384GroupsCall,
}

/// Source-backed state at the exact callback boundary. Recorded checksum values are absent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame384GroupsCallAuthority {
    pub revision: u64,
    pub composition_digest: [u8; 32],
    pub source: Frame384GroupsCallSource,
    pub replay_file_sha256: [u8; 32],
    pub setup_composition_digest: [u8; 32],
    pub frame379_chronology_digest: [u8; 32],
    pub frame: i32,
    pub world_checksum: WorldChecksum,
    pub random_state: i32,
    pub leader_active: [bool; NUM_LEADERS],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame384GroupsProcessMountReceipt {
    pub chronology_revision: u64,
    pub chronology_digest: [u8; 32],
    pub source: Frame384GroupsCallSource,
    pub replay_file_sha256: [u8; 32],
    pub setup_composition_digest: [u8; 32],
    pub frame379_chronology_digest: [u8; 32],
    pub frame: i32,
    pub serial64_after: GroupsChecksum,
    pub before: GroupsChecksum,
    pub after: GroupsChecksum,
    pub canonical_setup_members: usize,
    pub group_authority: SetupGroupMoveAuthorityReceipt,
    pub process: GroupsProcessReceipt,
    pub random_state_before: i32,
    pub random_state_after: i32,
    pub next_exact_boundary: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame384GroupsProcessMountError {
    MissingChronologyRevision,
    MissingChronologyDigest,
    ChronologyReplayMismatch,
    ChronologySetupMismatch,
    Frame379ChronologyMismatch,
    InvalidFrame379Receipt,
    WrongFrame {
        expected: i32,
        actual: i32,
    },
    WrongCursor {
        expected: i32,
        actual: i32,
    },
    WorldMismatch,
    RandomMismatch,
    LeaderActiveMismatch,
    CommandStateRevisionMismatch {
        expected: u64,
        actual: u64,
    },
    Serial64GroupsMismatch {
        expected: GroupsChecksum,
        actual: GroupsChecksum,
    },
    SetupMember(CanonicalSetupMemberError),
    LandSpeedContent(ReplayLandSpeedContentError),
    GroupAuthority(SetupGroupMoveAuthorityError),
    GroupsChecksum(GroupSimChannelError),
    GroupsProcess(GroupsProcessAuthorityError),
    PostCommitRandomAdvanced {
        before: i32,
        after: i32,
    },
}

impl fmt::Display for Frame384GroupsProcessMountError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "2024 frame-384 Groups mount refused: {self:?}")
    }
}

impl std::error::Error for Frame384GroupsProcessMountError {}

impl From<CanonicalSetupMemberError> for Frame384GroupsProcessMountError {
    fn from(value: CanonicalSetupMemberError) -> Self {
        Self::SetupMember(value)
    }
}

impl From<ReplayLandSpeedContentError> for Frame384GroupsProcessMountError {
    fn from(value: ReplayLandSpeedContentError) -> Self {
        Self::LandSpeedContent(value)
    }
}

impl From<SetupGroupMoveAuthorityError> for Frame384GroupsProcessMountError {
    fn from(value: SetupGroupMoveAuthorityError) -> Self {
        Self::GroupAuthority(value)
    }
}

impl From<GroupSimChannelError> for Frame384GroupsProcessMountError {
    fn from(value: GroupSimChannelError) -> Self {
        Self::GroupsChecksum(value)
    }
}

impl From<GroupsProcessAuthorityError> for Frame384GroupsProcessMountError {
    fn from(value: GroupsProcessAuthorityError) -> Self {
        Self::GroupsProcess(value)
    }
}

fn validate_frame379_receipt(command: &Frame379GroupMoveMountReceipt) -> bool {
    command.replay_file_sha256 == REPLAY_FILE_SHA256
        && command.command_source == command.transition.source
        && command.command_source.package_frame == GROUP_MOVE_FRAME
        && command.command_source.lockstep_serial == GROUP_MOVE_SERIAL
        && command.command_source.play == PAIR_PLAY
        && command.transition.host.frame == GROUP_MOVE_FRAME
        && command.transition.host.lockstep_serial == GROUP_MOVE_SERIAL
        && command.transition.host.play == PAIR_PLAY
        && command.transition.host.who == 0
        && command.transition.host.group_slot == 0
        && command.transition.host.selected.len() == SELECTED_CITIZENS.len()
        && command
            .transition
            .host
            .selected
            .iter()
            .zip(SELECTED_CITIZENS)
            .all(|(identity, o)| identity.who == 0 && identity.o == o)
        && command.transition.channel_changed()
        && command.transition.host.groups_checksum == command.transition.after.checksum
        && command.group_authority.frame == GROUP_MOVE_FRAME
        && command.group_authority.setup_digest == command.setup_composition_digest
        && command.canonical_setup_members == SETUP_CALLS
}

/// Commit the exact frame-384 slot-zero `Groups::process` callback.
///
/// `candidate` must be the canonical Sim immediately before this one callback, not merely a Sim
/// whose frame scalar was changed to 384. Ownership makes refusal externally atomic: a caller can
/// discard the returned candidate, and no already-published owner is mutated.
pub fn mount_frame384_groups_process(
    replay: &Replay,
    setup: &Frame379SetupReceipt,
    command: &Frame379GroupMoveMountReceipt,
    mut candidate: Sim,
    chronology: &Frame384GroupsCallAuthority,
) -> Result<(Sim, Frame384GroupsProcessMountReceipt), (Frame384GroupsProcessMountError, Sim)> {
    let validate = || -> Result<GroupsChecksum, Frame384GroupsProcessMountError> {
        if chronology.revision == 0 {
            return Err(Frame384GroupsProcessMountError::MissingChronologyRevision);
        }
        if chronology.composition_digest == [0; 32] {
            return Err(Frame384GroupsProcessMountError::MissingChronologyDigest);
        }
        if chronology.replay_file_sha256 != REPLAY_FILE_SHA256
            || setup.replay_file_sha256 != REPLAY_FILE_SHA256
            || command.replay_file_sha256 != REPLAY_FILE_SHA256
        {
            return Err(Frame384GroupsProcessMountError::ChronologyReplayMismatch);
        }
        if chronology.setup_composition_digest != setup.canonical_composition_digest
            || command.setup_composition_digest != setup.canonical_composition_digest
        {
            return Err(Frame384GroupsProcessMountError::ChronologySetupMismatch);
        }
        if chronology.frame379_chronology_digest != command.chronology_digest {
            return Err(Frame384GroupsProcessMountError::Frame379ChronologyMismatch);
        }
        if !validate_frame379_receipt(command) {
            return Err(Frame384GroupsProcessMountError::InvalidFrame379Receipt);
        }
        if chronology.frame != GROUPS_NORMALIZE_FRAME
            || candidate.world.frame != GROUPS_NORMALIZE_FRAME
        {
            return Err(Frame384GroupsProcessMountError::WrongFrame {
                expected: GROUPS_NORMALIZE_FRAME,
                actual: candidate.world.frame,
            });
        }
        if candidate.groups.proc_group != GROUPS_NORMALIZE_SLOT {
            return Err(Frame384GroupsProcessMountError::WrongCursor {
                expected: GROUPS_NORMALIZE_SLOT,
                actual: candidate.groups.proc_group,
            });
        }
        if candidate.map.world.checksum_sections() != chronology.world_checksum {
            return Err(Frame384GroupsProcessMountError::WorldMismatch);
        }
        if candidate.world.random.state() != chronology.random_state {
            return Err(Frame384GroupsProcessMountError::RandomMismatch);
        }
        let active = std::array::from_fn(|who| candidate.leaders[who].active);
        if active != chronology.leader_active {
            return Err(Frame384GroupsProcessMountError::LeaderActiveMismatch);
        }
        let command_state_revision = candidate.command_package_state.revision();
        if command_state_revision != command.transition.host.command_state_revision {
            return Err(
                Frame384GroupsProcessMountError::CommandStateRevisionMismatch {
                    expected: command.transition.host.command_state_revision,
                    actual: command_state_revision,
                },
            );
        }
        let groups = sim_groups_checksum(&candidate.groups)?;
        if groups != command.transition.after {
            return Err(Frame384GroupsProcessMountError::Serial64GroupsMismatch {
                expected: command.transition.after,
                actual: groups,
            });
        }
        Ok(groups)
    };
    let before = match validate() {
        Ok(before) => before,
        Err(error) => return Err((error, candidate)),
    };

    let snapshot = CanonicalSetupSnapshotAuthority {
        revision: chronology.revision,
        composition_digest: chronology.composition_digest,
        source: CanonicalSetupMemberSource::ReplayRulesCompleteInitReceiptAndCanonicalSim,
        replay_file_sha256: chronology.replay_file_sha256,
        frame: chronology.frame,
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
        Err(error) => {
            return Err((
                Frame384GroupsProcessMountError::SetupMember(error),
                candidate,
            ))
        }
    };
    let content = match produce_replay_land_speed_content(replay) {
        Ok(content) => content,
        Err(error) => {
            return Err((
                Frame384GroupsProcessMountError::LandSpeedContent(error),
                candidate,
            ));
        }
    };
    let group_authority = match produce_replay_current_type_cohort_group_move_authority(
        &candidate,
        &members,
        &content,
        DESTINATION,
        false,
    ) {
        Ok(authority) => authority,
        Err(error) => {
            return Err((
                Frame384GroupsProcessMountError::GroupAuthority(error),
                candidate,
            ));
        }
    };
    candidate.replace_group_move_authority(group_authority.authority.clone());
    let prepared = match prepare_groups_process(
        &candidate.world,
        &candidate.groups,
        &chronology.leader_active,
        &candidate.group_move_authority,
    ) {
        Ok(prepared) => prepared,
        Err(error) => {
            return Err((
                Frame384GroupsProcessMountError::GroupsProcess(error),
                candidate,
            ));
        }
    };
    let process = match commit_groups_process(&mut candidate.groups, prepared) {
        Ok(receipt) => receipt,
        Err(error) => {
            return Err((
                Frame384GroupsProcessMountError::GroupsProcess(error),
                candidate,
            ));
        }
    };
    let after = match sim_groups_checksum(&candidate.groups) {
        Ok(after) => after,
        Err(error) => {
            return Err((
                Frame384GroupsProcessMountError::GroupsChecksum(error),
                candidate,
            ));
        }
    };
    let random_state_after = candidate.world.random.state();
    if random_state_after != chronology.random_state {
        return Err((
            Frame384GroupsProcessMountError::PostCommitRandomAdvanced {
                before: chronology.random_state,
                after: random_state_after,
            },
            candidate,
        ));
    }

    Ok((
        candidate,
        Frame384GroupsProcessMountReceipt {
            chronology_revision: chronology.revision,
            chronology_digest: chronology.composition_digest,
            source: chronology.source,
            replay_file_sha256: chronology.replay_file_sha256,
            setup_composition_digest: chronology.setup_composition_digest,
            frame379_chronology_digest: chronology.frame379_chronology_digest,
            frame: chronology.frame,
            serial64_after: command.transition.after,
            before,
            after,
            canonical_setup_members: members.len(),
            group_authority,
            process,
            random_state_before: chronology.random_state,
            random_state_after,
            next_exact_boundary:
                "complete frames 384..391 chronology, then compare the first changed recorded checkpoint",
        },
    ))
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use don_sim::systems::canonical_group_move_host::{
        GroupMoveAuthority, GroupMovePackageReceipt, UnitIdentity,
    };
    use don_sim::world::Handle;

    use super::*;
    use crate::groups_sim_channel::GroupChannelTransitionReceipt;
    use crate::setup_2024_frame379_group_move::{
        discover_frame379_group_move_source, Frame379CommandEntrySource,
    };
    use crate::setup_group_move_authority::{
        SetupGroupMoveAuthorityReceipt, SetupGroupMoveAuthoritySource,
    };

    fn repo_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf()
    }

    fn structural_receipt(replay: &Replay) -> Frame379GroupMoveMountReceipt {
        let source = discover_frame379_group_move_source(replay).unwrap();
        let before = GroupsChecksum {
            checksum: 0x1020_3040,
            bytes_walked: 36_896,
            retail_byte_counter: 36_864,
            groups_walked: 512,
        };
        let after = GroupsChecksum {
            checksum: 0x5060_7080,
            ..before
        };
        let selected = SELECTED_CITIZENS
            .into_iter()
            .enumerate()
            .map(|(index, o)| UnitIdentity {
                handle: Handle {
                    id: index as u32,
                    generation: 1,
                },
                who: 0,
                o,
                uid: index as u16,
            })
            .collect();
        let host = GroupMovePackageReceipt {
            play: PAIR_PLAY,
            lockstep_serial: GROUP_MOVE_SERIAL,
            frame: GROUP_MOVE_FRAME,
            who: 0,
            group_slot: 0,
            selected,
            command_state_revision: 1,
            groups_checksum: after.checksum,
            random_state_before: 0x1234,
            random_state_after: 0x1234,
        };
        Frame379GroupMoveMountReceipt {
            chronology_revision: 1,
            chronology_digest: [0x33; 32],
            source: Frame379CommandEntrySource::AuthoritativePlaybackChronologyFrameZeroThrough379,
            replay_file_sha256: REPLAY_FILE_SHA256,
            setup_composition_digest: [0x44; 32],
            command_source: source.clone(),
            canonical_setup_members: SETUP_CALLS,
            group_authority: SetupGroupMoveAuthorityReceipt {
                source:
                    SetupGroupMoveAuthoritySource::CompleteCanonicalSetupSnapshotAndBoundLandSpeed,
                setup_source:
                    CanonicalSetupMemberSource::ReplayRulesCompleteInitReceiptAndCanonicalSim,
                setup_revision: 1,
                setup_digest: [0x44; 32],
                replay_file_sha256: REPLAY_FILE_SHA256,
                frame: GROUP_MOVE_FRAME,
                setup_members: SETUP_CALLS,
                land_speed_revision: 1,
                land_speed_digest: [0x55; 32],
                land_speed_state_digest: 1,
                authority: GroupMoveAuthority::default(),
            },
            transition: GroupChannelTransitionReceipt {
                source,
                before,
                after,
                host,
            },
            next_exact_boundary: "frame384",
        }
    }

    #[test]
    fn frame379_handoff_validation_bites_structural_mutations() {
        let path =
            repo_root().join("ron-data/replays/multi/Playback___2024.02.23_20_49_35__Fri_.rcx");
        let Ok(replay) = Replay::open(&path) else {
            eprintln!("SKIPPED -- NOT A PASS: missing {}", path.display());
            return;
        };
        let receipt = structural_receipt(&replay);
        assert!(validate_frame379_receipt(&receipt));

        let mut wrong_serial = receipt.clone();
        wrong_serial.transition.host.lockstep_serial += 1;
        assert!(!validate_frame379_receipt(&wrong_serial));

        let mut stale_channel = receipt.clone();
        stale_channel.transition.after = stale_channel.transition.before;
        stale_channel.transition.host.groups_checksum = stale_channel.transition.after.checksum;
        assert!(!validate_frame379_receipt(&stale_channel));

        let mut wrong_identity = receipt;
        wrong_identity.transition.host.selected[2].o = 9;
        assert!(!validate_frame379_receipt(&wrong_identity));
    }
}
