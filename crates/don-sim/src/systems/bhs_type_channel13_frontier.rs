//! Fail-closed channel-13 projection for the canonical BHS mutable type owner.
//!
//! This frontier deliberately consumes normalized `walk_rules_data` segments instead of a raw
//! C++ object image.  Bytes belonging to `String`, vptrs, pointers, padding, and derived caches
//! therefore cannot leak into the checksum by accident.  The caller must separately prove the
//! retail String gate, virtual-dispatch bands, and cache exclusion before projection is admitted.

#![forbid(unsafe_code)]

use std::fmt;

use super::bhs_type_factory::{Sha256Digest, TypeBuiltinProvenance};
use super::bhs_type_table::{TypeBody, TypeBuiltinState, TypeRow, NUM_TYPES};

pub const RETAIL_AFTER_TYPES: u32 = 0x72e0_c3b6;
pub const RETAIL_TYPE_WALKED_BYTES: u64 = 473_984;
pub const SHIPPED_OBJECT_ARRAY_ELEMENTS: usize = 2_363;

const TYPE_BASE_START: usize = 4;
const TYPE_BASE_LEN: usize = 90;
const OBJECT_START: usize = 484;
const OBJECT_LEN: usize = 152;

/// The seven effective virtual walkers reached by `Types::walk_rules_data`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TypeRuleKind {
    Good,
    Unit,
    Build,
    Object,
    Tech,
    Spell,
    Type,
}

impl TypeRuleKind {
    pub const fn for_shipped_slot(slot: usize) -> Option<Self> {
        match slot {
            0..=49 => Some(Self::Good),
            50..=413 => Some(Self::Unit),
            414..=542 => Some(Self::Build),
            543 => Some(Self::Object),
            544..=628 => Some(Self::Tech),
            629..=683 => Some(Self::Spell),
            684..=805 => Some(Self::Type),
            _ => None,
        }
    }

    const fn uses_object_walk(self) -> bool {
        matches!(self, Self::Good | Self::Unit | Self::Build | Self::Object)
    }
}

/// Proof that `String::walk_data` is reached with `DataWalk::is_checksum != 0` and emits no
/// bytes.  String payloads remain owned by [`TypeBuiltinState`] for persistence; they are not
/// represented as fake zeroes in this projection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StringWalkProof {
    ChecksumGateAt006631b3,
}

/// Proof that each shipped slot uses its fixed most-derived virtual walker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VirtualWalkProof {
    ShippedSlotBandsAt00669800,
}

/// Proof that only the named instruction-derived ranges and two ObjectType arrays are visited.
/// Runtime vptrs, backing pointers, padding, and derived caches are absent from the normalized
/// source rather than accepted as unknowable bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CacheWalkProof {
    NamedRangesOnly,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TypeWalkBoundaryProof {
    pub strings: Option<StringWalkProof>,
    pub virtuals: Option<VirtualWalkProof>,
    pub caches: Option<CacheWalkProof>,
}

/// Exact checksum-visible state of one `SimpleArray<unsigned short>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct U16ArraySource {
    pub capacity: i32,
    pub grow: u16,
    pub flags: u8,
    pub elements: Vec<u16>,
}

/// The concrete-tail ranges, in retail call order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TypeTailSource {
    None,
    Good(Vec<u8>),
    Unit {
        at_692: Vec<u8>,
        at_724: Vec<u8>,
        at_732: Vec<u8>,
        at_736: Vec<u8>,
    },
    Build(Vec<u8>),
    Tech(Vec<u8>),
    Spell(Vec<u8>),
}

/// Exact pristine String values retained for save/clean-state admission.  They are deliberately
/// separate from checksum bytes: retail gates their `walk_data` calls, but a clean owner whose
/// display String differs from this source is still an unreceipted persistence mutation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeStringSource {
    pub name: String,
    pub display_name: String,
    pub type_name: String,
}

/// One normalized pristine row.  Every vector is a visited byte range, not a raw object dump.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeWalkSourceRow {
    pub slot: usize,
    pub kind: Option<TypeRuleKind>,
    /// `Type::walk_rules_data`: `[this+4, this+94)`.
    pub type_base: Option<Vec<u8>>,
    /// `ObjectType::walk_rules_data`: `[this+484, this+636)`.
    pub object: Option<Vec<u8>>,
    pub object_arrays: Option<[U16ArraySource; 2]>,
    pub tail: Option<TypeTailSource>,
    pub strings: Option<TypeStringSource>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TypeWalkSourceMode {
    /// Exact unmodified retail source.  The live `after Types` checkpoint and walked width are
    /// mandatory gates.
    ShippedRetail,
    /// An exact synchronized mod composition with an independently retained pristine checkpoint.
    ExactComposition { pristine_after_types: u32 },
}

/// Complete pristine row projection paired with the same factory provenance as the mutable
/// owner.  `type_rows_sha256` binds the scalar/String/relation source before BHS mutation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeWalkSource {
    pub provenance: TypeBuiltinProvenance,
    pub mode: TypeWalkSourceMode,
    pub boundary: TypeWalkBoundaryProof,
    pub rows: Vec<TypeWalkSourceRow>,
}

/// Session-owned receipt.  `Option` is intentional: an adapter that cannot distinguish clean
/// from dirty, or cannot bind the exact mutation revision, is rejected instead of guessed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InstalledTypeOwnerReceipt {
    pub provenance: TypeBuiltinProvenance,
    pub dirty: Option<bool>,
    pub mutation_revision: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TypeChannel13Error {
    /// The canonical owner was installed without its immutable normalized walk source.
    SourceUnowned,
    UnknownProvenance,
    ProvenanceMismatch,
    UnknownDirtyState,
    UnknownMutationRevision,
    OwnerReceiptMismatch {
        state_dirty: bool,
        state_revision: u64,
        receipt_dirty: bool,
        receipt_revision: u64,
    },
    DirtyRevisionAmbiguous {
        dirty: bool,
        revision: u64,
    },
    UnknownStringWalk,
    UnknownVirtualWalk,
    UnknownCacheWalk,
    WrongRowCount {
        actual: usize,
    },
    SlotOrder {
        position: usize,
        slot: usize,
    },
    UnknownKind {
        slot: usize,
    },
    KindOrder {
        slot: usize,
        expected: TypeRuleKind,
        actual: TypeRuleKind,
    },
    MissingRange {
        slot: usize,
        range: &'static str,
    },
    RangeWidth {
        slot: usize,
        range: &'static str,
        expected: usize,
        actual: usize,
    },
    UnexpectedObjectProjection {
        slot: usize,
    },
    TailKind {
        slot: usize,
        kind: TypeRuleKind,
    },
    UnknownStrings {
        slot: usize,
    },
    ImmutableStringMismatch {
        slot: usize,
    },
    CleanDisplayStringDiverged {
        slot: usize,
    },
    ArrayElementCount {
        slot: usize,
        array: usize,
        actual: usize,
    },
    ArrayCapacity {
        slot: usize,
        array: usize,
        count: usize,
        capacity: i32,
    },
    RelationProjectionMismatch {
        slot: usize,
    },
    SourceCheckpoint {
        expected: u32,
        actual: u32,
    },
    ShippedArrayCardinality {
        expected: usize,
        actual: usize,
    },
    ShippedWalkedWidth {
        expected: u64,
        actual: u64,
    },
    CleanOwnerDiverged {
        pristine: u32,
        projected: u32,
    },
}

impl fmt::Display for TypeChannel13Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for TypeChannel13Error {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectedTypeRow {
    slot: usize,
    kind: TypeRuleKind,
    type_base: Vec<u8>,
    object: Option<Vec<u8>>,
    object_arrays: Option<[U16ArraySource; 2]>,
    tail: TypeTailSource,
    pristine_strings: TypeStringSource,
}

impl ProjectedTypeRow {
    pub fn slot(&self) -> usize {
        self.slot
    }

    pub fn kind(&self) -> TypeRuleKind {
        self.kind
    }

    /// Inspect an absolute retail object offset that belongs to a walked scalar range.
    /// Excluded String/pointer/cache offsets return `None`.
    pub fn walked_byte(&self, offset: usize) -> Option<u8> {
        if (TYPE_BASE_START..TYPE_BASE_START + TYPE_BASE_LEN).contains(&offset) {
            return Some(self.type_base[offset - TYPE_BASE_START]);
        }
        if (OBJECT_START..OBJECT_START + OBJECT_LEN).contains(&offset) {
            return self
                .object
                .as_ref()
                .map(|bytes| bytes[offset - OBJECT_START]);
        }
        tail_byte(&self.tail, offset)
    }

    pub fn object_arrays(&self) -> Option<&[U16ArraySource; 2]> {
        self.object_arrays.as_ref()
    }
}

/// The complete projected Type prefix of channel 13 plus the installed owner it was derived
/// from.  Keeping the borrow here prevents a caller from retaining only the digest and losing
/// the live String/Leader-mask state required by a future save section.
#[derive(Debug)]
pub struct ProjectedTypeRules<'a> {
    owner: &'a TypeBuiltinState,
    provenance: TypeBuiltinProvenance,
    dirty: bool,
    mutation_revision: u64,
    pristine_after_types: u32,
    after_types: u32,
    bytes_walked: u64,
    rows: Vec<ProjectedTypeRow>,
}

impl<'a> ProjectedTypeRules<'a> {
    pub fn rows(&self) -> &[ProjectedTypeRow] {
        &self.rows
    }

    pub fn after_types(&self) -> u32 {
        self.after_types
    }

    pub fn pristine_after_types(&self) -> u32 {
        self.pristine_after_types
    }

    pub fn bytes_walked(&self) -> u64 {
        self.bytes_walked
    }

    /// Minimal persistence contract: exact provenance, dirty/revision identity, and the entire
    /// canonical owner remain inseparable.  This does not claim a DoNSave wire encoding.
    pub fn persistence_owner(&self) -> TypePersistenceOwner<'a> {
        TypePersistenceOwner {
            state: self.owner,
            provenance: self.provenance,
            dirty: self.dirty,
            mutation_revision: self.mutation_revision,
            projected_after_types: self.after_types,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct TypePersistenceOwner<'a> {
    state: &'a TypeBuiltinState,
    provenance: TypeBuiltinProvenance,
    dirty: bool,
    mutation_revision: u64,
    projected_after_types: u32,
}

impl<'a> TypePersistenceOwner<'a> {
    pub fn state(&self) -> &'a TypeBuiltinState {
        self.state
    }

    pub fn provenance(&self) -> TypeBuiltinProvenance {
        self.provenance
    }

    pub fn dirty(&self) -> bool {
        self.dirty
    }

    pub fn mutation_revision(&self) -> u64 {
        self.mutation_revision
    }

    pub fn projected_after_types(&self) -> u32 {
        self.projected_after_types
    }
}

/// Compute the candidate pristine checkpoint for a normalized capture.  Projection still
/// requires the caller to retain that value independently in [`TypeWalkSourceMode`].
pub fn candidate_source_checkpoint(
    source: &TypeWalkSource,
) -> Result<(u32, u64, usize), TypeChannel13Error> {
    validate_boundary(source.boundary)?;
    let rows = normalize_source_rows(&source.rows)?;
    checksum_rows(&rows)
}

pub fn project_type_owner<'a>(
    owner: &'a TypeBuiltinState,
    receipt: InstalledTypeOwnerReceipt,
    source: &TypeWalkSource,
) -> Result<ProjectedTypeRules<'a>, TypeChannel13Error> {
    validate_provenance(source.provenance)?;
    validate_provenance(receipt.provenance)?;
    if source.provenance != receipt.provenance {
        return Err(TypeChannel13Error::ProvenanceMismatch);
    }
    let dirty = receipt.dirty.ok_or(TypeChannel13Error::UnknownDirtyState)?;
    let mutation_revision = receipt
        .mutation_revision
        .ok_or(TypeChannel13Error::UnknownMutationRevision)?;
    if dirty != (mutation_revision != 0) {
        return Err(TypeChannel13Error::DirtyRevisionAmbiguous {
            dirty,
            revision: mutation_revision,
        });
    }
    if dirty != owner.is_dirty() || mutation_revision != owner.mutation_revision() {
        return Err(TypeChannel13Error::OwnerReceiptMismatch {
            state_dirty: owner.is_dirty(),
            state_revision: owner.mutation_revision(),
            receipt_dirty: dirty,
            receipt_revision: mutation_revision,
        });
    }

    validate_boundary(source.boundary)?;
    let mut rows = normalize_source_rows(&source.rows)?;
    let (source_checkpoint, source_bytes, array_elements) = checksum_rows(&rows)?;
    if matches!(source.mode, TypeWalkSourceMode::ShippedRetail) {
        if array_elements != SHIPPED_OBJECT_ARRAY_ELEMENTS {
            return Err(TypeChannel13Error::ShippedArrayCardinality {
                expected: SHIPPED_OBJECT_ARRAY_ELEMENTS,
                actual: array_elements,
            });
        }
        if source_bytes != RETAIL_TYPE_WALKED_BYTES {
            return Err(TypeChannel13Error::ShippedWalkedWidth {
                expected: RETAIL_TYPE_WALKED_BYTES,
                actual: source_bytes,
            });
        }
    }
    let pristine_after_types = match source.mode {
        TypeWalkSourceMode::ShippedRetail => RETAIL_AFTER_TYPES,
        TypeWalkSourceMode::ExactComposition {
            pristine_after_types,
        } => pristine_after_types,
    };
    if source_checkpoint != pristine_after_types {
        return Err(TypeChannel13Error::SourceCheckpoint {
            expected: pristine_after_types,
            actual: source_checkpoint,
        });
    }

    for (slot, (projected, row)) in rows.iter_mut().zip(owner.types.rows()).enumerate() {
        if projected.pristine_strings.name != row.name
            || projected.pristine_strings.type_name != row.type_name
        {
            return Err(TypeChannel13Error::ImmutableStringMismatch { slot });
        }
        if !dirty && projected.pristine_strings.display_name != row.common.display_name {
            return Err(TypeChannel13Error::CleanDisplayStringDiverged { slot });
        }
        overlay_owned_row(projected, row);
        if projected.kind.uses_object_walk() {
            let arrays = projected
                .object_arrays
                .as_ref()
                .expect("normalization requires ObjectType arrays");
            if arrays[0].elements != row.is_list {
                return Err(TypeChannel13Error::RelationProjectionMismatch { slot });
            }
        }
    }

    let (after_types, bytes_walked, _) = checksum_rows(&rows)?;
    if !dirty && after_types != pristine_after_types {
        return Err(TypeChannel13Error::CleanOwnerDiverged {
            pristine: pristine_after_types,
            projected: after_types,
        });
    }

    Ok(ProjectedTypeRules {
        owner,
        provenance: receipt.provenance,
        dirty,
        mutation_revision,
        pristine_after_types,
        after_types,
        bytes_walked,
        rows,
    })
}

fn validate_provenance(provenance: TypeBuiltinProvenance) -> Result<(), TypeChannel13Error> {
    let digests = [
        provenance.composition.0,
        provenance.manifest_sha256,
        provenance.type_rows_sha256,
        provenance.tribe_roster_sha256,
        provenance.leader_masks_sha256,
    ];
    if digests.iter().any(|digest| digest_is_zero(*digest)) {
        return Err(TypeChannel13Error::UnknownProvenance);
    }
    Ok(())
}

fn digest_is_zero(digest: Sha256Digest) -> bool {
    digest.0.iter().all(|byte| *byte == 0)
}

fn validate_boundary(boundary: TypeWalkBoundaryProof) -> Result<(), TypeChannel13Error> {
    boundary
        .strings
        .ok_or(TypeChannel13Error::UnknownStringWalk)?;
    boundary
        .virtuals
        .ok_or(TypeChannel13Error::UnknownVirtualWalk)?;
    boundary
        .caches
        .ok_or(TypeChannel13Error::UnknownCacheWalk)?;
    Ok(())
}

fn normalize_source_rows(
    source: &[TypeWalkSourceRow],
) -> Result<Vec<ProjectedTypeRow>, TypeChannel13Error> {
    if source.len() != NUM_TYPES {
        return Err(TypeChannel13Error::WrongRowCount {
            actual: source.len(),
        });
    }
    source
        .iter()
        .enumerate()
        .map(|(position, row)| normalize_source_row(position, row))
        .collect()
}

fn normalize_source_row(
    position: usize,
    source: &TypeWalkSourceRow,
) -> Result<ProjectedTypeRow, TypeChannel13Error> {
    if source.slot != position {
        return Err(TypeChannel13Error::SlotOrder {
            position,
            slot: source.slot,
        });
    }
    let kind = source
        .kind
        .ok_or(TypeChannel13Error::UnknownKind { slot: position })?;
    let expected = TypeRuleKind::for_shipped_slot(position).expect("position is below 806");
    if kind != expected {
        return Err(TypeChannel13Error::KindOrder {
            slot: position,
            expected,
            actual: kind,
        });
    }
    let type_base = exact_range(
        position,
        "Type+4..+94",
        source.type_base.as_ref(),
        TYPE_BASE_LEN,
    )?;

    let (object, object_arrays) = if kind.uses_object_walk() {
        let object = exact_range(
            position,
            "ObjectType+484..+636",
            source.object.as_ref(),
            OBJECT_LEN,
        )?;
        let arrays = source
            .object_arrays
            .clone()
            .ok_or(TypeChannel13Error::MissingRange {
                slot: position,
                range: "ObjectType u16 arrays",
            })?;
        for (array, value) in arrays.iter().enumerate() {
            if value.elements.len() > NUM_TYPES {
                return Err(TypeChannel13Error::ArrayElementCount {
                    slot: position,
                    array,
                    actual: value.elements.len(),
                });
            }
            if value.capacity < value.elements.len() as i32 {
                return Err(TypeChannel13Error::ArrayCapacity {
                    slot: position,
                    array,
                    count: value.elements.len(),
                    capacity: value.capacity,
                });
            }
            if value
                .elements
                .iter()
                .any(|element| usize::from(*element) >= NUM_TYPES)
            {
                return Err(TypeChannel13Error::ArrayElementCount {
                    slot: position,
                    array,
                    actual: value.elements.len(),
                });
            }
        }
        (Some(object), Some(arrays))
    } else {
        if source.object.is_some() || source.object_arrays.is_some() {
            return Err(TypeChannel13Error::UnexpectedObjectProjection { slot: position });
        }
        (None, None)
    };

    let tail = source
        .tail
        .clone()
        .ok_or(TypeChannel13Error::MissingRange {
            slot: position,
            range: "concrete tail",
        })?;
    validate_tail(position, kind, &tail)?;
    let pristine_strings = source
        .strings
        .clone()
        .ok_or(TypeChannel13Error::UnknownStrings { slot: position })?;
    Ok(ProjectedTypeRow {
        slot: position,
        kind,
        type_base,
        object,
        object_arrays,
        tail,
        pristine_strings,
    })
}

fn exact_range(
    slot: usize,
    range: &'static str,
    bytes: Option<&Vec<u8>>,
    expected: usize,
) -> Result<Vec<u8>, TypeChannel13Error> {
    let bytes = bytes.ok_or(TypeChannel13Error::MissingRange { slot, range })?;
    if bytes.len() != expected {
        return Err(TypeChannel13Error::RangeWidth {
            slot,
            range,
            expected,
            actual: bytes.len(),
        });
    }
    Ok(bytes.clone())
}

fn validate_tail(
    slot: usize,
    kind: TypeRuleKind,
    tail: &TypeTailSource,
) -> Result<(), TypeChannel13Error> {
    let range = |name: &'static str, bytes: &Vec<u8>, expected| {
        if bytes.len() == expected {
            Ok(())
        } else {
            Err(TypeChannel13Error::RangeWidth {
                slot,
                range: name,
                expected,
                actual: bytes.len(),
            })
        }
    };
    match (kind, tail) {
        (TypeRuleKind::Good, TypeTailSource::Good(bytes)) => range("Good+692..+760", bytes, 68),
        (
            TypeRuleKind::Unit,
            TypeTailSource::Unit {
                at_692,
                at_724,
                at_732,
                at_736,
            },
        ) => {
            range("Unit+692..+716", at_692, 24)?;
            range("Unit+724..+732", at_724, 8)?;
            range("Unit+732..+736", at_732, 4)?;
            range("Unit+736..+1492", at_736, 756)
        }
        (TypeRuleKind::Build, TypeTailSource::Build(bytes)) => range("Build+692..+741", bytes, 49),
        (TypeRuleKind::Tech, TypeTailSource::Tech(bytes)) => range("Tech+456..+483", bytes, 27),
        (TypeRuleKind::Spell, TypeTailSource::Spell(bytes)) => range("Spell+456..+504", bytes, 48),
        (TypeRuleKind::Object | TypeRuleKind::Type, TypeTailSource::None) => Ok(()),
        _ => Err(TypeChannel13Error::TailKind { slot, kind }),
    }
}

fn overlay_owned_row(projected: &mut ProjectedTypeRow, row: &TypeRow) {
    put_i32(projected, 4, row.index);
    put_u32(projected, 8, row.common.job_time);
    put_u32(projected, 16, row.common.tribe_mask);
    for (index, value) in row.common.costs.iter().enumerate() {
        put_i32(projected, 24 + index * 4, *value);
    }
    for (index, value) in row.common.preq.iter().enumerate() {
        put_i32(projected, 48 + index * 4, *value);
    }
    put_i32(projected, 60, row.from);
    put_i32(projected, 64, row.where_type);
    put_i32(projected, 88, row.modified);
    put_bytes(projected, 92, &[row.grid_x as u8, row.grid_y as u8]);

    match &row.body {
        TypeBody::Unit { object, unit } => {
            put_i32(projected, 488, object.attack);
            put_i32(projected, 504, object.min_range);
            put_i32(projected, 508, object.max_range);
            put_i32(projected, 528, object.hits);
            put_i32(projected, 532, object.armor);
            put_i32(projected, 540, object.los);
            put_i32(projected, 544, object.science_los);
            put_i32(projected, 704, unit.moves);
            put_i32(projected, 708, unit.turn_speed);
            put_i32(projected, 748, unit.mana);
            put_i32(projected, 752, unit.control_cost);
        }
        TypeBody::Build { object, build } => {
            put_i32(projected, 488, object.attack);
            put_i32(projected, 504, object.min_range);
            put_i32(projected, 508, object.max_range);
            put_i32(projected, 528, object.hits);
            put_i32(projected, 532, object.armor);
            put_i32(projected, 540, object.los);
            put_i32(projected, 544, object.science_los);
            put_i32(projected, 692, build.town_hits);
            put_i32(projected, 708, build.most_shots);
            put_i32(projected, 712, build.garrison_max);
            put_i32(projected, 716, build.base_arrows);
            put_i32(projected, 720, build.wonder_val);
            put_i32(projected, 724, build.plunder_value);
            put_i32(projected, 728, build.plunder_good);
        }
        TypeBody::Other => {}
    }
}

fn put_u32(row: &mut ProjectedTypeRow, offset: usize, value: u32) {
    put_bytes(row, offset, &value.to_le_bytes());
}

fn put_i32(row: &mut ProjectedTypeRow, offset: usize, value: i32) {
    put_bytes(row, offset, &value.to_le_bytes());
}

fn put_bytes(row: &mut ProjectedTypeRow, offset: usize, bytes: &[u8]) {
    if (TYPE_BASE_START..TYPE_BASE_START + TYPE_BASE_LEN).contains(&offset) {
        let at = offset - TYPE_BASE_START;
        row.type_base[at..at + bytes.len()].copy_from_slice(bytes);
        return;
    }
    if (OBJECT_START..OBJECT_START + OBJECT_LEN).contains(&offset) {
        let at = offset - OBJECT_START;
        row.object
            .as_mut()
            .expect("owned Unit/Build rows always have ObjectType projection")
            [at..at + bytes.len()]
            .copy_from_slice(bytes);
        return;
    }
    put_tail_bytes(&mut row.tail, offset, bytes);
}

fn put_tail_bytes(tail: &mut TypeTailSource, offset: usize, bytes: &[u8]) {
    let target = match tail {
        TypeTailSource::Good(target) if (692..760).contains(&offset) => (target, offset - 692),
        TypeTailSource::Unit { at_692, .. } if (692..716).contains(&offset) => {
            (at_692, offset - 692)
        }
        TypeTailSource::Unit { at_724, .. } if (724..732).contains(&offset) => {
            (at_724, offset - 724)
        }
        TypeTailSource::Unit { at_732, .. } if (732..736).contains(&offset) => {
            (at_732, offset - 732)
        }
        TypeTailSource::Unit { at_736, .. } if (736..1_492).contains(&offset) => {
            (at_736, offset - 736)
        }
        TypeTailSource::Build(target) if (692..741).contains(&offset) => (target, offset - 692),
        TypeTailSource::Tech(target) if (456..483).contains(&offset) => (target, offset - 456),
        TypeTailSource::Spell(target) if (456..504).contains(&offset) => (target, offset - 456),
        _ => panic!("owner field offset {offset} is outside the row's retail walk"),
    };
    target.0[target.1..target.1 + bytes.len()].copy_from_slice(bytes);
}

fn tail_byte(tail: &TypeTailSource, offset: usize) -> Option<u8> {
    match tail {
        TypeTailSource::Good(bytes) => (692..760).contains(&offset).then(|| bytes[offset - 692]),
        TypeTailSource::Unit {
            at_692,
            at_724,
            at_732,
            at_736,
        } => {
            if (692..716).contains(&offset) {
                Some(at_692[offset - 692])
            } else if (724..732).contains(&offset) {
                Some(at_724[offset - 724])
            } else if (732..736).contains(&offset) {
                Some(at_732[offset - 732])
            } else if (736..1_492).contains(&offset) {
                Some(at_736[offset - 736])
            } else {
                None
            }
        }
        TypeTailSource::Build(bytes) => (692..741).contains(&offset).then(|| bytes[offset - 692]),
        TypeTailSource::Tech(bytes) => (456..483).contains(&offset).then(|| bytes[offset - 456]),
        TypeTailSource::Spell(bytes) => (456..504).contains(&offset).then(|| bytes[offset - 456]),
        TypeTailSource::None => None,
    }
}

fn checksum_rows(rows: &[ProjectedTypeRow]) -> Result<(u32, u64, usize), TypeChannel13Error> {
    let mut adler = Adler32::new();
    let mut array_elements = 0;
    for row in rows {
        adler.update(&row.type_base);
        if row.kind.uses_object_walk() {
            adler.update(row.object.as_ref().expect("normalized ObjectType range"));
            for (array, value) in row
                .object_arrays
                .as_ref()
                .expect("normalized ObjectType arrays")
                .iter()
                .enumerate()
            {
                array_elements += value.elements.len();
                walk_array(row.slot, array, value, &mut adler)?;
            }
        }
        match &row.tail {
            TypeTailSource::None => {}
            TypeTailSource::Good(bytes)
            | TypeTailSource::Build(bytes)
            | TypeTailSource::Tech(bytes)
            | TypeTailSource::Spell(bytes) => adler.update(bytes),
            TypeTailSource::Unit {
                at_692,
                at_724,
                at_732,
                at_736,
            } => {
                adler.update(at_692);
                adler.update(at_724);
                adler.update(at_732);
                adler.update(at_736);
            }
        }
    }
    Ok((adler.value(), adler.bytes, array_elements))
}

fn walk_array(
    slot: usize,
    array: usize,
    value: &U16ArraySource,
    adler: &mut Adler32,
) -> Result<(), TypeChannel13Error> {
    let count =
        i32::try_from(value.elements.len()).map_err(|_| TypeChannel13Error::ArrayElementCount {
            slot,
            array,
            actual: value.elements.len(),
        })?;
    adler.update(&count.to_le_bytes());
    if count != 0 {
        adler.update(&value.capacity.to_le_bytes());
        adler.update(&value.grow.to_le_bytes());
        adler.update(&[value.flags & 0xbf]);
        for element in &value.elements {
            adler.update(&element.to_le_bytes());
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug)]
struct Adler32 {
    s1: u32,
    s2: u32,
    bytes: u64,
}

impl Adler32 {
    fn new() -> Self {
        Self {
            s1: 1,
            s2: 0,
            bytes: 0,
        }
    }

    fn update(&mut self, bytes: &[u8]) {
        const MOD: u32 = 65_521;
        for byte in bytes {
            self.s1 = (self.s1 + u32::from(*byte)) % MOD;
            self.s2 = (self.s2 + self.s1) % MOD;
        }
        self.bytes += bytes.len() as u64;
    }

    fn value(self) -> u32 {
        (self.s2 << 16) | self.s1
    }
}
