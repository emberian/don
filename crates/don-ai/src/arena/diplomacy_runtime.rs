// SPDX-License-Identifier: GPL-3.0-or-later
//! The Arena's declaration command and its retail side-effect channel.
//!
//! `arena-diplomacy-model` records the gap exactly: *"the exact explicit relation table
//! adapter is available, but arena matches still expose no declaration command or runtime
//! side-effect channel."* This module is that channel. It does not re-derive
//! `Leader::set_diplo`; the complete atomic transaction already lives in
//! [`don_sim::systems::leader_set_diplo`], which had no `mod` declaration anywhere in the
//! `don-sim` library until 2026-08-11 and therefore could not be reached by any consumer.
//!
//! What lives here is only the two things retail keeps outside that body:
//!
//! 1. the `LeaderData` cells the transaction reads and writes besides `diplos` —
//!    `treaties` (`+0x94`), `ally_mask` (`+0x6929`), `leader_flags` (`+0x00`) and
//!    `leader_flags2` (`+0x04`) — held as Arena state, and
//! 2. the host that answers every fact `plan_set_diplo` asks for, and either executes or
//!    **refuses by name** every step the plan emits.
//!
//! # What the Arena can and cannot execute
//!
//! Every refusal below names the retail VA that would have to be hosted. Nothing here
//! approximates a missing body.
//!
//! | plan step | Arena |
//! |---|---|
//! | `WriteDeclaration` | executed against [`DiplomacyState`] |
//! | `ClearSharedVision` / `GrantSharedVision` | executed against `ally_mask`, which the fog planes consume |
//! | `MarkInterfaceDirty` | recorded; `IFaceData+0x22A` is presentation and the Arena renders nothing |
//! | `Presentation(..)` | recorded, never simulated |
//! | `Authority(ComeOut / KillContainedUnit / AddAirStrafeOrder)` | refused — `eject_my_shit_from_his_ass` `0x006D0220` |
//! | `Authority(ForceArmyProcess)` | refused — `Leader::force_army_process` `0x006F30F0` |
//! | `Authority(Victory)` | refused — `Leader::victory` `0x006EC9B0` |
//! | `SetVictoryBit22` | unreachable: retail only reaches it after `Leader::victory` |
//!
//! The first two authority rows are refusals the Arena never actually reaches, and it does
//! not reach them *for a derived reason rather than a convenient one*: the Arena
//! materializes no contained objects, so `ObjectData::get_inside` is `None` for every one
//! of its objects, and it materializes no `Army`, so every `valid_armies` bit is clear.
//! `Leader::victory` is different — see [`ArenaDeclarationRefusal::UnhostedVictory`]. It is
//! genuinely reached, and this lane does not close it.

use don_sim::systems::leader_set_diplo::{
    plan_set_diplo, EjectionUnitFact, Relation, SetDiploAuthority, SetDiploImage,
    SetDiploMutation, SetDiploPlan, SetDiploPlanError, SetDiploPresentation, SetDiploReceipt,
    SetDiploRequest, SetDiploStep, SetDiploTransactionRequest, SetDiploTransactionStatus,
    ARMY_SLOTS, DIPLO_SLOTS,
};
use don_sim::systems::victory_score::{leader_flag, Diplo, NUM_LEADERS};

use super::retail_systems::DiplomacyState;

/// `Leader::set_diplo`. The transaction this module hosts.
pub const LEADER_SET_DIPLO_VA: u32 = 0x006e_c6a0;
/// `Leader::eject_my_shit_from_his_ass` — the revoked-alliance containment sweep.
pub const EJECT_MY_SHIT_VA: u32 = 0x006d_0220;
/// `Leader::victory(int victory_type, int instant)`.
pub const LEADER_VICTORY_VA: u32 = 0x006e_c9b0;
/// `Leader::force_army_process`.
pub const FORCE_ARMY_PROCESS_VA: u32 = 0x006f_30f0;
/// `LeaderData::has_treaty(int who, uint bit)` — `[this + 0x94 + who*4] & bit`, with the
/// receiver supplied by `get_scary_console_leader` `0x005833E0`.
pub const LEADER_HAS_TREATY_VA: u32 = 0x006e_11e0;
/// `get_scary_console_leader` — `&Leaders[Console::who]`, `Console::who` at
/// `[0x00C06210] + 0x298`.
pub const GET_SCARY_CONSOLE_LEADER_VA: u32 = 0x0058_33e0;
/// `LeaderData::has_preq`. Only the `ALLY_LOS` query reaches this module.
pub const LEADER_HAS_PREQ_VA: u32 = 0x006d_b810;
/// The `Leader::init` eight-target loop that opens `diplos`, `treaties` and `ally_mask`.
/// [`don_sim::systems::leader_init_diplomacy_loop`] owns it; the Arena's opening rows are
/// pinned against it by test rather than recomputed at construction — see the doc.
pub const LEADER_INIT_DIPLOMACY_LOOP_VA: u32 = 0x006e_3bf9;
/// `TypeIndex::ALLY_LOS`. `Leader::set_diplo`'s shared-vision prerequisite.
pub const ALLY_LOS_TYPE: i32 = 0x2b0;

/// One leader's `LeaderData` diplomacy cells outside `diplos`.
///
/// `diplos` itself stays in [`DiplomacyState`], which is where every existing Arena
/// relation consumer already reads it. Duplicating it here would create a second
/// authority for the same eight words.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArenaLeaderDiplomacyRow {
    /// `LeaderData::treaties[8]` (`+0x94`). Retail writes these in `Leader::init`'s loop
    /// (`treaties[t] = (reveal_map == 3)`, then `|= 1` when `is_team(who, t, 0)`) and in
    /// the `TreatyCommand` opcode-37 handler. The Arena hosts neither a team table nor
    /// opcode 37, so the array stays at its opening value; it is a real field rather than a
    /// constant so a future treaty runtime has somewhere authoritative to write.
    pub treaties: [i32; NUM_LEADERS],
    /// `LeaderData::ally_mask` (`+0x6929`) — the byte
    /// [`don_sim::systems::borders_fog::FogLeader::player_mask`] tests every fog plane
    /// with. `Leader::init` opens it at `1 << who`; `Leader::set_diplo` is the only
    /// runtime writer.
    pub ally_mask: u8,
    /// `LeaderData::has_preq(TypeIndex::ALLY_LOS)`. The Arena has no BonusType/effect
    /// runtime, so nothing grants it; the bit exists so that a runtime which does grant it
    /// writes here instead of the shared-vision decision being re-derived.
    pub ally_los: bool,
}

impl ArenaLeaderDiplomacyRow {
    /// The opening row `Leader::init`'s loop produces for leader `who` under the Arena's
    /// declared options. `ally_mask = 1 << who` is unconditional at
    /// `leader_init_diplomacy_loop` `next.row.ally_mask = 1u8 << who`; the ally bits are
    /// added only on the `has_preq(ALLY_LOS)` / `reveal_map >= 1` arms, and the Arena's
    /// all-war opening has no allied pair to add.
    ///
    /// `treaties[who]` opens at **1**, not 0: the loop's `treaties[t] |= 1` gate is
    /// `is_team(who, t, 0)`, and `LeaderData::is_team` `0x006EBD39` returns true for
    /// `t == LeaderData::who` before it looks at any team byte. Every other cell is
    /// `base_treaty = (reveal_map == 3)`, which is 0 for the Arena. This is pinned against
    /// the registered loop by `arena_diplomacy_runtime::the_opening_row_is_the_leader_init_loop_output`
    /// — it was wrong here first, and the pin is what caught it.
    pub fn opening(who: usize) -> Self {
        let mut treaties = [0; NUM_LEADERS];
        treaties[who] = 1;
        Self {
            treaties,
            ally_mask: 1u8 << who,
            ally_los: false,
        }
    }
}

/// The per-frame Arena facts a declaration reads that are not diplomacy state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArenaLeaderRuntime {
    /// `PlayerState::alive`. Retail's `Leader::defeat` clears `ACTIVE` and sets `DEFEATED`.
    pub alive: bool,
    /// `PlayerState::leader_ai`. `leader_flag::HUMAN` is clear for an AI leader.
    pub leader_ai: bool,
}

/// One live Arena object, in the shape `eject_my_shit_from_his_ass` walks the owner list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArenaDiplomacyObject {
    pub owner: usize,
    /// `ObjectData::o` — the owner-local banded index, not the Arena's dense handle.
    pub object_o: i32,
    /// The object's `vt+0x08` unit query.
    pub is_unit: bool,
    /// `ObjectData::get_inside`. The Arena materializes no carrier, so this is always
    /// `None`; it is carried per object rather than assumed so a transport runtime changes
    /// one producer instead of this module's reasoning.
    pub carrier_who: Option<usize>,
}

/// Everything a declaration needs from the live Arena, gathered before any mutation.
pub struct ArenaDeclarationInputs<'a> {
    pub diplomacy: &'a DiplomacyState,
    /// One entry per Arena player slot, in slot order.
    pub rows: &'a [ArenaLeaderDiplomacyRow],
    pub leaders: &'a [ArenaLeaderRuntime],
    pub objects: &'a [ArenaDiplomacyObject],
    /// `Console::who`, `[0x00C06210] + 0x298`. Retail always has a display client and
    /// `get_scary_console_leader` `0x005833E0` indexes `Leaders` with it unguarded, so a
    /// headless `-1` would be an out-of-bounds read rather than a neutral value. The Arena
    /// therefore names a slot.
    pub console_who: usize,
    /// `GameInfo::reveal_map`, `Game+0x30`. `Leader::set_diplo`'s shared-vision fallback
    /// is `>= 1`; the Arena's fog planes run the `Fog::option` default, which is `0`.
    pub reveal_map: u8,
    /// The game victory mask holding retail bit 22 (`Game::semaphore` bit 22,
    /// `game_sem::VICTORY_RESOLVED`).
    pub victory_mask: u32,
}

/// A typed stop. Every variant names the retail body the Arena would have to host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArenaDeclarationRefusal {
    /// A slot outside the Arena's player list, or outside retail's eight.
    UnknownLeader { slot: usize },
    /// The Arena's per-slot vectors disagree in length.
    IncoherentLeaderTables { rows: usize, leaders: usize },
    /// `plan_set_diplo` rejected the image. The Arena never reaches this with a coherent
    /// world; it is surfaced rather than unwrapped.
    Plan(SetDiploPlanError),
    /// `Leader::victory` `0x006EC9B0`. Reached whenever the declaration is an alliance that
    /// leaves no independent active leader — which in a two-player Arena is *every*
    /// alliance. `don_sim::systems::victory_score::Leaders::victory` implements the state
    /// part completely, but it needs a `Leaders`/`Match` owner and the Arena has neither;
    /// its `check_defeat` is a separate elimination model.
    UnhostedVictory { winner: usize },
    /// `eject_my_shit_from_his_ass` `0x006D0220`. Only reachable once the Arena
    /// materializes contained objects.
    UnhostedEjection { owner: usize, object_o: i32 },
    /// `Leader::force_army_process` `0x006F30F0`. Only reachable once the Arena
    /// materializes `Army` slots.
    UnhostedArmyProcess { owner: usize, army_slot: usize },
    /// The applied receipt did not validate against `plan_set_diplo` re-run on the same
    /// before-image. A host bug, not a gameplay outcome.
    ReceiptRejected,
}

impl ArenaDeclarationRefusal {
    /// The retail virtual address a reader should open to close this refusal.
    pub fn retail_va(self) -> Option<u32> {
        match self {
            Self::UnhostedVictory { .. } => Some(LEADER_VICTORY_VA),
            Self::UnhostedEjection { .. } => Some(EJECT_MY_SHIT_VA),
            Self::UnhostedArmyProcess { .. } => Some(FORCE_ARMY_PROCESS_VA),
            Self::Plan(_) | Self::ReceiptRejected => Some(LEADER_SET_DIPLO_VA),
            Self::UnknownLeader { .. } | Self::IncoherentLeaderTables { .. } => None,
        }
    }
}

impl From<SetDiploPlanError> for ArenaDeclarationRefusal {
    fn from(value: SetDiploPlanError) -> Self {
        match value {
            // The planner fails closed on the ejection sweep *before* it can emit the
            // authority step, because `Unit::come_out`'s return decides whether the unit is
            // ejected or killed. Report it as the sweep it is rather than as a generic
            // plan error, so the refusal names `0x006D0220` like every other host gap.
            SetDiploPlanError::MissingEjectionFact {
                owner, object_id, ..
            } => Self::UnhostedEjection {
                owner,
                object_o: object_id,
            },
            SetDiploPlanError::MissingEjectionRoster { owner } => Self::UnhostedEjection {
                owner,
                object_o: -1,
            },
            other => Self::Plan(other),
        }
    }
}

fn map_relation(state: Diplo) -> Relation {
    match state {
        Diplo::War => Relation::War,
        Diplo::Peace => Relation::Peace,
        Diplo::Ally => Relation::Ally,
    }
}

fn map_back(state: Relation) -> Diplo {
    match state {
        Relation::War => Diplo::War,
        Relation::Peace => Diplo::Peace,
        Relation::Ally => Diplo::Ally,
    }
}

/// `LeaderData::leader_flags` (`+0x00`) for one Arena slot.
///
/// `Leader::set_diplo` reads three bits of it: the `& 3 == 3` active-independent test in
/// the alliance-victory scan, and `& 1` / `& 0x0C` in the army gate. Retail's `Leader::init`
/// sets `VALID | ACTIVE`; `Leader::defeat` `0x006ECB00` clears `ACTIVE` and sets `DEFEATED`
/// — which is exactly the Arena's `PlayerState::alive` transition in `World::check_defeat`.
/// `HUMAN` is clear for an AI leader, and every Arena leader is one.
fn leader_flags(runtime: ArenaLeaderRuntime) -> u32 {
    let mut flags = leader_flag::VALID as u32;
    if runtime.alive {
        flags |= leader_flag::ACTIVE as u32;
    } else {
        flags |= leader_flag::DEFEATED as u32;
    }
    if !runtime.leader_ai {
        flags |= leader_flag::HUMAN as u32;
    }
    flags
}

/// Build the complete `SetDiploImage` `plan_set_diplo` reads.
///
/// Absent Arena slots are filled as present-but-invalid leaders: `who` is set (the planner
/// requires `leaders[i].who == i` only for the two parties, but the alliance scan reads
/// every slot's flags), `leader_flags` is zero, and their declarations are the all-war
/// opening `DiplomacyState` already holds. That is the same shape retail leaves a slot in
/// when no player occupies it — `Leader::init` never ran, so `leader_flags & 1` is clear
/// and the scan skips it.
pub fn build_image(
    inputs: &ArenaDeclarationInputs<'_>,
) -> Result<SetDiploImage, ArenaDeclarationRefusal> {
    if inputs.rows.len() != inputs.leaders.len() {
        return Err(ArenaDeclarationRefusal::IncoherentLeaderTables {
            rows: inputs.rows.len(),
            leaders: inputs.leaders.len(),
        });
    }
    let present = inputs.rows.len();
    if present > DIPLO_SLOTS {
        return Err(ArenaDeclarationRefusal::UnknownLeader { slot: present - 1 });
    }
    if inputs.console_who >= present {
        return Err(ArenaDeclarationRefusal::UnknownLeader {
            slot: inputs.console_who,
        });
    }

    let declared = inputs.diplomacy.declarations();
    let mut image = SetDiploImage::default();
    image.local_who = Some(inputs.console_who as i32);
    image.global_shared_vision = Some(inputs.reveal_map >= 1);
    // `IFaceData+0x22A`. The Arena renders nothing, so it opens clean every declaration and
    // the transaction's own write is the only thing that sets it.
    image.interface_dirty = false;
    image.victory_mask = inputs.victory_mask;

    for slot in 0..DIPLO_SLOTS {
        let facts = &mut image.leaders[slot];
        facts.who = Some(slot as i32);
        let mut diplos = [None; DIPLO_SLOTS];
        for (target, cell) in diplos.iter_mut().enumerate() {
            *cell = Some(match declared[slot][target] {
                x if x == Diplo::Peace as i32 => Relation::Peace,
                x if x == Diplo::Ally as i32 => Relation::Ally,
                _ => Relation::War,
            });
        }
        facts.diplos = diplos;

        if slot < present {
            facts.leader_flags = Some(leader_flags(inputs.leaders[slot]));
            // `LeaderData::leader_flags2` (`+0x04`). `Leader::set_diplo` reads only
            // `& 0x0A` — `UNIT_AI_OFF` (2) and the unnamed bit 3. Retail sets
            // `UNIT_AI_OFF` in `Leader::defeat`; the Arena's defeat model does not run
            // `Leader::defeat`, so a defeated Arena leader is reported without it and the
            // army gate is left open. That gate reaches `valid_armies`, which is all-clear,
            // so the difference is not observable — but it is a difference and it is
            // recorded here rather than in a comment somewhere else.
            facts.leader_flags2 = Some(0);
            facts.shared_vision = Some(inputs.rows[slot].ally_mask);
            facts.has_shared_vision_preq = Some(inputs.rows[slot].ally_los);
            facts.valid_armies = Some([false; ARMY_SLOTS]);
            facts.ejection_units = Some(
                inputs
                    .objects
                    .iter()
                    .filter(|o| o.owner == slot)
                    .map(|o| EjectionUnitFact {
                        object_id: o.object_o,
                        is_unit: o.is_unit,
                        carrier_who: o.carrier_who,
                        // Required only when `carrier_who` names the revoked ally. The
                        // Arena has no carrier, so no entry ever needs them; a transport
                        // runtime that sets `carrier_who` will fail closed here on the
                        // exact missing fact instead of guessing a `Unit::come_out` result.
                        come_out_return: None,
                        domain: None,
                        has_air_patrol_order: None,
                    })
                    .collect(),
            );
        } else {
            facts.leader_flags = Some(0);
            facts.leader_flags2 = Some(0);
            facts.shared_vision = Some(0);
            facts.has_shared_vision_preq = Some(false);
            facts.valid_armies = Some([false; ARMY_SLOTS]);
            facts.ejection_units = Some(Vec::new());
        }

        // `get_scary_console_leader()->has_treaty(slot, 1)` — the *console* leader's
        // `treaties` row indexed by the party, not either party's own row.
        image.console_treaty_one[slot] =
            Some(inputs.rows[inputs.console_who].treaties[slot] & 1 != 0);
    }

    Ok(image)
}

/// A declaration the Arena has checked it can execute end to end.
///
/// Holding the before-image beside the plan is what lets [`apply_declaration`] hand the
/// result back through `don-sim`'s own [`SetDiploReceipt::validates`] rather than trusting
/// this file's application loop.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArenaDeclarationPlan {
    before: SetDiploImage,
    request: SetDiploRequest,
    plan: SetDiploPlan,
}

impl ArenaDeclarationPlan {
    pub fn plan(&self) -> &SetDiploPlan {
        &self.plan
    }
    pub fn request(&self) -> SetDiploRequest {
        self.request
    }
    /// Retail's declaration before this transaction, as the mutual-minimum-free raw cell.
    pub fn old_state(&self) -> Diplo {
        map_back(self.plan.old_state)
    }
    /// The presentation envelopes retail would have emitted. The Arena renders none of
    /// them; they are returned so a front end can, without this module simulating one.
    pub fn presentation(&self) -> Vec<SetDiploPresentation> {
        self.plan
            .steps
            .iter()
            .filter_map(|step| match step {
                SetDiploStep::Presentation(p) => Some(*p),
                _ => None,
            })
            .collect()
    }
}

/// Plan one declaration and prove, before anything mutates, that every step it emits is one
/// the Arena can execute.
///
/// This is the fail-closed order the charter asks for: the refusal happens at plan time, so
/// a declaration that reaches `Leader::victory` leaves the world byte-identical rather than
/// half-applied.
pub fn plan_declaration(
    inputs: &ArenaDeclarationInputs<'_>,
    actor: usize,
    target: usize,
    state: Diplo,
) -> Result<ArenaDeclarationPlan, ArenaDeclarationRefusal> {
    let present = inputs.rows.len();
    if actor >= present {
        return Err(ArenaDeclarationRefusal::UnknownLeader { slot: actor });
    }
    if target >= present {
        return Err(ArenaDeclarationRefusal::UnknownLeader { slot: target });
    }
    let before = build_image(inputs)?;
    let request = SetDiploRequest {
        actor,
        target,
        state: map_relation(state),
    };
    let plan = plan_set_diplo(&before, request)?;

    for step in &plan.steps {
        let SetDiploStep::Authority(call) = step else {
            continue;
        };
        match *call {
            SetDiploAuthority::Victory { winner, .. } => {
                return Err(ArenaDeclarationRefusal::UnhostedVictory { winner })
            }
            SetDiploAuthority::ComeOut { owner, object_id }
            | SetDiploAuthority::KillContainedUnit {
                owner, object_id, ..
            }
            | SetDiploAuthority::AddAirStrafeOrder {
                owner, object_id, ..
            } => {
                return Err(ArenaDeclarationRefusal::UnhostedEjection {
                    owner,
                    object_o: object_id,
                })
            }
            SetDiploAuthority::ForceArmyProcess {
                owner, army_slot, ..
            } => {
                return Err(ArenaDeclarationRefusal::UnhostedArmyProcess { owner, army_slot })
            }
        }
    }

    Ok(ArenaDeclarationPlan {
        before,
        request,
        plan,
    })
}

/// What a committed declaration changed, in retail's own order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArenaDeclarationOutcome {
    pub receipt: SetDiploReceipt,
    /// Ordered `(from, to, state)` writes applied to [`DiplomacyState`].
    pub declarations: Vec<(usize, usize, Diplo)>,
    /// Ordered `(viewer, source, granted)` `ally_mask` bit writes.
    pub shared_vision: Vec<(usize, usize, bool)>,
    /// `IFaceData+0x22A` was set. Presentation; recorded, not simulated.
    pub interface_dirty: bool,
    pub presentation: Vec<SetDiploPresentation>,
}

/// Execute a planned declaration.
///
/// The mutation loop walks `plan.steps` in emitted order — retail writes the actor's own
/// `diplos` cell before the global one, and clears shared vision *before* the declaration
/// rather than after, so applying the after-image wholesale would lose the ordering the
/// transaction was recovered to preserve.
pub fn apply_declaration(
    diplomacy: &mut DiplomacyState,
    rows: &mut [ArenaLeaderDiplomacyRow],
    planned: &ArenaDeclarationPlan,
) -> Result<ArenaDeclarationOutcome, ArenaDeclarationRefusal> {
    let mut outcome = ArenaDeclarationOutcome {
        receipt: SetDiploReceipt {
            request: SetDiploTransactionRequest {
                before: planned.before.clone(),
                change: planned.request,
            },
            status: SetDiploTransactionStatus::Applied,
            plan: Some(planned.plan.clone()),
            authority: Vec::new(),
        },
        declarations: Vec::new(),
        shared_vision: Vec::new(),
        interface_dirty: false,
        presentation: Vec::new(),
    };

    for step in &planned.plan.steps {
        match step {
            SetDiploStep::Mutation(SetDiploMutation::WriteDeclaration { from, to, state }) => {
                let state = map_back(*state);
                diplomacy
                    .write_declaration_state_only(*from, *to, state)
                    .map_err(|slot| ArenaDeclarationRefusal::UnknownLeader { slot: slot.0 })?;
                outcome.declarations.push((*from, *to, state));
            }
            SetDiploStep::Mutation(SetDiploMutation::ClearSharedVision { viewer, source }) => {
                let row = rows
                    .get_mut(*viewer)
                    .ok_or(ArenaDeclarationRefusal::UnknownLeader { slot: *viewer })?;
                row.ally_mask &= !(1u8 << *source);
                outcome.shared_vision.push((*viewer, *source, false));
            }
            SetDiploStep::Mutation(SetDiploMutation::GrantSharedVision { viewer, source }) => {
                let row = rows
                    .get_mut(*viewer)
                    .ok_or(ArenaDeclarationRefusal::UnknownLeader { slot: *viewer })?;
                row.ally_mask |= 1u8 << *source;
                outcome.shared_vision.push((*viewer, *source, true));
            }
            SetDiploStep::Mutation(SetDiploMutation::MarkInterfaceDirty) => {
                outcome.interface_dirty = true;
            }
            // Retail reaches this only through `Leader::victory`, which `plan_declaration`
            // refuses before anything is applied. Reaching it here would mean the plan was
            // built by something other than `plan_declaration`.
            SetDiploStep::Mutation(SetDiploMutation::SetVictoryBit22) => {
                return Err(ArenaDeclarationRefusal::UnhostedVictory {
                    winner: planned.request.actor,
                })
            }
            SetDiploStep::Authority(SetDiploAuthority::Victory { winner, .. }) => {
                return Err(ArenaDeclarationRefusal::UnhostedVictory { winner: *winner })
            }
            SetDiploStep::Authority(SetDiploAuthority::ComeOut { owner, object_id })
            | SetDiploStep::Authority(SetDiploAuthority::KillContainedUnit {
                owner,
                object_id,
                ..
            })
            | SetDiploStep::Authority(SetDiploAuthority::AddAirStrafeOrder {
                owner,
                object_id,
                ..
            }) => {
                return Err(ArenaDeclarationRefusal::UnhostedEjection {
                    owner: *owner,
                    object_o: *object_id,
                })
            }
            SetDiploStep::Authority(SetDiploAuthority::ForceArmyProcess {
                owner,
                army_slot,
                ..
            }) => {
                return Err(ArenaDeclarationRefusal::UnhostedArmyProcess {
                    owner: *owner,
                    army_slot: *army_slot,
                })
            }
            SetDiploStep::Presentation(p) => outcome.presentation.push(*p),
        }
    }

    // The gate is `don-sim`'s, not this file's: it re-plans from the same before-image and
    // checks the applied receipt against the result, including that every authority call
    // the plan required was acknowledged.
    let expected = SetDiploTransactionRequest {
        before: planned.before.clone(),
        change: planned.request,
    };
    if !outcome.receipt.validates(&expected) {
        return Err(ArenaDeclarationRefusal::ReceiptRejected);
    }
    Ok(outcome)
}

/// The Arena's diplomacy runtime: `LeaderData`'s non-`diplos` cells plus the two match
/// scalars `Leader::set_diplo` reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArenaDiplomacy {
    rows: Vec<ArenaLeaderDiplomacyRow>,
    console_who: usize,
    reveal_map: u8,
    victory_mask: u32,
    /// Every declaration the match attempted, committed or refused, in submission order.
    /// A refused declaration is not a silent no-op: `arena-diplomacy-model` exists because
    /// relation state was invisible, and an invisible refusal would be the same mistake.
    log: Vec<ArenaDeclarationRecord>,
}

/// One attempted declaration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArenaDeclarationRecord {
    pub actor: usize,
    pub target: usize,
    pub state: Diplo,
    pub outcome: Result<ArenaDeclarationOutcome, ArenaDeclarationRefusal>,
}

impl ArenaDiplomacy {
    /// Open one match's rows. `console_who` is the display client's slot; see
    /// [`ArenaDeclarationInputs::console_who`] for why the Arena names one instead of
    /// running headless with `-1`.
    pub fn opening(players: usize, console_who: usize) -> Self {
        Self {
            rows: (0..players).map(ArenaLeaderDiplomacyRow::opening).collect(),
            console_who: console_who.min(players.saturating_sub(1)),
            // `Fog::option` defaults to `FogOption(0)` in `World::visibility_policy`, so
            // the Arena's declared `Game+0x30` is 0 and shared vision has no fallback.
            reveal_map: 0,
            victory_mask: 0,
            log: Vec::new(),
        }
    }

    pub fn rows(&self) -> &[ArenaLeaderDiplomacyRow] {
        &self.rows
    }

    /// `LeaderData::ally_mask` for one slot — the byte `FogLeader::player_mask` is.
    pub fn ally_mask(&self, who: usize) -> u8 {
        self.rows.get(who).map_or(0, |row| row.ally_mask)
    }

    pub fn console_who(&self) -> usize {
        self.console_who
    }

    pub fn reveal_map(&self) -> u8 {
        self.reveal_map
    }

    pub fn log(&self) -> &[ArenaDeclarationRecord] {
        &self.log
    }

    /// Split borrow helper for the caller that owns both halves of the state.
    pub fn rows_mut(&mut self) -> &mut [ArenaLeaderDiplomacyRow] {
        &mut self.rows
    }

    pub fn record(&mut self, record: ArenaDeclarationRecord) {
        self.log.push(record);
    }

    pub fn inputs<'a>(
        &'a self,
        diplomacy: &'a DiplomacyState,
        leaders: &'a [ArenaLeaderRuntime],
        objects: &'a [ArenaDiplomacyObject],
    ) -> ArenaDeclarationInputs<'a> {
        ArenaDeclarationInputs {
            diplomacy,
            rows: &self.rows,
            leaders,
            objects,
            console_who: self.console_who,
            reveal_map: self.reveal_map,
            victory_mask: self.victory_mask,
        }
    }
}

/// One player-scoped command. `don-env` addresses these on the **player** head set, not the
/// unit one: `DECLARE` is `PLAYER_VERBS[4]`, opcode 38, and its actor is a leader slot
/// rather than an entity — which is why it is not a [`super::cmd::Cmd`], whose whole
/// contract is that every variant carries an acting `EntId`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayerCmd {
    /// `DECLARE` — `DeclareCommand`, opcode 38.
    Declare { target: u8, state: Diplo },
}

impl PlayerCmd {
    /// The `don-env` player-verb index this command is.
    pub fn verb(&self) -> usize {
        match self {
            PlayerCmd::Declare { .. } => don_env::generated::pv::DECLARE,
        }
    }

    pub fn verb_name(&self) -> &'static str {
        don_env::generated::PLAYER_VERBS[self.verb()].name
    }

    /// The shipped `CommandTypes` opcode.
    pub fn opcode(&self) -> u8 {
        don_env::generated::PLAYER_VERBS[self.verb()].opcode
    }
}
