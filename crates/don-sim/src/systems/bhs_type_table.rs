//! Canonical mutable type ownership for the retail BHS type builtins.
//!
//! This module is intentionally not exported yet.  It freezes the state and mutation
//! contracts for ScenarioFuncSet registrations 284, 286, 288..=291, and 815..=819 without
//! creating a second availability facade.  Integration must make [`TypeBuiltinState`] the one
//! owner seen by rules, checksum channel 13, save/load, and the script runtime.

#![allow(dead_code)]

use std::fmt;

pub const NUM_TYPES: usize = 806;
pub const NUM_TRIBES: usize = 24;
pub const NUM_LEADERS: usize = 8;
pub const BITMASK_BYTES: usize = (NUM_TYPES + 7) / 8;

pub const REGULAR_UNIT_BEGIN: usize = 50;
pub const REGULAR_UNIT_END: usize = 402;
pub const UNIT_END: usize = 414;
pub const BUILD_BEGIN: usize = 414;
pub const BUILD_END: usize = 543;
pub const SPELL_BEGIN: usize = 629;
pub const SPELL_END: usize = 684;

const DISABLED_PREQ: i32 = -2;
const RETAIL_TIME_SCALE: i32 = 100;

/// The concrete retail class behind a slot in `Types[806]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TypeDomain {
    Other,
    Unit,
    Build,
}

impl TypeDomain {
    fn for_index(index: usize) -> Self {
        if (REGULAR_UNIT_BEGIN..UNIT_END).contains(&index) {
            Self::Unit
        } else if (BUILD_BEGIN..BUILD_END).contains(&index) {
            Self::Build
        } else {
            Self::Other
        }
    }

    fn is_script_target(self) -> bool {
        matches!(self, Self::Unit | Self::Build)
    }
}

/// Fields restored by `Type::restore(TypeBak*)` at `0x00668000`.
///
/// `display_name` is `TypeData +0x74`.  Despite the backup member being named `name`, the
/// restore body does not replace the internal lookup name at `TypeData +0x60`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommonRestoreFields {
    pub job_time: u32,
    pub tribe_mask: u32,
    pub display_name: String,
    pub preq: [i32; 3],
    pub costs: [i32; 6],
}

/// Fields restored by `ObjectType::restore(ObjectTypeBak*)` at `0x0065FB30`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ObjectRestoreFields {
    pub attack: i32,
    pub min_range: i32,
    pub max_range: i32,
    pub hits: i32,
    pub armor: i32,
    pub los: i32,
    pub science_los: i32,
}

/// Extra fields restored for a unit by builtin 286 / `UnitType::restore`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UnitRestoreFields {
    pub moves: i32,
    pub turn_speed: i32,
    pub mana: i32,
    pub control_cost: i32,
}

/// Extra fields restored by `BuildType::restore(BuildTypeBak*)` at `0x00631F80`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BuildRestoreFields {
    pub town_hits: i32,
    pub plunder_value: i32,
    pub plunder_good: i32,
    pub garrison_max: i32,
    pub base_arrows: i32,
    pub most_shots: i32,
    pub wonder_val: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TypeBody {
    Other,
    Unit {
        object: ObjectRestoreFields,
        unit: UnitRestoreFields,
    },
    Build {
        object: ObjectRestoreFields,
        build: BuildRestoreFields,
    },
}

impl TypeBody {
    fn domain(&self) -> TypeDomain {
        match self {
            Self::Other => TypeDomain::Other,
            Self::Unit { .. } => TypeDomain::Unit,
            Self::Build { .. } => TypeDomain::Build,
        }
    }
}

/// One live row of the canonical 806-row type table.
///
/// `is_list` is the materialised non-strict `TypeData::is(query, 0)` relation.  The retail
/// loader derives it from source relationships, but the BHS mutation must consume the same
/// canonical relation used by all other type queries; reconstructing it inside a handler would
/// create a second truth.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeRow {
    pub index: i32,
    pub name: String,
    pub type_name: String,
    pub common: CommonRestoreFields,
    pub from: i32,
    pub where_type: i32,
    pub modified: i32,
    pub grid_x: i8,
    pub grid_y: i8,
    pub is_list: Vec<u16>,
    pub body: TypeBody,
}

impl TypeRow {
    /// An inert row with the retail concrete class implied by `index`.
    pub fn empty(index: usize) -> Self {
        let body = match TypeDomain::for_index(index) {
            TypeDomain::Other => TypeBody::Other,
            TypeDomain::Unit => TypeBody::Unit {
                object: ObjectRestoreFields::default(),
                unit: UnitRestoreFields::default(),
            },
            TypeDomain::Build => TypeBody::Build {
                object: ObjectRestoreFields::default(),
                build: BuildRestoreFields::default(),
            },
        };
        Self {
            index: index as i32,
            name: String::new(),
            type_name: String::new(),
            common: CommonRestoreFields::default(),
            from: -1,
            where_type: -1,
            modified: 0,
            grid_x: 0,
            grid_y: 0,
            is_list: vec![index as u16],
            body,
        }
    }

    pub fn domain(&self) -> TypeDomain {
        self.body.domain()
    }

    fn is_non_strict(&self, query: usize) -> bool {
        self.is_list
            .iter()
            .any(|&index| usize::from(index) == query)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BackupBody {
    Other,
    Unit {
        object: ObjectRestoreFields,
        unit: UnitRestoreFields,
    },
    Build {
        object: ObjectRestoreFields,
        build: BuildRestoreFields,
    },
}

impl BackupBody {
    fn domain(&self) -> TypeDomain {
        match self {
            Self::Other => TypeDomain::Other,
            Self::Unit { .. } => TypeDomain::Unit,
            Self::Build { .. } => TypeDomain::Build,
        }
    }
}

/// The immutable restore payload owned separately from the live row.
///
/// Retail has 566 `TypeBak` records.  The builtins in this module require a compatible backup
/// for every ordinary unit (50..402) and building (414..543); the other slots may be absent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeBackup {
    pub common: CommonRestoreFields,
    pub body: BackupBody,
}

impl TypeBackup {
    /// Capture at rules-composition time only.  Calling this lazily after script mutation would
    /// make builtin 286 restore modified state and is therefore an integration error.
    pub fn capture_pristine(row: &TypeRow) -> Self {
        let body = match &row.body {
            TypeBody::Other => BackupBody::Other,
            TypeBody::Unit { object, unit } => BackupBody::Unit {
                object: *object,
                unit: *unit,
            },
            TypeBody::Build { object, build } => BackupBody::Build {
                object: *object,
                build: *build,
            },
        };
        Self {
            common: row.common.clone(),
            body,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TypeTableError {
    WrongRowCount {
        got: usize,
    },
    WrongBackupCount {
        got: usize,
    },
    WrongTribeCount {
        got: usize,
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
    MissingBackup {
        slot: usize,
    },
    BackupDomainMismatch {
        slot: usize,
        expected: TypeDomain,
        got: TypeDomain,
    },
    RelationOutOfRange {
        slot: usize,
        target: usize,
    },
    NonAsciiRetailName,
    /// `TypeData::time(-1)` asks a spell row to read the anomalous retail `leaders[-1]`.
    SpellTimeRequiresLeaderMinusOne {
        slot: usize,
    },
}

impl fmt::Display for TypeTableError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for TypeTableError {}

/// Exact-size live table plus immutable restore owner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeTable {
    rows: Box<[TypeRow]>,
    backups: Box<[Option<TypeBackup>]>,
}

impl TypeTable {
    pub fn new(
        rows: Vec<TypeRow>,
        backups: Vec<Option<TypeBackup>>,
    ) -> Result<Self, TypeTableError> {
        if rows.len() != NUM_TYPES {
            return Err(TypeTableError::WrongRowCount { got: rows.len() });
        }
        if backups.len() != NUM_TYPES {
            return Err(TypeTableError::WrongBackupCount { got: backups.len() });
        }

        for (slot, row) in rows.iter().enumerate() {
            if row.index != slot as i32 {
                return Err(TypeTableError::RowIndexMismatch {
                    slot,
                    row_index: row.index,
                });
            }
            let expected = TypeDomain::for_index(slot);
            let got = row.domain();
            if expected != got {
                return Err(TypeTableError::DomainMismatch {
                    slot,
                    expected,
                    got,
                });
            }
            if !row.name.is_ascii() || !row.type_name.is_ascii() {
                return Err(TypeTableError::NonAsciiRetailName);
            }
            for &target in &row.is_list {
                let target = usize::from(target);
                if target >= NUM_TYPES {
                    return Err(TypeTableError::RelationOutOfRange { slot, target });
                }
            }

            if is_restore_candidate(slot) {
                let backup = backups[slot]
                    .as_ref()
                    .ok_or(TypeTableError::MissingBackup { slot })?;
                if backup.body.domain() != expected {
                    return Err(TypeTableError::BackupDomainMismatch {
                        slot,
                        expected,
                        got: backup.body.domain(),
                    });
                }
            }
        }

        Ok(Self {
            rows: rows.into_boxed_slice(),
            backups: backups.into_boxed_slice(),
        })
    }

    pub fn rows(&self) -> &[TypeRow] {
        &self.rows
    }

    pub fn row(&self, index: usize) -> &TypeRow {
        &self.rows[index]
    }

    pub fn row_mut(&mut self, index: usize) -> &mut TypeRow {
        &mut self.rows[index]
    }

    fn find_first(&self, field: LookupField, query: &str) -> Result<Option<usize>, TypeTableError> {
        validate_query(query)?;
        if query.is_empty() {
            return Ok(None);
        }
        Ok(self.rows.iter().position(|row| {
            let candidate = match field {
                LookupField::Name => &row.name,
                LookupField::TypeName => &row.type_name,
            };
            retail_string_eq(candidate, query)
        }))
    }

    fn related_candidates(&self, selected: usize) -> Vec<usize> {
        let range = match self.rows[selected].domain() {
            TypeDomain::Unit => REGULAR_UNIT_BEGIN..REGULAR_UNIT_END,
            TypeDomain::Build => BUILD_BEGIN..BUILD_END,
            TypeDomain::Other => 0..0,
        };
        range
            .filter(|&candidate| self.rows[candidate].is_non_strict(selected))
            .collect()
    }

    /// Registrations 288, 290, and 291 scan every one of the 806 rows, unlike the narrower
    /// Unit/Build candidate ranges used by registrations 284 and 286.
    fn all_related_candidates(&self, selected: usize) -> Vec<usize> {
        (0..NUM_TYPES)
            .filter(|&candidate| self.rows[candidate].is_non_strict(selected))
            .collect()
    }

    fn restore(&mut self, index: usize) {
        let backup = self.backups[index]
            .as_ref()
            .expect("constructor requires every reachable restore backup")
            .clone();
        let row = &mut self.rows[index];

        // Preserve the retail order: concrete tail, ObjectType, then Type common fields.
        match (&mut row.body, backup.body) {
            (
                TypeBody::Unit { object, unit },
                BackupBody::Unit {
                    object: saved_object,
                    unit: saved_unit,
                },
            ) => {
                *unit = saved_unit;
                *object = saved_object;
            }
            (
                TypeBody::Build { object, build },
                BackupBody::Build {
                    object: saved_object,
                    build: saved_build,
                },
            ) => {
                *build = saved_build;
                *object = saved_object;
            }
            _ => unreachable!("constructor pins live/backup concrete domains"),
        }
        row.common = backup.common;
        // `modified`, internal `name`, `type_name`, `where`, and grid bytes are not restored.
    }
}

fn is_restore_candidate(index: usize) -> bool {
    (REGULAR_UNIT_BEGIN..REGULAR_UNIT_END).contains(&index)
        || (BUILD_BEGIN..BUILD_END).contains(&index)
}

#[derive(Clone, Copy)]
enum LookupField {
    Name,
    TypeName,
}

fn validate_query(query: &str) -> Result<(), TypeTableError> {
    if query.is_ascii() {
        Ok(())
    } else {
        // Retail uses `_wcsicmp`.  ASCII is exact for all shipped rule names; accepting a
        // non-ASCII mod name with Rust Unicode folding would silently change that contract.
        Err(TypeTableError::NonAsciiRetailName)
    }
}

fn retail_string_eq(left: &str, right: &str) -> bool {
    left.len() == right.len() && left.eq_ignore_ascii_case(right)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TribeRoster {
    names: Box<[String]>,
}

impl TribeRoster {
    pub fn new(names: Vec<String>) -> Result<Self, TypeTableError> {
        if names.len() != NUM_TRIBES {
            return Err(TypeTableError::WrongTribeCount { got: names.len() });
        }
        if names.iter().any(|name| !name.is_ascii()) {
            return Err(TypeTableError::NonAsciiRetailName);
        }
        Ok(Self {
            names: names.into_boxed_slice(),
        })
    }

    fn find(&self, query: &str) -> Result<Option<usize>, TypeTableError> {
        validate_query(query)?;
        Ok(self
            .names
            .iter()
            .position(|candidate| retail_string_eq(candidate, query)))
    }
}

/// Retail `BitMask<806>` header and inline 101-byte payload.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetailTypeBitMask {
    pub bits: i32,
    pub size: i32,
    pub flags: i32,
    pub bytes: [u8; BITMASK_BYTES],
}

impl Default for RetailTypeBitMask {
    fn default() -> Self {
        Self {
            bits: NUM_TYPES as i32,
            size: BITMASK_BYTES as i32,
            flags: 0,
            bytes: [0; BITMASK_BYTES],
        }
    }
}

impl RetailTypeBitMask {
    pub fn get(&self, index: usize) -> bool {
        index < NUM_TYPES && self.bytes[index >> 3] & (1 << (index & 7)) != 0
    }

    pub fn set(&mut self, index: usize, value: bool) {
        assert!(index < NUM_TYPES);
        let bit = 1 << (index & 7);
        if value {
            self.bytes[index >> 3] |= bit;
        } else {
            self.bytes[index >> 3] &= !bit;
        }
    }

    fn clear_and_mark_dirty(&mut self, index: usize) {
        self.set(index, false);
        if self.flags == 0 {
            self.flags = 2;
        }
    }
}

/// The exact Leader fields read or written by builtins 815..=819.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LeaderTypeMasks {
    pub leader_flags: i32,
    pub tribe: i32,
    pub tech: RetailTypeBitMask,
    pub obs_flags: RetailTypeBitMask,
}

impl LeaderTypeMasks {
    fn is_active_for(&self, tribe: usize) -> bool {
        self.leader_flags & 1 != 0 && self.tribe == tribe as i32
    }
}

/// Canonical owner consumed by the retail type-query and mutation registrations frozen here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeBuiltinState {
    pub types: TypeTable,
    pub tribes: TribeRoster,
    pub leaders: [LeaderTypeMasks; NUM_LEADERS],
    dirty: bool,
}

impl TypeBuiltinState {
    pub fn new(
        types: TypeTable,
        tribes: TribeRoster,
        leaders: [LeaderTypeMasks; NUM_LEADERS],
    ) -> Self {
        Self {
            types,
            tribes,
            leaders,
            dirty: false,
        }
    }

    /// True after any successful retail mutation.  Save admission must use this to fail closed
    /// until DoNSave owns the mutable rows and Leader mask effects.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Builtin 288, `ScenarioFuncSet::rename_type`, `0x009EA6C0`.
    ///
    /// The first argument resolves through the unchanged internal `name`.  Retail then scans
    /// all 806 rows in ascending order and replaces `display_name` on every non-strict related
    /// row.  The replacement is deliberately not passed through `validate_query`: empty and
    /// non-ASCII display strings are valid even though lookup strings currently fail closed to
    /// the shipped ASCII name domain.
    pub fn rename_type(&mut self, name: &str, replacement: &str) -> Result<i32, TypeTableError> {
        let Some(selected) = self.types.find_first(LookupField::Name, name)? else {
            return Ok(-1);
        };
        let targets = self.types.all_related_candidates(selected);
        for target in targets {
            self.types.row_mut(target).common.display_name = replacement.to_owned();
        }
        // Retail leaves TypeData::modified untouched.  The owner-level bit records that live
        // display state can no longer be reconstructed from pristine rules during save/load.
        self.dirty = true;
        Ok(1)
    }

    /// Builtin 289, `ScenarioFuncSet::type_build_time`, `0x009EA760`.
    ///
    /// Non-spell `TypeData::time(-1)` is exactly wrapping `job_time * 100`.  Spell rows take a
    /// different retail branch through `LeaderData::has_preq` using `leaders[-1]`; this owner
    /// rejects that anomalous dependency explicitly rather than guessing between `job_time`
    /// and the currently unowned `res_time`.
    pub fn type_build_time(&self, name: &str) -> Result<i32, TypeTableError> {
        let Some(selected) = self.types.find_first(LookupField::Name, name)? else {
            return Ok(-1);
        };
        if (SPELL_BEGIN..SPELL_END).contains(&selected) {
            return Err(TypeTableError::SpellTimeRequiresLeaderMinusOne { slot: selected });
        }
        Ok(self
            .types
            .row(selected)
            .common
            .job_time
            .wrapping_mul(RETAIL_TIME_SCALE as u32) as i32)
    }

    /// Builtin 290, `ScenarioFuncSet::set_type_build_time`, `0x009EA7D0`.
    ///
    /// Retail performs signed division truncating toward zero, stores the quotient into the
    /// unsigned `job_time`, and substitutes one only when that stored value equals zero.
    /// Negative nonzero quotients therefore remain their wrapped `u32` bit patterns.
    pub fn set_type_build_time(&mut self, name: &str, seconds: i32) -> Result<i32, TypeTableError> {
        let Some(selected) = self.types.find_first(LookupField::Name, name)? else {
            return Ok(-1);
        };
        let quotient = seconds / RETAIL_TIME_SCALE;
        let job_time = if quotient == 0 { 1 } else { quotient as u32 };
        let targets = self.types.all_related_candidates(selected);
        for target in targets {
            self.types.row_mut(target).common.job_time = job_time;
        }
        // The handler does not set TypeData::modified, but job_time is live channel-13 state.
        self.dirty = true;
        Ok(seconds)
    }

    /// Builtin 291, `ScenarioFuncSet::set_type_job_time`, `0x009EA880`.
    ///
    /// This is not an alias for registration 290: every signed input below 200, including all
    /// negative values, becomes one.  Only values at least 200 are divided by 100.
    pub fn set_type_job_time(&mut self, name: &str, seconds: i32) -> Result<i32, TypeTableError> {
        let Some(selected) = self.types.find_first(LookupField::Name, name)? else {
            return Ok(-1);
        };
        let job_time = if seconds < 200 {
            1
        } else {
            (seconds / RETAIL_TIME_SCALE) as u32
        };
        let targets = self.types.all_related_candidates(selected);
        for target in targets {
            self.types.row_mut(target).common.job_time = job_time;
        }
        self.dirty = true;
        Ok(seconds)
    }

    /// Builtin 284, `ScenarioFuncSet::disable_type`, `0x009EA130`.
    pub fn disable_type(&mut self, name: &str) -> Result<i32, TypeTableError> {
        let Some(selected) = self.types.find_first(LookupField::Name, name)? else {
            return Ok(-1);
        };
        if !self.types.row(selected).domain().is_script_target() {
            return Ok(-1);
        }
        let targets = self.types.related_candidates(selected);
        for target in targets {
            let row = self.types.row_mut(target);
            row.common.preq[0] = DISABLED_PREQ;
            row.common.tribe_mask = 0;
            row.modified = 1;
        }
        self.dirty = true;
        Ok(selected as i32)
    }

    /// Builtin 286, `ScenarioFuncSet::enable_type`, `0x009EA3E0`.
    pub fn enable_type(&mut self, name: &str) -> Result<i32, TypeTableError> {
        let Some(selected) = self.types.find_first(LookupField::Name, name)? else {
            return Ok(-1);
        };
        if !self.types.row(selected).domain().is_script_target() {
            return Ok(-1);
        }
        let targets = self.types.related_candidates(selected);
        for target in targets {
            self.types.restore(target);
        }
        self.dirty = true;
        Ok(selected as i32)
    }

    /// Builtin 815, `disable_type_by_tribe`, `0x00A006A0`.
    pub fn disable_type_by_tribe(
        &mut self,
        name: &str,
        tribe_name: &str,
    ) -> Result<i32, TypeTableError> {
        self.set_type_for_tribe(LookupField::Name, name, tribe_name, false)
    }

    /// Builtin 816, `enable_type_by_tribe`, `0x00A009A0`.
    pub fn enable_type_by_tribe(
        &mut self,
        name: &str,
        tribe_name: &str,
    ) -> Result<i32, TypeTableError> {
        self.set_type_for_tribe(LookupField::Name, name, tribe_name, true)
    }

    /// Builtin 818, `enable_type_by_tribe_with_type_name`, `0x00A00D90`.
    pub fn enable_type_by_tribe_with_type_name(
        &mut self,
        type_name: &str,
        tribe_name: &str,
    ) -> Result<i32, TypeTableError> {
        self.set_type_for_tribe(LookupField::TypeName, type_name, tribe_name, true)
    }

    /// Builtin 817, the five-argument `enable_type_by_tribe`, `0x00A00CA0`.
    pub fn enable_type_by_tribe_at(
        &mut self,
        name: &str,
        tribe_name: &str,
        building_name: &str,
        row: i32,
        column: i32,
    ) -> Result<i32, TypeTableError> {
        self.enable_type_by_tribe_at_impl(
            LookupField::Name,
            name,
            tribe_name,
            building_name,
            row,
            column,
        )
    }

    /// Builtin 819, the five-argument `enable_type_by_tribe_with_type_name`, `0x00A01090`.
    pub fn enable_type_by_tribe_with_type_name_at(
        &mut self,
        type_name: &str,
        tribe_name: &str,
        building_name: &str,
        row: i32,
        column: i32,
    ) -> Result<i32, TypeTableError> {
        self.enable_type_by_tribe_at_impl(
            LookupField::TypeName,
            type_name,
            tribe_name,
            building_name,
            row,
            column,
        )
    }

    fn set_type_for_tribe(
        &mut self,
        lookup: LookupField,
        type_query: &str,
        tribe_query: &str,
        enable: bool,
    ) -> Result<i32, TypeTableError> {
        let Some(selected) = self.types.find_first(lookup, type_query)? else {
            return Ok(-1);
        };
        let Some(tribe) = self.tribes.find(tribe_query)? else {
            return Ok(-1);
        };
        if !self.types.row(selected).domain().is_script_target() {
            return Ok(-1);
        }

        let bit = 1u32 << tribe;
        let type_row = self.types.row_mut(selected);
        if enable {
            type_row.common.tribe_mask |= bit;
        } else {
            type_row.common.tribe_mask &= !bit;
        }
        type_row.modified = 1;

        for leader in &mut self.leaders {
            if leader.is_active_for(tribe) {
                if enable {
                    leader.obs_flags.clear_and_mark_dirty(selected);
                } else {
                    leader.tech.clear_and_mark_dirty(selected);
                }
            }
        }
        self.dirty = true;
        Ok(selected as i32)
    }

    fn enable_type_by_tribe_at_impl(
        &mut self,
        lookup: LookupField,
        type_query: &str,
        tribe_query: &str,
        building_query: &str,
        row: i32,
        column: i32,
    ) -> Result<i32, TypeTableError> {
        let selected = self.set_type_for_tribe(lookup, type_query, tribe_query, true)?;
        if selected < 0 {
            return Ok(-1);
        }
        let selected = selected as usize;
        if self.types.row(selected).domain() == TypeDomain::Build {
            // Retail does not inspect building/row/column at all on this path.
            return Ok(1);
        }

        let Some(building) = self.types.find_first(LookupField::Name, building_query)? else {
            return Ok(-1);
        };
        if self.types.row(building).domain() != TypeDomain::Build {
            return Ok(-1);
        }

        let type_row = self.types.row_mut(selected);
        type_row.where_type = building as i32;
        type_row.grid_x = column as i8;
        type_row.grid_y = row as i8;
        Ok(1)
    }
}
