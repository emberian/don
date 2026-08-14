//! Golden frame-8/16/24 `Wall::check_ever_seen(0)` early-return authority.
//!
//! Retail's Build-band `Build::process` begins with `Wall::process`.  On the owner-phased
//! eight-frame slot, that base call first decays `targeted`, then calls
//! `Wall::check_ever_seen(0)` (`0x0063CE70`).  The child has an important exact fast path:
//! started objects compare both `ever_seen` bytes with `Game::everyone_mask`; when both
//! already cover that mask, execution jumps to `0x0063CFD4`, observes that `ever_seen` did
//! not change and the argument is zero, and returns at `0x0063D074`.  No footprint, World
//! visibility plane, Leader notification, or `update_local_seen` child is reached.
//!
//! `Game::everyone_mask` is not saved by the compact Sim.  The supported retail executable
//! initializes it to zero, then `0x0058A2DB..0x0058A301` walks the eight Leader rows and sets
//! bit `who` exactly when `LeaderData::leader_flags & 1` is nonzero.  This module reconstructs
//! that one scalar from the fully captured frame-one Leader rows, joins it to the replay's
//! active cohort and the captured Village/Market identities and visibility bytes, and installs
//! a self-hashed sidecar.  The Sim still rechecks identity, frame, started state, and both live
//! bytes at every lookup.  Any mismatch therefore retains the existing atomic child boundary.

#![forbid(unsafe_code)]

use std::fmt;

use don_sim::systems::production;
use don_sim::systems::victory_score::NUM_LEADERS;
use don_sim::systems::walls::{
    build_wall_periodic_authority_digest, BuildWallPeriodicAuthority, BuildWallPeriodicIdentity,
    GOLDEN_BUILD_WALL_PERIODIC_FRAMES,
};
use don_sim::tick::Sim;

use crate::replay::Replay;
use crate::setup_2024_frame379::{
    Frame379SetupEntryReceipt, DUTCH_STARTING_MARKET_O, DUTCH_STARTING_MARKET_TYPE, OWNER,
    REPLAY_FILE_SHA256,
};
use crate::setup_2024_golden_capture::{
    validate_frame1_post_command_authority, Frame1GoldenBindError, Frame1PostCommandAuthority,
    StartingBuildInventory,
};
use crate::setup_cities_builds::CITY_CENTER_TYPE;
use crate::world_owner_frontier::sha256;

pub const WALL_CHECK_EVER_SEEN_VA: u32 = 0x0063_CE70;
pub const WALL_CHECK_EVER_SEEN_MASK_GATE_VA: u32 = 0x0063_CEC0;
pub const WALL_CHECK_EVER_SEEN_FAST_JOIN_VA: u32 = 0x0063_CFD4;
pub const WALL_CHECK_EVER_SEEN_RETURN_VA: u32 = 0x0063_D074;
pub const GAME_EVERYONE_MASK_ZERO_VA: u32 = 0x0058_9DA2;
pub const GAME_EVERYONE_MASK_WRITER_VA: u32 = 0x0058_A2DB;
pub const GAME_EVERYONE_MASK_WRITER_END_VA: u32 = 0x0058_A301;
pub const NEXT_EXACT_BOUNDARY: &str =
    "Market o2001 territory f15; Village o2000 territory f16; Market o2001 territory f31";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GoldenBuildWallPeriodicSource {
    /// Exact retail mask writer joined to the full captured frame-one Sim.
    RetailLeaderFlagsMaskWriterAndCapturedStartingBuilds,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GoldenBuildWallMaskWitness {
    pub identity: BuildWallPeriodicIdentity,
    pub flags: u8,
    pub ever_seen: u8,
    pub ever_seen_completed: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GoldenBuildWallPeriodicReceipt {
    pub revision: u64,
    pub composition_digest: [u8; 32],
    pub source: GoldenBuildWallPeriodicSource,
    pub replay_file_sha256: [u8; 32],
    pub frame1_composition_digest: [u8; 32],
    pub entry_frame: i32,
    pub everyone_mask: u8,
    pub replay_active_mask: u8,
    pub live_leader_flags_mask: u8,
    pub periodic_frames: [i32; 3],
    pub builds: [GoldenBuildWallMaskWitness; 2],
    pub installed_authority: BuildWallPeriodicAuthority,
    pub next_exact_boundary: &'static str,
}

#[derive(Debug)]
pub enum GoldenBuildWallPeriodicError {
    ReplayRead(String),
    ReplayMismatch,
    Frame1(Frame1GoldenBindError),
    ActiveOwnerOutOfRange(u8),
    WrongGoldenActiveCohort(u8),
    MissingLeaderRows,
    LeaderMaskMismatch {
        replay: u8,
        live: u8,
    },
    StartingBuildInventoryMismatch,
    MissingBuild(usize),
    BuildIdentityMismatch(usize),
    BuildNotStartedAndActive(usize),
    BuildMaskNotCovered {
        row: usize,
        everyone_mask: u8,
        ever_seen: u8,
        ever_seen_completed: u8,
    },
    EmptyAuthorityDigest,
}

impl fmt::Display for GoldenBuildWallPeriodicError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "2024 golden Build/Wall periodic bind refused: {self:?}")
    }
}

impl std::error::Error for GoldenBuildWallPeriodicError {}

impl From<Frame1GoldenBindError> for GoldenBuildWallPeriodicError {
    fn from(value: Frame1GoldenBindError) -> Self {
        Self::Frame1(value)
    }
}

fn replay_active_mask(replay: &Replay) -> Result<u8, GoldenBuildWallPeriodicError> {
    let mut mask = 0_u8;
    for player in replay.initial.active_players() {
        if usize::from(player.who) >= NUM_LEADERS {
            return Err(GoldenBuildWallPeriodicError::ActiveOwnerOutOfRange(
                player.who,
            ));
        }
        mask |= 1_u8 << player.who;
    }
    if mask != 1_u8 << OWNER {
        return Err(GoldenBuildWallPeriodicError::WrongGoldenActiveCohort(mask));
    }
    Ok(mask)
}

fn live_leader_flags_mask(sim: &Sim) -> Result<u8, GoldenBuildWallPeriodicError> {
    if sim.vic_leaders.slots.len() < NUM_LEADERS {
        return Err(GoldenBuildWallPeriodicError::MissingLeaderRows);
    }
    Ok(sim
        .vic_leaders
        .slots
        .iter()
        .take(NUM_LEADERS)
        .enumerate()
        .fold(0_u8, |mask, (who, leader)| {
            if leader.leader_flags & 1 != 0 {
                mask | (1_u8 << who)
            } else {
                mask
            }
        }))
}

fn expected_identities(
    inventory: &StartingBuildInventory,
) -> Result<[BuildWallPeriodicIdentity; 2], GoldenBuildWallPeriodicError> {
    if inventory.center_build_o != 2_000
        || inventory.center_type != CITY_CENTER_TYPE
        || inventory.market_build_o != DUTCH_STARTING_MARKET_O
        || inventory.market_type != DUTCH_STARTING_MARKET_TYPE
    {
        return Err(GoldenBuildWallPeriodicError::StartingBuildInventoryMismatch);
    }
    let center_o = i16::try_from(inventory.center_build_o)
        .map_err(|_| GoldenBuildWallPeriodicError::StartingBuildInventoryMismatch)?;
    let market_o = i16::try_from(inventory.market_build_o)
        .map_err(|_| GoldenBuildWallPeriodicError::StartingBuildInventoryMismatch)?;
    Ok([
        BuildWallPeriodicIdentity {
            row: inventory.center_build_row,
            who: OWNER,
            o: center_o,
            uid: inventory.center_uid,
            type_index: inventory.center_type,
        },
        BuildWallPeriodicIdentity {
            row: inventory.market_build_row,
            who: OWNER,
            o: market_o,
            uid: inventory.market_uid,
            type_index: inventory.market_type,
        },
    ])
}

fn plan_authority(
    inventory: &StartingBuildInventory,
    revision: u64,
    everyone_mask: u8,
    sim: &Sim,
) -> Result<
    ([GoldenBuildWallMaskWitness; 2], BuildWallPeriodicAuthority),
    GoldenBuildWallPeriodicError,
> {
    let identities = expected_identities(inventory)?;
    let active = production::flag::VALID | production::flag::STARTED | production::flag::ACTIVE;
    let mut witnesses = [GoldenBuildWallMaskWitness {
        identity: BuildWallPeriodicIdentity::default(),
        flags: 0,
        ever_seen: 0,
        ever_seen_completed: 0,
    }; 2];

    for (index, identity) in identities.iter().copied().enumerate() {
        let build = sim
            .builds
            .get(identity.row)
            .ok_or(GoldenBuildWallPeriodicError::MissingBuild(identity.row))?;
        let type_index = sim
            .production_runtime
            .build_types
            .get(identity.row)
            .copied()
            .flatten();
        if build.who != identity.who
            || build.object_id() != identity.o
            || build.uid != identity.uid
            || type_index != Some(identity.type_index)
        {
            return Err(GoldenBuildWallPeriodicError::BuildIdentityMismatch(
                identity.row,
            ));
        }
        if build.flags & active != active {
            return Err(GoldenBuildWallPeriodicError::BuildNotStartedAndActive(
                identity.row,
            ));
        }
        if build.ever_seen & everyone_mask != everyone_mask
            || build.ever_seen_completed & everyone_mask != everyone_mask
        {
            return Err(GoldenBuildWallPeriodicError::BuildMaskNotCovered {
                row: identity.row,
                everyone_mask,
                ever_seen: build.ever_seen,
                ever_seen_completed: build.ever_seen_completed,
            });
        }
        witnesses[index] = GoldenBuildWallMaskWitness {
            identity,
            flags: build.flags,
            ever_seen: build.ever_seen,
            ever_seen_completed: build.ever_seen_completed,
        };
    }

    let mut authority = BuildWallPeriodicAuthority {
        revision,
        composition_digest: 0,
        everyone_mask,
        frames: GOLDEN_BUILD_WALL_PERIODIC_FRAMES,
        builds: identities,
    };
    authority.composition_digest = build_wall_periodic_authority_digest(&authority);
    if authority.composition_digest == 0 {
        return Err(GoldenBuildWallPeriodicError::EmptyAuthorityDigest);
    }
    Ok((witnesses, authority))
}

pub fn golden_build_wall_periodic_composition_digest(
    receipt: &GoldenBuildWallPeriodicReceipt,
) -> [u8; 32] {
    let mut image = b"don-2024-golden-build-wall-periodic-v1".to_vec();
    image.extend_from_slice(&receipt.revision.to_le_bytes());
    image.push(match receipt.source {
        GoldenBuildWallPeriodicSource::RetailLeaderFlagsMaskWriterAndCapturedStartingBuilds => 1,
    });
    image.extend_from_slice(&receipt.replay_file_sha256);
    image.extend_from_slice(&receipt.frame1_composition_digest);
    image.extend_from_slice(&receipt.entry_frame.to_le_bytes());
    image.push(receipt.everyone_mask);
    image.push(receipt.replay_active_mask);
    image.push(receipt.live_leader_flags_mask);
    for frame in receipt.periodic_frames {
        image.extend_from_slice(&frame.to_le_bytes());
    }
    for witness in receipt.builds {
        image.extend_from_slice(&(witness.identity.row as u64).to_le_bytes());
        image.push(witness.identity.who);
        image.extend_from_slice(&witness.identity.o.to_le_bytes());
        image.extend_from_slice(&witness.identity.uid.to_le_bytes());
        image.extend_from_slice(&witness.identity.type_index.to_le_bytes());
        image.push(witness.flags);
        image.push(witness.ever_seen);
        image.push(witness.ever_seen_completed);
    }
    image.extend_from_slice(&receipt.installed_authority.revision.to_le_bytes());
    image.extend_from_slice(&receipt.installed_authority.composition_digest.to_le_bytes());
    image.push(receipt.installed_authority.everyone_mask);
    for frame in receipt.installed_authority.frames {
        image.extend_from_slice(&frame.to_le_bytes());
    }
    for identity in receipt.installed_authority.builds {
        image.extend_from_slice(&(identity.row as u64).to_le_bytes());
        image.push(identity.who);
        image.extend_from_slice(&identity.o.to_le_bytes());
        image.extend_from_slice(&identity.uid.to_le_bytes());
        image.extend_from_slice(&identity.type_index.to_le_bytes());
    }
    image.extend_from_slice(&GAME_EVERYONE_MASK_ZERO_VA.to_le_bytes());
    image.extend_from_slice(&GAME_EVERYONE_MASK_WRITER_VA.to_le_bytes());
    image.extend_from_slice(&GAME_EVERYONE_MASK_WRITER_END_VA.to_le_bytes());
    image.extend_from_slice(&WALL_CHECK_EVER_SEEN_VA.to_le_bytes());
    image.extend_from_slice(&WALL_CHECK_EVER_SEEN_MASK_GATE_VA.to_le_bytes());
    image.extend_from_slice(&WALL_CHECK_EVER_SEEN_FAST_JOIN_VA.to_le_bytes());
    image.extend_from_slice(&WALL_CHECK_EVER_SEEN_RETURN_VA.to_le_bytes());
    image.extend_from_slice(receipt.next_exact_boundary.as_bytes());
    sha256(&image)
}

/// Validate the complete frame-one after-image, then atomically install only the exact
/// frame-8/16/24 visibility no-op projection.  No Build or Leader state is changed.
pub fn mount_golden_build_wall_periodic_noops(
    replay: &Replay,
    setup_entry: &Frame379SetupEntryReceipt,
    frame1: &Frame1PostCommandAuthority,
    sim: &mut Sim,
) -> Result<GoldenBuildWallPeriodicReceipt, GoldenBuildWallPeriodicError> {
    let replay_bytes = std::fs::read(&replay.path)
        .map_err(|error| GoldenBuildWallPeriodicError::ReplayRead(error.to_string()))?;
    if sha256(&replay_bytes) != REPLAY_FILE_SHA256
        || frame1.replay_file_sha256 != REPLAY_FILE_SHA256
    {
        return Err(GoldenBuildWallPeriodicError::ReplayMismatch);
    }
    validate_frame1_post_command_authority(setup_entry, frame1, sim)?;

    let replay_mask = replay_active_mask(replay)?;
    let leader_mask = live_leader_flags_mask(sim)?;
    if replay_mask != leader_mask {
        return Err(GoldenBuildWallPeriodicError::LeaderMaskMismatch {
            replay: replay_mask,
            live: leader_mask,
        });
    }
    let (builds, authority) =
        plan_authority(&frame1.starting_builds, frame1.revision, leader_mask, sim)?;
    let mut receipt = GoldenBuildWallPeriodicReceipt {
        revision: frame1.revision,
        composition_digest: [0; 32],
        source: GoldenBuildWallPeriodicSource::RetailLeaderFlagsMaskWriterAndCapturedStartingBuilds,
        replay_file_sha256: REPLAY_FILE_SHA256,
        frame1_composition_digest: frame1.composition_digest,
        entry_frame: sim.world.frame,
        everyone_mask: leader_mask,
        replay_active_mask: replay_mask,
        live_leader_flags_mask: leader_mask,
        periodic_frames: GOLDEN_BUILD_WALL_PERIODIC_FRAMES,
        builds,
        installed_authority: authority.clone(),
        next_exact_boundary: NEXT_EXACT_BOUNDARY,
    };
    receipt.composition_digest = golden_build_wall_periodic_composition_digest(&receipt);
    if receipt.composition_digest == [0; 32] {
        return Err(GoldenBuildWallPeriodicError::EmptyAuthorityDigest);
    }

    sim.replace_build_wall_periodic_authority(authority);
    Ok(receipt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn inventory(rows: [usize; 2], uids: [u16; 2]) -> StartingBuildInventory {
        StartingBuildInventory {
            center_build_row: rows[0],
            center_build_o: 2_000,
            center_uid: uids[0],
            center_type: CITY_CENTER_TYPE,
            center_city_slot: 0,
            market_build_row: rows[1],
            market_build_o: DUTCH_STARTING_MARKET_O,
            market_uid: uids[1],
            market_type: DUTCH_STARTING_MARKET_TYPE,
            market_city_slot: 0,
        }
    }

    #[test]
    fn planner_binds_exact_starting_builds_masks_and_cadence() {
        let mut sim = Sim::new(30, 8);
        let mut rows = [0; 2];
        for (index, (uid, ty)) in [(17_u16, CITY_CENTER_TYPE), (18, DUTCH_STARTING_MARKET_TYPE)]
            .into_iter()
            .enumerate()
        {
            rows[index] = sim.spawn_build(
                OWNER as usize,
                production::BuildData {
                    flags: production::flag::VALID
                        | production::flag::STARTED
                        | production::flag::ACTIVE,
                    uid,
                    ever_seen: 1,
                    ever_seen_completed: 1,
                    ..Default::default()
                },
            );
            sim.production_runtime.register_build(rows[index], ty);
        }

        let (witnesses, authority) = plan_authority(&inventory(rows, [17, 18]), 9, 1, &sim)
            .expect("captured mask-covered Builds should bind");
        assert_eq!(witnesses.map(|witness| witness.identity.o), [2_000, 2_001]);
        assert_eq!(authority.frames, [8, 16, 24]);
        assert_eq!(
            authority.everyone_mask_for(8, rows[0], &sim.builds[rows[0]], Some(CITY_CENTER_TYPE)),
            Some(1)
        );
        assert_eq!(
            authority.everyone_mask_for(32, rows[0], &sim.builds[rows[0]], Some(CITY_CENTER_TYPE)),
            None
        );
    }

    #[test]
    fn planner_refuses_before_install_when_a_completed_mask_is_missing() {
        let mut sim = Sim::new(30, 8);
        let mut rows = [0; 2];
        for (index, (uid, ty)) in [(17_u16, CITY_CENTER_TYPE), (18, DUTCH_STARTING_MARKET_TYPE)]
            .into_iter()
            .enumerate()
        {
            rows[index] = sim.spawn_build(
                OWNER as usize,
                production::BuildData {
                    flags: production::flag::VALID
                        | production::flag::STARTED
                        | production::flag::ACTIVE,
                    uid,
                    ever_seen: 1,
                    ever_seen_completed: u8::from(index == 0),
                    ..Default::default()
                },
            );
            sim.production_runtime.register_build(rows[index], ty);
        }

        assert!(matches!(
            plan_authority(&inventory(rows, [17, 18]), 9, 1, &sim),
            Err(GoldenBuildWallPeriodicError::BuildMaskNotCovered { row, .. })
                if row == rows[1]
        ));
    }

    #[test]
    fn installed_golden_replay_has_the_bound_owner0_cohort() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../ron-data/replays/multi/Playback___2024.02.23_20_49_35__Fri_.rcx");
        if !path.exists() {
            eprintln!("SKIPPED -- NOT A PASS: missing {}", path.display());
            return;
        }
        let replay = Replay::open(&path).expect("installed 2024 witness must decode");
        assert_eq!(replay_active_mask(&replay).unwrap(), 1_u8 << OWNER);
    }
}
