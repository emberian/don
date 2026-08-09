//! Live, typed dispatch boundary for the retail BHS type builtins.
//!
//! [`super::bhs_type_table::TypeBuiltinState`] is the sole mutable owner.  This adapter does
//! not copy a rule projection into the script VM: it verifies the shipped declaration identity,
//! executes the owner method, and publishes a revision-bound receipt.  Save and checksum
//! admission methods remain explicit and reject the external owner until their complete
//! projections exist.  Global enforcement still requires Sim/session ownership.

use don_bhs::{builtin, BuiltinDecl, ScriptTy, Value};

use super::bhs_type_table::{TypeBuiltinState, TypeTableError};

/// A successful native-handler-shaped call into the canonical type owner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeBuiltinReceipt {
    pub index: u32,
    pub handler_va: u32,
    pub request: TypeBuiltinRequest,
    pub outcome: TypeBuiltinOutcome,
    pub revision_before: u64,
    pub revision_after: u64,
    pub state_dirty: bool,
}

/// Exact scalar request retained independently of the VM stack lifetime.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TypeBuiltinRequest {
    DisableType {
        name: String,
    },
    EnableType {
        name: String,
    },
    RenameType {
        name: String,
        replacement: String,
    },
    TypeBuildTime {
        name: String,
    },
    SetTypeBuildTime {
        name: String,
        seconds: i32,
    },
    SetTypeJobTime {
        name: String,
        seconds: i32,
    },
    TypeByTribe {
        enable: bool,
        lookup: TypeLookupRequest,
        tribe: String,
        placement: Option<TypePlacementRequest>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TypeLookupRequest {
    Name(String),
    TypeName(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypePlacementRequest {
    pub building_name: String,
    pub row: i32,
    pub column: i32,
}

impl TypeBuiltinReceipt {
    /// Whether this call crossed an actual mutation commit point.
    ///
    /// This is deliberately independent of the integer return.  Registrations 290/291 may
    /// successfully return `-1`, and registrations 817/819 may return `-1` after their type and
    /// Leader-mask prefix already committed.
    pub fn mutated(&self) -> bool {
        self.revision_after.wrapping_sub(self.revision_before) == 1
    }
}

/// Native return or an owner fault reached after exact declaration/argument admission.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TypeBuiltinOutcome {
    Returned(i32),
    OwnerFault(TypeTableError),
}

/// A supported registration failed before a receipt could be issued.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TypeBuiltinRuntimeError {
    /// The global index matched, but name/arity/types/handler VA did not match the shipped PE
    /// registration.  This prevents a table from another executable generation being admitted.
    DeclarationMismatch { index: u32 },
    /// Direct callers must obey the same exact scalar shapes the VM checks before `Host::call`.
    BadArguments { index: u32 },
    /// A typed owner boundary, including non-ASCII lookup and spell-time `leaders[-1]`.
    Owner(TypeTableError),
}

/// Why a larger persistence/checksum operation cannot yet consume this owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TypeBuiltinBoundaryError {
    /// DoNSave v6 cannot restore the external owner or prove its synchronized rules/mod source.
    SaveOwnerUnowned { mutation_revision: u64, dirty: bool },
    /// The partial simulation digest has no complete retail `Types::walk_rules_data` projection.
    Channel13ProjectionUnowned,
}

/// One installed, canonical type owner plus its observable dispatch receipts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeBuiltinRuntime {
    state: TypeBuiltinState,
    last_receipt: Option<TypeBuiltinReceipt>,
    last_fault: Option<TypeBuiltinRuntimeError>,
}

impl TypeBuiltinRuntime {
    pub fn new(state: TypeBuiltinState) -> Self {
        Self {
            state,
            last_receipt: None,
            last_fault: None,
        }
    }

    pub fn state(&self) -> &TypeBuiltinState {
        &self.state
    }

    pub fn last_receipt(&self) -> Option<&TypeBuiltinReceipt> {
        self.last_receipt.as_ref()
    }

    pub fn last_fault(&self) -> Option<&TypeBuiltinRuntimeError> {
        self.last_fault.as_ref()
    }

    /// DoNSave v6 has no chunk or rules/mod provenance for this externally installed owner.
    /// Even pristine state is rejected: `load_sim` returns only `Sim` and could not reattach it.
    pub fn admit_save_v6(&self) -> Result<(), TypeBuiltinBoundaryError> {
        Err(TypeBuiltinBoundaryError::SaveOwnerUnowned {
            mutation_revision: self.state.mutation_revision(),
            dirty: self.state.is_dirty(),
        })
    }

    /// No digest may claim this installed owner until channel 13 walks its full live projection.
    pub fn admit_partial_channel_digest(&self) -> Result<(), TypeBuiltinBoundaryError> {
        Err(TypeBuiltinBoundaryError::Channel13ProjectionUnowned)
    }

    /// Dispatch one exact global builtin declaration.  `Ok(None)` means this cohort does not own
    /// the index and the normal Scenario host must be consulted.
    pub fn dispatch(
        &mut self,
        decl: &BuiltinDecl,
        args: &[Value],
    ) -> Result<Option<TypeBuiltinReceipt>, TypeBuiltinRuntimeError> {
        if !owns_registration(decl.index) {
            return Ok(None);
        }
        self.last_receipt = None;
        self.last_fault = None;

        let Some(shipped) = builtin(decl.index) else {
            return self.fail(TypeBuiltinRuntimeError::DeclarationMismatch { index: decl.index });
        };
        if decl != shipped || shipped.ret != ScriptTy::Int {
            return self.fail(TypeBuiltinRuntimeError::DeclarationMismatch { index: decl.index });
        }
        if !arguments_match(args, shipped.params) {
            return self.fail(TypeBuiltinRuntimeError::BadArguments { index: decl.index });
        }

        let revision_before = self.state.mutation_revision();
        let request = typed_request(decl.index, args);
        let result = self.call_owner(decl.index, args);
        let receipt = TypeBuiltinReceipt {
            index: decl.index,
            handler_va: decl.handler_va,
            request,
            outcome: match &result {
                Ok(value) => TypeBuiltinOutcome::Returned(*value),
                Err(fault) => TypeBuiltinOutcome::OwnerFault(fault.clone()),
            },
            revision_before,
            revision_after: self.state.mutation_revision(),
            state_dirty: self.state.is_dirty(),
        };
        self.last_receipt = Some(receipt.clone());
        match result {
            Ok(_) => Ok(Some(receipt)),
            Err(fault) => self.fail(TypeBuiltinRuntimeError::Owner(fault)),
        }
    }

    fn fail<T>(&mut self, fault: TypeBuiltinRuntimeError) -> Result<T, TypeBuiltinRuntimeError> {
        self.last_fault = Some(fault.clone());
        Err(fault)
    }

    fn call_owner(&mut self, index: u32, args: &[Value]) -> Result<i32, TypeTableError> {
        let string = |slot: usize| match &args[slot] {
            Value::Str(value) => value.as_str(),
            _ => unreachable!("arguments_match admitted exact scalar shapes"),
        };
        let integer = |slot: usize| match &args[slot] {
            Value::Int(value) => *value,
            _ => unreachable!("arguments_match admitted exact scalar shapes"),
        };

        match index {
            284 => self.state.disable_type(string(0)),
            286 => self.state.enable_type(string(0)),
            288 => self.state.rename_type(string(0), string(1)),
            289 => self.state.type_build_time(string(0)),
            290 => self.state.set_type_build_time(string(0), integer(1)),
            291 => self.state.set_type_job_time(string(0), integer(1)),
            815 => self.state.disable_type_by_tribe(string(0), string(1)),
            816 => self.state.enable_type_by_tribe(string(0), string(1)),
            817 => self.state.enable_type_by_tribe_at(
                string(0),
                string(1),
                string(2),
                integer(3),
                integer(4),
            ),
            818 => self
                .state
                .enable_type_by_tribe_with_type_name(string(0), string(1)),
            819 => self.state.enable_type_by_tribe_with_type_name_at(
                string(0),
                string(1),
                string(2),
                integer(3),
                integer(4),
            ),
            _ => unreachable!("declaration admitted this exact cohort"),
        }
    }
}

fn owns_registration(index: u32) -> bool {
    matches!(
        index,
        284 | 286 | 288 | 289 | 290 | 291 | 815 | 816 | 817 | 818 | 819
    )
}

fn typed_request(index: u32, args: &[Value]) -> TypeBuiltinRequest {
    let string = |slot: usize| match &args[slot] {
        Value::Str(value) => value.as_str().to_owned(),
        _ => unreachable!("arguments_match admitted exact scalar shapes"),
    };
    let integer = |slot: usize| match &args[slot] {
        Value::Int(value) => *value,
        _ => unreachable!("arguments_match admitted exact scalar shapes"),
    };
    let tribe_request = |enable, type_name_lookup, placement: Option<TypePlacementRequest>| {
        TypeBuiltinRequest::TypeByTribe {
            enable,
            lookup: if type_name_lookup {
                TypeLookupRequest::TypeName(string(0))
            } else {
                TypeLookupRequest::Name(string(0))
            },
            tribe: string(1),
            placement,
        }
    };

    match index {
        284 => TypeBuiltinRequest::DisableType { name: string(0) },
        286 => TypeBuiltinRequest::EnableType { name: string(0) },
        288 => TypeBuiltinRequest::RenameType {
            name: string(0),
            replacement: string(1),
        },
        289 => TypeBuiltinRequest::TypeBuildTime { name: string(0) },
        290 => TypeBuiltinRequest::SetTypeBuildTime {
            name: string(0),
            seconds: integer(1),
        },
        291 => TypeBuiltinRequest::SetTypeJobTime {
            name: string(0),
            seconds: integer(1),
        },
        815 => tribe_request(false, false, None),
        816 => tribe_request(true, false, None),
        817 => tribe_request(
            true,
            false,
            Some(TypePlacementRequest {
                building_name: string(2),
                row: integer(3),
                column: integer(4),
            }),
        ),
        818 => tribe_request(true, true, None),
        819 => tribe_request(
            true,
            true,
            Some(TypePlacementRequest {
                building_name: string(2),
                row: integer(3),
                column: integer(4),
            }),
        ),
        _ => unreachable!("owns_registration admitted this exact cohort"),
    }
}

fn arguments_match(args: &[Value], params: &[ScriptTy]) -> bool {
    args.len() == params.len()
        && args.iter().zip(params).all(|(value, expected)| {
            matches!(
                (value, expected),
                (Value::Int(_), ScriptTy::Int) | (Value::Str(_), ScriptTy::Str)
            )
        })
}
