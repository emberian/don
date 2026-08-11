// SPDX-License-Identifier: GPL-3.0-or-later
//! The `Sim`-side host for the recovered player-lifecycle command tails.
//!
//! `docs/mechanics/player-lifecycle-command-tails.md` recovered `Player::resign`
//! `0x006EDCB0`, `Player::quit` `0x006EDC00`, `Player::drop` `0x006EDE80`,
//! `Player::leave_game` `0x006EE010` and `DropControl::process_drop` `0x00959500`
//! completely, and then stopped at one thing:
//!
//! > `Leader::defeat` `0x006ECB00` is implemented completely by
//! > [`victory_score::Leaders::defeat`], but no [`crate::command::Fleet`] implementor owns a
//! > `Leaders`/`Match`.
//!
//! [`Sim`] owns both (`Sim::vic_leaders`, `Sim::vic_match`). This module is the missing
//! link: it materializes the [`lifecycle::LifecycleImage`] those bodies read out of live
//! `Sim` state, executes the resulting plan, and discharges every
//! [`lifecycle::LifecycleCall::LeaderDefeat`] through `Leaders::defeat` — which cleans the
//! terminal queues, requests the defeated-owner Unit sweep, restarts the musical-chairs
//! interval and calls `Game::check_victory` `0x005926B0`. That is the whole wire → end-game
//! path.
//!
//! **Tier C.** Nothing here re-derives a retail body: every branch, constant and store
//! executed by this module lives in [`lifecycle`] (instruction-derived by the `op-life`
//! lane) or in [`victory_score`]. What this file adds is *execution against live state*,
//! plus two things it does derive, both cited inline:
//!
//! * the mapping from `Sim` channels to `LifecycleImage` fields, and
//! * the row-71 handler prefix at `CommandPackage::process_quit`
//!   `0x00943A7B..0x00943AA6`, which [`tail::plan_tail_command`] applies to a private copy
//!   of the image rather than emitting as a [`tail::TailEffect`]. Because that prefix is
//!   invisible in the effect list, the host has to reproduce it — so it is **pinned**: the
//!   host replans the lifecycle body from its own prefixed image and refuses the whole
//!   transaction unless the result is bit-identical to the plan the row planner carried
//!   ([`SimTailError::QuitPrefixDisagreement`]). A silent divergence is not reachable.
//!
//! # What is deliberately not executed
//!
//! * [`lifecycle::LifecycleCall::LeaderActionDeclare`] (`Leader::action_declare`
//!   `0x006DAB50`) — command row 38's open tail. Reached only by `DropControl` states 1
//!   and 2, which therefore refuse **before mutating anything**.
//! * [`lifecycle::LifecycleBoundary::FindCapitalForDefeat`] (`LeaderData::find_capital`
//!   `0x006EB930`) — reached only by `Player::leave_game`'s
//!   `GameInfo::elimination == 1` arm with a running `LeaderData::lost_capital_timer`.
//!   `Sim` owns no City band, so the boundary stays a boundary.
//! * Rows 73 (`LeaderOptions`) and 78 (`ConsoleCmd`): `Sim` owns none of the
//!   `LeaderOptionRowReceipt` / console-parser state those rows read, so
//!   [`Sim::tail_command_facts`] hands them [`tail::TailCommandFacts::NoExternalFacts`]
//!   and the row planner refuses them with `FactsMismatch`.

use super::Sim;
use crate::command::tail_command_transactions as tail;
use crate::systems::victory_score::{DefeatType, Leaders, Match, NUM_LEADERS};

use tail::lifecycle::{
    self, LifecycleBoundary, LifecycleCall, LifecycleEffect, LifecycleError, LifecycleImage,
    LifecyclePlan, LifecyclePresentation, LifecycleReceipt, LifecycleRequest, LifecycleStatus,
    PlayerRow, PLAYER_SLOTS,
};
use tail::{
    TailCommandFacts, TailCommandReceipt, TailCommandRequest, TailDecision, TailEffect,
    TailOpenBoundary, TailPlanError, TailTransactionStatus,
};

/// `Match::semaphore` is a `u32`, so it models `Game::semaphore` bits 0..32 only. Every bit
/// these bodies touch (2, 4, 6, 15, 18) is inside that window; a plan that reached a higher
/// bit would be a fact this host cannot hold and is refused rather than truncated.
pub const MODELLED_SEMAPHORE_BITS: u32 = 32;

// ---------------------------------------------------------------------------
// The live `GameInfo::player[8]` table plus the `Game`/`Console`/`DropControl` scalars
// `victory_score::Match` does not model.
// ---------------------------------------------------------------------------

/// The retail state these five bodies read and write that lives outside
/// [`victory_score::Match`] and [`victory_score::Leaders`].
///
/// `GameInfo::player[8]` is `Game+0x44`, stride `0x8C`
/// (`docs/mechanics/player-lifecycle-command-tails.md`). `PlayerRow` is exactly the four
/// fields the lifecycle bodies touch; retail's setup invariant `players[i].play == i` is
/// enforced by [`lifecycle`]'s own validator, so [`PlayerTable::new`] establishes it.
///
/// This is deliberately **not** [`super::player_setup::AppliedPlayerSetup`]'s
/// [`super::setup_diplomacy::SetupDiplomacy`]: that image is a receipt of what the one-shot
/// frame-zero team transaction did, and `Player::resign` mutates `Player::flags` live.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayerTable {
    /// `GameInfo::player[8]`, `Game+0x44`, stride `0x8C`.
    pub players: [PlayerRow; PLAYER_SLOTS],
    /// `Game::playing` `+0x558`, cleared by `Player::quit`.
    pub playing: i32,
    /// `Game::semaphore.flags` `+0x81C` — the `BitMask<256>::flags` dword, **not** a bit.
    /// `Game::walk_data` `0x00589600` walks `[0x814, 0x81C)`, which stops one dword short of
    /// it, so this scalar is outside checksum channel `Game`.
    pub semaphore_flags: i32,
    /// `Console::play` `+0x2A0`. `-1` means "no local player", which makes every lifecycle
    /// body take its remote arm.
    pub console_play: i32,
    /// `Console::who` `+0x298`.
    pub console_who: i32,
    /// `DropControl+0x94` — the "vote window already opened" latch.
    pub drop_window_open: bool,
}

impl PlayerTable {
    /// A table with `players[i].play == i` and no player present. `console_play`/`console_who`
    /// default to `-1`: a headless sim has no local console, and `-1` is exactly the value
    /// `departure_sound` treats as out of range rather than as slot zero.
    pub fn new() -> Self {
        Self {
            players: std::array::from_fn(|slot| PlayerRow {
                flags: 0,
                who: 0,
                team: 0,
                play: slot as u8,
            }),
            playing: 1,
            semaphore_flags: 0,
            console_play: -1,
            console_who: -1,
            drop_window_open: false,
        }
    }

    /// Seat one player: `Player::flags`, `Player::who` and `Player::team`. `play` is the slot
    /// and is not settable — retail's `GameInfo::player[]` is indexed by it.
    pub fn seat(&mut self, play: usize, flags: u16, who: u8, team: i8) {
        self.players[play].flags = flags;
        self.players[play].who = who;
        self.players[play].team = team;
    }

    /// The complete before-image these bodies read, joined out of the live `Sim` channels.
    ///
    /// * `players` — this table.
    /// * `leaders[i]` — `victory_score::LeaderState`: `leader_flags` `+0x00`, `who` `+0x08`,
    ///   `multi_diff` `+0x50`, `diplos[8]` `+0x74`, `lost_capital_timer` `+0x418`.
    /// * `team_style` / `elimination` — `GameInfo+0x18` / `+0x2B`, i.e. `Game+0x24` /
    ///   `Game+0x37`, already held as `MatchOptions`.
    /// * `semaphore` — `Match::semaphore`'s low 32 bits, little-endian, which is the same
    ///   indexing `BitMask<256>::ptr` uses (`bit n` is byte `n / 8`, mask `1 << (n % 8)`).
    pub fn image(&self, m: &Match, ls: &Leaders) -> LifecycleImage {
        let mut semaphore = [0u8; lifecycle::SEMAPHORE_BYTES];
        semaphore[..4].copy_from_slice(&m.semaphore.to_le_bytes());
        LifecycleImage {
            players: self.players,
            leaders: std::array::from_fn(|slot| lifecycle::LeaderRow {
                leader_flags: ls.slots[slot].leader_flags,
                who: ls.slots[slot].who,
                multi_diff: ls.slots[slot].multi_diff,
                lost_capital_timer: ls.slots[slot].lost_capital_timer,
                diplos: ls.slots[slot].diplos,
            }),
            team_style: m.options.team_style,
            elimination: m.options.elimination,
            playing: self.playing,
            semaphore,
            semaphore_flags: self.semaphore_flags,
            console_play: self.console_play,
            console_who: self.console_who,
            drop_window_open: self.drop_window_open,
        }
    }
}

impl Default for PlayerTable {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Why a planned lifecycle transaction cannot commit against this host. Every variant is
/// raised **before** the first mutation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LifecycleHostError {
    /// [`lifecycle::plan_lifecycle`] refused the image.
    Plan(LifecycleError),
    /// The plan reached an unrecovered callee.
    Boundary(LifecycleBoundary),
    /// A [`LifecycleCall`] this host does not own — today only
    /// `Leader::action_declare` `0x006DAB50`, command row 38's open tail.
    UnexecutableCall(LifecycleCall),
    /// `Leader::defeat`'s first argument is not a `DefeatTypeIndex`.
    UnknownDefeatType { defeat_type: i32 },
    /// `Leader::defeat` named a leader slot outside `leaders[8]`.
    DefeatTargetOutOfRange { who: u8 },
    /// The plan wrote a `Game::semaphore` bit above [`MODELLED_SEMAPHORE_BITS`].
    SemaphoreBitOutOfRange { bit: u32 },
}

/// Why a wire row cannot commit against this host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SimTailError {
    /// `Sim::players` is not installed. Without a `GameInfo::player[8]` image this host owns
    /// no lifecycle facts at all, and the row keeps op-life's whole-row boundary.
    NoPlayerTable,
    /// The row planner refused.
    TailPlan(TailPlanError),
    /// The row planned, but reached an effect or an open boundary this host does not own —
    /// rows 73 and 78 have a `LeaderOptionDataState` store and a console-parser cascade
    /// `Sim` holds nothing for.
    UnsupportedRow {
        opcode: u8,
    },
    Lifecycle(LifecycleHostError),
    /// The host's own `CommandPackage::process_quit` `0x00943A7B..0x00943AA6` prefix
    /// transcription does not reproduce the plan the row planner carried. Nothing mutated.
    QuitPrefixDisagreement,
}

// ---------------------------------------------------------------------------
// Executing one lifecycle plan against live `Sim` channels
// ---------------------------------------------------------------------------

/// Preflight: every reason this plan cannot commit, computed before any mutation.
fn preflight(plan: &LifecyclePlan) -> Result<(), LifecycleHostError> {
    if let Some(boundary) = plan.boundary {
        return Err(LifecycleHostError::Boundary(boundary));
    }
    for effect in &plan.effects {
        match effect {
            LifecycleEffect::SetSemaphoreBit { bit, .. } => {
                if *bit >= MODELLED_SEMAPHORE_BITS {
                    return Err(LifecycleHostError::SemaphoreBitOutOfRange { bit: *bit });
                }
            }
            LifecycleEffect::Call(LifecycleCall::LeaderDefeat {
                who, defeat_type, ..
            }) => {
                if usize::from(*who) >= NUM_LEADERS {
                    return Err(LifecycleHostError::DefeatTargetOutOfRange { who: *who });
                }
                if DefeatType::from_i32(*defeat_type).is_none() {
                    return Err(LifecycleHostError::UnknownDefeatType {
                        defeat_type: *defeat_type,
                    });
                }
            }
            LifecycleEffect::Call(call) => {
                return Err(LifecycleHostError::UnexecutableCall(*call));
            }
            _ => {}
        }
    }
    Ok(())
}

/// One committed lifecycle transaction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppliedLifecycle {
    /// op-life's atomic receipt. `LifecycleReceipt::validates` replans it from `before` and
    /// checks every [`LifecycleCall`] was acknowledged in instruction order.
    pub receipt: LifecycleReceipt,
    /// The ordered presentation envelope. These are product-boundary observations; the
    /// simulation state above is what committed.
    pub presentations: Vec<LifecyclePresentation>,
}

/// Commit one already-planned lifecycle transaction against live state.
///
/// Effects are applied in instruction order, and each [`LifecycleCall::LeaderDefeat`] runs
/// `Leaders::defeat` at exactly the point the plan reached it — which matters, because
/// `Leader::defeat` `0x006ECDD8` calls `Game::check_victory` and can set
/// `game_sem::GAME_OVER` / `game_sem::VICTORY_RESOLVED` between two of the plan's own
/// semaphore writes.
fn commit(
    players: &mut PlayerTable,
    m: &mut Match,
    ls: &mut Leaders,
    request: LifecycleRequest,
    before: LifecycleImage,
    plan: LifecyclePlan,
) -> AppliedLifecycle {
    let mut executed_calls = Vec::new();
    let mut presentations = Vec::new();
    for effect in &plan.effects {
        match *effect {
            LifecycleEffect::SetPlayerFlags { play, flags } => {
                players.players[usize::from(play)].flags = flags;
            }
            LifecycleEffect::SetPlayerTeam { play, team } => {
                players.players[usize::from(play)].team = team;
            }
            LifecycleEffect::SetLeaderFlags { who, leader_flags } => {
                ls.slots[usize::from(who)].leader_flags = leader_flags;
            }
            LifecycleEffect::SetLeaderMultiDiff { who, multi_diff } => {
                ls.slots[usize::from(who)].multi_diff = multi_diff;
            }
            LifecycleEffect::SetTeamStyle(style) => {
                m.options.team_style = style;
            }
            LifecycleEffect::SetPlaying(v) => {
                players.playing = v;
            }
            LifecycleEffect::SetSemaphoreBit { bit, on } => {
                if on {
                    m.set_sem(bit);
                } else {
                    m.clear_sem(bit);
                }
            }
            LifecycleEffect::SetSemaphoreFlags(v) => {
                players.semaphore_flags = v;
            }
            LifecycleEffect::SetDropWindowOpen(v) => {
                players.drop_window_open = v;
            }
            LifecycleEffect::Presentation(p) => presentations.push(p),
            LifecycleEffect::Call(call) => {
                let LifecycleCall::LeaderDefeat {
                    who,
                    defeat_type,
                    arg,
                    instant,
                } = call
                else {
                    // `preflight` refuses every other call before this loop runs.
                    unreachable!("unexecutable call reached commit");
                };
                let dt = DefeatType::from_i32(defeat_type).expect("preflighted defeat type");
                ls.defeat(m, usize::from(who), dt, arg, instant);
                executed_calls.push(call);
            }
        }
    }
    AppliedLifecycle {
        receipt: LifecycleReceipt {
            request,
            status: LifecycleStatus::Applied,
            before: Some(before),
            plan: Some(plan),
            executed_calls,
        },
        presentations,
    }
}

// ---------------------------------------------------------------------------
// The wire row
// ---------------------------------------------------------------------------

/// One committed wire row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommittedTailRow {
    /// The row planner's decision, verbatim. It carries the row-level effect/observation
    /// list, so this host stores no second copy of it.
    pub decision: TailDecision,
    /// Present whenever the decision carried a [`LifecyclePlan`], i.e. for every row 70/71
    /// and for the row-80 network arm. Absent for a row the planner authorized with no
    /// lifecycle body at all.
    pub lifecycle: Option<AppliedLifecycle>,
    /// Present when the decision was [`TailDecision::Apply`] — the row planner authorized
    /// the whole row atomically, so op-life's own row receipt is meaningful.
    pub row: Option<TailCommandReceipt>,
}

/// What this host did with one wire row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SimTailOutcome {
    Committed(Box<CommittedTailRow>),
    /// Planned; nothing mutated.
    Refused(SimTailError),
}

/// One wire row's transaction against this `Sim`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SimTailReceipt {
    pub request: TailCommandRequest,
    /// Exactly the facts the row planner was given.
    pub facts: TailCommandFacts,
    pub outcome: SimTailOutcome,
}

impl SimTailReceipt {
    /// True when the row committed. A refusal is a valid receipt, not a failed one.
    pub fn committed(&self) -> bool {
        matches!(self.outcome, SimTailOutcome::Committed(_))
    }

    /// The lifecycle transaction this row committed, if any.
    pub fn lifecycle(&self) -> Option<&AppliedLifecycle> {
        match &self.outcome {
            SimTailOutcome::Committed(c) => c.lifecycle.as_ref(),
            SimTailOutcome::Refused(_) => None,
        }
    }

    /// Recompute this receipt from its own recorded facts.
    ///
    /// A committed outcome validates only when the row planner still reaches exactly the
    /// recorded decision, the recorded row receipt (if any) still validates against those
    /// facts, and op-life's own [`LifecycleReceipt::validates`] accepts the executed-call
    /// list against the plan the decision carried.
    pub fn validates(&self) -> bool {
        let SimTailOutcome::Committed(committed) = &self.outcome else {
            return true;
        };
        let Ok(decision) = tail::plan_tail_command(&self.request, &self.facts) else {
            return false;
        };
        if decision != committed.decision {
            return false;
        }
        if let Some(row) = committed.row.as_ref() {
            if !matches!(decision, TailDecision::Apply(_))
                || !row.validates_for(&self.request, &self.facts)
            {
                return false;
            }
        } else if matches!(decision, TailDecision::Apply(_)) {
            return false;
        }
        let carried = carried_lifecycle(&committed.decision);
        match (carried, committed.lifecycle.as_ref()) {
            (None, None) => true,
            (Some((request, plan)), Some(applied)) => {
                applied.receipt.request == request
                    && applied.receipt.plan.as_ref() == Some(plan)
                    && applied.receipt.validates(request)
            }
            _ => false,
        }
    }
}

/// The `(LifecycleRequest, LifecyclePlan)` pair a decision carries, if it carries one.
///
/// [`TailDecision::Apply`] carries it as a [`TailEffect::PlayerLifecycle`] inside the effect
/// list — which is easy to miss, and missing it silently drops every mutation of a clean
/// `DropControl` state-3 drop. [`TailDecision::Boundary`] carries it as the boundary itself.
fn carried_lifecycle(decision: &TailDecision) -> Option<(LifecycleRequest, &LifecyclePlan)> {
    match decision {
        TailDecision::Apply(plan) => plan.effects.iter().find_map(|e| match e {
            TailEffect::PlayerLifecycle { request, plan } => Some((*request, plan.as_ref())),
            _ => None,
        }),
        TailDecision::Boundary(boundary) => match &boundary.boundary {
            TailOpenBoundary::PlayerLifecycle { request, plan } => Some((*request, plan.as_ref())),
            _ => None,
        },
    }
}

/// Row-level effects this host cannot commit, checked before any mutation.
///
/// `TailEffect::StoreLeaderOptions` is row 73's `LeaderOptionDataState` store, and
/// `Sim` owns no such record; the row is unreachable here anyway because
/// [`Sim::tail_command_facts`] hands row 73 `NoExternalFacts`, but a decision that reached
/// this host with one is refused rather than silently dropped.
fn unsupported_row_effect(decision: &TailDecision) -> bool {
    let effects = match decision {
        TailDecision::Apply(plan) => &plan.effects,
        TailDecision::Boundary(boundary) => &boundary.prefix,
    };
    effects
        .iter()
        .any(|e| matches!(e, TailEffect::StoreLeaderOptions { .. }))
}

/// `CommandPackage::process_quit` `0x00943A7B..0x00943AA6`, the semaphore prefix the row-71
/// handler runs before `Player::quit(0)`.
///
/// [`tail::plan_tail_command`] applies this to a private copy of the image instead of
/// emitting it as a [`tail::TailEffect`], so a host that wants to commit row 71 has to
/// reproduce it. The reproduction is *pinned* by [`Sim::apply_tail_command_transaction`],
/// which refuses the transaction unless replanning from this prefixed image yields exactly
/// the plan the row planner carried.
fn quit_handler_prefix(image: &mut LifecycleImage, play: i32, replay: u8) {
    if play != image.console_play || replay == 0 {
        return;
    }
    image.set_semaphore_bit(lifecycle::SEM_LOCAL_LEFT, false);
    if image.semaphore_flags == 0 {
        image.semaphore_flags = 2;
    }
    image.set_semaphore_bit(lifecycle::SEM_QUIT_PREFIX, true);
    image.semaphore_flags = 0;
}

/// Write the row-71 prefix through to live state. Called only after the pin below held.
fn commit_quit_handler_prefix(players: &mut PlayerTable, m: &mut Match, play: i32, replay: u8) {
    if play != players.console_play || replay == 0 {
        return;
    }
    m.clear_sem(lifecycle::SEM_LOCAL_LEFT);
    if players.semaphore_flags == 0 {
        players.semaphore_flags = 2;
    }
    m.set_sem(lifecycle::SEM_QUIT_PREFIX);
    players.semaphore_flags = 0;
}

impl Sim {
    /// The complete before-image the lifecycle bodies read, or `None` when no
    /// `GameInfo::player[8]` table is installed.
    pub fn lifecycle_image(&self) -> Option<LifecycleImage> {
        self.players
            .as_ref()
            .map(|t| t.image(&self.vic_match, &self.vic_leaders))
    }

    /// Host-owned facts read before the first possible mutation, in the shape
    /// [`tail::plan_tail_command`] expects.
    ///
    /// Rows 70/71/80 get the full [`TailCommandFacts::PlayerLifecycle`] image once a
    /// [`PlayerTable`] is installed. Every other row — and every row at all without a table —
    /// gets [`TailCommandFacts::NoExternalFacts`], which is exactly op-life's fail-closed
    /// whole-row boundary.
    pub fn tail_command_facts(&self, request: &TailCommandRequest) -> TailCommandFacts {
        let lifecycle_row = matches!(
            request,
            TailCommandRequest::Resign(_)
                | TailCommandRequest::Quit(_)
                | TailCommandRequest::UngracefulDrop(_)
        );
        match (lifecycle_row, self.lifecycle_image()) {
            (true, Some(image)) => {
                let game_semaphore = image.semaphore[0];
                TailCommandFacts::PlayerLifecycle {
                    image: Box::new(image),
                    game_semaphore,
                }
            }
            _ => TailCommandFacts::NoExternalFacts,
        }
    }

    /// Plan and commit one wire row against live `Sim` state.
    ///
    /// This is the command-pump entry, not a `Game::do_frame` step: retail runs
    /// `CommandPackage::process_*` from the turn pump, which is outside the 29 entries of
    /// `DO_FRAME` (steps 0..3 are `OutOfScope` for exactly this reason). Commands committed
    /// here are visible to the very next `Sim::do_frame`, whose step 11
    /// (`Leaders::strategy_all`) and step 27 (`Game::process_end_game`) carry the resulting
    /// end-game transition.
    ///
    /// All fallible work is planned first. A refusal leaves every `Sim` channel unchanged.
    pub fn apply_tail_command_transaction(
        &mut self,
        request: &TailCommandRequest,
    ) -> SimTailReceipt {
        let facts = self.tail_command_facts(request);
        let refuse = |facts: TailCommandFacts, err: SimTailError| SimTailReceipt {
            request: request.clone(),
            facts,
            outcome: SimTailOutcome::Refused(err),
        };

        if self.players.is_none() {
            return refuse(facts, SimTailError::NoPlayerTable);
        }

        let decision = match tail::plan_tail_command(request, &facts) {
            Ok(decision) => decision,
            Err(err) => return refuse(facts, SimTailError::TailPlan(err)),
        };

        if unsupported_row_effect(&decision) {
            return refuse(
                facts,
                SimTailError::UnsupportedRow {
                    opcode: request.opcode(),
                },
            );
        }

        // A `Boundary` this host cannot discharge is a refusal, not a partial commit — a
        // boundary decision authorizes no prefix mutation in the first place.
        let carried = carried_lifecycle(&decision);
        if carried.is_none() {
            if let TailDecision::Boundary(_) = &decision {
                return refuse(
                    facts,
                    SimTailError::UnsupportedRow {
                        opcode: request.opcode(),
                    },
                );
            }
        }

        // ---- preflight; every refusal below leaves `Sim` untouched ----
        let mut prefixed_and_plan = None;
        if let Some((lifecycle_request, carried_plan)) = carried {
            if let Err(err) = preflight(carried_plan) {
                return refuse(facts, SimTailError::Lifecycle(err));
            }
            // The host's own image, plus the row-71 handler prefix, must replan to *exactly*
            // the plan the row planner carried. This is the pin described in the module
            // header: the prefix is the one thing this host re-transcribes, and it cannot
            // diverge silently.
            let table = self.players.as_ref().expect("checked above");
            let mut prefixed = table.image(&self.vic_match, &self.vic_leaders);
            if let TailCommandRequest::Quit(quit) = request {
                quit_handler_prefix(&mut prefixed, quit.play, quit.replay);
            }
            match lifecycle::plan_lifecycle(&prefixed, lifecycle_request) {
                Ok(replanned) if replanned == *carried_plan => {}
                _ => return refuse(facts, SimTailError::QuitPrefixDisagreement),
            }
            prefixed_and_plan = Some((lifecycle_request, prefixed, carried_plan.clone()));
        }

        // ---- past this point nothing can fail ----
        let table = self.players.as_mut().expect("checked above");
        if let TailCommandRequest::Quit(quit) = request {
            commit_quit_handler_prefix(table, &mut self.vic_match, quit.play, quit.replay);
        }
        let lifecycle = prefixed_and_plan.map(|(lifecycle_request, before, plan)| {
            commit(
                table,
                &mut self.vic_match,
                &mut self.vic_leaders,
                lifecycle_request,
                before,
                plan,
            )
        });
        // `Leader::defeat` `0x006ECB00` requests the terminal Build-queue sweep and the
        // defeated-owner Unit sweep. Step 11 drains them through the same helper; a command
        // that defeats a leader between frames must not leave them queued for a tick.
        self.flush_terminal_queue_cleanup();

        let row = match &decision {
            TailDecision::Apply(plan) => Some(TailCommandReceipt {
                request: request.clone(),
                status: TailTransactionStatus::Applied,
                facts: Some(facts.clone()),
                plan: Some(plan.clone()),
            }),
            TailDecision::Boundary(_) => None,
        };
        SimTailReceipt {
            request: request.clone(),
            facts,
            outcome: SimTailOutcome::Committed(Box::new(CommittedTailRow {
                decision,
                lifecycle,
                row,
            })),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::systems::victory_score::{game_sem, leader_flag, Elimination};

    /// Two seated players, leader `i` held by play `i`, and a local console on slot 0.
    ///
    /// The console is not decoration: `Player::resign`'s remote arm reaches
    /// `departure_sound`, which reads `leaders[Console::who]`, so the fail-closed default
    /// `console_who == -1` refuses every remote departure with `ConsoleWhoOutOfRange`.
    fn two_player_sim() -> Sim {
        let mut sim = Sim::new(7, 8);
        sim.activate(0);
        sim.activate(1);
        let mut table = PlayerTable::new();
        let seated = lifecycle::PLAYER_PRESENT | lifecycle::PLAYER_LEAVE_SCAN_REQUIRED;
        table.seat(0, seated, 0, 0);
        table.seat(1, seated, 1, 1);
        table.console_play = 0;
        table.console_who = 0;
        sim.players = Some(table);
        sim
    }

    #[test]
    fn image_joins_match_semaphore_into_the_bitmask_bytes() {
        let mut sim = two_player_sim();
        sim.vic_match.set_sem(game_sem::NET_OR_RECORDING);
        sim.vic_match.set_sem(lifecycle::SEM_QUIT_PREFIX);
        let image = sim.lifecycle_image().expect("table installed");
        assert!(image.sem(lifecycle::SEM_NET_OR_RECORDING));
        assert!(image.sem(lifecycle::SEM_QUIT_PREFIX));
        assert!(!image.sem(lifecycle::SEM_LOCAL_LEFT));
        assert_eq!(image.semaphore[0], 0x04);
        assert_eq!(image.semaphore[2], 0x04);
    }

    #[test]
    fn no_player_table_keeps_the_whole_row_boundary() {
        let mut sim = Sim::new(7, 8);
        sim.activate(0);
        let request = TailCommandRequest::Resign(tail::ResignRequest { play: 0 });
        assert_eq!(
            sim.tail_command_facts(&request),
            TailCommandFacts::NoExternalFacts
        );
        let receipt = sim.apply_tail_command_transaction(&request);
        assert_eq!(
            receipt.outcome,
            SimTailOutcome::Refused(SimTailError::NoPlayerTable)
        );
        assert!(receipt.validates());
        assert!(!sim.vic_leaders.slots[0].flag(leader_flag::DEFEATED));
    }

    #[test]
    fn drop_states_that_declare_war_refuse_before_mutating() {
        let mut sim = two_player_sim();
        sim.vic_match.set_sem(lifecycle::SEM_DROP_CONTROL);
        let before_flags = sim.players.as_ref().unwrap().players[1].flags;
        let request = TailCommandRequest::UngracefulDrop(tail::late::UngracefulDropRequest {
            play: 1,
            state: 1,
        });
        let receipt = sim.apply_tail_command_transaction(&request);
        let SimTailOutcome::Refused(SimTailError::Lifecycle(LifecycleHostError::UnexecutableCall(
            LifecycleCall::LeaderActionDeclare { .. },
        ))) = receipt.outcome
        else {
            panic!(
                "expected an action_declare refusal, got {:?}",
                receipt.outcome
            );
        };
        // Nothing committed: the vote window latch, the team style and the player row are
        // all untouched.
        assert!(!sim.players.as_ref().unwrap().drop_window_open);
        assert_eq!(sim.players.as_ref().unwrap().players[1].flags, before_flags);
        assert!(!sim.vic_leaders.slots[1].flag(leader_flag::DEFEATED));
    }

    #[test]
    fn capital_elimination_arm_stays_a_boundary() {
        let mut sim = two_player_sim();
        sim.vic_match.options.elimination = Elimination::Capital as u8;
        sim.vic_leaders.slots[1].lost_capital_timer = 5;
        let request = TailCommandRequest::Resign(tail::ResignRequest { play: 1 });
        let receipt = sim.apply_tail_command_transaction(&request);
        assert_eq!(
            receipt.outcome,
            SimTailOutcome::Refused(SimTailError::Lifecycle(LifecycleHostError::Boundary(
                LifecycleBoundary::FindCapitalForDefeat { who: 1 }
            )))
        );
        assert!(!sim.vic_leaders.slots[1].flag(leader_flag::DEFEATED));
    }
}
