//! Source-bound `Unit::set_anim(0, 0, 1)` authority for the 2024 setup Units.
//!
//! Frame-1 `Unit::do_idle` reaches this child before its later Unit/Leader writes. The replay
//! does not carry the live Guy animations which select the 4.7 KiB `Guy::set_anim` body, and
//! frame zero has already processed the Scout and Merchants. This module therefore does not
//! derive an after-image. It binds coherent supported-retail entry/return captures, including
//! the complete UnitGuys state and game RNG at every per-Guy child call.

#![forbid(unsafe_code)]

use std::fmt;

use don_sim::rng::Random;
use don_sim::systems::groups_guys::UnitGuys;
use don_sim::systems::save_load::{load_sim, save_sim, SaveError};
use don_sim::systems::setup_idle_prefix::IdleSetAnimRequest;
use don_sim::systems::unit_inctime::SUPPORTED_RETAIL_EXE_SHA256;
use don_sim::systems::unit_inctime::{anim_class, CLASS_ATTACK, CLASS_OUT_OF_TABLE};
use don_sim::tick::Sim;

use crate::replay::Replay;
use crate::setup_2024_frame1_leader_options::{
    LEADER_OPTIONS_FRAME, SETUP_CITIZEN_TYPE, SETUP_MERCHANT_TYPE, SETUP_SCOUT_TYPE,
};
use crate::setup_2024_frame379::{Frame379SetupReceipt, REPLAY_FILE_SHA256};
use crate::setup_unit_member_authority::{
    bind_canonical_setup_members, CanonicalSetupMemberError, CanonicalSetupMemberSource,
    CanonicalSetupSnapshotAuthority, CanonicalSetupUnitMemberReceipt,
};
use crate::world_owner_frontier::sha256;

pub const UNIT_SET_ANIM_VA: u32 = 0x0061_6f40;
pub const UNIT_SET_ANIM_RETURN_VA: u32 = 0x0061_7009;
pub const UNIT_SET_ANIM_BODY_BYTES: usize = 201;
pub const UNIT_SET_ANIM_BODY_SHA256: &str =
    "798f485753eb1853dc19ce55e43115674f6e3988370210dd9d5d4272d386f6ee";
pub const GUY_SET_ANIM_VA: u32 = 0x005d_a300;
pub const GUY_SET_ANIM_RETURN_VA: u32 = 0x005d_b573;
pub const GUY_SET_ANIM_BODY_BYTES: usize = 4_723;
pub const GUY_SET_ANIM_BODY_SHA256: &str =
    "be76d8eb8e4301d6c10888efa8b2ca1dde0ca02045f46b9c0c98b576d68f68b3";
pub const PROOF_DOCUMENT: &str = "docs/assembly/replay-2024-frame1-idle-set-anim.md";

const SETUP_TYPES: [i32; 7] = [
    SETUP_SCOUT_TYPE,
    SETUP_MERCHANT_TYPE,
    SETUP_MERCHANT_TYPE,
    SETUP_CITIZEN_TYPE,
    SETUP_CITIZEN_TYPE,
    SETUP_CITIZEN_TYPE,
    SETUP_CITIZEN_TYPE,
];
const SETUP_GUY_SHAPES: [(i32, i32); 7] = [(1, 1), (1, 2), (1, 2), (1, 0), (1, 0), (1, 0), (1, 0)];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame1IdleSetAnimCaptureSource {
    /// Coherent entry/return captures around `Unit::set_anim` `0x00616F40`, with a nested
    /// capture around every `Guy::set_anim` `0x005DA300` call.
    SupportedRetailUnitAndGuyCallTrace,
}

/// One source-captured `Guy::set_anim` child in Unit wrapper order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame1CapturedGuySetAnimCall {
    pub guy_index: usize,
    /// Complete receiver image after Unit's conditional `hold_attack=0` prefix and before
    /// the child call.
    pub guys_at_child_entry: UnitGuys,
    /// Complete receiver image immediately after the child returns.
    pub guys_at_child_return: UnitGuys,
    pub random_before: i32,
    pub random_after: i32,
    /// Number of `game_random` advances observed inside this child, including recursion.
    pub random_draws: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame1IdleSetAnimCapture {
    pub revision: u64,
    pub source: Frame1IdleSetAnimCaptureSource,
    pub replay_file_sha256: [u8; 32],
    pub executable_sha256: [u8; 32],
    pub request_authority_digest: [u8; 32],
    pub before_sim_sha256: [u8; 32],
    pub after_sim_sha256: [u8; 32],
    pub calls: Vec<Frame1CapturedGuySetAnimCall>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame1IdleSetAnimReceipt {
    pub composition_digest: [u8; 32],
    pub capture_revision: u64,
    pub source: Frame1IdleSetAnimCaptureSource,
    pub replay_file_sha256: [u8; 32],
    pub executable_sha256: [u8; 32],
    pub request: IdleSetAnimRequest,
    pub setup_ordinal: usize,
    pub row: usize,
    pub type_index: i32,
    pub before_sim_sha256: [u8; 32],
    pub after_sim_sha256: [u8; 32],
    pub guys_before: UnitGuys,
    pub guys_after: UnitGuys,
    pub random_before: i32,
    pub random_after: i32,
    pub calls: Vec<Frame1CapturedGuySetAnimCall>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Frame1IdleSetAnimError {
    MissingRequestRevision,
    MissingRequestDigest,
    MissingCaptureRevision,
    RequestDigestMismatch,
    ReplayMismatch,
    UnsupportedExecutable,
    SetupMismatch,
    WrongFrame { expected: i32, actual: i32 },
    WrongCallArguments,
    Snapshot(SaveError),
    BeforeSnapshotMismatch,
    AfterSnapshotMismatch,
    SetupMember(CanonicalSetupMemberError),
    WrongSetupCohort,
    StaleUnit,
    MissingUnitGuys,
    GuyImageMismatch,
    InvalidGuyArray,
    GuyIdentityMismatch { index: usize },
    UnsupportedAnimation { index: usize, animation: i8 },
    WrongCallCount { expected: usize, actual: usize },
    WrongCallOrder { expected: usize, actual: usize },
    ChildEntryMismatch { index: usize },
    ChildRandomMismatch { index: usize },
    ChildRandomDrawMismatch { index: usize },
    ChildReturnTopologyMismatch { index: usize },
    FinalGuyImageMismatch,
    FinalRandomMismatch,
    UnrelatedCanonicalStateChanged,
}

impl fmt::Display for Frame1IdleSetAnimError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "2024 frame-1 idle set-animation capture refused: {self:?}"
        )
    }
}

impl std::error::Error for Frame1IdleSetAnimError {}

impl From<CanonicalSetupMemberError> for Frame1IdleSetAnimError {
    fn from(value: CanonicalSetupMemberError) -> Self {
        Self::SetupMember(value)
    }
}

fn snapshot_authority(
    setup: &Frame379SetupReceipt,
    revision: u64,
    digest: [u8; 32],
    sim: &Sim,
) -> CanonicalSetupSnapshotAuthority {
    CanonicalSetupSnapshotAuthority {
        revision,
        composition_digest: digest,
        source: CanonicalSetupMemberSource::ReplayRulesCompleteInitReceiptAndCanonicalSim,
        replay_file_sha256: setup.replay_file_sha256,
        frame: sim.world.frame,
        world_checksum: sim.map.world.checksum_sections(),
        random_state: sim.world.random.state(),
    }
}

fn bind_setup_cohort(
    replay: &Replay,
    setup: &Frame379SetupReceipt,
    sim: &Sim,
    authority: &CanonicalSetupSnapshotAuthority,
) -> Result<Vec<CanonicalSetupUnitMemberReceipt>, Frame1IdleSetAnimError> {
    if sim.world.unit_mark(0) != Some(7) {
        return Err(Frame1IdleSetAnimError::WrongSetupCohort);
    }
    let ordinals = (0..SETUP_TYPES.len()).collect::<Vec<_>>();
    let members =
        bind_canonical_setup_members(replay, &setup.plan, &setup.setup, sim, &ordinals, authority)?;
    if members.len() != SETUP_TYPES.len() {
        return Err(Frame1IdleSetAnimError::WrongSetupCohort);
    }
    for (ordinal, member) in members.iter().enumerate() {
        if member.setup_ordinal != ordinal
            || member.member_ordinal != 0
            || member.current_type != SETUP_TYPES[ordinal]
            || member.type_facts.type_index != SETUP_TYPES[ordinal]
            || (member.type_facts.squad_size, member.type_facts.crew_size)
                != SETUP_GUY_SHAPES[ordinal]
            || member.unit.identity.who != 0
            || i32::from(member.unit.identity.o) != ordinal as i32
        {
            return Err(Frame1IdleSetAnimError::WrongSetupCohort);
        }
    }
    Ok(members)
}

fn validate_complete_guys(
    guys: &UnitGuys,
    who: i8,
    o: i16,
    type_index: i32,
    squad_size: i32,
    crew_size: i32,
) -> Result<(), Frame1IdleSetAnimError> {
    let squad = usize::try_from(squad_size)
        .ok()
        .ok_or(Frame1IdleSetAnimError::InvalidGuyArray)?;
    let crew = usize::try_from(crew_size)
        .ok()
        .ok_or(Frame1IdleSetAnimError::InvalidGuyArray)?;
    let mark = usize::try_from(guys.guy_mark)
        .ok()
        .filter(|&mark| mark <= squad)
        .ok_or(Frame1IdleSetAnimError::InvalidGuyArray)?;
    if guys.guys.len() != squad + crew
        || guys.size < 0
        || guys.guys.len() > guys.size as usize
        || guys.guys[..mark].iter().any(Option::is_none)
        || guys.guys[mark..squad].iter().any(Option::is_some)
        || guys.guys[squad..].iter().any(Option::is_none)
    {
        return Err(Frame1IdleSetAnimError::InvalidGuyArray);
    }
    for (index, guy) in guys.guys.iter().enumerate() {
        let Some(guy) = guy else { continue };
        if guy.who != who
            || guy.o != o
            || guy.ty != type_index
            || usize::try_from(guy.guy_num).ok() != Some(index)
        {
            return Err(Frame1IdleSetAnimError::GuyIdentityMismatch { index });
        }
        if anim_class(guy.cur_anim) == CLASS_OUT_OF_TABLE {
            return Err(Frame1IdleSetAnimError::UnsupportedAnimation {
                index,
                animation: guy.cur_anim,
            });
        }
    }
    Ok(())
}

fn same_topology(a: &UnitGuys, b: &UnitGuys) -> bool {
    a.guys.len() == b.guys.len()
        && a.size == b.size
        && a.increment == b.increment
        && a.flags == b.flags
        && a.guy_mark == b.guy_mark
        && a.guys
            .iter()
            .zip(&b.guys)
            .all(|(a, b)| a.is_some() == b.is_some())
}

fn expected_call_order(guys: &UnitGuys, squad_size: i32) -> Vec<usize> {
    let mark = guys.guy_mark as usize;
    let squad = squad_size as usize;
    (0..mark).chain(squad..guys.guys.len()).collect()
}

fn clear_hold_attack_prefix(
    running: &UnitGuys,
    guy_index: usize,
) -> Result<UnitGuys, Frame1IdleSetAnimError> {
    let Some(Some(lead)) = running.guys.first() else {
        return Err(Frame1IdleSetAnimError::InvalidGuyArray);
    };
    let lead_class = anim_class(lead.cur_anim);
    if lead_class == CLASS_OUT_OF_TABLE {
        return Err(Frame1IdleSetAnimError::UnsupportedAnimation {
            index: 0,
            animation: lead.cur_anim,
        });
    }
    let mut entry = running.clone();
    if lead_class != CLASS_ATTACK {
        entry.guys[guy_index]
            .as_mut()
            .ok_or(Frame1IdleSetAnimError::InvalidGuyArray)?
            .hold_attack = 0;
    }
    Ok(entry)
}

fn lcg_state_after(state: i32, mut draws: u32) -> i32 {
    // Exponentiate the affine LCG transform, so a corrupt capture cannot force a huge loop.
    let mut acc_mul = 1u32;
    let mut acc_add = 0u32;
    let mut cur_mul = Random::MUL as u32;
    let mut cur_add = Random::ADD as u32;
    while draws != 0 {
        if draws & 1 != 0 {
            acc_mul = acc_mul.wrapping_mul(cur_mul);
            acc_add = acc_add.wrapping_mul(cur_mul).wrapping_add(cur_add);
        }
        cur_add = cur_add.wrapping_mul(cur_mul.wrapping_add(1));
        cur_mul = cur_mul.wrapping_mul(cur_mul);
        draws >>= 1;
    }
    (acc_mul.wrapping_mul(state as u32).wrapping_add(acc_add)) as i32
}

fn append_guy_image(image: &mut Vec<u8>, guys: &UnitGuys) {
    image.extend_from_slice(&(guys.guys.len() as u64).to_le_bytes());
    image.extend_from_slice(&guys.size.to_le_bytes());
    image.extend_from_slice(&guys.increment.to_le_bytes());
    image.push(guys.flags);
    image.push(guys.guy_mark as u8);
    for guy in &guys.guys {
        image.push(u8::from(guy.is_some()));
    }
    for guy in guys.guys.iter().flatten() {
        image.extend_from_slice(&guy.walk_bytes());
    }
}

fn validate_call_journal(
    request: &IdleSetAnimRequest,
    type_index: i32,
    squad_size: i32,
    crew_size: i32,
    calls: &[Frame1CapturedGuySetAnimCall],
    guys_after: &UnitGuys,
    random_after: i32,
) -> Result<(), Frame1IdleSetAnimError> {
    let order = expected_call_order(&request.guys_before, squad_size);
    if calls.len() != order.len() {
        return Err(Frame1IdleSetAnimError::WrongCallCount {
            expected: order.len(),
            actual: calls.len(),
        });
    }
    let mut running = request.guys_before.clone();
    let mut random = request.random_before;
    for (&expected_index, call) in order.iter().zip(calls) {
        if call.guy_index != expected_index {
            return Err(Frame1IdleSetAnimError::WrongCallOrder {
                expected: expected_index,
                actual: call.guy_index,
            });
        }
        let child_entry = clear_hold_attack_prefix(&running, expected_index)?;
        if call.guys_at_child_entry != child_entry {
            return Err(Frame1IdleSetAnimError::ChildEntryMismatch {
                index: expected_index,
            });
        }
        if call.random_before != random {
            return Err(Frame1IdleSetAnimError::ChildRandomMismatch {
                index: expected_index,
            });
        }
        if lcg_state_after(call.random_before, call.random_draws) != call.random_after {
            return Err(Frame1IdleSetAnimError::ChildRandomDrawMismatch {
                index: expected_index,
            });
        }
        validate_complete_guys(
            &call.guys_at_child_return,
            request.who as i8,
            request.o,
            type_index,
            squad_size,
            crew_size,
        )?;
        if !same_topology(&call.guys_at_child_entry, &call.guys_at_child_return) {
            return Err(Frame1IdleSetAnimError::ChildReturnTopologyMismatch {
                index: expected_index,
            });
        }
        running = call.guys_at_child_return.clone();
        random = call.random_after;
    }
    if &running != guys_after {
        return Err(Frame1IdleSetAnimError::FinalGuyImageMismatch);
    }
    if random != random_after {
        return Err(Frame1IdleSetAnimError::FinalRandomMismatch);
    }
    Ok(())
}

/// Bind one exact source-captured `Unit::set_anim(0,0,1)` entry/return pair.
///
/// This function is read-only. It returns an authority receipt for a later idle continuation;
/// it never installs `guys_after` or reseeds a candidate Sim. To exclude a conveniently edited
/// after-image, it reloads the captured after Sim, restores only this Unit's complete Guys owner
/// and RNG to their entry values, and requires the resulting DoNSave bytes to equal the complete
/// captured before Sim byte-for-byte.
pub fn bind_captured_frame1_idle_set_anim(
    replay: &Replay,
    setup: &Frame379SetupReceipt,
    request: &IdleSetAnimRequest,
    before: &Sim,
    after: &Sim,
    capture: &Frame1IdleSetAnimCapture,
) -> Result<Frame1IdleSetAnimReceipt, Frame1IdleSetAnimError> {
    if request.authority_revision == 0 {
        return Err(Frame1IdleSetAnimError::MissingRequestRevision);
    }
    if request.authority_digest == [0; 32] {
        return Err(Frame1IdleSetAnimError::MissingRequestDigest);
    }
    if capture.revision == 0 {
        return Err(Frame1IdleSetAnimError::MissingCaptureRevision);
    }
    if capture.request_authority_digest != request.authority_digest {
        return Err(Frame1IdleSetAnimError::RequestDigestMismatch);
    }
    if capture.replay_file_sha256 != REPLAY_FILE_SHA256
        || setup.replay_file_sha256 != REPLAY_FILE_SHA256
    {
        return Err(Frame1IdleSetAnimError::ReplayMismatch);
    }
    if capture.executable_sha256 != SUPPORTED_RETAIL_EXE_SHA256 {
        return Err(Frame1IdleSetAnimError::UnsupportedExecutable);
    }
    if setup.canonical_frame != 0 || setup.canonical_composition_digest == [0; 32] {
        return Err(Frame1IdleSetAnimError::SetupMismatch);
    }
    if request.frame != LEADER_OPTIONS_FRAME as i32
        || before.world.frame != request.frame
        || after.world.frame != request.frame
    {
        return Err(Frame1IdleSetAnimError::WrongFrame {
            expected: LEADER_OPTIONS_FRAME as i32,
            actual: before.world.frame,
        });
    }
    if request.animation != 0 || request.arg2 != 0 || request.arg3 != 1 {
        return Err(Frame1IdleSetAnimError::WrongCallArguments);
    }
    let before_bytes = save_sim(before).map_err(Frame1IdleSetAnimError::Snapshot)?;
    let after_bytes = save_sim(after).map_err(Frame1IdleSetAnimError::Snapshot)?;
    if sha256(&before_bytes) != capture.before_sim_sha256 {
        return Err(Frame1IdleSetAnimError::BeforeSnapshotMismatch);
    }
    if sha256(&after_bytes) != capture.after_sim_sha256 {
        return Err(Frame1IdleSetAnimError::AfterSnapshotMismatch);
    }

    let before_authority = snapshot_authority(
        setup,
        request.authority_revision,
        request.authority_digest,
        before,
    );
    let after_authority =
        snapshot_authority(setup, capture.revision, capture.after_sim_sha256, after);
    let before_members = bind_setup_cohort(replay, setup, before, &before_authority)?;
    let after_members = bind_setup_cohort(replay, setup, after, &after_authority)?;
    if before_members
        .iter()
        .zip(&after_members)
        .any(|(before, after)| {
            before.setup_ordinal != after.setup_ordinal
                || before.current_type != after.current_type
                || before.unit.identity != after.unit.identity
        })
    {
        return Err(Frame1IdleSetAnimError::WrongSetupCohort);
    }
    let (setup_ordinal, member) = before_members
        .iter()
        .enumerate()
        .find(|(_, member)| member.unit.identity.handle == request.unit)
        .ok_or(Frame1IdleSetAnimError::StaleUnit)?;
    if setup_ordinal > 6
        || request.who != 0
        || i32::from(request.o) != setup_ordinal as i32
        || member.current_type != SETUP_TYPES[setup_ordinal]
        || member.unit.identity.who != request.who
        || member.unit.identity.o != request.o
    {
        return Err(Frame1IdleSetAnimError::StaleUnit);
    }
    let row = member.row;
    let guys_before = before
        .unit_guys
        .get(row)
        .and_then(Option::as_ref)
        .ok_or(Frame1IdleSetAnimError::MissingUnitGuys)?;
    let after_row = after
        .world
        .row_of(request.unit)
        .ok_or(Frame1IdleSetAnimError::StaleUnit)?;
    let guys_after = after
        .unit_guys
        .get(after_row)
        .and_then(Option::as_ref)
        .ok_or(Frame1IdleSetAnimError::MissingUnitGuys)?;
    if guys_before != &request.guys_before
        || before.world.random.state() != request.random_before
        || before.world.units.guy_mark()[row] != request.guys_before.guy_mark
        || after.world.units.guy_mark()[after_row] != guys_after.guy_mark
    {
        return Err(Frame1IdleSetAnimError::GuyImageMismatch);
    }
    validate_complete_guys(
        guys_before,
        request.who as i8,
        request.o,
        member.current_type,
        member.type_facts.squad_size,
        member.type_facts.crew_size,
    )?;
    validate_complete_guys(
        guys_after,
        request.who as i8,
        request.o,
        member.current_type,
        member.type_facts.squad_size,
        member.type_facts.crew_size,
    )?;
    validate_call_journal(
        request,
        member.current_type,
        member.type_facts.squad_size,
        member.type_facts.crew_size,
        &capture.calls,
        guys_after,
        after.world.random.state(),
    )?;

    // Prove that the complete canonical after-image differs only in the exact UnitGuys owner
    // and game RNG carried by this source trace.
    let mut normalized = load_sim(&after_bytes).map_err(Frame1IdleSetAnimError::Snapshot)?;
    let normalized_row = normalized
        .world
        .row_of(request.unit)
        .ok_or(Frame1IdleSetAnimError::StaleUnit)?;
    normalized.unit_guys[normalized_row] = Some(request.guys_before.clone());
    normalized.world.random.reseed(request.random_before);
    let normalized_bytes = save_sim(&normalized).map_err(Frame1IdleSetAnimError::Snapshot)?;
    if normalized_bytes != before_bytes {
        return Err(Frame1IdleSetAnimError::UnrelatedCanonicalStateChanged);
    }

    let mut image = b"don-frame1-idle-set-anim-capture-v1".to_vec();
    image.extend_from_slice(&capture.revision.to_le_bytes());
    image.extend_from_slice(&capture.replay_file_sha256);
    image.extend_from_slice(&capture.executable_sha256);
    image.extend_from_slice(&request.authority_revision.to_le_bytes());
    image.extend_from_slice(&request.authority_digest);
    image.extend_from_slice(&capture.before_sim_sha256);
    image.extend_from_slice(&capture.after_sim_sha256);
    image.extend_from_slice(&request.unit.id.to_le_bytes());
    image.extend_from_slice(&request.unit.generation.to_le_bytes());
    image.extend_from_slice(&(setup_ordinal as u64).to_le_bytes());
    image.extend_from_slice(&member.current_type.to_le_bytes());
    append_guy_image(&mut image, guys_before);
    append_guy_image(&mut image, guys_after);
    image.extend_from_slice(&request.random_before.to_le_bytes());
    image.extend_from_slice(&after.world.random.state().to_le_bytes());
    for call in &capture.calls {
        image.extend_from_slice(&(call.guy_index as u64).to_le_bytes());
        append_guy_image(&mut image, &call.guys_at_child_entry);
        append_guy_image(&mut image, &call.guys_at_child_return);
        image.extend_from_slice(&call.random_before.to_le_bytes());
        image.extend_from_slice(&call.random_after.to_le_bytes());
        image.extend_from_slice(&call.random_draws.to_le_bytes());
    }

    Ok(Frame1IdleSetAnimReceipt {
        composition_digest: sha256(&image),
        capture_revision: capture.revision,
        source: capture.source,
        replay_file_sha256: capture.replay_file_sha256,
        executable_sha256: capture.executable_sha256,
        request: request.clone(),
        setup_ordinal,
        row,
        type_index: member.current_type,
        before_sim_sha256: capture.before_sim_sha256,
        after_sim_sha256: capture.after_sim_sha256,
        guys_before: guys_before.clone(),
        guys_after: guys_after.clone(),
        random_before: request.random_before,
        random_after: after.world.random.state(),
        calls: capture.calls.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use don_sim::systems::groups_guys::GuyData;
    use don_sim::world::Handle;

    fn guy(index: i8, animation: i8, hold_attack: i8) -> Option<GuyData> {
        Some(GuyData {
            ty: 62,
            who: 0,
            o: 1,
            guy_num: index,
            cur_anim: animation,
            hold_attack,
            ..GuyData::default()
        })
    }

    #[test]
    fn unit_wrapper_uses_the_lead_animation_for_every_hold_attack_clear() {
        let running = UnitGuys {
            guys: vec![guy(0, 0, 7), guy(1, 12, 9), guy(2, 12, 11)],
            size: 3,
            increment: 1,
            flags: 0,
            guy_mark: 1,
        };
        let entry = clear_hold_attack_prefix(&running, 2).unwrap();
        assert_eq!(entry.guys[0].unwrap().hold_attack, 7);
        assert_eq!(entry.guys[1].unwrap().hold_attack, 9);
        assert_eq!(entry.guys[2].unwrap().hold_attack, 0);

        let mut attacking_lead = running;
        attacking_lead.guys[0].as_mut().unwrap().cur_anim = 12;
        let entry = clear_hold_attack_prefix(&attacking_lead, 2).unwrap();
        assert_eq!(entry.guys[2].unwrap().hold_attack, 11);
    }

    #[test]
    fn captured_draw_count_is_checked_without_iterating_the_capture_count() {
        let seed = 0x1234_5678;
        let mut rng = Random::new(seed);
        for draws in 0..32 {
            assert_eq!(lcg_state_after(seed, draws), rng.state());
            rng.advance();
        }
        assert_ne!(lcg_state_after(seed, u32::MAX), seed);
    }

    #[test]
    fn supported_retail_body_anchors_are_frozen() {
        assert_eq!(UNIT_SET_ANIM_VA, 0x0061_6f40);
        assert_eq!(UNIT_SET_ANIM_RETURN_VA, 0x0061_7009);
        assert_eq!(UNIT_SET_ANIM_BODY_BYTES, 201);
        assert_eq!(
            UNIT_SET_ANIM_BODY_SHA256,
            "798f485753eb1853dc19ce55e43115674f6e3988370210dd9d5d4272d386f6ee"
        );
        assert_eq!(GUY_SET_ANIM_VA, 0x005d_a300);
        assert_eq!(GUY_SET_ANIM_RETURN_VA, 0x005d_b573);
        assert_eq!(GUY_SET_ANIM_BODY_BYTES, 4_723);
        assert_eq!(
            GUY_SET_ANIM_BODY_SHA256,
            "be76d8eb8e4301d6c10888efa8b2ca1dde0ca02045f46b9c0c98b576d68f68b3"
        );
        assert_eq!(
            PROOF_DOCUMENT,
            "docs/assembly/replay-2024-frame1-idle-set-anim.md"
        );
    }

    #[test]
    fn exact_merchant_journal_tracks_dynamic_lead_class_and_rng() {
        let before = UnitGuys {
            guys: vec![guy(0, 0, 7), guy(1, 12, 9), guy(2, 12, 11)],
            size: 3,
            increment: 1,
            flags: 0,
            guy_mark: 1,
        };
        let request = IdleSetAnimRequest {
            authority_revision: 1,
            authority_digest: [1; 32],
            frame: 1,
            unit: Handle {
                id: 7,
                generation: 3,
            },
            who: 0,
            o: 1,
            animation: 0,
            arg2: 0,
            arg3: 1,
            guys_before: before.clone(),
            random_before: 0x1234_5678,
        };

        let call0_entry = clear_hold_attack_prefix(&before, 0).unwrap();
        let mut call0_return = call0_entry.clone();
        call0_return.guys[0].as_mut().unwrap().cur_anim = 11;
        call0_return.guys[0].as_mut().unwrap().cur_time = 4;
        let after_draw = lcg_state_after(request.random_before, 1);

        // Guy 0 now has attack class 12, so the wrapper must retain the crew Guys'
        // hold_attack bytes even though those addressed Guys were already attacking too.
        let call1_entry = clear_hold_attack_prefix(&call0_return, 1).unwrap();
        let mut call1_return = call1_entry.clone();
        call1_return.guys[1].as_mut().unwrap().cur_time = 5;
        let call2_entry = clear_hold_attack_prefix(&call1_return, 2).unwrap();
        let mut call2_return = call2_entry.clone();
        call2_return.guys[2].as_mut().unwrap().cur_time = 6;

        let calls = vec![
            Frame1CapturedGuySetAnimCall {
                guy_index: 0,
                guys_at_child_entry: call0_entry,
                guys_at_child_return: call0_return,
                random_before: request.random_before,
                random_after: after_draw,
                random_draws: 1,
            },
            Frame1CapturedGuySetAnimCall {
                guy_index: 1,
                guys_at_child_entry: call1_entry,
                guys_at_child_return: call1_return,
                random_before: after_draw,
                random_after: after_draw,
                random_draws: 0,
            },
            Frame1CapturedGuySetAnimCall {
                guy_index: 2,
                guys_at_child_entry: call2_entry,
                guys_at_child_return: call2_return.clone(),
                random_before: after_draw,
                random_after: after_draw,
                random_draws: 0,
            },
        ];
        validate_call_journal(&request, 62, 1, 2, &calls, &call2_return, after_draw).unwrap();

        let mut wrong_entry = calls.clone();
        wrong_entry[1].guys_at_child_entry.guys[1]
            .as_mut()
            .unwrap()
            .hold_attack = 0;
        assert_eq!(
            validate_call_journal(&request, 62, 1, 2, &wrong_entry, &call2_return, after_draw,),
            Err(Frame1IdleSetAnimError::ChildEntryMismatch { index: 1 })
        );

        let mut wrong_draw_count = calls;
        wrong_draw_count[0].random_draws = 0;
        assert_eq!(
            validate_call_journal(
                &request,
                62,
                1,
                2,
                &wrong_draw_count,
                &call2_return,
                after_draw,
            ),
            Err(Frame1IdleSetAnimError::ChildRandomDrawMismatch { index: 0 })
        );
    }
}
