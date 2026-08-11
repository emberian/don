//! Canonical runtime owner for BHS registrations 508--510.
//!
//! The retail receiver is intentionally split at its first irreversible side effect.  Type
//! names and relations come from the session's canonical [`TypeBuiltinState`]; current-upgrade,
//! graft, domain, Unit flags, transport, and scenario numeric-group state are admitted here as
//! one composition-bound projection.  The live [`crate::tick::Sim`] supplies Leader flags and
//! terrain through [`CreateUnitLiveHost`].
//!
//! The currently executable production subset is every native prefix rejection, the
//! post-clear route rejection, and a zero-count success.  Positive counts stop at the typed
//! allocation-authority boundary before the first `Objects::init_unit` call.  The sequential
//! helper below freezes retail's non-atomic allocation rule for the future host: every attempt
//! runs, every successful id is published immediately, and the return value is the *last*
//! attempt even when an earlier unit remains live.

use std::collections::BTreeMap;
use std::fmt;

use don_bhs::{BuiltinDecl, Value};

use super::bhs_create_unit_frontier::{
    plan_create_unit_pre_group, plan_create_unit_route, retail_last_init_result, CreateUnitBuiltin,
    CreateUnitFacts, CreateUnitPreGroupPlan, CreateUnitPrefixError, CreateUnitRequest,
    CreateUnitRoute, CREATE_UNIT_COHORT_CALLS, SHIPPED_CORPUS_CALLS,
};
use super::bhs_type_factory::{RulesCompositionId, Sha256Digest, TypeBuiltinProvenance};
use super::bhs_type_table::{TypeBody, TypeBuiltinState, NUM_LEADERS, NUM_TYPES};

/// Shipped calls that can be proven to terminate inside the currently owned subset from source
/// literals alone.  The corpus has no zero/negative/oversized counts and no detectable
/// Transport-Barge type name, so registration reach must not be reported as executed coverage.
pub const STATICALLY_COMPLETE_SHIPPED_CALLS: u32 = 0;

/// Actual fully handled shipped-call coverage added by this deliberately fail-closed tranche.
pub const FULLY_HANDLED_SHIPPED_CALL_DELTA: u32 = 0;

/// Immutable source witness for the create-unit projection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CreateUnitProjectionWitness {
    pub composition: RulesCompositionId,
    pub manifest_sha256: Sha256Digest,
    pub component_sha256: Sha256Digest,
}

/// Installed facts read lazily from one effective Unit type after the scenario-group policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CreateUnitTypeProjection {
    pub type_index: i32,
    pub domain: i32,
    pub unit_flags: u32,
}

/// Installed `LeaderData` projections indexed by canonical type id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreateUnitLeaderProjection {
    pub leader_slot: usize,
    pub current_upgrade: Vec<Option<i32>>,
    pub graft: Vec<Option<i32>>,
}

/// One successful id retained by a persistent ScenarioData numeric group.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CreateUnitGroupMember {
    pub owner: u8,
    pub object_id: i32,
}

/// Composition-bound setup input. Sparse `None` entries are authority gaps, never defaults.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreateUnitRuntimeInput {
    pub witness: CreateUnitProjectionWitness,
    pub types: Vec<Option<CreateUnitTypeProjection>>,
    pub leaders: Vec<Option<CreateUnitLeaderProjection>>,
    pub numeric_groups: Vec<(i32, Vec<CreateUnitGroupMember>)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CreateUnitRuntimeSetupError {
    EmptyComponentDigest,
    CompositionMismatch,
    ManifestMismatch,
    WrongTypeCount {
        got: usize,
    },
    WrongLeaderCount {
        got: usize,
    },
    TypeIndexMismatch {
        slot: usize,
        type_index: i32,
    },
    NonUnitProjection {
        slot: usize,
    },
    InvalidDomain {
        slot: usize,
        domain: i32,
    },
    LeaderIndexMismatch {
        slot: usize,
        leader_slot: usize,
    },
    WrongUpgradeCount {
        leader: usize,
        got: usize,
    },
    WrongGraftCount {
        leader: usize,
        got: usize,
    },
    ProjectionTargetOutOfRange {
        leader: usize,
        source: usize,
        target: i32,
    },
    DuplicateNumericGroup(i32),
    InvalidGroupMember {
        key: i32,
        owner: u8,
        object_id: i32,
    },
}

impl fmt::Display for CreateUnitRuntimeSetupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for CreateUnitRuntimeSetupError {}

/// Why a registered call could not cross an unowned production tail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CreateUnitOwnerFault {
    Prefix(CreateUnitPrefixError),
    PositiveAllocationAuthority,
}

/// Observable terminal state of one registered call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CreateUnitOutcome {
    Returned(i32),
    OwnerFault(CreateUnitOwnerFault),
}

/// Production receipt retained even when the live VM stops on an authority fault.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreateUnitReceipt {
    pub registration: u32,
    pub request: OwnedCreateUnitRequest,
    pub pre_group: Option<CreateUnitPreGroupPlan>,
    pub route: Option<CreateUnitRoute>,
    pub cleared_group_key: Option<i32>,
    pub allocation_results: Vec<i32>,
    pub outcome: CreateUnitOutcome,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnedCreateUnitRequest {
    pub who: i32,
    pub x: i32,
    pub y: i32,
    pub requested_type_name: String,
    pub count: i32,
}

impl OwnedCreateUnitRequest {
    fn borrowed(&self) -> CreateUnitRequest<'_> {
        CreateUnitRequest {
            who: self.who,
            x: self.x,
            y: self.y,
            requested_type_name: &self.requested_type_name,
            count: self.count,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CreateUnitRuntimeError {
    BadArguments,
    OwnerFault(CreateUnitOwnerFault),
}

/// Live facts already authoritative on the joined simulation owner.
pub trait CreateUnitLiveHost {
    fn create_unit_leader_flags(&self, leader_slot: usize) -> Option<u32>;
    fn create_unit_world_valid(&self, world_x: i32, world_y: i32) -> Option<bool>;
    fn create_unit_world_is_ocean(&self, world_x: i32, world_y: i32) -> Option<bool>;
    fn create_unit_can_transport(&self, leader_slot: usize) -> Option<bool>;
}

impl CreateUnitLiveHost for crate::tick::Sim {
    fn create_unit_leader_flags(&self, leader_slot: usize) -> Option<u32> {
        self.step8
            .leaders
            .get(leader_slot)
            .map(|leader| leader.flags)
    }

    fn create_unit_world_valid(&self, world_x: i32, world_y: i32) -> Option<bool> {
        Some(self.map.world.valid_w(world_x, world_y))
    }

    fn create_unit_world_is_ocean(&self, world_x: i32, world_y: i32) -> Option<bool> {
        self.map
            .world
            .valid_w(world_x, world_y)
            .then(|| self.map.world.is_ocean(world_x, world_y))
    }

    fn create_unit_can_transport(&self, leader_slot: usize) -> Option<bool> {
        let flags = self.step8.leaders.get(leader_slot)?.flags;
        let level = if flags & 0x100 != 0 {
            3
        } else if flags & 0x200 != 0 {
            2
        } else {
            (flags >> 10) & 1
        };
        Some(level != 0)
    }
}

/// Persistent ScenarioFuncSet owner installed inside one [`crate::script_runtime::ScriptRuntime`].
#[derive(Clone, Debug)]
pub struct BhsCreateUnitRuntime {
    witness: CreateUnitProjectionWitness,
    types: Box<[Option<CreateUnitTypeProjection>]>,
    leaders: Box<[Option<CreateUnitLeaderProjection>]>,
    numeric_groups: BTreeMap<i32, Vec<CreateUnitGroupMember>>,
    last_receipt: Option<CreateUnitReceipt>,
    completed_calls: u64,
    faulted_calls: u64,
}

impl BhsCreateUnitRuntime {
    pub fn new(
        input: CreateUnitRuntimeInput,
        state: &TypeBuiltinState,
        provenance: TypeBuiltinProvenance,
    ) -> Result<Self, CreateUnitRuntimeSetupError> {
        if input
            .witness
            .component_sha256
            .0
            .iter()
            .all(|byte| *byte == 0)
        {
            return Err(CreateUnitRuntimeSetupError::EmptyComponentDigest);
        }
        if input.witness.composition != provenance.composition {
            return Err(CreateUnitRuntimeSetupError::CompositionMismatch);
        }
        if input.witness.manifest_sha256 != provenance.manifest_sha256 {
            return Err(CreateUnitRuntimeSetupError::ManifestMismatch);
        }
        if input.types.len() != NUM_TYPES {
            return Err(CreateUnitRuntimeSetupError::WrongTypeCount {
                got: input.types.len(),
            });
        }
        if input.leaders.len() != NUM_LEADERS {
            return Err(CreateUnitRuntimeSetupError::WrongLeaderCount {
                got: input.leaders.len(),
            });
        }
        for (slot, projection) in input.types.iter().enumerate() {
            let Some(projection) = projection else {
                continue;
            };
            if projection.type_index != slot as i32 {
                return Err(CreateUnitRuntimeSetupError::TypeIndexMismatch {
                    slot,
                    type_index: projection.type_index,
                });
            }
            if !matches!(state.types.row(slot).body, TypeBody::Unit { .. }) {
                return Err(CreateUnitRuntimeSetupError::NonUnitProjection { slot });
            }
            if !(0..=2).contains(&projection.domain) {
                return Err(CreateUnitRuntimeSetupError::InvalidDomain {
                    slot,
                    domain: projection.domain,
                });
            }
        }
        for (slot, projection) in input.leaders.iter().enumerate() {
            let Some(projection) = projection else {
                continue;
            };
            if projection.leader_slot != slot {
                return Err(CreateUnitRuntimeSetupError::LeaderIndexMismatch {
                    slot,
                    leader_slot: projection.leader_slot,
                });
            }
            if projection.current_upgrade.len() != NUM_TYPES {
                return Err(CreateUnitRuntimeSetupError::WrongUpgradeCount {
                    leader: slot,
                    got: projection.current_upgrade.len(),
                });
            }
            if projection.graft.len() != NUM_TYPES {
                return Err(CreateUnitRuntimeSetupError::WrongGraftCount {
                    leader: slot,
                    got: projection.graft.len(),
                });
            }
            for (source, target) in projection
                .current_upgrade
                .iter()
                .chain(projection.graft.iter())
                .copied()
                .enumerate()
                .filter_map(|(source, target)| target.map(|target| (source % NUM_TYPES, target)))
            {
                if !(0..NUM_TYPES as i32).contains(&target) {
                    return Err(CreateUnitRuntimeSetupError::ProjectionTargetOutOfRange {
                        leader: slot,
                        source,
                        target,
                    });
                }
            }
        }

        let mut numeric_groups = BTreeMap::new();
        for (key, members) in input.numeric_groups {
            if numeric_groups.contains_key(&key) {
                return Err(CreateUnitRuntimeSetupError::DuplicateNumericGroup(key));
            }
            for member in &members {
                if member.owner as usize >= NUM_LEADERS || member.object_id < 0 {
                    return Err(CreateUnitRuntimeSetupError::InvalidGroupMember {
                        key,
                        owner: member.owner,
                        object_id: member.object_id,
                    });
                }
            }
            numeric_groups.insert(key, members);
        }

        Ok(Self {
            witness: input.witness,
            types: input.types.into_boxed_slice(),
            leaders: input.leaders.into_boxed_slice(),
            numeric_groups,
            last_receipt: None,
            completed_calls: 0,
            faulted_calls: 0,
        })
    }

    pub fn witness(&self) -> CreateUnitProjectionWitness {
        self.witness
    }

    pub fn last_receipt(&self) -> Option<&CreateUnitReceipt> {
        self.last_receipt.as_ref()
    }

    pub fn numeric_group(&self, key: i32) -> Option<&[CreateUnitGroupMember]> {
        self.numeric_groups.get(&key).map(Vec::as_slice)
    }

    pub fn completed_calls(&self) -> u64 {
        self.completed_calls
    }

    pub fn faulted_calls(&self) -> u64 {
        self.faulted_calls
    }

    /// Resolve the exact read-only projection shared by the type-count builtins:
    /// `LeaderData::current_upgrade(source)` followed by
    /// `LeaderData::get_graft(current)`. Missing composition authority remains
    /// `None`; callers must not substitute the source type.
    pub fn effective_type_for_count(&self, leader_slot: usize, source_type: usize) -> Option<i32> {
        let leader = self.leaders.get(leader_slot)?.as_ref()?;
        let current =
            usize::try_from(leader.current_upgrade.get(source_type).copied().flatten()?).ok()?;
        leader.graft.get(current).copied().flatten()
    }

    pub fn shipped_prefix_reachable_calls(&self) -> u32 {
        CREATE_UNIT_COHORT_CALLS
    }

    pub fn shipped_prefix_reachable_percentage_points(&self) -> f64 {
        f64::from(CREATE_UNIT_COHORT_CALLS) * 100.0 / f64::from(SHIPPED_CORPUS_CALLS)
    }

    pub fn dispatch<H: CreateUnitLiveHost>(
        &mut self,
        host: &H,
        state: &TypeBuiltinState,
        decl: &BuiltinDecl,
        args: &[Value],
    ) -> Result<Option<i32>, CreateUnitRuntimeError> {
        let Some(builtin) = builtin_for_registration(decl.index) else {
            return Ok(None);
        };
        let request = parse_request(args).ok_or(CreateUnitRuntimeError::BadArguments)?;
        let facts = CanonicalFacts {
            runtime: self,
            state,
            host,
        };

        let pre = match plan_create_unit_pre_group(&facts, builtin, &request.borrowed()) {
            Ok(pre) => pre,
            Err(error) if native_prefix_rejection(error) => {
                self.completed_calls = self.completed_calls.wrapping_add(1);
                self.last_receipt = Some(CreateUnitReceipt {
                    registration: decl.index,
                    request,
                    pre_group: None,
                    route: None,
                    cleared_group_key: None,
                    allocation_results: Vec::new(),
                    outcome: CreateUnitOutcome::Returned(-1),
                });
                return Ok(Some(-1));
            }
            Err(error) => {
                let fault = CreateUnitOwnerFault::Prefix(error);
                self.faulted_calls = self.faulted_calls.wrapping_add(1);
                self.last_receipt = Some(CreateUnitReceipt {
                    registration: decl.index,
                    request,
                    pre_group: None,
                    route: None,
                    cleared_group_key: None,
                    allocation_results: Vec::new(),
                    outcome: CreateUnitOutcome::OwnerFault(fault),
                });
                return Err(CreateUnitRuntimeError::OwnerFault(fault));
            }
        };

        let cleared_group_key = pre.clear_group_key;
        if let Some(key) = cleared_group_key {
            self.numeric_groups.entry(key).or_default().clear();
        }

        // Rebuild the read-only facade after the mutable clear. The facts it exposes are
        // immutable projections; the numeric-group map is deliberately not borrowed by it.
        let facts = CanonicalFacts {
            runtime: self,
            state,
            host,
        };
        let route = match plan_create_unit_route(&facts, &pre) {
            Ok(route) => route,
            Err(error) => {
                let fault = CreateUnitOwnerFault::Prefix(error);
                self.faulted_calls = self.faulted_calls.wrapping_add(1);
                self.last_receipt = Some(CreateUnitReceipt {
                    registration: decl.index,
                    request,
                    pre_group: Some(pre),
                    route: None,
                    cleared_group_key,
                    allocation_results: Vec::new(),
                    outcome: CreateUnitOutcome::OwnerFault(fault),
                });
                return Err(CreateUnitRuntimeError::OwnerFault(fault));
            }
        };

        if route == CreateUnitRoute::RejectAfterGroupPolicy || pre.count == 0 {
            self.completed_calls = self.completed_calls.wrapping_add(1);
            self.last_receipt = Some(CreateUnitReceipt {
                registration: decl.index,
                request,
                pre_group: Some(pre),
                route: Some(route),
                cleared_group_key,
                allocation_results: Vec::new(),
                outcome: CreateUnitOutcome::Returned(-1),
            });
            return Ok(Some(-1));
        }

        let fault = CreateUnitOwnerFault::PositiveAllocationAuthority;
        self.faulted_calls = self.faulted_calls.wrapping_add(1);
        self.last_receipt = Some(CreateUnitReceipt {
            registration: decl.index,
            request,
            pre_group: Some(pre),
            route: Some(route),
            cleared_group_key,
            allocation_results: Vec::new(),
            outcome: CreateUnitOutcome::OwnerFault(fault),
        });
        Err(CreateUnitRuntimeError::OwnerFault(fault))
    }
}

struct CanonicalFacts<'a, H> {
    runtime: &'a BhsCreateUnitRuntime,
    state: &'a TypeBuiltinState,
    host: &'a H,
}

impl<H: CreateUnitLiveHost> CreateUnitFacts for CanonicalFacts<'_, H> {
    fn resolve_type(&self, name: &str) -> Option<i32> {
        if !name.is_ascii() || name.is_empty() {
            return None;
        }
        self.state
            .types
            .rows()
            .iter()
            .position(|row| row.name.len() == name.len() && row.name.eq_ignore_ascii_case(name))
            .map(|slot| slot as i32)
    }

    fn canonical_type_name(&self, type_id: i32) -> Option<&str> {
        usize::try_from(type_id)
            .ok()
            .and_then(|slot| self.state.types.rows().get(slot))
            .map(|row| row.name.as_str())
    }

    fn leader_flags(&self, leader_slot: usize) -> Option<u32> {
        self.host.create_unit_leader_flags(leader_slot)
    }

    fn current_upgrade(&self, leader_slot: usize, type_id: i32) -> Option<i32> {
        let type_id = usize::try_from(type_id).ok()?;
        self.runtime
            .leaders
            .get(leader_slot)?
            .as_ref()?
            .current_upgrade
            .get(type_id)
            .copied()
            .flatten()
    }

    fn leader_graft(&self, leader_slot: usize, type_id: i32) -> Option<i32> {
        let type_id = usize::try_from(type_id).ok()?;
        self.runtime
            .leaders
            .get(leader_slot)?
            .as_ref()?
            .graft
            .get(type_id)
            .copied()
            .flatten()
    }

    fn is_unit_type(&self, type_id: i32) -> Option<bool> {
        let slot = usize::try_from(type_id).ok()?;
        self.state
            .types
            .rows()
            .get(slot)
            .map(|row| matches!(row.body, TypeBody::Unit { .. }))
    }

    fn is_transport_barge_relation(&self, type_id: i32) -> Option<bool> {
        let slot = usize::try_from(type_id).ok()?;
        self.state.types.rows().get(slot).map(|row| {
            row.is_list
                .iter()
                .any(|&relation| i32::from(relation) == 0x140)
        })
    }

    fn domain(&self, type_id: i32) -> Option<i32> {
        let slot = usize::try_from(type_id).ok()?;
        self.runtime
            .types
            .get(slot)?
            .as_ref()
            .map(|facts| facts.domain)
    }

    fn unit_flags(&self, type_id: i32) -> Option<u32> {
        let slot = usize::try_from(type_id).ok()?;
        self.runtime
            .types
            .get(slot)?
            .as_ref()
            .map(|facts| facts.unit_flags)
    }

    fn world_valid(&self, world_x: i32, world_y: i32) -> Option<bool> {
        self.host.create_unit_world_valid(world_x, world_y)
    }

    fn world_is_ocean(&self, world_x: i32, world_y: i32) -> Option<bool> {
        self.host.create_unit_world_is_ocean(world_x, world_y)
    }

    fn can_transport(&self, leader_slot: usize) -> Option<bool> {
        self.host.create_unit_can_transport(leader_slot)
    }
}

fn builtin_for_registration(index: u32) -> Option<CreateUnitBuiltin> {
    match index {
        508 => Some(CreateUnitBuiltin::CreateUnit),
        509 => Some(CreateUnitBuiltin::CreateUnitUpgrade),
        510 => Some(CreateUnitBuiltin::CreateUnitInGroup),
        _ => None,
    }
}

fn parse_request(args: &[Value]) -> Option<OwnedCreateUnitRequest> {
    if args.len() != 5 {
        return None;
    }
    let Value::Int(who) = args[0] else {
        return None;
    };
    let Value::Int(x) = args[1] else { return None };
    let Value::Int(y) = args[2] else { return None };
    let Value::Str(ref name) = args[3] else {
        return None;
    };
    let Value::Int(count) = args[4] else {
        return None;
    };
    Some(OwnedCreateUnitRequest {
        who,
        x,
        y,
        requested_type_name: name.to_string(),
        count,
    })
}

fn native_prefix_rejection(error: CreateUnitPrefixError) -> bool {
    matches!(
        error,
        CreateUnitPrefixError::RequestedTypeMissing
            | CreateUnitPrefixError::PlayerOutOfRange
            | CreateUnitPrefixError::WrapperLeaderNotInGame
            | CreateUnitPrefixError::CoreLeaderNotActive
            | CreateUnitPrefixError::CountOutOfRange
            | CreateUnitPrefixError::EffectiveTypeMissing
            | CreateUnitPrefixError::NotAUnitType
            | CreateUnitPrefixError::InvalidWorldCoordinate
    )
}

/// Execute the native non-atomic allocation loop against a fully preflighted future host.
///
/// `allocate` is called exactly `count` times. `publish_success` runs immediately for every
/// nonnegative id, before the next attempt. A negative final result is returned even when an
/// earlier success remains published.
pub fn execute_non_atomic_allocations(
    count: u32,
    mut allocate: impl FnMut() -> i32,
    mut publish_success: impl FnMut(i32),
) -> (Vec<i32>, i32) {
    let mut results = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let object_id = allocate();
        if object_id >= 0 {
            publish_success(object_id);
        }
        results.push(object_id);
    }
    let returned = retail_last_init_result(results.iter().copied());
    (results, returned)
}
