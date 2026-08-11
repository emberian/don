//! Dynamic producer for checksum channel 5 (`CheckSums::check_groups`).
//!
//! The initial producer in [`crate::groups_channel`] reconstructs the exact 512-slot
//! `Groups::clear` image. This module owns the next seam: it sends recorded opcode-0
//! bytes through [`don_sim::command::Bridge`] and images that bridge's authoritative
//! group pool through the already-derived `Group::walk_data` order.
//!
//! Object state is not present in a `.rcx` command. `process_group` reads liveness, UID,
//! building class, and `UnitTypeData::role`; callers therefore supply an explicit
//! [`GroupMemberFact`] snapshot. A missing fact rejects the command before mutation.
//! Likewise, [`DynamicGroupsProducer::apply_recorded_package`] rejects any other Sim
//! opcode in the package before applying its selection. That is the bounded honest
//! frontier: a selection followed by an unowned action body is not half-committed.

#![forbid(unsafe_code)]

use crate::groups_channel::{
    groups_checksum, GroupMembers, GroupRecord, GroupWindow, GroupsChannelError, GroupsChecksum,
    LAST_GROUP_COUNT,
};
use crate::replay::OwnedCommand;
use crate::wire::{classify, CommandClass};
use don_sim::command::{wire_len, Bridge, ObjectTable, Package, Slot, GROUP_OWNER_SLOTS};
use don_sim::systems::groups_guys::GROUP_MAX_MEMBERS;
use std::collections::{BTreeMap, BTreeSet};

/// The measured first-mutation delay on all seven human-only checksum recordings.
///
/// This is corpus evidence, not a scheduler default: callers still pass the actual
/// execution frame and choose when to invoke the producer.
pub const HUMAN_CORPUS_FIRST_GROUP_DELAY_TURNS: i32 = 2;

/// Object columns read by `CommandPackage::process_group` / `Group::add`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GroupMemberFact {
    pub who: u8,
    pub o: i16,
    pub uid: u16,
    pub alive: bool,
    pub is_unit: bool,
    pub is_building: bool,
    pub role: i32,
}

impl GroupMemberFact {
    /// A live unit fact with no presentation or order-side assumptions.
    pub const fn unit(who: u8, o: i16, uid: u16, role: i32) -> Self {
        Self {
            who,
            o,
            uid,
            alive: true,
            is_unit: true,
            is_building: false,
            role,
        }
    }

    /// A live building fact. Some retail buildings derive from Unit, so `is_unit`
    /// remains an explicit column rather than being inferred from `is_building`.
    pub const fn building(who: u8, o: i16, uid: u16, role: i32, is_unit: bool) -> Self {
        Self {
            who,
            o,
            uid,
            alive: true,
            is_unit,
            is_building: true,
            role,
        }
    }
}

/// A successful opcode-0 transaction and the exact channel image it produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupCommandReceipt {
    pub issued_turn: i32,
    pub execution_frame: i32,
    pub play: i32,
    pub who: u8,
    /// Wire order, including negative and duplicate entries retail may skip.
    pub requested: Vec<i16>,
    /// The live, unique list that reached the interned `Group`.
    pub selected: Vec<i16>,
    pub group_slot: i32,
    pub before: GroupsChecksum,
    pub after: GroupsChecksum,
}

/// Result of routing one whole recorded package through the Groups-only boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupPackageReceipt {
    pub group: Option<GroupCommandReceipt>,
    pub ignored_non_sim_opcodes: Vec<u8>,
}

/// Typed refusals. Every arm is detected before the bridge or its object table mutates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DynamicGroupsError {
    NotGroupCommand { opcode: Option<u8> },
    BadWire { detail: String },
    InvalidOwner { who: i8 },
    TooManyMembers { num: usize },
    DuplicateFact { who: u8, o: i16 },
    MissingMemberFact { who: u8, o: i16 },
    FactObjectOutOfRange { who: u8, o: i16, capacity: usize },
    MultipleGroupCommands,
    UnsupportedSimTail { opcode: u8 },
    Checksum(GroupsChannelError),
}

impl std::fmt::Display for DynamicGroupsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotGroupCommand { opcode } => write!(f, "expected GroupCommand, got {opcode:?}"),
            Self::BadWire { detail } => write!(f, "bad GroupCommand wire: {detail}"),
            Self::InvalidOwner { who } => write!(f, "group owner {who} is outside 0..8"),
            Self::TooManyMembers { num } => {
                write!(
                    f,
                    "GroupCommand names {num} members; retail Group holds 128"
                )
            }
            Self::DuplicateFact { who, o } => write!(f, "duplicate object fact ({who}, {o})"),
            Self::MissingMemberFact { who, o } => {
                write!(
                    f,
                    "missing object fact for GroupCommand member ({who}, {o})"
                )
            }
            Self::FactObjectOutOfRange { who, o, capacity } => write!(
                f,
                "object fact ({who}, {o}) is outside the configured {capacity}-slot band"
            ),
            Self::MultipleGroupCommands => write!(f, "package contains multiple GroupCommands"),
            Self::UnsupportedSimTail { opcode } => write!(
                f,
                "package contains Sim opcode 0x{opcode:02x} beyond the Groups-only owner"
            ),
            Self::Checksum(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for DynamicGroupsError {}

impl From<GroupsChannelError> for DynamicGroupsError {
    fn from(value: GroupsChannelError) -> Self {
        Self::Checksum(value)
    }
}

#[derive(Debug)]
struct DecodedGroup {
    who: u8,
    requested: Vec<i16>,
}

fn decode_group(bytes: &[u8]) -> Result<DecodedGroup, DynamicGroupsError> {
    if bytes.first().copied() != Some(0) {
        return Err(DynamicGroupsError::NotGroupCommand {
            opcode: bytes.first().copied(),
        });
    }
    let exact = wire_len(bytes).map_err(|error| DynamicGroupsError::BadWire {
        detail: format!("{error:?}"),
    })?;
    if exact != bytes.len() {
        return Err(DynamicGroupsError::BadWire {
            detail: format!("declared length {exact}, supplied {}", bytes.len()),
        });
    }
    let num = bytes[1] as usize;
    if num > GROUP_MAX_MEMBERS {
        return Err(DynamicGroupsError::TooManyMembers { num });
    }
    let signed_who = bytes[2] as i8;
    if signed_who < 0 || signed_who as usize >= GROUP_OWNER_SLOTS {
        return Err(DynamicGroupsError::InvalidOwner { who: signed_who });
    }
    let requested = bytes[3..]
        .chunks_exact(2)
        .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    Ok(DecodedGroup {
        who: signed_who as u8,
        requested,
    })
}

/// Image the command bridge's pool through the exact channel-5 walk.
///
/// This is a projection, not a second state owner. The bridge remains the sole mutator;
/// every member slice borrows its live prefix directly from its `GroupData`.
pub fn bridge_groups_checksum(bridge: &Bridge) -> Result<GroupsChecksum, GroupsChannelError> {
    let records: Vec<GroupRecord<'_>> = bridge
        .groups
        .slots()
        .iter()
        .map(|group| {
            let n = group.num.clamp(0, GROUP_MAX_MEMBERS as i32) as usize;
            GroupRecord {
                window: GroupWindow {
                    id: group.id,
                    army: group.army,
                    num: group.num,
                    form: group.form,
                    stamp: group.stamp,
                    ox: group.ox,
                    oy: group.oy,
                    o_dist: group.o_dist,
                    o_angle: group.o_angle,
                    disband: group.disband,
                    order_num: group.order_num,
                    priority: group.priority,
                    role: group.role,
                    think_frame: group.think_frame,
                    new_speed: group.new_speed,
                    speed: group.speed,
                    form_num: group.form_num,
                    facing: group.facing,
                    buildings: group.buildings,
                    who: group.who,
                    march: group.march,
                },
                members: GroupMembers {
                    list: &group.list[..n],
                    off_x: &group.off_x[..n],
                    off_y: &group.off_y[..n],
                    curr_x: &group.curr_x[..n],
                    curr_y: &group.curr_y[..n],
                    angles: &group.angles[..n],
                },
            }
        })
        .collect();
    let last_group: [i32; LAST_GROUP_COUNT] =
        std::array::from_fn(|who| bridge.groups.cur(who as u8));
    groups_checksum(&records, &last_group)
}

/// Exact dynamic Groups owner for the bounded replay command slice.
pub struct DynamicGroupsProducer {
    bridge: Bridge,
    objects: ObjectTable,
    object_capacity: usize,
    /// Mirrors opcode 0's `(o, uid)` cache only so a `num == 0` command can be fully
    /// preflighted before the private bridge cache is allowed to mutate.
    last_selection: [Vec<(i16, u16)>; GROUP_OWNER_SLOTS],
}

impl DynamicGroupsProducer {
    pub fn new(object_capacity: usize) -> Self {
        Self {
            bridge: Bridge::new(),
            objects: ObjectTable::new(object_capacity),
            object_capacity,
            last_selection: std::array::from_fn(|_| Vec::new()),
        }
    }

    pub fn bridge(&self) -> &Bridge {
        &self.bridge
    }

    pub fn checksum(&self) -> Result<GroupsChecksum, GroupsChannelError> {
        bridge_groups_checksum(&self.bridge)
    }

    /// Apply one recorded opcode-0 command after admitting all object reads.
    pub fn apply_group_command(
        &mut self,
        issued_turn: i32,
        execution_frame: i32,
        play: i32,
        bytes: &[u8],
        facts: &[GroupMemberFact],
    ) -> Result<GroupCommandReceipt, DynamicGroupsError> {
        let decoded = decode_group(bytes)?;
        let fact_map = self.preflight_facts(decoded.who, &decoded.requested, facts)?;
        let before = self.checksum()?;

        // All fallible work is above this point. Refresh only the GroupCommand columns;
        // preserve the backlink owned by prior successful bridge transactions.
        for fact in fact_map.values() {
            let prior_group = self
                .objects
                .get(fact.who, fact.o)
                .map_or(-1, |slot| slot.group);
            let mut slot = if fact.is_building {
                Slot::building(fact.uid, 0, 0)
            } else {
                Slot::unit(fact.uid, 0, 0)
            };
            slot.alive = fact.alive;
            slot.is_unit = fact.is_unit;
            slot.is_building = fact.is_building;
            slot.role = fact.role;
            slot.group = prior_group;
            self.objects.put(fact.who, fact.o, slot);
        }

        self.bridge.frame = execution_frame;
        let mut package = Package::new(play, execution_frame as u32);
        self.bridge
            .process_one(&mut package, bytes, &mut self.objects);

        let selected = if package.group < 0 {
            Vec::new()
        } else {
            let group = self
                .bridge
                .groups
                .get(package.group)
                .expect("bridge returned an in-pool group slot");
            let n = group.num.clamp(0, GROUP_MAX_MEMBERS as i32) as usize;
            group.list[..n].to_vec()
        };
        if !decoded.requested.is_empty() {
            self.last_selection[decoded.who as usize] = selected
                .iter()
                .map(|o| (*o, self.objects.get(decoded.who, *o).unwrap().uid))
                .collect();
        }
        let after = self.checksum()?;
        Ok(GroupCommandReceipt {
            issued_turn,
            execution_frame,
            play,
            who: decoded.who,
            requested: decoded.requested,
            selected,
            group_slot: package.group,
            before,
            after,
        })
    }

    /// Route one decoded replay package. Presentation/lockstep rows are inert with
    /// respect to Groups; any other Sim row is an explicit open tail and rejects the
    /// package before opcode 0 is applied.
    pub fn apply_recorded_package(
        &mut self,
        issued_turn: i32,
        execution_frame: i32,
        play: i32,
        commands: &[OwnedCommand],
        facts: &[GroupMemberFact],
    ) -> Result<GroupPackageReceipt, DynamicGroupsError> {
        let mut group = None;
        let mut ignored = Vec::new();
        for command in commands {
            if command.opcode == 0 {
                if group.is_some() {
                    return Err(DynamicGroupsError::MultipleGroupCommands);
                }
                decode_group(&command.bytes)?;
                group = Some(&command.bytes);
            } else if classify(command.opcode) == CommandClass::Sim {
                return Err(DynamicGroupsError::UnsupportedSimTail {
                    opcode: command.opcode,
                });
            } else {
                ignored.push(command.opcode);
            }
        }
        let group = match group {
            Some(bytes) => {
                Some(self.apply_group_command(issued_turn, execution_frame, play, bytes, facts)?)
            }
            None => None,
        };
        Ok(GroupPackageReceipt {
            group,
            ignored_non_sim_opcodes: ignored,
        })
    }

    fn preflight_facts(
        &self,
        who: u8,
        requested: &[i16],
        facts: &[GroupMemberFact],
    ) -> Result<BTreeMap<(u8, i16), GroupMemberFact>, DynamicGroupsError> {
        let mut map = BTreeMap::new();
        for fact in facts {
            if map.insert((fact.who, fact.o), *fact).is_some() {
                return Err(DynamicGroupsError::DuplicateFact {
                    who: fact.who,
                    o: fact.o,
                });
            }
            if fact.who as usize >= GROUP_OWNER_SLOTS
                || fact.o < 0
                || fact.o as usize >= self.object_capacity
            {
                return Err(DynamicGroupsError::FactObjectOutOfRange {
                    who: fact.who,
                    o: fact.o,
                    capacity: self.object_capacity,
                });
            }
        }
        let required: BTreeSet<i16> = if requested.is_empty() {
            self.last_selection[who as usize]
                .iter()
                .map(|(o, _)| *o)
                .collect()
        } else {
            requested.iter().copied().filter(|o| *o >= 0).collect()
        };
        for o in required {
            if !map.contains_key(&(who, o)) {
                return Err(DynamicGroupsError::MissingMemberFact { who, o });
            }
        }
        Ok(map)
    }
}
