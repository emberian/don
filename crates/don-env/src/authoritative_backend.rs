//! Side-by-side fidelity backend over [`AuthoritativeEpisode`].
//!
//! This is deliberately narrower than `VecEnv`: it proves that policy actions,
//! observations, rewards, and ticks can share one [`don_sim::tick::Sim`] owner without
//! replacing the compact high-throughput backend in the same change. Every generated verb
//! has a frozen route below. Only `MOVE_TO` is admitted today, and only when the live
//! collision host proves every active object needed by the transaction. Unsupported verbs
//! fail before mutation; there is no accepted-no-effect result.

use crate::authoritative_episode::{AuthoritativeEpisode, EpisodeError, ScenarioSpec, StepReceipt};
use don_sim::order::{Order, OrderIndex, ORDER_FLEEING};
use don_sim::systems::map_terrain::{Coord, FCoord};
use don_sim::systems::movement_live::{LiveCollisionFault, LiveCollisionSource};
use don_sim::systems::victory_score::{leader_flag, Diplo};
use don_sim::world::OBJ_FLAG_ACTIVE;
use don_sim::Handle;

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
    /// Single selected unit -> `Sim::issue` -> retail-ordered `Sim::do_frame`.
    SimIssue,
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
    refused!("ATTACK", 4, CombatTargetHost),
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

/// Raw core-coordinate request produced after the policy head decoder resolves grid cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnitActionRequest {
    /// Generated unit Verb head: 0 is NOOP, `n + 1` indexes `UNIT_INTEGRATION[n]`.
    pub verb_head: u16,
    pub actor: Handle,
    pub target_x: i32,
    pub target_y: i32,
    pub queue: QueuePosition,
    pub order_flags: u8,
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
    InvalidDestination {
        x: i32,
        y: i32,
    },
    UnsupportedQueue(QueuePosition),
    UnsupportedOrderFlags(u8),
    MovementHost(LiveCollisionFault),
    MovementSourceState {
        actor: Handle,
        expected_action: i32,
        observed_action: i32,
        moving: bool,
    },
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

pub struct AuthoritativeBackend {
    episode: AuthoritativeEpisode,
}

impl AuthoritativeBackend {
    pub fn from_spec(spec: ScenarioSpec) -> Result<Self, EpisodeError> {
        Ok(Self {
            episode: AuthoritativeEpisode::from_spec(spec)?,
        })
    }

    pub fn reset(&mut self) -> Result<(), EpisodeError> {
        self.episode.reset()
    }

    pub fn step_frames(&mut self, frames: u32) -> StepReceipt {
        self.episode.step_frames(frames)
    }

    pub fn sim(&self) -> &don_sim::tick::Sim {
        self.episode.sim()
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

    pub fn apply_unit(
        &mut self,
        who: u8,
        request: UnitActionRequest,
    ) -> Result<ApplyReceipt, ApplyRefusal> {
        let sim = self.episode.sim_mut_for_backend();
        if request.verb_head == 0 {
            return Ok(ApplyReceipt::Noop {
                frame: sim.world.frame,
            });
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
            .movement_collision
            .source(row)
            .ok_or(ApplyRefusal::MovementHost(
                LiveCollisionFault::MissingSource(row),
            ))?;
        if !source.moving || source.action != kind as i32 {
            return Err(ApplyRefusal::MovementSourceState {
                actor: request.actor,
                expected_action: kind as i32,
                observed_action: source.action,
                moving: source.moving,
            });
        }

        let order = Order {
            kind,
            flags: request.order_flags,
            x: request.target_x,
            y: request.target_y,
            tolerance: 0,
            ..Order::default()
        };
        if !sim.issue(request.actor, order) {
            return Err(ApplyRefusal::CoreRejectedAfterPreflight);
        }
        Ok(ApplyReceipt::OrderInstalled {
            frame: sim.world.frame,
            actor: request.actor,
            kind,
            queue_len: sim.world.orders(row).len(),
        })
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
            VerbRoute::SimIssue => Err(ApplyRefusal::CoreRejectedAfterPreflight),
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
