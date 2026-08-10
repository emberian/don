//! First full-body tranche of retail `Unit::come_out(int)`.
//!
//! This source-only planner owns `0x00617C10..0x006186B3`: entry cleanup, captain
//! redirection, the complete uncontained search, the Oil Platform transport bridge, and the
//! contained placement/unlink phase.  It deliberately stops immediately before the common
//! `Unit::set_new_location(x, y, 1, 1)` call at `0x006186B4`.

pub const UNIT_COME_OUT_VA: u32 = 0x0061_7c10;
pub const UNIT_COME_OUT_BYTES: u32 = 9_925;
pub const UNIT_COME_OUT_END_VA: u32 = UNIT_COME_OUT_VA + UNIT_COME_OUT_BYTES;
pub const PREFIX_END_VA: u32 = 0x0061_86b4;
pub const PREFIX_BYTES: u32 = PREFIX_END_VA - UNIT_COME_OUT_VA;
pub const RESIDUAL_BYTES: u32 = UNIT_COME_OUT_BYTES - PREFIX_BYTES;
pub const PREFIX_DIRECT_RANDOM_GET_CALL_VAS: [u32; 0] = [];
pub const RESIDUAL_DIRECT_RANDOM_GET_CALL_VAS: [u32; 2] = [0x0061_a1bb, 0x0061_a1d5];

pub const TRANSPORT_BARGE_TYPE: i32 = 0x140;
pub const UNIVERSITY_TYPE: i32 = 0x1a4;
pub const OIL_PLATFORM_TYPE: i32 = 0x1a6;
pub const LEADER_EXIT_FLAG: u32 = 0x0200_0000;
pub const DIRECT_CONTAINER_LOCATION_MASK: u32 = 0x0800_0000;
pub const INITIAL_EXIT_ANGLE: u32 = 0x8000_0000;
pub const GATHER_EXIT_ANGLE: u32 = 0x5555_5555;
pub const FIRST_GUY_TURRET_INC_90_BITS: u32 = 0x42b4_0000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ObjectIdentity {
    pub owner: i8,
    pub object: i16,
}

impl ObjectIdentity {
    pub const fn new(owner: i8, object: i16) -> Self {
        Self { owner, object }
    }

    pub const fn valid(self) -> bool {
        self.owner >= 0 && self.object >= 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RngStamp {
    pub seed: u32,
    pub draws: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChildCallKind {
    CaptainComeOut,
    InitTransport,
    TransportComeOut,
    TransportDie,
}

/// Opaque receipt for a transitive child.  The prefix itself contains no direct
/// `Random::get` call; receipts retain any draws made below `Objects::init_unit`, recursive
/// `Unit::come_out`, or a virtual death override in their exact call order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChildCallReceipt {
    pub kind: ChildCallKind,
    pub result: i32,
    pub before: RngStamp,
    pub after: RngStamp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NearbyRequest {
    pub receiver_type: i32,
    pub centre: Point,
    pub min_radius: i32,
    pub max_radius: i32,
    pub radial_step: i32,
    pub base_angle: u32,
    pub filter: i32,
    pub actor: ObjectIdentity,
    pub accept_without_collision: i32,
    pub expanded: i32,
    pub overlap_object: i32,
    pub overlap_owner: i32,
    pub required_region: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NearbyOutcome {
    Found(Point),
    Blocked,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NearbyObservation {
    pub request: NearbyRequest,
    pub outcome: NearbyOutcome,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExitConstants {
    /// `Constants +0x84`.
    pub land_min: i32,
    /// `Constants +0x88`.
    pub land_max: i32,
    /// `Constants +0x8C`.
    pub water_min: i32,
    /// `Constants +0x90`.
    pub water_max: i32,
    /// `Constants +0x9C`.
    pub ordinary_padding: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GatherPointSource {
    Coordinate(Point),
    Object {
        identity: ObjectIdentity,
        point: Point,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GatherDirectionFacts {
    /// Retail only reaches the first list node when the list is non-empty and
    /// `BuildData::gather_inside()` returned zero.
    pub source: GatherPointSource,
    pub observation: NearbyObservation,
    /// Authoritative return of `find_angle(found - container)` at `0x0092D130` when the
    /// observation found a point. Keeping this host result explicit avoids substituting a
    /// floating-point or partial integer-trig approximation.
    pub angle_after: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GatherGateFacts {
    pub list_non_empty: bool,
    pub gather_inside: bool,
    pub direction: Option<GatherDirectionFacts>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContainerFacts {
    pub identity: ObjectIdentity,
    pub point: Point,
    pub angle: u32,
    pub matches_university: bool,
    pub matches_oil_platform: bool,
    pub is_build: bool,
    pub is_wallbuild: bool,
    pub gpiece: i32,
    /// Low object flag byte at `SubObjectData +8`; bit zero selects the non-zero wall
    /// minimum radius.
    pub object_flags_low: u8,
    pub type_block_radius: i32,
    pub type_x_size: i32,
    pub type_y_size: i32,
    pub gather: Option<GatherGateFacts>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OilPlatformBridgeFacts {
    /// Retail substitutes Transport Barge when `current_upgrade(0x140)` is negative.
    pub current_transport_upgrade: i32,
    pub transport: Option<ObjectIdentity>,
    pub init: ChildCallReceipt,
    pub recursive_come_out: Option<ChildCallReceipt>,
    pub failed_transport_die: Option<ChildCallReceipt>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InsideFacts {
    /// Result of the first `ObjectData::get_inside(&who)` call.
    pub direct_container: ContainerFacts,
    /// Container selected after `get_captain()` for a non-captain actor.  It must equal the
    /// direct container for a captain.
    pub placement_container: Option<ContainerFacts>,
    pub leader_flags_before: Option<u32>,
    pub oil_bridge: Option<OilPlatformBridgeFacts>,
    /// Ordered observations at the common container placement calls.  Zero, one, or two are
    /// consumed according to retail branch reachability.
    pub placement_searches: Vec<NearbyObservation>,
    /// Authoritative result of `TerrainOut::find_tcoord_z` for the selected point.
    pub terrain_z: Option<i32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnitComeOutPrefixFacts {
    pub actor: ObjectIdentity,
    pub argument: i32,
    pub point: Point,
    pub actor_type: i32,
    pub actor_domain: i32,
    pub actor_obj_masks: u32,
    pub actor_block_radius: i32,
    pub actor_big_radius: i32,
    pub actor_is_captain: bool,
    /// Exact result of virtual `get_captain()` when retail reaches it.
    pub captain_object: Option<i16>,
    pub guy_hint_lengths: Vec<i32>,
    pub first_guy_turret_inc_bits: Option<u32>,
    pub constants: ExitConstants,
    pub initial_rng: RngStamp,
    pub captain_recursive: Option<ChildCallReceipt>,
    pub inside: Option<InsideFacts>,
    pub uncontained_searches: Vec<NearbyObservation>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrefixStep {
    ClearScratchGroup {
        call_va: u32,
        argument: i32,
    },
    ClearGuyAnimHints {
        store_va: u32,
        guy_index: usize,
        before: i32,
    },
    CaptainRecursiveComeOut {
        call_va: u32,
        captain: ObjectIdentity,
        receipt: ChildCallReceipt,
    },
    NearbySearch {
        call_va: u32,
        observation: NearbyObservation,
    },
    InitTransport {
        call_va: u32,
        container: ObjectIdentity,
        type_index: i32,
        receipt: ChildCallReceipt,
    },
    SameDamage {
        call_va: u32,
        transport: ObjectIdentity,
        actor: ObjectIdentity,
    },
    InsertInside {
        call_va: u32,
        object: ObjectIdentity,
        container: ObjectIdentity,
    },
    RecursiveTransportComeOut {
        call_va: u32,
        transport: ObjectIdentity,
        receipt: ChildCallReceipt,
    },
    DieTransport {
        call_va: u32,
        transport: ObjectIdentity,
        receipt: ChildCallReceipt,
    },
    RemoveFromInside {
        call_va: u32,
        object: ObjectIdentity,
    },
    SetLeaderFlags {
        store_va: u32,
        owner: i8,
        before: u32,
        after: u32,
    },
    CopyGatherHotKey {
        call_va: u32,
        source: ObjectIdentity,
        actor: ObjectIdentity,
    },
    SetContainerAngle {
        store_va: u32,
        container: ObjectIdentity,
        before: u32,
        after: u32,
    },
    SetFirstGuyTurretIncrement {
        store_va: u32,
        before_bits: u32,
        after_bits: u32,
    },
    SetActorPoint {
        x_store_va: u32,
        y_store_va: u32,
        before: Point,
        after: Point,
    },
    SetActorZ {
        terrain_call_va: u32,
        store_va: u32,
        after: i32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnitComeOutContinuation {
    pub resume_va: u32,
    pub actor: ObjectIdentity,
    pub point: Point,
    pub z: Option<i32>,
    pub direct_container: Option<ObjectIdentity>,
    pub placement_container: Option<ObjectIdentity>,
    pub container_gpiece: i32,
    pub rng: RngStamp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnitComeOutPrefixExit {
    Returned { value: i32, rng: RngStamp },
    ContinueAtCommonRelease(UnitComeOutContinuation),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnitComeOutPrefixPlan {
    pub steps: Vec<PrefixStep>,
    pub exit: UnitComeOutPrefixExit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MissingFact {
    CaptainObject,
    CaptainReceipt,
    PlacementContainer,
    LeaderFlags,
    OilBridge,
    TransportComeOutReceipt,
    TransportDieReceipt,
    GatherDirection,
    GatherAngle,
    TerrainZ,
    FirstGuyTurretIncrement,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnitComeOutPrefixError {
    InvalidIdentity(ObjectIdentity),
    Missing(MissingFact),
    Unexpected(&'static str),
    ReceiptKind {
        expected: ChildCallKind,
        observed: ChildCallKind,
    },
    ReceiptContinuity,
    InvalidChildResult(i32),
    TransportOwnerMismatch {
        actor: ObjectIdentity,
        transport: ObjectIdentity,
    },
    SearchCount {
        expected_at_least: usize,
        observed: usize,
    },
    SearchRequestMismatch {
        expected: NearbyRequest,
        observed: NearbyRequest,
    },
    InvalidRadiusRange {
        min: i32,
        max: i32,
    },
    PlacementContainerMismatch {
        expected: ObjectIdentity,
        observed: ObjectIdentity,
    },
    PlacementContainerFactsMismatch,
}

fn validate_receipt(
    receipt: ChildCallReceipt,
    kind: ChildCallKind,
    before: RngStamp,
) -> Result<(), UnitComeOutPrefixError> {
    if receipt.kind != kind {
        return Err(UnitComeOutPrefixError::ReceiptKind {
            expected: kind,
            observed: receipt.kind,
        });
    }
    if receipt.before != before || receipt.after.draws < receipt.before.draws {
        return Err(UnitComeOutPrefixError::ReceiptContinuity);
    }
    if receipt.before.seed != receipt.after.seed && receipt.before.draws == receipt.after.draws {
        return Err(UnitComeOutPrefixError::ReceiptContinuity);
    }
    Ok(())
}

fn observe(
    observations: &[NearbyObservation],
    index: &mut usize,
    expected: NearbyRequest,
    call_va: u32,
    steps: &mut Vec<PrefixStep>,
) -> Result<NearbyOutcome, UnitComeOutPrefixError> {
    let Some(observation) = observations.get(*index).copied() else {
        return Err(UnitComeOutPrefixError::SearchCount {
            expected_at_least: *index + 1,
            observed: observations.len(),
        });
    };
    *index += 1;
    if observation.request != expected {
        return Err(UnitComeOutPrefixError::SearchRequestMismatch {
            expected,
            observed: observation.request,
        });
    }
    steps.push(PrefixStep::NearbySearch {
        call_va,
        observation,
    });
    Ok(observation.outcome)
}

fn request(
    facts: &UnitComeOutPrefixFacts,
    centre: Point,
    min_radius: i32,
    max_radius: i32,
    base_angle: u32,
    filter: i32,
    accept_without_collision: i32,
    expanded: i32,
) -> NearbyRequest {
    NearbyRequest {
        receiver_type: facts.actor_type,
        centre,
        min_radius,
        max_radius,
        radial_step: 0,
        base_angle,
        filter,
        actor: facts.actor,
        accept_without_collision,
        expanded,
        overlap_object: -1,
        overlap_owner: 0,
        required_region: -1,
    }
}

fn placement_radii(
    facts: &UnitComeOutPrefixFacts,
    container: ContainerFacts,
) -> Result<(i32, i32), UnitComeOutPrefixError> {
    let (min, max) = if container.is_wallbuild {
        let span = container
            .type_x_size
            .wrapping_add(container.type_y_size)
            .wrapping_mul(0x30);
        let inner = if facts.actor_domain == 1 {
            facts
                .constants
                .water_min
                .wrapping_add(span)
                .wrapping_add(facts.actor_big_radius)
        } else {
            facts.constants.land_min.wrapping_add(span)
        };
        let maximum = if facts.actor_domain == 1 {
            facts
                .constants
                .water_max
                .wrapping_sub(facts.constants.water_min)
                .wrapping_add(inner)
        } else {
            facts
                .constants
                .land_max
                .wrapping_sub(facts.constants.land_min)
                .wrapping_add(inner)
        };
        let minimum = if container.object_flags_low & 1 != 0 {
            inner
        } else {
            0
        };
        (minimum, maximum)
    } else {
        (
            container.type_block_radius,
            facts
                .constants
                .ordinary_padding
                .wrapping_add(container.type_block_radius),
        )
    };
    if min < 0 || max < min {
        return Err(UnitComeOutPrefixError::InvalidRadiusRange { min, max });
    }
    Ok((min, max))
}

fn no_unreached_payloads(
    facts: &UnitComeOutPrefixFacts,
    allow_captain_receipt: bool,
) -> Result<(), UnitComeOutPrefixError> {
    if !allow_captain_receipt && facts.captain_recursive.is_some() {
        return Err(UnitComeOutPrefixError::Unexpected(
            "captain recursive receipt on an unreached branch",
        ));
    }
    Ok(())
}

pub fn plan_unit_come_out_prefix(
    facts: &UnitComeOutPrefixFacts,
) -> Result<UnitComeOutPrefixPlan, UnitComeOutPrefixError> {
    if !facts.actor.valid() {
        return Err(UnitComeOutPrefixError::InvalidIdentity(facts.actor));
    }
    let mut steps = vec![PrefixStep::ClearScratchGroup {
        call_va: 0x0061_7c66,
        argument: -1,
    }];
    for (guy_index, before) in facts.guy_hint_lengths.iter().copied().enumerate() {
        steps.push(PrefixStep::ClearGuyAnimHints {
            store_va: 0x0061_7c8f,
            guy_index,
            before,
        });
    }

    if facts.argument == 0 && !facts.actor_is_captain {
        let captain = ObjectIdentity::new(
            facts.actor.owner,
            facts
                .captain_object
                .ok_or(UnitComeOutPrefixError::Missing(MissingFact::CaptainObject))?,
        );
        if !captain.valid() {
            return Err(UnitComeOutPrefixError::InvalidIdentity(captain));
        }
        let receipt = facts
            .captain_recursive
            .ok_or(UnitComeOutPrefixError::Missing(MissingFact::CaptainReceipt))?;
        validate_receipt(receipt, ChildCallKind::CaptainComeOut, facts.initial_rng)?;
        if !matches!(receipt.result, 0 | 1) {
            return Err(UnitComeOutPrefixError::InvalidChildResult(receipt.result));
        }
        if facts.inside.is_some() || !facts.uncontained_searches.is_empty() {
            return Err(UnitComeOutPrefixError::Unexpected(
                "placement payload after captain redirection",
            ));
        }
        steps.push(PrefixStep::CaptainRecursiveComeOut {
            call_va: 0x0061_7cf7,
            captain,
            receipt,
        });
        return Ok(UnitComeOutPrefixPlan {
            steps,
            exit: UnitComeOutPrefixExit::Returned {
                value: receipt.result,
                rng: receipt.after,
            },
        });
    }
    no_unreached_payloads(facts, false)?;

    let Some(inside) = facts.inside.as_ref() else {
        let min = facts.actor_block_radius;
        let max = facts.constants.ordinary_padding.wrapping_add(min);
        if min < 0 || max < min {
            return Err(UnitComeOutPrefixError::InvalidRadiusRange { min, max });
        }
        let first = request(facts, facts.point, min, max, INITIAL_EXIT_ANGLE, 3, 0, 0);
        let mut used = 0;
        let selected = match observe(
            &facts.uncontained_searches,
            &mut used,
            first,
            0x0061_7d82,
            &mut steps,
        )? {
            NearbyOutcome::Found(point) => point,
            NearbyOutcome::Blocked => {
                let second = NearbyRequest {
                    accept_without_collision: 1,
                    ..first
                };
                match observe(
                    &facts.uncontained_searches,
                    &mut used,
                    second,
                    0x0061_7dd1,
                    &mut steps,
                )? {
                    NearbyOutcome::Found(point) => point,
                    NearbyOutcome::Blocked => facts.point,
                }
            }
        };
        if used != facts.uncontained_searches.len() {
            return Err(UnitComeOutPrefixError::Unexpected(
                "extra uncontained search observation",
            ));
        }
        return Ok(UnitComeOutPrefixPlan {
            steps,
            exit: UnitComeOutPrefixExit::ContinueAtCommonRelease(UnitComeOutContinuation {
                resume_va: PREFIX_END_VA,
                actor: facts.actor,
                point: selected,
                z: None,
                direct_container: None,
                placement_container: None,
                // `local_9f0` is initialized to zero and is only replaced by the
                // placement-container `gpiece` call on the contained general branch.
                container_gpiece: 0,
                rng: facts.initial_rng,
            }),
        });
    };

    if !inside.direct_container.identity.valid() {
        return Err(UnitComeOutPrefixError::InvalidIdentity(
            inside.direct_container.identity,
        ));
    }
    if !facts.uncontained_searches.is_empty() {
        return Err(UnitComeOutPrefixError::Unexpected(
            "uncontained search observations while inside",
        ));
    }

    if inside.direct_container.matches_oil_platform && facts.actor_domain == 0 {
        if inside.placement_container.is_some()
            || inside.leader_flags_before.is_some()
            || !inside.placement_searches.is_empty()
            || inside.terrain_z.is_some()
            || facts.first_guy_turret_inc_bits.is_some()
        {
            return Err(UnitComeOutPrefixError::Unexpected(
                "general placement payload on Oil Platform bridge",
            ));
        }
        let bridge = inside
            .oil_bridge
            .as_ref()
            .ok_or(UnitComeOutPrefixError::Missing(MissingFact::OilBridge))?;
        validate_receipt(bridge.init, ChildCallKind::InitTransport, facts.initial_rng)?;
        let transport_type = if bridge.current_transport_upgrade < 0 {
            TRANSPORT_BARGE_TYPE
        } else {
            bridge.current_transport_upgrade
        };
        if bridge.init.result < 0 {
            if bridge.transport.is_some()
                || bridge.recursive_come_out.is_some()
                || bridge.failed_transport_die.is_some()
            {
                return Err(UnitComeOutPrefixError::Unexpected(
                    "transport payload after failed allocation",
                ));
            }
            steps.push(PrefixStep::InitTransport {
                call_va: 0x0061_7eed,
                container: inside.direct_container.identity,
                type_index: transport_type,
                receipt: bridge.init,
            });
            return Ok(UnitComeOutPrefixPlan {
                steps,
                exit: UnitComeOutPrefixExit::Returned {
                    value: 1,
                    rng: bridge.init.after,
                },
            });
        }
        let transport = bridge.transport.ok_or(UnitComeOutPrefixError::Unexpected(
            "successful init without transport identity",
        ))?;
        if !transport.valid() || i32::from(transport.object) != bridge.init.result {
            return Err(UnitComeOutPrefixError::InvalidIdentity(transport));
        }
        if transport.owner != facts.actor.owner {
            return Err(UnitComeOutPrefixError::TransportOwnerMismatch {
                actor: facts.actor,
                transport,
            });
        }
        steps.push(PrefixStep::InitTransport {
            call_va: 0x0061_7eed,
            container: inside.direct_container.identity,
            type_index: transport_type,
            receipt: bridge.init,
        });
        steps.push(PrefixStep::SameDamage {
            call_va: 0x0061_7f28,
            transport,
            actor: facts.actor,
        });
        steps.push(PrefixStep::InsertInside {
            call_va: 0x0061_7f41,
            object: transport,
            container: facts.actor,
        });
        let recursive = bridge
            .recursive_come_out
            .ok_or(UnitComeOutPrefixError::Missing(
                MissingFact::TransportComeOutReceipt,
            ))?;
        validate_receipt(
            recursive,
            ChildCallKind::TransportComeOut,
            bridge.init.after,
        )?;
        if !matches!(recursive.result, 0 | 1) {
            return Err(UnitComeOutPrefixError::InvalidChildResult(recursive.result));
        }
        steps.push(PrefixStep::RecursiveTransportComeOut {
            call_va: 0x0061_7f52,
            transport,
            receipt: recursive,
        });
        if recursive.result != 0 {
            let die = bridge
                .failed_transport_die
                .ok_or(UnitComeOutPrefixError::Missing(
                    MissingFact::TransportDieReceipt,
                ))?;
            validate_receipt(die, ChildCallKind::TransportDie, recursive.after)?;
            steps.push(PrefixStep::DieTransport {
                call_va: 0x0061_7f84,
                transport,
                receipt: die,
            });
            return Ok(UnitComeOutPrefixPlan {
                steps,
                exit: UnitComeOutPrefixExit::Returned {
                    value: 1,
                    rng: die.after,
                },
            });
        }
        if bridge.failed_transport_die.is_some() {
            return Err(UnitComeOutPrefixError::Unexpected(
                "death receipt after successful recursive come_out",
            ));
        }
        steps.push(PrefixStep::RemoveFromInside {
            call_va: 0x0061_7f99,
            object: facts.actor,
        });
        steps.push(PrefixStep::InsertInside {
            call_va: 0x0061_7fa8,
            object: facts.actor,
            container: transport,
        });
        return Ok(UnitComeOutPrefixPlan {
            steps,
            exit: UnitComeOutPrefixExit::Returned {
                value: 0,
                rng: recursive.after,
            },
        });
    }
    if inside.oil_bridge.is_some() {
        return Err(UnitComeOutPrefixError::Unexpected(
            "Oil Platform bridge on an unreached branch",
        ));
    }

    if facts.actor_obj_masks & DIRECT_CONTAINER_LOCATION_MASK != 0 {
        if inside.placement_container.is_some()
            || inside.leader_flags_before.is_some()
            || !inside.placement_searches.is_empty()
            || inside.terrain_z.is_none()
        {
            return Err(UnitComeOutPrefixError::Unexpected(
                "general placement payload on direct-container branch",
            ));
        }
        let before_bits =
            facts
                .first_guy_turret_inc_bits
                .ok_or(UnitComeOutPrefixError::Missing(
                    MissingFact::FirstGuyTurretIncrement,
                ))?;
        steps.push(PrefixStep::SetFirstGuyTurretIncrement {
            store_va: 0x0061_8003,
            before_bits,
            after_bits: FIRST_GUY_TURRET_INC_90_BITS,
        });
        let z = inside.terrain_z.unwrap();
        steps.push(PrefixStep::SetActorPoint {
            x_store_va: 0x0061_8667,
            y_store_va: 0x0061_8675,
            before: facts.point,
            after: inside.direct_container.point,
        });
        steps.push(PrefixStep::SetActorZ {
            terrain_call_va: 0x0061_8694,
            store_va: 0x0061_86a4,
            after: z,
        });
        steps.push(PrefixStep::RemoveFromInside {
            call_va: 0x0061_86a7,
            object: facts.actor,
        });
        return Ok(UnitComeOutPrefixPlan {
            steps,
            exit: UnitComeOutPrefixExit::ContinueAtCommonRelease(UnitComeOutContinuation {
                resume_va: PREFIX_END_VA,
                actor: facts.actor,
                point: inside.direct_container.point,
                z: Some(z),
                direct_container: Some(inside.direct_container.identity),
                placement_container: Some(inside.direct_container.identity),
                // The direct-location-mask arm skips the `gpiece` virtual call.
                container_gpiece: 0,
                rng: facts.initial_rng,
            }),
        });
    }
    if facts.first_guy_turret_inc_bits.is_some() {
        return Err(UnitComeOutPrefixError::Unexpected(
            "first-Guy turret payload on a non-direct branch",
        ));
    }

    let placement = inside
        .placement_container
        .ok_or(UnitComeOutPrefixError::Missing(
            MissingFact::PlacementContainer,
        ))?;
    if !placement.identity.valid() {
        return Err(UnitComeOutPrefixError::InvalidIdentity(placement.identity));
    }
    let expected_placement = if facts.actor_is_captain {
        inside.direct_container.identity
    } else {
        ObjectIdentity::new(
            facts.actor.owner,
            facts
                .captain_object
                .ok_or(UnitComeOutPrefixError::Missing(MissingFact::CaptainObject))?,
        )
    };
    if placement.identity != expected_placement {
        return Err(UnitComeOutPrefixError::PlacementContainerMismatch {
            expected: expected_placement,
            observed: placement.identity,
        });
    }
    if facts.actor_is_captain && placement != inside.direct_container {
        return Err(UnitComeOutPrefixError::PlacementContainerFactsMismatch);
    }

    if placement.is_build && (placement.matches_university || placement.matches_oil_platform) {
        let before = inside
            .leader_flags_before
            .ok_or(UnitComeOutPrefixError::Missing(MissingFact::LeaderFlags))?;
        steps.push(PrefixStep::SetLeaderFlags {
            store_va: 0x0061_80ea,
            owner: facts.actor.owner,
            before,
            after: before | LEADER_EXIT_FLAG,
        });
    } else if inside.leader_flags_before.is_some() {
        return Err(UnitComeOutPrefixError::Unexpected(
            "leader flags on an unreached branch",
        ));
    }

    let mut working_angle = if placement.is_wallbuild {
        INITIAL_EXIT_ANGLE
    } else {
        placement.angle
    };
    if let Some(gather) = placement.gather {
        let reached = placement.is_build && gather.list_non_empty && !gather.gather_inside;
        if reached {
            let direction = gather.direction.ok_or(UnitComeOutPrefixError::Missing(
                MissingFact::GatherDirection,
            ))?;
            let centre = match direction.source {
                GatherPointSource::Coordinate(point) => point,
                GatherPointSource::Object { identity, point } => {
                    if !identity.valid() {
                        return Err(UnitComeOutPrefixError::InvalidIdentity(identity));
                    }
                    if facts.actor_is_captain && identity.owner < 8 {
                        steps.push(PrefixStep::CopyGatherHotKey {
                            call_va: 0x0061_81f6,
                            source: identity,
                            actor: facts.actor,
                        });
                    }
                    point
                }
            };
            let expected = request(
                facts,
                centre,
                0,
                0x600,
                GATHER_EXIT_ANGLE,
                if facts.actor_block_radius == 0 { 0 } else { 3 },
                0,
                if facts.actor_block_radius == 0 { 1 } else { 0 },
            );
            if direction.observation.request != expected {
                return Err(UnitComeOutPrefixError::SearchRequestMismatch {
                    expected,
                    observed: direction.observation.request,
                });
            }
            steps.push(PrefixStep::NearbySearch {
                call_va: if facts.actor_block_radius == 0 {
                    0x0061_827f
                } else {
                    0x0061_8314
                },
                observation: direction.observation,
            });
            match direction.observation.outcome {
                NearbyOutcome::Found(_) => {
                    working_angle = direction
                        .angle_after
                        .ok_or(UnitComeOutPrefixError::Missing(MissingFact::GatherAngle))?;
                    steps.push(PrefixStep::SetContainerAngle {
                        store_va: if facts.actor_block_radius == 0 {
                            0x0061_82e3
                        } else {
                            0x0061_8374
                        },
                        container: placement.identity,
                        before: placement.angle,
                        after: working_angle,
                    });
                }
                NearbyOutcome::Blocked if direction.angle_after.is_some() => {
                    return Err(UnitComeOutPrefixError::Unexpected(
                        "gather angle after blocked direction probe",
                    ));
                }
                NearbyOutcome::Blocked => {}
            }
        } else if gather.direction.is_some() {
            return Err(UnitComeOutPrefixError::Unexpected(
                "gather direction on an unreached branch",
            ));
        }
    } else if placement.is_build {
        return Err(UnitComeOutPrefixError::Unexpected(
            "missing gather gate facts for build placement container",
        ));
    }

    let (min, max) = placement_radii(facts, placement)?;
    let filter = if facts.actor_block_radius == 0 { 0 } else { 3 };
    let first = request(
        facts,
        placement.point,
        min,
        max,
        working_angle,
        filter,
        0,
        0,
    );
    let mut used = 0;
    let first_va = if facts.actor_block_radius == 0 {
        0x0061_84ee
    } else {
        0x0061_85de
    };
    let selected = match observe(
        &inside.placement_searches,
        &mut used,
        first,
        first_va,
        &mut steps,
    )? {
        NearbyOutcome::Found(point) => Some(point),
        NearbyOutcome::Blocked => {
            let second = NearbyRequest {
                min_radius: if facts.actor_block_radius == 0 {
                    min.wrapping_mul(2)
                } else {
                    min
                },
                max_radius: if facts.actor_block_radius == 0 {
                    max.wrapping_mul(2)
                } else {
                    max
                },
                accept_without_collision: 1,
                ..first
            };
            let second_va = if facts.actor_block_radius == 0 {
                0x0061_855c
            } else {
                0x0061_8642
            };
            match observe(
                &inside.placement_searches,
                &mut used,
                second,
                second_va,
                &mut steps,
            )? {
                NearbyOutcome::Found(point) => Some(point),
                NearbyOutcome::Blocked if facts.actor_block_radius == 0 => Some(placement.point),
                NearbyOutcome::Blocked => None,
            }
        }
    };
    if used != inside.placement_searches.len() {
        return Err(UnitComeOutPrefixError::Unexpected(
            "extra contained placement search observation",
        ));
    }
    let Some(selected) = selected else {
        if inside.terrain_z.is_some() {
            return Err(UnitComeOutPrefixError::Unexpected(
                "terrain Z after failed contained placement",
            ));
        }
        return Ok(UnitComeOutPrefixPlan {
            steps,
            exit: UnitComeOutPrefixExit::Returned {
                value: 1,
                rng: facts.initial_rng,
            },
        });
    };
    let z = inside
        .terrain_z
        .ok_or(UnitComeOutPrefixError::Missing(MissingFact::TerrainZ))?;
    steps.push(PrefixStep::SetActorPoint {
        x_store_va: 0x0061_8667,
        y_store_va: 0x0061_8675,
        before: facts.point,
        after: selected,
    });
    steps.push(PrefixStep::SetActorZ {
        terrain_call_va: 0x0061_8694,
        store_va: 0x0061_86a4,
        after: z,
    });
    steps.push(PrefixStep::RemoveFromInside {
        call_va: 0x0061_86a7,
        object: facts.actor,
    });
    Ok(UnitComeOutPrefixPlan {
        steps,
        exit: UnitComeOutPrefixExit::ContinueAtCommonRelease(UnitComeOutContinuation {
            resume_va: PREFIX_END_VA,
            actor: facts.actor,
            point: selected,
            z: Some(z),
            direct_container: Some(inside.direct_container.identity),
            placement_container: Some(placement.identity),
            container_gpiece: placement.gpiece,
            rng: facts.initial_rng,
        }),
    })
}
