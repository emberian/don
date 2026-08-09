//! Fail-closed dispatcher plans for every group opcode still marked `unimplemented`.
//!
//! This module is intentionally not exported from `systems/mod.rs` and does not change the
//! shared command table.  It freezes the deterministic prefix of the seven `Port::Todo`
//! group rows: exact wire decoding, the signed package group-index gate, the addressed-object
//! flag gate used by opcodes 5/6/23, and the exact `Group::action_*` call ABI.
//!
//! Each reached action remains a typed delegate.  A `Planned` receipt proves only that this
//! prefix recomputes exactly; it is not evidence that the delegated group transaction ran.

pub const SIEGE_ATTACK_OPCODE: u8 = 5;
pub const SWARM_AROUND_OPCODE: u8 = 6;
pub const SPELL_OPCODE: u8 = 23;
pub const QUEUE_UP_OPCODE: u8 = 24;
pub const BUILD_OPCODE: u8 = 25;
pub const FLIGHT_OPCODE: u8 = 28;
pub const RECALL_OPCODE: u8 = 35;

pub const SIEGE_ATTACK_WIRE_BYTES: usize = 13;
pub const SWARM_AROUND_WIRE_BYTES: usize = 17;
pub const SPELL_WIRE_BYTES: usize = 21;
pub const QUEUE_UP_WIRE_BYTES: usize = 9;
pub const BUILD_WIRE_BYTES: usize = 25;
pub const FLIGHT_WIRE_BYTES: usize = 25;
pub const RECALL_WIRE_BYTES: usize = 1;

pub const FRONTIER_OPCODES: [u8; 7] = [
    SIEGE_ATTACK_OPCODE,
    SWARM_AROUND_OPCODE,
    SPELL_OPCODE,
    QUEUE_UP_OPCODE,
    BUILD_OPCODE,
    FLIGHT_OPCODE,
    RECALL_OPCODE,
];

/// Literal fifth argument supplied by `process_swarm_around` at `0x00949AAA`.
pub const SWARM_AROUND_RETAIL_MODE: i32 = 1;

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

/// Raw signed fields are retained until the eventual adapter validates enum/index domains.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnimplementedGroupCommandRequest {
    SiegeAttack {
        ox: i32,
        whom: i32,
        queued: i32,
    },
    SwarmAround {
        ox: i32,
        whom: i32,
        queued: i32,
        orders: i32,
    },
    Spell {
        ox: i32,
        whom: i32,
        type_index: i32,
        x: i32,
        y: i32,
    },
    QueueUp {
        type_index: i32,
        num: i32,
    },
    Build {
        x: i32,
        y: i32,
        x2: i32,
        y2: i32,
        type_index: i32,
        queued: i32,
    },
    Flight {
        ox: i32,
        whom: i32,
        shift: i32,
        ctrl: i32,
        alt: i32,
        orders: i32,
    },
    Recall,
}

impl UnimplementedGroupCommandRequest {
    pub fn opcode(self) -> u8 {
        match self {
            Self::SiegeAttack { .. } => SIEGE_ATTACK_OPCODE,
            Self::SwarmAround { .. } => SWARM_AROUND_OPCODE,
            Self::Spell { .. } => SPELL_OPCODE,
            Self::QueueUp { .. } => QUEUE_UP_OPCODE,
            Self::Build { .. } => BUILD_OPCODE,
            Self::Flight { .. } => FLIGHT_OPCODE,
            Self::Recall => RECALL_OPCODE,
        }
    }

    pub fn wire_bytes(self) -> usize {
        match self {
            Self::SiegeAttack { .. } => SIEGE_ATTACK_WIRE_BYTES,
            Self::SwarmAround { .. } => SWARM_AROUND_WIRE_BYTES,
            Self::Spell { .. } => SPELL_WIRE_BYTES,
            Self::QueueUp { .. } => QUEUE_UP_WIRE_BYTES,
            Self::Build { .. } => BUILD_WIRE_BYTES,
            Self::Flight { .. } => FLIGHT_WIRE_BYTES,
            Self::Recall => RECALL_WIRE_BYTES,
        }
    }

    fn target_pair(self) -> Option<(i32, i32)> {
        match self {
            Self::SiegeAttack { ox, whom, .. }
            | Self::SwarmAround { ox, whom, .. }
            | Self::Spell { ox, whom, .. } => Some((ox, whom)),
            Self::QueueUp { .. } | Self::Build { .. } | Self::Flight { .. } | Self::Recall => None,
        }
    }
}

pub fn decode_unimplemented_group_command(wire: &[u8]) -> Option<UnimplementedGroupCommandRequest> {
    let opcode = wire.first().copied()?;
    match opcode {
        SIEGE_ATTACK_OPCODE if wire.len() == SIEGE_ATTACK_WIRE_BYTES => {
            Some(UnimplementedGroupCommandRequest::SiegeAttack {
                ox: read_i32(wire, 1)?,
                whom: read_i32(wire, 5)?,
                queued: read_i32(wire, 9)?,
            })
        }
        SWARM_AROUND_OPCODE if wire.len() == SWARM_AROUND_WIRE_BYTES => {
            Some(UnimplementedGroupCommandRequest::SwarmAround {
                ox: read_i32(wire, 1)?,
                whom: read_i32(wire, 5)?,
                queued: read_i32(wire, 9)?,
                orders: read_i32(wire, 13)?,
            })
        }
        SPELL_OPCODE if wire.len() == SPELL_WIRE_BYTES => {
            Some(UnimplementedGroupCommandRequest::Spell {
                ox: read_i32(wire, 1)?,
                whom: read_i32(wire, 5)?,
                type_index: read_i32(wire, 9)?,
                x: read_i32(wire, 13)?,
                y: read_i32(wire, 17)?,
            })
        }
        QUEUE_UP_OPCODE if wire.len() == QUEUE_UP_WIRE_BYTES => {
            Some(UnimplementedGroupCommandRequest::QueueUp {
                type_index: read_i32(wire, 1)?,
                num: read_i32(wire, 5)?,
            })
        }
        BUILD_OPCODE if wire.len() == BUILD_WIRE_BYTES => {
            Some(UnimplementedGroupCommandRequest::Build {
                x: read_i32(wire, 1)?,
                y: read_i32(wire, 5)?,
                x2: read_i32(wire, 9)?,
                y2: read_i32(wire, 13)?,
                type_index: read_i32(wire, 17)?,
                queued: read_i32(wire, 21)?,
            })
        }
        FLIGHT_OPCODE if wire.len() == FLIGHT_WIRE_BYTES => {
            Some(UnimplementedGroupCommandRequest::Flight {
                ox: read_i32(wire, 1)?,
                whom: read_i32(wire, 5)?,
                shift: read_i32(wire, 9)?,
                ctrl: read_i32(wire, 13)?,
                alt: read_i32(wire, 17)?,
                orders: read_i32(wire, 21)?,
            })
        }
        RECALL_OPCODE if wire.len() == RECALL_WIRE_BYTES => {
            Some(UnimplementedGroupCommandRequest::Recall)
        }
        _ => None,
    }
}

/// Branch-sensitive facts owned by the eventual command adapter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnimplementedGroupCommandFacts {
    /// Signed `CommandPackage +0x0C`.  Every recovered handler skips the action when negative.
    pub group_index: i32,
    /// For opcodes 5/6/23 only: bit zero of the resolved addressed-object byte at `+0x08`
    /// when both `ox` and `whom` are nonnegative. `None` also covers failed resolution; the
    /// handler does not read this on any other path.
    pub addressed_object_flag_1: Option<bool>,
}

/// Arguments are ordered as the PDB `Group::action_*` signatures, not wire field order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupActionCall {
    SiegeAttack {
        ox: i32,
        whom: i32,
        queued: i32,
    },
    SwarmAround {
        ox: i32,
        whom: i32,
        queued: i32,
        orders: i32,
        retail_mode: i32,
    },
    Spell {
        type_index: i32,
        ox: i32,
        whom: i32,
        x: i32,
        y: i32,
    },
    QueueUp {
        type_index: i32,
        num: i32,
    },
    Build {
        x: i32,
        y: i32,
        x2: i32,
        y2: i32,
        type_index: i32,
        queued: i32,
    },
    Flight {
        ox: i32,
        whom: i32,
        orders: i32,
        shift: i32,
        ctrl: i32,
        alt: i32,
    },
    Recall,
}

impl From<UnimplementedGroupCommandRequest> for GroupActionCall {
    fn from(request: UnimplementedGroupCommandRequest) -> Self {
        match request {
            UnimplementedGroupCommandRequest::SiegeAttack { ox, whom, queued } => {
                Self::SiegeAttack { ox, whom, queued }
            }
            UnimplementedGroupCommandRequest::SwarmAround {
                ox,
                whom,
                queued,
                orders,
            } => Self::SwarmAround {
                ox,
                whom,
                queued,
                orders,
                retail_mode: SWARM_AROUND_RETAIL_MODE,
            },
            UnimplementedGroupCommandRequest::Spell {
                ox,
                whom,
                type_index,
                x,
                y,
            } => Self::Spell {
                type_index,
                ox,
                whom,
                x,
                y,
            },
            UnimplementedGroupCommandRequest::QueueUp { type_index, num } => {
                Self::QueueUp { type_index, num }
            }
            UnimplementedGroupCommandRequest::Build {
                x,
                y,
                x2,
                y2,
                type_index,
                queued,
            } => Self::Build {
                x,
                y,
                x2,
                y2,
                type_index,
                queued,
            },
            UnimplementedGroupCommandRequest::Flight {
                ox,
                whom,
                shift,
                ctrl,
                alt,
                orders,
            } => Self::Flight {
                ox,
                whom,
                orders,
                shift,
                ctrl,
                alt,
            },
            UnimplementedGroupCommandRequest::Recall => Self::Recall,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DelegatedGroupAction {
    pub group_index: i32,
    pub call: GroupActionCall,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnimplementedGroupCommandPlan {
    pub delegate: Option<DelegatedGroupAction>,
    pub downstream_required: bool,
}

pub fn plan_unimplemented_group_command(
    request: UnimplementedGroupCommandRequest,
    facts: &UnimplementedGroupCommandFacts,
) -> Option<UnimplementedGroupCommandPlan> {
    if facts.group_index < 0 {
        return Some(UnimplementedGroupCommandPlan {
            delegate: None,
            downstream_required: false,
        });
    }

    if let Some((ox, whom)) = request.target_pair() {
        if ox >= 0 && whom >= 0 && !facts.addressed_object_flag_1? {
            return Some(UnimplementedGroupCommandPlan {
                delegate: None,
                downstream_required: false,
            });
        }
    }

    Some(UnimplementedGroupCommandPlan {
        delegate: Some(DelegatedGroupAction {
            group_index: facts.group_index,
            call: request.into(),
        }),
        downstream_required: true,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnimplementedGroupCommandReceipt {
    pub request: UnimplementedGroupCommandRequest,
    pub facts: Option<UnimplementedGroupCommandFacts>,
    pub status: PlanStatus,
    pub plan: Option<UnimplementedGroupCommandPlan>,
}

impl UnimplementedGroupCommandReceipt {
    pub fn unavailable(request: UnimplementedGroupCommandRequest) -> Self {
        Self {
            request,
            facts: None,
            status: PlanStatus::Unavailable,
            plan: None,
        }
    }

    pub fn validates(&self, expected: UnimplementedGroupCommandRequest) -> bool {
        if self.request != expected {
            return false;
        }
        match self.status {
            PlanStatus::Unavailable => self.facts.is_none() && self.plan.is_none(),
            PlanStatus::Planned => {
                let (Some(facts), Some(observed)) = (self.facts.as_ref(), self.plan.as_ref())
                else {
                    return false;
                };
                plan_unimplemented_group_command(expected, facts)
                    .is_some_and(|plan| plan == *observed)
            }
        }
    }
}
