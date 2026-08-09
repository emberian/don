//! Source-only frontier for the retail BHS type-stat mutation cohort.
//!
//! The canonical owner remains [`super::bhs_type_table::TypeBuiltinState`].  This module only
//! derives a stale-state-bindable transaction plan from that owner; it deliberately cannot
//! apply the writes or mark the owner dirty.  The production adapter, checksum channel 13, and
//! full save ownership therefore remain explicit red gates.

#![allow(dead_code)]

use super::bhs_type_table::{
    TypeBody, TypeBuiltinState, TypeDomain, BUILD_BEGIN, BUILD_END, NUM_TYPES, REGULAR_UNIT_BEGIN,
    REGULAR_UNIT_END,
};

pub const TYPE_STAT_CORPUS_CALLS: u32 = 194;

/// Instruction-derived identity of one registered handler in the cohort.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TypeStatBuiltin {
    SetMaxHealth,
    SetArmor,
    SetAttack,
    SetMaxRange,
    SetMinRange,
    SetUnitSpeed,
    SetUnitMaxCraft,
    SetLineOfSight,
}

impl TypeStatBuiltin {
    pub const ALL: [Self; 8] = [
        Self::SetMaxHealth,
        Self::SetArmor,
        Self::SetAttack,
        Self::SetMaxRange,
        Self::SetMinRange,
        Self::SetUnitSpeed,
        Self::SetUnitMaxCraft,
        Self::SetLineOfSight,
    ];

    pub const fn registration(self) -> u16 {
        match self {
            Self::SetMaxHealth => 529,
            Self::SetArmor => 531,
            Self::SetAttack => 532,
            Self::SetMaxRange => 533,
            Self::SetMinRange => 534,
            Self::SetUnitSpeed => 535,
            Self::SetUnitMaxCraft => 538,
            Self::SetLineOfSight => 814,
        }
    }

    pub const fn from_registration(registration: u32) -> Option<Self> {
        match registration {
            529 => Some(Self::SetMaxHealth),
            531 => Some(Self::SetArmor),
            532 => Some(Self::SetAttack),
            533 => Some(Self::SetMaxRange),
            534 => Some(Self::SetMinRange),
            535 => Some(Self::SetUnitSpeed),
            538 => Some(Self::SetUnitMaxCraft),
            814 => Some(Self::SetLineOfSight),
            _ => None,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::SetMaxHealth => "set_object_type_max_health",
            Self::SetArmor => "set_object_type_armor",
            Self::SetAttack => "set_object_type_attack",
            Self::SetMaxRange => "set_object_type_max_range",
            Self::SetMinRange => "set_object_type_min_range",
            Self::SetUnitSpeed => "set_unit_type_speed",
            Self::SetUnitMaxCraft => "set_unit_type_max_craft",
            Self::SetLineOfSight => "set_type_line_of_sight",
        }
    }

    pub const fn retail_va(self) -> u32 {
        match self {
            Self::SetMaxHealth => 0x009f_5da0,
            Self::SetArmor => 0x009f_5fb0,
            Self::SetAttack => 0x009f_6170,
            Self::SetMaxRange => 0x009f_6330,
            Self::SetMinRange => 0x009f_64f0,
            Self::SetUnitSpeed => 0x009f_66b0,
            Self::SetUnitMaxCraft => 0x009f_6980,
            Self::SetLineOfSight => 0x00a0_0550,
        }
    }

    pub const fn retail_bytes(self) -> u16 {
        match self {
            Self::SetMaxHealth
            | Self::SetArmor
            | Self::SetAttack
            | Self::SetMaxRange
            | Self::SetMinRange => 433,
            Self::SetUnitSpeed => 369,
            Self::SetUnitMaxCraft => 291,
            Self::SetLineOfSight => 327,
        }
    }

    /// Every registration in this cohort is exactly `(string, int) -> int`.
    pub const fn arity(self) -> u8 {
        2
    }

    pub const fn field(self) -> TypeStatField {
        match self {
            Self::SetMaxHealth => TypeStatField::Hits,
            Self::SetArmor => TypeStatField::Armor,
            Self::SetAttack => TypeStatField::Attack,
            Self::SetMaxRange => TypeStatField::MaxRange,
            Self::SetMinRange => TypeStatField::MinRange,
            Self::SetUnitSpeed => TypeStatField::Moves,
            Self::SetUnitMaxCraft => TypeStatField::Mana,
            Self::SetLineOfSight => TypeStatField::LineOfSight,
        }
    }

    const fn unit_only(self) -> bool {
        matches!(self, Self::SetUnitSpeed | Self::SetUnitMaxCraft)
    }

    const fn line_of_sight(self) -> bool {
        matches!(self, Self::SetLineOfSight)
    }

    const fn recalc(self) -> LeaderStatRecalc {
        match self {
            Self::SetUnitMaxCraft => LeaderStatRecalc::UnitOnly,
            _ => LeaderStatRecalc::WallThenUnit,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TypeStatCorpusRow {
    pub builtin: TypeStatBuiltin,
    pub calls: u16,
    pub files: u16,
}

/// Comment/string-stripped lexical census over all 363 shipped `.bhs` files.
pub const TYPE_STAT_CORPUS_CENSUS: [TypeStatCorpusRow; 8] = [
    TypeStatCorpusRow {
        builtin: TypeStatBuiltin::SetMaxHealth,
        calls: 124,
        files: 31,
    },
    TypeStatCorpusRow {
        builtin: TypeStatBuiltin::SetArmor,
        calls: 4,
        files: 3,
    },
    TypeStatCorpusRow {
        builtin: TypeStatBuiltin::SetAttack,
        calls: 4,
        files: 3,
    },
    TypeStatCorpusRow {
        builtin: TypeStatBuiltin::SetMaxRange,
        calls: 4,
        files: 3,
    },
    TypeStatCorpusRow {
        builtin: TypeStatBuiltin::SetMinRange,
        calls: 0,
        files: 0,
    },
    TypeStatCorpusRow {
        builtin: TypeStatBuiltin::SetUnitSpeed,
        calls: 5,
        files: 4,
    },
    TypeStatCorpusRow {
        builtin: TypeStatBuiltin::SetUnitMaxCraft,
        calls: 9,
        files: 9,
    },
    TypeStatCorpusRow {
        builtin: TypeStatBuiltin::SetLineOfSight,
        calls: 44,
        files: 6,
    },
];

/// Canonical PDB field written by one handler.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TypeStatField {
    Attack,
    MinRange,
    MaxRange,
    Hits,
    Armor,
    LineOfSight,
    Moves,
    /// PDB `UnitTypeData::mana +0x2EC`; registration 538 calls it maximum craft.
    Mana,
}

impl TypeStatField {
    pub const fn retail_offset(self) -> u16 {
        match self {
            Self::Attack => 0x1e8,
            Self::MinRange => 0x1f8,
            Self::MaxRange => 0x1fc,
            Self::Hits => 0x210,
            Self::Armor => 0x214,
            Self::LineOfSight => 0x21c,
            Self::Moves => 0x2c0,
            Self::Mana => 0x2ec,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeaderStatRecalc {
    /// `Leader::calc_wall_stats` `0x006CF7C0`, then `Leader::calc_unit_stats` `0x006CF970`.
    WallThenUnit,
    /// `Leader::calc_unit_stats` only.
    UnitOnly,
}

/// One canonical row write, bound to the value and modified flag observed while planning.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TypeStatWrite {
    pub row: usize,
    pub field: TypeStatField,
    pub expected_value: i32,
    pub replacement_value: i32,
    pub expected_modified: i32,
    pub replacement_modified: i32,
}

/// One exact Leader cache-recalculation boundary emitted after all type writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LeaderStatRecalcCall {
    pub leader_slot: usize,
    pub kind: LeaderStatRecalc,
}

/// Complete plan for one admitted builtin call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeStatMutationPlan {
    pub builtin: TypeStatBuiltin,
    pub selected: usize,
    pub stored_value: i32,
    pub return_value: i32,
    pub writes: Vec<TypeStatWrite>,
    pub leader_recalcs: Vec<LeaderStatRecalcCall>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TypeStatPlan {
    /// Retail returns `-1` without mutating type rows or recomputing Leader caches.
    Rejected,
    Admitted(TypeStatMutationPlan),
}

impl TypeStatPlan {
    pub const fn return_value(&self) -> i32 {
        match self {
            Self::Rejected => -1,
            Self::Admitted(plan) => plan.return_value,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TypeStatFrontierError {
    /// Shipped names are ASCII.  Retail uses `_wcsicmp`; Unicode mod-name folding is unowned.
    NonAsciiRetailName,
    /// Construction/factory invariants should make this impossible for an admitted candidate.
    OwnerDomainMismatch { row: usize, field: TypeStatField },
    /// A caller retained a plan across another owner write. Every row is checked before any
    /// field is changed, so this error always leaves the canonical owner untouched.
    StaleWrite {
        row: usize,
        field: TypeStatField,
        expected_value: i32,
        observed_value: i32,
        expected_modified: i32,
        observed_modified: i32,
    },
    /// Only plans emitted by [`plan_type_stat_mutation`] may cross the commit boundary.
    MalformedPlan,
}

impl TypeBuiltinState {
    /// Atomically commit one plan against the canonical owner.
    ///
    /// The complete write set is preflighted before the first store. A stale receipt can never
    /// partially update a relation family, and one admitted handler advances the owner revision
    /// exactly once even when its relation has no writable candidate rows (retail still runs the
    /// Leader recalculation tail in that case).
    pub fn apply_type_stat_plan(
        &mut self,
        plan: &TypeStatMutationPlan,
    ) -> Result<(), TypeStatFrontierError> {
        if plan.selected >= NUM_TYPES
            || plan.writes.iter().any(|write| {
                write.field != plan.builtin.field()
                    || write.replacement_value != plan.stored_value
                    || write.replacement_modified != 1
                    || write.row >= NUM_TYPES
            })
        {
            return Err(TypeStatFrontierError::MalformedPlan);
        }

        for write in &plan.writes {
            let row = self.types.row(write.row);
            let observed_value = read_field(row.body.clone(), write.row, write.field)?;
            if observed_value != write.expected_value || row.modified != write.expected_modified {
                return Err(TypeStatFrontierError::StaleWrite {
                    row: write.row,
                    field: write.field,
                    expected_value: write.expected_value,
                    observed_value,
                    expected_modified: write.expected_modified,
                    observed_modified: row.modified,
                });
            }
        }

        for write in &plan.writes {
            let row = self.types.row_mut(write.row);
            write_field(
                &mut row.body,
                write.row,
                write.field,
                write.replacement_value,
            )?;
            row.modified = write.replacement_modified;
        }
        self.mark_mutated();
        Ok(())
    }
}

/// Derive the instruction-equivalent writes without mutating the canonical owner.
pub fn plan_type_stat_mutation(
    state: &TypeBuiltinState,
    builtin: TypeStatBuiltin,
    query: &str,
    raw_value: i32,
) -> Result<TypeStatPlan, TypeStatFrontierError> {
    if !query.is_ascii() {
        return Err(TypeStatFrontierError::NonAsciiRetailName);
    }
    if query.is_empty() {
        return Ok(TypeStatPlan::Rejected);
    }

    let Some(selected) = state
        .types
        .rows()
        .iter()
        .position(|row| retail_string_eq(&row.name, query))
    else {
        return Ok(TypeStatPlan::Rejected);
    };

    let selected_domain = state.types.row(selected).domain();
    let stored_value = if builtin.line_of_sight() {
        raw_value.clamp(0, 64)
    } else {
        if raw_value <= 0 {
            return Ok(TypeStatPlan::Rejected);
        }
        raw_value
    };

    let candidate_range = if builtin.line_of_sight() {
        // Retail asks only `is_unit_type`; every false result takes the Build candidate range.
        if selected_domain == TypeDomain::Unit {
            REGULAR_UNIT_BEGIN..REGULAR_UNIT_END
        } else {
            BUILD_BEGIN..BUILD_END
        }
    } else {
        match selected_domain {
            TypeDomain::Unit => REGULAR_UNIT_BEGIN..REGULAR_UNIT_END,
            TypeDomain::Build if !builtin.unit_only() => BUILD_BEGIN..BUILD_END,
            TypeDomain::Other | TypeDomain::Build => return Ok(TypeStatPlan::Rejected),
        }
    };

    let field = builtin.field();
    let mut writes = Vec::new();
    for row_index in candidate_range {
        let row = state.types.row(row_index);
        if !row
            .is_list
            .iter()
            .any(|&target| usize::from(target) == selected)
        {
            continue;
        }
        let expected_value = read_field(row.body.clone(), row_index, field)?;
        writes.push(TypeStatWrite {
            row: row_index,
            field,
            expected_value,
            replacement_value: stored_value,
            expected_modified: row.modified,
            replacement_modified: 1,
        });
    }

    let leader_recalcs = state
        .leaders
        .iter()
        .enumerate()
        .filter(|(_, leader)| leader.leader_flags & 3 == 3)
        .map(|(leader_slot, _)| LeaderStatRecalcCall {
            leader_slot,
            kind: builtin.recalc(),
        })
        .collect();

    let return_value = if builtin.line_of_sight() {
        1
    } else {
        selected as i32
    };
    debug_assert!(selected < NUM_TYPES);
    Ok(TypeStatPlan::Admitted(TypeStatMutationPlan {
        builtin,
        selected,
        stored_value,
        return_value,
        writes,
        leader_recalcs,
    }))
}

fn read_field(
    body: TypeBody,
    row: usize,
    field: TypeStatField,
) -> Result<i32, TypeStatFrontierError> {
    let value = match (body, field) {
        (TypeBody::Unit { object, .. } | TypeBody::Build { object, .. }, TypeStatField::Attack) => {
            object.attack
        }
        (
            TypeBody::Unit { object, .. } | TypeBody::Build { object, .. },
            TypeStatField::MinRange,
        ) => object.min_range,
        (
            TypeBody::Unit { object, .. } | TypeBody::Build { object, .. },
            TypeStatField::MaxRange,
        ) => object.max_range,
        (TypeBody::Unit { object, .. } | TypeBody::Build { object, .. }, TypeStatField::Hits) => {
            object.hits
        }
        (TypeBody::Unit { object, .. } | TypeBody::Build { object, .. }, TypeStatField::Armor) => {
            object.armor
        }
        (
            TypeBody::Unit { object, .. } | TypeBody::Build { object, .. },
            TypeStatField::LineOfSight,
        ) => object.los,
        (TypeBody::Unit { unit, .. }, TypeStatField::Moves) => unit.moves,
        (TypeBody::Unit { unit, .. }, TypeStatField::Mana) => unit.mana,
        (_, field) => return Err(TypeStatFrontierError::OwnerDomainMismatch { row, field }),
    };
    Ok(value)
}

fn write_field(
    body: &mut TypeBody,
    row: usize,
    field: TypeStatField,
    value: i32,
) -> Result<(), TypeStatFrontierError> {
    match (body, field) {
        (TypeBody::Unit { object, .. } | TypeBody::Build { object, .. }, TypeStatField::Attack) => {
            object.attack = value;
        }
        (
            TypeBody::Unit { object, .. } | TypeBody::Build { object, .. },
            TypeStatField::MinRange,
        ) => object.min_range = value,
        (
            TypeBody::Unit { object, .. } | TypeBody::Build { object, .. },
            TypeStatField::MaxRange,
        ) => object.max_range = value,
        (TypeBody::Unit { object, .. } | TypeBody::Build { object, .. }, TypeStatField::Hits) => {
            object.hits = value;
        }
        (TypeBody::Unit { object, .. } | TypeBody::Build { object, .. }, TypeStatField::Armor) => {
            object.armor = value;
        }
        (
            TypeBody::Unit { object, .. } | TypeBody::Build { object, .. },
            TypeStatField::LineOfSight,
        ) => object.los = value,
        (TypeBody::Unit { unit, .. }, TypeStatField::Moves) => unit.moves = value,
        (TypeBody::Unit { unit, .. }, TypeStatField::Mana) => unit.mana = value,
        (_, field) => return Err(TypeStatFrontierError::OwnerDomainMismatch { row, field }),
    }
    Ok(())
}

fn retail_string_eq(left: &str, right: &str) -> bool {
    left.len() == right.len() && left.eq_ignore_ascii_case(right)
}
