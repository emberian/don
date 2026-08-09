//! Side-by-side fidelity backend over [`AuthoritativeEpisode`].
//!
//! This is deliberately narrower than `VecEnv`: it proves that policy actions,
//! observations, rewards, and ticks can share one [`don_sim::tick::Sim`] owner without
//! replacing the compact high-throughput backend in the same change. Every generated verb
//! has a frozen route below. Only `MOVE_TO` is admitted today, and only when the live
//! collision host proves every active object needed by the transaction. `ATTACK` has a
//! production Sim issue/execution route, but remains masked and refused until a policy target
//! ordinal can be bound to a cloak/detection-aware external observation. Unsupported verbs
//! fail before mutation; there is no accepted-no-effect result.

use crate::authoritative_episode::{AuthoritativeEpisode, EpisodeError, ScenarioSpec, StepReceipt};
use don_sim::order::{Order, OrderIndex, ORDER_FLEEING};
use don_sim::systems::map_terrain::{Coord, FCoord};
use don_sim::systems::movement_live::{
    LiveCollisionFault, LiveCollisionSource, MovementSourceState,
};
use don_sim::systems::victory_score::{leader_flag, Diplo};
use don_sim::world::{OBJ_FLAG_ACTIVE, SUBTILE};
use don_sim::Handle;
use std::fmt;

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
    /// The production Sim owns attack order installation and execution. Policy entry remains
    /// fail-closed at the target identity/visibility projection boundary.
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
    /// Explicitly false until external entities pass current visibility plus cloak detection.
    pub external_entities_complete: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectionRefusal {
    InvalidPlayer(u8),
    PlayerNotInGame(u8),
    MissingHandle(usize),
    MissingType(usize),
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
}

impl AuthoritativeBackend {
    pub fn from_spec(spec: ScenarioSpec) -> Result<Self, EpisodeError> {
        Ok(Self {
            episode: AuthoritativeEpisode::from_spec(spec)?,
            episode_revision: 0,
            scenario_movement_sources: Vec::new(),
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
        })
    }

    /// Rebuild seed, allocation identity, and captured movement sources as one replacement.
    /// The current episode remains intact if construction or source installation is refused.
    pub fn reset(&mut self) -> Result<(), ScenarioSetupError> {
        let mut replacement = Self::from_authoritative_scenario(AuthoritativeScenarioSpec {
            episode: self.episode.spec().clone(),
            movement_sources: self.scenario_movement_sources.clone(),
        })?;
        replacement.episode_revision = self.episode_revision.wrapping_add(1);
        *self = replacement;
        Ok(())
    }

    pub fn step_frames(&mut self, frames: u32) -> StepReceipt {
        self.episode.step_frames(frames)
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
            *slot = preflight_unit(self.episode.sim(), self.episode_revision, who, request).is_ok();
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
        preflight_unit(self.episode.sim(), self.episode_revision, who, request)
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
        Ok(ApplyReceipt::OrderInstalled {
            frame: sim.world.frame,
            actor: prepared.request.actor,
            kind: prepared.kind,
            queue_len: sim.world.orders(prepared.row).len(),
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

    /// Own-state-only projection. The exact fog query is already available in Sim, but
    /// external observation also needs cloaking/type facts not yet stored by this owner.
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
            external_entities_complete: false,
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

/// Read-only half of the action transaction, shared byte-for-byte by masking and apply.
fn preflight_unit(
    sim: &don_sim::tick::Sim,
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
        if request.target_entity == 0 {
            return Err(ApplyRefusal::MissingTargetEntity { verb_index });
        }
        // `observe()` currently exposes own rows only. An ATTACK target must be external, and
        // fog alone cannot prove it policy-visible: retail additionally evaluates dynamic
        // object cloak flags, type cloak flags and the dedicated detection plane. Preserve the
        // ordinal for the future binding transaction, but never resolve it through scenario
        // allocation order or an omniscient World walk.
        return Err(ApplyRefusal::TargetIdentityVisibilityUnavailable {
            verb_index,
            target_entity: request.target_entity,
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
