//! Golden-2024 frame-zero Scout spellcaster frontier.
//!
//! `Unit::think_spellcaster` (`0x005F27A0`, 1,854 bytes) has two top-level arms.  A
//! `LeaderData::flags & 4` receiver takes the short human arm at `0x005F27C3`; the much
//! larger difficulty/Spy/Sniper/RNG arm starts at `0x005F28CF` only when that bit is clear.
//! The supported replay's owner-zero Scout is human, so this module owns the former and
//! returns a typed residual for the latter.
//!
//! The human arm is small.  This module now owns the complete source-exact
//! `SpellTypeData::is_castable`, `UnitData::mana`, and `SpellTypeData::get_range` cone from
//! replay-carried Rules plus one adjacent live call-entry image.  The first unmounted owner is
//! the global `ObjectsData::find` spatial traversal.  A composition-bound native traversal
//! receipt may advance the detached transaction, but no caller state changes until
//! [`commit_no_cast`] accepts a complete no-cast plan.  A found target stops at an exact
//! `Unit::add_cast_order` request; the order/path/Guy after-image is not guessed.
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

/// Source-owned write-set receipt for the complete frame-zero spellcaster body.
///
/// `Unit::think_spellcaster` can write the Unit order/path owners and Objects search scratch,
/// but the complete function contains no read or write through `CasterData::active_spells`.
/// Keeping this as a typed receipt lets the frame-one chronology consume that narrow invariant
/// without pretending that the rest of frame zero was reconstructed offline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame0ScoutCasterWriteSetReceipt {
    pub executable_sha256: [u8; 32],
    pub body_va: u32,
    pub body_bytes: u32,
    pub body_sha256: [u8; 32],
    pub caster_active_spell_reads: u32,
    pub caster_active_spell_writes: u32,
    pub add_cast_order_va: u32,
    pub add_cast_order_bytes: u32,
    pub add_cast_order_sha256: [u8; 32],
    pub add_cast_order_caster_active_spell_writes: u32,
    pub successful_write_owner_offset: u32,
    pub successful_write_order_index: i32,
}

/// Return the immutable source receipt shared by every human-Scout outcome.
///
/// A successful Counterintel search opens `Unit::add_cast_order`; that owner is
/// `UnitData+0xCC`, not the Caster active-spell array.  Consequently the invariant is valid for
/// ready no-cast outcomes and every typed residual, including the found-target residual.
pub const fn frame0_scout_caster_write_set() -> Frame0ScoutCasterWriteSetReceipt {
    Frame0ScoutCasterWriteSetReceipt {
        executable_sha256: SUPPORTED_RETAIL_EXE_SHA256,
        body_va: UNIT_THINK_SPELLCASTER_VA,
        body_bytes: UNIT_THINK_SPELLCASTER_SIZE,
        body_sha256: UNIT_THINK_SPELLCASTER_SHA256,
        caster_active_spell_reads: 0,
        caster_active_spell_writes: 0,
        add_cast_order_va: UNIT_ADD_CAST_ORDER_VA,
        add_cast_order_bytes: UNIT_ADD_CAST_ORDER_SIZE,
        add_cast_order_sha256: UNIT_ADD_CAST_ORDER_SHA256,
        add_cast_order_caster_active_spell_writes: 0,
        successful_write_owner_offset: UNIT_ORDER_LIST_OFFSET,
        successful_write_order_index: CAST_ORDER_INDEX,
    }
}
pub const HUMAN_COUNTERINTEL_ARM_VA: u32 = 0x005F_27C3;
pub const AI_SPELLCASTER_ARM_VA: u32 = 0x005F_28CF;
pub const NO_CAST_RETURN_VA: u32 = 0x005F_2EB1;
pub const CAST_RETURN_ONE_VA: u32 = 0x005F_28C3;

pub const UNIT_DATA_IS_SPECIAL_VA: u32 = 0x0046_CEA0;
pub const UNIT_DATA_IS_SUPPLY_VA: u32 = 0x0046_CE80;
pub const OBJECT_DATA_IS_UNIT_TRUE_VA: u32 = 0x0041_E0E0;
pub const TYPE_DATA_IS_PACK_VA: u32 = 0x0047_0540;
pub const TYPE_DATA_IS_UNPACK_VA: u32 = 0x0047_0510;
pub const SPELL_IS_CASTABLE_VA: u32 = 0x0067_5BC0;
pub const UNIT_DATA_MANA_VA: u32 = 0x0060_9A50;
pub const SPELL_GET_RANGE_VA: u32 = 0x0067_6A80;
pub const OBJECTS_FIND_VA: u32 = 0x0065_C6B0;
pub const UNIT_ADD_CAST_ORDER_VA: u32 = 0x005E_4A60;
pub const UNIT_ADD_CAST_ORDER_SIZE: u32 = 541;
pub const UNIT_ADD_CAST_ORDER_SHA256: [u8; 32] = [
    0x6a, 0xcb, 0x03, 0xec, 0x4d, 0x92, 0x32, 0x63, 0xa5, 0x91, 0x11, 0xd4, 0xb8, 0x4f, 0x54, 0xc9,
    0xf7, 0xdc, 0xff, 0xb4, 0x54, 0x0d, 0xef, 0x5a, 0x94, 0x6b, 0xd6, 0xac, 0xdd, 0x32, 0x71, 0x58,
];
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
pub const GOLDEN_COUNTERINTEL_FROM_TYPE: i32 = 58;
pub const GOLDEN_COUNTERINTEL_FROM2_TYPE: i32 = SCOUT_TYPE;
pub const GOLDEN_COUNTERINTEL_FLAGS: u32 = 0x10B6;
pub const GOLDEN_COUNTERINTEL_RANGE: i32 = 1_920;
pub const GOLDEN_SPY_BRIBE_UPGRADE_RANGE: i32 = 2;
pub const GOLDEN_TERRA_COTTA_RANGE: i32 = 0;
pub const TERRA_COTTA_WONDER_TYPE: i32 = 0x211;
pub const RETAIL_CASTABLE_DEFAULT_RESULT: i32 = 3;
pub const GOLDEN_SERIALIZED_RULES_BYTES: usize = 1_024_221;
pub const CAST_ORDER_INDEX: i32 = 0x0E;
pub const UNIT_ORDER_LIST_OFFSET: u32 = 0xCC;
pub const UNIT_CURRENT_ORDER_LINK_OFFSET: u32 = 0xDC;
pub const FILTER_INDEX_20: i32 = 20;
pub const OBJECT_COORD_XOR: i32 = 0x0006_3637;
pub const IS_SPECIAL_MASK: u32 = 0x10;
pub const IS_SUPPLY_MASK: u32 = 0x40;
pub const HUMAN_LEADER_MASK: u32 = 0x04;
pub const OBJECTS_FIND_SENTINEL: i32 = 0x05F5_E0FF;

/// SHA-256 of the one supported 2024 replay file.  This transaction is intentionally not a
/// generic Scout policy: its serialized Rules provenance and live call-entry authority must
/// join this recording.
pub const GOLDEN_REPLAY_FILE_SHA256: [u8; 32] = [
    0x16, 0x90, 0x43, 0x1a, 0x5e, 0xf1, 0x9b, 0x38, 0xa3, 0x42, 0x5d, 0x3d, 0xd7, 0x31, 0x1e, 0x8e,
    0x83, 0xca, 0x0d, 0x27, 0xc5, 0x6f, 0xab, 0xe4, 0x9d, 0x77, 0x6a, 0x9f, 0x14, 0x21, 0xb2, 0x51,
];

/// SHA-256 of the supported retail executable whose PDB and code bytes define this port.
pub const SUPPORTED_RETAIL_EXE_SHA256: [u8; 32] = [
    0x30, 0x47, 0x8a, 0x44, 0xb5, 0x77, 0xcb, 0x11, 0xeb, 0xcb, 0xbb, 0xf5, 0x3d, 0x3e, 0x93, 0xba,
    0x02, 0xfd, 0x2a, 0xac, 0xf3, 0xbd, 0xef, 0xa6, 0x55, 0x2c, 0x9b, 0x64, 0x49, 0x62, 0x50, 0x79,
];

/// Replay byte ownership for one fixed retail-walked field family.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RulesByteSpan {
    pub offset: usize,
    pub bytes: usize,
}

/// Exact replay-carried Rules projection used by the Counterintel cone.
///
/// A replay adapter must construct this from the admitted serialized Rules section.  Keeping
/// the spans and whole-section digest here prevents copied scalar values from masquerading as
/// authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GoldenCounterintelRules {
    pub replay_file_sha256: [u8; 32],
    pub serialized_rules_sha256: [u8; 32],
    pub serialized_rules_span: RulesByteSpan,
    pub scout_object_span: RulesByteSpan,
    pub scout_unit_span: RulesByteSpan,
    pub counterintel_type_base_span: RulesByteSpan,
    pub counterintel_spell_span: RulesByteSpan,
    pub constants_span: RulesByteSpan,
    pub unit_type_flags2: u32,
    pub unit_domain: i32,
    pub unit_type_mana: i32,
    pub spell_type: i32,
    pub from_type: i32,
    pub from2_type: i32,
    pub spell_flags: u32,
    pub spell_range: i32,
    pub spell_mana: i32,
    pub spy_bribe_upgrade_range: i32,
    pub terra_cotta_range: i32,
}

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
    /// Digest of the complete adjacent retail call-entry composition.  This is independent of
    /// the completed setup digest: earlier frame-zero receivers may have changed live state.
    pub call_entry_composition_digest: [u8; 32],
    pub executable_sha256: [u8; 32],
    pub who: u8,
    pub o: i16,
    pub type_index: i32,
    /// `LeaderData::flags`, tested with bit `4` at `0x005F27B6`.
    pub leader_flags: u32,
    /// Live `UnitData+0x68`. Bit zero selects the pack/unpack exclusion inside
    /// `SpellTypeData::is_castable`.
    pub unit_masks: u32,
    /// Runtime vtable slot `+0xD4`.  The ordinary Unit implementation is admitted directly.
    pub is_special_vfunc_va: u32,
    /// Runtime vtable slot `+0xCC`, used by `UnitData::mana`.
    pub is_supply_vfunc_va: u32,
    /// Runtime vtable slot `+0x18`, consumed by `SpellTypeData::is_castable`.
    pub is_unit_vfunc_va: u32,
    /// Counterintel vtable slots `+0x50/+0x54`, reached only when `unit_masks & 1 != 0`.
    pub spell_is_pack_vfunc_va: u32,
    pub spell_is_unpack_vfunc_va: u32,
    /// Exact replay-carried static source.
    pub rules: GoldenCounterintelRules,
    /// Signed `UnitData+0x96`.
    pub mana_burn: i16,
    /// Exact result of `LeaderData::get_spy_upgrade` at this call entry.
    pub spy_upgrade: i32,
    /// Exact `LeaderData::has_wonder(0x211)` result.  Retail performs the call even though the
    /// shipped Terra Cotta range constant is zero.
    pub has_terra_cotta: bool,
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
        self.rules.unit_type_mana
    }
}

fn span_is_inside(inner: RulesByteSpan, outer: RulesByteSpan) -> bool {
    let Some(inner_end) = inner.offset.checked_add(inner.bytes) else {
        return false;
    };
    let Some(outer_end) = outer.offset.checked_add(outer.bytes) else {
        return false;
    };
    inner.bytes != 0 && inner.offset >= outer.offset && inner_end <= outer_end
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetailChildSource {
    /// Supported retail executed the complete `ObjectsData::find` call synchronously from
    /// `Unit::think_spellcaster` at `0x005F2884`.
    CompleteRetailObjectsFindAtGoldenScoutCounterintel,
    SyntheticOrUnknown,
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
    pub call_entry_composition_digest: [u8; 32],
    /// SHA-256 over the complete ordered spatial-cell candidate chain and every live field read
    /// by SearchIndexBH(0), FilterIndex(20), and Counterintel `is_valid_target`.  Don does not
    /// invent this chain while the canonical object index remains unmounted.
    pub candidate_traversal_sha256: [u8; 32],
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
    ObjectsFind(ObjectsFindRequest),
    AddCastOrder(AddCastOrderRequest),
}

/// The static retail children are owned locally.  The only admissible child authority is the
/// complete `ObjectsData::find` traversal at the exact Counterintel callsite.
pub type ChildReceipt = ObjectsFindReceipt;

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

/// Canonical setup owner which established the empty Caster queue before frame zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame0ScoutCasterSetupJoin {
    pub completed_init_revision: u64,
    pub completed_init_digest: [u8; 32],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame0ScoutCasterInvariantBranch {
    ReturnedNoCast(NoCastReason),
    /// The exact human body reached its final Unit-order child.  That child owns Unit orders,
    /// path, Guy/action, and target UID state; it cannot alias `CasterData::active_spells`.
    AddCastOrderOwnerResidual,
}

/// Narrow authority that the complete bounded human-Scout transaction preserved the Caster
/// active-spell owner.  It deliberately makes no claim about Unit order/path or Objects search
/// scratch, which remain in the enclosing prepared outcome and its sibling receipt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame0ScoutCasterInvariantAuthority {
    pub setup: Frame0ScoutCasterSetupJoin,
    /// Adjacent entry revision/digest copied from the exact [`GoldenScoutInput`], not derived
    /// from or equated with completed setup.
    pub call_entry_revision: u64,
    pub call_entry_composition_digest: [u8; 32],
    pub executable_sha256: [u8; 32],
    pub who: u8,
    pub o: i16,
    pub type_index: i32,
    pub branch: Frame0ScoutCasterInvariantBranch,
    pub consumed_child_receipts: usize,
    pub before_revision: u64,
    pub before_length: u32,
    pub after_revision: u64,
    pub after_length: u32,
    pub write_set: Frame0ScoutCasterWriteSetReceipt,
    pub caster_owner_transaction_complete: bool,
    pub unit_order_and_search_scratch_invariance_claimed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrepareError {
    UnsupportedExecutable,
    WrongGoldenReplay,
    MissingRulesAuthority,
    MissingCallEntryCompositionDigest,
    MissingSnapshotRevision,
    WrongGoldenIdentity,
    WrongScoutType,
    WrongGoldenRules,
    NonEmptyGoldenCasterArray,
    UnsupportedCastabilityShape,
    UnsupportedScoutManaShape,
    ReceiptSourceMismatch { index: usize },
    ReceiptRevisionMismatch { index: usize },
    ReceiptRequestMismatch { index: usize },
    ReceiptCompositionMismatch { index: usize },
    MissingCandidateTraversalDigest,
    ObjectsRevisionMismatch,
    ObjectsRevisionDidNotAdvance,
    InvalidFindResult,
    InvalidNoTargetScratch,
    UnexpectedReceipt { index: usize },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame0ScoutCasterInvariantError {
    Prepare(PrepareError),
    MissingSetupJoin,
    SetupRevisionMismatch,
    RelabeledSetupAsCallEntry,
    HumanTransactionStillOpen,
    NonHumanOrDynamicBranch,
    CasterOwnerChanged,
    RngOwnerChanged,
}

impl fmt::Display for Frame0ScoutCasterInvariantError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "frame-zero Scout Caster invariant refused: {self:?}")
    }
}

impl std::error::Error for Frame0ScoutCasterInvariantError {}

impl From<PrepareError> for Frame0ScoutCasterInvariantError {
    fn from(value: PrepareError) -> Self {
        Self::Prepare(value)
    }
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
    if source != RetailChildSource::CompleteRetailObjectsFindAtGoldenScoutCounterintel {
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
    if input.rules.replay_file_sha256 != GOLDEN_REPLAY_FILE_SHA256 {
        return Err(PrepareError::WrongGoldenReplay);
    }
    if input.rules.serialized_rules_sha256 == [0; 32]
        || input.rules.serialized_rules_span.bytes != GOLDEN_SERIALIZED_RULES_BYTES
        || input.rules.scout_object_span.bytes != 152
        || input.rules.scout_unit_span.bytes != 792
        || input.rules.counterintel_type_base_span.bytes != 90
        || input.rules.counterintel_spell_span.bytes != 48
        || input.rules.constants_span.bytes != 0x0D40
        || !span_is_inside(
            input.rules.scout_object_span,
            input.rules.serialized_rules_span,
        )
        || !span_is_inside(
            input.rules.scout_unit_span,
            input.rules.serialized_rules_span,
        )
        || !span_is_inside(
            input.rules.counterintel_type_base_span,
            input.rules.serialized_rules_span,
        )
        || !span_is_inside(
            input.rules.counterintel_spell_span,
            input.rules.serialized_rules_span,
        )
        || !span_is_inside(
            input.rules.constants_span,
            input.rules.serialized_rules_span,
        )
    {
        return Err(PrepareError::MissingRulesAuthority);
    }
    if input.call_entry_composition_digest == [0; 32] {
        return Err(PrepareError::MissingCallEntryCompositionDigest);
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
    if input.rules.unit_type_flags2 != GOLDEN_SCOUT_UNIT_FLAGS2
        || input.rules.unit_domain != GOLDEN_SCOUT_DOMAIN
        || input.rules.unit_type_mana != GOLDEN_SCOUT_BASE_MANA
        || input.rules.spell_type != COUNTERINTEL_SPELL
        || input.rules.from_type != GOLDEN_COUNTERINTEL_FROM_TYPE
        || input.rules.from2_type != GOLDEN_COUNTERINTEL_FROM2_TYPE
        || input.rules.spell_flags != GOLDEN_COUNTERINTEL_FLAGS
        || input.rules.spell_range != GOLDEN_COUNTERINTEL_RANGE
        || input.rules.spell_mana != GOLDEN_COUNTERINTEL_MANA_COST
        || input.rules.spy_bribe_upgrade_range != GOLDEN_SPY_BRIBE_UPGRADE_RANGE
        || input.rules.terra_cotta_range != GOLDEN_TERRA_COTTA_RANGE
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
    if input.rules.unit_type_flags2 & IS_SPECIAL_MASK == 0 {
        reject_extra(receipts, 0)?;
        return Ok(ready(input, input.before, NoCastReason::NotSpecial, 0));
    }

    // `SpellTypeData::is_castable(0,0,0)` first proves the live receiver is a Unit, then
    // matches Counterintel's direct `from2=Scout(69)` relation.  Its switch default returns 3.
    // Only `unit_masks & 1` diverts through the spell's pack/unpack predicates; type 631 is
    // neither, so that arm returns zero.  There are no writes or RNG calls in either arm.
    if input.is_unit_vfunc_va != OBJECT_DATA_IS_UNIT_TRUE_VA
        || input.spell_is_pack_vfunc_va != TYPE_DATA_IS_PACK_VA
        || input.spell_is_unpack_vfunc_va != TYPE_DATA_IS_UNPACK_VA
    {
        return Err(PrepareError::UnsupportedCastabilityShape);
    }
    let castable_result = if input.unit_masks & 1 == 0 {
        RETAIL_CASTABLE_DEFAULT_RESULT
    } else {
        0
    };
    if castable_result == 0 {
        reject_extra(receipts, 0)?;
        return Ok(ready(
            input,
            input.before,
            NoCastReason::CounterintelNotCastable,
            0,
        ));
    }

    // Scout 69's source-exact UnitData::mana result is its +0x2EC base.  Domain 2 and a
    // dynamic/supply receiver could change that result and are therefore rejected.
    if input.rules.unit_domain == 2
        || input.is_supply_vfunc_va != UNIT_DATA_IS_SUPPLY_VA
        || input.rules.unit_type_flags2 & IS_SUPPLY_MASK != 0
    {
        return Err(PrepareError::UnsupportedScoutManaShape);
    }
    let required_mana = i32::from(input.mana_burn).wrapping_add(input.rules.spell_mana);
    if required_mana > input.mana_capacity() {
        reject_extra(receipts, 0)?;
        return Ok(ready(
            input,
            input.before,
            NoCastReason::InsufficientMana,
            0,
        ));
    }

    // `get_range` calls both live Leader children in this order.  The target `-1` arm has no
    // cap.  Preserve retail's wrapping i32 arithmetic even though the golden values are small.
    let range = input
        .rules
        .spell_range
        .wrapping_add(
            input
                .spy_upgrade
                .wrapping_mul(input.rules.spy_bribe_upgrade_range)
                .wrapping_mul(192),
        )
        .wrapping_add(if input.has_terra_cotta {
            input.rules.terra_cotta_range.wrapping_mul(192)
        } else {
            0
        });

    let find_request = ObjectsFindRequest {
        callsite_va: OBJECTS_FIND_CALLSITE_VA,
        function_va: OBJECTS_FIND_VA,
        x: input.x(),
        y: input.y(),
        search_index_bh: 0,
        owner: i32::from(input.who),
        range,
        spell_slot_offset: COUNTERINTEL_SLOT_OFFSET,
        filter_index: FILTER_INDEX_20,
        type_index: COUNTERINTEL_SPELL,
        query_owner: i32::from(input.who),
        unread_args_10_to_12: UnreadStackWords::ThreeCompilerAlignmentWords,
        stop_on_first: 0,
    };
    let Some(first) = receipts.first() else {
        return Ok(residual(
            input,
            input.before,
            ExternalRequest::ObjectsFind(find_request),
            0,
            None,
        ));
    };
    let find = *first;
    check_receipt_header(input, find.source, find.snapshot_revision, 0)?;
    if find.call_entry_composition_digest != input.call_entry_composition_digest {
        return Err(PrepareError::ReceiptCompositionMismatch { index: 0 });
    }
    if find.candidate_traversal_sha256 == [0; 32] {
        return Err(PrepareError::MissingCandidateTraversalDigest);
    }
    if find.request != find_request {
        return Err(PrepareError::ReceiptRequestMismatch { index: 0 });
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
        reject_extra(receipts, 1)?;
        return Ok(ready(input, staged, NoCastReason::NoTarget, 1));
    }

    if !(0..10).contains(&find.scratch_after.selected_owner) {
        return Err(PrepareError::InvalidFindResult);
    }
    reject_extra(receipts, 1)?;
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
        1,
        Some(1),
    ))
}

/// Execute the pure frame-zero planner and publish its narrow Caster-owner invariant.
///
/// Intermediate dynamic-child requests are not enough: the bounded transaction must either
/// have returned through a complete no-cast arm or reached the final `Unit::add_cast_order`
/// owner boundary.  The latter leaves the Caster authority complete while the distinct Unit
/// order/search-scratch transaction remains open for its sibling receipt.
pub fn bind_frame0_scout_caster_invariant(
    input: &GoldenScoutInput,
    receipts: &[ChildReceipt],
    setup: Frame0ScoutCasterSetupJoin,
) -> Result<(PrepareOutcome, Frame0ScoutCasterInvariantAuthority), Frame0ScoutCasterInvariantError>
{
    if setup.completed_init_revision == 0 || setup.completed_init_digest == [0; 32] {
        return Err(Frame0ScoutCasterInvariantError::MissingSetupJoin);
    }
    if input.before.caster_active_spells_revision != setup.completed_init_revision {
        return Err(Frame0ScoutCasterInvariantError::SetupRevisionMismatch);
    }
    if input.call_entry_composition_digest == setup.completed_init_digest {
        return Err(Frame0ScoutCasterInvariantError::RelabeledSetupAsCallEntry);
    }
    let outcome = prepare_golden_scout_spellcaster(input, receipts)?;
    let (branch, consumed_child_receipts, after) = match outcome {
        PrepareOutcome::Ready(ready) => (
            Frame0ScoutCasterInvariantBranch::ReturnedNoCast(ready.reason),
            ready.consumed_receipts,
            ready.after,
        ),
        PrepareOutcome::ExternalRequired(residual) => match residual.request {
            ExternalRequest::AddCastOrder(_) => (
                Frame0ScoutCasterInvariantBranch::AddCastOrderOwnerResidual,
                residual.consumed_receipts,
                residual.staged,
            ),
            ExternalRequest::AiSpellcasterArm { .. } | ExternalRequest::DynamicIsSpecial { .. } => {
                return Err(Frame0ScoutCasterInvariantError::NonHumanOrDynamicBranch)
            }
            ExternalRequest::ObjectsFind(_) => {
                return Err(Frame0ScoutCasterInvariantError::HumanTransactionStillOpen)
            }
        },
    };
    if after.caster_active_spells_revision != input.before.caster_active_spells_revision
        || after.caster_active_spells_len != input.before.caster_active_spells_len
    {
        return Err(Frame0ScoutCasterInvariantError::CasterOwnerChanged);
    }
    if after.rng_state != input.before.rng_state {
        return Err(Frame0ScoutCasterInvariantError::RngOwnerChanged);
    }
    let authority = Frame0ScoutCasterInvariantAuthority {
        setup,
        call_entry_revision: input.snapshot_revision,
        call_entry_composition_digest: input.call_entry_composition_digest,
        executable_sha256: input.executable_sha256,
        who: input.who,
        o: input.o,
        type_index: input.type_index,
        branch,
        consumed_child_receipts,
        before_revision: input.before.caster_active_spells_revision,
        before_length: input.before.caster_active_spells_len,
        after_revision: after.caster_active_spells_revision,
        after_length: after.caster_active_spells_len,
        write_set: frame0_scout_caster_write_set(),
        caster_owner_transaction_complete: true,
        unit_order_and_search_scratch_invariance_claimed: false,
    };
    Ok((outcome, authority))
}

/// Structural gate for downstream chronology adapters which did not execute the planner.
pub fn validate_frame0_scout_caster_invariant(
    authority: &Frame0ScoutCasterInvariantAuthority,
) -> bool {
    authority.setup.completed_init_revision != 0
        && authority.setup.completed_init_digest != [0; 32]
        && authority.call_entry_revision != 0
        && authority.call_entry_composition_digest != [0; 32]
        && authority.call_entry_composition_digest != authority.setup.completed_init_digest
        && authority.executable_sha256 == SUPPORTED_RETAIL_EXE_SHA256
        && authority.who == GOLDEN_OWNER
        && authority.o == GOLDEN_SCOUT_O
        && authority.type_index == SCOUT_TYPE
        && authority.before_revision == authority.setup.completed_init_revision
        && authority.before_length == 0
        && authority.after_revision == authority.before_revision
        && authority.after_length == authority.before_length
        && match authority.branch {
            Frame0ScoutCasterInvariantBranch::ReturnedNoCast(NoCastReason::NotSpecial)
            | Frame0ScoutCasterInvariantBranch::ReturnedNoCast(
                NoCastReason::CounterintelNotCastable,
            )
            | Frame0ScoutCasterInvariantBranch::ReturnedNoCast(NoCastReason::InsufficientMana) => {
                authority.consumed_child_receipts == 0
            }
            Frame0ScoutCasterInvariantBranch::ReturnedNoCast(NoCastReason::NoTarget)
            | Frame0ScoutCasterInvariantBranch::AddCastOrderOwnerResidual => {
                authority.consumed_child_receipts == 1
            }
        }
        && authority.write_set == frame0_scout_caster_write_set()
        && authority.caster_owner_transaction_complete
        && !authority.unit_order_and_search_scratch_invariance_claimed
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

    #[test]
    fn complete_frame_zero_body_publishes_the_narrow_caster_write_set() {
        let receipt = frame0_scout_caster_write_set();
        assert_eq!(receipt.executable_sha256, SUPPORTED_RETAIL_EXE_SHA256);
        assert_eq!(receipt.body_va, UNIT_THINK_SPELLCASTER_VA);
        assert_eq!(receipt.body_bytes, UNIT_THINK_SPELLCASTER_SIZE);
        assert_eq!(receipt.body_sha256, UNIT_THINK_SPELLCASTER_SHA256);
        assert_eq!(receipt.caster_active_spell_reads, 0);
        assert_eq!(receipt.caster_active_spell_writes, 0);
        assert_eq!(receipt.add_cast_order_va, UNIT_ADD_CAST_ORDER_VA);
        assert_eq!(receipt.add_cast_order_bytes, UNIT_ADD_CAST_ORDER_SIZE);
        assert_eq!(receipt.add_cast_order_sha256, UNIT_ADD_CAST_ORDER_SHA256);
        assert_eq!(receipt.add_cast_order_caster_active_spell_writes, 0);
        assert_eq!(
            receipt.successful_write_owner_offset,
            UNIT_ORDER_LIST_OFFSET
        );
        assert_eq!(receipt.successful_write_order_index, CAST_ORDER_INDEX);
    }

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
            call_entry_composition_digest: [4; 32],
            executable_sha256: SUPPORTED_RETAIL_EXE_SHA256,
            who: GOLDEN_OWNER,
            o: GOLDEN_SCOUT_O,
            type_index: SCOUT_TYPE,
            leader_flags: HUMAN_LEADER_MASK,
            unit_masks: 0,
            is_special_vfunc_va: UNIT_DATA_IS_SPECIAL_VA,
            is_supply_vfunc_va: UNIT_DATA_IS_SUPPLY_VA,
            is_unit_vfunc_va: OBJECT_DATA_IS_UNIT_TRUE_VA,
            spell_is_pack_vfunc_va: TYPE_DATA_IS_PACK_VA,
            spell_is_unpack_vfunc_va: TYPE_DATA_IS_UNPACK_VA,
            rules: GoldenCounterintelRules {
                replay_file_sha256: GOLDEN_REPLAY_FILE_SHA256,
                serialized_rules_sha256: [3; 32],
                serialized_rules_span: RulesByteSpan {
                    offset: 0,
                    bytes: GOLDEN_SERIALIZED_RULES_BYTES,
                },
                scout_object_span: RulesByteSpan {
                    offset: 100,
                    bytes: 152,
                },
                scout_unit_span: RulesByteSpan {
                    offset: 252,
                    bytes: 792,
                },
                counterintel_type_base_span: RulesByteSpan {
                    offset: 2_000,
                    bytes: 90,
                },
                counterintel_spell_span: RulesByteSpan {
                    offset: 2_100,
                    bytes: 48,
                },
                constants_span: RulesByteSpan {
                    offset: 500_335,
                    bytes: 0x0D40,
                },
                unit_type_flags2: GOLDEN_SCOUT_UNIT_FLAGS2,
                unit_domain: GOLDEN_SCOUT_DOMAIN,
                unit_type_mana: GOLDEN_SCOUT_BASE_MANA,
                spell_type: COUNTERINTEL_SPELL,
                from_type: GOLDEN_COUNTERINTEL_FROM_TYPE,
                from2_type: GOLDEN_COUNTERINTEL_FROM2_TYPE,
                spell_flags: GOLDEN_COUNTERINTEL_FLAGS,
                spell_range: GOLDEN_COUNTERINTEL_RANGE,
                spell_mana: GOLDEN_COUNTERINTEL_MANA_COST,
                spy_bribe_upgrade_range: GOLDEN_SPY_BRIBE_UPGRADE_RANGE,
                terra_cotta_range: GOLDEN_TERRA_COTTA_RANGE,
            },
            mana_burn: 0,
            spy_upgrade: 0,
            has_terra_cotta: false,
            encoded_x: 0x1234 ^ OBJECT_COORD_XOR,
            encoded_y: 0x5678 ^ OBJECT_COORD_XOR,
            before: boundary(),
        }
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
        ObjectsFindReceipt {
            source: RetailChildSource::CompleteRetailObjectsFindAtGoldenScoutCounterintel,
            snapshot_revision: i.snapshot_revision,
            call_entry_composition_digest: i.call_entry_composition_digest,
            candidate_traversal_sha256: [5; 32],
            request: find_request(i, range),
            objects_revision_before: i.before.objects_revision,
            objects_revision_after: 18,
            result_o,
            scratch_after,
        }
    }

    #[test]
    fn completed_human_transaction_binds_only_the_caster_owner_invariant() {
        let i = input();
        let setup = Frame0ScoutCasterSetupJoin {
            completed_init_revision: i.before.caster_active_spells_revision,
            completed_init_digest: [0x51; 32],
        };
        let mut no_cast = i;
        no_cast.unit_masks = 1;
        let (outcome, authority) =
            bind_frame0_scout_caster_invariant(&no_cast, &[], setup).unwrap();
        assert!(matches!(
            outcome,
            PrepareOutcome::Ready(PreparedNoCast {
                reason: NoCastReason::CounterintelNotCastable,
                ..
            })
        ));
        assert!(validate_frame0_scout_caster_invariant(&authority));
        assert_eq!(authority.call_entry_revision, no_cast.snapshot_revision);
        assert_eq!(
            authority.call_entry_composition_digest,
            no_cast.call_entry_composition_digest
        );
        assert!(authority.caster_owner_transaction_complete);
        assert!(!authority.unit_order_and_search_scratch_invariance_claimed);

        let relabeled_setup = Frame0ScoutCasterSetupJoin {
            completed_init_digest: i.call_entry_composition_digest,
            ..setup
        };
        assert_eq!(
            bind_frame0_scout_caster_invariant(&no_cast, &[], relabeled_setup),
            Err(Frame0ScoutCasterInvariantError::RelabeledSetupAsCallEntry)
        );

        assert_eq!(
            bind_frame0_scout_caster_invariant(&i, &[], setup),
            Err(Frame0ScoutCasterInvariantError::HumanTransactionStillOpen)
        );

        let found = [find(
            &i,
            GOLDEN_COUNTERINTEL_RANGE,
            4,
            ObjectsFindScratch {
                best_metric: 77,
                selected_owner: 1,
            },
        )];
        let (outcome, authority) = bind_frame0_scout_caster_invariant(&i, &found, setup).unwrap();
        assert!(matches!(
            outcome,
            PrepareOutcome::ExternalRequired(TypedResidual {
                request: ExternalRequest::AddCastOrder(_),
                ..
            })
        ));
        assert_eq!(
            authority.branch,
            Frame0ScoutCasterInvariantBranch::AddCastOrderOwnerResidual
        );
        assert_eq!(authority.consumed_child_receipts, 1);
        assert!(validate_frame0_scout_caster_invariant(&authority));
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
    fn source_owned_castability_mana_and_range_reach_objects_find_without_rng() {
        let i = input();
        let result = prepare_golden_scout_spellcaster(&i, &[]).unwrap();
        let PrepareOutcome::ExternalRequired(r) = result else {
            panic!("expected typed child residual");
        };
        assert_eq!(
            r.request,
            ExternalRequest::ObjectsFind(find_request(&i, GOLDEN_COUNTERINTEL_RANGE))
        );
        assert_eq!(r.staged, i.before);
        assert_eq!(r.consumed_receipts, 0);
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
    fn packed_unit_mask_is_exact_not_castable_no_mutation_return() {
        let mut i = input();
        i.unit_masks = 1;
        let PrepareOutcome::Ready(b) = prepare_golden_scout_spellcaster(&i, &[]).unwrap() else {
            panic!("expected no-cast plan");
        };
        assert_eq!(b.reason, NoCastReason::CounterintelNotCastable);
        assert_eq!(b.before, b.after);
        assert_eq!(b.consumed_receipts, 0);
    }

    #[test]
    fn scout_mana_gate_is_signed_and_stops_before_get_range() {
        let mut i = input();
        i.mana_burn = 1;
        let PrepareOutcome::Ready(p) = prepare_golden_scout_spellcaster(&i, &[]).unwrap() else {
            panic!("expected insufficient-mana return");
        };
        assert_eq!(p.reason, NoCastReason::InsufficientMana);
        assert_eq!(p.consumed_receipts, 0);
    }

    #[test]
    fn exact_spy_upgrade_range_is_folded_before_find_abi() {
        let mut i = input();
        i.spy_upgrade = 1;
        let PrepareOutcome::ExternalRequired(find_residual) =
            prepare_golden_scout_spellcaster(&i, &[]).unwrap()
        else {
            panic!("expected find residual");
        };
        assert_eq!(
            find_residual.request,
            ExternalRequest::ObjectsFind(find_request(&i, 2_304))
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
        let receipts = [find(&i, GOLDEN_COUNTERINTEL_RANGE, -1, no_target_scratch)];
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
        let receipts = [find(&i, GOLDEN_COUNTERINTEL_RANGE, 44, selected)];
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
        let no_target = ObjectsFindScratch {
            best_metric: OBJECTS_FIND_SENTINEL,
            selected_owner: 0,
        };
        let mut stale = find(&i, GOLDEN_COUNTERINTEL_RANGE, -1, no_target);
        stale.snapshot_revision += 1;
        assert_eq!(
            prepare_golden_scout_spellcaster(&i, &[stale]),
            Err(PrepareError::ReceiptRevisionMismatch { index: 0 })
        );

        let mut untrusted = find(&i, GOLDEN_COUNTERINTEL_RANGE, -1, no_target);
        untrusted.source = RetailChildSource::SyntheticOrUnknown;
        assert_eq!(
            prepare_golden_scout_spellcaster(&i, &[untrusted]),
            Err(PrepareError::ReceiptSourceMismatch { index: 0 })
        );

        let mut wrong_composition = find(&i, GOLDEN_COUNTERINTEL_RANGE, -1, no_target);
        wrong_composition.call_entry_composition_digest[0] ^= 1;
        assert_eq!(
            prepare_golden_scout_spellcaster(&i, &[wrong_composition]),
            Err(PrepareError::ReceiptCompositionMismatch { index: 0 })
        );

        let mut incomplete = find(&i, GOLDEN_COUNTERINTEL_RANGE, -1, no_target);
        incomplete.candidate_traversal_sha256 = [0; 32];
        assert_eq!(
            prepare_golden_scout_spellcaster(&i, &[incomplete]),
            Err(PrepareError::MissingCandidateTraversalDigest)
        );

        let mut wrong_rules = i;
        wrong_rules.rules.unit_type_mana += 1;
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
        let mut i = input();
        i.unit_masks = 1;
        let PrepareOutcome::Ready(p) = prepare_golden_scout_spellcaster(&i, &[]).unwrap() else {
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
