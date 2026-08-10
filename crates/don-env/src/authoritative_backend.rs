//! Side-by-side fidelity backend over [`AuthoritativeEpisode`].
//!
//! This is deliberately narrower than `VecEnv`: it proves that policy actions,
//! observations, rewards, and ticks can share one [`don_sim::tick::Sim`] owner without
//! replacing the compact high-throughput backend in the same change. Every generated verb
//! has a frozen route below. Only `MOVE_TO` is admitted today, and only when the live
//! collision host proves every active object needed by the transaction. `ATTACK` has a
//! production Sim issue/execution route. A complete current-frame capture now binds its policy
//! ordinal through retail's cloak/detection/fog predicate and revalidates `(Handle,who,o,uid)`.
//! The prepared transaction and walked order now retain that identity, visibility revision,
//! hostile eligibility, and an exact proof of the current executor's optional combat inputs.
//! ATTACK remains masked because Step-12 freshness and that proof are not yet atomically consumed
//! by the production tick. Unsupported verbs fail before mutation; there is no accepted-no-effect
//! result.

use crate::authoritative_episode::{AuthoritativeEpisode, EpisodeError, ScenarioSpec, StepReceipt};
use don_sim::order::{Order, OrderIndex, OrderTargetIdentity, ORDER_FLEEING};
use don_sim::systems::external_entity_visibility_frontier::{
    ExternalEntityIdentity, ExternalEntityPublicState, ExternalEntityVisibilityOwner,
    ExternalUnitFrameRow, ExternalVisibilityFrame, RetailUnitVisibilityFacts, RetailViewerFacts,
    VisibilityInstallFault, VisibilityProjectionFault, VisibleExternalEntity, VisibleTargetBinding,
    UNIT_MASK_DETECTION_BYPASS,
};
use don_sim::systems::map_terrain::{Coord, FCoord, WCoord};
use don_sim::systems::movement_live::{
    LiveCollisionFault, LiveCollisionSource, MovementSourceState,
};
use don_sim::systems::victory_score::{leader_flag, Diplo};
use don_sim::world::{UnitTypeStats, OBJ_FLAG_ACTIVE, SUBTILE};
use don_sim::Handle;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

pub const UNIT_VERB_COUNT: usize = 33;
pub const PLAYER_VERB_COUNT: usize = 16;
pub const MOVE_TO_VERB_INDEX: usize = 5;

/// Missing authoritative owner which keeps a generated verb out of the fidelity backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntegrationBoundary {
    GroupCommandHost,
    FormationHost,
    CombatTargetHost,
    MovementCommandHost,
    PatrolAirframeHost,
    TransportContainmentHost,
    GatheringHost,
    ConstructionHost,
    ProductionHost,
    SpellHost,
    DiplomacyHost,
    MarketHost,
    TributeProposalHost,
    TerminalLifecycleHost,
    LeaderOptionsHost,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerbRoute {
    /// Single selected unit -> Sim-owned action-state CAS -> `Sim::issue` -> retail-ordered
    /// `Sim::do_frame`.
    SimIssue,
    /// The production Sim owns attack order installation and execution. Policy entry can cross
    /// exact target identity/visibility preflight, but remains fail-closed at target commit.
    SimAttackIssue,
    Refused(IntegrationBoundary),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VerbIntegration {
    pub name: &'static str,
    pub opcode: u8,
    pub route: VerbRoute,
}

macro_rules! refused {
    ($name:literal, $opcode:literal, $boundary:ident) => {
        VerbIntegration {
            name: $name,
            opcode: $opcode,
            route: VerbRoute::Refused(IntegrationBoundary::$boundary),
        }
    };
}

/// Index-aligned with `generated::UNIT_VERBS`; the contract test freezes names/opcodes.
pub const UNIT_INTEGRATION: [VerbIntegration; UNIT_VERB_COUNT] = [
    refused!("STANCE", 2, GroupCommandHost),
    refused!("FORM", 3, FormationHost),
    VerbIntegration {
        name: "ATTACK",
        opcode: 4,
        route: VerbRoute::SimAttackIssue,
    },
    refused!("SIEGE_ATTACK", 5, CombatTargetHost),
    refused!("SWARM_AROUND", 6, FormationHost),
    VerbIntegration {
        name: "MOVE_TO",
        opcode: 7,
        route: VerbRoute::SimIssue,
    },
    refused!("MOVE_NEAR", 8, MovementCommandHost),
    refused!("ATTACK_GROUND", 9, CombatTargetHost),
    refused!("PATROL", 10, PatrolAirframeHost),
    refused!("LAUNCH_PATROL", 11, PatrolAirframeHost),
    refused!("HALT", 12, GroupCommandHost),
    refused!("TRANSPORT", 13, TransportContainmentHost),
    refused!("SET_TRANSPORT", 14, TransportContainmentHost),
    refused!("BOARD_SHIP", 15, TransportContainmentHost),
    refused!("REPAIR", 16, ConstructionHost),
    refused!("TRADE", 17, GatheringHost),
    refused!("CITY_GATHER", 18, GatheringHost),
    refused!("GATHER", 19, GatheringHost),
    refused!("GARRISON", 20, TransportContainmentHost),
    refused!("DISBAND", 21, TerminalLifecycleHost),
    refused!("GATHER_POINT", 22, GatheringHost),
    refused!("SPELL", 23, SpellHost),
    refused!("QUEUE_UP", 24, ProductionHost),
    refused!("BUILD", 25, ConstructionHost),
    refused!("EJECTALL", 26, TransportContainmentHost),
    refused!("FLIGHT", 28, PatrolAirframeHost),
    refused!("STOP_SPELL", 29, SpellHost),
    refused!("FOLLOW", 30, MovementCommandHost),
    refused!("GUARD", 31, MovementCommandHost),
    refused!("RECALL", 35, TransportContainmentHost),
    refused!("SCRAMBLE", 36, PatrolAirframeHost),
    refused!("UNQUEUE", 48, ProductionHost),
    refused!("COME_OUT", 49, TransportContainmentHost),
];

/// Index-aligned with `generated::PLAYER_VERBS`. No player verb is admitted until its
/// complete Sim-owned command/lifecycle transaction exists.
pub const PLAYER_INTEGRATION: [VerbIntegration; PLAYER_VERB_COUNT] = [
    refused!("ALARM", 27, LeaderOptionsHost),
    refused!("UNITMASK", 32, GroupCommandHost),
    refused!("BUILDMASK", 33, GroupCommandHost),
    refused!("TREATY", 37, DiplomacyHost),
    refused!("DECLARE", 38, DiplomacyHost),
    refused!("CLEAR_TRIBUTES", 39, TributeProposalHost),
    refused!("CLEAR_ALL", 40, TributeProposalHost),
    refused!("ACCEPT", 41, TributeProposalHost),
    refused!("REJECT", 42, TributeProposalHost),
    refused!("TRIBUTE", 43, TributeProposalHost),
    refused!("DEMAND_TRIBUTE", 44, TributeProposalHost),
    refused!("PROPOSE_ATTACK", 45, TributeProposalHost),
    refused!("BUY", 46, MarketHost),
    refused!("SELL", 47, MarketHost),
    refused!("RESIGN", 70, TerminalLifecycleHost),
    refused!("LEADER_OPTIONS", 73, LeaderOptionsHost),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueuePosition {
    First,
    Last,
    Replace,
}

/// Exact movement/collision facts captured for one unit in scenario allocation order.
///
/// Handles are deliberately absent: reset reconstructs them, while the unit ordinal is part
/// of [`ScenarioSpec`]'s deterministic allocation contract.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScenarioMovementSource {
    pub unit: usize,
    pub source: LiveCollisionSource,
}

/// Additive authoritative setup surface which leaves the original [`ScenarioSpec`] stable.
///
/// Movement sources are installed in declaration order after every unit has spawned. The
/// declaration is retained by [`AuthoritativeBackend`] and replayed by
/// [`AuthoritativeBackend::reset`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthoritativeScenarioSpec {
    pub episode: ScenarioSpec,
    pub movement_sources: Vec<ScenarioMovementSource>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScenarioSetupError {
    Episode(EpisodeError),
    MovementSourceUnitOutOfRange {
        source: usize,
        unit: usize,
        units: usize,
    },
    DuplicateMovementSource {
        source: usize,
        unit: usize,
    },
    MovementSource {
        source: usize,
        unit: usize,
        fault: LiveCollisionFault,
    },
}

impl fmt::Display for ScenarioSetupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Episode(error) => write!(f, "{error}"),
            Self::MovementSourceUnitOutOfRange {
                source,
                unit,
                units,
            } => write!(
                f,
                "scenario movement source {source} names unit {unit}, but only {units} units exist"
            ),
            Self::DuplicateMovementSource { source, unit } => write!(
                f,
                "scenario movement source {source} duplicates source for unit {unit}"
            ),
            Self::MovementSource {
                source,
                unit,
                fault,
            } => write!(
                f,
                "scenario movement source {source} for unit {unit} was refused: {fault:?}"
            ),
        }
    }
}

impl std::error::Error for ScenarioSetupError {}

impl From<EpisodeError> for ScenarioSetupError {
    fn from(error: EpisodeError) -> Self {
        Self::Episode(error)
    }
}

/// Action-owned transition into `UnitData::moving/action_type`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActionSourceState {
    pub moving: bool,
    pub action: OrderIndex,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActionSourceStateRequest {
    pub actor: Handle,
    pub expected_revision: u64,
    pub state: ActionSourceState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionSourceStateSetterRoute {
    /// Snapshot the Sim-owned source, then compare-exchange its action fields immediately
    /// before installing the order.
    SimCompareExchange,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActionSourceStateSetterIntegration {
    pub owner: &'static str,
    pub required_method: &'static str,
    pub route: ActionSourceStateSetterRoute,
}

/// Typed integration map for MOVE_TO's Sim-owned action-state transition.
///
/// The backend stores no action-state sidecar. Prepared requests carry the observed revision,
/// and the Sim owner validates identity plus revision before changing either source field.
pub const ACTION_SOURCE_STATE_SETTER: ActionSourceStateSetterIntegration =
    ActionSourceStateSetterIntegration {
        owner: "don_sim::tick::Sim -> movement_live::LiveCollisionRuntime",
        required_method: "Sim::movement_source_state + Sim::compare_exchange_movement_source_state",
        route: ActionSourceStateSetterRoute::SimCompareExchange,
    };

/// Raw core-coordinate request produced after the policy head decoder resolves grid cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnitActionRequest {
    /// Generated unit Verb head: 0 is NOOP, `n + 1` indexes `UNIT_INTEGRATION[n]`.
    pub verb_head: u16,
    pub actor: Handle,
    pub target_x: i32,
    pub target_y: i32,
    /// Generated `TargetEntity` head. Zero means no target; `n + 1` must eventually bind to
    /// row `n` of the same authoritative observation image used by the policy.
    pub target_entity: u16,
    pub queue: QueuePosition,
    pub order_flags: u8,
}

pub const UNIT_VERB_HEAD_COUNT: usize = UNIT_VERB_COUNT + 1;

/// Exact conditional mask for the verb head of one complete request template.
///
/// Actor, destination, queue, and order flags are held fixed. Consequently a set bit means
/// replacing only `verb_head` with that index is accepted by the same read-only preflight
/// used by [`AuthoritativeBackend::apply_unit`]. Index zero is NOOP.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuthoritativeUnitVerbMask {
    pub allowed: [bool; UNIT_VERB_HEAD_COUNT],
}

impl AuthoritativeUnitVerbMask {
    pub fn allows(&self, verb_head: usize) -> bool {
        self.allowed.get(verb_head).copied().unwrap_or(false)
    }
}

/// Shape needed to decode the generated ten-head policy action without borrowing the
/// compact backend. Coordinates retain `VecEnv`'s public contract: one head cell is one
/// quarter-tile [`SUBTILE`], addressed at its centre.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FactoredUnitActionSpace {
    pub grid_w: usize,
    pub grid_h: usize,
    pub max_entities: usize,
}

/// Refusal produced before the authoritative core is borrowed or mutated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeadDecodeRefusal {
    WrongHeadCount {
        actual: usize,
        expected: usize,
    },
    ValueOutOfRange {
        head: usize,
        value: i32,
        exclusive_max: usize,
    },
    CoordinateOverflow {
        head: usize,
        value: i32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FactoredApplyRefusal {
    Decode(HeadDecodeRefusal),
    Apply(ApplyRefusal),
}

/// Decode exactly the generated `UnitHead` layout into the narrower authoritative request.
///
/// Every head is range-checked even when the selected verb does not consume it. This keeps
/// an unmasked/broken policy distinguishable from a hosted verb refusal and avoids compact
/// `UnitAction::from_slice`'s intentionally permissive missing/negative-to-zero conversion.
pub fn decode_unit_heads(
    actor: Handle,
    heads: &[i32],
    space: FactoredUnitActionSpace,
) -> Result<UnitActionRequest, HeadDecodeRefusal> {
    if heads.len() != crate::generated::N_UNIT_HEADS {
        return Err(HeadDecodeRefusal::WrongHeadCount {
            actual: heads.len(),
            expected: crate::generated::N_UNIT_HEADS,
        });
    }
    let sizes = [
        crate::generated::N_UNIT_VERBS + 1,
        space.grid_w,
        space.grid_h,
        space.max_entities.saturating_add(1),
        crate::generated::NUM_TYPES,
        3,
        4,
        crate::generated::FORMS.len(),
        8,
        5,
    ];
    for (head, (&value, &exclusive_max)) in heads.iter().zip(&sizes).enumerate() {
        if value < 0 || usize::try_from(value).map_or(true, |value| value >= exclusive_max) {
            return Err(HeadDecodeRefusal::ValueOutOfRange {
                head,
                value,
                exclusive_max,
            });
        }
    }

    let coord = |head: usize| {
        heads[head]
            .checked_mul(SUBTILE)
            .and_then(|value| value.checked_add(SUBTILE / 2))
            .ok_or(HeadDecodeRefusal::CoordinateOverflow {
                head,
                value: heads[head],
            })
    };
    let queue = match heads[crate::generated::UnitHead::QueuePos as usize] {
        0 => QueuePosition::First,
        1 => QueuePosition::Last,
        2 => QueuePosition::Replace,
        _ => unreachable!("queue head was range-checked"),
    };
    Ok(UnitActionRequest {
        verb_head: heads[crate::generated::UnitHead::Verb as usize] as u16,
        actor,
        target_x: coord(crate::generated::UnitHead::TargetX as usize)?,
        target_y: coord(crate::generated::UnitHead::TargetY as usize)?,
        target_entity: heads[crate::generated::UnitHead::TargetEntity as usize] as u16,
        queue,
        order_flags: heads[crate::generated::UnitHead::OrderMods as usize] as u8,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApplyReceipt {
    Noop {
        frame: i32,
    },
    OrderInstalled {
        frame: i32,
        actor: Handle,
        kind: OrderIndex,
        queue_len: usize,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApplyRefusal {
    UnknownVerb(u16),
    Unhosted {
        verb_index: usize,
        boundary: IntegrationBoundary,
    },
    InvalidPlayer(u8),
    PlayerNotAlive(u8),
    StaleActor(Handle),
    ActorNotOwned {
        actor: Handle,
        expected: u8,
        actual: u8,
    },
    InactiveActor(Handle),
    MissingTargetEntity {
        verb_index: usize,
    },
    /// The target head named an observation ordinal, but the authoritative backend exposes no
    /// external row image whose identity and cloak/detection visibility are both complete.
    TargetIdentityVisibilityUnavailable {
        verb_index: usize,
        target_entity: u16,
    },
    /// A complete visibility image exists, but this ordinal is absent from the exact image
    /// shown to the named viewer (or that image was invalidated before apply).
    TargetVisibility {
        verb_index: usize,
        target_entity: u16,
        fault: VisibilityProjectionFault,
    },
    /// Identity, visibility, combat dependencies, and can-hurt were revalidated against `Sim`;
    /// the remaining production ATTACK transaction does not yet consume that prepared proof.
    AttackTargetCommitUnavailable {
        verb_index: usize,
        target: ExternalEntityIdentity,
        boundary: IntegrationBoundary,
    },
    /// Identity and visibility were current, but the target fails a retail-known eligibility
    /// or the current executor's exact positive-damage predicate.
    AttackTargetIneligible {
        verb_index: usize,
        target: ExternalEntityIdentity,
        reason: AttackTargetEligibilityRefusal,
    },
    /// The exact target is visible and eligible, but the production executor would reach a
    /// silent dependency exit. These sources are setup authority, never inferred defaults.
    AttackExecutionUnavailable {
        verb_index: usize,
        target: ExternalEntityIdentity,
        dependency: AttackExecutionDependencyRefusal,
    },
    InvalidDestination {
        x: i32,
        y: i32,
    },
    UnsupportedQueue(QueuePosition),
    UnsupportedOrderFlags(u8),
    MovementHost(LiveCollisionFault),
    StaleEpisodeRevision {
        expected: u64,
        observed: u64,
    },
    MovementSourceState(LiveCollisionFault),
    CoreRejectedAfterPreflight,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttackTargetEligibilityRefusal {
    ActorIsTarget,
    RelationUnavailable { owner: u8 },
    NotEnemy { relation: Diplo },
    TargetNotAlive { hits: i32 },
    CannotHurt { predicted_damage: i32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttackExecutionDependencyRefusal {
    ActorTypeUnavailable {
        row: usize,
    },
    TargetTypeUnavailable {
        row: usize,
    },
    TargetPublicStateChanged {
        captured: ExternalEntityPublicState,
        observed: ExternalEntityPublicState,
    },
    BalanceTableUnavailable,
    AttackerTypeStatsUnavailable {
        type_id: i32,
    },
    DefenderTypeStatsUnavailable {
        type_id: i32,
    },
    BalanceEntryUnavailable {
        attacker_type_id: i32,
        defender_type_id: i32,
    },
}

/// Setup-source validation for the ATTACK executor's optional combat tables.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttackExecutionSourceRefusal {
    InvalidTypeId {
        index: usize,
        type_id: i32,
    },
    DuplicateTypeId {
        first: usize,
        second: usize,
        type_id: i32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Relation {
    Own,
    Ally,
    Peace,
    Enemy,
}

/// Minimal policy-visible unit row. Opponent rows are intentionally absent until a cloak/
/// detection-aware visibility host exists; this backend never substitutes omniscience.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OwnEntityObservation {
    pub handle: Handle,
    pub who: u8,
    pub object_o: i16,
    pub uid: u16,
    pub type_id: i32,
    pub x: i32,
    pub y: i32,
    pub hits: i32,
    pub angle: i32,
    pub speed: i16,
    pub recharge: u8,
    pub order: OrderIndex,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoreObservation {
    pub who: u8,
    pub frame: i32,
    pub seconds: i32,
    pub alive: bool,
    pub won: bool,
    pub score: i32,
    pub economy: [i32; 6],
    pub income: [i32; 6],
    pub diplomacy: [Relation; 8],
    pub own_entities: Vec<OwnEntityObservation>,
    /// Stable, one-based policy target rows from the exact captured frame image.
    pub external_entities: Vec<VisibleExternalEntity>,
    /// True only when `external_entities` is the complete current-frame projection.
    pub external_entities_complete: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectionRefusal {
    InvalidPlayer(u8),
    PlayerNotInGame(u8),
    MissingHandle(usize),
    MissingType(usize),
    ExternalVisibility(VisibilityProjectionFault),
}

/// Static UnitType cloak facts must enter through an explicit captured/shipped-data source.
/// A zero-filled default `TypeTable` is not silently treated as authoritative.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VisibilityStaticSourceRefusal {
    InvalidType(i32),
    InvalidViewer(u8),
    InvalidFogOption(u8),
    ViewerMaskExcludesSelf { viewer: u8, mask: u8 },
}

/// Failure to materialise one complete current-frame visibility image from `Sim`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VisibilityCaptureRefusal {
    MissingHandle(usize),
    MissingType(usize),
    TypeFlagsUnavailable {
        row: usize,
        type_id: i32,
    },
    TypeFlagsChanged {
        type_id: i32,
        captured: u32,
        observed: u32,
    },
    ViewerMaskUnavailable {
        viewer: u8,
    },
    ViewerMaskChanged {
        viewer: u8,
        captured: u8,
        observed: u8,
    },
    /// The core currently owns `seen3`, but its step-12 producer still stamps every object
    /// with `detector=false`. A cloaked row therefore cannot be called complete yet.
    DetectionPlaneCompletenessUnavailable {
        row: usize,
    },
    InvalidFogCell {
        row: usize,
        fx: i32,
        fy: i32,
    },
    InvalidWorldCell {
        row: usize,
        wx: i32,
        wy: i32,
    },
    Install(VisibilityInstallFault),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CoreRewardSnapshot {
    pub score: i32,
    pub economy_total: i32,
    pub alive: bool,
    pub won: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CoreReward {
    pub score_delta: i32,
    pub economy_delta: i32,
    pub win: bool,
    pub loss: bool,
    pub alive: bool,
}

/// Immutable transaction prepared against one exact Sim-owned movement-source revision.
///
/// The fields are private so callers cannot fabricate a plan. Keeping a plan is optional:
/// [`AuthoritativeBackend::apply_unit`] prepares and commits in one borrow. The explicit API
/// exists for schedulers which separate read-only planning from mutation and must receive a
/// typed stale-revision refusal if another action wins first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreparedUnitAction {
    who: u8,
    request: UnitActionRequest,
    episode_revision: u64,
    row: usize,
    kind: OrderIndex,
    source: MovementSourceState,
}

/// Exact production inputs which must stay unchanged between ATTACK prepare and commit.
///
/// This is evidence, not an admission permit: the production tick does not yet consume this
/// proof atomically, so [`PreparedAttackTargetTransaction`] remains fail-closed at commit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AttackExecutionProof {
    frame: i32,
    actor_who: u8,
    actor_x: i32,
    actor_y: i32,
    actor_speed: i16,
    actor_recharge: u8,
    actor_type_id: i32,
    target_type_id: i32,
    attacker: UnitTypeStats,
    defender: UnitTypeStats,
    balance_pct: i32,
    combat_rules: don_sim::mechanics::CombatRules,
    recharge_rules: don_sim::systems::combat::CombatConstants,
    shooter: Option<don_sim::systems::ammo::ShooterRules>,
    predicted_damage: i32,
    recharge_frames: i32,
}

impl AttackExecutionProof {
    pub const fn actor_type_id(self) -> i32 {
        self.actor_type_id
    }

    pub const fn target_type_id(self) -> i32 {
        self.target_type_id
    }

    pub const fn balance_pct(self) -> i32 {
        self.balance_pct
    }

    pub const fn predicted_damage(self) -> i32 {
        self.predicted_damage
    }

    pub const fn recharge_frames(self) -> i32 {
        self.recharge_frames
    }

    pub const fn uses_projectile(self) -> bool {
        self.shooter.is_some()
    }
}

#[derive(Clone)]
struct AttackExecutionSources {
    balance: Arc<don_sim::balance::BalanceTable>,
    unit_stats: Arc<Vec<UnitTypeStats>>,
}

/// Maximal fail-closed ATTACK transaction before the production executor boundary.
///
/// This token retains the policy request, Sim episode revision, opaque visibility binding,
/// exact stable/retail target identity, hostile eligibility, captured public state, and every
/// optional input read by the current executor. It can produce the non-lossy queue node, but it
/// is not an admission token: the normal mask and apply routes remain red until the production
/// tick atomically consumes the proof under authoritative Step-12 freshness.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreparedAttackTargetTransaction {
    who: u8,
    request: UnitActionRequest,
    episode_revision: u64,
    actor_row: usize,
    target_row: usize,
    binding: VisibleTargetBinding,
    target: ExternalEntityIdentity,
    target_public: ExternalEntityPublicState,
    relation: Diplo,
    execution: AttackExecutionProof,
}

impl PreparedAttackTargetTransaction {
    pub const fn actor(&self) -> Handle {
        self.request.actor
    }

    pub const fn target(&self) -> ExternalEntityIdentity {
        self.target
    }

    pub const fn episode_revision(&self) -> u64 {
        self.episode_revision
    }

    pub const fn visibility_revision(&self) -> u64 {
        self.binding.owner_revision()
    }

    pub const fn visibility_frame(&self) -> i32 {
        self.binding.frame()
    }

    pub const fn target_ordinal(&self) -> u16 {
        self.binding.ordinal()
    }

    pub const fn relation(&self) -> Diplo {
        self.relation
    }

    pub const fn target_public(&self) -> ExternalEntityPublicState {
        self.target_public
    }

    pub const fn execution_proof(&self) -> AttackExecutionProof {
        self.execution
    }

    /// Build the exact queue payload without claiming that the production consumer is ready.
    pub fn retained_order(&self) -> Order {
        Order::attack_exact(OrderTargetIdentity {
            handle: self.target.handle,
            who: self.target.who as i8,
            o: self.target.object_o,
            uid: self.target.uid,
        })
    }
}

impl PreparedUnitAction {
    pub fn actor(&self) -> Handle {
        self.request.actor
    }

    pub fn kind(&self) -> OrderIndex {
        self.kind
    }

    pub fn source_revision(&self) -> u64 {
        self.source.revision
    }

    pub fn episode_revision(&self) -> u64 {
        self.episode_revision
    }
}

pub struct AuthoritativeBackend {
    episode: AuthoritativeEpisode,
    episode_revision: u64,
    scenario_movement_sources: Vec<ScenarioMovementSource>,
    external_visibility: ExternalEntityVisibilityOwner,
    visibility_type_flags: BTreeMap<i32, u32>,
    visibility_viewer_masks: BTreeMap<u8, u8>,
    visibility_fog_option: Option<u8>,
    attack_execution_sources: Option<AttackExecutionSources>,
}

impl AuthoritativeBackend {
    pub fn from_spec(spec: ScenarioSpec) -> Result<Self, EpisodeError> {
        Ok(Self {
            episode: AuthoritativeEpisode::from_spec(spec)?,
            episode_revision: 0,
            scenario_movement_sources: Vec::new(),
            external_visibility: ExternalEntityVisibilityOwner::default(),
            visibility_type_flags: BTreeMap::new(),
            visibility_viewer_masks: BTreeMap::new(),
            visibility_fog_option: None,
            attack_execution_sources: None,
        })
    }

    /// Construct a scenario and atomically install its captured movement sources.
    ///
    /// Source ordinals and duplicates are rejected before the episode is constructed. Any
    /// collision-host refusal then destroys the private candidate rather than exposing a
    /// partly installed backend.
    pub fn from_authoritative_scenario(
        setup: AuthoritativeScenarioSpec,
    ) -> Result<Self, ScenarioSetupError> {
        validate_scenario_movement_sources(&setup)?;
        let AuthoritativeScenarioSpec {
            episode: spec,
            movement_sources,
        } = setup;
        let mut episode = AuthoritativeEpisode::from_spec(spec)?;
        install_scenario_movement_sources(&mut episode, &movement_sources)?;
        Ok(Self {
            episode,
            episode_revision: 0,
            scenario_movement_sources: movement_sources,
            external_visibility: ExternalEntityVisibilityOwner::default(),
            visibility_type_flags: BTreeMap::new(),
            visibility_viewer_masks: BTreeMap::new(),
            visibility_fog_option: None,
            attack_execution_sources: None,
        })
    }

    /// Rebuild seed, allocation identity, and captured movement sources as one replacement.
    /// The current episode remains intact if construction or source installation is refused.
    pub fn reset(&mut self) -> Result<(), ScenarioSetupError> {
        let mut replacement = Self::from_authoritative_scenario(AuthoritativeScenarioSpec {
            episode: self.episode.spec().clone(),
            movement_sources: self.scenario_movement_sources.clone(),
        })?;
        replacement.visibility_type_flags = self.visibility_type_flags.clone();
        replacement.visibility_viewer_masks = self.visibility_viewer_masks.clone();
        replacement.visibility_fog_option = self.visibility_fog_option;
        if let Some(option) = replacement.visibility_fog_option {
            replacement.episode.sim_mut_for_backend().map.fog.option =
                don_sim::systems::borders_fog::FogOption(option);
        }
        replacement.attack_execution_sources = self.attack_execution_sources.clone();
        if let Some(sources) = replacement.attack_execution_sources.as_ref() {
            apply_attack_execution_sources(replacement.episode.sim_mut_for_backend(), sources);
        }
        apply_visibility_type_flags(
            replacement.episode.sim_mut_for_backend(),
            &replacement.visibility_type_flags,
        );
        apply_visibility_viewer_masks(
            replacement.episode.sim_mut_for_backend(),
            &replacement.visibility_viewer_masks,
        );
        let mut external_visibility = std::mem::take(&mut self.external_visibility);
        external_visibility
            .reset()
            .expect("a visibility revision cannot exhaust in a process lifetime");
        replacement.external_visibility = external_visibility;
        replacement.episode_revision = self.episode_revision.wrapping_add(1);
        *self = replacement;
        Ok(())
    }

    pub fn step_frames(&mut self, frames: u32) -> StepReceipt {
        let receipt = self.episode.step_frames(frames);
        if frames != 0 {
            self.invalidate_external_visibility();
        }
        receipt
    }

    pub fn sim(&self) -> &don_sim::tick::Sim {
        self.episode.sim()
    }

    /// Captured setup sources in deterministic installation order.
    pub fn scenario_movement_sources(&self) -> &[ScenarioMovementSource] {
        &self.scenario_movement_sources
    }

    /// Scenario/content setup installs exact non-column movement facts through Sim's own
    /// atomic spatial transaction. This is setup authority, not a policy action.
    pub fn install_movement_source(
        &mut self,
        actor: Handle,
        source: LiveCollisionSource,
    ) -> Result<usize, LiveCollisionFault> {
        self.episode
            .sim_mut_for_backend()
            .install_movement_collision_source(actor, source)
    }

    /// Atomically install the captured combat tables which the production ATTACK executor
    /// reads. The backend retains these immutable sources across deterministic reset.
    ///
    /// This is setup authority, not a permissive fallback: incomplete type coverage remains a
    /// typed per-action refusal. Replacing the source invalidates every visible target ordinal
    /// so a prepared action can never combine an old observation with new combat content.
    pub fn install_attack_execution_sources(
        &mut self,
        balance: Arc<don_sim::balance::BalanceTable>,
        unit_stats: Vec<UnitTypeStats>,
    ) -> Result<(), AttackExecutionSourceRefusal> {
        validate_attack_execution_sources(&unit_stats)?;
        let sources = AttackExecutionSources {
            balance,
            unit_stats: Arc::new(unit_stats),
        };
        apply_attack_execution_sources(self.episode.sim_mut_for_backend(), &sources);
        self.attack_execution_sources = Some(sources);
        self.invalidate_external_visibility();
        Ok(())
    }

    /// Install one exact `UnitTypeData::unit_flags +0x2B4` capture into the Sim-owned type
    /// table. The capture is retained across deterministic reset; changing it invalidates
    /// every outstanding external ordinal.
    pub fn install_visibility_type_flags(
        &mut self,
        type_id: i32,
        unit_flags: u32,
    ) -> Result<(), VisibilityStaticSourceRefusal> {
        let type_index = usize::try_from(type_id)
            .ok()
            .filter(|&index| index < self.episode.sim().vic_leaders.types.rows.len())
            .ok_or(VisibilityStaticSourceRefusal::InvalidType(type_id))?;
        self.episode.sim_mut_for_backend().vic_leaders.types.rows[type_index].unit_flags =
            unit_flags as i32;
        self.visibility_type_flags.insert(type_id, unit_flags);
        self.invalidate_external_visibility();
        Ok(())
    }

    /// Install one exact `LeaderData::ally_mask +0x6929` capture. This byte is not inferred
    /// from diplomacy: retail reads the stored mask directly in both fog and detection queries.
    pub fn install_visibility_viewer_mask(
        &mut self,
        viewer: u8,
        mask: u8,
    ) -> Result<(), VisibilityStaticSourceRefusal> {
        let index = usize::from(viewer);
        if index >= self.episode.sim().map.fog.leaders.len() {
            return Err(VisibilityStaticSourceRefusal::InvalidViewer(viewer));
        }
        if mask & (1u8 << viewer) == 0 {
            return Err(VisibilityStaticSourceRefusal::ViewerMaskExcludesSelf { viewer, mask });
        }
        self.episode.sim_mut_for_backend().map.fog.leaders[index].player_mask = mask;
        self.visibility_viewer_masks.insert(viewer, mask);
        self.invalidate_external_visibility();
        Ok(())
    }

    /// Install the captured `GameData +0x30` fog policy used by external visibility.
    ///
    /// In particular, option 3 is an explicit all-current-fog-visible policy fact; it is not a
    /// substitute for stamping `seen` or `seen3` when the Step-12 producer is unavailable.
    pub fn install_visibility_fog_option(
        &mut self,
        option: u8,
    ) -> Result<(), VisibilityStaticSourceRefusal> {
        if option > 3 {
            return Err(VisibilityStaticSourceRefusal::InvalidFogOption(option));
        }
        self.episode.sim_mut_for_backend().map.fog.option =
            don_sim::systems::borders_fog::FogOption(option);
        self.visibility_fog_option = Some(option);
        self.invalidate_external_visibility();
        Ok(())
    }

    /// Capture every active Unit row and every valid viewer from the sole `Sim` owner, then
    /// atomically install the cloak/detection/fog image which both observation and ATTACK
    /// preflight consume. Every live type must have an explicit `unit_flags` source.
    pub fn capture_external_visibility(&mut self) -> Result<u64, VisibilityCaptureRefusal> {
        let frame = capture_external_visibility_frame(
            self.episode.sim(),
            &self.visibility_type_flags,
            &self.visibility_viewer_masks,
        )?;
        self.external_visibility
            .install_frame(frame)
            .map_err(VisibilityCaptureRefusal::Install)
    }

    pub fn external_visibility_revision(&self) -> u64 {
        self.external_visibility.revision()
    }

    fn invalidate_external_visibility(&mut self) {
        self.external_visibility
            .reset()
            .expect("a visibility revision cannot exhaust in a process lifetime");
    }

    /// Compute the verb mask without borrowing any mutable simulation store.
    pub fn unit_verb_mask(
        &self,
        who: u8,
        template: UnitActionRequest,
    ) -> AuthoritativeUnitVerbMask {
        let mut allowed = [false; UNIT_VERB_HEAD_COUNT];
        for (verb_head, slot) in allowed.iter_mut().enumerate() {
            let request = UnitActionRequest {
                verb_head: verb_head as u16,
                ..template
            };
            *slot = preflight_unit(
                self.episode.sim(),
                &self.external_visibility,
                self.episode_revision,
                who,
                request,
            )
            .is_ok();
        }
        AuthoritativeUnitVerbMask { allowed }
    }

    pub fn apply_unit(
        &mut self,
        who: u8,
        request: UnitActionRequest,
    ) -> Result<ApplyReceipt, ApplyRefusal> {
        let Some(prepared) = self.prepare_unit(who, request)? else {
            return Ok(ApplyReceipt::Noop {
                frame: self.episode.sim().world.frame,
            });
        };
        self.apply_prepared_unit(prepared)
    }

    /// Prepare one action without borrowing or mutating any simulation store.
    ///
    /// The returned plan contains the Sim-owned movement-source revision observed by the
    /// shared mask/apply preflight. It is caller-owned transaction state, not a backend or
    /// masking sidecar.
    pub fn prepare_unit(
        &self,
        who: u8,
        request: UnitActionRequest,
    ) -> Result<Option<PreparedUnitAction>, ApplyRefusal> {
        preflight_unit(
            self.episode.sim(),
            &self.external_visibility,
            self.episode_revision,
            who,
            request,
        )
    }

    /// Prepare ATTACK through exact actor, identity, visibility, and hostile-eligibility
    /// ownership without admitting it to the policy mask.
    ///
    /// Callers may retain this token across scheduler phases. A tick, visibility refresh, or
    /// reset makes its revisions stale; [`Self::apply_prepared_attack_target`] then refuses
    /// before mutation. Even a current token stops at `CombatTargetHost` until the production
    /// executor consumes both the retained identity and this execution proof atomically.
    pub fn prepare_attack_target(
        &self,
        who: u8,
        request: UnitActionRequest,
    ) -> Result<PreparedAttackTargetTransaction, ApplyRefusal> {
        prepare_attack_target_transaction(
            self.episode.sim(),
            &self.external_visibility,
            self.episode_revision,
            who,
            request,
        )
    }

    /// Revalidate a prepared ATTACK transaction, preserving typed staleness and the final
    /// production-consumer refusal. This method never installs an order in the current tranche.
    pub fn apply_prepared_attack_target(
        &mut self,
        prepared: PreparedAttackTargetTransaction,
    ) -> Result<ApplyReceipt, ApplyRefusal> {
        if prepared.episode_revision != self.episode_revision {
            return Err(ApplyRefusal::StaleEpisodeRevision {
                expected: prepared.episode_revision,
                observed: self.episode_revision,
            });
        }
        validate_prepared_attack_target(self.episode.sim(), &self.external_visibility, prepared)?;
        Err(ApplyRefusal::AttackTargetCommitUnavailable {
            verb_index: crate::generated::uv::ATTACK,
            target: prepared.target,
            boundary: IntegrationBoundary::CombatTargetHost,
        })
    }

    /// Commit a previously prepared action or refuse it before any mutation.
    ///
    /// Ordinary request facts are revalidated read-only. The prepared source revision is then
    /// compared and exchanged immediately before `Sim::issue`; consequently a stale plan can
    /// neither overwrite a newer action state nor replace its order.
    pub fn apply_prepared_unit(
        &mut self,
        prepared: PreparedUnitAction,
    ) -> Result<ApplyReceipt, ApplyRefusal> {
        if prepared.episode_revision != self.episode_revision {
            return Err(ApplyRefusal::StaleEpisodeRevision {
                expected: prepared.episode_revision,
                observed: self.episode_revision,
            });
        }
        let Some(current) = preflight_unit(
            self.episode.sim(),
            &self.external_visibility,
            self.episode_revision,
            prepared.who,
            prepared.request,
        )?
        else {
            unreachable!("a prepared non-NOOP action cannot become NOOP")
        };
        debug_assert_eq!(current.row, prepared.row);
        debug_assert_eq!(current.kind, prepared.kind);

        let order = Order {
            kind: prepared.kind,
            flags: prepared.request.order_flags,
            x: prepared.request.target_x,
            y: prepared.request.target_y,
            tolerance: 0,
            ..Order::default()
        };
        let sim = self.episode.sim_mut_for_backend();
        sim.compare_exchange_movement_source_state(
            prepared.request.actor,
            prepared.source.revision,
            true,
            prepared.kind,
        )
        .map_err(ApplyRefusal::MovementSourceState)?;
        // The successful identity-bound CAS proves the handle live. There is deliberately no
        // fallible backend operation between this transition and order installation.
        assert!(
            sim.issue(prepared.request.actor, order),
            "movement-source CAS proved actor live immediately before Sim::issue"
        );
        let frame = sim.world.frame;
        let queue_len = sim.world.orders(prepared.row).len();
        self.invalidate_external_visibility();
        Ok(ApplyReceipt::OrderInstalled {
            frame,
            actor: prepared.request.actor,
            kind: prepared.kind,
            queue_len,
        })
    }

    /// Decode and apply one generated ten-head action without entering compact `EnvWorld`.
    pub fn apply_unit_heads(
        &mut self,
        who: u8,
        actor: Handle,
        heads: &[i32],
        space: FactoredUnitActionSpace,
    ) -> Result<ApplyReceipt, FactoredApplyRefusal> {
        let request =
            decode_unit_heads(actor, heads, space).map_err(FactoredApplyRefusal::Decode)?;
        self.apply_unit(who, request)
            .map_err(FactoredApplyRefusal::Apply)
    }

    /// Player-verb counterpart to [`Self::apply_unit`]. The map is executable even while
    /// every player transaction is red: callers receive the precise missing owner and no
    /// action can accidentally fall through to an accepted no-effect result.
    pub fn apply_player(&mut self, verb_head: u16) -> Result<ApplyReceipt, ApplyRefusal> {
        if verb_head == 0 {
            return Ok(ApplyReceipt::Noop {
                frame: self.episode.sim().world.frame,
            });
        }
        let verb_index = usize::from(verb_head - 1);
        let integration = PLAYER_INTEGRATION
            .get(verb_index)
            .ok_or(ApplyRefusal::UnknownVerb(verb_head))?;
        match integration.route {
            VerbRoute::Refused(boundary) => Err(ApplyRefusal::Unhosted {
                verb_index,
                boundary,
            }),
            VerbRoute::SimIssue | VerbRoute::SimAttackIssue => {
                Err(ApplyRefusal::CoreRejectedAfterPreflight)
            }
        }
    }

    /// Sim-owned own-state plus the optional complete current-frame external projection.
    /// Without an explicit type-flags source and a fresh capture, no external row leaks.
    pub fn observe(&self, who: u8) -> Result<CoreObservation, ProjectionRefusal> {
        let sim = self.episode.sim();
        let player = sim
            .vic_leaders
            .slots
            .get(usize::from(who))
            .ok_or(ProjectionRefusal::InvalidPlayer(who))?;
        if !player.flag(leader_flag::VALID) {
            return Err(ProjectionRefusal::PlayerNotInGame(who));
        }
        let mut own_entities = Vec::new();
        for row in 0..sim.world.live_count() as usize {
            if sim.world.units.get_who(row) != who {
                continue;
            }
            let handle = sim
                .world
                .handle_at_row(row)
                .ok_or(ProjectionRefusal::MissingHandle(row))?;
            let type_id = *sim
                .unit_type
                .get(row)
                .ok_or(ProjectionRefusal::MissingType(row))?;
            own_entities.push(OwnEntityObservation {
                handle,
                who,
                object_o: sim.world.units.o()[row],
                uid: sim.world.units.get_uid(row),
                type_id,
                x: sim.world.units.x_internal()[row],
                y: sim.world.units.y_internal()[row],
                hits: sim.world.units.myhits()[row],
                angle: sim.world.units.angle()[row],
                speed: sim.world.units.myspeed()[row],
                recharge: sim.world.units.get_recharging(row),
                order: sim.world.orders(row).order_type(),
            });
        }
        let diplomacy = std::array::from_fn(|other| {
            if other == usize::from(who) {
                Relation::Own
            } else {
                match sim.vic_leaders.get_diplo(usize::from(who), other) {
                    Diplo::Ally => Relation::Ally,
                    Diplo::Peace => Relation::Peace,
                    Diplo::War => Relation::Enemy,
                }
            }
        });
        let (external_entities, external_entities_complete) =
            match self.external_visibility.project(who) {
                Ok(projection) => (projection.rows, true),
                Err(VisibilityProjectionFault::Uninstalled) => (Vec::new(), false),
                Err(fault) => return Err(ProjectionRefusal::ExternalVisibility(fault)),
            };
        Ok(CoreObservation {
            who,
            frame: sim.world.frame,
            seconds: sim.world.seconds,
            alive: player.is_alive(),
            won: player.flag(leader_flag::WON),
            score: player.score,
            economy: player.economy.bucket,
            income: player.economy.income,
            diplomacy,
            own_entities,
            external_entities,
            external_entities_complete,
        })
    }

    pub fn currently_visible(&self, who: u8, x: i32, y: i32) -> bool {
        let sim = self.episode.sim();
        if !sim.map.world.valid_coord(x, y) || usize::from(who) >= sim.vic_leaders.slots.len() {
            return false;
        }
        let fx = FCoord::from_coord(Coord(x)).0;
        let fy = FCoord::from_coord(Coord(y)).0;
        sim.map.fog.is_seen(&sim.map.world, fx, fy, i32::from(who))
    }

    pub fn reward_snapshot(&self, who: u8) -> Result<CoreRewardSnapshot, ProjectionRefusal> {
        let sim = self.episode.sim();
        let player = sim
            .vic_leaders
            .slots
            .get(usize::from(who))
            .ok_or(ProjectionRefusal::InvalidPlayer(who))?;
        if !player.flag(leader_flag::VALID) {
            return Err(ProjectionRefusal::PlayerNotInGame(who));
        }
        Ok(CoreRewardSnapshot {
            score: player.score,
            economy_total: player.economy.bucket.iter().sum(),
            alive: player.is_alive(),
            won: player.flag(leader_flag::WON),
        })
    }

    pub fn reward_since(
        &self,
        who: u8,
        before: CoreRewardSnapshot,
    ) -> Result<CoreReward, ProjectionRefusal> {
        let now = self.reward_snapshot(who)?;
        Ok(CoreReward {
            score_delta: now.score - before.score,
            economy_delta: now.economy_total - before.economy_total,
            win: !before.won && now.won,
            loss: before.alive && !now.alive,
            alive: now.alive,
        })
    }
}

fn validate_scenario_movement_sources(
    setup: &AuthoritativeScenarioSpec,
) -> Result<(), ScenarioSetupError> {
    let units = setup.episode.units.len();
    let mut seen = vec![false; units];
    for (source_index, captured) in setup.movement_sources.iter().enumerate() {
        if captured.unit >= units {
            return Err(ScenarioSetupError::MovementSourceUnitOutOfRange {
                source: source_index,
                unit: captured.unit,
                units,
            });
        }
        if std::mem::replace(&mut seen[captured.unit], true) {
            return Err(ScenarioSetupError::DuplicateMovementSource {
                source: source_index,
                unit: captured.unit,
            });
        }
    }
    Ok(())
}

fn install_scenario_movement_sources(
    episode: &mut AuthoritativeEpisode,
    sources: &[ScenarioMovementSource],
) -> Result<(), ScenarioSetupError> {
    let spawned = episode.spawned().to_vec();
    for (source_index, captured) in sources.iter().enumerate() {
        let handle = spawned[captured.unit];
        episode
            .sim_mut_for_backend()
            .install_movement_collision_source(handle, captured.source.clone())
            .map_err(|fault| ScenarioSetupError::MovementSource {
                source: source_index,
                unit: captured.unit,
                fault,
            })?;
    }
    Ok(())
}

fn apply_visibility_type_flags(sim: &mut don_sim::tick::Sim, flags: &BTreeMap<i32, u32>) {
    for (&type_id, &unit_flags) in flags {
        let index = usize::try_from(type_id)
            .expect("validated visibility type id became negative across reset");
        sim.vic_leaders.types.rows[index].unit_flags = unit_flags as i32;
    }
}

fn apply_visibility_viewer_masks(sim: &mut don_sim::tick::Sim, masks: &BTreeMap<u8, u8>) {
    for (&viewer, &mask) in masks {
        sim.map.fog.leaders[usize::from(viewer)].player_mask = mask;
    }
}

fn capture_external_visibility_frame(
    sim: &don_sim::tick::Sim,
    type_sources: &BTreeMap<i32, u32>,
    viewer_sources: &BTreeMap<u8, u8>,
) -> Result<ExternalVisibilityFrame, VisibilityCaptureRefusal> {
    let mut rows = Vec::new();
    for row in 0..sim.world.live_count() as usize {
        if sim.world.units.get_flags(row) & OBJ_FLAG_ACTIVE == 0 {
            continue;
        }
        let handle = sim
            .world
            .handle_at_row(row)
            .ok_or(VisibilityCaptureRefusal::MissingHandle(row))?;
        let type_id = *sim
            .unit_type
            .get(row)
            .ok_or(VisibilityCaptureRefusal::MissingType(row))?;
        let captured_type_flags = *type_sources
            .get(&type_id)
            .ok_or(VisibilityCaptureRefusal::TypeFlagsUnavailable { row, type_id })?;
        let observed_type_flags = sim
            .vic_leaders
            .types
            .rows
            .get(usize::try_from(type_id).unwrap_or(usize::MAX))
            .map(|type_row| type_row.unit_flags as u32)
            .ok_or(VisibilityCaptureRefusal::TypeFlagsUnavailable { row, type_id })?;
        if observed_type_flags != captured_type_flags {
            return Err(VisibilityCaptureRefusal::TypeFlagsChanged {
                type_id,
                captured: captured_type_flags,
                observed: observed_type_flags,
            });
        }

        let x = sim.world.units.x_internal()[row];
        let y = sim.world.units.y_internal()[row];
        let fx = FCoord::from_coord(Coord(x)).0;
        let fy = FCoord::from_coord(Coord(y)).0;
        if !sim.map.world.valid_f(fx, fy) {
            return Err(VisibilityCaptureRefusal::InvalidFogCell { row, fx, fy });
        }
        let wx = WCoord::from_coord(Coord(x)).0;
        let wy = WCoord::from_coord(Coord(y)).0;
        if !sim.map.world.valid_w(wx, wy) {
            return Err(VisibilityCaptureRefusal::InvalidWorldCell { row, wx, wy });
        }
        let fog_index = sim.map.world.f_index(fx, fy);
        let visibility = RetailUnitVisibilityFacts {
            active: true,
            unit_masks: sim.world.units.get_unit_masks(row),
            type_unit_flags: captured_type_flags,
            unit_masks2: sim.world.units.get_unit_masks2(row),
            has_order: sim.world.orders(row).order_type() != OrderIndex::None,
            object_visible_mask: sim.world.units.visible()[row] as u8,
            cell_seen_mask: sim.map.world.seen[fog_index],
            cell_detected_mask: sim.map.world.seen3[fog_index],
            territory_owner: sim.map.world.wdata(wx, wy).who,
        };
        if visibility.is_cloaked() && visibility.unit_masks & UNIT_MASK_DETECTION_BYPASS == 0 {
            return Err(VisibilityCaptureRefusal::DetectionPlaneCompletenessUnavailable { row });
        }
        rows.push(ExternalUnitFrameRow {
            identity: ExternalEntityIdentity {
                handle,
                who: sim.world.units.get_who(row),
                object_o: sim.world.units.o()[row],
                uid: sim.world.units.get_uid(row),
            },
            public: ExternalEntityPublicState {
                type_id,
                x,
                y,
                hits: sim.world.units.myhits()[row],
                angle: sim.world.units.angle()[row],
                speed: sim.world.units.myspeed()[row],
                recharge: sim.world.units.get_recharging(row),
                order_index: sim.world.orders(row).order_type() as u16,
            },
            visibility,
        });
    }

    let mut viewers = Vec::new();
    for (who, player) in sim.vic_leaders.slots.iter().enumerate() {
        if !player.flag(leader_flag::VALID) {
            continue;
        }
        let viewer = who as u8;
        let captured_viewer_mask = *viewer_sources
            .get(&viewer)
            .ok_or(VisibilityCaptureRefusal::ViewerMaskUnavailable { viewer })?;
        let fog = sim.map.fog.leaders[who];
        if fog.player_mask != captured_viewer_mask {
            return Err(VisibilityCaptureRefusal::ViewerMaskChanged {
                viewer,
                captured: captured_viewer_mask,
                observed: fog.player_mask,
            });
        }
        let allied_territory_mask = (0..sim.vic_leaders.slots.len()).fold(0u8, |mask, other| {
            if sim.vic_leaders.is_ally(who, other) {
                mask | (1u8 << other)
            } else {
                mask
            }
        });
        viewers.push(RetailViewerFacts {
            who: viewer,
            vision_mask: captured_viewer_mask,
            see_all: fog.see_all,
            reveal_counter: fog.reveal_counter,
            see_own_territory: fog.see_own_territory,
            allied_territory_mask,
        });
    }

    Ok(ExternalVisibilityFrame {
        frame: sim.world.frame,
        fog_option: sim.map.fog.option.0,
        rows,
        viewers,
    })
}

fn validate_attack_execution_sources(
    unit_stats: &[UnitTypeStats],
) -> Result<(), AttackExecutionSourceRefusal> {
    let last_type_id = don_sim::balance::FIRST_UNIT_TYPE_ID + don_sim::balance::DIM as i32 - 1;
    for (index, stats) in unit_stats.iter().enumerate() {
        if !(don_sim::balance::FIRST_UNIT_TYPE_ID..=last_type_id).contains(&stats.type_id) {
            return Err(AttackExecutionSourceRefusal::InvalidTypeId {
                index,
                type_id: stats.type_id,
            });
        }
        if let Some(first) = unit_stats[..index]
            .iter()
            .position(|earlier| earlier.type_id == stats.type_id)
        {
            return Err(AttackExecutionSourceRefusal::DuplicateTypeId {
                first,
                second: index,
                type_id: stats.type_id,
            });
        }
    }
    Ok(())
}

fn apply_attack_execution_sources(sim: &mut don_sim::tick::Sim, sources: &AttackExecutionSources) {
    sim.world.rules.balance = Some(Arc::clone(&sources.balance));
    sim.world.rules.unit_stats = Arc::clone(&sources.unit_stats);
}

fn live_external_public_state(
    sim: &don_sim::tick::Sim,
    row: usize,
    type_id: i32,
) -> ExternalEntityPublicState {
    ExternalEntityPublicState {
        type_id,
        x: sim.world.units.x_internal()[row],
        y: sim.world.units.y_internal()[row],
        hits: sim.world.units.myhits()[row],
        angle: sim.world.units.angle()[row],
        speed: sim.world.units.myspeed()[row],
        recharge: sim.world.units.get_recharging(row),
        order_index: sim.world.orders(row).order_type() as u16,
    }
}

fn preflight_attack_execution(
    sim: &don_sim::tick::Sim,
    verb_index: usize,
    target: ExternalEntityIdentity,
    captured_target: ExternalEntityPublicState,
    actor_row: usize,
    target_row: usize,
) -> Result<AttackExecutionProof, ApplyRefusal> {
    let actor_type_id =
        *sim.unit_type
            .get(actor_row)
            .ok_or(ApplyRefusal::AttackExecutionUnavailable {
                verb_index,
                target,
                dependency: AttackExecutionDependencyRefusal::ActorTypeUnavailable {
                    row: actor_row,
                },
            })?;
    let target_type_id =
        *sim.unit_type
            .get(target_row)
            .ok_or(ApplyRefusal::AttackExecutionUnavailable {
                verb_index,
                target,
                dependency: AttackExecutionDependencyRefusal::TargetTypeUnavailable {
                    row: target_row,
                },
            })?;
    let observed_target = live_external_public_state(sim, target_row, target_type_id);
    if observed_target != captured_target {
        return Err(ApplyRefusal::AttackExecutionUnavailable {
            verb_index,
            target,
            dependency: AttackExecutionDependencyRefusal::TargetPublicStateChanged {
                captured: captured_target,
                observed: observed_target,
            },
        });
    }
    if observed_target.hits <= 0 {
        return Err(ApplyRefusal::AttackTargetIneligible {
            verb_index,
            target,
            reason: AttackTargetEligibilityRefusal::TargetNotAlive {
                hits: observed_target.hits,
            },
        });
    }

    let balance =
        sim.world
            .rules
            .balance
            .as_ref()
            .ok_or(ApplyRefusal::AttackExecutionUnavailable {
                verb_index,
                target,
                dependency: AttackExecutionDependencyRefusal::BalanceTableUnavailable,
            })?;
    let attacker = sim
        .world
        .rules
        .unit_stats
        .iter()
        .find(|stats| stats.type_id == actor_type_id)
        .copied()
        .ok_or(ApplyRefusal::AttackExecutionUnavailable {
            verb_index,
            target,
            dependency: AttackExecutionDependencyRefusal::AttackerTypeStatsUnavailable {
                type_id: actor_type_id,
            },
        })?;
    let defender = sim
        .world
        .rules
        .unit_stats
        .iter()
        .find(|stats| stats.type_id == target_type_id)
        .copied()
        .ok_or(ApplyRefusal::AttackExecutionUnavailable {
            verb_index,
            target,
            dependency: AttackExecutionDependencyRefusal::DefenderTypeStatsUnavailable {
                type_id: target_type_id,
            },
        })?;
    let balance_pct = balance.get(attacker.type_id, defender.type_id).ok_or(
        ApplyRefusal::AttackExecutionUnavailable {
            verb_index,
            target,
            dependency: AttackExecutionDependencyRefusal::BalanceEntryUnavailable {
                attacker_type_id: attacker.type_id,
                defender_type_id: defender.type_id,
            },
        },
    )?;

    let dx = observed_target.x - sim.world.units.x_internal()[actor_row];
    let dy = observed_target.y - sim.world.units.y_internal()[actor_row];
    let combat_rules = sim.world.rules.combat;
    let predicted_damage = don_sim::mechanics::damage(
        &don_sim::mechanics::DamageInput {
            balance_pct,
            attack: don_sim::mechanics::get_attack(attacker.attack, false, 0, 0),
            armor: don_sim::mechanics::get_armor(defender.armor, false, 0, 0),
            attack_dir: don_sim::trig::find_angle(dx, dy),
            attacker_player: u32::from(sim.world.units.get_who(actor_row)),
            attacker_type_id: attacker.type_id,
            defender_type_id: defender.type_id,
            defender_facing: observed_target.angle,
            defender_facing_entrench: observed_target.angle,
            current_frame: sim.world.frame,
            ..Default::default()
        },
        &don_sim::mechanics::DamagePredicates::default(),
        &combat_rules,
        &don_sim::mechanics::UnreachedTerms::default(),
    );
    if predicted_damage <= 0 {
        return Err(ApplyRefusal::AttackTargetIneligible {
            verb_index,
            target,
            reason: AttackTargetEligibilityRefusal::CannotHurt { predicted_damage },
        });
    }
    let shooter = sim
        .shooter_rules
        .iter()
        .find(|(type_id, _)| *type_id == attacker.type_id)
        .map(|(_, rules)| *rules);
    let recharge_frames = don_sim::systems::combat::recharge_frames(
        &don_sim::systems::combat::RechargeInput {
            base_recharge: attacker.recharge,
            ..Default::default()
        },
        &sim.combat_rules,
    );

    Ok(AttackExecutionProof {
        frame: sim.world.frame,
        actor_who: sim.world.units.get_who(actor_row),
        actor_x: sim.world.units.x_internal()[actor_row],
        actor_y: sim.world.units.y_internal()[actor_row],
        actor_speed: sim.world.units.myspeed()[actor_row],
        actor_recharge: sim.world.units.get_recharging(actor_row),
        actor_type_id,
        target_type_id,
        attacker,
        defender,
        balance_pct,
        combat_rules,
        recharge_rules: sim.combat_rules,
        shooter,
        predicted_damage,
        recharge_frames,
    })
}

fn prepare_attack_target_transaction(
    sim: &don_sim::tick::Sim,
    external_visibility: &ExternalEntityVisibilityOwner,
    episode_revision: u64,
    who: u8,
    request: UnitActionRequest,
) -> Result<PreparedAttackTargetTransaction, ApplyRefusal> {
    if request.verb_head == 0 {
        return Err(ApplyRefusal::UnknownVerb(0));
    }
    let verb_index = usize::from(request.verb_head - 1);
    let integration = UNIT_INTEGRATION
        .get(verb_index)
        .ok_or(ApplyRefusal::UnknownVerb(request.verb_head))?;
    if integration.route != VerbRoute::SimAttackIssue {
        return Err(match integration.route {
            VerbRoute::Refused(boundary) => ApplyRefusal::Unhosted {
                verb_index,
                boundary,
            },
            VerbRoute::SimIssue => ApplyRefusal::Unhosted {
                verb_index,
                boundary: IntegrationBoundary::CombatTargetHost,
            },
            VerbRoute::SimAttackIssue => unreachable!(),
        });
    }
    let player = sim
        .vic_leaders
        .slots
        .get(usize::from(who))
        .ok_or(ApplyRefusal::InvalidPlayer(who))?;
    if !player.is_alive() {
        return Err(ApplyRefusal::PlayerNotAlive(who));
    }
    let actor_row = sim
        .world
        .row_of(request.actor)
        .ok_or(ApplyRefusal::StaleActor(request.actor))?;
    let actual = sim.world.units.get_who(actor_row);
    if actual != who {
        return Err(ApplyRefusal::ActorNotOwned {
            actor: request.actor,
            expected: who,
            actual,
        });
    }
    if sim.world.units.get_flags(actor_row) & OBJ_FLAG_ACTIVE == 0 {
        return Err(ApplyRefusal::InactiveActor(request.actor));
    }
    if request.target_entity == 0 {
        return Err(ApplyRefusal::MissingTargetEntity { verb_index });
    }
    let binding = match external_visibility.bind_target(who, request.target_entity) {
        Ok(binding) => binding,
        Err(VisibilityProjectionFault::Uninstalled) => {
            return Err(ApplyRefusal::TargetIdentityVisibilityUnavailable {
                verb_index,
                target_entity: request.target_entity,
            });
        }
        Err(fault) => {
            return Err(ApplyRefusal::TargetVisibility {
                verb_index,
                target_entity: request.target_entity,
                fault,
            });
        }
    };
    let target = external_visibility
        .revalidate_target(binding)
        .map_err(|fault| ApplyRefusal::TargetVisibility {
            verb_index,
            target_entity: request.target_entity,
            fault,
        })?;
    let target_row =
        sim.world
            .row_of(target.identity.handle)
            .ok_or(ApplyRefusal::TargetVisibility {
                verb_index,
                target_entity: request.target_entity,
                fault: VisibilityProjectionFault::BindingIdentityChanged,
            })?;
    if sim.world.units.get_flags(target_row) & OBJ_FLAG_ACTIVE == 0
        || sim.world.units.get_who(target_row) != target.identity.who
        || sim.world.units.o()[target_row] != target.identity.object_o
        || sim.world.units.get_uid(target_row) != target.identity.uid
    {
        return Err(ApplyRefusal::TargetVisibility {
            verb_index,
            target_entity: request.target_entity,
            fault: VisibilityProjectionFault::BindingIdentityChanged,
        });
    }
    if actor_row == target_row || request.actor == target.identity.handle {
        return Err(ApplyRefusal::AttackTargetIneligible {
            verb_index,
            target: target.identity,
            reason: AttackTargetEligibilityRefusal::ActorIsTarget,
        });
    }
    if usize::from(target.identity.who) >= sim.vic_leaders.slots.len() {
        return Err(ApplyRefusal::AttackTargetIneligible {
            verb_index,
            target: target.identity,
            reason: AttackTargetEligibilityRefusal::RelationUnavailable {
                owner: target.identity.who,
            },
        });
    }
    let relation = sim
        .vic_leaders
        .get_diplo(usize::from(who), usize::from(target.identity.who));
    if relation != Diplo::War {
        return Err(ApplyRefusal::AttackTargetIneligible {
            verb_index,
            target: target.identity,
            reason: AttackTargetEligibilityRefusal::NotEnemy { relation },
        });
    }
    if request.queue != QueuePosition::Replace {
        return Err(ApplyRefusal::UnsupportedQueue(request.queue));
    }
    if request.order_flags != 0 {
        return Err(ApplyRefusal::UnsupportedOrderFlags(request.order_flags));
    }
    let execution = preflight_attack_execution(
        sim,
        verb_index,
        target.identity,
        target.public,
        actor_row,
        target_row,
    )?;

    Ok(PreparedAttackTargetTransaction {
        who,
        request,
        episode_revision,
        actor_row,
        target_row,
        binding,
        target: target.identity,
        target_public: target.public,
        relation,
        execution,
    })
}

fn validate_prepared_attack_target(
    sim: &don_sim::tick::Sim,
    external_visibility: &ExternalEntityVisibilityOwner,
    prepared: PreparedAttackTargetTransaction,
) -> Result<(), ApplyRefusal> {
    let verb_index = crate::generated::uv::ATTACK;
    external_visibility
        .revalidate_target(prepared.binding)
        .map_err(|fault| ApplyRefusal::TargetVisibility {
            verb_index,
            target_entity: prepared.request.target_entity,
            fault,
        })?;
    let current = prepare_attack_target_transaction(
        sim,
        external_visibility,
        prepared.episode_revision,
        prepared.who,
        prepared.request,
    )?;
    if current != prepared {
        return Err(ApplyRefusal::TargetVisibility {
            verb_index,
            target_entity: prepared.request.target_entity,
            fault: VisibilityProjectionFault::BindingIdentityChanged,
        });
    }
    debug_assert_eq!(current.actor_row, prepared.actor_row);
    debug_assert_eq!(current.target_row, prepared.target_row);
    Ok(())
}

/// Read-only half of the action transaction, shared byte-for-byte by masking and apply.
fn preflight_unit(
    sim: &don_sim::tick::Sim,
    external_visibility: &ExternalEntityVisibilityOwner,
    episode_revision: u64,
    who: u8,
    request: UnitActionRequest,
) -> Result<Option<PreparedUnitAction>, ApplyRefusal> {
    if request.verb_head == 0 {
        return Ok(None);
    }
    let verb_index = usize::from(request.verb_head - 1);
    let integration = UNIT_INTEGRATION
        .get(verb_index)
        .ok_or(ApplyRefusal::UnknownVerb(request.verb_head))?;
    if let VerbRoute::Refused(boundary) = integration.route {
        return Err(ApplyRefusal::Unhosted {
            verb_index,
            boundary,
        });
    }

    let player = sim
        .vic_leaders
        .slots
        .get(usize::from(who))
        .ok_or(ApplyRefusal::InvalidPlayer(who))?;
    if !player.is_alive() {
        return Err(ApplyRefusal::PlayerNotAlive(who));
    }
    let row = sim
        .world
        .row_of(request.actor)
        .ok_or(ApplyRefusal::StaleActor(request.actor))?;
    let actual = sim.world.units.get_who(row);
    if actual != who {
        return Err(ApplyRefusal::ActorNotOwned {
            actor: request.actor,
            expected: who,
            actual,
        });
    }
    if sim.world.units.get_flags(row) & OBJ_FLAG_ACTIVE == 0 {
        return Err(ApplyRefusal::InactiveActor(request.actor));
    }
    if integration.route == VerbRoute::SimAttackIssue {
        let prepared = prepare_attack_target_transaction(
            sim,
            external_visibility,
            episode_revision,
            who,
            request,
        )?;
        return Err(ApplyRefusal::AttackTargetCommitUnavailable {
            verb_index,
            target: prepared.target,
            boundary: IntegrationBoundary::CombatTargetHost,
        });
    }
    debug_assert_eq!(integration.route, VerbRoute::SimIssue);
    if request.queue != QueuePosition::Replace {
        return Err(ApplyRefusal::UnsupportedQueue(request.queue));
    }
    if request.order_flags & !ORDER_FLEEING != 0 {
        return Err(ApplyRefusal::UnsupportedOrderFlags(request.order_flags));
    }
    if !sim
        .map
        .world
        .valid_coord(request.target_x, request.target_y)
    {
        return Err(ApplyRefusal::InvalidDestination {
            x: request.target_x,
            y: request.target_y,
        });
    }

    sim.movement_collision
        .preflight(&sim.world, &sim.map.world, &sim.paths)
        .map_err(ApplyRefusal::MovementHost)?;
    sim.movement_collision
        .actor_ready(&sim.world, row)
        .map_err(ApplyRefusal::MovementHost)?;
    let kind = if request.order_flags & ORDER_FLEEING != 0 {
        OrderIndex::FleeTo
    } else {
        OrderIndex::MoveTo
    };
    let source = sim
        .movement_source_state(request.actor)
        .map_err(ApplyRefusal::MovementHost)?;
    debug_assert_eq!(source.row, row);
    Ok(Some(PreparedUnitAction {
        who,
        request,
        episode_revision,
        row,
        kind,
        source,
    }))
}
