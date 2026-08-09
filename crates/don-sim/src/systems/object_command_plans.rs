//! Pure recovery plans for object-addressed command rows which are not yet wired into
//! [`crate::command::CommandBridge`].
//!
//! The first recovered row is opcode 75, `RenameCityCommand`.  Retail does more than
//! replace a string: after the conditional city-type write it normalizes the addressed
//! owner's eighteen hot-key groups, stamps the first group containing the object, and
//! asks presentation to update that group's name twice.  This module keeps those calls
//! ordered and makes every world- or presentation-owned result an echoed receipt.

pub const RENAME_CITY_OPCODE: u8 = 75;
pub const RENAME_CITY_WIRE_BYTES: usize = 53;
pub const RENAME_CITY_NAME_UNITS: usize = 22;
pub const OWNER_SLOTS: usize = 8;
pub const HOTKEY_GROUPS_PER_OWNER: i32 = 18;

const OBJECT_VALID_FLAG: u8 = 0x01;
const OBJECT_CITY_FLAG: u8 = 0x20;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ObjectCommandDecodeError {
    WrongLength { expected: usize, actual: usize },
    WrongOpcode { expected: u8, actual: u8 },
}

/// Exact fixed wire body of `RenameCityCommand` (53 bytes).
///
/// The name is deliberately retained as all twenty-two little-endian UTF-16 code units.
/// Retail passes `+9` to `String::operator=(wchar_t *)`; interpreting or truncating it is
/// the city store host's responsibility, not the wire decoder's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenameCityCommand {
    pub who: i32,
    pub object: i32,
    pub name: [u16; RENAME_CITY_NAME_UNITS],
}

pub fn decode_rename_city(bytes: &[u8]) -> Result<RenameCityCommand, ObjectCommandDecodeError> {
    if bytes.len() != RENAME_CITY_WIRE_BYTES {
        return Err(ObjectCommandDecodeError::WrongLength {
            expected: RENAME_CITY_WIRE_BYTES,
            actual: bytes.len(),
        });
    }
    if bytes[0] != RENAME_CITY_OPCODE {
        return Err(ObjectCommandDecodeError::WrongOpcode {
            expected: RENAME_CITY_OPCODE,
            actual: bytes[0],
        });
    }

    let who = i32::from_le_bytes(bytes[1..5].try_into().expect("fixed slice"));
    let object = i32::from_le_bytes(bytes[5..9].try_into().expect("fixed slice"));
    let mut name = [0; RENAME_CITY_NAME_UNITS];
    for (unit, pair) in name.iter_mut().zip(bytes[9..53].chunks_exact(2)) {
        *unit = u16::from_le_bytes([pair[0], pair[1]]);
    }
    Ok(RenameCityCommand { who, object, name })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenameCityObjectLookupRequest {
    pub who: i32,
    pub object: i32,
}

/// World-owned result of the unchecked retail `Objects[who][object]` lookup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenameCityObjectLookupReceipt {
    pub request: RenameCityObjectLookupRequest,
    /// Raw `Object+0x08` byte used by both gates in this handler.
    pub object_flags: u8,
    /// Signed `TypeData+0x72`, read only when `object_flags & 0x20 != 0`.
    pub city_type: Option<i16>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NormalizeSelectionRequest {
    pub slot: i32,
    /// `HotKeyGroups::find_group` writes `Group+0x30 = 1` before `Group::normalize`.
    pub normalize_flag: i32,
}

/// The post-normalize columns which `HotKeyGroups::find_group` reads before continuing.
/// The product host owns the full `GroupData` mutation and echoes this projection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NormalizedSelectionView {
    pub who: u8,
    /// Live prefix after normalization. Retail compares each sign-extended `i16` member
    /// against the command's full signed object index.
    pub members: Vec<i16>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NormalizeSelectionReceipt {
    pub request: NormalizeSelectionRequest,
    pub post: NormalizedSelectionView,
}

/// Facts/results outside this pure planner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenameCityHostFacts {
    pub object: RenameCityObjectLookupReceipt,
    /// One receipt for every visited hot-key slot, in retail scan order.  A matching slot
    /// ends the scan, so receipts after a match are malformed rather than ignored.
    pub normalized_groups: Vec<NormalizeSelectionReceipt>,
    /// `Game+0x550`, stored into the matching `Group+0x14` inside `find_group`.
    pub frame: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CityNameMutation {
    pub who: u8,
    pub city_type: i16,
    pub name: [u16; RENAME_CITY_NAME_UNITS],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HotKeyNameUpdateOrigin {
    FindGroup,
    RenameCityCaller,
}

/// Typed work which remains outside deterministic object/selection state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RenameCityPresentationReceipt {
    SyncDiagnostic {
        who: i32,
        object: i32,
        name: [u16; RENAME_CITY_NAME_UNITS],
    },
    HotKeyNameUpdate {
        slot: i32,
        mode: i32,
        origin: HotKeyNameUpdateOrigin,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RenameCityStep {
    Presentation(RenameCityPresentationReceipt),
    LookupObject(RenameCityObjectLookupRequest),
    WriteCityName(CityNameMutation),
    NormalizeSelection(NormalizeSelectionReceipt),
    StampSelectionFrame { slot: i32, frame: i32 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenameCityPlan {
    pub steps: Vec<RenameCityStep>,
    pub matching_group: Option<i32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RenameCityPlanError {
    UnsafeObjectIndex { who: i32, object: i32 },
    ObjectReceiptMismatch,
    MissingCityType,
    UnexpectedCityType,
    MissingNormalizeReceipt { slot: i32 },
    NormalizeReceiptMismatch { expected_slot: i32 },
    ExtraNormalizeReceipt { first_extra_slot: i32 },
}

/// Reconstruct opcode 75's instruction-ordered state and presentation boundary.
///
/// Retail performs unchecked array indexing before `find_group`; this safe planner refuses
/// negative object indices and owner rows outside 0..8 instead of manufacturing a result.
/// That is an availability rule for a host adapter, not a claim that retail contained the
/// same bounds check.
pub fn plan_rename_city(
    command: &RenameCityCommand,
    facts: &RenameCityHostFacts,
) -> Result<RenameCityPlan, RenameCityPlanError> {
    if !(0..OWNER_SLOTS as i32).contains(&command.who) || command.object < 0 {
        return Err(RenameCityPlanError::UnsafeObjectIndex {
            who: command.who,
            object: command.object,
        });
    }

    let lookup = RenameCityObjectLookupRequest {
        who: command.who,
        object: command.object,
    };
    if facts.object.request != lookup {
        return Err(RenameCityPlanError::ObjectReceiptMismatch);
    }

    let is_city = facts.object.object_flags & OBJECT_CITY_FLAG != 0;
    if is_city && facts.object.city_type.is_none() {
        return Err(RenameCityPlanError::MissingCityType);
    }
    if !is_city && facts.object.city_type.is_some() {
        return Err(RenameCityPlanError::UnexpectedCityType);
    }

    let mut steps = vec![
        RenameCityStep::Presentation(RenameCityPresentationReceipt::SyncDiagnostic {
            who: command.who,
            object: command.object,
            name: command.name,
        }),
        RenameCityStep::LookupObject(lookup),
    ];
    if let Some(city_type) = facts.object.city_type {
        steps.push(RenameCityStep::WriteCityName(CityNameMutation {
            who: command.who as u8,
            city_type,
            name: command.name,
        }));
    }

    let first_slot = command.who * HOTKEY_GROUPS_PER_OWNER;
    let mut receipts = facts.normalized_groups.iter();
    let mut matching_group = None;
    for slot in first_slot..first_slot + HOTKEY_GROUPS_PER_OWNER {
        let Some(receipt) = receipts.next() else {
            return Err(RenameCityPlanError::MissingNormalizeReceipt { slot });
        };
        let expected = NormalizeSelectionRequest {
            slot,
            normalize_flag: 1,
        };
        if receipt.request != expected {
            return Err(RenameCityPlanError::NormalizeReceiptMismatch {
                expected_slot: slot,
            });
        }
        steps.push(RenameCityStep::NormalizeSelection(receipt.clone()));

        let contains_object = receipt
            .post
            .members
            .iter()
            .any(|&member| i32::from(member) == command.object);
        if facts.object.object_flags & OBJECT_VALID_FLAG != 0
            && receipt.post.who == command.who as u8
            && contains_object
        {
            matching_group = Some(slot);
            steps.push(RenameCityStep::StampSelectionFrame {
                slot,
                frame: facts.frame,
            });
            steps.push(RenameCityStep::Presentation(
                RenameCityPresentationReceipt::HotKeyNameUpdate {
                    slot,
                    mode: 0,
                    origin: HotKeyNameUpdateOrigin::FindGroup,
                },
            ));
            steps.push(RenameCityStep::Presentation(
                RenameCityPresentationReceipt::HotKeyNameUpdate {
                    slot,
                    mode: 0,
                    origin: HotKeyNameUpdateOrigin::RenameCityCaller,
                },
            ));
            break;
        }
    }

    if let Some(extra) = receipts.next() {
        return Err(RenameCityPlanError::ExtraNormalizeReceipt {
            first_extra_slot: extra.request.slot,
        });
    }

    Ok(RenameCityPlan {
        steps,
        matching_group,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenameCityTransactionStatus {
    Applied,
    Unavailable,
}

/// Atomic host echo for eventual dispatcher integration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenameCityTransactionReceipt {
    pub request: RenameCityCommand,
    pub status: RenameCityTransactionStatus,
    pub facts: Option<RenameCityHostFacts>,
    pub plan: Option<RenameCityPlan>,
}

impl RenameCityTransactionReceipt {
    pub fn unavailable(request: RenameCityCommand) -> Self {
        Self {
            request,
            status: RenameCityTransactionStatus::Unavailable,
            facts: None,
            plan: None,
        }
    }

    pub fn validates(&self, expected: &RenameCityCommand) -> bool {
        if &self.request != expected {
            return false;
        }
        match self.status {
            RenameCityTransactionStatus::Unavailable => self.facts.is_none() && self.plan.is_none(),
            RenameCityTransactionStatus::Applied => {
                let (Some(facts), Some(observed)) = (&self.facts, &self.plan) else {
                    return false;
                };
                plan_rename_city(expected, facts).is_ok_and(|planned| planned == *observed)
            }
        }
    }
}
