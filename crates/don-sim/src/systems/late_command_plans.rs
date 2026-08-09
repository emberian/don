//! Fail-closed plans for two fixed-size commands near the end of `CommandTypes`.
//!
//! This module is intentionally not wired into `command.rs`.  It isolates the complete
//! deterministic portion of these red opcode rows while keeping UI, audio, and network-owned
//! work as typed effects:
//!
//! * opcode 77, `CommandPackage::process_cannon_time` `0x009464F0`, followed by the
//!   state-writing portion of `TurnControl::start_cannon_time` `0x00956500`;
//! * opcode 80, `CommandPackage::process_ungraceful_player_drop` `0x00943EA0`.
//!
//! Constants and branch order below come from linear x86 disassembly of the supported
//! Extended Edition executable.  PDB layouts establish the wire fields.  A `Planned`
//! receipt means only that the pure plan recomputes exactly; an adapter must still execute
//! every typed effect atomically.  In particular, opcode 80 remains incomplete until the
//! `DropControl::process_drop` tail at `0x00959500` has its own recovered transaction.

pub const CANNON_TIME_OPCODE: u8 = 77;
pub const CANNON_TIME_WIRE_BYTES: usize = 2;
pub const UNGRACEFUL_DROP_OPCODE: u8 = 80;
pub const UNGRACEFUL_DROP_WIRE_BYTES: usize = 3;

/// `SoundGlobalCat` values passed by `TurnControl::start_cannon_time`.
pub const SOUND_CANNON_DENIED: i32 = 0x40;
pub const SOUND_CANNON_STARTED: i32 = 0x139;
pub const SOUND_CANNON_SPEED_CHANGED: i32 = 0x156;

/// The wall-clock target passed to `TurnControl::set_new_target_time` after forcing speed
/// zero.  This is a pacing/UI boundary, not headless simulation state.
pub const CANNON_TARGET_TIME: u32 = 200;

/// `Game +0x820` bit tested by opcode 80 before entering `DropControl`.
pub const GAME_SEM_NETWORK_DROP: u8 = 0x10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlanStatus {
    Planned,
    Unavailable,
}

// ---------------------------------------------------------------------------
// Opcode 77: CannonTimeCommand
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CannonTimeRequest {
    /// Signed `CommandPackage::play`.  Retail uses this to read the issuing `Player` row.
    pub package_play: i32,
    /// Raw byte at wire `+1`.  The handler logs it but does not pass it to
    /// `start_cannon_time`; retaining it prevents a diagnostic/presentation alias.
    pub wire_state: u8,
}

pub fn decode_cannon_time(package_play: i32, wire: &[u8]) -> Option<CannonTimeRequest> {
    if wire.len() != CANNON_TIME_WIRE_BYTES || wire.first().copied()? != CANNON_TIME_OPCODE {
        return None;
    }
    Some(CannonTimeRequest {
        package_play,
        wire_state: wire[1],
    })
}

/// State at `TurnControl +0x24/+0x28/+0x2C/+0x30/+0x44`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CannonTimeState {
    pub active_player: i32,
    pub start_frame: i32,
    pub saved_speed: i32,
    pub current_speed: i32,
    pub cannon_time_start: i32,
}

/// Host reads needed by the handler and the reached branch of `start_cannon_time`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CannonTimeFacts {
    /// Zero-extended `PlayerData::who` byte at player-row `+0x77`.
    pub package_player_who: Option<u8>,
    /// `Console::display_play` at `+0x298`.
    pub display_who: i32,
    /// Signed simulation frame from `Game +0x550`.
    pub frame: i32,
    /// State copied from `Game +0x560` into `TurnControl +0x44` on success.
    pub game_cannon_time_start: i32,
    /// `LeaderData::cannon_time` for `package_player_who`.  It is not read when another
    /// cannon-time interval is already active.
    pub remaining_uses: Option<u32>,
    pub state: CannonTimeState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CannonTimeUi {
    AlreadyActive,
    NoUsesRemaining,
    StartedLocal,
    StartedRemote { who: u8 },
    SpeedChangedToZero,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CannonTimeEffect {
    Diagnostic {
        wire_state: u8,
        frame: i32,
    },
    SetRemainingUses {
        who: u8,
        value: u32,
    },
    SetActivePlayer(i32),
    SetStartFrame(i32),
    SetSavedSpeed(i32),
    SetCannonTimeStart(i32),
    SetCurrentSpeed(i32),
    Ui(CannonTimeUi),
    /// Ordered `SoundGlobal::play(category)` request.  The adapter must execute this
    /// through the retail-compatible sound RNG stream; planning consumes no draw.
    Audio {
        category: i32,
    },
    /// Wall-clock pacing callback after speed becomes zero.
    RetargetWallClock {
        target_time: u32,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CannonTimePlan {
    pub state: CannonTimeState,
    /// New charge only on a successful start.
    pub remaining_uses: Option<u32>,
    pub effects: Vec<CannonTimeEffect>,
}

pub fn plan_cannon_time(
    request: &CannonTimeRequest,
    facts: &CannonTimeFacts,
) -> Option<CannonTimePlan> {
    let who = facts.package_player_who?;
    let mut state = facts.state;
    let mut effects = vec![CannonTimeEffect::Diagnostic {
        wire_state: request.wire_state,
        frame: facts.frame,
    }];

    if state.active_player >= 0 {
        if i32::from(who) == facts.display_who {
            effects.push(CannonTimeEffect::Ui(CannonTimeUi::AlreadyActive));
            effects.push(CannonTimeEffect::Audio {
                category: SOUND_CANNON_DENIED,
            });
        }
        return Some(CannonTimePlan {
            state,
            remaining_uses: None,
            effects,
        });
    }

    let uses = facts.remaining_uses?;
    if uses == 0 {
        if i32::from(who) == facts.display_who {
            effects.push(CannonTimeEffect::Ui(CannonTimeUi::NoUsesRemaining));
            effects.push(CannonTimeEffect::Audio {
                category: SOUND_CANNON_DENIED,
            });
        }
        return Some(CannonTimePlan {
            state,
            remaining_uses: None,
            effects,
        });
    }

    let new_uses = uses.wrapping_sub(1);
    effects.push(CannonTimeEffect::SetRemainingUses {
        who,
        value: new_uses,
    });
    state.active_player = i32::from(who);
    effects.push(CannonTimeEffect::SetActivePlayer(state.active_player));
    state.start_frame = facts.frame;
    effects.push(CannonTimeEffect::SetStartFrame(state.start_frame));
    state.saved_speed = state.current_speed;
    effects.push(CannonTimeEffect::SetSavedSpeed(state.saved_speed));
    state.cannon_time_start = facts.game_cannon_time_start;
    effects.push(CannonTimeEffect::SetCannonTimeStart(
        state.cannon_time_start,
    ));

    effects.push(CannonTimeEffect::Ui(
        if i32::from(who) == facts.display_who {
            CannonTimeUi::StartedLocal
        } else {
            CannonTimeUi::StartedRemote { who }
        },
    ));
    effects.push(CannonTimeEffect::Audio {
        category: SOUND_CANNON_STARTED,
    });

    if state.current_speed != 0 {
        state.current_speed = 0;
        effects.push(CannonTimeEffect::SetCurrentSpeed(0));
        effects.push(CannonTimeEffect::RetargetWallClock {
            target_time: CANNON_TARGET_TIME,
        });
        effects.push(CannonTimeEffect::Ui(CannonTimeUi::SpeedChangedToZero));
        effects.push(CannonTimeEffect::Audio {
            category: SOUND_CANNON_SPEED_CHANGED,
        });
    }

    Some(CannonTimePlan {
        state,
        remaining_uses: Some(new_uses),
        effects,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CannonTimeReceipt {
    pub request: CannonTimeRequest,
    pub facts: Option<CannonTimeFacts>,
    pub status: PlanStatus,
    pub plan: Option<CannonTimePlan>,
}

impl CannonTimeReceipt {
    pub fn unavailable(request: CannonTimeRequest) -> Self {
        Self {
            request,
            facts: None,
            status: PlanStatus::Unavailable,
            plan: None,
        }
    }

    pub fn validates(&self, expected: &CannonTimeRequest) -> bool {
        if &self.request != expected {
            return false;
        }
        match self.status {
            PlanStatus::Unavailable => self.facts.is_none() && self.plan.is_none(),
            PlanStatus::Planned => {
                let (Some(facts), Some(observed)) = (self.facts.as_ref(), self.plan.as_ref())
                else {
                    return false;
                };
                plan_cannon_time(expected, facts).is_some_and(|plan| plan == *observed)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Opcode 80: UngracefulPlayerDrop
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UngracefulDropRequest {
    /// Both fields are zero-extended bytes at the call to `DropControl::process_drop`.
    pub play: u8,
    pub state: u8,
}

pub fn decode_ungraceful_drop(wire: &[u8]) -> Option<UngracefulDropRequest> {
    if wire.len() != UNGRACEFUL_DROP_WIRE_BYTES || wire.first().copied()? != UNGRACEFUL_DROP_OPCODE
    {
        return None;
    }
    Some(UngracefulDropRequest {
        play: wire[1],
        state: wire[2],
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UngracefulDropEffect {
    Diagnostic {
        play: u8,
        state: u8,
    },
    /// Exact typed network boundary.  Its downstream state transition is deliberately not
    /// flattened into this opcode planner.
    DelegateToDropControl {
        play: u8,
        state: u8,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UngracefulDropPlan {
    pub effects: Vec<UngracefulDropEffect>,
    /// False is a measured simulation no-op after the diagnostic; true still requires the
    /// unrecovered DropControl receipt before opcode 80 can be closure-green.
    pub downstream_required: bool,
}

pub fn plan_ungraceful_drop(
    request: UngracefulDropRequest,
    game_semaphore: u8,
) -> UngracefulDropPlan {
    let mut effects = vec![UngracefulDropEffect::Diagnostic {
        play: request.play,
        state: request.state,
    }];
    let network_mode = game_semaphore & GAME_SEM_NETWORK_DROP != 0;
    if network_mode {
        effects.push(UngracefulDropEffect::DelegateToDropControl {
            play: request.play,
            state: request.state,
        });
    }
    UngracefulDropPlan {
        effects,
        downstream_required: network_mode,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UngracefulDropReceipt {
    pub request: UngracefulDropRequest,
    /// Raw `Game +0x820` byte; validation recomputes the `0x10` gate.
    pub game_semaphore: Option<u8>,
    pub status: PlanStatus,
    pub plan: Option<UngracefulDropPlan>,
}

impl UngracefulDropReceipt {
    pub fn validates(&self, expected: UngracefulDropRequest) -> bool {
        if self.request != expected {
            return false;
        }
        match self.status {
            PlanStatus::Unavailable => self.game_semaphore.is_none() && self.plan.is_none(),
            PlanStatus::Planned => {
                let (Some(game_semaphore), Some(observed)) =
                    (self.game_semaphore, self.plan.as_ref())
                else {
                    return false;
                };
                plan_ungraceful_drop(expected, game_semaphore) == *observed
            }
        }
    }
}
