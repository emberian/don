//! Exact command prefixes for six `Group::action_*` rows.
//!
//! `stop_spell` has a complete, recomputable transaction plan.  The other five rows
//! expose their exact wire ABI and deterministic `action_begin`/group-field prefix, but
//! retain a typed open tail for their world-owning cascades.

use crate::order::OrderIndex;
use crate::systems::groups_guys::{GroupData, GROUP_MAX_MEMBERS};

pub const TRANSPORT_OPCODE: u8 = 13;
pub const CITY_GATHER_OPCODE: u8 = 18;
pub const GATHER_POINT_OPCODE: u8 = 22;
pub const EJECT_ALL_OPCODE: u8 = 26;
pub const ALARM_OPCODE: u8 = 27;
pub const STOP_SPELL_OPCODE: u8 = 29;

fn read_i32(wire: &[u8], offset: usize) -> Option<i32> {
    Some(i32::from_le_bytes([
        *wire.get(offset)?,
        *wire.get(offset + 1)?,
        *wire.get(offset + 2)?,
        *wire.get(offset + 3)?,
    ]))
}

pub fn decode_stop_spell(wire: &[u8]) -> bool {
    wire == [STOP_SPELL_OPCODE]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenGroupActionCommand {
    Transport,
    CityGather {
        wanted_type: i32,
        /// Logged on the wire but not read by `Group::action_city_gather`.
        queued_unread: i32,
    },
    GatherPoint {
        x: i32,
        y: i32,
        action: i32,
        add_to_end: i32,
    },
    EjectAll {
        back_to_work: i32,
        who: i32,
        eject_o: i32,
        eject_who: i32,
    },
    Alarm,
}

pub fn decode_open_group_action(wire: &[u8]) -> Option<OpenGroupActionCommand> {
    match wire.first().copied()? {
        TRANSPORT_OPCODE if wire.len() == 1 => Some(OpenGroupActionCommand::Transport),
        CITY_GATHER_OPCODE if wire.len() == 9 => Some(OpenGroupActionCommand::CityGather {
            wanted_type: read_i32(wire, 1)?,
            queued_unread: read_i32(wire, 5)?,
        }),
        GATHER_POINT_OPCODE if wire.len() == 17 => Some(OpenGroupActionCommand::GatherPoint {
            x: read_i32(wire, 1)?,
            y: read_i32(wire, 5)?,
            action: read_i32(wire, 9)?,
            add_to_end: read_i32(wire, 13)?,
        }),
        EJECT_ALL_OPCODE if wire.len() == 17 => Some(OpenGroupActionCommand::EjectAll {
            back_to_work: read_i32(wire, 1)?,
            who: read_i32(wire, 5)?,
            eject_o: read_i32(wire, 9)?,
            eject_who: read_i32(wire, 13)?,
        }),
        ALARM_OPCODE if wire.len() == 1 => Some(OpenGroupActionCommand::Alarm),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenGroupActionTail {
    SpawnTransportAndBoard,
    RebuildCityGatherLists,
    GatherPointWorldAndFlight,
    EjectContainment,
    AlarmCityOrPeasant,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OpenGroupActionPlan {
    pub group: GroupData,
    pub command: OpenGroupActionCommand,
    pub tail: OpenGroupActionTail,
}

/// Exact deterministic prefix shared with the still-open world cascades.
pub fn plan_open_group_action(
    group: &GroupData,
    command: OpenGroupActionCommand,
) -> OpenGroupActionPlan {
    let mut next = group.clone();
    // Every row reaches concrete Group::action_begin directly or through
    // action_alarm_peasant before its first member/world mutation.
    next.disband = 0;
    let tail = match command {
        OpenGroupActionCommand::Transport => {
            if next.buildings == 0 {
                next.form = -1;
            }
            OpenGroupActionTail::SpawnTransportAndBoard
        }
        OpenGroupActionCommand::CityGather { .. } => OpenGroupActionTail::RebuildCityGatherLists,
        OpenGroupActionCommand::GatherPoint { .. } => {
            OpenGroupActionTail::GatherPointWorldAndFlight
        }
        OpenGroupActionCommand::EjectAll { who, eject_who, .. } => {
            if (eject_who < 0 && who == i32::from(next.who)) || who < 0 {
                next.form = -1;
            }
            OpenGroupActionTail::EjectContainment
        }
        OpenGroupActionCommand::Alarm => OpenGroupActionTail::AlarmCityOrPeasant,
    };
    OpenGroupActionPlan {
        group: next,
        command,
        tail,
    }
}

// ---------------------------------------------------------------------------
// STOP_SPELL: complete atomic plan
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct StopSpellRequest {
    pub group: GroupData,
    pub frame: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StopSpellMemberFacts {
    pub o: i16,
    pub valid_unit: bool,
    pub on_map: bool,
    pub current_order: Option<OrderIndex>,
    pub unit_masks: u32,
    pub type_index: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopSpellStep {
    SetUnitMasks { o: i16, value: u32 },
    ClearPathAnchor { o: i16 },
    CloseOrders { o: i16, argument: i32 },
    ClearPartialPath { o: i16 },
    UpdateAction { o: i16 },
    ClearSpellWord98 { o: i16 },
    SetObjectsFlag22c,
    UpdateGpiece,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StopSpellPlan {
    pub group: GroupData,
    pub steps: Vec<StopSpellStep>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupActionPlanError {
    MemberCount,
    MemberIdentity,
}

pub fn plan_stop_spell(
    group_after_ignore_orders: &GroupData,
    members: &[StopSpellMemberFacts],
) -> Result<StopSpellPlan, GroupActionPlanError> {
    let mut group = group_after_ignore_orders.clone();
    group.disband = 0;
    let n = group.num.clamp(0, GROUP_MAX_MEMBERS as i32) as usize;
    if members.len() != n {
        return Err(GroupActionPlanError::MemberCount);
    }
    if members
        .iter()
        .zip(&group.list[..n])
        .any(|(facts, &o)| facts.o != o)
    {
        return Err(GroupActionPlanError::MemberIdentity);
    }
    let mut steps = Vec::new();
    if group.buildings == 0 {
        for facts in members {
            if !facts.valid_unit
                || !facts.on_map
                || facts.current_order != Some(OrderIndex::CastSpell)
            {
                continue;
            }
            steps.extend([
                StopSpellStep::SetUnitMasks {
                    o: facts.o,
                    value: facts.unit_masks & !0x0400_0000,
                },
                StopSpellStep::ClearPathAnchor { o: facts.o },
                StopSpellStep::CloseOrders {
                    o: facts.o,
                    argument: 0,
                },
                StopSpellStep::ClearPartialPath { o: facts.o },
                StopSpellStep::UpdateAction { o: facts.o },
                StopSpellStep::ClearSpellWord98 { o: facts.o },
            ]);
            if matches!(facts.type_index, 61 | 62 | 400) {
                steps.push(StopSpellStep::SetObjectsFlag22c);
                steps.push(StopSpellStep::UpdateGpiece);
            }
        }
    }
    Ok(StopSpellPlan { group, steps })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupActionTransactionStatus {
    Applied,
    Unavailable,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StopSpellReceipt {
    pub request: StopSpellRequest,
    pub status: GroupActionTransactionStatus,
    pub group_after_ignore_orders: Option<GroupData>,
    pub members: Vec<StopSpellMemberFacts>,
    pub plan: Option<StopSpellPlan>,
}

impl StopSpellReceipt {
    pub fn unavailable(request: StopSpellRequest) -> Self {
        Self {
            request,
            status: GroupActionTransactionStatus::Unavailable,
            group_after_ignore_orders: None,
            members: Vec::new(),
            plan: None,
        }
    }

    pub fn validates(&self, expected: &StopSpellRequest) -> bool {
        if &self.request != expected {
            return false;
        }
        match self.status {
            GroupActionTransactionStatus::Unavailable => {
                self.group_after_ignore_orders.is_none()
                    && self.members.is_empty()
                    && self.plan.is_none()
            }
            GroupActionTransactionStatus::Applied => {
                let (Some(group), Some(plan)) =
                    (self.group_after_ignore_orders.as_ref(), self.plan.as_ref())
                else {
                    return false;
                };
                plan_stop_spell(group, &self.members).is_ok_and(|expected| &expected == plan)
            }
        }
    }
}
