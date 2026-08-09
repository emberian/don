//! Authoritative setup producer for the mutable BHS type owner.
//!
//! The retail rules loader composes base XML, localized names, and ordered mod overlays before
//! scripts run.  This module is the narrow projection boundary after that composition: all three
//! materialized components must carry the same rules-manifest identity, every retail slot must be
//! present, and immutable restore backups are captured exactly once from the admitted pristine
//! rows.  Parsing incomplete XML fragments or synthesizing missing relations is deliberately out
//! of scope.

use std::fmt;

use super::bhs_type_table::{
    LeaderTypeMasks, RetailTypeBitMask, TribeRoster, TypeBody, TypeBuiltinState, TypeDomain,
    TypeRow, TypeTable, TypeTableError, BUILD_BEGIN, BUILD_END, NUM_LEADERS, NUM_TRIBES, NUM_TYPES,
    REGULAR_UNIT_BEGIN, REGULAR_UNIT_END, UNIT_END,
};

/// SHA-256 identity supplied by the synchronized rules composer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sha256Digest(pub [u8; 32]);

impl Sha256Digest {
    fn is_zero(self) -> bool {
        self.0.iter().all(|byte| *byte == 0)
    }
}

/// Identity of one ordered base-rules plus mod-overlay composition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RulesCompositionId(pub Sha256Digest);

/// The three independently materialized inputs required by the owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TypeSourceRole {
    TypeRows,
    TribeRoster,
    LeaderMasks,
}

/// Immutable proof that a component came from one synchronized composition.
///
/// `manifest_sha256` binds the ordered list of shipped source files and mod overlays.
/// `component_sha256` binds the fully materialized component after those overlays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TypeSourceWitness {
    pub role: TypeSourceRole,
    pub composition: RulesCompositionId,
    pub manifest_sha256: Sha256Digest,
    pub component_sha256: Sha256Digest,
}

/// One component plus its immutable source witness.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WitnessedTypeSource<T> {
    pub witness: TypeSourceWitness,
    pub value: T,
}

/// A row after the normal rules/mod composer has resolved scalar fields and names.
///
/// The relation remains optional here so omission is distinguishable from a deliberately empty
/// list and can fail closed.  A valid retail non-strict relation is never empty: it contains the
/// row itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComposedTypeRow {
    pub index: i32,
    pub name: String,
    pub type_name: String,
    pub common: super::bhs_type_table::CommonRestoreFields,
    pub from: i32,
    pub where_type: i32,
    pub modified: i32,
    pub grid_x: i8,
    pub grid_y: i8,
    pub is_non_strict: Option<Vec<u16>>,
    pub body: TypeBody,
}

/// All setup inputs.  `None` is retained per slot until admission so a sparse loader cannot turn
/// a missing row, tribe, or Leader into a zero-filled retail object.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeBuiltinFactoryInput {
    pub types: WitnessedTypeSource<Vec<Option<ComposedTypeRow>>>,
    pub tribes: WitnessedTypeSource<Vec<Option<String>>>,
    pub leaders: WitnessedTypeSource<Vec<Option<LeaderTypeMasks>>>,
}

/// Provenance retained alongside the produced mutable owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TypeBuiltinProvenance {
    pub composition: RulesCompositionId,
    pub manifest_sha256: Sha256Digest,
    pub type_rows_sha256: Sha256Digest,
    pub tribe_roster_sha256: Sha256Digest,
    pub leader_masks_sha256: Sha256Digest,
}

/// One admitted owner and the exact rules/mod identity from which its backups were captured.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProducedTypeBuiltinState {
    state: TypeBuiltinState,
    provenance: TypeBuiltinProvenance,
}

impl ProducedTypeBuiltinState {
    pub fn state(&self) -> &TypeBuiltinState {
        &self.state
    }

    pub fn provenance(&self) -> TypeBuiltinProvenance {
        self.provenance
    }

    /// The integration seam deliberately yields both values.  Persistence ownership must retain
    /// provenance rather than extracting only the mutable state and losing backup identity.
    pub fn into_parts(self) -> (TypeBuiltinState, TypeBuiltinProvenance) {
        (self.state, self.provenance)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequiredName {
    Internal,
    Type,
    Tribe,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FactoryComponent {
    TypeRows,
    Tribes,
    Leaders,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeaderMaskKind {
    Tech,
    ObservationFlags,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TypeBuiltinFactoryError {
    WrongSourceRole {
        expected: TypeSourceRole,
        got: TypeSourceRole,
    },
    EmptyCompositionIdentity,
    EmptyManifestDigest,
    EmptyComponentDigest {
        role: TypeSourceRole,
    },
    MixedComposition {
        role: TypeSourceRole,
    },
    MixedManifest {
        role: TypeSourceRole,
    },
    WrongComponentCount {
        component: FactoryComponent,
        expected: usize,
        got: usize,
    },
    MissingTypeRow {
        slot: usize,
    },
    MissingTribe {
        slot: usize,
    },
    MissingLeader {
        slot: usize,
    },
    MissingName {
        slot: usize,
        field: RequiredName,
    },
    RowIndexMismatch {
        slot: usize,
        row_index: i32,
    },
    DomainMismatch {
        slot: usize,
        expected: TypeDomain,
        got: TypeDomain,
    },
    MissingNonStrictRelation {
        slot: usize,
    },
    RelationMissingSelf {
        slot: usize,
    },
    RelationOutOfRange {
        slot: usize,
        target: usize,
    },
    DuplicateRelationTarget {
        slot: usize,
        target: usize,
    },
    TribeMaskOutOfRange {
        slot: usize,
        mask: u32,
    },
    ActiveLeaderTribeOutOfRange {
        leader: usize,
        tribe: i32,
    },
    LeaderMaskHeaderMismatch {
        leader: usize,
        mask: LeaderMaskKind,
        bits: i32,
        size: i32,
    },
    LeaderMaskPaddingSet {
        leader: usize,
        mask: LeaderMaskKind,
        tail: u8,
    },
    Owner(TypeTableError),
}

impl fmt::Display for TypeBuiltinFactoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for TypeBuiltinFactoryError {}

impl From<TypeTableError> for TypeBuiltinFactoryError {
    fn from(value: TypeTableError) -> Self {
        Self::Owner(value)
    }
}

/// Produce the one mutable BHS type owner from a synchronized, already-composed rules image.
pub fn produce_type_builtin_state(
    input: TypeBuiltinFactoryInput,
) -> Result<ProducedTypeBuiltinState, TypeBuiltinFactoryError> {
    let provenance = validate_witnesses(&input)?;

    let TypeBuiltinFactoryInput {
        types,
        tribes,
        leaders,
    } = input;
    let rows = project_rows(types.value)?;
    let backups = rows
        .iter()
        .enumerate()
        .map(|(slot, row)| {
            is_restore_candidate(slot)
                .then(|| super::bhs_type_table::TypeBackup::capture_pristine(row))
        })
        .collect();
    let types = TypeTable::new(rows, backups)?;
    let tribes = project_tribes(tribes.value)?;
    let leaders = project_leaders(leaders.value)?;

    Ok(ProducedTypeBuiltinState {
        state: TypeBuiltinState::new(types, tribes, leaders),
        provenance,
    })
}

fn validate_witnesses(
    input: &TypeBuiltinFactoryInput,
) -> Result<TypeBuiltinProvenance, TypeBuiltinFactoryError> {
    let witnesses = [
        (TypeSourceRole::TypeRows, input.types.witness),
        (TypeSourceRole::TribeRoster, input.tribes.witness),
        (TypeSourceRole::LeaderMasks, input.leaders.witness),
    ];
    for (expected, witness) in witnesses {
        if witness.role != expected {
            return Err(TypeBuiltinFactoryError::WrongSourceRole {
                expected,
                got: witness.role,
            });
        }
        if witness.composition.0.is_zero() {
            return Err(TypeBuiltinFactoryError::EmptyCompositionIdentity);
        }
        if witness.manifest_sha256.is_zero() {
            return Err(TypeBuiltinFactoryError::EmptyManifestDigest);
        }
        if witness.component_sha256.is_zero() {
            return Err(TypeBuiltinFactoryError::EmptyComponentDigest { role: witness.role });
        }
    }

    let type_witness = input.types.witness;
    for witness in [input.tribes.witness, input.leaders.witness] {
        if witness.composition != type_witness.composition {
            return Err(TypeBuiltinFactoryError::MixedComposition { role: witness.role });
        }
        if witness.manifest_sha256 != type_witness.manifest_sha256 {
            return Err(TypeBuiltinFactoryError::MixedManifest { role: witness.role });
        }
    }

    Ok(TypeBuiltinProvenance {
        composition: type_witness.composition,
        manifest_sha256: type_witness.manifest_sha256,
        type_rows_sha256: type_witness.component_sha256,
        tribe_roster_sha256: input.tribes.witness.component_sha256,
        leader_masks_sha256: input.leaders.witness.component_sha256,
    })
}

fn project_rows(
    rows: Vec<Option<ComposedTypeRow>>,
) -> Result<Vec<TypeRow>, TypeBuiltinFactoryError> {
    require_count(FactoryComponent::TypeRows, NUM_TYPES, rows.len())?;
    let mut projected = Vec::with_capacity(NUM_TYPES);

    for (slot, source) in rows.into_iter().enumerate() {
        let source = source.ok_or(TypeBuiltinFactoryError::MissingTypeRow { slot })?;
        if source.index != slot as i32 {
            return Err(TypeBuiltinFactoryError::RowIndexMismatch {
                slot,
                row_index: source.index,
            });
        }
        require_name(slot, RequiredName::Internal, &source.name)?;
        require_name(slot, RequiredName::Type, &source.type_name)?;

        let expected = expected_domain(slot);
        let got = body_domain(&source.body);
        if got != expected {
            return Err(TypeBuiltinFactoryError::DomainMismatch {
                slot,
                expected,
                got,
            });
        }
        if source.common.tribe_mask & 0xff00_0000 != 0 {
            return Err(TypeBuiltinFactoryError::TribeMaskOutOfRange {
                slot,
                mask: source.common.tribe_mask,
            });
        }

        let relation = source
            .is_non_strict
            .ok_or(TypeBuiltinFactoryError::MissingNonStrictRelation { slot })?;
        validate_relation(slot, &relation)?;
        projected.push(TypeRow {
            index: source.index,
            name: source.name,
            type_name: source.type_name,
            common: source.common,
            from: source.from,
            where_type: source.where_type,
            modified: source.modified,
            grid_x: source.grid_x,
            grid_y: source.grid_y,
            is_list: relation,
            body: source.body,
        });
    }

    Ok(projected)
}

fn project_tribes(tribes: Vec<Option<String>>) -> Result<TribeRoster, TypeBuiltinFactoryError> {
    require_count(FactoryComponent::Tribes, NUM_TRIBES, tribes.len())?;
    let mut projected = Vec::with_capacity(NUM_TRIBES);
    for (slot, tribe) in tribes.into_iter().enumerate() {
        let tribe = tribe.ok_or(TypeBuiltinFactoryError::MissingTribe { slot })?;
        require_name(slot, RequiredName::Tribe, &tribe)?;
        projected.push(tribe);
    }
    Ok(TribeRoster::new(projected)?)
}

fn project_leaders(
    leaders: Vec<Option<LeaderTypeMasks>>,
) -> Result<[LeaderTypeMasks; NUM_LEADERS], TypeBuiltinFactoryError> {
    require_count(FactoryComponent::Leaders, NUM_LEADERS, leaders.len())?;
    let mut projected = Vec::with_capacity(NUM_LEADERS);
    for (slot, leader) in leaders.into_iter().enumerate() {
        let leader = leader.ok_or(TypeBuiltinFactoryError::MissingLeader { slot })?;
        if leader.leader_flags & 1 != 0 && !(0..NUM_TRIBES as i32).contains(&leader.tribe) {
            return Err(TypeBuiltinFactoryError::ActiveLeaderTribeOutOfRange {
                leader: slot,
                tribe: leader.tribe,
            });
        }
        validate_leader_mask(slot, LeaderMaskKind::Tech, &leader.tech)?;
        validate_leader_mask(slot, LeaderMaskKind::ObservationFlags, &leader.obs_flags)?;
        projected.push(leader);
    }
    projected
        .try_into()
        .map_err(
            |values: Vec<LeaderTypeMasks>| TypeBuiltinFactoryError::WrongComponentCount {
                component: FactoryComponent::Leaders,
                expected: NUM_LEADERS,
                got: values.len(),
            },
        )
}

fn require_count(
    component: FactoryComponent,
    expected: usize,
    got: usize,
) -> Result<(), TypeBuiltinFactoryError> {
    if got == expected {
        Ok(())
    } else {
        Err(TypeBuiltinFactoryError::WrongComponentCount {
            component,
            expected,
            got,
        })
    }
}

fn require_name(
    slot: usize,
    field: RequiredName,
    name: &str,
) -> Result<(), TypeBuiltinFactoryError> {
    if name.trim().is_empty() {
        Err(TypeBuiltinFactoryError::MissingName { slot, field })
    } else {
        Ok(())
    }
}

fn validate_relation(slot: usize, relation: &[u16]) -> Result<(), TypeBuiltinFactoryError> {
    let mut seen = [false; NUM_TYPES];
    for target in relation.iter().copied().map(usize::from) {
        if target >= NUM_TYPES {
            return Err(TypeBuiltinFactoryError::RelationOutOfRange { slot, target });
        }
        if seen[target] {
            return Err(TypeBuiltinFactoryError::DuplicateRelationTarget { slot, target });
        }
        seen[target] = true;
    }
    if !seen[slot] {
        return Err(TypeBuiltinFactoryError::RelationMissingSelf { slot });
    }
    Ok(())
}

fn validate_leader_mask(
    leader: usize,
    kind: LeaderMaskKind,
    mask: &RetailTypeBitMask,
) -> Result<(), TypeBuiltinFactoryError> {
    if mask.bits != NUM_TYPES as i32 || mask.size != mask.bytes.len() as i32 {
        return Err(TypeBuiltinFactoryError::LeaderMaskHeaderMismatch {
            leader,
            mask: kind,
            bits: mask.bits,
            size: mask.size,
        });
    }
    let tail = mask.bytes[mask.bytes.len() - 1];
    if tail & 0xc0 != 0 {
        return Err(TypeBuiltinFactoryError::LeaderMaskPaddingSet {
            leader,
            mask: kind,
            tail,
        });
    }
    Ok(())
}

fn expected_domain(slot: usize) -> TypeDomain {
    if (REGULAR_UNIT_BEGIN..UNIT_END).contains(&slot) {
        TypeDomain::Unit
    } else if (BUILD_BEGIN..BUILD_END).contains(&slot) {
        TypeDomain::Build
    } else {
        TypeDomain::Other
    }
}

fn body_domain(body: &TypeBody) -> TypeDomain {
    match body {
        TypeBody::Other => TypeDomain::Other,
        TypeBody::Unit { .. } => TypeDomain::Unit,
        TypeBody::Build { .. } => TypeDomain::Build,
    }
}

fn is_restore_candidate(slot: usize) -> bool {
    (REGULAR_UNIT_BEGIN..REGULAR_UNIT_END).contains(&slot)
        || (BUILD_BEGIN..BUILD_END).contains(&slot)
}
