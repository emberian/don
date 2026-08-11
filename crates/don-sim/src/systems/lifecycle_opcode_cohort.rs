// SPDX-License-Identifier: GPL-3.0-or-later
//! Exact remaining capital lookup for the opcode-70/71 player-lifecycle cohort.
//!
//! `Player::leave_game` `0x006EE010` reaches `LeaderData::find_capital`
//! `0x006EB930` only when capital elimination is active and the departing leader has a
//! running `lost_capital_timer`.  The lifecycle planner deliberately retained that call as
//! a typed boundary.  This module discharges it against [`Sim`](super::Sim)'s canonical
//! [`CityPool`] and [`Leaders`] owners without weakening the boundary for any other host.
//!
//! The implementation is Tier C: Capstone over the supported PE32 image
//! (`30478a44…625079`) fixes every branch in `0x006EB930..0x006EBA4A`.  In particular, the
//! pass-two `leader_flags & 3` guard at `0x006EB9A4..0x006EB9B0` uses the departing
//! leader's precomputed `who * 0x6EEC` offset on every iteration.  It is intentionally
//! loop-invariant; testing the candidate leader would be a plausible but wrong repair of
//! the retail body.

use crate::command::tail_command_transactions::lifecycle::{
    plan_lifecycle, LifecycleBoundary, LifecycleCall, LifecycleEffect, LifecycleError,
    LifecycleImage, LifecyclePlan, LifecycleRequest, DEFEAT_TYPE_CAPITAL, LEADER_VALID_ACTIVE,
};
use crate::command::{Fleet, ObjectTable};
use crate::systems::order_dispatch::OrderQueue;
use crate::systems::tech_cities::CityPool;
use crate::systems::victory_score::Leaders;
use crate::tick::lifecycle_host::{SimTailError, SimTailOutcome, SimTailReceipt};
use crate::tick::Sim;
use std::cell::RefCell;

pub const LEADER_FIND_CAPITAL_VA: u32 = 0x006e_b930;
pub const LEADER_FIND_CAPITAL_END_VA: u32 = 0x006e_ba4a;
pub const CITY_VALID: u16 = 0x0001;
pub const CITY_CAPITAL: u16 = 0x0010;
pub const PLAYER_SLOTS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FindCapitalRequest {
    pub who: usize,
    pub skip_city: i32,
    pub skip_who: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CapitalIdentity {
    pub city: i32,
    pub who: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapitalPass {
    Own,
    Former,
    Fallback,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FindCapitalError {
    LeaderOutOfRange {
        who: usize,
    },
    /// `LeaderData::city_mark +0x408` may not index beyond the installed PtrArray image.
    CityMarkOutOfRange {
        who: usize,
        mark: i32,
        installed: usize,
    },
    Lifecycle(LifecycleError),
    ResolutionShape,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CapitalRowImage {
    pub city_flags: u16,
    pub was_capital_flags: u8,
}

/// Complete immutable input to `find_capital`. Keeping it on the receipt makes Bridge
/// validation independent of the post-command CityPool and prevents a host from blessing
/// a result with a bare boolean.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FindCapitalImage {
    pub rows: [Vec<CapitalRowImage>; PLAYER_SLOTS],
    pub leader_flags: [i32; PLAYER_SLOTS],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FindCapitalReceipt {
    pub request: FindCapitalRequest,
    pub image: FindCapitalImage,
    pub result: CapitalIdentity,
    pub pass: CapitalPass,
    /// Number of City pointer rows tested, in retail traversal order.
    pub visited: usize,
    /// The pass-two guard is retained in the receipt so a candidate-index mutation is
    /// visible even when the selected capital happens to agree.
    pub departing_leader_active_guard: bool,
}

impl FindCapitalReceipt {
    pub fn validates(&self) -> bool {
        self.request.who < PLAYER_SLOTS
            && find_capital_image(self.image.clone(), self.request) == *self
    }
}

fn checked_mark(cities: &CityPool, who: usize) -> Result<usize, FindCapitalError> {
    let mark = cities.city_mark[who];
    let installed = cities.slots[who].len();
    usize::try_from(mark)
        .ok()
        .filter(|&mark| mark <= installed)
        .ok_or(FindCapitalError::CityMarkOutOfRange {
            who,
            mark,
            installed,
        })
}

/// `LeaderData::find_capital(int*, int*, int, int)` `0x006EB930`.
///
/// Retail first scans `[0, city_mark)` in the departing owner's PtrArray for a current
/// capital.  It then scans the seven other PtrArrays for the first live City whose
/// `was_capital_flags` contains the departing owner.  If neither pass succeeds it writes
/// `out_city = -1, out_who = this->who`.
pub fn find_capital(
    cities: &CityPool,
    leaders: &Leaders,
    request: FindCapitalRequest,
) -> Result<FindCapitalReceipt, FindCapitalError> {
    if request.who >= PLAYER_SLOTS || request.who >= leaders.slots.len() {
        return Err(FindCapitalError::LeaderOutOfRange { who: request.who });
    }

    // Preflight every PtrArray bound before observing a result. Retail cannot have an
    // invalid city_mark, but a malformed local owner must refuse without making the later
    // lifecycle transaction depend on where the first capital happened to be.
    let mut rows: [Vec<CapitalRowImage>; PLAYER_SLOTS] = std::array::from_fn(|_| Vec::new());
    for (who, image_rows) in rows.iter_mut().enumerate() {
        let mark = checked_mark(cities, who)?;
        image_rows.extend(cities.slots[who][..mark].iter().map(|row| CapitalRowImage {
            city_flags: row.city_flags,
            was_capital_flags: row.was_capital_flags,
        }));
    }
    let mut leader_flags = [0i32; PLAYER_SLOTS];
    for (who, flags) in leader_flags.iter_mut().enumerate() {
        *flags = leaders.slots[who].leader_flags;
    }
    Ok(find_capital_image(
        FindCapitalImage { rows, leader_flags },
        request,
    ))
}

fn find_capital_image(image: FindCapitalImage, request: FindCapitalRequest) -> FindCapitalReceipt {
    let marks = std::array::from_fn::<_, PLAYER_SLOTS, _>(|who| image.rows[who].len());
    let departing_leader_active_guard =
        image.leader_flags[request.who] & LEADER_VALID_ACTIVE == LEADER_VALID_ACTIVE;

    let mut visited = 0usize;
    for city in 0..marks[request.who] {
        visited += 1;
        let row = image.rows[request.who][city];
        let wanted = row.city_flags & (CITY_VALID | CITY_CAPITAL) == (CITY_VALID | CITY_CAPITAL);
        let skipped = city as i32 == request.skip_city && request.who as i32 == request.skip_who;
        if wanted && !skipped {
            return FindCapitalReceipt {
                request,
                image,
                result: CapitalIdentity {
                    city: city as i32,
                    who: request.who as i32,
                },
                pass: CapitalPass::Own,
                visited,
                departing_leader_active_guard,
            };
        }
    }

    if departing_leader_active_guard {
        let prior_capital_bit = 1u8 << ((request.who as u32) & 0x1f);
        for candidate in 0..PLAYER_SLOTS {
            if candidate == request.who {
                continue;
            }
            for city in 0..marks[candidate] {
                visited += 1;
                let row = image.rows[candidate][city];
                let wanted = row.city_flags & CITY_VALID != 0
                    && row.was_capital_flags & prior_capital_bit != 0;
                let skipped =
                    city as i32 == request.skip_city && candidate as i32 == request.skip_who;
                if wanted && !skipped {
                    return FindCapitalReceipt {
                        request,
                        image,
                        result: CapitalIdentity {
                            city: city as i32,
                            who: candidate as i32,
                        },
                        pass: CapitalPass::Former,
                        visited,
                        departing_leader_active_guard,
                    };
                }
            }
        }
    }

    FindCapitalReceipt {
        request,
        image,
        result: CapitalIdentity {
            city: -1,
            who: request.who as i32,
        },
        pass: CapitalPass::Fallback,
        visited,
        departing_leader_active_guard,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedCapitalBoundary {
    pub request: LifecycleRequest,
    pub before: LifecycleImage,
    pub source: LifecyclePlan,
    pub lookup: FindCapitalReceipt,
    pub resolved: LifecyclePlan,
}

impl ResolvedCapitalBoundary {
    pub fn validates(&self) -> bool {
        if !self.lookup.validates() {
            return false;
        }
        if plan_lifecycle(&self.before, self.request).as_ref() != Ok(&self.source) {
            return false;
        }
        resolved_from_lookup(
            &self.before,
            self.request,
            &self.source,
            self.lookup.clone(),
        )
        .as_ref()
            == Ok(self)
    }
}

/// Replace only [`LifecycleBoundary::FindCapitalForDefeat`] with the exact call retail
/// makes after the lookup. No other boundary is accepted by this cohort.
pub fn resolve_capital_boundary(
    cities: &CityPool,
    leaders: &Leaders,
    before: &LifecycleImage,
    request: LifecycleRequest,
    source: &LifecyclePlan,
) -> Result<ResolvedCapitalBoundary, FindCapitalError> {
    let Some(LifecycleBoundary::FindCapitalForDefeat { who }) = source.boundary else {
        return Err(FindCapitalError::ResolutionShape);
    };
    let lookup = find_capital(
        cities,
        leaders,
        FindCapitalRequest {
            who: usize::from(who),
            skip_city: -1,
            skip_who: -1,
        },
    )?;
    resolved_from_lookup(before, request, source, lookup)
}

fn resolved_from_lookup(
    before: &LifecycleImage,
    request: LifecycleRequest,
    source: &LifecyclePlan,
    lookup: FindCapitalReceipt,
) -> Result<ResolvedCapitalBoundary, FindCapitalError> {
    let Some(LifecycleBoundary::FindCapitalForDefeat { who }) = source.boundary else {
        return Err(FindCapitalError::ResolutionShape);
    };
    // Re-enter the already-recovered body with only the lookup-producing elimination gate
    // bypassed. `lost_capital_timer` remains live, so `append_leave_game` emits the exact
    // DEFEAT_CAPITAL call and (for Quit) continues through the saved-local-left,
    // Game::playing and report tail that follows the call in retail.
    let mut bypass = before.clone();
    bypass.elimination = 0;
    let mut resolved = plan_lifecycle(&bypass, request).map_err(FindCapitalError::Lifecycle)?;
    if resolved.boundary.is_some() {
        return Err(FindCapitalError::ResolutionShape);
    }
    let Some(call_at) = resolved.effects.iter().position(|effect| {
        matches!(
            effect,
            LifecycleEffect::Call(LifecycleCall::LeaderDefeat {
                who: call_who,
                defeat_type: DEFEAT_TYPE_CAPITAL,
                instant: 0,
                ..
            }) if *call_who == who
        )
    }) else {
        return Err(FindCapitalError::ResolutionShape);
    };
    if resolved.effects[..call_at] != source.effects {
        return Err(FindCapitalError::ResolutionShape);
    }
    resolved.effects[call_at] = LifecycleEffect::Call(LifecycleCall::LeaderDefeat {
        who,
        defeat_type: DEFEAT_TYPE_CAPITAL,
        arg: lookup.result.who,
        instant: 0,
    });
    // `elimination` is input, not a body mutation. Restore the authoritative fact in the
    // after-image after using the bypass only to expose the already-recovered continuation.
    resolved.image.elimination = before.elimination;
    Ok(ResolvedCapitalBoundary {
        request,
        before: before.clone(),
        source: source.clone(),
        lookup,
        resolved,
    })
}

// ---------------------------------------------------------------------------
// Real Bridge adapter
// ---------------------------------------------------------------------------

/// Join the Bridge's object receiver with the canonical Sim lifecycle owner for one
/// synchronous command pump. Object/group commands continue to delegate to ObjectTable;
/// only opcodes 70 and 71 opt into the discharged-lifecycle callback.
pub struct LifecycleFleet<'a> {
    pub objects: &'a mut ObjectTable,
    pub sim: &'a mut Sim,
    prepared_capital: RefCell<
        Option<(
            crate::command::tail_command_transactions::TailCommandRequest,
            FindCapitalReceipt,
        )>,
    >,
}

impl<'a> LifecycleFleet<'a> {
    pub fn new(objects: &'a mut ObjectTable, sim: &'a mut Sim) -> Self {
        Self {
            objects,
            sim,
            prepared_capital: RefCell::new(None),
        }
    }
}

impl Fleet for LifecycleFleet<'_> {
    fn alive(&self, who: u8, o: i16) -> bool {
        self.objects.alive(who, o)
    }

    fn is_unit(&self, who: u8, o: i16) -> bool {
        self.objects.is_unit(who, o)
    }

    fn is_building(&self, who: u8, o: i16) -> bool {
        self.objects.is_building(who, o)
    }

    fn group_of(&self, who: u8, o: i16) -> i16 {
        self.objects.group_of(who, o)
    }

    fn set_group_of(&mut self, who: u8, o: i16, slot: i16) {
        self.objects.set_group_of(who, o, slot);
    }

    fn uid(&self, who: u8, o: i16) -> u16 {
        self.objects.uid(who, o)
    }

    fn pos(&self, who: u8, o: i16) -> (i32, i32) {
        self.objects.pos(who, o)
    }

    fn orders(&self, who: u8, o: i16) -> Option<&OrderQueue> {
        self.objects.orders(who, o)
    }

    fn orders_mut(&mut self, who: u8, o: i16) -> Option<&mut OrderQueue> {
        self.objects.orders_mut(who, o)
    }

    fn set_stance(&mut self, who: u8, o: i16, stance: i8) {
        self.objects.set_stance(who, o, stance);
    }

    fn disband(&mut self, who: u8, o: i16) {
        self.objects.disband(who, o);
    }

    fn tail_command_facts(
        &self,
        request: &crate::command::tail_command_transactions::TailCommandRequest,
    ) -> Option<crate::command::tail_command_transactions::TailCommandFacts> {
        use crate::command::tail_command_transactions::{
            plan_tail_command, TailDecision, TailOpenBoundary,
        };

        let facts = self.sim.tail_command_facts(request);
        let prepared = match plan_tail_command(request, &facts) {
            Ok(TailDecision::Boundary(boundary)) => match boundary.boundary {
                TailOpenBoundary::PlayerLifecycle { plan, .. } => match plan.boundary {
                    Some(LifecycleBoundary::FindCapitalForDefeat { who }) => find_capital(
                        &self.sim.cities,
                        &self.sim.vic_leaders,
                        FindCapitalRequest {
                            who: usize::from(who),
                            skip_city: -1,
                            skip_who: -1,
                        },
                    )
                    .ok()
                    .map(|proof| (request.clone(), proof)),
                    _ => None,
                },
                _ => None,
            },
            _ => None,
        };
        *self.prepared_capital.borrow_mut() = prepared;
        Some(facts)
    }

    fn apply_discharged_tail_command_transaction(
        &mut self,
        request: crate::command::tail_command_transactions::TailCommandRequest,
        facts: crate::command::tail_command_transactions::TailCommandFacts,
    ) -> Option<SimTailReceipt> {
        use crate::command::tail_command_transactions::TailCommandRequest;

        if !matches!(
            request,
            TailCommandRequest::Resign(_) | TailCommandRequest::Quit(_)
        ) {
            return None;
        }
        if self.sim.tail_command_facts(&request) != facts {
            return Some(SimTailReceipt {
                request,
                facts,
                outcome: SimTailOutcome::Refused(SimTailError::StaleFacts),
            });
        }
        if let Some((prepared_request, prepared_lookup)) = self.prepared_capital.borrow_mut().take()
        {
            if prepared_request != request {
                return Some(SimTailReceipt {
                    request,
                    facts,
                    outcome: SimTailOutcome::Refused(SimTailError::StaleFacts),
                });
            }
            return Some(
                self.sim
                    .apply_tail_command_transaction_with_capital_lookup(&request, prepared_lookup),
            );
        }
        Some(self.sim.apply_tail_command_transaction(&request))
    }
}
