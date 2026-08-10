//! Live, typed dispatch boundary for the retail BHS type builtins.
//!
//! [`super::bhs_type_table::TypeBuiltinState`] is the sole mutable owner.  This adapter does
//! not copy a rule projection into the script VM: it verifies the shipped declaration identity,
//! executes the owner method, and publishes a revision-bound receipt.  Save and checksum
//! admission methods remain explicit. DoNSave v7 still rejects the external owner; checksum
//! admission requires the exact immutable Type walk source and reprojects the current mutation
//! revision. [`crate::bhs_session::BhsSession`] supplies the opaque production owner.

use don_bhs::{builtin, BuiltinDecl, ScriptTy, Value};

use super::bhs_type_channel13_frontier::{
    project_type_owner, InstalledTypeOwnerReceipt, ProjectedTypeRules, TypeChannel13Error,
    TypePersistenceOwner, TypeWalkSource,
};
use super::bhs_type_factory::TypeBuiltinProvenance;
use super::bhs_type_stat_frontier::{
    plan_type_stat_mutation, LeaderStatRecalcCall, TypeStatBuiltin, TypeStatFrontierError,
    TypeStatPlan,
};
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
    /// Immediate retail cache calls required after the canonical row commit.
    pub leader_recalcs: Vec<LeaderStatRecalcCall>,
    /// False is a visible production boundary: direct runtime dispatch owns no live `Sim`.
    pub leader_recalcs_applied: bool,
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
    SetTypeStat {
        builtin: TypeStatBuiltin,
        name: String,
        value: i32,
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
    TypeStatOwnerFault(TypeStatFrontierError),
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
    /// A typed type-stat planning/commit boundary, including a stale relation-family plan.
    TypeStatOwner(TypeStatFrontierError),
    /// A cache-effect acknowledgement did not name the currently published owner receipt.
    StaleReceipt { index: u32, revision_after: u64 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct OwnerCall {
    value: i32,
    leader_recalcs: Vec<LeaderStatRecalcCall>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum OwnerCallError {
    Table(TypeTableError),
    TypeStat(TypeStatFrontierError),
}

/// Why a larger persistence/checksum operation cannot yet consume this owner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TypeBuiltinBoundaryError {
    /// DoNSave v7 cannot restore the external owner or prove its synchronized rules/mod source.
    SaveOwnerUnowned { mutation_revision: u64, dirty: bool },
    /// The partial simulation digest has no complete retail `Types::walk_rules_data` projection.
    Channel13ProjectionUnowned,
    /// The installed projection source no longer admits the live owner/revision.
    Channel13ProjectionRejected(TypeChannel13Error),
    /// Scenario numeric groups and create-unit receipts live beside the Sim and have no v6 or
    /// checksum-channel encoding yet. Even a pristine installed owner cannot be omitted.
    CreateUnitOwnerUnowned {
        completed_calls: u64,
        faulted_calls: u64,
    },
}

/// One installed, canonical type owner plus its observable dispatch receipts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeBuiltinRuntime {
    state: TypeBuiltinState,
    channel13_provenance: Option<TypeBuiltinProvenance>,
    channel13_source: Option<TypeWalkSource>,
    last_receipt: Option<TypeBuiltinReceipt>,
    last_fault: Option<TypeBuiltinRuntimeError>,
}

impl TypeBuiltinRuntime {
    pub fn new(state: TypeBuiltinState) -> Self {
        Self {
            state,
            channel13_provenance: None,
            channel13_source: None,
            last_receipt: None,
            last_fault: None,
        }
    }

    /// Join the canonical mutable owner to its immutable normalized Type walk before scripts run.
    ///
    /// Construction projects the pristine owner immediately, so mismatched provenance, Strings,
    /// relations, scalar bytes, walker bands, or source checkpoints fail before the runtime can
    /// publish a session. Later checksum/persistence reads project again against the live mutation
    /// revision rather than retaining a stale digest.
    pub fn new_with_channel13(
        state: TypeBuiltinState,
        provenance: TypeBuiltinProvenance,
        channel13_source: TypeWalkSource,
    ) -> Result<Self, TypeChannel13Error> {
        let runtime = Self {
            state,
            channel13_provenance: Some(provenance),
            channel13_source: Some(channel13_source),
            last_receipt: None,
            last_fault: None,
        };
        runtime.projected_type_rules()?;
        Ok(runtime)
    }

    pub fn state(&self) -> &TypeBuiltinState {
        &self.state
    }

    pub fn has_channel13_source(&self) -> bool {
        self.channel13_source.is_some()
    }

    /// Live revision-bound projection of the exact Type prefix of retail checksum channel 13.
    pub fn projected_type_rules(&self) -> Result<ProjectedTypeRules<'_>, TypeChannel13Error> {
        let provenance = self
            .channel13_provenance
            .ok_or(TypeChannel13Error::SourceUnowned)?;
        let source = self
            .channel13_source
            .as_ref()
            .ok_or(TypeChannel13Error::SourceUnowned)?;
        project_type_owner(
            &self.state,
            InstalledTypeOwnerReceipt {
                provenance,
                dirty: Some(self.state.is_dirty()),
                mutation_revision: Some(self.state.mutation_revision()),
            },
            source,
        )
    }

    /// Exact Types-prefix checkpoint admitted for the current owner revision.
    pub fn type_channel13_checkpoint(&self) -> Result<u32, TypeChannel13Error> {
        Ok(self.projected_type_rules()?.after_types())
    }

    /// Borrowed persistence contract retaining the full owner, source identity, and live revision.
    /// This is not an admission for the current DoNSave format, which still cannot restore it.
    pub fn type_persistence_owner(&self) -> Result<TypePersistenceOwner<'_>, TypeChannel13Error> {
        Ok(self.projected_type_rules()?.persistence_owner())
    }

    pub fn last_receipt(&self) -> Option<&TypeBuiltinReceipt> {
        self.last_receipt.as_ref()
    }

    pub fn last_fault(&self) -> Option<&TypeBuiltinRuntimeError> {
        self.last_fault.as_ref()
    }

    /// The current DoNSave format has no chunk or rules/mod provenance for this externally
    /// installed owner.
    /// Even pristine state is rejected: `load_sim` returns only `Sim` and could not reattach it.
    pub fn admit_save(&self) -> Result<(), TypeBuiltinBoundaryError> {
        Err(TypeBuiltinBoundaryError::SaveOwnerUnowned {
            mutation_revision: self.state.mutation_revision(),
            dirty: self.state.is_dirty(),
        })
    }

    /// Admit the partial simulation digest only after a live revision-bound Types projection.
    pub fn admit_partial_channel_digest(&self) -> Result<(), TypeBuiltinBoundaryError> {
        match self.projected_type_rules() {
            Ok(_) => Ok(()),
            Err(TypeChannel13Error::SourceUnowned) => {
                Err(TypeBuiltinBoundaryError::Channel13ProjectionUnowned)
            }
            Err(error) => Err(TypeBuiltinBoundaryError::Channel13ProjectionRejected(error)),
        }
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
        let leader_recalcs = result
            .as_ref()
            .map(|call| call.leader_recalcs.clone())
            .unwrap_or_default();
        let receipt = TypeBuiltinReceipt {
            index: decl.index,
            handler_va: decl.handler_va,
            request,
            outcome: match &result {
                Ok(call) => TypeBuiltinOutcome::Returned(call.value),
                Err(OwnerCallError::Table(fault)) => TypeBuiltinOutcome::OwnerFault(fault.clone()),
                Err(OwnerCallError::TypeStat(fault)) => {
                    TypeBuiltinOutcome::TypeStatOwnerFault(*fault)
                }
            },
            revision_before,
            revision_after: self.state.mutation_revision(),
            state_dirty: self.state.is_dirty(),
            leader_recalcs_applied: leader_recalcs.is_empty(),
            leader_recalcs,
        };
        self.last_receipt = Some(receipt.clone());
        match result {
            Ok(_) => Ok(Some(receipt)),
            Err(OwnerCallError::Table(fault)) => self.fail(TypeBuiltinRuntimeError::Owner(fault)),
            Err(OwnerCallError::TypeStat(fault)) => {
                self.fail(TypeBuiltinRuntimeError::TypeStatOwner(fault))
            }
        }
    }

    /// Confirm that the live simulation applied this receipt's immediate Leader tail.
    ///
    /// The revision/index check prevents a delayed host acknowledgement from blessing the
    /// cache effects of a newer mutation. Production calls this before returning to the VM.
    pub(crate) fn confirm_leader_recalcs(
        &mut self,
        receipt: &TypeBuiltinReceipt,
    ) -> Result<(), TypeBuiltinRuntimeError> {
        let current_matches = self.last_receipt.as_ref().is_some_and(|current| {
            current.index == receipt.index
                && current.revision_before == receipt.revision_before
                && current.revision_after == receipt.revision_after
                && current.request == receipt.request
                && current.leader_recalcs == receipt.leader_recalcs
        });
        if !current_matches {
            return self.fail(TypeBuiltinRuntimeError::StaleReceipt {
                index: receipt.index,
                revision_after: receipt.revision_after,
            });
        }
        self.last_receipt
            .as_mut()
            .expect("current_matches requires a published receipt")
            .leader_recalcs_applied = true;
        Ok(())
    }

    fn fail<T>(&mut self, fault: TypeBuiltinRuntimeError) -> Result<T, TypeBuiltinRuntimeError> {
        self.last_fault = Some(fault.clone());
        Err(fault)
    }

    fn call_owner(&mut self, index: u32, args: &[Value]) -> Result<OwnerCall, OwnerCallError> {
        let string = |slot: usize| match &args[slot] {
            Value::Str(value) => value.as_str(),
            _ => unreachable!("arguments_match admitted exact scalar shapes"),
        };
        let integer = |slot: usize| match &args[slot] {
            Value::Int(value) => *value,
            _ => unreachable!("arguments_match admitted exact scalar shapes"),
        };

        let table_call = |result: Result<i32, TypeTableError>| {
            result
                .map(|value| OwnerCall {
                    value,
                    leader_recalcs: Vec::new(),
                })
                .map_err(OwnerCallError::Table)
        };

        match index {
            284 => table_call(self.state.disable_type(string(0))),
            286 => table_call(self.state.enable_type(string(0))),
            288 => table_call(self.state.rename_type(string(0), string(1))),
            289 => table_call(self.state.type_build_time(string(0))),
            290 => table_call(self.state.set_type_build_time(string(0), integer(1))),
            291 => table_call(self.state.set_type_job_time(string(0), integer(1))),
            529 | 531 | 532 | 533 | 534 | 535 | 538 | 814 => {
                let builtin = TypeStatBuiltin::from_registration(index)
                    .expect("matched registration has a frozen type-stat identity");
                let plan = plan_type_stat_mutation(&self.state, builtin, string(0), integer(1))
                    .map_err(OwnerCallError::TypeStat)?;
                match plan {
                    TypeStatPlan::Rejected => Ok(OwnerCall {
                        value: -1,
                        leader_recalcs: Vec::new(),
                    }),
                    TypeStatPlan::Admitted(plan) => {
                        self.state
                            .apply_type_stat_plan(&plan)
                            .map_err(OwnerCallError::TypeStat)?;
                        Ok(OwnerCall {
                            value: plan.return_value,
                            leader_recalcs: plan.leader_recalcs,
                        })
                    }
                }
            }
            815 => table_call(self.state.disable_type_by_tribe(string(0), string(1))),
            816 => table_call(self.state.enable_type_by_tribe(string(0), string(1))),
            817 => table_call(self.state.enable_type_by_tribe_at(
                string(0),
                string(1),
                string(2),
                integer(3),
                integer(4),
            )),
            818 => table_call(
                self.state
                    .enable_type_by_tribe_with_type_name(string(0), string(1)),
            ),
            819 => table_call(self.state.enable_type_by_tribe_with_type_name_at(
                string(0),
                string(1),
                string(2),
                integer(3),
                integer(4),
            )),
            _ => unreachable!("declaration admitted this exact cohort"),
        }
    }
}

fn owns_registration(index: u32) -> bool {
    matches!(
        index,
        284 | 286
            | 288
            | 289
            | 290
            | 291
            | 529
            | 531
            | 532
            | 533
            | 534
            | 535
            | 538
            | 814
            | 815
            | 816
            | 817
            | 818
            | 819
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
        529 | 531 | 532 | 533 | 534 | 535 | 538 | 814 => TypeBuiltinRequest::SetTypeStat {
            builtin: TypeStatBuiltin::from_registration(index)
                .expect("matched registration has a frozen type-stat identity"),
            name: string(0),
            value: integer(1),
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
