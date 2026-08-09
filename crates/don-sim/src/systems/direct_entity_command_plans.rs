//! Fail-closed plans for the adjacent market/entity command rows 46 through 49.
//!
//! This module is deliberately not exported from `systems/mod.rs`.  It first froze the
//! deterministic command-prefix ABI outside the dispatcher; `command.rs` now reaches it
//! through the sibling atomic integration module while leaving incomplete state-writing
//! tails behind an explicit typed delegation:
//!
//! * opcodes 46/47 delegate the successful market loop to the existing
//!   `economy::{do_buy, do_sell}` primitives;
//! * opcode 48 delegates to `Unit::action_unqueue(1)` or
//!   `Build::action_unqueue(type)`;
//! * opcode 49 delegates to `Unit::action_come_out()`.
//!
//! A `Planned` receipt therefore proves only that the prefix was decoded and planned
//! exactly.  It must not be treated as proof that the downstream transaction ran.

pub const BUY_OPCODE: u8 = 46;
pub const SELL_OPCODE: u8 = 47;
pub const UNQUEUE_OPCODE: u8 = 48;
pub const COME_OUT_OPCODE: u8 = 49;

pub const MARKET_WIRE_BYTES: usize = 13;
pub const UNQUEUE_WIRE_BYTES: usize = 15;
pub const COME_OUT_WIRE_BYTES: usize = 11;

/// `SoundGlobalCat` passed by `Leader::tell_embargo`.
pub const SOUND_MARKET_EMBARGO: i32 = 0x40;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlanStatus {
    Planned,
    Unavailable,
}

fn read_i32(wire: &[u8], offset: usize) -> Option<i32> {
    Some(i32::from_le_bytes([
        *wire.get(offset)?,
        *wire.get(offset + 1)?,
        *wire.get(offset + 2)?,
        *wire.get(offset + 3)?,
    ]))
}

fn read_i16(wire: &[u8], offset: usize) -> Option<i16> {
    Some(i16::from_le_bytes([
        *wire.get(offset)?,
        *wire.get(offset + 1)?,
    ]))
}

// ---------------------------------------------------------------------------
// Opcodes 46/47: BuyCommand / SellCommand
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarketSide {
    Buy,
    Sell,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MarketCommandRequest {
    pub side: MarketSide,
    /// Signed wire dword at `+1`.  Retail uses it to select the leader row.
    pub who: i32,
    /// Signed wire dword at `+5`; adapters must reject invalid resource indexes before
    /// indexing Rust storage rather than normalising the value.
    pub good: i32,
    /// Signed wire dword at `+9`.  Mask tests below operate on its exact two's-complement
    /// bit pattern.
    pub flags: i32,
}

pub fn decode_market_command(wire: &[u8]) -> Option<MarketCommandRequest> {
    if wire.len() != MARKET_WIRE_BYTES {
        return None;
    }
    let side = match wire.first().copied()? {
        BUY_OPCODE => MarketSide::Buy,
        SELL_OPCODE => MarketSide::Sell,
        _ => return None,
    };
    Some(MarketCommandRequest {
        side,
        who: read_i32(wire, 1)?,
        good: read_i32(wire, 5)?,
        flags: read_i32(wire, 9)?,
    })
}

/// Maximum `do_buy`/`do_sell` calls made by the reached retail loop.  Either loop stops
/// earlier on the first refused transaction.
pub fn market_attempt_limit(side: MarketSide, flags: i32) -> u32 {
    let bits = flags as u32;
    let mut count = if bits & 1 == 0 { 1 } else { 5 };
    if side == MarketSide::Buy && bits & 4 != 0 {
        count = if count == 1 { 10 } else { 100 };
    }
    if bits & 2 != 0 {
        count = 99_999;
    }
    count
}

/// Branch-sensitive host reads.  `Option` is intentional: facts which retail does not
/// read on a short-circuited path may remain absent, while a missing reached read fails
/// planning closed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MarketCommandFacts {
    /// Signed `Game::frame`, used by the command diagnostic only.
    pub frame: i32,
    /// Aggregate result of `Leader::can_buy_sell`, read only by opcode 46.
    pub can_buy_sell: Option<bool>,
    /// Opcode 47 first evaluates `has_tribe_bonus(4)`.
    pub has_tribe_bonus_4: Option<bool>,
    /// Opcode 47 evaluates `has_preq(0x2ad)` only when the tribe bonus is absent.
    pub has_preq_0x2ad: Option<bool>,
    /// Opcode 47 evaluates `has_market()` only after the preceding OR gate succeeds.
    pub has_market: Option<bool>,
    /// Result of `get_nuke_embargo()`, read only after the side-specific trade gate.
    pub nuke_embargo: Option<i32>,
    /// `LeaderData::who` at `+8`, read by `tell_embargo` only on the embargo branch.
    pub selected_leader_who: Option<i32>,
    /// `Console::display_play` used by `tell_embargo`'s local-presentation gate.
    pub display_who: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarketCommandEffect {
    Diagnostic {
        side: MarketSide,
        who: i32,
        good: i32,
        flags: i32,
        frame: i32,
    },
    /// Local-only UI work performed by `Leader::tell_embargo`.
    ShowEmbargo { embargo: i32 },
    /// Ordered sound request.  Planning consumes no RNG; the adapter must use the
    /// retail-compatible sound RNG stream when executing it.
    Audio { category: i32 },
    /// Atomic state-writing tail.  The adapter calls the corresponding existing economy
    /// primitive until `max_attempts` succeeds or the first `Refused` result.
    DelegateMarketLoop {
        side: MarketSide,
        who: i32,
        good: i32,
        max_attempts: u32,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarketCommandPlan {
    pub effects: Vec<MarketCommandEffect>,
    /// True exactly when the plan reaches the state-writing market loop.
    pub downstream_required: bool,
}

pub fn plan_market_command(
    request: MarketCommandRequest,
    facts: &MarketCommandFacts,
) -> Option<MarketCommandPlan> {
    let mut effects = vec![MarketCommandEffect::Diagnostic {
        side: request.side,
        who: request.who,
        good: request.good,
        flags: request.flags,
        frame: facts.frame,
    }];

    let eligible = match request.side {
        MarketSide::Buy => facts.can_buy_sell?,
        MarketSide::Sell => {
            let has_trade_preq = if facts.has_tribe_bonus_4? {
                true
            } else {
                facts.has_preq_0x2ad?
            };
            has_trade_preq
                && (if has_trade_preq {
                    facts.has_market?
                } else {
                    false
                })
        }
    };
    if !eligible {
        return Some(MarketCommandPlan {
            effects,
            downstream_required: false,
        });
    }

    let embargo = facts.nuke_embargo?;
    if embargo != 0 {
        if facts.selected_leader_who? == facts.display_who {
            effects.push(MarketCommandEffect::ShowEmbargo { embargo });
            effects.push(MarketCommandEffect::Audio {
                category: SOUND_MARKET_EMBARGO,
            });
        }
        return Some(MarketCommandPlan {
            effects,
            downstream_required: false,
        });
    }

    effects.push(MarketCommandEffect::DelegateMarketLoop {
        side: request.side,
        who: request.who,
        good: request.good,
        max_attempts: market_attempt_limit(request.side, request.flags),
    });
    Some(MarketCommandPlan {
        effects,
        downstream_required: true,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarketCommandReceipt {
    pub request: MarketCommandRequest,
    pub facts: Option<MarketCommandFacts>,
    pub status: PlanStatus,
    pub plan: Option<MarketCommandPlan>,
}

impl MarketCommandReceipt {
    pub fn unavailable(request: MarketCommandRequest) -> Self {
        Self {
            request,
            facts: None,
            status: PlanStatus::Unavailable,
            plan: None,
        }
    }

    pub fn validates(&self, expected: MarketCommandRequest) -> bool {
        if self.request != expected {
            return false;
        }
        match self.status {
            PlanStatus::Unavailable => self.facts.is_none() && self.plan.is_none(),
            PlanStatus::Planned => match self.facts.as_ref() {
                Some(facts) => match (self.plan.as_ref(), plan_market_command(self.request, facts))
                {
                    (Some(actual), Some(expected)) => actual == &expected,
                    _ => false,
                },
                None => false,
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Opcodes 48/49: UnqueueCommand / ComeOutCommand
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirectEntityCommandRequest {
    Unqueue {
        who: i32,
        object_index: i32,
        type_index: i32,
        uid: i16,
    },
    ComeOut {
        who: i32,
        object_index: i32,
        uid: i16,
    },
}

pub fn decode_direct_entity_command(wire: &[u8]) -> Option<DirectEntityCommandRequest> {
    match wire.first().copied()? {
        UNQUEUE_OPCODE if wire.len() == UNQUEUE_WIRE_BYTES => {
            Some(DirectEntityCommandRequest::Unqueue {
                who: read_i32(wire, 1)?,
                object_index: read_i32(wire, 5)?,
                type_index: read_i32(wire, 9)?,
                uid: read_i16(wire, 13)?,
            })
        }
        COME_OUT_OPCODE if wire.len() == COME_OUT_WIRE_BYTES => {
            Some(DirectEntityCommandRequest::ComeOut {
                who: read_i32(wire, 1)?,
                object_index: read_i32(wire, 5)?,
                uid: read_i16(wire, 9)?,
            })
        }
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirectEntityKind {
    Unit,
    Build,
}

/// The resolved concrete target after opcode 48's virtual class probe / build resolution,
/// or opcode 49's direct unit-table lookup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DirectEntityTargetFacts {
    pub kind: DirectEntityKind,
    /// Bit zero of the byte at entity `+8`.
    pub active: bool,
    /// Zero-extended word at entity `+0x30`.
    pub uid: u16,
}

/// Retail compares `(uint)(u16)object_uid` to `(int)(i16)wire_uid`.  Because the unsigned
/// operand controls the usual conversion, a negative wire UID never aliases `0xffff`.
pub fn entity_uid_matches(object_uid: u16, wire_uid: i16) -> bool {
    i32::from(object_uid) == i32::from(wire_uid)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirectEntityCommandEffect {
    Diagnostic {
        request: DirectEntityCommandRequest,
        frame: i32,
    },
    DelegateUnitActionUnqueue {
        who: i32,
        object_index: i32,
        argument: i32,
    },
    DelegateBuildActionUnqueue {
        who: i32,
        object_index: i32,
        type_index: i32,
    },
    DelegateUnitActionComeOut {
        who: i32,
        object_index: i32,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectEntityCommandPlan {
    pub effects: Vec<DirectEntityCommandEffect>,
    pub downstream_required: bool,
}

pub fn plan_direct_entity_command(
    request: DirectEntityCommandRequest,
    frame: i32,
    target: &DirectEntityTargetFacts,
) -> Option<DirectEntityCommandPlan> {
    let mut effects = vec![DirectEntityCommandEffect::Diagnostic { request, frame }];
    let mut downstream_required = false;

    match request {
        DirectEntityCommandRequest::Unqueue {
            who,
            object_index,
            type_index,
            uid,
        } => {
            if target.active && entity_uid_matches(target.uid, uid) {
                downstream_required = true;
                effects.push(match target.kind {
                    DirectEntityKind::Unit => {
                        DirectEntityCommandEffect::DelegateUnitActionUnqueue {
                            who,
                            object_index,
                            argument: 1,
                        }
                    }
                    DirectEntityKind::Build => {
                        DirectEntityCommandEffect::DelegateBuildActionUnqueue {
                            who,
                            object_index,
                            type_index,
                        }
                    }
                });
            }
        }
        DirectEntityCommandRequest::ComeOut {
            who,
            object_index,
            uid,
        } => {
            // Opcode 49 resolves from the unit table and calls Unit directly.  A Build fact
            // indicates that the host resolved the wrong ABI, so fail closed.
            if target.kind != DirectEntityKind::Unit {
                return None;
            }
            if target.active && entity_uid_matches(target.uid, uid) {
                downstream_required = true;
                effects.push(DirectEntityCommandEffect::DelegateUnitActionComeOut {
                    who,
                    object_index,
                });
            }
        }
    }

    Some(DirectEntityCommandPlan {
        effects,
        downstream_required,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectEntityCommandReceipt {
    pub request: DirectEntityCommandRequest,
    pub frame: Option<i32>,
    pub target: Option<DirectEntityTargetFacts>,
    pub status: PlanStatus,
    pub plan: Option<DirectEntityCommandPlan>,
}

impl DirectEntityCommandReceipt {
    pub fn unavailable(request: DirectEntityCommandRequest) -> Self {
        Self {
            request,
            frame: None,
            target: None,
            status: PlanStatus::Unavailable,
            plan: None,
        }
    }

    pub fn validates(&self, expected: DirectEntityCommandRequest) -> bool {
        if self.request != expected {
            return false;
        }
        match self.status {
            PlanStatus::Unavailable => {
                self.frame.is_none() && self.target.is_none() && self.plan.is_none()
            }
            PlanStatus::Planned => match (self.frame, self.target.as_ref()) {
                (Some(frame), Some(target)) => match (
                    self.plan.as_ref(),
                    plan_direct_entity_command(self.request, frame, target),
                ) {
                    (Some(actual), Some(expected)) => actual == &expected,
                    _ => false,
                },
                _ => false,
            },
        }
    }
}
