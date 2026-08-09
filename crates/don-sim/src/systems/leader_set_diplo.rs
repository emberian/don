// SPDX-License-Identifier: GPL-3.0-or-later
//! Atomic plan for `Leader::set_diplo` (`0x006EC6A0`) and its resource-owning callers.
//!
//! This module deliberately has no dependency on the command dispatcher or the live `World`.
//! It is a transaction contract: a host supplies every fact read by the retail body, the
//! planner emits instruction-ordered simulation calls and typed presentation envelopes, and an
//! applied receipt is valid only when every authoritative call is acknowledged.  Missing facts
//! fail before a mutable after-image is returned.

pub const DIPLO_SLOTS: usize = 8;
pub const NUM_GOODS: usize = 6;
pub const ARMY_SLOTS: usize = 16;
pub const SHARED_VISION_PREQ: i32 = 0x2b0;
pub const AIR_PATROL_ORDER: i32 = 0x136;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum Relation {
    War = 0,
    Peace = 1,
    Ally = 2,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EjectionUnitFact {
    /// Stable object-array identity.  Entries must be in retail owner-list order.
    pub object_id: i32,
    /// Result of the object's `vt+0x08` unit query.
    pub is_unit: bool,
    /// `ObjectData::get_inside`; `None` means the object is not contained.
    pub carrier_who: Option<usize>,
    /// `Unit::come_out(0)` return value. Required only when `carrier_who` is the revoked ally.
    /// Retail treats zero as success and nonzero as failure.
    pub come_out_return: Option<i32>,
    /// `UnitTypeData::domain` (`+0x218`). Required after a successful `come_out`.
    pub domain: Option<i32>,
    /// Result of the virtual order query `(AIR_PATROL_ORDER, 0)`. Required only for domain 2.
    pub has_air_patrol_order: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LeaderSetDiploFacts {
    /// Retail invariant: the leader at slot `i` has `LeaderData::who == i`.
    pub who: Option<i32>,
    pub leader_flags: Option<u32>,
    pub leader_flags2: Option<u32>,
    pub diplos: [Option<Relation>; DIPLO_SLOTS],
    /// Byte at `LeaderData+0x6929`; one bit per leader.
    pub shared_vision: Option<u8>,
    /// Exact result of `has_preq(SHARED_VISION_PREQ)`.
    pub has_shared_vision_preq: Option<bool>,
    /// Full owner object-list image used by `eject_my_shit_from_his_ass`.
    pub ejection_units: Option<Vec<EjectionUnitFact>>,
    /// Valid bit for each preallocated army, in slot order.
    pub valid_armies: Option<[bool; ARMY_SLOTS]>,
}

impl Default for LeaderSetDiploFacts {
    fn default() -> Self {
        Self {
            who: None,
            leader_flags: None,
            leader_flags2: None,
            diplos: [None; DIPLO_SLOTS],
            shared_vision: None,
            has_shared_vision_preq: None,
            ejection_units: None,
            valid_armies: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetDiploImage {
    pub leaders: [LeaderSetDiploFacts; DIPLO_SLOTS],
    /// `Console+0x298`. `-1` is a valid no-local-player value.
    pub local_who: Option<i32>,
    /// `get_scary_console_leader()->has_treaty(who, 1)`, not either party's own predicate.
    pub console_treaty_one: [Option<bool>; DIPLO_SLOTS],
    /// `GameData+0x30 >= 1`, the fallback after `has_preq(0x2b0)`.
    pub global_shared_vision: Option<bool>,
    /// `IFaceData+0x22a`.
    pub interface_dirty: bool,
    /// The game victory mask containing retail bit 22.
    pub victory_mask: u32,
}

impl Default for SetDiploImage {
    fn default() -> Self {
        Self {
            leaders: std::array::from_fn(|_| LeaderSetDiploFacts::default()),
            local_who: None,
            console_treaty_one: [None; DIPLO_SLOTS],
            global_shared_vision: None,
            interface_dirty: false,
            victory_mask: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SetDiploRequest {
    pub actor: usize,
    pub target: usize,
    pub state: Relation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MissingEjectionFact {
    ComeOutReturn,
    Domain,
    AirPatrolOrder,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetDiploPlanError {
    LeaderOutOfRange {
        value: usize,
    },
    MissingIdentity {
        slot: usize,
    },
    LeaderIdentityMismatch {
        slot: usize,
        who: i32,
    },
    MissingRelation {
        from: usize,
        to: usize,
    },
    MissingLocalWho,
    MissingConsoleTreaty {
        who: usize,
    },
    MissingSharedVision {
        who: usize,
    },
    MissingSharedVisionPreq {
        who: usize,
    },
    MissingGlobalSharedVision,
    MissingLeaderFlags {
        who: usize,
    },
    MissingLeaderFlags2 {
        who: usize,
    },
    MissingEjectionRoster {
        owner: usize,
    },
    MissingEjectionFact {
        owner: usize,
        object_id: i32,
        fact: MissingEjectionFact,
    },
    MissingArmyRoster {
        owner: usize,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ForeignRelationKind {
    War,
    Peace,
    Alliance,
    AllianceBroken,
}

/// Presentation is intentionally not a simulation mutation.  A renderer may localize these
/// envelopes after the lockstep transaction commits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetDiploPresentation {
    ForeignRelation {
        actor: usize,
        target: usize,
        kind: ForeignRelationKind,
    },
    Sound {
        id: i32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetDiploMutation {
    ClearSharedVision {
        viewer: usize,
        source: usize,
    },
    WriteDeclaration {
        from: usize,
        to: usize,
        state: Relation,
    },
    GrantSharedVision {
        viewer: usize,
        source: usize,
    },
    SetVictoryBit22,
    MarkInterfaceDirty,
}

/// Calls whose complete mutation belongs to another authoritative subsystem.  They are still
/// instruction ordered and mandatory: an applied receipt must acknowledge every one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetDiploAuthority {
    ComeOut {
        owner: usize,
        object_id: i32,
    },
    KillContainedUnit {
        owner: usize,
        object_id: i32,
        reason: i32,
    },
    AddAirStrafeOrder {
        owner: usize,
        object_id: i32,
        queue_pos: i32,
    },
    Victory {
        winner: usize,
        victory_type: i32,
        instant: i32,
    },
    ForceArmyProcess {
        owner: usize,
        army_slot: usize,
        forced: i32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetDiploStep {
    Mutation(SetDiploMutation),
    Authority(SetDiploAuthority),
    Presentation(SetDiploPresentation),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetDiploPlan {
    pub request: SetDiploRequest,
    pub old_state: Relation,
    pub after: SetDiploImage,
    pub steps: Vec<SetDiploStep>,
}

fn check_slot(slot: usize) -> Result<(), SetDiploPlanError> {
    if slot < DIPLO_SLOTS {
        Ok(())
    } else {
        Err(SetDiploPlanError::LeaderOutOfRange { value: slot })
    }
}

fn identity(image: &SetDiploImage, slot: usize) -> Result<(), SetDiploPlanError> {
    let who = image.leaders[slot]
        .who
        .ok_or(SetDiploPlanError::MissingIdentity { slot })?;
    if who != slot as i32 {
        return Err(SetDiploPlanError::LeaderIdentityMismatch { slot, who });
    }
    Ok(())
}

fn relation(image: &SetDiploImage, from: usize, to: usize) -> Result<Relation, SetDiploPlanError> {
    if from == to {
        return Ok(Relation::Ally);
    }
    image.leaders[from].diplos[to].ok_or(SetDiploPlanError::MissingRelation { from, to })
}

fn is_ally(image: &SetDiploImage, first: usize, second: usize) -> Result<bool, SetDiploPlanError> {
    if first == second {
        return Ok(true);
    }
    Ok(relation(image, first, second)? == Relation::Ally
        && relation(image, second, first)? == Relation::Ally)
}

fn set_vision(
    image: &mut SetDiploImage,
    viewer: usize,
    source: usize,
    enabled: bool,
) -> Result<(), SetDiploPlanError> {
    let byte = image.leaders[viewer]
        .shared_vision
        .as_mut()
        .ok_or(SetDiploPlanError::MissingSharedVision { who: viewer })?;
    if enabled {
        *byte |= 1u8 << source;
    } else {
        *byte &= !(1u8 << source);
    }
    Ok(())
}

fn plan_ejection(
    image: &SetDiploImage,
    owner: usize,
    revoked_carrier: usize,
    steps: &mut Vec<SetDiploStep>,
) -> Result<(), SetDiploPlanError> {
    let units = image.leaders[owner]
        .ejection_units
        .as_ref()
        .ok_or(SetDiploPlanError::MissingEjectionRoster { owner })?;
    for unit in units {
        if !unit.is_unit || unit.carrier_who != Some(revoked_carrier) {
            continue;
        }
        let missing = |fact| SetDiploPlanError::MissingEjectionFact {
            owner,
            object_id: unit.object_id,
            fact,
        };
        let come_out = unit
            .come_out_return
            .ok_or_else(|| missing(MissingEjectionFact::ComeOutReturn))?;
        steps.push(SetDiploStep::Authority(SetDiploAuthority::ComeOut {
            owner,
            object_id: unit.object_id,
        }));
        if come_out != 0 {
            steps.push(SetDiploStep::Authority(
                SetDiploAuthority::KillContainedUnit {
                    owner,
                    object_id: unit.object_id,
                    reason: 0,
                },
            ));
            continue;
        }
        let domain = unit
            .domain
            .ok_or_else(|| missing(MissingEjectionFact::Domain))?;
        if domain == 2 {
            let has_order = unit
                .has_air_patrol_order
                .ok_or_else(|| missing(MissingEjectionFact::AirPatrolOrder))?;
            if !has_order {
                steps.push(SetDiploStep::Authority(
                    SetDiploAuthority::AddAirStrafeOrder {
                        owner,
                        object_id: unit.object_id,
                        queue_pos: 2,
                    },
                ));
            }
        }
    }
    Ok(())
}

fn grant_vision_if_enabled(
    image: &mut SetDiploImage,
    viewer: usize,
    source: usize,
    steps: &mut Vec<SetDiploStep>,
) -> Result<(), SetDiploPlanError> {
    let preq = image.leaders[viewer]
        .has_shared_vision_preq
        .ok_or(SetDiploPlanError::MissingSharedVisionPreq { who: viewer })?;
    let enabled = if preq {
        true
    } else {
        image
            .global_shared_vision
            .ok_or(SetDiploPlanError::MissingGlobalSharedVision)?
    };
    if enabled {
        set_vision(image, viewer, source, true)?;
        steps.push(SetDiploStep::Mutation(
            SetDiploMutation::GrantSharedVision { viewer, source },
        ));
    }
    Ok(())
}

fn owner_armies_enabled(image: &SetDiploImage, who: usize) -> Result<bool, SetDiploPlanError> {
    let flags = image.leaders[who]
        .leader_flags
        .ok_or(SetDiploPlanError::MissingLeaderFlags { who })?;
    if flags & 1 == 0 || flags & 0x0c == 4 {
        return Ok(false);
    }
    let flags2 = image.leaders[who]
        .leader_flags2
        .ok_or(SetDiploPlanError::MissingLeaderFlags2 { who })?;
    Ok(flags2 & 0x0a == 0)
}

/// Plan the exact `Leader::set_diplo` body.  The returned after-image contains only the fields
/// owned directly by that body; unit, victory, and army calls remain mandatory authority steps.
pub fn plan_set_diplo(
    before: &SetDiploImage,
    request: SetDiploRequest,
) -> Result<SetDiploPlan, SetDiploPlanError> {
    check_slot(request.actor)?;
    check_slot(request.target)?;
    identity(before, request.actor)?;
    identity(before, request.target)?;
    let old_state = relation(before, request.actor, request.target)?;
    let mut after = before.clone();
    let mut steps = Vec::new();

    // 0x006EC6BB: equal raw declarations return before every side effect.
    if old_state == request.state {
        return Ok(SetDiploPlan {
            request,
            old_state,
            after,
            steps,
        });
    }

    if old_state == Relation::Ally {
        set_vision(&mut after, request.actor, request.target, false)?;
        steps.push(SetDiploStep::Mutation(
            SetDiploMutation::ClearSharedVision {
                viewer: request.actor,
                source: request.target,
            },
        ));
        set_vision(&mut after, request.target, request.actor, false)?;
        steps.push(SetDiploStep::Mutation(
            SetDiploMutation::ClearSharedVision {
                viewer: request.target,
                source: request.actor,
            },
        ));
        plan_ejection(before, request.actor, request.target, &mut steps)?;
        plan_ejection(before, request.target, request.actor, &mut steps)?;
    }

    let local = before.local_who.ok_or(SetDiploPlanError::MissingLocalWho)?;
    if local != request.actor as i32 && local != request.target as i32 {
        let actor_treaty = before.console_treaty_one[request.actor]
            .ok_or(SetDiploPlanError::MissingConsoleTreaty { who: request.actor })?;
        if actor_treaty {
            let target_treaty = before.console_treaty_one[request.target].ok_or(
                SetDiploPlanError::MissingConsoleTreaty {
                    who: request.target,
                },
            )?;
            if target_treaty {
                let kind = match (request.state, old_state) {
                    (Relation::War, _) => ForeignRelationKind::War,
                    (Relation::Peace, Relation::Ally) => ForeignRelationKind::AllianceBroken,
                    (Relation::Peace, _) => ForeignRelationKind::Peace,
                    (Relation::Ally, _) => ForeignRelationKind::Alliance,
                };
                steps.push(SetDiploStep::Presentation(
                    SetDiploPresentation::ForeignRelation {
                        actor: request.actor,
                        target: request.target,
                        kind,
                    },
                ));
                steps.push(SetDiploStep::Presentation(SetDiploPresentation::Sound {
                    id: 0x135,
                }));
            }
        }
    }

    // These are two distinct writes: self's row first, then the global target row.
    after.leaders[request.actor].diplos[request.target] = Some(request.state);
    steps.push(SetDiploStep::Mutation(SetDiploMutation::WriteDeclaration {
        from: request.actor,
        to: request.target,
        state: request.state,
    }));
    after.leaders[request.target].diplos[request.actor] = Some(request.state);
    steps.push(SetDiploStep::Mutation(SetDiploMutation::WriteDeclaration {
        from: request.target,
        to: request.actor,
        state: request.state,
    }));

    if request.state == Relation::Ally {
        grant_vision_if_enabled(&mut after, request.actor, request.target, &mut steps)?;
        grant_vision_if_enabled(&mut after, request.target, request.actor, &mut steps)?;

        let mut independent_active = 0usize;
        for candidate in 0..DIPLO_SLOTS {
            if candidate == request.actor || candidate == request.target {
                continue;
            }
            let flags = after.leaders[candidate]
                .leader_flags
                .ok_or(SetDiploPlanError::MissingLeaderFlags { who: candidate })?;
            if flags & 3 == 3 {
                // Both `is_ally` calls use the candidate record's `who` field.
                identity(&after, candidate)?;
                if !is_ally(&after, candidate, request.actor)?
                    && !is_ally(&after, candidate, request.target)?
                {
                    independent_active += 1;
                }
            }
        }
        if independent_active == 0 {
            steps.push(SetDiploStep::Authority(SetDiploAuthority::Victory {
                winner: request.actor,
                victory_type: 0,
                instant: 0,
            }));
            after.victory_mask |= 1u32 << 22;
            steps.push(SetDiploStep::Mutation(SetDiploMutation::SetVictoryBit22));
        }
    }

    if owner_armies_enabled(&after, request.actor)? {
        let armies = after.leaders[request.actor].valid_armies.ok_or(
            SetDiploPlanError::MissingArmyRoster {
                owner: request.actor,
            },
        )?;
        for (army_slot, valid) in armies.into_iter().enumerate() {
            if valid {
                steps.push(SetDiploStep::Authority(
                    SetDiploAuthority::ForceArmyProcess {
                        owner: request.actor,
                        army_slot,
                        forced: 1,
                    },
                ));
            }
        }
    }

    after.interface_dirty = true;
    steps.push(SetDiploStep::Mutation(SetDiploMutation::MarkInterfaceDirty));
    Ok(SetDiploPlan {
        request,
        old_state,
        after,
        steps,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetDiploTransactionStatus {
    Applied,
    Unavailable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetDiploTransactionRequest {
    pub before: SetDiploImage,
    pub change: SetDiploRequest,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetDiploReceipt {
    pub request: SetDiploTransactionRequest,
    pub status: SetDiploTransactionStatus,
    pub plan: Option<SetDiploPlan>,
    /// Exact authority calls completed by the host, in order. Presentation is deliberately absent.
    pub authority: Vec<SetDiploAuthority>,
}

impl SetDiploReceipt {
    pub fn unavailable(request: SetDiploTransactionRequest) -> Self {
        Self {
            request,
            status: SetDiploTransactionStatus::Unavailable,
            plan: None,
            authority: Vec::new(),
        }
    }

    pub fn validates(&self, expected: &SetDiploTransactionRequest) -> bool {
        if &self.request != expected {
            return false;
        }
        let planned = plan_set_diplo(&expected.before, expected.change);
        match self.status {
            SetDiploTransactionStatus::Unavailable => {
                self.plan.is_none() && self.authority.is_empty() && planned.is_ok()
            }
            SetDiploTransactionStatus::Applied => {
                let Ok(plan) = planned else {
                    return false;
                };
                let required: Vec<_> = plan
                    .steps
                    .iter()
                    .filter_map(|step| match step {
                        SetDiploStep::Authority(call) => Some(*call),
                        _ => None,
                    })
                    .collect();
                self.plan.as_ref() == Some(&plan) && self.authority == required
            }
        }
    }
}

// ---------------------------------------------------------------------------
// `action_respond(..., 1)` resource movement used by opcode 41.
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DealResourceLeader {
    pub buckets: [i32; NUM_GOODS],
    /// Six dwords at `LeaderData+0x498`, holding resources reserved by the pending deal.
    pub escrow: [i32; NUM_GOODS],
    pub sent_raw: i32,
    pub received_scaled: i32,
    pub tribute_scale_percent: Option<i32>,
    pub type_available: [Option<bool>; NUM_GOODS],
}

impl Default for DealResourceLeader {
    fn default() -> Self {
        Self {
            buckets: [0; NUM_GOODS],
            escrow: [0; NUM_GOODS],
            sent_raw: 0,
            received_scaled: 0,
            tribute_scale_percent: None,
            type_available: [None; NUM_GOODS],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AcceptedDealResources {
    pub leaders: [DealResourceLeader; DIPLO_SLOTS],
    /// Directional pair record: `offers[from][to][good]`.
    pub offers: [[[i32; NUM_GOODS]; DIPLO_SLOTS]; DIPLO_SLOTS],
    /// Directional `Diplomacy+0x24` array.
    pub declaration_costs: [[[i32; NUM_GOODS]; DIPLO_SLOTS]; DIPLO_SLOTS],
}

impl Default for AcceptedDealResources {
    fn default() -> Self {
        Self {
            leaders: std::array::from_fn(|_| DealResourceLeader::default()),
            offers: [[[0; NUM_GOODS]; DIPLO_SLOTS]; DIPLO_SLOTS],
            declaration_costs: [[[0; NUM_GOODS]; DIPLO_SLOTS]; DIPLO_SLOTS],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DealResourcePlanError {
    LeaderOutOfRange { value: usize },
    MissingTypeAvailability { who: usize, good: usize },
    MissingTributeScale { who: usize },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AcceptedDealResourceStep {
    RefundDeclarationCost {
        who: usize,
        good: usize,
        amount: i32,
    },
    ClearDeclarationCost {
        from: usize,
        to: usize,
        good: usize,
    },
    CreditScaledTribute {
        from: usize,
        to: usize,
        good: usize,
        raw: i32,
        scaled: i32,
    },
    DebitEscrow {
        who: usize,
        good: usize,
        raw: i32,
    },
    RecordSentRaw {
        who: usize,
        raw: i32,
    },
    RecordReceivedScaled {
        who: usize,
        scaled: i32,
    },
    ConsiderTribute {
        receiver: usize,
        sender: usize,
        good: usize,
        raw: i32,
    },
    ClampEscrow {
        who: usize,
        good: usize,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AcceptedDealResourcePlan {
    pub accepter: usize,
    pub proposer: usize,
    pub after: AcceptedDealResources,
    pub steps: Vec<AcceptedDealResourceStep>,
}

fn scale_tribute(raw: i32, percent: i32) -> i32 {
    let percent = percent.clamp(1, 100);
    if percent == 100 {
        return raw;
    }
    let product = raw.wrapping_mul(percent);
    if raw < 20 {
        product / 100
    } else if raw < 100 {
        product.wrapping_add(50) / 100
    } else {
        product.wrapping_add(99) / 100
    }
}

fn transfer_one_direction(
    state: &mut AcceptedDealResources,
    from: usize,
    to: usize,
    good: usize,
    raw: i32,
    steps: &mut Vec<AcceptedDealResourceStep>,
) -> Result<(), DealResourcePlanError> {
    let percent = state.leaders[from]
        .tribute_scale_percent
        .ok_or(DealResourcePlanError::MissingTributeScale { who: from })?;
    let scaled = scale_tribute(raw, percent);
    state.leaders[to].buckets[good] = state.leaders[to].buckets[good].wrapping_add(scaled);
    steps.push(AcceptedDealResourceStep::CreditScaledTribute {
        from,
        to,
        good,
        raw,
        scaled,
    });
    state.leaders[from].escrow[good] = state.leaders[from].escrow[good].wrapping_sub(raw);
    steps.push(AcceptedDealResourceStep::DebitEscrow {
        who: from,
        good,
        raw,
    });
    state.leaders[from].sent_raw = state.leaders[from].sent_raw.wrapping_add(raw);
    steps.push(AcceptedDealResourceStep::RecordSentRaw { who: from, raw });
    state.leaders[to].received_scaled = state.leaders[to].received_scaled.wrapping_add(scaled);
    steps.push(AcceptedDealResourceStep::RecordReceivedScaled { who: to, scaled });
    steps.push(AcceptedDealResourceStep::ConsiderTribute {
        receiver: to,
        sender: from,
        good,
        raw,
    });
    Ok(())
}

/// Reproduce the resource loop at `0x006D0E36..0x006D0FFA`.
///
/// For every good retail first refunds both declaration-cost reservations and clears their pair
/// fields. If the good is available to both leaders it then transfers proposer -> accepter before
/// accepter -> proposer; each direction credits the scaled receiver before debiting raw escrow.
pub fn plan_accepted_deal_resources(
    before: &AcceptedDealResources,
    accepter: usize,
    proposer: usize,
) -> Result<AcceptedDealResourcePlan, DealResourcePlanError> {
    if accepter >= DIPLO_SLOTS {
        return Err(DealResourcePlanError::LeaderOutOfRange { value: accepter });
    }
    if proposer >= DIPLO_SLOTS {
        return Err(DealResourcePlanError::LeaderOutOfRange { value: proposer });
    }
    let mut after = before.clone();
    let mut steps = Vec::new();
    for good in 0..NUM_GOODS {
        // Retail performs both bucket credits before clearing either declaration-cost field.
        for (from, to) in [(accepter, proposer), (proposer, accepter)] {
            let amount = after.declaration_costs[from][to][good];
            after.leaders[from].buckets[good] =
                after.leaders[from].buckets[good].wrapping_add(amount);
            steps.push(AcceptedDealResourceStep::RefundDeclarationCost {
                who: from,
                good,
                amount,
            });
        }
        for (from, to) in [(accepter, proposer), (proposer, accepter)] {
            after.declaration_costs[from][to][good] = 0;
            steps.push(AcceptedDealResourceStep::ClearDeclarationCost { from, to, good });
        }

        let accepter_available = after.leaders[accepter].type_available[good].ok_or(
            DealResourcePlanError::MissingTypeAvailability {
                who: accepter,
                good,
            },
        )?;
        if !accepter_available {
            continue;
        }
        let proposer_available = after.leaders[proposer].type_available[good].ok_or(
            DealResourcePlanError::MissingTypeAvailability {
                who: proposer,
                good,
            },
        )?;
        if proposer_available {
            let proposer_raw = after.offers[proposer][accepter][good].max(0);
            let accepter_raw = after.offers[accepter][proposer][good].max(0);
            transfer_one_direction(
                &mut after,
                proposer,
                accepter,
                good,
                proposer_raw,
                &mut steps,
            )?;
            transfer_one_direction(
                &mut after,
                accepter,
                proposer,
                good,
                accepter_raw,
                &mut steps,
            )?;
            for who in [accepter, proposer] {
                if after.leaders[who].escrow[good] < 0 {
                    after.leaders[who].escrow[good] = 0;
                }
                steps.push(AcceptedDealResourceStep::ClampEscrow { who, good });
            }
        }
    }
    Ok(AcceptedDealResourcePlan {
        accepter,
        proposer,
        after,
        steps,
    })
}

// ---------------------------------------------------------------------------
// `Leader::pay_dow` resource debit used by opcode 38.
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeclarationPaymentImage {
    /// Encoded `Resources` values at the `0x00E41248` family, decoded for the planner.
    pub authoritative_buckets: [i32; NUM_GOODS],
    /// Mirrored `LeaderData+0x468` values (`0x00E3A7F8` family).
    pub leader_buckets: [i32; NUM_GOODS],
    pub type_available: [Option<bool>; NUM_GOODS],
    /// Result of the DOW type's virtual cost query for each good.
    pub costs: [Option<i32>; NUM_GOODS],
}

impl Default for DeclarationPaymentImage {
    fn default() -> Self {
        Self {
            authoritative_buckets: [0; NUM_GOODS],
            leader_buckets: [0; NUM_GOODS],
            type_available: [None; NUM_GOODS],
            costs: [None; NUM_GOODS],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeclarationPaymentError {
    MissingTypeAvailability { good: usize },
    MissingCost { good: usize },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeclarationPaymentStep {
    DebitAuthoritativeBucket {
        payment: usize,
        good: usize,
        cost: i32,
    },
    DebitLeaderBucket {
        payment: usize,
        good: usize,
        cost: i32,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeclarationPaymentPlan {
    pub payments: usize,
    pub after: DeclarationPaymentImage,
    pub steps: Vec<DeclarationPaymentStep>,
}

/// Reproduce `Leader::pay_dow` (`0x006D2B10`) once or twice.
///
/// `action_declare` calls it once before all declaration statistics and `set_diplo`; the retail
/// allied-to-war arm calls it a second time immediately after the first. Each payment walks goods
/// 0..5 and debits/clamps the authoritative bucket before the `LeaderData` mirror.
pub fn plan_declaration_payments(
    before: &DeclarationPaymentImage,
    payments: usize,
) -> Result<DeclarationPaymentPlan, DeclarationPaymentError> {
    let mut after = before.clone();
    let mut steps = Vec::new();
    for payment in 0..payments {
        for good in 0..NUM_GOODS {
            let available = after.type_available[good]
                .ok_or(DeclarationPaymentError::MissingTypeAvailability { good })?;
            if !available {
                continue;
            }
            let cost = after.costs[good].ok_or(DeclarationPaymentError::MissingCost { good })?;
            after.authoritative_buckets[good] =
                after.authoritative_buckets[good].wrapping_sub(cost).max(0);
            steps.push(DeclarationPaymentStep::DebitAuthoritativeBucket {
                payment,
                good,
                cost,
            });
            after.leader_buckets[good] = after.leader_buckets[good].wrapping_sub(cost).max(0);
            steps.push(DeclarationPaymentStep::DebitLeaderBucket {
                payment,
                good,
                cost,
            });
        }
    }
    Ok(DeclarationPaymentPlan {
        payments,
        after,
        steps,
    })
}
