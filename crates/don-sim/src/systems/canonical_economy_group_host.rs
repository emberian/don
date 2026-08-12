// SPDX-License-Identifier: GPL-3.0-or-later
//! Canonical packet planner for the first Board/Repair/Trade Group cohort.
//!
//! The selector is the production Group→Move selector: one play-keyed `(o,uid)` cache,
//! one fixed [`Groups`] allocator and one set of staged Unit backlinks. The economy action
//! then plans against those detached after-images. Nothing in this module can publish only
//! the selection; [`crate::tick::Sim::process_economy_command_package`] commits the fixed
//! `Groups`, Unit/order/path columns, and play-keyed selection cache together. The resulting
//! concrete orders use the v13 typed save envelope.

use crate::command::economy_group_actions::{EconomyStep, QUEUE_FIRST, QUEUE_LAST, QUEUE_NEW};
use crate::order::{OrderIndex, ORDER_GROUP};
use crate::systems::canonical_group_move_host::{
    ensure_mutation_for_row, groups_equal, prepare_group_selection, unit_still_current,
    CommandPackageState, GroupMoveAuthority, GroupSelectionUse, PackageError,
    PreparedGroupSelection, UnitIdentity, NETWORK_PLAYERS,
};
use crate::systems::economy_containment_group_host::{
    decode_economy_group_packet_pair, CachedSelectionIdentity, CanonicalGroupLocator,
    CanonicalGroupMutation, CanonicalGroupSelectionReceipt, CanonicalGroupSelectionResult,
    CanonicalMemberGroupBacklink, CanonicalObjectIdentity, CanonicalSelectionCacheImage,
    CanonicalStateDigest, CommandPackagePosition, EconomyGroupAction, EconomyGroupActionFacts,
    EconomyGroupPacketPair, EconomyGroupPreflightError, EconomyGroupTransactionRequest,
    EconomyOrderCapabilities,
};
use crate::systems::economy_order_payload_authority::{
    CastOrderPayload, EconomyOrderAuthorityError, EconomyOrderHeader, EconomyOrderNode,
    EconomyOrderPayload, StableTargetIdentity, TradeOrderPayload, DON_SAVE_V13,
};
use crate::systems::group_action_trade_frontier::{
    plan_group_action_trade, GroupActionTradeFacts, GroupActionTradeRequest,
    GroupActionTradeTransaction, GroupAfterAuthority, ObjectIdentity, RawTradeAction,
    TradeActionEvent, TradeGroupSnapshot, TradeInstallStep, TradeOutcome, TradePlanError,
};
use crate::systems::groups_guys::{CheckSum, GroupData, Groups, GROUP_MAX_MEMBERS};
use crate::systems::movement::PathStack;
use crate::systems::sparse_object_bands_authority_frontier::{RetailBand, RetailObjectAddress};
use crate::world::{World, WorldObjectIdentity};

/// Stable binding for one target/member fact supplied by an installed economy adapter.
///
/// Unit identities bind to the generational World handle. Build/Wall identities bind to the
/// corresponding sparse-registry row. The composition digest covers the external UID/type facts
/// which the compact World does not own yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EconomyObjectBinding {
    pub identity: CanonicalObjectIdentity,
    pub stable: WorldObjectIdentity,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EconomyActionAuthority {
    pub action: EconomyGroupAction,
    pub facts: EconomyGroupActionFacts,
    /// Mandatory for TRADE. Its exact 1,022-byte frontier owns installer chronology and the
    /// complete two-endpoint TradeRoute payload.
    pub trade: Option<GroupActionTradeFacts>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EconomyRuntimeAuthority {
    pub revision: u64,
    pub composition_digest: [u8; 32],
    pub ignore_orders: bool,
    pub bindings: Vec<EconomyObjectBinding>,
    pub actions: Vec<EconomyActionAuthority>,
}

impl EconomyRuntimeAuthority {
    fn action(&self, action: EconomyGroupAction) -> Option<&EconomyActionAuthority> {
        self.actions.iter().find(|entry| entry.action == action)
    }

    fn binding(&self, identity: CanonicalObjectIdentity) -> Option<EconomyObjectBinding> {
        self.bindings
            .iter()
            .copied()
            .find(|entry| entry.identity == identity)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EconomyPackageError {
    Selection(PackageError),
    Wire(crate::systems::economy_containment_group_host::EconomyGroupWireError),
    PlayOutOfRange { play: usize },
    MissingPlayerMap { play: usize },
    PlayerOwnerMismatch { play: usize, expected: u8, got: u8 },
    MissingAuthority,
    MissingCompositionDigest,
    DuplicateAuthorityAction,
    MissingObjectBinding(CanonicalObjectIdentity),
    StaleObjectBinding(CanonicalObjectIdentity),
    SelectionReceipt,
    Preflight(EconomyGroupPreflightError),
    QueueFirstUnsupported,
    MissingTradeFrontier,
    TradeFrontier(TradePlanError),
    TradeFrontierMismatch,
    MissingActionTarget,
    MissingSelectedMember(i16),
    Payload(EconomyOrderAuthorityError),
    StaleCommandState,
    StaleGroups,
    StaleSelectionAuthority,
    StaleEconomyAuthority,
    StaleUnit(crate::Handle),
    StaleRandomState,
}

impl From<PackageError> for EconomyPackageError {
    fn from(value: PackageError) -> Self {
        Self::Selection(value)
    }
}

impl From<EconomyOrderAuthorityError> for EconomyPackageError {
    fn from(value: EconomyOrderAuthorityError) -> Self {
        Self::Payload(value)
    }
}

/// Ordered mutation requested by one exact group-action body. The shared commit adapter applies
/// these to the detached Unit/order/path after-images before its one revalidate/publish boundary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlannedEconomyEffect {
    InstallOrder {
        actor: UnitIdentity,
        queue: i32,
        node: EconomyOrderNode,
    },
    ClearOrders {
        actor: CanonicalObjectIdentity,
    },
    ClearPartialPath {
        actor: CanonicalObjectIdentity,
    },
    SetUnitMasks {
        actor: CanonicalObjectIdentity,
        after: u32,
    },
    RetireRepairTarget {
        target: CanonicalObjectIdentity,
    },
}

#[derive(Clone, Debug)]
pub struct PreparedEconomyGroupPackage {
    pub play: usize,
    pub lockstep_serial: i32,
    pub frame: i32,
    pub random_state: i32,
    pub pair: EconomyGroupPacketPair,
    pub selection: PreparedGroupSelection,
    pub request: EconomyGroupTransactionRequest,
    pub plan: crate::systems::economy_containment_group_host::CanonicalEconomyGroupPlan,
    pub trade: Option<GroupActionTradeTransaction>,
    pub effects: Vec<PlannedEconomyEffect>,
    pub authority_revision: u64,
    pub authority_digest: [u8; 32],
    pub authority_snapshot: EconomyRuntimeAuthority,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EconomyGroupPackageReceipt {
    pub play: usize,
    pub lockstep_serial: i32,
    pub frame: i32,
    pub opcode: u8,
    pub who: u8,
    pub group_slot: usize,
    pub installed_orders: usize,
    pub command_state_revision: u64,
    pub groups_checksum: u32,
    pub random_state_before: i32,
    pub random_state_after: i32,
}

fn decode_package(
    frame: i32,
    play: usize,
    lockstep_serial: i32,
    bytes: &[u8],
) -> Result<EconomyGroupPacketPair, EconomyPackageError> {
    if bytes.len() < 3 {
        return Err(EconomyPackageError::Wire(
            crate::systems::economy_containment_group_host::EconomyGroupWireError::Empty,
        ));
    }
    let group_len = 3usize
        .checked_add(usize::from(bytes[1]).saturating_mul(2))
        .ok_or(EconomyPackageError::Wire(
            crate::systems::economy_containment_group_host::EconomyGroupWireError::TruncatedGroupPrefix(
                bytes.len(),
            ),
        ))?;
    if group_len > bytes.len() {
        return Err(EconomyPackageError::Wire(
            crate::systems::economy_containment_group_host::EconomyGroupWireError::TruncatedGroupPrefix(
                bytes.len(),
            ),
        ));
    }
    decode_economy_group_packet_pair(
        CommandPackagePosition {
            game_frame: frame,
            package_serial: lockstep_serial as u32,
            play: play as i32,
            group_command_index: 0,
            action_command_index: 1,
        },
        &bytes[..group_len],
        &bytes[group_len..],
    )
    .map_err(EconomyPackageError::Wire)
}

fn retail_band(o: i16) -> Option<RetailBand> {
    let o = i32::from(o);
    RetailBand::ALL.into_iter().find(|band| band.contains(o))
}

fn binding_is_current(world: &World, binding: EconomyObjectBinding) -> bool {
    let identity = binding.identity;
    let Some(band) = retail_band(identity.o) else {
        return false;
    };
    let address = RetailObjectAddress::new(identity.owner, band, i32::from(identity.o));
    if world.object_bands().live_identity(address) != Some(binding.stable) {
        return false;
    }
    match binding.stable {
        WorldObjectIdentity::Unit { id, generation } => world
            .unit_row_at(i32::from(identity.owner), i32::from(identity.o))
            .is_some_and(|row| {
                generation == identity.generation
                    && world.units.get_uid(row) == identity.uid
                    && world.handle_at_row(row) == Some(crate::Handle { id, generation })
            }),
        // Build/Wall pools do not expose a separate generation/UID column yet. Their sparse
        // stable row is the canonical runtime binding; the installed composition digest owns
        // the external UID fact used by the retail order.
        WorldObjectIdentity::BuildRow(row) | WorldObjectIdentity::WallRow(row) => {
            row == identity.generation
        }
    }
}

fn validate_fact_identities(
    world: &World,
    authority: &EconomyRuntimeAuthority,
    facts: &EconomyGroupActionFacts,
) -> Result<(), EconomyPackageError> {
    let check = |identity: CanonicalObjectIdentity| {
        let binding = authority
            .binding(identity)
            .ok_or(EconomyPackageError::MissingObjectBinding(identity))?;
        if !binding_is_current(world, binding) {
            return Err(EconomyPackageError::StaleObjectBinding(identity));
        }
        Ok(())
    };
    match facts {
        EconomyGroupActionFacts::BoardShip { ship, members } => {
            if let Some(ship) = ship {
                check(*ship)?;
            }
            for member in members {
                check(member.identity)?;
            }
        }
        EconomyGroupActionFacts::Repair { target, members } => {
            if let Some(target) = target {
                check(*target)?;
            }
            for member in members {
                check(member.identity)?;
            }
        }
        EconomyGroupActionFacts::Trade {
            first,
            second,
            members,
            ..
        } => {
            if let Some(first) = first {
                check(*first)?;
            }
            if let Some(second) = second {
                check(*second)?;
            }
            for member in members {
                check(member.identity)?;
            }
        }
    }
    Ok(())
}

fn mix_u64(lanes: &mut [u64; 4], value: u64) {
    for (index, lane) in lanes.iter_mut().enumerate() {
        *lane ^= value.rotate_left((index * 13) as u32);
        *lane = lane
            .wrapping_mul(0x1000_0000_01b3)
            .rotate_left((11 + index) as u32);
    }
}

fn selection_digest(
    world_digest: u64,
    groups: &Groups,
    paths: &[PathStack],
    command_state: &CommandPackageState,
    authority_digest: [u8; 32],
) -> CanonicalStateDigest {
    let mut lanes = [
        0xcbf2_9ce4_8422_2325,
        0x9e37_79b9_7f4a_7c15,
        0xd6e8_feb8_6659_fd93,
        0xa076_1d64_78bd_642f,
    ];
    mix_u64(&mut lanes, world_digest);
    let mut checksum = CheckSum::default();
    groups.check_groups(&mut checksum);
    mix_u64(&mut lanes, u64::from(checksum.value));
    mix_u64(&mut lanes, command_state.revision());
    for play in 0..NETWORK_PLAYERS {
        for entry in command_state.selection(play).unwrap_or_default() {
            mix_u64(
                &mut lanes,
                ((play as u64) << 48) ^ ((entry.o as u16 as u64) << 16) ^ u64::from(entry.uid),
            );
        }
    }
    for path in paths {
        for byte in path.walk_bytes() {
            mix_u64(&mut lanes, u64::from(byte));
        }
    }
    for chunk in authority_digest.chunks_exact(8) {
        mix_u64(&mut lanes, u64::from_le_bytes(chunk.try_into().unwrap()));
    }
    let mut out = [0u8; 32];
    for (index, lane) in lanes.into_iter().enumerate() {
        out[index * 8..index * 8 + 8].copy_from_slice(&lane.to_le_bytes());
    }
    CanonicalStateDigest(out)
}

fn canonical_identity(member: &UnitIdentity) -> CanonicalObjectIdentity {
    CanonicalObjectIdentity {
        owner: member.who,
        o: member.o,
        uid: member.uid,
        generation: member.handle.generation,
    }
}

fn selection_receipt(
    pair: EconomyGroupPacketPair,
    selection: &PreparedGroupSelection,
    state_before: CanonicalStateDigest,
    state_after: CanonicalStateDigest,
) -> Result<CanonicalGroupSelectionReceipt, EconomyPackageError> {
    let before_cache = selection
        .command_state_before
        .selection(selection.play)
        .ok_or(EconomyPackageError::SelectionReceipt)?;
    let after_cache = selection
        .command_state_after
        .selection(selection.play)
        .ok_or(EconomyPackageError::SelectionReceipt)?;
    let selected: Vec<_> = selection
        .members
        .iter()
        .map(|member| canonical_identity(&member.identity))
        .collect();
    let mut group_mutations = Vec::new();
    for index in 0..selection.groups_before.list.len() {
        if selection.groups_before.list[index] != selection.groups_after.list[index]
            || index == selection.group_slot
        {
            group_mutations.push(CanonicalGroupMutation {
                locator: CanonicalGroupLocator::from_index(index)
                    .ok_or(EconomyPackageError::SelectionReceipt)?,
                before: selection.groups_before.list[index].clone(),
                after: selection.groups_after.list[index].clone(),
            });
        }
    }
    let mut member_backlinks = Vec::with_capacity(selected.len());
    for (member, identity) in selection.members.iter().zip(&selected) {
        let mutation = selection
            .units
            .iter()
            .find(|mutation| mutation.before.identity.handle == member.identity.handle)
            .ok_or(EconomyPackageError::SelectionReceipt)?;
        member_backlinks.push(CanonicalMemberGroupBacklink {
            identity: *identity,
            before: mutation.before.group,
            after: mutation.after.group,
        });
    }
    Ok(CanonicalGroupSelectionReceipt {
        pair,
        cache_before: CanonicalSelectionCacheImage {
            revision: selection.command_state_before.revision(),
            entries: before_cache
                .iter()
                .map(|entry| CachedSelectionIdentity {
                    o: entry.o,
                    uid: entry.uid,
                })
                .collect(),
        },
        cache_after: CanonicalSelectionCacheImage {
            revision: selection.command_state_after.revision(),
            entries: after_cache
                .iter()
                .map(|entry| CachedSelectionIdentity {
                    o: entry.o,
                    uid: entry.uid,
                })
                .collect(),
        },
        selected,
        group_mutations,
        member_backlinks,
        last_group_before: selection.groups_before.last_group,
        last_group_after: selection.groups_after.last_group,
        state_before,
        state_after,
        result: CanonicalGroupSelectionResult::Selected {
            locator: CanonicalGroupLocator::from_index(selection.group_slot)
                .ok_or(EconomyPackageError::SelectionReceipt)?,
            retained_group_id: selection.groups_before.list[selection.group_slot].id,
        },
    })
}

fn stable_target(
    authority: &EconomyRuntimeAuthority,
    identity: CanonicalObjectIdentity,
) -> Result<StableTargetIdentity, EconomyPackageError> {
    let binding = authority
        .binding(identity)
        .ok_or(EconomyPackageError::MissingObjectBinding(identity))?;
    Ok(match binding.stable {
        WorldObjectIdentity::Unit { id, generation } => StableTargetIdentity::live(
            i32::from(identity.o),
            i32::from(identity.owner),
            identity.uid,
            crate::Handle { id, generation },
        ),
        WorldObjectIdentity::BuildRow(_) | WorldObjectIdentity::WallRow(_) => {
            StableTargetIdentity::banded(
                i32::from(identity.o),
                i32::from(identity.owner),
                identity.uid,
            )
        }
    })
}

fn selected_actor(
    selection: &PreparedGroupSelection,
    o: i16,
) -> Result<UnitIdentity, EconomyPackageError> {
    selection
        .members
        .iter()
        .find(|member| member.identity.o == o)
        .map(|member| member.identity.clone())
        .ok_or(EconomyPackageError::MissingSelectedMember(o))
}

fn node(
    kind: OrderIndex,
    flags: u8,
    x: i32,
    y: i32,
    primary: StableTargetIdentity,
    payload: EconomyOrderPayload,
) -> Result<EconomyOrderNode, EconomyPackageError> {
    let node = EconomyOrderNode {
        metric: 0,
        header: EconomyOrderHeader {
            kind,
            flags,
            x,
            y,
            primary,
        },
        payload,
    };
    node.validate()?;
    Ok(node)
}

fn plan_nontrade_effects(
    selection: &PreparedGroupSelection,
    authority: &EconomyRuntimeAuthority,
    facts: &EconomyGroupActionFacts,
    steps: &[EconomyStep],
) -> Result<Vec<PlannedEconomyEffect>, EconomyPackageError> {
    let target = match facts {
        EconomyGroupActionFacts::BoardShip { ship, .. } => *ship,
        EconomyGroupActionFacts::Repair { target, .. } => *target,
        EconomyGroupActionFacts::Trade { .. } => None,
    };
    let mut effects = Vec::new();
    for step in steps {
        match *step {
            EconomyStep::SetGroupForm(_) => {}
            EconomyStep::AddCastOrder {
                o,
                x,
                y,
                spell,
                queue,
            } => {
                if queue == QUEUE_FIRST {
                    return Err(EconomyPackageError::QueueFirstUnsupported);
                }
                effects.push(PlannedEconomyEffect::InstallOrder {
                    actor: selected_actor(selection, o)?,
                    queue,
                    node: node(
                        OrderIndex::CastSpell,
                        ORDER_GROUP,
                        x,
                        y,
                        StableTargetIdentity::NONE,
                        EconomyOrderPayload::CastSpell(CastOrderPayload { paid: 0, spell }),
                    )?,
                });
            }
            EconomyStep::AddRepairOrder { o, queue, .. } => {
                let target = target.ok_or(EconomyPackageError::MissingActionTarget)?;
                effects.push(PlannedEconomyEffect::InstallOrder {
                    actor: selected_actor(selection, o)?,
                    queue,
                    node: node(
                        OrderIndex::Repair,
                        ORDER_GROUP,
                        0,
                        0,
                        stable_target(authority, target)?,
                        EconomyOrderPayload::TargetOnly,
                    )?,
                });
            }
            EconomyStep::AddBoardOrder { o, queue, .. } => {
                let ship = target.ok_or(EconomyPackageError::MissingActionTarget)?;
                effects.push(PlannedEconomyEffect::InstallOrder {
                    actor: selected_actor(selection, o)?,
                    queue,
                    node: node(
                        OrderIndex::BoardShip,
                        ORDER_GROUP,
                        0,
                        0,
                        stable_target(authority, ship)?,
                        EconomyOrderPayload::TargetOnly,
                    )?,
                });
            }
            EconomyStep::ClearShipOrders { .. } => {
                effects.push(PlannedEconomyEffect::ClearOrders {
                    actor: target.ok_or(EconomyPackageError::MissingActionTarget)?,
                });
            }
            EconomyStep::AddAwaitBoardOrder { passenger_o, .. } => {
                let ship = target.ok_or(EconomyPackageError::MissingActionTarget)?;
                let passenger = selection
                    .members
                    .iter()
                    .find(|member| member.identity.o == passenger_o)
                    .map(|member| canonical_identity(&member.identity))
                    .ok_or(EconomyPackageError::MissingSelectedMember(passenger_o))?;
                let ship_actor = authority
                    .binding(ship)
                    .and_then(|binding| match binding.stable {
                        WorldObjectIdentity::Unit { id, generation } => Some(UnitIdentity {
                            handle: crate::Handle { id, generation },
                            who: ship.owner,
                            o: ship.o,
                            uid: ship.uid,
                        }),
                        _ => None,
                    })
                    .ok_or(EconomyPackageError::MissingActionTarget)?;
                effects.push(PlannedEconomyEffect::InstallOrder {
                    actor: ship_actor,
                    queue: QUEUE_LAST,
                    node: node(
                        OrderIndex::AwaitBoard,
                        ORDER_GROUP,
                        0,
                        0,
                        stable_target(authority, passenger)?,
                        EconomyOrderPayload::TargetOnly,
                    )?,
                });
            }
            EconomyStep::RetireRepairTarget { .. } => {
                effects.push(PlannedEconomyEffect::RetireRepairTarget {
                    target: target.ok_or(EconomyPackageError::MissingActionTarget)?,
                });
            }
            EconomyStep::AddTradeOrder { .. } => {
                return Err(EconomyPackageError::TradeFrontierMismatch)
            }
        }
    }
    Ok(effects)
}

fn trade_group_snapshot(slot: usize, group: &GroupData) -> TradeGroupSnapshot {
    let count = group.num.clamp(0, GROUP_MAX_MEMBERS as i32) as usize;
    TradeGroupSnapshot {
        group_slot: slot as i32,
        id: group.id,
        owner: group.who,
        num: count as i32,
        form: group.form,
        disband: group.disband,
        members: group.list[..count].to_vec(),
    }
}

fn frontier_identity(
    authority: &EconomyRuntimeAuthority,
    identity: ObjectIdentity,
) -> Result<CanonicalObjectIdentity, EconomyPackageError> {
    authority
        .bindings
        .iter()
        .find(|binding| {
            i32::from(binding.identity.o) == identity.key.o
                && i32::from(binding.identity.owner) == identity.key.who
                && binding.identity.uid == identity.uid
        })
        .map(|binding| binding.identity)
        .ok_or(EconomyPackageError::TradeFrontierMismatch)
}

fn plan_trade_effects(
    selection: &PreparedGroupSelection,
    authority: &EconomyRuntimeAuthority,
    transaction: &GroupActionTradeTransaction,
) -> Result<Vec<PlannedEconomyEffect>, EconomyPackageError> {
    if transaction.group_after.authority != GroupAfterAuthority::Exact
        || !matches!(transaction.outcome, TradeOutcome::Completed { .. })
    {
        return Err(EconomyPackageError::QueueFirstUnsupported);
    }
    let mut effects = Vec::new();
    for event in &transaction.events {
        let TradeActionEvent::AddTradeOrder(install) = event else {
            continue;
        };
        if install.queue == QUEUE_FIRST {
            return Err(EconomyPackageError::QueueFirstUnsupported);
        }
        let actor_id = frontier_identity(authority, install.actor)?;
        let first = frontier_identity(authority, install.payload.first)?;
        let second = frontier_identity(authority, install.payload.second)?;
        if install
            .steps
            .iter()
            .any(|step| matches!(step, TradeInstallStep::CloseOrders { arg: 0 }))
        {
            effects.push(PlannedEconomyEffect::ClearOrders { actor: actor_id });
            effects.push(PlannedEconomyEffect::ClearPartialPath { actor: actor_id });
        }
        effects.push(PlannedEconomyEffect::SetUnitMasks {
            actor: actor_id,
            after: install.unit_flags_after_projection,
        });
        effects.push(PlannedEconomyEffect::InstallOrder {
            actor: selected_actor(selection, actor_id.o)?,
            queue: install.queue,
            node: node(
                OrderIndex::TradeRoute,
                install.payload.flags,
                0,
                0,
                stable_target(authority, first)?,
                EconomyOrderPayload::TradeRoute(TradeOrderPayload {
                    second: stable_target(authority, second)?,
                    started: install.payload.started,
                    loaded: install.payload.loaded,
                }),
            )?,
        });
    }
    Ok(effects)
}

fn unit_row_for_canonical(
    world: &World,
    authority: &EconomyRuntimeAuthority,
    identity: CanonicalObjectIdentity,
) -> Result<usize, EconomyPackageError> {
    let binding = authority
        .binding(identity)
        .ok_or(EconomyPackageError::MissingObjectBinding(identity))?;
    if !binding_is_current(world, binding) {
        return Err(EconomyPackageError::StaleObjectBinding(identity));
    }
    match binding.stable {
        WorldObjectIdentity::Unit { .. } => world
            .unit_row_at(i32::from(identity.owner), i32::from(identity.o))
            .ok_or(EconomyPackageError::StaleObjectBinding(identity)),
        _ => Err(EconomyPackageError::MissingActionTarget),
    }
}

fn stage_effects(
    world: &World,
    paths: &[PathStack],
    authority: &EconomyRuntimeAuthority,
    selection: &mut PreparedGroupSelection,
    plan: &crate::systems::economy_containment_group_host::CanonicalEconomyGroupPlan,
    effects: &[PlannedEconomyEffect],
) -> Result<(), EconomyPackageError> {
    selection.groups_after.list[selection.group_slot] = plan.group_after.clone();
    for effect in effects {
        match effect {
            PlannedEconomyEffect::InstallOrder { actor, queue, node } => {
                let row = world
                    .row_of(actor.handle)
                    .filter(|&row| {
                        world.units.get_who(row) == actor.who
                            && world.units.o()[row] == actor.o
                            && world.units.get_uid(row) == actor.uid
                    })
                    .ok_or(EconomyPackageError::StaleUnit(actor.handle))?;
                let index = ensure_mutation_for_row(&mut selection.units, world, paths, row)?;
                let mutation = &mut selection.units[index];
                let order = crate::order::Order::economy(*node)?;
                match *queue {
                    QUEUE_NEW => {
                        mutation.after.orders.replace(order);
                        mutation.after.path.clear();
                    }
                    QUEUE_LAST => mutation.after.orders.push_front(order),
                    QUEUE_FIRST => return Err(EconomyPackageError::QueueFirstUnsupported),
                    _ => return Err(EconomyPackageError::QueueFirstUnsupported),
                }
            }
            PlannedEconomyEffect::ClearOrders { actor } => {
                let row = unit_row_for_canonical(world, authority, *actor)?;
                let index = ensure_mutation_for_row(&mut selection.units, world, paths, row)?;
                selection.units[index].after.orders.clear();
            }
            PlannedEconomyEffect::ClearPartialPath { actor } => {
                let row = unit_row_for_canonical(world, authority, *actor)?;
                let index = ensure_mutation_for_row(&mut selection.units, world, paths, row)?;
                selection.units[index].after.path.clear();
            }
            PlannedEconomyEffect::SetUnitMasks { actor, after } => {
                let row = unit_row_for_canonical(world, authority, *actor)?;
                let index = ensure_mutation_for_row(&mut selection.units, world, paths, row)?;
                selection.units[index].after.unit_masks = *after;
            }
            PlannedEconomyEffect::RetireRepairTarget { target } => {
                let row = unit_row_for_canonical(world, authority, *target)?;
                let index = ensure_mutation_for_row(&mut selection.units, world, paths, row)?;
                let mutation = &mut selection.units[index];
                mutation.after.unit_masks &=
                    !crate::command::economy_group_actions::CAST_SPELL_ACTIVE_MASK;
                mutation.after.orders.clear();
                mutation.after.path.clear();
            }
        }
    }
    Ok(())
}

/// Prepare Group selection and one Board/Repair/Trade action without mutating any owner.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub fn prepare_economy_group_package(
    world: &World,
    groups: &Groups,
    paths: &[PathStack],
    command_state: &CommandPackageState,
    selection_authority: &GroupMoveAuthority,
    authority: &EconomyRuntimeAuthority,
    player_who: &[Option<u8>; NETWORK_PLAYERS],
    frame: i32,
    play: usize,
    lockstep_serial: i32,
    bytes: &[u8],
) -> Result<PreparedEconomyGroupPackage, EconomyPackageError> {
    if authority.composition_digest == [0; 32] {
        return Err(EconomyPackageError::MissingCompositionDigest);
    }
    if authority.actions.iter().enumerate().any(|(index, entry)| {
        authority.actions[..index]
            .iter()
            .any(|old| old.action == entry.action)
    }) {
        return Err(EconomyPackageError::DuplicateAuthorityAction);
    }
    let pair = decode_package(frame, play, lockstep_serial, bytes)?;
    if play >= NETWORK_PLAYERS {
        return Err(EconomyPackageError::PlayOutOfRange { play });
    }
    let expected = player_who[play].ok_or(EconomyPackageError::MissingPlayerMap { play })?;
    if expected != pair.group.owner {
        return Err(EconomyPackageError::PlayerOwnerMismatch {
            play,
            expected,
            got: pair.group.owner,
        });
    }
    let action_authority = authority
        .action(pair.action)
        .ok_or(EconomyPackageError::MissingAuthority)?;
    validate_fact_identities(world, authority, &action_authority.facts)?;

    let mut selection = prepare_group_selection(
        world,
        groups,
        paths,
        command_state,
        selection_authority,
        frame,
        play,
        pair.group.owner,
        &pair.group.requested,
        GroupSelectionUse::EconomyOrderInstall,
    )?;
    let state_before = selection_digest(
        world.digest(),
        &selection.groups_before,
        paths,
        &selection.command_state_before,
        authority.composition_digest,
    );
    let state_after_selection = selection_digest(
        world.digest(),
        &selection.groups_after,
        paths,
        &selection.command_state_after,
        authority.composition_digest,
    );
    let receipt = selection_receipt(
        pair.clone(),
        &selection,
        state_before,
        state_after_selection,
    )?;
    let request = EconomyGroupTransactionRequest::snapshot_from_selection(
        &selection.groups_after,
        receipt,
        authority.ignore_orders,
        state_after_selection,
    )
    .ok_or(EconomyPackageError::SelectionReceipt)?;
    let capabilities = EconomyOrderCapabilities {
        format_version: DON_SAVE_V13,
        typed_cast_v1: true,
        typed_trade_v1: true,
        // The current OrderList does not yet own set_up_insert/finish_insert chronology.
        exact_group_queue_first: false,
    };
    let plan = crate::systems::economy_containment_group_host::preflight_economy_group_transaction(
        &request,
        &action_authority.facts,
        capabilities,
    )
    .map_err(EconomyPackageError::Preflight)?;

    let (trade, effects) = match pair.action {
        EconomyGroupAction::Trade {
            first_o,
            first_owner,
            second_o,
            second_owner,
            queued,
        } => {
            let facts = action_authority
                .trade
                .clone()
                .ok_or(EconomyPackageError::MissingTradeFrontier)?;
            let transaction = plan_group_action_trade(
                GroupActionTradeRequest {
                    group: trade_group_snapshot(selection.group_slot, request.group_before()),
                    action: RawTradeAction {
                        ox: first_o,
                        whom: first_owner,
                        oxx: second_o,
                        whose: second_owner,
                        queued,
                    },
                },
                facts,
            )
            .map_err(EconomyPackageError::TradeFrontier)?;
            let after = &transaction.group_after.last_known;
            let count = plan.group_after.num.clamp(0, GROUP_MAX_MEMBERS as i32) as usize;
            if after.id != plan.group_after.id
                || after.owner != plan.group_after.who
                || after.num != plan.group_after.num
                || after.form != plan.group_after.form
                || after.disband != plan.group_after.disband
                || after.members != plan.group_after.list[..count]
            {
                return Err(EconomyPackageError::TradeFrontierMismatch);
            }
            let effects = plan_trade_effects(&selection, authority, &transaction)?;
            (Some(transaction), effects)
        }
        EconomyGroupAction::BoardShip { .. } | EconomyGroupAction::Repair { .. } => {
            let steps = plan
                .action_plan
                .as_ref()
                .map_or(&[][..], |action| action.steps.as_slice());
            (
                None,
                plan_nontrade_effects(&selection, authority, &action_authority.facts, steps)?,
            )
        }
    };

    stage_effects(world, paths, authority, &mut selection, &plan, &effects)?;

    Ok(PreparedEconomyGroupPackage {
        play,
        lockstep_serial,
        frame,
        random_state: world.random.state(),
        pair,
        selection,
        request,
        plan,
        trade,
        effects,
        authority_revision: authority.revision,
        authority_digest: authority.composition_digest,
        authority_snapshot: authority.clone(),
    })
}

/// Revalidate every fixed owner and publish Group selection plus the economy action as one
/// assignment-only transaction.
pub fn commit_economy_group_package(
    world: &mut World,
    groups: &mut Groups,
    paths: &mut [PathStack],
    command_state: &mut CommandPackageState,
    selection_authority: &GroupMoveAuthority,
    authority: &EconomyRuntimeAuthority,
    prepared: PreparedEconomyGroupPackage,
) -> Result<EconomyGroupPackageReceipt, EconomyPackageError> {
    if command_state != &prepared.selection.command_state_before {
        return Err(EconomyPackageError::StaleCommandState);
    }
    if !groups_equal(groups, &prepared.selection.groups_before) {
        return Err(EconomyPackageError::StaleGroups);
    }
    if selection_authority.revision != prepared.selection.authority_revision
        || selection_authority.composition_digest != prepared.selection.authority_digest
        || selection_authority.members != prepared.selection.authority_members
    {
        return Err(EconomyPackageError::StaleSelectionAuthority);
    }
    if authority.revision != prepared.authority_revision
        || authority.composition_digest != prepared.authority_digest
        || authority != &prepared.authority_snapshot
    {
        return Err(EconomyPackageError::StaleEconomyAuthority);
    }
    let action_authority = authority
        .action(prepared.pair.action)
        .ok_or(EconomyPackageError::MissingAuthority)?;
    validate_fact_identities(world, authority, &action_authority.facts)?;
    if world.random.state() != prepared.random_state {
        return Err(EconomyPackageError::StaleRandomState);
    }
    for mutation in &prepared.selection.units {
        if !unit_still_current(world, paths, &mutation.before) {
            return Err(EconomyPackageError::StaleUnit(
                mutation.before.identity.handle,
            ));
        }
    }

    let mut checksum = CheckSum::default();
    prepared.selection.groups_after.check_groups(&mut checksum);
    let receipt = EconomyGroupPackageReceipt {
        play: prepared.play,
        lockstep_serial: prepared.lockstep_serial,
        frame: prepared.frame,
        opcode: prepared.pair.action.opcode(),
        who: prepared.pair.group.owner,
        group_slot: prepared.selection.group_slot,
        installed_orders: prepared
            .effects
            .iter()
            .filter(|effect| matches!(effect, PlannedEconomyEffect::InstallOrder { .. }))
            .count(),
        command_state_revision: prepared.selection.command_state_after.revision(),
        groups_checksum: checksum.value,
        random_state_before: prepared.random_state,
        random_state_after: prepared.random_state,
    };

    *groups = prepared.selection.groups_after;
    *command_state = prepared.selection.command_state_after;
    for mutation in prepared.selection.units {
        let row = world
            .row_of(mutation.before.identity.handle)
            .expect("all economy identities were revalidated before publication");
        world.units.group_mut()[row] = mutation.after.group;
        world.units.set_unit_masks(row, mutation.after.unit_masks);
        world.units.orders_x_mut()[row] = mutation.after.orders_x;
        world.units.orders_y_mut()[row] = mutation.after.orders_y;
        *world.orders_mut(row) = mutation.after.orders;
        paths[row] = mutation.after.path;
    }
    Ok(receipt)
}
