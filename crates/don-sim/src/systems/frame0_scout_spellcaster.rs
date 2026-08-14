//! Golden-2024 frame-zero Scout spellcaster frontier.
//!
//! `Unit::think_spellcaster` (`0x005F27A0`, 1,854 bytes) has two top-level arms.  A
//! `LeaderData::flags & 4` receiver takes the short human arm at `0x005F27C3`; the much
//! larger difficulty/Spy/Sniper/RNG arm starts at `0x005F28CF` only when that bit is clear.
//! The supported replay's owner-zero Scout is human, so this module owns the former and
//! returns a typed residual for the latter.
//!
//! The human arm is small but crosses two product-owned authorities: the 2,033-byte
//! `SpellTypeData::is_castable` predicate and the global `ObjectsData::find` traversal.
//! This module therefore prepares a detached transaction.  Retail-derived receipts may
//! advance the transaction, but no caller state changes until [`commit_no_cast`] accepts a
//! complete no-cast plan.  A found target stops at an exact `Unit::add_cast_order` request;
//! the order/path/Guy after-image is not guessed.
//!
//! Most importantly, this function never reads or writes `CasterData::active_spells`.
//! Its successful arm allocates a Unit `CastOrder` (order index `0x0E`) in `Unit+0xCC`.
//! `Caster::process_spells` at frame one therefore sees the same active-spell array that
//! existed before this frame-zero call, even when a CastOrder is queued.

#![forbid(unsafe_code)]

use std::fmt;

pub const UNIT_THINK_SPELLCASTER_VA: u32 = 0x005F_27A0;
pub const UNIT_THINK_SPELLCASTER_SIZE: u32 = 1_854;
pub const UNIT_THINK_SPELLCASTER_END_VA: u32 = 0x005F_2EDE;
/// SHA-256 of the exact 1,854-byte function extent in the supported executable.
pub const UNIT_THINK_SPELLCASTER_SHA256: [u8; 32] = [
    0xfa, 0xfa, 0x5e, 0x59, 0x5e, 0x78, 0xe8, 0x40, 0xdc, 0x83, 0x69, 0x29, 0x3f, 0xb5, 0x69, 0x09,
    0x63, 0x8d, 0x41, 0xe4, 0x58, 0xfd, 0x38, 0x93, 0x54, 0xf7, 0xd2, 0xf9, 0x87, 0x53, 0x39, 0x04,
];
pub const HUMAN_COUNTERINTEL_ARM_VA: u32 = 0x005F_27C3;
pub const AI_SPELLCASTER_ARM_VA: u32 = 0x005F_28CF;
pub const NO_CAST_RETURN_VA: u32 = 0x005F_2EB1;
pub const CAST_RETURN_ONE_VA: u32 = 0x005F_28C3;

pub const UNIT_DATA_IS_SPECIAL_VA: u32 = 0x0046_CEA0;
pub const UNIT_DATA_IS_SUPPLY_VA: u32 = 0x0046_CE80;
pub const SPELL_IS_CASTABLE_VA: u32 = 0x0067_5BC0;
pub const UNIT_DATA_MANA_VA: u32 = 0x0060_9A50;
pub const SPELL_GET_RANGE_VA: u32 = 0x0067_6A80;
pub const OBJECTS_FIND_VA: u32 = 0x0065_C6B0;
pub const UNIT_ADD_CAST_ORDER_VA: u32 = 0x005E_4A60;
pub const ORDERS_GET_OBJECT_VA: u32 = 0x0073_0AC0;
pub const UNIT_CLEAR_PARTIAL_PATH_VA: u32 = 0x005E_3920;
pub const UNIT_UPDATE_ACTION_VA: u32 = 0x0060_A870;
pub const ORDER_LIST_ADD_VA: u32 = 0x0046_D5A0;

pub const SPELL_IS_CASTABLE_CALLSITE_VA: u32 = 0x005F_2804;
pub const UNIT_DATA_MANA_CALLSITE_VA: u32 = 0x005F_281B;
pub const SPELL_GET_RANGE_CALLSITE_VA: u32 = 0x005F_2875;
pub const OBJECTS_FIND_CALLSITE_VA: u32 = 0x005F_2884;
pub const UNIT_ADD_CAST_ORDER_CALLSITE_VA: u32 = 0x005F_28BE;

pub const SCOUT_TYPE: i32 = 69;
pub const GOLDEN_OWNER: u8 = 0;
pub const GOLDEN_SCOUT_O: i16 = 0;
pub const GOLDEN_SCOUT_UNIT_FLAGS2: u32 = 0x12;
pub const GOLDEN_SCOUT_DOMAIN: i32 = 0;
pub const GOLDEN_SCOUT_BASE_MANA: i32 = 500;
pub const COUNTERINTEL_SPELL: i32 = 0x277;
pub const COUNTERINTEL_SLOT_OFFSET: u32 = 0x09DC;
pub const GOLDEN_COUNTERINTEL_MANA_COST: i32 = 500;
pub const CAST_ORDER_INDEX: i32 = 0x0E;
pub const UNIT_ORDER_LIST_OFFSET: u32 = 0xCC;
pub const UNIT_CURRENT_ORDER_LINK_OFFSET: u32 = 0xDC;
pub const FILTER_INDEX_20: i32 = 20;
pub const OBJECT_COORD_XOR: i32 = 0x0006_3637;
pub const IS_SPECIAL_MASK: u32 = 0x10;
pub const IS_SUPPLY_MASK: u32 = 0x40;
pub const HUMAN_LEADER_MASK: u32 = 0x04;
pub const OBJECTS_FIND_SENTINEL: i32 = 0x05F5_E0FF;

/// SHA-256 of the supported retail executable whose PDB and code bytes define this port.
pub const SUPPORTED_RETAIL_EXE_SHA256: [u8; 32] = [
    0x30, 0x47, 0x8a, 0x44, 0xb5, 0x77, 0xcb, 0x11, 0xeb, 0xcb, 0xbb, 0xf5, 0x3d, 0x3e, 0x93, 0xba,
    0x02, 0xfd, 0x2a, 0xac, 0xf3, 0xbd, 0xef, 0xa6, 0x55, 0x2c, 0x9b, 0x64, 0x49, 0x62, 0x50, 0x79,
];

/// The two mutable words `ObjectsData::find` initializes before enumerating candidates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObjectsFindScratch {
    /// `ObjectsData+0x1FC`.
    pub best_metric: i32,
    /// `ObjectsData+0x200`.
    pub selected_owner: i32,
}

/// Detached checksum-facing boundary.  Revisions are authority identities, not counters that
/// this module invents.  Only `objects_revision/search_scratch` can change on a completed
/// no-cast human path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame0ScoutBoundary {
    pub unit_body_revision: u64,
    pub unit_orders_revision: u64,
    pub unit_path_revision: u64,
    pub guy_revision: u64,
    pub leader_revision: u64,
    pub world_revision: u64,
    pub objects_revision: u64,
    pub search_scratch: ObjectsFindScratch,
    pub rng_state: i32,
    pub caster_active_spells_revision: u64,
    pub caster_active_spells_len: u32,
}

/// Exact source inputs for the supported replay's owner-zero Scout receiver.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GoldenScoutInput {
    pub snapshot_revision: u64,
    pub executable_sha256: [u8; 32],
    pub who: u8,
    pub o: i16,
    pub type_index: i32,
    /// `LeaderData::flags`, tested with bit `4` at `0x005F27B6`.
    pub leader_flags: u32,
    /// Runtime vtable slot `+0xD4`.  The ordinary Unit implementation is admitted directly.
    pub is_special_vfunc_va: u32,
    /// Runtime vtable slot `+0xCC`, used by `UnitData::mana`.
    pub is_supply_vfunc_va: u32,
    /// `UnitTypeData+0x2B8`.
    pub unit_type_flags2: u32,
    /// `UnitTypeData+0x218`; Scout 69 is not domain 2.
    pub unit_domain: i32,
    /// `UnitTypeData+0x2EC`.  For Scout 69 the other `UnitData::mana` arms cannot alter it.
    pub unit_type_mana: i32,
    /// Signed `UnitData+0x96`.
    pub mana_burn: i16,
    /// `SpellTypeData+0x1D0` for Counterintel 631.
    pub counterintel_mana_cost: i32,
    /// Encoded `ObjectData+0x10/+0x14`; retail decodes with `^ 0x63637`.
    pub encoded_x: i32,
    pub encoded_y: i32,
    pub before: Frame0ScoutBoundary,
}

impl GoldenScoutInput {
    #[inline]
    pub const fn x(&self) -> i32 {
        self.encoded_x ^ OBJECT_COORD_XOR
    }

    #[inline]
    pub const fn y(&self) -> i32 {
        self.encoded_y ^ OBJECT_COORD_XOR
    }

    #[inline]
    pub const fn mana_capacity(&self) -> i32 {
        self.unit_type_mana
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetailChildSource {
    SupportedRetailExecutable,
    SyntheticOrUnknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IsCastableRequest {
    pub callsite_va: u32,
    pub function_va: u32,
    pub spell_type: i32,
    pub o: i32,
    pub who: i32,
    pub final_arg: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IsCastableReceipt {
    pub source: RetailChildSource,
    pub snapshot_revision: u64,
    pub request: IsCastableRequest,
    /// Retail tests only zero/nonzero.
    pub result: i32,
}

/// The four stack arguments to `SpellTypeData::get_range` in their source order.  Argument one
/// is the address of the Counterintel pointer slot, not the spell number; this function does not
/// read that argument on the Counterintel arm.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpellRangeRequest {
    pub callsite_va: u32,
    pub function_va: u32,
    pub spell_type: i32,
    pub spell_slot_offset: u32,
    pub owner: i32,
    pub target_o: i32,
    pub target_owner: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpellRangeReceipt {
    pub source: RetailChildSource,
    pub snapshot_revision: u64,
    pub request: SpellRangeRequest,
    pub range: i32,
}

/// The consumed portion of the 13-dword `ObjectsData::find` call at `0x005F2884`.
/// Arguments 10--12 are compiler alignment words and are never read by the 964-byte callee;
/// [`UnreadStackWords`] records that fact without inventing their values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObjectsFindRequest {
    pub callsite_va: u32,
    pub function_va: u32,
    pub x: i32,
    pub y: i32,
    pub search_index_bh: i32,
    pub owner: i32,
    pub range: i32,
    pub spell_slot_offset: u32,
    pub filter_index: i32,
    pub type_index: i32,
    pub query_owner: i32,
    pub unread_args_10_to_12: UnreadStackWords,
    pub stop_on_first: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnreadStackWords {
    /// Linear disassembly proves the callee never reads `[ebp+0x2C..=0x34]`.
    ThreeCompilerAlignmentWords,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObjectsFindReceipt {
    pub source: RetailChildSource,
    pub snapshot_revision: u64,
    pub request: ObjectsFindRequest,
    pub objects_revision_before: u64,
    pub objects_revision_after: u64,
    /// `-1` for no target; a nonnegative object ordinal otherwise.
    pub result_o: i32,
    pub scratch_after: ObjectsFindScratch,
}

/// Exact call ABI and the direct `CastOrder` fields written by `Unit::add_cast_order` for spell
/// 631.  `target_uid` is intentionally absent: the child reads it from the live target only
/// after allocating order index `0x0E`, so a detached planner cannot lawfully guess it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AddCastOrderRequest {
    pub callsite_va: u32,
    pub function_va: u32,
    pub actor_who: i32,
    pub actor_o: i32,
    pub target_o: i32,
    pub target_owner: i32,
    pub x: i32,
    pub y: i32,
    pub spell_type: i32,
    pub queue_pos: i32,
    pub final_flag: i32,
    pub allocated_order_index: i32,
    pub target_uid_source: TargetUidSource,
    pub order_offset_1c: i32,
    pub order_flag_04: bool,
    pub order_list_offset: u32,
    pub closes_orders: bool,
    pub clears_partial_path: bool,
    pub current_order_link_offset: u32,
    pub calls_update_action: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetUidSource {
    LiveObjectWordAtOffset0x30,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExternalRequest {
    /// Dynamic `is_special` override at `0x005F2EBA`; absent on the admitted Scout image.
    DynamicIsSpecial {
        function_va: u32,
        who: i32,
        o: i32,
    },
    /// The non-human arm is intentionally outside this golden transaction.
    AiSpellcasterArm {
        entry_va: u32,
        who: i32,
        o: i32,
    },
    IsCastable(IsCastableRequest),
    SpellRange(SpellRangeRequest),
    ObjectsFind(ObjectsFindRequest),
    AddCastOrder(AddCastOrderRequest),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChildReceipt {
    IsCastable(IsCastableReceipt),
    SpellRange(SpellRangeReceipt),
    ObjectsFind(ObjectsFindReceipt),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoCastReason {
    NotSpecial,
    CounterintelNotCastable,
    InsufficientMana,
    NoTarget,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreparedNoCast {
    pub snapshot_revision: u64,
    pub before: Frame0ScoutBoundary,
    pub after: Frame0ScoutBoundary,
    pub reason: NoCastReason,
    pub retail_return: i32,
    pub return_va: u32,
    pub consumed_receipts: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TypedResidual {
    pub snapshot_revision: u64,
    /// State staged by completed receipts.  It is not committed by preparation.
    pub staged: Frame0ScoutBoundary,
    pub request: ExternalRequest,
    pub consumed_receipts: usize,
    /// Known only for the final add-order child.
    pub retail_return_after_child: Option<i32>,
}

impl TypedResidual {
    /// The frame-zero human arm never advances the game RNG or the Caster active-spell array.
    pub fn preserves_replay_critical_invariants(&self, before: &Frame0ScoutBoundary) -> bool {
        self.staged.rng_state == before.rng_state
            && self.staged.caster_active_spells_revision == before.caster_active_spells_revision
            && self.staged.caster_active_spells_len == before.caster_active_spells_len
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrepareOutcome {
    Ready(PreparedNoCast),
    ExternalRequired(TypedResidual),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrepareError {
    UnsupportedExecutable,
    MissingSnapshotRevision,
    WrongGoldenIdentity,
    WrongScoutType,
    WrongGoldenRules,
    NonEmptyGoldenCasterArray,
    UnsupportedScoutManaShape,
    ReceiptKindMismatch { index: usize },
    ReceiptSourceMismatch { index: usize },
    ReceiptRevisionMismatch { index: usize },
    ReceiptRequestMismatch { index: usize },
    ObjectsRevisionMismatch,
    ObjectsRevisionDidNotAdvance,
    InvalidFindResult,
    InvalidNoTargetScratch,
    UnexpectedReceipt { index: usize },
}

impl fmt::Display for PrepareError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "frame-zero golden Scout spellcaster refused: {self:?}")
    }
}

impl std::error::Error for PrepareError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommitError {
    BoundaryChanged,
    PreparedInvariantBroken,
}

impl fmt::Display for CommitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "frame-zero golden Scout spellcaster commit refused: {self:?}"
        )
    }
}

impl std::error::Error for CommitError {}

fn residual(
    input: &GoldenScoutInput,
    staged: Frame0ScoutBoundary,
    request: ExternalRequest,
    consumed_receipts: usize,
    retail_return_after_child: Option<i32>,
) -> PrepareOutcome {
    PrepareOutcome::ExternalRequired(TypedResidual {
        snapshot_revision: input.snapshot_revision,
        staged,
        request,
        consumed_receipts,
        retail_return_after_child,
    })
}

fn ready(
    input: &GoldenScoutInput,
    after: Frame0ScoutBoundary,
    reason: NoCastReason,
    consumed_receipts: usize,
) -> PrepareOutcome {
    PrepareOutcome::Ready(PreparedNoCast {
        snapshot_revision: input.snapshot_revision,
        before: input.before,
        after,
        reason,
        retail_return: 0,
        return_va: NO_CAST_RETURN_VA,
        consumed_receipts,
    })
}

fn check_receipt_header(
    input: &GoldenScoutInput,
    source: RetailChildSource,
    revision: u64,
    index: usize,
) -> Result<(), PrepareError> {
    if source != RetailChildSource::SupportedRetailExecutable {
        return Err(PrepareError::ReceiptSourceMismatch { index });
    }
    if revision != input.snapshot_revision {
        return Err(PrepareError::ReceiptRevisionMismatch { index });
    }
    Ok(())
}

fn reject_extra(receipts: &[ChildReceipt], index: usize) -> Result<(), PrepareError> {
    if index != receipts.len() {
        return Err(PrepareError::UnexpectedReceipt { index });
    }
    Ok(())
}

/// Prepare the exact golden human-Scout arm.  The function is pure: a returned residual has not
/// executed its external request, and a ready plan changes caller state only via
/// [`commit_no_cast`].
pub fn prepare_golden_scout_spellcaster(
    input: &GoldenScoutInput,
    receipts: &[ChildReceipt],
) -> Result<PrepareOutcome, PrepareError> {
    if input.executable_sha256 != SUPPORTED_RETAIL_EXE_SHA256 {
        return Err(PrepareError::UnsupportedExecutable);
    }
    if input.snapshot_revision == 0 {
        return Err(PrepareError::MissingSnapshotRevision);
    }
    if input.who != GOLDEN_OWNER || input.o != GOLDEN_SCOUT_O {
        return Err(PrepareError::WrongGoldenIdentity);
    }
    if input.type_index != SCOUT_TYPE {
        return Err(PrepareError::WrongScoutType);
    }
    if input.unit_type_flags2 & !IS_SPECIAL_MASK != GOLDEN_SCOUT_UNIT_FLAGS2 & !IS_SPECIAL_MASK
        || input.unit_domain != GOLDEN_SCOUT_DOMAIN
        || input.unit_type_mana != GOLDEN_SCOUT_BASE_MANA
        || input.counterintel_mana_cost != GOLDEN_COUNTERINTEL_MANA_COST
    {
        return Err(PrepareError::WrongGoldenRules);
    }
    if input.before.caster_active_spells_len != 0 {
        return Err(PrepareError::NonEmptyGoldenCasterArray);
    }

    if input.leader_flags & HUMAN_LEADER_MASK == 0 {
        reject_extra(receipts, 0)?;
        return Ok(residual(
            input,
            input.before,
            ExternalRequest::AiSpellcasterArm {
                entry_va: AI_SPELLCASTER_ARM_VA,
                who: i32::from(input.who),
                o: i32::from(input.o),
            },
            0,
            None,
        ));
    }

    if input.is_special_vfunc_va != UNIT_DATA_IS_SPECIAL_VA {
        reject_extra(receipts, 0)?;
        return Ok(residual(
            input,
            input.before,
            ExternalRequest::DynamicIsSpecial {
                function_va: input.is_special_vfunc_va,
                who: i32::from(input.who),
                o: i32::from(input.o),
            },
            0,
            None,
        ));
    }
    if input.unit_type_flags2 & IS_SPECIAL_MASK == 0 {
        reject_extra(receipts, 0)?;
        return Ok(ready(input, input.before, NoCastReason::NotSpecial, 0));
    }

    let castable_request = IsCastableRequest {
        callsite_va: SPELL_IS_CASTABLE_CALLSITE_VA,
        function_va: SPELL_IS_CASTABLE_VA,
        spell_type: COUNTERINTEL_SPELL,
        o: i32::from(input.o),
        who: i32::from(input.who),
        final_arg: 0,
    };
    let Some(first) = receipts.first() else {
        return Ok(residual(
            input,
            input.before,
            ExternalRequest::IsCastable(castable_request),
            0,
            None,
        ));
    };
    let ChildReceipt::IsCastable(castable) = *first else {
        return Err(PrepareError::ReceiptKindMismatch { index: 0 });
    };
    check_receipt_header(input, castable.source, castable.snapshot_revision, 0)?;
    if castable.request != castable_request {
        return Err(PrepareError::ReceiptRequestMismatch { index: 0 });
    }
    if castable.result == 0 {
        reject_extra(receipts, 1)?;
        return Ok(ready(
            input,
            input.before,
            NoCastReason::CounterintelNotCastable,
            1,
        ));
    }

    // Scout 69's source-exact UnitData::mana result is its +0x2EC base.  Domain 2 and a
    // dynamic/supply receiver could change that result and are therefore rejected.
    if input.unit_domain == 2
        || input.is_supply_vfunc_va != UNIT_DATA_IS_SUPPLY_VA
        || input.unit_type_flags2 & IS_SUPPLY_MASK != 0
    {
        return Err(PrepareError::UnsupportedScoutManaShape);
    }
    let required_mana = i32::from(input.mana_burn).wrapping_add(input.counterintel_mana_cost);
    if required_mana > input.mana_capacity() {
        reject_extra(receipts, 1)?;
        return Ok(ready(
            input,
            input.before,
            NoCastReason::InsufficientMana,
            1,
        ));
    }

    let range_request = SpellRangeRequest {
        callsite_va: SPELL_GET_RANGE_CALLSITE_VA,
        function_va: SPELL_GET_RANGE_VA,
        spell_type: COUNTERINTEL_SPELL,
        spell_slot_offset: COUNTERINTEL_SLOT_OFFSET,
        owner: i32::from(input.who),
        target_o: -1,
        target_owner: -1,
    };
    let Some(second) = receipts.get(1) else {
        return Ok(residual(
            input,
            input.before,
            ExternalRequest::SpellRange(range_request),
            1,
            None,
        ));
    };
    let ChildReceipt::SpellRange(range) = *second else {
        return Err(PrepareError::ReceiptKindMismatch { index: 1 });
    };
    check_receipt_header(input, range.source, range.snapshot_revision, 1)?;
    if range.request != range_request {
        return Err(PrepareError::ReceiptRequestMismatch { index: 1 });
    }

    let find_request = ObjectsFindRequest {
        callsite_va: OBJECTS_FIND_CALLSITE_VA,
        function_va: OBJECTS_FIND_VA,
        x: input.x(),
        y: input.y(),
        search_index_bh: 0,
        owner: i32::from(input.who),
        range: range.range,
        spell_slot_offset: COUNTERINTEL_SLOT_OFFSET,
        filter_index: FILTER_INDEX_20,
        type_index: COUNTERINTEL_SPELL,
        query_owner: i32::from(input.who),
        unread_args_10_to_12: UnreadStackWords::ThreeCompilerAlignmentWords,
        stop_on_first: 0,
    };
    let Some(third) = receipts.get(2) else {
        return Ok(residual(
            input,
            input.before,
            ExternalRequest::ObjectsFind(find_request),
            2,
            None,
        ));
    };
    let ChildReceipt::ObjectsFind(find) = *third else {
        return Err(PrepareError::ReceiptKindMismatch { index: 2 });
    };
    check_receipt_header(input, find.source, find.snapshot_revision, 2)?;
    if find.request != find_request {
        return Err(PrepareError::ReceiptRequestMismatch { index: 2 });
    }
    if find.objects_revision_before != input.before.objects_revision {
        return Err(PrepareError::ObjectsRevisionMismatch);
    }
    if find.objects_revision_after == 0
        || find.objects_revision_after == find.objects_revision_before
    {
        return Err(PrepareError::ObjectsRevisionDidNotAdvance);
    }
    if find.result_o < -1 {
        return Err(PrepareError::InvalidFindResult);
    }

    let mut staged = input.before;
    staged.objects_revision = find.objects_revision_after;
    staged.search_scratch = find.scratch_after;
    if find.result_o == -1 {
        if find.scratch_after
            != (ObjectsFindScratch {
                best_metric: OBJECTS_FIND_SENTINEL,
                selected_owner: i32::from(input.who),
            })
        {
            return Err(PrepareError::InvalidNoTargetScratch);
        }
        reject_extra(receipts, 3)?;
        return Ok(ready(input, staged, NoCastReason::NoTarget, 3));
    }

    if !(0..10).contains(&find.scratch_after.selected_owner) {
        return Err(PrepareError::InvalidFindResult);
    }
    reject_extra(receipts, 3)?;
    let add_request = AddCastOrderRequest {
        callsite_va: UNIT_ADD_CAST_ORDER_CALLSITE_VA,
        function_va: UNIT_ADD_CAST_ORDER_VA,
        actor_who: i32::from(input.who),
        actor_o: i32::from(input.o),
        target_o: find.result_o,
        target_owner: find.scratch_after.selected_owner,
        x: input.x(),
        y: input.y(),
        spell_type: COUNTERINTEL_SPELL,
        queue_pos: 0,
        final_flag: 0,
        allocated_order_index: CAST_ORDER_INDEX,
        target_uid_source: TargetUidSource::LiveObjectWordAtOffset0x30,
        order_offset_1c: 0,
        order_flag_04: false,
        order_list_offset: UNIT_ORDER_LIST_OFFSET,
        closes_orders: false,
        clears_partial_path: true,
        current_order_link_offset: UNIT_CURRENT_ORDER_LINK_OFFSET,
        calls_update_action: true,
    };
    Ok(residual(
        input,
        staged,
        ExternalRequest::AddCastOrder(add_request),
        3,
        Some(1),
    ))
}

/// Commit a complete no-cast transaction after revalidating the entire detached boundary.
/// Found-target plans cannot reach this API; they require the product host to transact the
/// `Unit::add_cast_order` child and its order/path/Guy side effects atomically.
pub fn commit_no_cast(
    boundary: &mut Frame0ScoutBoundary,
    prepared: PreparedNoCast,
) -> Result<(), CommitError> {
    if *boundary != prepared.before {
        return Err(CommitError::BoundaryChanged);
    }
    if prepared.after.unit_body_revision != prepared.before.unit_body_revision
        || prepared.after.unit_orders_revision != prepared.before.unit_orders_revision
        || prepared.after.unit_path_revision != prepared.before.unit_path_revision
        || prepared.after.guy_revision != prepared.before.guy_revision
        || prepared.after.leader_revision != prepared.before.leader_revision
        || prepared.after.world_revision != prepared.before.world_revision
        || prepared.after.rng_state != prepared.before.rng_state
        || prepared.after.caster_active_spells_revision
            != prepared.before.caster_active_spells_revision
        || prepared.after.caster_active_spells_len != prepared.before.caster_active_spells_len
        || prepared.retail_return != 0
        || prepared.return_va != NO_CAST_RETURN_VA
    {
        return Err(CommitError::PreparedInvariantBroken);
    }
    *boundary = prepared.after;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn boundary() -> Frame0ScoutBoundary {
        Frame0ScoutBoundary {
            unit_body_revision: 11,
            unit_orders_revision: 12,
            unit_path_revision: 13,
            guy_revision: 14,
            leader_revision: 15,
            world_revision: 16,
            objects_revision: 17,
            search_scratch: ObjectsFindScratch {
                best_metric: 123,
                selected_owner: 7,
            },
            rng_state: 0x1020_3040,
            caster_active_spells_revision: 19,
            caster_active_spells_len: 0,
        }
    }

    fn input() -> GoldenScoutInput {
        GoldenScoutInput {
            snapshot_revision: 9,
            executable_sha256: SUPPORTED_RETAIL_EXE_SHA256,
            who: GOLDEN_OWNER,
            o: GOLDEN_SCOUT_O,
            type_index: SCOUT_TYPE,
            leader_flags: HUMAN_LEADER_MASK,
            is_special_vfunc_va: UNIT_DATA_IS_SPECIAL_VA,
            is_supply_vfunc_va: UNIT_DATA_IS_SUPPLY_VA,
            unit_type_flags2: GOLDEN_SCOUT_UNIT_FLAGS2,
            unit_domain: GOLDEN_SCOUT_DOMAIN,
            unit_type_mana: GOLDEN_SCOUT_BASE_MANA,
            mana_burn: 0,
            counterintel_mana_cost: GOLDEN_COUNTERINTEL_MANA_COST,
            encoded_x: 0x1234 ^ OBJECT_COORD_XOR,
            encoded_y: 0x5678 ^ OBJECT_COORD_XOR,
            before: boundary(),
        }
    }

    fn castable_request(i: &GoldenScoutInput) -> IsCastableRequest {
        IsCastableRequest {
            callsite_va: SPELL_IS_CASTABLE_CALLSITE_VA,
            function_va: SPELL_IS_CASTABLE_VA,
            spell_type: COUNTERINTEL_SPELL,
            o: i32::from(i.o),
            who: i32::from(i.who),
            final_arg: 0,
        }
    }

    fn castable(i: &GoldenScoutInput, result: i32) -> ChildReceipt {
        ChildReceipt::IsCastable(IsCastableReceipt {
            source: RetailChildSource::SupportedRetailExecutable,
            snapshot_revision: i.snapshot_revision,
            request: castable_request(i),
            result,
        })
    }

    fn range_request(i: &GoldenScoutInput) -> SpellRangeRequest {
        SpellRangeRequest {
            callsite_va: SPELL_GET_RANGE_CALLSITE_VA,
            function_va: SPELL_GET_RANGE_VA,
            spell_type: COUNTERINTEL_SPELL,
            spell_slot_offset: COUNTERINTEL_SLOT_OFFSET,
            owner: i32::from(i.who),
            target_o: -1,
            target_owner: -1,
        }
    }

    fn range(i: &GoldenScoutInput, value: i32) -> ChildReceipt {
        ChildReceipt::SpellRange(SpellRangeReceipt {
            source: RetailChildSource::SupportedRetailExecutable,
            snapshot_revision: i.snapshot_revision,
            request: range_request(i),
            range: value,
        })
    }

    fn find_request(i: &GoldenScoutInput, range: i32) -> ObjectsFindRequest {
        ObjectsFindRequest {
            callsite_va: OBJECTS_FIND_CALLSITE_VA,
            function_va: OBJECTS_FIND_VA,
            x: i.x(),
            y: i.y(),
            search_index_bh: 0,
            owner: i32::from(i.who),
            range,
            spell_slot_offset: COUNTERINTEL_SLOT_OFFSET,
            filter_index: FILTER_INDEX_20,
            type_index: COUNTERINTEL_SPELL,
            query_owner: i32::from(i.who),
            unread_args_10_to_12: UnreadStackWords::ThreeCompilerAlignmentWords,
            stop_on_first: 0,
        }
    }

    fn find(
        i: &GoldenScoutInput,
        range: i32,
        result_o: i32,
        scratch_after: ObjectsFindScratch,
    ) -> ChildReceipt {
        ChildReceipt::ObjectsFind(ObjectsFindReceipt {
            source: RetailChildSource::SupportedRetailExecutable,
            snapshot_revision: i.snapshot_revision,
            request: find_request(i, range),
            objects_revision_before: i.before.objects_revision,
            objects_revision_after: 18,
            result_o,
            scratch_after,
        })
    }

    #[test]
    fn constants_pin_the_recovered_extent_and_human_arm() {
        assert_eq!(
            UNIT_THINK_SPELLCASTER_END_VA - UNIT_THINK_SPELLCASTER_VA,
            1_854
        );
        assert_eq!(HUMAN_COUNTERINTEL_ARM_VA, 0x005F_27C3);
        assert_eq!(AI_SPELLCASTER_ARM_VA, 0x005F_28CF);
        assert_eq!(COUNTERINTEL_SPELL, 631);
        assert_eq!(
            (
                GOLDEN_SCOUT_UNIT_FLAGS2,
                GOLDEN_SCOUT_DOMAIN,
                GOLDEN_SCOUT_BASE_MANA,
                GOLDEN_COUNTERINTEL_MANA_COST,
            ),
            (0x12, 0, 500, 500)
        );
        assert_eq!(
            UNIT_THINK_SPELLCASTER_SHA256,
            [
                0xfa, 0xfa, 0x5e, 0x59, 0x5e, 0x78, 0xe8, 0x40, 0xdc, 0x83, 0x69, 0x29, 0x3f, 0xb5,
                0x69, 0x09, 0x63, 0x8d, 0x41, 0xe4, 0x58, 0xfd, 0x38, 0x93, 0x54, 0xf7, 0xd2, 0xf9,
                0x87, 0x53, 0x39, 0x04,
            ]
        );
    }

    #[test]
    fn golden_first_external_child_is_counterintel_is_castable_and_rng_is_untouched() {
        let i = input();
        let result = prepare_golden_scout_spellcaster(&i, &[]).unwrap();
        let PrepareOutcome::ExternalRequired(r) = result else {
            panic!("expected typed child residual");
        };
        assert_eq!(r.request, ExternalRequest::IsCastable(castable_request(&i)));
        assert_eq!(r.staged, i.before);
        assert!(r.preserves_replay_critical_invariants(&i.before));
    }

    #[test]
    fn nonhuman_scout_goes_to_ai_residual_before_any_receipt_or_rng_draw() {
        let mut i = input();
        i.leader_flags = 0;
        let PrepareOutcome::ExternalRequired(r) =
            prepare_golden_scout_spellcaster(&i, &[]).unwrap()
        else {
            panic!("expected AI residual");
        };
        assert_eq!(
            r.request,
            ExternalRequest::AiSpellcasterArm {
                entry_va: AI_SPELLCASTER_ARM_VA,
                who: 0,
                o: 0,
            }
        );
        assert_eq!(r.staged.rng_state, i.before.rng_state);
    }

    #[test]
    fn not_special_and_not_castable_are_complete_no_mutation_returns() {
        let mut not_special = input();
        not_special.unit_type_flags2 &= !IS_SPECIAL_MASK;
        let PrepareOutcome::Ready(a) = prepare_golden_scout_spellcaster(&not_special, &[]).unwrap()
        else {
            panic!("expected no-cast plan");
        };
        assert_eq!(a.reason, NoCastReason::NotSpecial);
        assert_eq!(a.before, a.after);

        let i = input();
        let PrepareOutcome::Ready(b) =
            prepare_golden_scout_spellcaster(&i, &[castable(&i, 0)]).unwrap()
        else {
            panic!("expected no-cast plan");
        };
        assert_eq!(b.reason, NoCastReason::CounterintelNotCastable);
        assert_eq!(b.before, b.after);
    }

    #[test]
    fn scout_mana_gate_is_signed_and_stops_before_get_range() {
        let mut i = input();
        i.mana_burn = 1;
        let PrepareOutcome::Ready(p) =
            prepare_golden_scout_spellcaster(&i, &[castable(&i, 1)]).unwrap()
        else {
            panic!("expected insufficient-mana return");
        };
        assert_eq!(p.reason, NoCastReason::InsufficientMana);
        assert_eq!(p.consumed_receipts, 1);
    }

    #[test]
    fn accepted_castability_yields_exact_get_range_then_find_abi() {
        let i = input();
        let PrepareOutcome::ExternalRequired(range_residual) =
            prepare_golden_scout_spellcaster(&i, &[castable(&i, 1)]).unwrap()
        else {
            panic!("expected range residual");
        };
        assert_eq!(
            range_residual.request,
            ExternalRequest::SpellRange(range_request(&i))
        );

        let PrepareOutcome::ExternalRequired(find_residual) =
            prepare_golden_scout_spellcaster(&i, &[castable(&i, 1), range(&i, 960)]).unwrap()
        else {
            panic!("expected find residual");
        };
        assert_eq!(
            find_residual.request,
            ExternalRequest::ObjectsFind(find_request(&i, 960))
        );
        assert_eq!(find_residual.staged, i.before);
    }

    #[test]
    fn no_target_commits_only_objects_search_scratch() {
        let i = input();
        let no_target_scratch = ObjectsFindScratch {
            best_metric: OBJECTS_FIND_SENTINEL,
            selected_owner: 0,
        };
        let receipts = [
            castable(&i, 1),
            range(&i, 960),
            find(&i, 960, -1, no_target_scratch),
        ];
        let PrepareOutcome::Ready(p) = prepare_golden_scout_spellcaster(&i, &receipts).unwrap()
        else {
            panic!("expected complete no-target plan");
        };
        assert_eq!(p.reason, NoCastReason::NoTarget);
        assert_eq!(p.after.objects_revision, 18);
        assert_eq!(p.after.search_scratch, no_target_scratch);
        assert_eq!(p.after.unit_orders_revision, p.before.unit_orders_revision);
        assert_eq!(p.after.unit_path_revision, p.before.unit_path_revision);
        assert_eq!(p.after.guy_revision, p.before.guy_revision);
        assert_eq!(p.after.leader_revision, p.before.leader_revision);
        assert_eq!(p.after.world_revision, p.before.world_revision);
        assert_eq!(p.after.rng_state, p.before.rng_state);
        assert_eq!(
            p.after.caster_active_spells_revision,
            p.before.caster_active_spells_revision
        );
        let mut live = i.before;
        commit_no_cast(&mut live, p).unwrap();
        assert_eq!(live.objects_revision, 18);
        assert_eq!(live.search_scratch, no_target_scratch);
    }

    #[test]
    fn found_target_stops_at_exact_cast_order_without_guessing_post_state() {
        let i = input();
        let selected = ObjectsFindScratch {
            best_metric: 1234,
            selected_owner: 1,
        };
        let receipts = [castable(&i, 1), range(&i, 960), find(&i, 960, 44, selected)];
        let PrepareOutcome::ExternalRequired(r) =
            prepare_golden_scout_spellcaster(&i, &receipts).unwrap()
        else {
            panic!("expected add-cast-order residual");
        };
        assert_eq!(
            r.request,
            ExternalRequest::AddCastOrder(AddCastOrderRequest {
                callsite_va: UNIT_ADD_CAST_ORDER_CALLSITE_VA,
                function_va: UNIT_ADD_CAST_ORDER_VA,
                actor_who: 0,
                actor_o: 0,
                target_o: 44,
                target_owner: 1,
                x: 0x1234,
                y: 0x5678,
                spell_type: 631,
                queue_pos: 0,
                final_flag: 0,
                allocated_order_index: 14,
                target_uid_source: TargetUidSource::LiveObjectWordAtOffset0x30,
                order_offset_1c: 0,
                order_flag_04: false,
                order_list_offset: UNIT_ORDER_LIST_OFFSET,
                closes_orders: false,
                clears_partial_path: true,
                current_order_link_offset: UNIT_CURRENT_ORDER_LINK_OFFSET,
                calls_update_action: true,
            })
        );
        assert_eq!(r.retail_return_after_child, Some(1));
        assert_eq!(r.staged.objects_revision, 18);
        assert_eq!(r.staged.unit_orders_revision, i.before.unit_orders_revision);
        assert!(r.preserves_replay_critical_invariants(&i.before));
    }

    #[test]
    fn stale_or_mismatched_receipts_fail_closed() {
        let i = input();
        let mut stale = match castable(&i, 1) {
            ChildReceipt::IsCastable(r) => r,
            _ => unreachable!(),
        };
        stale.snapshot_revision += 1;
        assert_eq!(
            prepare_golden_scout_spellcaster(&i, &[ChildReceipt::IsCastable(stale)]),
            Err(PrepareError::ReceiptRevisionMismatch { index: 0 })
        );

        assert_eq!(
            prepare_golden_scout_spellcaster(&i, &[range(&i, 960)]),
            Err(PrepareError::ReceiptKindMismatch { index: 0 })
        );

        let mut untrusted = match castable(&i, 1) {
            ChildReceipt::IsCastable(r) => r,
            _ => unreachable!(),
        };
        untrusted.source = RetailChildSource::SyntheticOrUnknown;
        assert_eq!(
            prepare_golden_scout_spellcaster(&i, &[ChildReceipt::IsCastable(untrusted)]),
            Err(PrepareError::ReceiptSourceMismatch { index: 0 })
        );

        let mut wrong_rules = i;
        wrong_rules.unit_type_mana += 1;
        assert_eq!(
            prepare_golden_scout_spellcaster(&wrong_rules, &[]),
            Err(PrepareError::WrongGoldenRules)
        );

        let mut nonempty_caster = i;
        nonempty_caster.before.caster_active_spells_len = 1;
        assert_eq!(
            prepare_golden_scout_spellcaster(&nonempty_caster, &[]),
            Err(PrepareError::NonEmptyGoldenCasterArray)
        );
    }

    #[test]
    fn commit_revalidates_full_before_image_and_invariants() {
        let i = input();
        let PrepareOutcome::Ready(p) =
            prepare_golden_scout_spellcaster(&i, &[castable(&i, 0)]).unwrap()
        else {
            panic!("expected ready plan");
        };
        let mut changed = i.before;
        changed.unit_path_revision += 1;
        assert_eq!(
            commit_no_cast(&mut changed, p),
            Err(CommitError::BoundaryChanged)
        );
    }
}
