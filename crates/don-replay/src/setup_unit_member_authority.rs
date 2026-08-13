//! Canonical setup-Unit member binding for replay consumers.
//!
//! [`crate::setup_units_producer`] owns the exact `Setup::build_units` schedule and its
//! `Setup::place_unit -> Objects::init_unit` receipt boundary.  This module performs the
//! missing product join: it resolves selected receipt identities against one canonical
//! [`don_sim::tick::Sim`] snapshot and attaches the replay-carried `UnitTypeData` projection.
//! It creates no Unit and never consults a recorded checksum.

#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::fmt;

use don_sim::systems::canonical_group_move_host::{UnitIdentity, UnitImage};
use don_sim::systems::group_move_authority::{GroupMoveTypeFacts, ResolvedLandSpeed};
use don_sim::systems::map_terrain::WorldChecksum;
use don_sim::tick::Sim;
use don_sim::world::OBJ_FLAG_ACTIVE;

use crate::groups_pre_pair_unit_authority::{
    replay_unit_type_facts, PrePairUnitAuthorityError, ReplayUnitTypeFacts,
};
use crate::replay::{load_payload, Replay};
use crate::setup_units_producer::{
    validate_build_units_prefix_receipt, BuildUnitsPlan, BuildUnitsPrefixReceipt,
    BuildUnitsReceiptError, InitUnitAuthorityReceipt, PlaceUnitCall, PlacementOutcomeReceipt,
    StableUnitIdentityReceipt, StartingUnitPhase, UnitMemberAuthorityReceipt,
};
use crate::world_owner_frontier::sha256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CanonicalSetupMemberSource {
    ReplayRulesCompleteInitReceiptAndCanonicalSim,
}

/// Revisioned provenance for the canonical Sim snapshot being joined.
///
/// The build-units receipt owns allocation chronology.  These fields bind the later dynamic
/// Unit image to one replay and one whole Sim snapshot without pretending that the replay file
/// serialized that image.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalSetupSnapshotAuthority {
    pub revision: u64,
    pub composition_digest: [u8; 32],
    pub source: CanonicalSetupMemberSource,
    pub replay_file_sha256: [u8; 32],
    pub frame: i32,
    pub world_checksum: WorldChecksum,
    pub random_state: i32,
}

/// One exact setup allocation resolved against the current canonical Unit owner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalSetupUnitMemberReceipt {
    pub authority_revision: u64,
    pub authority_digest: [u8; 32],
    pub source: CanonicalSetupMemberSource,
    pub replay_file_sha256: [u8; 32],
    pub setup_ordinal: usize,
    pub member_ordinal: usize,
    pub call: PlaceUnitCall,
    pub init: InitUnitAuthorityReceipt,
    pub allocation: UnitMemberAuthorityReceipt,
    pub frame: i32,
    pub row: usize,
    pub current_type: i32,
    pub type_facts: ReplayUnitTypeFacts,
    pub unit: UnitImage,
}

impl CanonicalSetupUnitMemberReceipt {
    pub const fn stable_identity(&self) -> StableUnitIdentityReceipt {
        self.allocation.identity
    }

    pub const fn is_citizen(&self) -> bool {
        matches!(self.call.phase, StartingUnitPhase::Citizen { .. })
    }

    /// Exact immutable fields consumed by the canonical Group-Move authority producer.
    ///
    /// Dynamic formation width/facing comes from [`Self::unit`]. Land speed intentionally does
    /// not: retail `UnitData::speed` is a live terrain/Leader/hero/Constants join, not the Rules
    /// `moves` scalar.
    pub const fn group_move_type_facts(&self) -> GroupMoveTypeFacts {
        GroupMoveTypeFacts {
            type_id: self.type_facts.type_index,
            attack: self.type_facts.attack,
            max_range: self.type_facts.max_range,
            obj_masks: self.type_facts.obj_masks,
            unit_flags: self.type_facts.unit_flags,
            unit_flags2: self.type_facts.unit_flags2,
            role: self.type_facts.role,
            domain: self.type_facts.domain,
            age: self.type_facts.age,
            guy_spacing: self.type_facts.guy_spacing,
            x_spacing: self.type_facts.x_spacing,
            y_spacing: self.type_facts.y_spacing,
            uber_size: self.type_facts.uber_size,
        }
    }

    /// Bind one independently resolved land-speed result to this exact Unit generation.
    pub const fn bind_resolved_land_speed(&self, speed: i32) -> ResolvedLandSpeed {
        ResolvedLandSpeed {
            handle: self.unit.identity.handle,
            speed,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CanonicalSetupMemberError {
    MissingAuthorityRevision,
    MissingCompositionDigest,
    ReplayRead(String),
    ReplayFileSha256Mismatch,
    PayloadRead(String),
    MissingRules,
    SetupReceipt(BuildUnitsReceiptError),
    EmptySelection,
    NonIncreasingOrdinal {
        previous: usize,
        current: usize,
    },
    SetupOrdinalOutOfRange {
        ordinal: usize,
    },
    SelectedPlacementDidNotSpawn {
        ordinal: usize,
    },
    WrongFrame {
        expected: i32,
        actual: i32,
    },
    WorldChecksumMismatch,
    RandomStateMismatch,
    RegistryNotDenseEquivalent,
    UnitMarkBehindReceipt {
        owner: i32,
        mark: i32,
        required: i32,
    },
    MissingCanonicalUnit {
        owner: i32,
        o: i32,
    },
    InactiveCanonicalUnit {
        owner: i32,
        o: i32,
    },
    MissingCanonicalHandle {
        owner: i32,
        o: i32,
    },
    StaleCanonicalHandle {
        owner: i32,
        o: i32,
    },
    MissingCanonicalType {
        owner: i32,
        o: i32,
    },
    CanonicalTypeMismatch {
        owner: i32,
        o: i32,
        receipt_type: i32,
        world_type: i32,
        sim_type: i32,
    },
    MissingCanonicalPath {
        owner: i32,
        o: i32,
    },
    DuplicateCanonicalRow {
        row: usize,
    },
    TypeFacts(PrePairUnitAuthorityError),
    CitizenSelectionContainsNonCitizen {
        ordinal: usize,
    },
}

impl fmt::Display for CanonicalSetupMemberError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "canonical setup-Unit member join refused: {self:?}")
    }
}

impl std::error::Error for CanonicalSetupMemberError {}

impl From<BuildUnitsReceiptError> for CanonicalSetupMemberError {
    fn from(value: BuildUnitsReceiptError) -> Self {
        Self::SetupReceipt(value)
    }
}

impl From<PrePairUnitAuthorityError> for CanonicalSetupMemberError {
    fn from(value: PrePairUnitAuthorityError) -> Self {
        Self::TypeFacts(value)
    }
}

/// Resolve selected setup calls against a canonical Sim snapshot.
///
/// `ordinals` must be strictly increasing. Every member of every selected spawned call is
/// returned in receipt order. The complete setup receipt is validated first, including its RNG,
/// container-shape, Guy-identity, and generational-authority invariants.
pub fn bind_canonical_setup_members(
    replay: &Replay,
    plan: &BuildUnitsPlan,
    setup: &BuildUnitsPrefixReceipt,
    sim: &Sim,
    ordinals: &[usize],
    authority: &CanonicalSetupSnapshotAuthority,
) -> Result<Vec<CanonicalSetupUnitMemberReceipt>, CanonicalSetupMemberError> {
    if authority.revision == 0 {
        return Err(CanonicalSetupMemberError::MissingAuthorityRevision);
    }
    if authority.composition_digest == [0; 32] {
        return Err(CanonicalSetupMemberError::MissingCompositionDigest);
    }
    if ordinals.is_empty() {
        return Err(CanonicalSetupMemberError::EmptySelection);
    }
    for pair in ordinals.windows(2) {
        if pair[0] >= pair[1] {
            return Err(CanonicalSetupMemberError::NonIncreasingOrdinal {
                previous: pair[0],
                current: pair[1],
            });
        }
    }

    let replay_bytes = std::fs::read(&replay.path)
        .map_err(|error| CanonicalSetupMemberError::ReplayRead(error.to_string()))?;
    let replay_file_sha256 = sha256(&replay_bytes);
    if replay_file_sha256 != authority.replay_file_sha256 {
        return Err(CanonicalSetupMemberError::ReplayFileSha256Mismatch);
    }
    validate_build_units_prefix_receipt(plan, setup)?;
    if sim.world.frame != authority.frame {
        return Err(CanonicalSetupMemberError::WrongFrame {
            expected: authority.frame,
            actual: sim.world.frame,
        });
    }
    if sim.map.world.checksum_sections() != authority.world_checksum {
        return Err(CanonicalSetupMemberError::WorldChecksumMismatch);
    }
    if sim.world.random.state() != authority.random_state {
        return Err(CanonicalSetupMemberError::RandomStateMismatch);
    }
    if !sim.world.object_bands_are_dense_equivalent() {
        return Err(CanonicalSetupMemberError::RegistryNotDenseEquivalent);
    }

    let payload = load_payload(&replay.path)
        .map_err(|error| CanonicalSetupMemberError::PayloadRead(error.to_string()))?;
    let rules = replay
        .initial
        .rules
        .as_ref()
        .ok_or(CanonicalSetupMemberError::MissingRules)?;
    let mut rows = BTreeSet::new();
    let mut result = Vec::new();
    for &ordinal in ordinals {
        let call = *plan
            .calls
            .get(ordinal)
            .ok_or(CanonicalSetupMemberError::SetupOrdinalOutOfRange { ordinal })?;
        let placement = setup
            .placements
            .get(ordinal)
            .ok_or(CanonicalSetupMemberError::SetupOrdinalOutOfRange { ordinal })?;
        let PlacementOutcomeReceipt::Spawned(init) = &placement.outcome else {
            return Err(CanonicalSetupMemberError::SelectedPlacementDidNotSpawn { ordinal });
        };
        for (member_ordinal, allocation) in init.members.iter().enumerate() {
            let owner = allocation.identity.owner;
            let o = allocation.identity.o;
            let mark = usize::try_from(owner)
                .ok()
                .and_then(|owner| sim.world.unit_mark(owner))
                .unwrap_or(-1);
            if mark <= o {
                return Err(CanonicalSetupMemberError::UnitMarkBehindReceipt {
                    owner,
                    mark,
                    required: o.saturating_add(1),
                });
            }
            let row = sim
                .world
                .unit_row_at(owner, o)
                .ok_or(CanonicalSetupMemberError::MissingCanonicalUnit { owner, o })?;
            if sim.world.units.get_flags(row) & OBJ_FLAG_ACTIVE == 0 {
                return Err(CanonicalSetupMemberError::InactiveCanonicalUnit { owner, o });
            }
            let handle = sim
                .world
                .handle_at_row(row)
                .ok_or(CanonicalSetupMemberError::MissingCanonicalHandle { owner, o })?;
            if (handle.id, handle.generation)
                != (allocation.identity.id, allocation.identity.generation)
            {
                return Err(CanonicalSetupMemberError::StaleCanonicalHandle { owner, o });
            }
            let world_type = sim
                .world
                .unit_type_id(row)
                .ok_or(CanonicalSetupMemberError::MissingCanonicalType { owner, o })?;
            let sim_type = sim
                .unit_type
                .get(row)
                .copied()
                .ok_or(CanonicalSetupMemberError::MissingCanonicalType { owner, o })?;
            if allocation.ptype_index != call.place_unit_upgrade
                || world_type != allocation.ptype_index
                || sim_type != world_type
            {
                return Err(CanonicalSetupMemberError::CanonicalTypeMismatch {
                    owner,
                    o,
                    receipt_type: allocation.ptype_index,
                    world_type,
                    sim_type,
                });
            }
            let path = sim
                .paths
                .get(row)
                .cloned()
                .ok_or(CanonicalSetupMemberError::MissingCanonicalPath { owner, o })?;
            if !rows.insert(row) {
                return Err(CanonicalSetupMemberError::DuplicateCanonicalRow { row });
            }
            let who = u8::try_from(owner)
                .map_err(|_| CanonicalSetupMemberError::MissingCanonicalUnit { owner, o })?;
            let o_i16 = i16::try_from(o)
                .map_err(|_| CanonicalSetupMemberError::MissingCanonicalUnit { owner, o })?;
            let type_facts = replay_unit_type_facts(&payload, rules, world_type)?;
            result.push(CanonicalSetupUnitMemberReceipt {
                authority_revision: authority.revision,
                authority_digest: authority.composition_digest,
                source: authority.source,
                replay_file_sha256,
                setup_ordinal: ordinal,
                member_ordinal,
                call,
                init: init.clone(),
                allocation: allocation.clone(),
                frame: sim.world.frame,
                row,
                current_type: world_type,
                type_facts,
                unit: UnitImage {
                    identity: UnitIdentity {
                        handle,
                        who,
                        o: o_i16,
                        uid: sim.world.units.get_uid(row),
                    },
                    group: sim.world.units.group()[row],
                    unit_masks: sim.world.units.get_unit_masks(row),
                    form: sim.world.units.form()[row],
                    form_mod: sim.world.units.form_mod()[row],
                    angle: sim.world.units.angle()[row],
                    x: sim.world.units.x_internal()[row],
                    y: sim.world.units.y_internal()[row],
                    orders_x: sim.world.units.orders_x()[row],
                    orders_y: sim.world.units.orders_y()[row],
                    dest_angle: sim.world.units.dest_angle()[row],
                    orders: sim.world.orders(row).clone(),
                    path,
                },
            });
        }
    }
    Ok(result)
}

/// Bind every Citizen call in the setup plan, retaining its native ordinal.
///
/// This is the product hook needed by clear-pool Groups replays: for the ordinary Dutch opening
/// it selects Citizen calls 3..6, after the Scout and two Dutch Merchants. Nothing here assumes
/// those ordinals or object ids; the validated setup receipt decides them.
pub fn bind_canonical_setup_citizens(
    replay: &Replay,
    plan: &BuildUnitsPlan,
    setup: &BuildUnitsPrefixReceipt,
    sim: &Sim,
    authority: &CanonicalSetupSnapshotAuthority,
) -> Result<Vec<CanonicalSetupUnitMemberReceipt>, CanonicalSetupMemberError> {
    let ordinals: Vec<_> = plan
        .calls
        .iter()
        .enumerate()
        .filter_map(|(ordinal, call)| {
            matches!(call.phase, StartingUnitPhase::Citizen { .. }).then_some(ordinal)
        })
        .collect();
    let members = bind_canonical_setup_members(replay, plan, setup, sim, &ordinals, authority)?;
    if let Some(member) = members.iter().find(|member| !member.is_citizen()) {
        return Err(
            CanonicalSetupMemberError::CitizenSelectionContainsNonCitizen {
                ordinal: member.setup_ordinal,
            },
        );
    }
    Ok(members)
}
