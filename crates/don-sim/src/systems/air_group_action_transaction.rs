// SPDX-License-Identifier: GPL-3.0-or-later
//! Stateless transaction contract for the containment-launch command cohort.
//!
//! This module joins the already recovered whole-body planners for
//! `Group::action_launch_patrol` and `Group::action_scramble` to the shape a canonical
//! [`crate::tick::Sim`] host must expose.  It deliberately owns no group, object, order,
//! type, scenario, or RNG state.  A caller supplies an immutable preflight image; an
//! [`AtomicAirGroupActionHost`] revalidates that image and commits the prepared installs.
//! The wrapper checkpoints the host and restores it on any stale or partial write.
//!
//! The current command bridge owns a second `Groups`/`ObjectTable` image.  That image must
//! not implement this contract as the production owner.  The intended adapter is the one
//! `Sim` transaction that can validate `groups_guys::Groups`, `World` object identities and
//! order queues together.  Until that adapter, dynamic AIR_PATROL payload persistence and
//! AIR_PATROL `unit_work` dispatch exist, the capability fields below make the command fail
//! closed.  Consequently this module is architecture and executable contract evidence, not
//! a closure promotion.

use std::collections::BTreeSet;

use crate::command::air_launch_receivers::{
    plan_launch_patrol, plan_scramble, AirLaunchBoundary, AirLaunchFacts, AirLaunchInstall,
    LaunchPatrolPlan, LaunchPatrolRequest, ScramblePlan, LAUNCH_PATROL_WIRE_SIZE,
    SCRAMBLE_WIRE_SIZE,
};
use crate::order::OrderIndex;

pub const LAUNCH_PATROL_OPCODE: u8 = 11;
pub const SCRAMBLE_OPCODE: u8 = 36;
pub const GROUP_OPCODE: u8 = 0;
pub const GROUP_WIRE_PREFIX: usize = 3;
pub const GROUPS_PER_OWNER: u8 = 64;
pub const GROUP_OWNER_SLOTS: u8 = 8;
pub const OBJECT_OWNER_SLOTS: u8 = 10;
pub const MAX_GROUP_MEMBERS: usize = 128;

/// Exact wire decoder failures.  Fixed-size packets reject both truncation and trailing
/// bytes so a receipt is bound to the packet the lockstep stream actually carried.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AirGroupWireError {
    Empty,
    UnsupportedOpcode(u8),
    WrongSize {
        opcode: u8,
        expected: usize,
        actual: usize,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupSelectionWireError {
    Empty,
    WrongOpcode(u8),
    TruncatedPrefix(usize),
    TooManyMembers(u8),
    InvalidOwner(i8),
    WrongSize { expected: usize, actual: usize },
}

/// Exact opcode-0 payload.  An empty `requested` list is not an empty selection: retail
/// re-selects the package player's cached `(o, uid)` pairs.  The canonical package host
/// must therefore attach a cache revision before the action can proceed.
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

/// Decode one complete variable-length `GroupCommand` (`3 + 2*num`).
pub fn decode_group_selection_packet(
    packet: &[u8],
) -> Result<GroupSelectionCommand, GroupSelectionWireError> {
    let Some(&opcode) = packet.first() else {
        return Err(GroupSelectionWireError::Empty);
    };
    if opcode != GROUP_OPCODE {
        return Err(GroupSelectionWireError::WrongOpcode(opcode));
    }
    if packet.len() < GROUP_WIRE_PREFIX {
        return Err(GroupSelectionWireError::TruncatedPrefix(packet.len()));
    }
    let count = packet[1];
    if usize::from(count) > MAX_GROUP_MEMBERS {
        return Err(GroupSelectionWireError::TooManyMembers(count));
    }
    let owner = packet[2] as i8;
    if !(0..GROUP_OWNER_SLOTS as i8).contains(&owner) {
        return Err(GroupSelectionWireError::InvalidOwner(owner));
    }
    let expected = GROUP_WIRE_PREFIX + usize::from(count) * 2;
    if packet.len() != expected {
        return Err(GroupSelectionWireError::WrongSize {
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

/// Position of the paired commands in one canonical `CommandPackage` execution.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommandPackagePosition {
    pub game_frame: i32,
    pub package_serial: u32,
    pub play: i32,
    pub group_command_index: u16,
    pub action_command_index: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AirGroupPairError {
    Group(GroupSelectionWireError),
    Action(AirGroupWireError),
    ActionPrecedesGroup {
        group_command_index: u16,
        action_command_index: u16,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AirGroupPacketPair {
    pub position: CommandPackagePosition,
    pub group: GroupSelectionCommand,
    pub action: AirGroupCommand,
}

/// Decode the nearest preceding Group packet and its air action as one package-bound pair.
pub fn decode_air_group_packet_pair(
    position: CommandPackagePosition,
    group_packet: &[u8],
    action_packet: &[u8],
) -> Result<AirGroupPacketPair, AirGroupPairError> {
    if position.group_command_index >= position.action_command_index {
        return Err(AirGroupPairError::ActionPrecedesGroup {
            group_command_index: position.group_command_index,
            action_command_index: position.action_command_index,
        });
    }
    Ok(AirGroupPacketPair {
        position,
        group: decode_group_selection_packet(group_packet).map_err(AirGroupPairError::Group)?,
        action: decode_air_group_packet(action_packet).map_err(AirGroupPairError::Action)?,
    })
}

fn packet_digest(packet: &[u8]) -> u64 {
    packet.iter().fold(0xcbf2_9ce4_8422_2325, |h, byte| {
        (h ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// The two commands that share the containment-launch host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AirGroupCommand {
    Scramble,
    LaunchPatrol(LaunchPatrolRequest),
}

impl AirGroupCommand {
    pub const fn opcode(self) -> u8 {
        match self {
            Self::LaunchPatrol(_) => LAUNCH_PATROL_OPCODE,
            Self::Scramble => SCRAMBLE_OPCODE,
        }
    }
}

fn wire_i32(packet: &[u8], offset: usize) -> i32 {
    i32::from_le_bytes(
        packet[offset..offset + 4]
            .try_into()
            .expect("fixed packet field"),
    )
}

/// Decode opcode 11's six unaligned dwords or opcode 36's one-byte packet.
pub fn decode_air_group_packet(packet: &[u8]) -> Result<AirGroupCommand, AirGroupWireError> {
    let Some(&opcode) = packet.first() else {
        return Err(AirGroupWireError::Empty);
    };
    let expected = match opcode {
        LAUNCH_PATROL_OPCODE => LAUNCH_PATROL_WIRE_SIZE,
        SCRAMBLE_OPCODE => SCRAMBLE_WIRE_SIZE,
        _ => return Err(AirGroupWireError::UnsupportedOpcode(opcode)),
    };
    if packet.len() != expected {
        return Err(AirGroupWireError::WrongSize {
            opcode,
            expected,
            actual: packet.len(),
        });
    }
    Ok(match opcode {
        SCRAMBLE_OPCODE => AirGroupCommand::Scramble,
        LAUNCH_PATROL_OPCODE => AirGroupCommand::LaunchPatrol(LaunchPatrolRequest {
            to_x: wire_i32(packet, 1),
            to_y: wire_i32(packet, 5),
            queue: wire_i32(packet, 9),
            force_all: wire_i32(packet, 13),
            bombers_only: wire_i32(packet, 17),
            fighters_only: wire_i32(packet, 21),
        }),
        _ => unreachable!("opcode was matched above"),
    })
}

/// Retail's three disjoint object-index bands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CanonicalObjectBand {
    Unit,
    Build,
    Wall,
}

impl CanonicalObjectBand {
    pub const fn contains(self, o: i32) -> bool {
        match self {
            Self::Unit => o >= 0 && o < 2_000,
            Self::Build => o >= 2_000 && o < 3_000,
            Self::Wall => o >= 3_000 && o <= i16::MAX as i32,
        }
    }
}

/// The row-independent identity behind one retail `(owner, band, o)` address.
///
/// This mirrors `WorldObjectIdentity` instead of accepting a bare `(who, o)`: slot reuse
/// between preflight and commit must be detected even when the address is unchanged.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CanonicalObjectGeneration {
    Unit { id: u32, generation: u32 },
    BuildRow(u32),
    WallRow(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct CanonicalObjectIdentity {
    pub owner: u8,
    pub band: CanonicalObjectBand,
    pub o: i32,
    pub generation: CanonicalObjectGeneration,
}

impl CanonicalObjectIdentity {
    pub const fn address(self) -> (u8, i16) {
        (self.owner, self.o as i16)
    }

    pub const fn is_well_formed(self) -> bool {
        if self.owner >= OBJECT_OWNER_SLOTS || !self.band.contains(self.o) {
            return false;
        }
        matches!(
            (self.band, self.generation),
            (
                CanonicalObjectBand::Unit,
                CanonicalObjectGeneration::Unit { .. }
            ) | (
                CanonicalObjectBand::Build,
                CanonicalObjectGeneration::BuildRow(_)
            ) | (
                CanonicalObjectBand::Wall,
                CanonicalObjectGeneration::WallRow(_)
            )
        )
    }

    pub const fn is_unit(self) -> bool {
        matches!(self.generation, CanonicalObjectGeneration::Unit { .. })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CanonicalGroupKey {
    pub owner: u8,
    pub slot: u8,
    pub group_id: i32,
}

/// Full checksum-walked receiver image plus the stable identity of every live member.
///
/// `walk_image` is exactly `GroupData::walk`: the 72-byte scalar header, then the live
/// `list`, `off_x`, `off_y`, `curr_x`, `curr_y`, and `angles` prefixes.  Carrying the bytes
/// (rather than only a hash) lets revalidation distinguish a member reorder or formation
/// mutation without trusting a collision-prone summary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupBeforeImage {
    pub key: CanonicalGroupKey,
    pub revision: u64,
    pub walk_image: Vec<u8>,
    pub members: Vec<CanonicalObjectIdentity>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupImageError {
    OwnerOutOfRange(u8),
    SlotOutOfRange(u8),
    WalkTooShort(usize),
    InvalidMemberCount(i32),
    WrongWalkSize {
        expected: usize,
        actual: usize,
    },
    GroupIdMismatch {
        key: i32,
        walked: i32,
    },
    OwnerMismatch {
        key: u8,
        walked: u8,
    },
    MemberCountMismatch {
        walked: usize,
        identities: usize,
    },
    InvalidIdentity {
        index: usize,
    },
    MemberOwnerMismatch {
        index: usize,
        expected: u8,
        actual: u8,
    },
    MemberAddressMismatch {
        index: usize,
        walked: i16,
        identity: i32,
    },
    DuplicateMember {
        index: usize,
    },
}

fn image_i32(bytes: &[u8], offset: usize) -> i32 {
    i32::from_le_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("validated group header"),
    )
}

impl GroupBeforeImage {
    /// Validate the length prefixes and bind the walked member addresses to their stable
    /// identities.  Stale bytes beyond `num` are intentionally absent, as in retail.
    pub fn validate(&self) -> Result<(), GroupImageError> {
        if self.key.owner >= GROUP_OWNER_SLOTS {
            return Err(GroupImageError::OwnerOutOfRange(self.key.owner));
        }
        if self.key.slot >= GROUPS_PER_OWNER {
            return Err(GroupImageError::SlotOutOfRange(self.key.slot));
        }
        if self.walk_image.len() < 72 {
            return Err(GroupImageError::WalkTooShort(self.walk_image.len()));
        }
        let walked_id = image_i32(&self.walk_image, 0);
        if walked_id != self.key.group_id {
            return Err(GroupImageError::GroupIdMismatch {
                key: self.key.group_id,
                walked: walked_id,
            });
        }
        let walked_owner = self.walk_image[70];
        if walked_owner != self.key.owner {
            return Err(GroupImageError::OwnerMismatch {
                key: self.key.owner,
                walked: walked_owner,
            });
        }
        let count = image_i32(&self.walk_image, 8);
        if !(0..=MAX_GROUP_MEMBERS as i32).contains(&count) {
            return Err(GroupImageError::InvalidMemberCount(count));
        }
        let count = count as usize;
        let expected = 72 + count * (2 + 4 * 4 + 1);
        if self.walk_image.len() != expected {
            return Err(GroupImageError::WrongWalkSize {
                expected,
                actual: self.walk_image.len(),
            });
        }
        if self.members.len() != count {
            return Err(GroupImageError::MemberCountMismatch {
                walked: count,
                identities: self.members.len(),
            });
        }
        let mut seen = BTreeSet::new();
        for (index, identity) in self.members.iter().copied().enumerate() {
            if !identity.is_well_formed() {
                return Err(GroupImageError::InvalidIdentity { index });
            }
            if identity.owner != self.key.owner {
                return Err(GroupImageError::MemberOwnerMismatch {
                    index,
                    expected: self.key.owner,
                    actual: identity.owner,
                });
            }
            let offset = 72 + index * 2;
            let walked = i16::from_le_bytes(
                self.walk_image[offset..offset + 2]
                    .try_into()
                    .expect("validated member prefix"),
            );
            if i32::from(walked) != identity.o {
                return Err(GroupImageError::MemberAddressMismatch {
                    index,
                    walked,
                    identity: identity.o,
                });
            }
            if !seen.insert((identity.owner, identity.o)) {
                return Err(GroupImageError::DuplicateMember { index });
            }
        }
        Ok(())
    }

    /// A compact diagnostic only.  Revalidation must compare `walk_image`, not this hash.
    pub fn diagnostic_digest(&self) -> u64 {
        self.walk_image
            .iter()
            .fold(0xcbf2_9ce4_8422_2325, |h, byte| {
                (h ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
            })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuthorityRevisions {
    pub scenario: u64,
    pub types: u64,
    pub world_orders: u64,
}

/// Packet plus the canonical before-image the packet was addressed to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AirGroupActionRequest {
    pub position: CommandPackagePosition,
    pub group_packet: Vec<u8>,
    pub selection: GroupSelectionCommand,
    pub packet: Vec<u8>,
    pub command: AirGroupCommand,
    pub group_before: GroupBeforeImage,
    pub authority: AuthorityRevisions,
}

impl AirGroupActionRequest {
    pub fn from_paired_packets(
        position: CommandPackagePosition,
        group_packet: Vec<u8>,
        packet: Vec<u8>,
        group_before: GroupBeforeImage,
        authority: AuthorityRevisions,
    ) -> Result<Self, AirGroupPairError> {
        let pair = decode_air_group_packet_pair(position, &group_packet, &packet)?;
        Ok(Self {
            position,
            group_packet,
            selection: pair.group,
            packet,
            command: pair.action,
            group_before,
            authority,
        })
    }
}

/// The scenario scalar is an authority, not a default.  `Armed` remains a blocker until
/// the canonical host can commit `plan_ignore_order_kills` in this same transaction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IgnoreOrdersSnapshot {
    Unavailable,
    Clear { revision: u64 },
    Armed { revision: u64 },
}

/// Evidence that the values copied into `AirLaunchFacts` came from synchronized type data.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AirTypeAuthoritySnapshot {
    pub revision: u64,
    pub domain_column: bool,
    pub object_masks_column: bool,
    pub unit_flags_column: bool,
    /// Authoritative non-strict `ObjectData::is(BIPLANE/BOMBER/HELICOPTER, 0)` results.
    pub nonstrict_type_relations: bool,
}

/// Production capabilities intentionally absent from the present shadow bridge.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AirIntegrationCapabilities {
    pub packet_to_sim_route: bool,
    pub move_order_tag_v1: bool,
    pub air_patrol_order_tag_v1: bool,
    pub dynamic_patrol_arrays: bool,
    pub move_unit_work: bool,
    pub air_patrol_unit_work: bool,
    pub move_save_reload_resume: bool,
    pub air_patrol_save_reload_resume: bool,
}

/// Exact mutable columns captured for one install target before any target is changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AirOrderTargetBefore {
    pub identity: CanonicalObjectIdentity,
    pub order_revision: u64,
    pub order_digest: u64,
    pub unit_mask: u32,
    pub action_revision: u64,
    pub path_revision: u64,
}

/// Canonical package-host proof that opcode 0 resolved to the exact receiver image used by
/// the action.  In particular, an empty Group packet needs `selection_cache_revision`;
/// otherwise the packet contains no member identities at all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalGroupPacketReceipt {
    pub position: CommandPackagePosition,
    pub group_packet_digest: u64,
    pub selection: GroupSelectionCommand,
    pub resolved_group: CanonicalGroupKey,
    pub group_revision: u64,
    pub group_walk_digest: u64,
    pub selection_cache_revision: Option<u64>,
}

impl CanonicalGroupPacketReceipt {
    pub fn for_request(
        request: &AirGroupActionRequest,
        selection_cache_revision: Option<u64>,
    ) -> Self {
        Self {
            position: request.position,
            group_packet_digest: packet_digest(&request.group_packet),
            selection: request.selection.clone(),
            resolved_group: request.group_before.key,
            group_revision: request.group_before.revision,
            group_walk_digest: request.group_before.diagnostic_digest(),
            selection_cache_revision,
        }
    }

    fn validates_for(&self, request: &AirGroupActionRequest) -> bool {
        self.position == request.position
            && self.group_packet_digest == packet_digest(&request.group_packet)
            && self.selection == request.selection
            && self.resolved_group == request.group_before.key
            && self.group_revision == request.group_before.revision
            && self.group_walk_digest == request.group_before.diagnostic_digest()
    }
}

/// One immutable host capture.  `contained_identities` is parallel to
/// `facts.members[*].contained`; it supplies the generational identity the planner's bare
/// retail addresses intentionally do not carry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AirGroupActionSnapshot {
    pub group_before: GroupBeforeImage,
    pub authority: AuthorityRevisions,
    pub canonical_group_packet: Option<CanonicalGroupPacketReceipt>,
    pub ignore_orders: IgnoreOrdersSnapshot,
    pub type_authority: AirTypeAuthoritySnapshot,
    pub integration: AirIntegrationCapabilities,
    pub facts: AirLaunchFacts,
    pub contained_identities: Vec<Vec<CanonicalObjectIdentity>>,
    /// Parallel to the computed install list, in retail emission order.
    pub target_orders: Vec<AirOrderTargetBefore>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AirTypeAuthorityColumn {
    Domain,
    ObjectMasks,
    UnitFlags,
    NonstrictTypeRelations,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AirPayloadCapability {
    MoveOrderTagV1,
    AirPatrolOrderTagV1,
    DynamicPatrolArrays,
    MoveUnitWork,
    AirPatrolUnitWork,
    MoveSaveReloadResume,
    AirPatrolSaveReloadResume,
}

/// Every refusal is a named authority or exact-image mismatch.  None substitutes a default.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AirTransactionBlocker {
    ActionPrecedesGroup {
        group_command_index: u16,
        action_command_index: u16,
    },
    GroupWire(GroupSelectionWireError),
    Wire(AirGroupWireError),
    GroupSelectionDoesNotMatchPacket,
    CommandDoesNotMatchPacket,
    GroupOwnerDoesNotMatchReceiver {
        packet: u8,
        receiver: u8,
    },
    CanonicalGroupPackageHostUnavailable,
    CanonicalGroupPackageReceiptMismatch,
    GroupReselectionCacheUnavailable,
    MalformedGroupImage(GroupImageError),
    GroupBeforeImageChanged,
    AuthorityRevisionChanged,
    IgnoreOrdersUnavailable,
    IgnoreOrdersPruneUnavailable,
    PacketToSimRouteUnavailable,
    TypeAuthorityUnavailable(AirTypeAuthorityColumn),
    FactsOwnerMismatch {
        expected: u8,
        actual: u8,
    },
    MemberFactCountMismatch {
        expected: usize,
        actual: usize,
    },
    MemberFactAddressMismatch {
        index: usize,
    },
    ContainmentChainCountMismatch {
        expected: usize,
        actual: usize,
    },
    ContainmentIdentityCountMismatch {
        member: usize,
        expected: usize,
        actual: usize,
    },
    ContainmentIdentityMismatch {
        member: usize,
        link: usize,
    },
    InvalidContainmentIdentity {
        member: usize,
        link: usize,
    },
    Planner(AirLaunchBoundary),
    TargetBeforeCountMismatch {
        expected: usize,
        actual: usize,
    },
    TargetBeforeMismatch {
        install: usize,
    },
    DuplicateInstallTarget {
        install: usize,
    },
    PayloadCapabilityUnavailable(AirPayloadCapability),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AirGroupActionPlan {
    Scramble(ScramblePlan),
    LaunchPatrol(LaunchPatrolPlan),
}

impl AirGroupActionPlan {
    pub fn installs(&self) -> &[AirLaunchInstall] {
        match self {
            Self::Scramble(plan) => &plan.installs,
            Self::LaunchPatrol(plan) => &plan.installs,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedAirGroupAction {
    pub request: AirGroupActionRequest,
    pub snapshot: AirGroupActionSnapshot,
    pub plan: AirGroupActionPlan,
}

fn validate_type_authority(
    command: AirGroupCommand,
    authority: AirTypeAuthoritySnapshot,
    expected_revision: u64,
) -> Result<(), AirTransactionBlocker> {
    if authority.revision != expected_revision {
        return Err(AirTransactionBlocker::AuthorityRevisionChanged);
    }
    for (present, column) in [
        (authority.domain_column, AirTypeAuthorityColumn::Domain),
        (
            authority.object_masks_column,
            AirTypeAuthorityColumn::ObjectMasks,
        ),
        (
            authority.unit_flags_column,
            AirTypeAuthorityColumn::UnitFlags,
        ),
    ] {
        if !present {
            return Err(AirTransactionBlocker::TypeAuthorityUnavailable(column));
        }
    }
    if matches!(command, AirGroupCommand::LaunchPatrol(_)) && !authority.nonstrict_type_relations {
        return Err(AirTransactionBlocker::TypeAuthorityUnavailable(
            AirTypeAuthorityColumn::NonstrictTypeRelations,
        ));
    }
    Ok(())
}

fn validate_fact_identities(
    snapshot: &AirGroupActionSnapshot,
) -> Result<(), AirTransactionBlocker> {
    let owner = snapshot.group_before.key.owner;
    if snapshot.facts.who != owner {
        return Err(AirTransactionBlocker::FactsOwnerMismatch {
            expected: owner,
            actual: snapshot.facts.who,
        });
    }
    let expected = snapshot.group_before.members.len();
    if snapshot.facts.members.len() != expected {
        return Err(AirTransactionBlocker::MemberFactCountMismatch {
            expected,
            actual: snapshot.facts.members.len(),
        });
    }
    if snapshot.contained_identities.len() != expected {
        return Err(AirTransactionBlocker::ContainmentChainCountMismatch {
            expected,
            actual: snapshot.contained_identities.len(),
        });
    }
    for member in 0..expected {
        if snapshot.facts.members[member].object != snapshot.group_before.members[member].address()
        {
            return Err(AirTransactionBlocker::MemberFactAddressMismatch { index: member });
        }
        let facts = &snapshot.facts.members[member].contained;
        let identities = &snapshot.contained_identities[member];
        if identities.len() != facts.len() {
            return Err(AirTransactionBlocker::ContainmentIdentityCountMismatch {
                member,
                expected: facts.len(),
                actual: identities.len(),
            });
        }
        for (link, (fact, identity)) in facts.iter().zip(identities).enumerate() {
            if !identity.is_well_formed() || (fact.is_unit && !identity.is_unit()) {
                return Err(AirTransactionBlocker::InvalidContainmentIdentity { member, link });
            }
            if fact.object != identity.address() {
                return Err(AirTransactionBlocker::ContainmentIdentityMismatch { member, link });
            }
        }
    }
    Ok(())
}

fn identity_for_install(
    snapshot: &AirGroupActionSnapshot,
    install: AirLaunchInstall,
) -> Option<CanonicalObjectIdentity> {
    snapshot
        .facts
        .members
        .iter()
        .zip(&snapshot.contained_identities)
        .flat_map(|(member, identities)| member.contained.iter().zip(identities))
        .find_map(|(fact, identity)| (fact.object == install.plane()).then_some(*identity))
}

fn validate_targets(
    snapshot: &AirGroupActionSnapshot,
    installs: &[AirLaunchInstall],
) -> Result<(), AirTransactionBlocker> {
    if snapshot.target_orders.len() != installs.len() {
        return Err(AirTransactionBlocker::TargetBeforeCountMismatch {
            expected: installs.len(),
            actual: snapshot.target_orders.len(),
        });
    }
    let mut seen = BTreeSet::new();
    for (index, (install, target)) in installs.iter().zip(&snapshot.target_orders).enumerate() {
        let expected = identity_for_install(snapshot, *install);
        if expected != Some(target.identity) || !target.identity.is_unit() {
            return Err(AirTransactionBlocker::TargetBeforeMismatch { install: index });
        }
        if !seen.insert(target.identity) {
            return Err(AirTransactionBlocker::DuplicateInstallTarget { install: index });
        }
    }
    Ok(())
}

fn require_payload_capabilities(
    integration: AirIntegrationCapabilities,
    installs: &[AirLaunchInstall],
) -> Result<(), AirTransactionBlocker> {
    let needs_move = installs
        .iter()
        .any(|install| install.kind() == OrderIndex::MoveTo);
    let needs_air_patrol = installs
        .iter()
        .any(|install| install.kind() == OrderIndex::AirPatrol);
    let required = [
        (
            needs_move && !integration.move_order_tag_v1,
            AirPayloadCapability::MoveOrderTagV1,
        ),
        (
            needs_move && !integration.move_unit_work,
            AirPayloadCapability::MoveUnitWork,
        ),
        (
            needs_move && !integration.move_save_reload_resume,
            AirPayloadCapability::MoveSaveReloadResume,
        ),
        (
            needs_air_patrol && !integration.air_patrol_order_tag_v1,
            AirPayloadCapability::AirPatrolOrderTagV1,
        ),
        (
            needs_air_patrol && !integration.dynamic_patrol_arrays,
            AirPayloadCapability::DynamicPatrolArrays,
        ),
        (
            needs_air_patrol && !integration.air_patrol_unit_work,
            AirPayloadCapability::AirPatrolUnitWork,
        ),
        (
            needs_air_patrol && !integration.air_patrol_save_reload_resume,
            AirPayloadCapability::AirPatrolSaveReloadResume,
        ),
    ];
    for (missing, capability) in required {
        if missing {
            return Err(AirTransactionBlocker::PayloadCapabilityUnavailable(
                capability,
            ));
        }
    }
    Ok(())
}

/// Validate one immutable image and recompute the recovered retail plan.
pub fn prepare_air_group_action(
    request: &AirGroupActionRequest,
    snapshot: &AirGroupActionSnapshot,
) -> Result<PreparedAirGroupAction, AirTransactionBlocker> {
    if request.position.group_command_index >= request.position.action_command_index {
        return Err(AirTransactionBlocker::ActionPrecedesGroup {
            group_command_index: request.position.group_command_index,
            action_command_index: request.position.action_command_index,
        });
    }
    let selection = decode_group_selection_packet(&request.group_packet)
        .map_err(AirTransactionBlocker::GroupWire)?;
    if selection != request.selection {
        return Err(AirTransactionBlocker::GroupSelectionDoesNotMatchPacket);
    }
    let decoded = decode_air_group_packet(&request.packet).map_err(AirTransactionBlocker::Wire)?;
    if decoded != request.command {
        return Err(AirTransactionBlocker::CommandDoesNotMatchPacket);
    }
    request
        .group_before
        .validate()
        .map_err(AirTransactionBlocker::MalformedGroupImage)?;
    snapshot
        .group_before
        .validate()
        .map_err(AirTransactionBlocker::MalformedGroupImage)?;
    if snapshot.group_before != request.group_before {
        return Err(AirTransactionBlocker::GroupBeforeImageChanged);
    }
    if request.selection.owner != request.group_before.key.owner {
        return Err(AirTransactionBlocker::GroupOwnerDoesNotMatchReceiver {
            packet: request.selection.owner,
            receiver: request.group_before.key.owner,
        });
    }
    let Some(group_receipt) = snapshot.canonical_group_packet.as_ref() else {
        return Err(AirTransactionBlocker::CanonicalGroupPackageHostUnavailable);
    };
    if !group_receipt.validates_for(request) {
        return Err(AirTransactionBlocker::CanonicalGroupPackageReceiptMismatch);
    }
    if request.selection.is_cached_reselection() && group_receipt.selection_cache_revision.is_none()
    {
        return Err(AirTransactionBlocker::GroupReselectionCacheUnavailable);
    }
    if snapshot.authority != request.authority {
        return Err(AirTransactionBlocker::AuthorityRevisionChanged);
    }
    match snapshot.ignore_orders {
        IgnoreOrdersSnapshot::Unavailable => {
            return Err(AirTransactionBlocker::IgnoreOrdersUnavailable)
        }
        IgnoreOrdersSnapshot::Armed { revision } => {
            if revision != request.authority.scenario {
                return Err(AirTransactionBlocker::AuthorityRevisionChanged);
            }
            return Err(AirTransactionBlocker::IgnoreOrdersPruneUnavailable);
        }
        IgnoreOrdersSnapshot::Clear { revision } => {
            if revision != request.authority.scenario {
                return Err(AirTransactionBlocker::AuthorityRevisionChanged);
            }
        }
    }
    if !snapshot.integration.packet_to_sim_route {
        return Err(AirTransactionBlocker::PacketToSimRouteUnavailable);
    }
    validate_type_authority(
        request.command,
        snapshot.type_authority,
        request.authority.types,
    )?;
    validate_fact_identities(snapshot)?;

    let plan = match request.command {
        AirGroupCommand::Scramble => AirGroupActionPlan::Scramble(
            plan_scramble(&snapshot.facts).map_err(AirTransactionBlocker::Planner)?,
        ),
        AirGroupCommand::LaunchPatrol(launch) => AirGroupActionPlan::LaunchPatrol(
            plan_launch_patrol(&launch, &snapshot.facts).map_err(AirTransactionBlocker::Planner)?,
        ),
    };
    validate_targets(snapshot, plan.installs())?;
    require_payload_capabilities(snapshot.integration, plan.installs())?;
    Ok(PreparedAirGroupAction {
        request: request.clone(),
        snapshot: snapshot.clone(),
        plan,
    })
}

/// The canonical state an adapter says it wrote for one planned install.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommittedAirInstall {
    pub identity: CanonicalObjectIdentity,
    pub kind: OrderIndex,
    pub order_revision_after: u64,
    pub order_digest_after: u64,
    pub unit_mask_after: u32,
    pub action_revision_after: u64,
    pub path_revision_after: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AirCommitEvidence {
    pub state_digest_before: u64,
    pub state_digest_after: u64,
    pub installs: Vec<CommittedAirInstall>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AirCommitFailure {
    StaleGroup,
    StaleScenario,
    StaleTypes,
    StaleWorldOrders,
    StaleTarget(CanonicalObjectIdentity),
    HostRejected(&'static str),
    CommitEvidenceMismatch,
}

/// Minimal host seam.  `checkpoint` and `restore` cover every canonical column the commit
/// may touch, including groups, order queues, unit masks, actions, and paths.
pub trait AtomicAirGroupActionHost {
    type Checkpoint;

    fn checkpoint(&self) -> Self::Checkpoint;
    fn state_digest(&self) -> u64;

    /// Re-read the exact group bytes/revision, ordered member identities, scenario/type/world
    /// revisions, containment identities, and target before-images in `prepared.snapshot`.
    fn revalidate(&self, prepared: &PreparedAirGroupAction) -> Result<(), AirCommitFailure>;

    /// Apply every install in retail order.  Returning an error after a partial write is
    /// allowed; [`execute_air_group_action`] restores the checkpoint before returning.
    fn commit(
        &mut self,
        prepared: &PreparedAirGroupAction,
    ) -> Result<Vec<CommittedAirInstall>, AirCommitFailure>;

    fn restore(&mut self, checkpoint: Self::Checkpoint);
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AirTransactionStatus {
    Blocked(AirTransactionBlocker),
    Applied(AirCommitEvidence),
    RolledBack {
        failure: AirCommitFailure,
        state_digest_before: u64,
        state_digest_after_restore: u64,
        restored: bool,
    },
}

/// Receipt containing enough immutable input to recompute every planner decision.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AirGroupActionReceipt {
    pub request: AirGroupActionRequest,
    pub snapshot: AirGroupActionSnapshot,
    pub plan: Option<AirGroupActionPlan>,
    pub status: AirTransactionStatus,
}

fn commit_evidence_matches(
    prepared: &PreparedAirGroupAction,
    evidence: &AirCommitEvidence,
) -> bool {
    let installs = prepared.plan.installs();
    if evidence.installs.len() != installs.len() {
        return false;
    }
    if installs.is_empty() {
        if evidence.state_digest_before != evidence.state_digest_after {
            return false;
        }
    } else if evidence.state_digest_before == evidence.state_digest_after {
        return false;
    }
    installs
        .iter()
        .zip(&prepared.snapshot.target_orders)
        .zip(&evidence.installs)
        .all(|((install, before), committed)| {
            let expected_mask = match install {
                AirLaunchInstall::MoveFacing {
                    clear_unit_mask, ..
                } => before.unit_mask & !clear_unit_mask,
                AirLaunchInstall::AirPatrol { .. } => before.unit_mask,
            };
            committed.identity == before.identity
                && committed.kind == install.kind()
                && committed.order_revision_after != before.order_revision
                && committed.unit_mask_after == expected_mask
                && committed.action_revision_after != before.action_revision
                && committed.path_revision_after != before.path_revision
        })
}

impl AirGroupActionReceipt {
    pub fn validates(&self) -> bool {
        match prepare_air_group_action(&self.request, &self.snapshot) {
            Err(blocker) => {
                self.plan.is_none() && self.status == AirTransactionStatus::Blocked(blocker)
            }
            Ok(prepared) => {
                if self.plan.as_ref() != Some(&prepared.plan) {
                    return false;
                }
                match &self.status {
                    AirTransactionStatus::Applied(evidence) => {
                        commit_evidence_matches(&prepared, evidence)
                    }
                    AirTransactionStatus::RolledBack {
                        state_digest_before,
                        state_digest_after_restore,
                        restored,
                        ..
                    } => *restored && state_digest_before == state_digest_after_restore,
                    AirTransactionStatus::Blocked(_) => false,
                }
            }
        }
    }

    pub fn validates_for(
        &self,
        request: &AirGroupActionRequest,
        snapshot: &AirGroupActionSnapshot,
    ) -> bool {
        &self.request == request && &self.snapshot == snapshot && self.validates()
    }
}

/// Prepare, revalidate and atomically commit one command.
pub fn execute_air_group_action<H: AtomicAirGroupActionHost>(
    host: &mut H,
    request: &AirGroupActionRequest,
    snapshot: &AirGroupActionSnapshot,
) -> AirGroupActionReceipt {
    let prepared = match prepare_air_group_action(request, snapshot) {
        Ok(prepared) => prepared,
        Err(blocker) => {
            return AirGroupActionReceipt {
                request: request.clone(),
                snapshot: snapshot.clone(),
                plan: None,
                status: AirTransactionStatus::Blocked(blocker),
            }
        }
    };
    let checkpoint = host.checkpoint();
    let before = host.state_digest();
    if let Err(failure) = host.revalidate(&prepared) {
        host.restore(checkpoint);
        let after = host.state_digest();
        return AirGroupActionReceipt {
            request: prepared.request,
            snapshot: prepared.snapshot,
            plan: Some(prepared.plan),
            status: AirTransactionStatus::RolledBack {
                failure,
                state_digest_before: before,
                state_digest_after_restore: after,
                restored: before == after,
            },
        };
    }
    let committed = match host.commit(&prepared) {
        Ok(committed) => committed,
        Err(failure) => {
            host.restore(checkpoint);
            let after = host.state_digest();
            return AirGroupActionReceipt {
                request: prepared.request,
                snapshot: prepared.snapshot,
                plan: Some(prepared.plan),
                status: AirTransactionStatus::RolledBack {
                    failure,
                    state_digest_before: before,
                    state_digest_after_restore: after,
                    restored: before == after,
                },
            };
        }
    };
    let after = host.state_digest();
    let evidence = AirCommitEvidence {
        state_digest_before: before,
        state_digest_after: after,
        installs: committed,
    };
    if !commit_evidence_matches(&prepared, &evidence) {
        host.restore(checkpoint);
        let after_restore = host.state_digest();
        return AirGroupActionReceipt {
            request: prepared.request,
            snapshot: prepared.snapshot,
            plan: Some(prepared.plan),
            status: AirTransactionStatus::RolledBack {
                failure: AirCommitFailure::CommitEvidenceMismatch,
                state_digest_before: before,
                state_digest_after_restore: after_restore,
                restored: before == after_restore,
            },
        };
    }
    AirGroupActionReceipt {
        request: prepared.request,
        snapshot: prepared.snapshot,
        plan: Some(prepared.plan),
        status: AirTransactionStatus::Applied(evidence),
    }
}
