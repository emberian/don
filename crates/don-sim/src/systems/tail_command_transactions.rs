// SPDX-License-Identifier: GPL-3.0-or-later
//! Branch-complete transactions for the five remaining non-group command rows.
//!
//! Rows 70/71 always cross player lifecycle tails. Rows 73/78/80 have exact no-tail
//! branches which may apply atomically, but each row also has a branch whose downstream
//! cascade, console parser, or drop-control body is still open. The row-level closure must
//! therefore remain red even when an individual request returns [`TailDecision::Apply`].
//!
//! [`lifecycle`] now recovers the `Player::resign` / `Player::quit` / `Player::drop` /
//! `DropControl::process_drop` bodies these three rows used to stop at. Their remaining
//! boundary is narrower and named: `Leader::defeat` `0x006ECB00` (owned by
//! `victory_score::Leaders::defeat`, but no [`crate::command::Fleet`] host owns a `Leaders`)
//! and `Leader::action_declare` `0x006DAB50` (command row 38's open tail).

#[path = "adjacent_command_prefixes.rs"]
pub mod adjacent;
#[path = "late_command_plans.rs"]
pub mod late;
#[path = "player_lifecycle_tails.rs"]
pub mod lifecycle;

use self::adjacent::{
    AdjacentOpenTailRequest, AdjacentPrefixPresentationReceipt, ConsoleCommand,
    LeaderOptionDataState, LeaderOptionRowReceipt, LeaderOptionsCommand,
};
use self::late::{UngracefulDropEffect, UngracefulDropRequest};
use self::lifecycle::{LifecycleError, LifecycleImage, LifecyclePlan, LifecycleRequest};

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
    /// The host owns no player-lifecycle image. Rows 70/71 then have no authorized state at
    /// all and stay whole-row boundaries, exactly as before [`lifecycle`] existed.
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
    /// The complete [`lifecycle::LifecycleImage`] rows 70/71/80 read. `game_semaphore` is the
    /// separate `Game+0x820` byte the row-80 handler tests at `0x00943EFA`; it must agree with
    /// `image.semaphore[0]`, and a disagreement is refused rather than silently reconciled.
    PlayerLifecycle {
        image: Box<LifecycleImage>,
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
    /// `CommandPackage::process_quit` `0x00943AC0..0x00943AEE`: `[0x00E335C4] = 1` then the
    /// `Console` virtual slot at `+0xC4`. A product exit callback, not simulation state.
    SystemQuitCallback,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TailEffect {
    Presentation(TailPresentationReceipt),
    StoreLeaderOptions {
        who: i32,
        state: LeaderOptionDataState,
    },
    UngracefulDrop(UngracefulDropEffect),
    /// The complete recovered `Player::*` / `DropControl::process_drop` transaction. Only a
    /// plan with no boundary and no outstanding [`lifecycle::LifecycleCall`] ever reaches an
    /// [`TailDecision::Apply`] effect list.
    PlayerLifecycle {
        request: LifecycleRequest,
        plan: Box<LifecyclePlan>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TailOpenBoundary {
    /// `Player::resign(0)` including `leave_game` / `Leader::defeat` continuations, when the
    /// host supplied no lifecycle image at all.
    PlayerResign(ResignRequest),
    /// The pre-quit gate, `Player::quit(0)`, report generation, and system-quit callback
    /// must commit together; no restart flag prefix is authorized on its own.
    PlayerQuit(QuitRequest),
    LeaderOptions(AdjacentOpenTailRequest),
    ConsoleCommand(AdjacentOpenTailRequest),
    /// The row-80 network arm, when the host supplied no lifecycle image.
    DropControlProcessDrop(UngracefulDropRequest),
    /// The recovered lifecycle transaction reached a typed authoritative call
    /// (`Leader::defeat`, `Leader::action_declare`) or an unrecovered callee
    /// (`LeaderData::find_capital`). The plan is carried verbatim so a host that owns those
    /// can execute it; the prefix effects remain observations.
    PlayerLifecycle {
        request: LifecycleRequest,
        plan: Box<LifecyclePlan>,
    },
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
    FactsMismatch {
        opcode: u8,
    },
    LeaderOptions(adjacent::LeaderOptionsPrefixError),
    Lifecycle(LifecycleError),
    /// `TailCommandFacts::PlayerLifecycle` carried a `game_semaphore` byte that disagrees
    /// with `image.semaphore[0]`. The row-80 handler and `DropControl` read the same byte.
    SemaphoreImageMismatch {
        supplied: u8,
        image: u8,
    },
}

/// Turn one recovered lifecycle plan into a tail decision.
///
/// A plan may commit atomically only when it reached no boundary **and** requires no
/// authoritative call. Rows 70/71/80 therefore still refuse every request that would defeat a
/// leader or declare a war, and say exactly which one.
fn decide_lifecycle(
    request: TailCommandRequest,
    prefix: Vec<TailEffect>,
    suffix: Vec<TailEffect>,
    lifecycle_request: LifecycleRequest,
    plan: LifecyclePlan,
) -> TailDecision {
    let clean = plan.boundary.is_none()
        && !plan
            .effects
            .iter()
            .any(|e| matches!(e, lifecycle::LifecycleEffect::Call(_)));
    let plan = Box::new(plan);
    if clean {
        let mut effects = prefix;
        effects.push(TailEffect::PlayerLifecycle {
            request: lifecycle_request,
            plan,
        });
        effects.extend(suffix);
        TailDecision::Apply(TailCommandPlan { request, effects })
    } else {
        TailDecision::Boundary(TailBoundaryPlan {
            request,
            prefix,
            boundary: TailOpenBoundary::PlayerLifecycle {
                request: lifecycle_request,
                plan,
            },
        })
    }
}

fn lifecycle_image<'a>(
    facts: &'a TailCommandFacts,
    opcode: u8,
) -> Result<(&'a LifecycleImage, u8), TailPlanError> {
    let TailCommandFacts::PlayerLifecycle {
        image,
        game_semaphore,
    } = facts
    else {
        return Err(TailPlanError::FactsMismatch { opcode });
    };
    if image.semaphore[0] != *game_semaphore {
        return Err(TailPlanError::SemaphoreImageMismatch {
            supplied: *game_semaphore,
            image: image.semaphore[0],
        });
    }
    Ok((image.as_ref(), *game_semaphore))
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
        (TailCommandRequest::Resign(request), TailCommandFacts::PlayerLifecycle { .. }) => {
            let (image, _) = lifecycle_image(facts, RESIGN_OPCODE)?;
            let lifecycle_request = LifecycleRequest::Resign {
                play: request.play,
                from_quit: 0,
            };
            let plan = lifecycle::plan_lifecycle(image, lifecycle_request)
                .map_err(TailPlanError::Lifecycle)?;
            let prefix = vec![TailEffect::Presentation(
                TailPresentationReceipt::ResignDiagnostic { play: request.play },
            )];
            Ok(decide_lifecycle(
                TailCommandRequest::Resign(*request),
                prefix,
                Vec::new(),
                lifecycle_request,
                plan,
            ))
        }
        (TailCommandRequest::Quit(request), TailCommandFacts::PlayerLifecycle { .. }) => {
            let (image, _) = lifecycle_image(facts, QUIT_OPCODE)?;
            // `CommandPackage::process_quit` `0x00943A64..0x00943AA6` runs its own semaphore
            // prefix before `Player::quit(0)`; it is part of the same transaction and is not
            // authorized separately, so it is planned as an image the lifecycle body then
            // reads.
            let prefix = vec![TailEffect::Presentation(
                TailPresentationReceipt::QuitDiagnostic {
                    play: request.play,
                    replay: request.replay,
                    system_quit: request.system_quit,
                },
            )];
            let mut image = image.clone();
            if request.play == image.console_play && request.replay != 0 {
                image.set_semaphore_bit(lifecycle::SEM_LOCAL_LEFT, false);
                if image.semaphore_flags == 0 {
                    image.semaphore_flags = 2;
                }
                image.set_semaphore_bit(lifecycle::SEM_QUIT_PREFIX, true);
                image.semaphore_flags = 0;
            }
            let lifecycle_request = LifecycleRequest::Quit {
                play: request.play,
                force_stop: 0,
            };
            let plan = lifecycle::plan_lifecycle(&image, lifecycle_request)
                .map_err(TailPlanError::Lifecycle)?;
            let mut suffix = Vec::new();
            if request.system_quit != 0
                && request.play == image.console_play
                && !image.sem(lifecycle::SEM_DROP_CONTROL)
            {
                suffix.push(TailEffect::Presentation(
                    TailPresentationReceipt::SystemQuitCallback,
                ));
            }
            Ok(decide_lifecycle(
                TailCommandRequest::Quit(*request),
                prefix,
                suffix,
                lifecycle_request,
                plan,
            ))
        }
        (TailCommandRequest::UngracefulDrop(request), TailCommandFacts::PlayerLifecycle { .. }) => {
            let (image, game_semaphore) = lifecycle_image(facts, late::UNGRACEFUL_DROP_OPCODE)?;
            let outline = late::plan_ungraceful_drop(*request, game_semaphore);
            let prefix: Vec<TailEffect> = outline
                .effects
                .iter()
                .copied()
                .map(TailEffect::UngracefulDrop)
                .collect();
            let request_value = TailCommandRequest::UngracefulDrop(*request);
            if !outline.downstream_required {
                return Ok(TailDecision::Apply(TailCommandPlan {
                    request: request_value,
                    effects: prefix,
                }));
            }
            let lifecycle_request = LifecycleRequest::ProcessDrop {
                play: i32::from(request.play),
                state: i32::from(request.state),
            };
            let plan = lifecycle::plan_lifecycle(image, lifecycle_request)
                .map_err(TailPlanError::Lifecycle)?;
            Ok(decide_lifecycle(
                request_value,
                prefix,
                Vec::new(),
                lifecycle_request,
                plan,
            ))
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
