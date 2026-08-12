// SPDX-License-Identifier: GPL-3.0-or-later
//! Canonical transaction contract for the first economy/containment Group cohort.
//!
//! This module is deliberately outside `systems/mod.rs`.  It freezes the future production
//! boundary for `BOARD_SHIP` (opcode 15), `REPAIR` (16), and `TRADE` (17) without claiming
//! that the command bridge reaches [`crate::systems::groups_guys::Groups`] yet.  In
//! particular, [`EconomyGroupTransactionRequest::snapshot`] accepts only that fixed,
//! save/checksum-owned pool.  The similarly named private pool in `command.rs` is not an
//! admissible authority.

use crate::command::economy_group_actions::{
    plan_board_ship, plan_repair, plan_trade, EconomyPlan, EconomyStep, Fact, MemberFacts,
    TradeTargetFacts, QUEUE_FIRST,
};
use crate::systems::groups_guys::{
    GroupData, Groups, GROUPS_PER_PLAYER, GROUP_MAX_MEMBERS, NUM_LEADERS,
};

pub const BOARD_SHIP_OPCODE: u8 = 15;
pub const REPAIR_OPCODE: u8 = 16;
pub const TRADE_OPCODE: u8 = 17;
pub const GROUP_OPCODE: u8 = 0;

pub const BOARD_SHIP_WIRE_SIZE: usize = 9;
pub const REPAIR_WIRE_SIZE: usize = 13;
pub const TRADE_WIRE_SIZE: usize = 21;
pub const GROUP_WIRE_PREFIX: usize = 3;

/// First DoNSave version reserving the typed Move/Gather/Cast/Trade tail. Later formats,
/// including v13's preceding node metric, retain this tail byte-for-byte.
pub const REQUIRED_ORDER_FORMAT_VERSION: u32 = 12;

/// Typed-v12 payload tags already reserved by the shared order-envelope owner.
pub const GATHER_PAYLOAD_TAG: u8 = 2;
pub const CAST_PAYLOAD_TAG: u8 = 3;
pub const TRADE_PAYLOAD_TAG: u8 = 4;
pub const ECONOMY_PAYLOAD_VERSION: u8 = 1;

/// The byte-exact concrete portion appended after the canonical generic Order fields.
///
/// The full retail walk includes the virtual `flags` byte and ten-byte primary target
/// identity. Cast additionally obtains `x/y` from the generic Order header, so only its
/// eight-byte history is new in the v12 envelope. Trade's second endpoint remains a typed
/// identity even though the retail walk carries only its object, owner, and UID scalars.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EconomyOrderPayloadContract {
    GatherV1,
    CastV1,
    TradeV1,
}

impl EconomyOrderPayloadContract {
    pub const fn tag(self) -> u8 {
        match self {
            Self::GatherV1 => GATHER_PAYLOAD_TAG,
            Self::CastV1 => CAST_PAYLOAD_TAG,
            Self::TradeV1 => TRADE_PAYLOAD_TAG,
        }
    }

    pub const fn payload_version(self) -> u8 {
        ECONOMY_PAYLOAD_VERSION
    }

    pub const fn v12_extra_bytes(self) -> usize {
        match self {
            Self::GatherV1 => 20,
            Self::CastV1 => 8,
            Self::TradeV1 => 18,
        }
    }

    pub const fn retail_walk_bytes(self) -> usize {
        match self {
            Self::GatherV1 => 31,
            Self::CastV1 => 27,
            Self::TradeV1 => 29,
        }
    }
}

/// One fixed slot in the canonical player-major `GroupsData` pool.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CanonicalGroupLocator {
    pub owner: u8,
    pub slot: u8,
}

impl CanonicalGroupLocator {
    pub fn index(self) -> Option<usize> {
        let owner = usize::from(self.owner);
        let slot = usize::from(self.slot);
        (owner < NUM_LEADERS && slot < GROUPS_PER_PLAYER)
            .then_some(owner * GROUPS_PER_PLAYER + slot)
    }

    pub fn from_index(index: usize) -> Option<Self> {
        (index < NUM_LEADERS * GROUPS_PER_PLAYER).then_some(Self {
            owner: (index / GROUPS_PER_PLAYER) as u8,
            slot: (index % GROUPS_PER_PLAYER) as u8,
        })
    }
}

/// Stable identity captured from the authoritative Object registry before planning.
///
/// `generation` is the canonical layer's anti-ABA identity.  `uid` is retained separately
/// because the shipped order payloads walk it and retail validates it at different points.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CanonicalObjectIdentity {
    pub owner: u8,
    pub o: i16,
    pub uid: u16,
    pub generation: u32,
}

impl CanonicalObjectIdentity {
    fn addresses(self, owner: i32, o: i32) -> bool {
        owner == i32::from(self.owner) && o == i32::from(self.o)
    }
}

/// Raw arguments retained from the real command packet until the canonical host resolves
/// every object and Group identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EconomyGroupAction {
    BoardShip {
        ship_o: i32,
        queued: i32,
    },
    Repair {
        target_o: i32,
        target_owner: i32,
        queued: i32,
    },
    Trade {
        first_o: i32,
        first_owner: i32,
        second_o: i32,
        second_owner: i32,
        queued: i32,
    },
}

impl EconomyGroupAction {
    pub const fn opcode(self) -> u8 {
        match self {
            Self::BoardShip { .. } => BOARD_SHIP_OPCODE,
            Self::Repair { .. } => REPAIR_OPCODE,
            Self::Trade { .. } => TRADE_OPCODE,
        }
    }

    pub const fn queued(self) -> i32 {
        match self {
            Self::BoardShip { queued, .. }
            | Self::Repair { queued, .. }
            | Self::Trade { queued, .. } => queued,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EconomyGroupWireError {
    Empty,
    WrongGroupOpcode(u8),
    UnsupportedActionOpcode(u8),
    TruncatedGroupPrefix(usize),
    TooManyGroupMembers(u8),
    InvalidGroupOwner(i8),
    WrongSize {
        opcode: u8,
        expected: usize,
        actual: usize,
    },
    ActionPrecedesGroup {
        group_command_index: u16,
        action_command_index: u16,
    },
}

/// Exact opcode-0 command. An empty `requested` vector is a cached `(o, uid)`
/// reselection, not an empty selection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupSelectionCommand {
    pub owner: u8,
    pub requested: Vec<i16>,
}

impl GroupSelectionCommand {
    pub fn is_cached_reselection(&self) -> bool {
        self.requested.is_empty()
    }
}

pub fn decode_group_selection_packet(
    packet: &[u8],
) -> Result<GroupSelectionCommand, EconomyGroupWireError> {
    let Some(&opcode) = packet.first() else {
        return Err(EconomyGroupWireError::Empty);
    };
    if opcode != GROUP_OPCODE {
        return Err(EconomyGroupWireError::WrongGroupOpcode(opcode));
    }
    if packet.len() < GROUP_WIRE_PREFIX {
        return Err(EconomyGroupWireError::TruncatedGroupPrefix(packet.len()));
    }
    let count = packet[1];
    if usize::from(count) > GROUP_MAX_MEMBERS {
        return Err(EconomyGroupWireError::TooManyGroupMembers(count));
    }
    let owner = packet[2] as i8;
    if !(0..NUM_LEADERS as i8).contains(&owner) {
        return Err(EconomyGroupWireError::InvalidGroupOwner(owner));
    }
    let expected = GROUP_WIRE_PREFIX + usize::from(count) * 2;
    if packet.len() != expected {
        return Err(EconomyGroupWireError::WrongSize {
            opcode,
            expected,
            actual: packet.len(),
        });
    }
    Ok(GroupSelectionCommand {
        owner: owner as u8,
        requested: packet[GROUP_WIRE_PREFIX..]
            .chunks_exact(2)
            .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
            .collect(),
    })
}

fn wire_i32(packet: &[u8], offset: usize) -> i32 {
    i32::from_le_bytes(
        packet[offset..offset + 4]
            .try_into()
            .expect("fixed-size economy packet field"),
    )
}

pub fn decode_economy_group_action_packet(
    packet: &[u8],
) -> Result<EconomyGroupAction, EconomyGroupWireError> {
    let Some(&opcode) = packet.first() else {
        return Err(EconomyGroupWireError::Empty);
    };
    let expected = match opcode {
        BOARD_SHIP_OPCODE => BOARD_SHIP_WIRE_SIZE,
        REPAIR_OPCODE => REPAIR_WIRE_SIZE,
        TRADE_OPCODE => TRADE_WIRE_SIZE,
        _ => return Err(EconomyGroupWireError::UnsupportedActionOpcode(opcode)),
    };
    if packet.len() != expected {
        return Err(EconomyGroupWireError::WrongSize {
            opcode,
            expected,
            actual: packet.len(),
        });
    }
    Ok(match opcode {
        BOARD_SHIP_OPCODE => EconomyGroupAction::BoardShip {
            ship_o: wire_i32(packet, 1),
            queued: wire_i32(packet, 5),
        },
        REPAIR_OPCODE => EconomyGroupAction::Repair {
            target_o: wire_i32(packet, 1),
            target_owner: wire_i32(packet, 5),
            queued: wire_i32(packet, 9),
        },
        TRADE_OPCODE => EconomyGroupAction::Trade {
            first_o: wire_i32(packet, 1),
            first_owner: wire_i32(packet, 5),
            second_o: wire_i32(packet, 9),
            second_owner: wire_i32(packet, 13),
            queued: wire_i32(packet, 17),
        },
        _ => unreachable!("opcode was matched above"),
    })
}

/// Command positions are retained because the current selection persists within one
/// package and only the nearest preceding opcode 0 addresses the action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommandPackagePosition {
    pub game_frame: i32,
    pub package_serial: u32,
    pub play: i32,
    pub group_command_index: u16,
    pub action_command_index: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EconomyGroupPacketPair {
    pub position: CommandPackagePosition,
    pub group: GroupSelectionCommand,
    pub action: EconomyGroupAction,
}

pub fn decode_economy_group_packet_pair(
    position: CommandPackagePosition,
    group_packet: &[u8],
    action_packet: &[u8],
) -> Result<EconomyGroupPacketPair, EconomyGroupWireError> {
    if position.group_command_index >= position.action_command_index {
        return Err(EconomyGroupWireError::ActionPrecedesGroup {
            group_command_index: position.group_command_index,
            action_command_index: position.action_command_index,
        });
    }
    Ok(EconomyGroupPacketPair {
        position,
        group: decode_group_selection_packet(group_packet)?,
        action: decode_economy_group_action_packet(action_packet)?,
    })
}

/// Digest of the complete canonical state relevant to a transaction boundary.
///
/// The host chooses the digest implementation, but it must cover the fixed Group slot,
/// Object registry identities, Unit/order/path columns, and every reached Build/World field.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CanonicalStateDigest(pub [u8; 32]);

/// Retail's cached opcode-0 identity omits the canonical generation but retains the UID.
/// The allocation host resolves it against the current Object registry before selecting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CachedSelectionIdentity {
    pub o: i16,
    pub uid: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalSelectionCacheImage {
    pub revision: u64,
    pub entries: Vec<CachedSelectionIdentity>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CanonicalGroupMutation {
    pub locator: CanonicalGroupLocator,
    pub before: GroupData,
    pub after: GroupData,
}

/// The UnitData.group backlink is part of the same `Groups::push_group` transaction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CanonicalMemberGroupBacklink {
    pub identity: CanonicalObjectIdentity,
    pub before: i16,
    pub after: i16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CanonicalGroupSelectionResult {
    /// All explicit or cached candidates were stale/inadmissible; the action is dropped.
    Dropped,
    Selected {
        locator: CanonicalGroupLocator,
        retained_group_id: i32,
    },
}

/// Typed evidence for the canonical opcode-0 selection/allocator stage.
///
/// `group_mutations` contains the destination slot even when retail reused it unchanged,
/// plus every old group from which a selected member was removed. `state_before/after`
/// bind the remaining Object/cache fields; they do not replace these typed after-images.
#[derive(Clone, Debug, PartialEq)]
pub struct CanonicalGroupSelectionReceipt {
    pub pair: EconomyGroupPacketPair,
    pub cache_before: CanonicalSelectionCacheImage,
    pub cache_after: CanonicalSelectionCacheImage,
    pub selected: Vec<CanonicalObjectIdentity>,
    pub group_mutations: Vec<CanonicalGroupMutation>,
    pub member_backlinks: Vec<CanonicalMemberGroupBacklink>,
    pub last_group_before: [i32; NUM_LEADERS],
    pub last_group_after: [i32; NUM_LEADERS],
    pub state_before: CanonicalStateDigest,
    pub state_after: CanonicalStateDigest,
    pub result: CanonicalGroupSelectionResult,
}

fn group_live_members(group: &GroupData) -> Option<&[i16]> {
    let count = usize::try_from(group.num).ok()?;
    (count <= GROUP_MAX_MEMBERS).then_some(&group.list[..count])
}

fn cached_selection_is_ordered_subsequence(
    owner: u8,
    cache: &[CachedSelectionIdentity],
    selected: &[CanonicalObjectIdentity],
) -> bool {
    let mut next = 0;
    for identity in selected {
        if identity.owner != owner {
            return false;
        }
        let Some(relative) = cache[next..]
            .iter()
            .position(|entry| entry.o == identity.o && entry.uid == identity.uid)
        else {
            return false;
        };
        next += relative + 1;
    }
    true
}

impl CanonicalGroupSelectionReceipt {
    /// Validate the selection against a staged fixed-`Groups` after-image. The caller must
    /// not publish that image until the paired action has also passed preflight.
    pub fn validates_staged_groups(&self, staged: &Groups) -> bool {
        let owner = self.pair.group.owner;
        if usize::from(owner) >= NUM_LEADERS
            || self.pair.position.group_command_index >= self.pair.position.action_command_index
            || self.pair.group.requested.len() > GROUP_MAX_MEMBERS
            || self.selected.len() > GROUP_MAX_MEMBERS
            || self.group_mutations.iter().any(|mutation| {
                mutation.locator.index().is_none() || mutation.before.id != mutation.after.id
            })
        {
            return false;
        }
        let mut mutation_indices = Vec::with_capacity(self.group_mutations.len());
        for mutation in &self.group_mutations {
            let index = mutation.locator.index().unwrap();
            if mutation_indices.contains(&index) || staged.list.get(index) != Some(&mutation.after)
            {
                return false;
            }
            mutation_indices.push(index);
        }
        if staged.last_group != self.last_group_after {
            return false;
        }
        for who in 0..NUM_LEADERS {
            if who != usize::from(owner)
                && self.last_group_before[who] != self.last_group_after[who]
            {
                return false;
            }
        }

        if self.pair.group.is_cached_reselection() {
            if self.cache_after.revision != self.cache_before.revision.wrapping_add(1)
                || self.cache_before.entries != self.cache_after.entries
                || !cached_selection_is_ordered_subsequence(
                    owner,
                    &self.cache_before.entries,
                    &self.selected,
                )
            {
                return false;
            }
        } else {
            // Retail retains every live explicit `(o,uid)` in the play-keyed cache, including
            // duplicates. Canonical Group construction de-duplicates the effective selection,
            // so that selection must be an ordered subsequence rather than equal to the cache.
            if self.cache_after.revision != self.cache_before.revision.wrapping_add(1)
                || !cached_selection_is_ordered_subsequence(
                    owner,
                    &self.cache_after.entries,
                    &self.selected,
                )
            {
                return false;
            }
        }

        match self.result {
            CanonicalGroupSelectionResult::Dropped => {
                self.selected.is_empty()
                    && self.group_mutations.is_empty()
                    && self.member_backlinks.is_empty()
                    && self.last_group_before == self.last_group_after
            }
            CanonicalGroupSelectionResult::Selected {
                locator,
                retained_group_id,
            } => {
                let Some(index) = locator.index() else {
                    return false;
                };
                if locator.owner != owner
                    || self.selected.is_empty()
                    || self.last_group_after[usize::from(owner)] != index as i32
                {
                    return false;
                }
                let Some(target) = self
                    .group_mutations
                    .iter()
                    .find(|mutation| mutation.locator == locator)
                else {
                    return false;
                };
                let Some(members) = group_live_members(&target.after) else {
                    return false;
                };
                if target.before.id != retained_group_id
                    || target.after.id != retained_group_id
                    || target.after.who != owner
                    || members.len() != self.selected.len()
                    || members
                        .iter()
                        .zip(&self.selected)
                        .any(|(&o, identity)| identity.owner != owner || identity.o != o)
                    || self.member_backlinks.len() != self.selected.len()
                {
                    return false;
                }
                for (backlink, identity) in self.member_backlinks.iter().zip(&self.selected) {
                    if backlink.identity != *identity || backlink.after != index as i16 {
                        return false;
                    }
                    if backlink.before >= 0 && backlink.before != backlink.after {
                        let Some(old_locator) =
                            CanonicalGroupLocator::from_index(backlink.before as usize)
                        else {
                            return false;
                        };
                        let Some(old) = self
                            .group_mutations
                            .iter()
                            .find(|mutation| mutation.locator == old_locator)
                        else {
                            return false;
                        };
                        let (Some(before), Some(after)) = (
                            group_live_members(&old.before),
                            group_live_members(&old.after),
                        ) else {
                            return false;
                        };
                        if !before.contains(&identity.o) || after.contains(&identity.o) {
                            return false;
                        }
                    }
                }
                true
            }
        }
    }
}

/// Request captured by the canonical owner, never by the command bridge's shadow pool.
#[derive(Clone, Debug, PartialEq)]
pub struct EconomyGroupTransactionRequest {
    locator: CanonicalGroupLocator,
    group_id: i32,
    group_before: GroupData,
    ignore_orders: bool,
    action: EconomyGroupAction,
    state_before: CanonicalStateDigest,
    selection: Option<CanonicalGroupSelectionReceipt>,
}

impl EconomyGroupTransactionRequest {
    /// Snapshot one exact slot from the save/checksum-owned fixed pool.
    ///
    /// Keeping this constructor typed to [`Groups`] is load-bearing: a `command::Groups`
    /// after-image cannot be passed here accidentally even though it also stores
    /// `GroupData` values. This lower-level constructor is for isolated action tests; a
    /// real packet route must use [`Self::snapshot_from_selection`].
    pub fn snapshot(
        groups: &Groups,
        locator: CanonicalGroupLocator,
        ignore_orders: bool,
        action: EconomyGroupAction,
        state_before: CanonicalStateDigest,
    ) -> Option<Self> {
        locator.index()?;
        let group = groups.get(usize::from(locator.owner), usize::from(locator.slot));
        if group.who != locator.owner {
            return None;
        }
        Some(Self {
            locator,
            group_id: group.id,
            group_before: group.clone(),
            ignore_orders,
            action,
            state_before,
            selection: None,
        })
    }

    /// Bind a same-package opcode-0/action pair to the staged canonical allocation image.
    ///
    /// `staged_groups` is the proposed selection after-image. The future Sim host preflights
    /// the action against it, then publishes selection and action together; a failed action
    /// must not leave only the selection committed.
    pub fn snapshot_from_selection(
        staged_groups: &Groups,
        selection: CanonicalGroupSelectionReceipt,
        ignore_orders: bool,
        state_after_selection: CanonicalStateDigest,
    ) -> Option<Self> {
        if selection.state_after != state_after_selection
            || !selection.validates_staged_groups(staged_groups)
        {
            return None;
        }
        let CanonicalGroupSelectionResult::Selected { locator, .. } = selection.result else {
            return None;
        };
        let mut request = Self::snapshot(
            staged_groups,
            locator,
            ignore_orders,
            selection.pair.action,
            state_after_selection,
        )?;
        request.selection = Some(selection);
        Some(request)
    }

    pub const fn locator(&self) -> CanonicalGroupLocator {
        self.locator
    }

    pub const fn group_id(&self) -> i32 {
        self.group_id
    }

    pub fn group_before(&self) -> &GroupData {
        &self.group_before
    }

    pub const fn action(&self) -> EconomyGroupAction {
        self.action
    }

    pub const fn state_before(&self) -> CanonicalStateDigest {
        self.state_before
    }

    pub fn selection_receipt(&self) -> Option<&CanonicalGroupSelectionReceipt> {
        self.selection.as_ref()
    }

    /// Revalidate the request immediately before the first mutation.
    pub fn still_current(&self, groups: &Groups, state: CanonicalStateDigest) -> bool {
        let Some(index) = self.locator.index() else {
            return false;
        };
        groups.list.get(index).is_some_and(|group| {
            group.id == self.group_id && group == &self.group_before && state == self.state_before
        })
    }
}

/// One member's stable registry identity plus the facts read by the retail action body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CanonicalMemberFacts {
    pub identity: CanonicalObjectIdentity,
    pub retail: MemberFacts,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EconomyGroupActionFacts {
    BoardShip {
        /// `None` is the exact command-handler object-validation refusal.
        ship: Option<CanonicalObjectIdentity>,
        members: Vec<CanonicalMemberFacts>,
    },
    Repair {
        target: Option<CanonicalObjectIdentity>,
        members: Vec<CanonicalMemberFacts>,
    },
    Trade {
        first: Option<CanonicalObjectIdentity>,
        second: Option<CanonicalObjectIdentity>,
        target: TradeTargetFacts,
        members: Vec<CanonicalMemberFacts>,
    },
}

/// Capabilities which must exist before a plan may leave the preflight boundary.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EconomyOrderCapabilities {
    pub format_version: u32,
    pub typed_cast_v1: bool,
    pub typed_trade_v1: bool,
    pub exact_group_queue_first: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EconomyGroupPreflightError {
    ActionFactsMismatch,
    GroupShape,
    TargetIdentity,
    MemberIdentity,
    UnknownRetailFact,
    ScenarioIgnoreOrdersPrelude,
    OrderEnvelopeV12,
    TypedCastPayload,
    TypedTradePayload,
    ExactGroupQueueFirst,
}

/// Complete recomputable plan through the canonical atomic-commit boundary.
#[derive(Clone, Debug, PartialEq)]
pub struct CanonicalEconomyGroupPlan {
    pub request: EconomyGroupTransactionRequest,
    /// False means the retail command handler rejected a stale/missing addressed object.
    pub action_reached: bool,
    /// Exact fixed-pool Group after-image. Handler rejection preserves the before-image.
    pub group_after: GroupData,
    /// Stable registry identities in target-then-member order. They are part of the
    /// recomputed plan so UID/generation tampering cannot validate against an old receipt.
    pub identities: Vec<CanonicalObjectIdentity>,
    /// `None` is also used by `action_trade`'s exact entry-gate no-op.
    pub action_plan: Option<EconomyPlan>,
}

fn members_exact(
    request: &EconomyGroupTransactionRequest,
    members: &[CanonicalMemberFacts],
) -> Result<Vec<MemberFacts>, EconomyGroupPreflightError> {
    let n = usize::try_from(request.group_before.num)
        .ok()
        .filter(|&n| n <= GROUP_MAX_MEMBERS)
        .ok_or(EconomyGroupPreflightError::GroupShape)?;
    if members.len() != n {
        return Err(EconomyGroupPreflightError::MemberIdentity);
    }
    let mut retail = Vec::with_capacity(n);
    for (facts, &expected_o) in members.iter().zip(&request.group_before.list[..n]) {
        if facts.identity.owner != request.locator.owner
            || facts.identity.o != expected_o
            || facts.retail.o != expected_o
        {
            return Err(EconomyGroupPreflightError::MemberIdentity);
        }
        retail.push(facts.retail);
    }
    Ok(retail)
}

/// `plan_trade` returns `None` for entry-gate refusals. Check that every gate retail reached
/// was nevertheless known; an unknown value must not become a falsely complete no-op.
fn trade_entry_facts_exact(target: TradeTargetFacts) -> bool {
    let known = |fact: Fact<bool>| match fact {
        Fact::Known(value) => Some(value),
        Fact::Unknown(_) => None,
    };
    let Some(live) = known(target.live_building) else {
        return false;
    };
    if !live {
        return true;
    }
    let Some(active) = known(target.active) else {
        return false;
    };
    if !active {
        return true;
    }
    let Some(is_trade_build) = known(target.build_is_trade) else {
        return false;
    };
    if !is_trade_build {
        return true;
    }
    known(target.group_has_trader).is_some()
}

fn require_capabilities(
    request: &EconomyGroupTransactionRequest,
    plan: &Option<EconomyPlan>,
    capabilities: EconomyOrderCapabilities,
) -> Result<(), EconomyGroupPreflightError> {
    if capabilities.format_version < REQUIRED_ORDER_FORMAT_VERSION {
        return Err(EconomyGroupPreflightError::OrderEnvelopeV12);
    }
    if request.action.queued() == QUEUE_FIRST && !capabilities.exact_group_queue_first {
        return Err(EconomyGroupPreflightError::ExactGroupQueueFirst);
    }
    let steps = plan.as_ref().map_or(&[][..], |plan| plan.steps.as_slice());
    if steps
        .iter()
        .any(|step| matches!(step, EconomyStep::AddCastOrder { .. }))
        && !capabilities.typed_cast_v1
    {
        return Err(EconomyGroupPreflightError::TypedCastPayload);
    }
    if steps
        .iter()
        .any(|step| matches!(step, EconomyStep::AddTradeOrder { .. }))
        && !capabilities.typed_trade_v1
    {
        return Err(EconomyGroupPreflightError::TypedTradePayload);
    }
    Ok(())
}

/// Recompute the retail action plan without mutating any owner.
pub fn preflight_economy_group_transaction(
    request: &EconomyGroupTransactionRequest,
    facts: &EconomyGroupActionFacts,
    capabilities: EconomyOrderCapabilities,
) -> Result<CanonicalEconomyGroupPlan, EconomyGroupPreflightError> {
    if request.group_before.who != request.locator.owner
        || request.group_before.id != request.group_id
    {
        return Err(EconomyGroupPreflightError::GroupShape);
    }

    // These handlers validate their addressed object(s) before entering Group::action_*.
    // Handler refusal is a complete no-op and needs no order-payload capability.
    let (target_reached, target_identities, members_empty_on_rejection) =
        match (request.action, facts) {
            (
                EconomyGroupAction::BoardShip { ship_o, .. },
                EconomyGroupActionFacts::BoardShip { ship, members },
            ) => match ship {
                Some(identity) if identity.addresses(i32::from(request.locator.owner), ship_o) => {
                    (true, vec![*identity], true)
                }
                Some(_) => return Err(EconomyGroupPreflightError::TargetIdentity),
                None => (false, Vec::new(), members.is_empty()),
            },
            (
                EconomyGroupAction::Repair {
                    target_o,
                    target_owner,
                    ..
                },
                EconomyGroupActionFacts::Repair { target, members },
            ) => match target {
                Some(identity) if identity.addresses(target_owner, target_o) => {
                    (true, vec![*identity], true)
                }
                Some(_) => return Err(EconomyGroupPreflightError::TargetIdentity),
                None => (false, Vec::new(), members.is_empty()),
            },
            (
                EconomyGroupAction::Trade {
                    first_o,
                    first_owner,
                    second_o,
                    second_owner,
                    ..
                },
                EconomyGroupActionFacts::Trade {
                    first,
                    second,
                    members,
                    ..
                },
            ) => match (first, second) {
                (Some(first), Some(second))
                    if first.addresses(first_owner, first_o)
                        && second.addresses(second_owner, second_o) =>
                {
                    (true, vec![*first, *second], true)
                }
                (Some(_), Some(_)) => return Err(EconomyGroupPreflightError::TargetIdentity),
                _ => (false, Vec::new(), members.is_empty()),
            },
            _ => return Err(EconomyGroupPreflightError::ActionFactsMismatch),
        };
    if !target_reached {
        if !members_empty_on_rejection {
            return Err(EconomyGroupPreflightError::ActionFactsMismatch);
        }
        return Ok(CanonicalEconomyGroupPlan {
            request: request.clone(),
            action_reached: false,
            group_after: request.group_before.clone(),
            identities: target_identities,
            action_plan: None,
        });
    }
    if request.ignore_orders {
        return Err(EconomyGroupPreflightError::ScenarioIgnoreOrdersPrelude);
    }

    // `Group::action_begin` is common and precedes the action-specific writes.
    let mut group_after_begin = request.group_before.clone();
    group_after_begin.disband = 0;
    let action_plan = match (request.action, facts) {
        (
            EconomyGroupAction::BoardShip { ship_o, queued },
            EconomyGroupActionFacts::BoardShip { members, .. },
        ) => {
            let members = members_exact(request, members)?;
            Some(
                plan_board_ship(&group_after_begin, ship_o, queued, &members)
                    .map_err(|_| EconomyGroupPreflightError::MemberIdentity)?,
            )
        }
        (
            EconomyGroupAction::Repair {
                target_o,
                target_owner,
                queued,
            },
            EconomyGroupActionFacts::Repair { members, .. },
        ) => {
            let members = members_exact(request, members)?;
            Some(
                plan_repair(&group_after_begin, target_o, target_owner, queued, &members)
                    .map_err(|_| EconomyGroupPreflightError::MemberIdentity)?,
            )
        }
        (
            EconomyGroupAction::Trade {
                first_o,
                first_owner,
                second_o,
                second_owner,
                queued,
            },
            EconomyGroupActionFacts::Trade {
                target, members, ..
            },
        ) => {
            if !trade_entry_facts_exact(*target) {
                return Err(EconomyGroupPreflightError::UnknownRetailFact);
            }
            let members = members_exact(request, members)?;
            plan_trade(
                &group_after_begin,
                first_o,
                first_owner,
                second_o,
                second_owner,
                queued,
                *target,
                &members,
            )
            .map_err(|_| EconomyGroupPreflightError::MemberIdentity)?
        }
        _ => return Err(EconomyGroupPreflightError::ActionFactsMismatch),
    };
    if action_plan.as_ref().is_some_and(|plan| !plan.is_exact()) {
        return Err(EconomyGroupPreflightError::UnknownRetailFact);
    }
    require_capabilities(request, &action_plan, capabilities)?;

    let member_identities = match facts {
        EconomyGroupActionFacts::BoardShip { members, .. }
        | EconomyGroupActionFacts::Repair { members, .. }
        | EconomyGroupActionFacts::Trade { members, .. } => {
            members.iter().map(|member| member.identity)
        }
    };

    let mut group_after = group_after_begin;
    if let Some(plan) = &action_plan {
        for step in &plan.steps {
            if let EconomyStep::SetGroupForm(form) = *step {
                group_after.form = form;
            }
        }
    }
    Ok(CanonicalEconomyGroupPlan {
        request: request.clone(),
        action_reached: true,
        group_after,
        identities: target_identities
            .into_iter()
            .chain(member_identities)
            .collect(),
        action_plan,
    })
}

/// Evidence that one non-commutative commit survived save/reload and one subsequent tick.
#[derive(Clone, Debug, PartialEq)]
pub struct EconomyAtomicCommitEvidence {
    pub state_before: CanonicalStateDigest,
    pub state_after: CanonicalStateDigest,
    pub group_after: GroupData,
    pub reloaded_state: CanonicalStateDigest,
    pub reloaded_group: GroupData,
    pub direct_next_tick: CanonicalStateDigest,
    pub reloaded_next_tick: CanonicalStateDigest,
}

impl EconomyAtomicCommitEvidence {
    fn validates(
        &self,
        request: &EconomyGroupTransactionRequest,
        plan: &CanonicalEconomyGroupPlan,
    ) -> bool {
        self.state_before == request.state_before
            && self.group_after == plan.group_after
            && self.reloaded_state == self.state_after
            && self.reloaded_group == self.group_after
            && self.direct_next_tick == self.reloaded_next_tick
            && (plan.action_reached || self.state_after == self.state_before)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EconomyGroupTransactionStatus {
    Applied,
    Unavailable,
}

/// Typed result returned by the future shared canonical Group callback.
#[derive(Clone, Debug, PartialEq)]
pub struct EconomyGroupTransactionReceipt {
    pub request: EconomyGroupTransactionRequest,
    pub facts: EconomyGroupActionFacts,
    pub capabilities: EconomyOrderCapabilities,
    pub status: EconomyGroupTransactionStatus,
    pub plan: Option<CanonicalEconomyGroupPlan>,
    pub commit: Option<EconomyAtomicCommitEvidence>,
}

impl EconomyGroupTransactionReceipt {
    pub fn validates(&self, expected: &EconomyGroupTransactionRequest) -> bool {
        if &self.request != expected {
            return false;
        }
        let recomputed =
            preflight_economy_group_transaction(expected, &self.facts, self.capabilities);
        match self.status {
            EconomyGroupTransactionStatus::Unavailable => {
                self.plan.is_none() && self.commit.is_none()
            }
            EconomyGroupTransactionStatus::Applied => {
                let (Ok(recomputed), Some(plan), Some(commit)) =
                    (recomputed, self.plan.as_ref(), self.commit.as_ref())
                else {
                    return false;
                };
                plan == &recomputed && commit.validates(expected, plan)
            }
        }
    }
}
