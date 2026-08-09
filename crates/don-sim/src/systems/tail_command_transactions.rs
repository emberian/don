// SPDX-License-Identifier: GPL-3.0-or-later
//! Branch-complete transactions for the five remaining non-group command rows.
//!
//! Rows 70/71 always cross player lifecycle tails. Rows 73/78/80 have exact no-tail
//! branches which may apply atomically, but each row also has a branch whose downstream
//! cascade, console parser, or drop-control body is still open. The row-level closure must
//! therefore remain red even when an individual request returns [`TailDecision::Apply`].

#[path = "adjacent_command_prefixes.rs"]
pub mod adjacent;
#[path = "late_command_plans.rs"]
pub mod late;

use self::adjacent::{
    AdjacentOpenTailRequest, AdjacentPrefixPresentationReceipt, ConsoleCommand,
    LeaderOptionDataState, LeaderOptionRowReceipt, LeaderOptionsCommand,
};
use self::late::{UngracefulDropEffect, UngracefulDropRequest};

pub const RESIGN_OPCODE: u8 = 70;
pub const RESIGN_WIRE_BYTES: usize = 5;
pub const QUIT_OPCODE: u8 = 71;
pub const QUIT_WIRE_BYTES: usize = 7;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResignRequest {
    pub play: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QuitRequest {
    pub play: i32,
    pub replay: u8,
    pub system_quit: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TailCommandRequest {
    Resign(ResignRequest),
    Quit(QuitRequest),
    LeaderOptions(LeaderOptionsCommand),
    ConsoleCommand(ConsoleCommand),
    UngracefulDrop(UngracefulDropRequest),
}

impl TailCommandRequest {
    pub fn opcode(&self) -> u8 {
        match self {
            Self::Resign(_) => RESIGN_OPCODE,
            Self::Quit(_) => QUIT_OPCODE,
            Self::LeaderOptions(_) => adjacent::LEADER_OPTIONS_OPCODE,
            Self::ConsoleCommand(_) => adjacent::CONSOLE_COMMAND_OPCODE,
            Self::UngracefulDrop(_) => late::UNGRACEFUL_DROP_OPCODE,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TailDecodeError {
    UnsupportedOpcode(u8),
    WrongLength {
        opcode: u8,
        expected: usize,
        actual: usize,
    },
    Adjacent(adjacent::AdjacentCommandDecodeError),
}

#[inline]
fn i32_at(bytes: &[u8], offset: usize) -> i32 {
    i32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("fixed slice"))
}

pub fn decode_tail_command(bytes: &[u8]) -> Result<TailCommandRequest, TailDecodeError> {
    let opcode = bytes.first().copied().unwrap_or(u8::MAX);
    match opcode {
        RESIGN_OPCODE => {
            if bytes.len() != RESIGN_WIRE_BYTES {
                return Err(TailDecodeError::WrongLength {
                    opcode,
                    expected: RESIGN_WIRE_BYTES,
                    actual: bytes.len(),
                });
            }
            Ok(TailCommandRequest::Resign(ResignRequest {
                play: i32_at(bytes, 1),
            }))
        }
        QUIT_OPCODE => {
            if bytes.len() != QUIT_WIRE_BYTES {
                return Err(TailDecodeError::WrongLength {
                    opcode,
                    expected: QUIT_WIRE_BYTES,
                    actual: bytes.len(),
                });
            }
            Ok(TailCommandRequest::Quit(QuitRequest {
                play: i32_at(bytes, 1),
                replay: bytes[5],
                system_quit: bytes[6],
            }))
        }
        adjacent::LEADER_OPTIONS_OPCODE => adjacent::decode_leader_options(bytes)
            .map(TailCommandRequest::LeaderOptions)
            .map_err(TailDecodeError::Adjacent),
        adjacent::CONSOLE_COMMAND_OPCODE => adjacent::decode_console_command(bytes)
            .map(TailCommandRequest::ConsoleCommand)
            .map_err(TailDecodeError::Adjacent),
        late::UNGRACEFUL_DROP_OPCODE => late::decode_ungraceful_drop(bytes)
            .map(TailCommandRequest::UngracefulDrop)
            .ok_or(TailDecodeError::WrongLength {
                opcode,
                expected: late::UNGRACEFUL_DROP_WIRE_BYTES,
                actual: bytes.len(),
            }),
        _ => Err(TailDecodeError::UnsupportedOpcode(opcode)),
    }
}

/// Host-owned facts read before the first possible mutation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TailCommandFacts {
    NoExternalFacts,
    LeaderOptions {
        previous: LeaderOptionRowReceipt,
        console_play: i32,
    },
    ConsoleCommand {
        console_present: bool,
    },
    UngracefulDrop {
        game_semaphore: u8,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TailPresentationReceipt {
    ResignDiagnostic {
        play: i32,
    },
    QuitDiagnostic {
        play: i32,
        replay: u8,
        system_quit: u8,
    },
    Adjacent(AdjacentPrefixPresentationReceipt),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TailEffect {
    Presentation(TailPresentationReceipt),
    StoreLeaderOptions {
        who: i32,
        state: LeaderOptionDataState,
    },
    UngracefulDrop(UngracefulDropEffect),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TailOpenBoundary {
    /// `Player::resign(0)` including `leave_game` / `Leader::defeat` continuations.
    PlayerResign(ResignRequest),
    /// The pre-quit gate, `Player::quit(0)`, report generation, and system-quit callback
    /// must commit together; no restart flag prefix is authorized on its own.
    PlayerQuit(QuitRequest),
    LeaderOptions(AdjacentOpenTailRequest),
    ConsoleCommand(AdjacentOpenTailRequest),
    DropControlProcessDrop(UngracefulDropRequest),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TailCommandPlan {
    pub request: TailCommandRequest,
    pub effects: Vec<TailEffect>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TailBoundaryPlan {
    pub request: TailCommandRequest,
    /// Observations only. A boundary decision authorizes no prefix mutation.
    pub prefix: Vec<TailEffect>,
    pub boundary: TailOpenBoundary,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TailDecision {
    Apply(TailCommandPlan),
    Boundary(TailBoundaryPlan),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TailPlanError {
    FactsMismatch { opcode: u8 },
    LeaderOptions(adjacent::LeaderOptionsPrefixError),
}

pub fn plan_tail_command(
    request: &TailCommandRequest,
    facts: &TailCommandFacts,
) -> Result<TailDecision, TailPlanError> {
    match (request, facts) {
        (TailCommandRequest::Resign(request), TailCommandFacts::NoExternalFacts) => {
            let effect = TailEffect::Presentation(TailPresentationReceipt::ResignDiagnostic {
                play: request.play,
            });
            Ok(TailDecision::Boundary(TailBoundaryPlan {
                request: TailCommandRequest::Resign(*request),
                prefix: vec![effect],
                boundary: TailOpenBoundary::PlayerResign(*request),
            }))
        }
        (TailCommandRequest::Quit(request), TailCommandFacts::NoExternalFacts) => {
            let effect = TailEffect::Presentation(TailPresentationReceipt::QuitDiagnostic {
                play: request.play,
                replay: request.replay,
                system_quit: request.system_quit,
            });
            Ok(TailDecision::Boundary(TailBoundaryPlan {
                request: TailCommandRequest::Quit(*request),
                prefix: vec![effect],
                boundary: TailOpenBoundary::PlayerQuit(*request),
            }))
        }
        (
            TailCommandRequest::LeaderOptions(command),
            TailCommandFacts::LeaderOptions {
                previous,
                console_play,
            },
        ) => {
            let plan =
                adjacent::plan_leader_options_prefix(command, previous.clone(), *console_play)
                    .map_err(TailPlanError::LeaderOptions)?;
            let effects = vec![
                TailEffect::Presentation(TailPresentationReceipt::Adjacent(
                    plan.presentation.clone(),
                )),
                TailEffect::StoreLeaderOptions {
                    who: command.data.who,
                    state: plan.stored,
                },
            ];
            let request = TailCommandRequest::LeaderOptions(*command);
            Ok(match plan.open_tail {
                Some(boundary) => TailDecision::Boundary(TailBoundaryPlan {
                    request,
                    prefix: effects,
                    boundary: TailOpenBoundary::LeaderOptions(boundary),
                }),
                None => TailDecision::Apply(TailCommandPlan { request, effects }),
            })
        }
        (
            TailCommandRequest::ConsoleCommand(command),
            TailCommandFacts::ConsoleCommand { console_present },
        ) => {
            let plan = adjacent::plan_console_command_prefix(command, *console_present);
            let effects = plan
                .presentation
                .into_iter()
                .map(|receipt| TailEffect::Presentation(TailPresentationReceipt::Adjacent(receipt)))
                .collect();
            let request = TailCommandRequest::ConsoleCommand(command.clone());
            Ok(match plan.open_tail {
                Some(boundary) => TailDecision::Boundary(TailBoundaryPlan {
                    request,
                    prefix: effects,
                    boundary: TailOpenBoundary::ConsoleCommand(boundary),
                }),
                None => TailDecision::Apply(TailCommandPlan { request, effects }),
            })
        }
        (
            TailCommandRequest::UngracefulDrop(request),
            TailCommandFacts::UngracefulDrop { game_semaphore },
        ) => {
            let plan = late::plan_ungraceful_drop(*request, *game_semaphore);
            let effects = plan
                .effects
                .into_iter()
                .map(TailEffect::UngracefulDrop)
                .collect();
            let request_value = TailCommandRequest::UngracefulDrop(*request);
            Ok(if plan.downstream_required {
                TailDecision::Boundary(TailBoundaryPlan {
                    request: request_value,
                    prefix: effects,
                    boundary: TailOpenBoundary::DropControlProcessDrop(*request),
                })
            } else {
                TailDecision::Apply(TailCommandPlan {
                    request: request_value,
                    effects,
                })
            })
        }
        _ => Err(TailPlanError::FactsMismatch {
            opcode: request.opcode(),
        }),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TailTransactionStatus {
    Applied,
    Unavailable,
}

/// Atomic receipt. Only a recomputable no-tail `Apply` decision can validate as applied.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TailCommandReceipt {
    pub request: TailCommandRequest,
    pub status: TailTransactionStatus,
    pub facts: Option<TailCommandFacts>,
    pub plan: Option<TailCommandPlan>,
}

impl TailCommandReceipt {
    pub fn unavailable(request: TailCommandRequest) -> Self {
        Self {
            request,
            status: TailTransactionStatus::Unavailable,
            facts: None,
            plan: None,
        }
    }

    pub fn validates(&self, expected: &TailCommandRequest) -> bool {
        if &self.request != expected {
            return false;
        }
        match self.status {
            TailTransactionStatus::Unavailable => self.facts.is_none() && self.plan.is_none(),
            TailTransactionStatus::Applied => {
                let (Some(facts), Some(observed)) = (self.facts.as_ref(), self.plan.as_ref())
                else {
                    return false;
                };
                matches!(
                    plan_tail_command(expected, facts),
                    Ok(TailDecision::Apply(plan)) if plan == *observed
                )
            }
        }
    }

    /// Bind an applied receipt to the exact preflight snapshot supplied by the bridge.
    /// Unavailable receipts carry no facts and remain valid identity-preserving refusals.
    pub fn validates_for(
        &self,
        expected: &TailCommandRequest,
        supplied_facts: &TailCommandFacts,
    ) -> bool {
        self.validates(expected)
            && (self.status == TailTransactionStatus::Unavailable
                || self.facts.as_ref() == Some(supplied_facts))
    }
}
