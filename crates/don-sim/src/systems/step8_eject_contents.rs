//! Fixed step-8 `Wall::update_hits` edge into `Object::eject_contents`.
//!
//! Retail calls `eject_contents(1, -1, 0, 1)` at `0x0063F653`.  Those arguments and
//! the Wall/Build carrier eliminate the selector, non-reset, non-killing and
//! carrier-Unit subtrees.  This source-only module freezes the remaining body as a
//! transaction plan; it is deliberately not exported or connected to the scheduler.
//!
//! `Unit::come_out` and `Object::die` remain separate, larger retail bodies.  A host
//! must therefore provide receipt-bound outcomes with continuous RNG/checksum stamps.
//! Missing or stale state is rejected before [`Step8EjectHost::commit`] is called.

/// `Object::eject_contents(int,int,int,int)`.
pub const RETAIL_EJECT_CONTENTS_VA: u32 = 0x0064_CD20;
/// PDB procedure size (`object.cpp:3461-3663`).
pub const RETAIL_EJECT_CONTENTS_SIZE: u32 = 0x0B92;
/// The direct call instruction in `Wall::update_hits`.
pub const RETAIL_STEP8_CALL_VA: u32 = 0x0063_F653;
/// `DomainIndex::AIR`, tested both by the caller and the deferred arm.
pub const DOMAIN_AIR: i32 = 2;
/// `WallData::build_masks +0x60` bit consumed later by `Build::process_ejection`.
pub const BUILD_EJECT_PENDING: u16 = 0x4000;
/// Passenger `UnitData::unit_masks +0x68` bit cleared before `come_out`.
pub const UNIT_RESET_MASK: u32 = 0x0400_0000;
/// `TypeIndex::AIRBASE`.
pub const AIRBASE_TYPE: i32 = 0x01BF;
/// Number of deterministic `Game::check_all` channels carried by a receipt stamp.
pub const CHECK_CHANNELS: usize = 15;

/// Stable object identity used by the owner-local object arrays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObjectKey {
    pub who: i8,
    pub object: i16,
    pub uid: u16,
}

/// RNG and channel state at one transaction boundary.
///
/// `rng_draws` makes a one-draw `Unit::come_out` tail distinguishable even if a seed
/// happens to repeat.  `channels` are the exact ordered retail check channels, not an
/// invented aggregate hash.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeterminismStamp {
    pub rng_seed: u32,
    pub rng_draws: u64,
    pub channels: [u32; CHECK_CHANNELS],
}

/// Result of retail `Unit::come_out(0)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ComeOutResult {
    /// Retail returned zero and removed the passenger from this carrier.
    Released,
    /// Retail returned non-zero; fixed `param1=1` makes ejection kill it.
    Blocked,
}

/// Exact specialist action selected after a successful release.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpecialistRoute {
    /// No militia conversion and no city was found (or the passenger is not 0x32..0x35).
    None,
    /// Citizen 0x32/0x33 passed the militia-tech gate and was converted.
    MilitiaUpgrade { type_index: i32 },
    /// The city lookup succeeded and emitted the fixed `add_move_order` call.
    MoveToCity { city: ObjectKey, x: i32, y: i32 },
}

/// Observable effect bound into one authoritative receipt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReceiptEffect {
    /// Clear mask, zero path count, close orders, clear partial path, update action,
    /// then call `Unit::come_out(0)`, in that order.
    ResetAndComeOut {
        result: ComeOutResult,
        carrier_head_after: Option<ObjectKey>,
    },
    /// Successful-release tail: specialist routing first, Airbase strafe second.
    ReleasedTail {
        route: SpecialistRoute,
        airbase_strafe: bool,
    },
    /// Fixed failed-release call `die(0, -1, 0.0)`.
    DieFailedRelease {
        carrier_head_after: Option<ObjectKey>,
    },
}

/// Authoritative proof for a still-external retail child/effect.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EffectReceipt {
    /// Non-zero host-issued token; prevents a default struct from authorizing writes.
    pub token: u64,
    pub carrier: ObjectKey,
    pub passenger: ObjectKey,
    pub effect: ReceiptEffect,
    pub before: DeterminismStamp,
    pub after: DeterminismStamp,
}

/// Canonical carrier facts read by the fixed step-8 edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WallCarrierSnapshot {
    pub key: ObjectKey,
    /// Virtual `ObjectData::is_on_map` (`vtable +0xBC`).
    pub on_map: bool,
    /// Must be true for the step-8 Wall/Build caller.
    pub is_build: bool,
    /// Caller reached the edge only after `can_carry(DOMAIN_AIR)==0`.
    pub can_carry_air: bool,
    /// `Game +0x821 & 0x08`: false defers; true executes immediately.
    pub immediate_ejection: bool,
    /// `WallData::build_masks +0x60`.
    pub build_masks: u16,
    /// Current `inside_down +0x28/+0x3E`; the caller requires a live head.
    pub inside_head: Option<ObjectKey>,
    /// Result of `carrier.is(AIRBASE_TYPE, 0)` for the successful tail.
    pub is_airbase: bool,
}

/// One passenger observed at a repeated read of the carrier's current head.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PassengerSnapshot {
    pub key: ObjectKey,
    pub type_index: i32,
    /// `UnitData::unit_masks +0x68` before the fixed cleanup.
    pub unit_masks: u32,
    /// `UnitData::path +0xC0` before the fixed cleanup.
    pub path_count: i32,
    pub result: ComeOutResult,
    pub head_after_come_out: Option<ObjectKey>,
    pub come_out: EffectReceipt,
    /// Required on success and forbidden on failure.
    pub released_tail: Option<EffectReceipt>,
    /// Required on failure and forbidden on success.
    pub failed_die: Option<EffectReceipt>,
}

/// Coherent canonical snapshot used for pure preflight.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WallEjectSnapshot {
    /// Owner revision checked again by the atomic commit adapter.
    pub revision: u64,
    pub initial_stamp: DeterminismStamp,
    pub carrier: WallCarrierSnapshot,
    /// Retail order: one row for every repeated carrier-head observation.
    pub passengers: Vec<PassengerSnapshot>,
}

/// Exact fixed cleanup and child call for one passenger.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResetAndComeOutOp {
    pub passenger: ObjectKey,
    pub expected_unit_masks: u32,
    pub unit_masks_after: u32,
    pub expected_path_count: i32,
    pub path_count_after: i32,
    pub close_orders_arg: i32,
    pub come_out_arg: i32,
    pub receipt: EffectReceipt,
}

/// Ordered immediate-mode operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImmediateOp {
    ResetAndComeOut(ResetAndComeOutOp),
    ReleasedTail {
        passenger: ObjectKey,
        route: SpecialistRoute,
        airbase_strafe: bool,
        receipt: EffectReceipt,
    },
    KillFailedRelease {
        passenger: ObjectKey,
        /// Frozen retail call arguments.
        die_arg_1: i32,
        die_arg_2: i32,
        die_arg_3_bits: u32,
        receipt: EffectReceipt,
    },
}

/// Complete plan for the fixed step-8 edge.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WallEjectPlan {
    OffMapNoOp {
        revision: u64,
        carrier: ObjectKey,
    },
    DeferredMask {
        revision: u64,
        carrier: ObjectKey,
        before: u16,
        after: u16,
    },
    Drained {
        revision: u64,
        carrier: ObjectKey,
        initial_stamp: DeterminismStamp,
        final_stamp: DeterminismStamp,
        ops: Vec<ImmediateOp>,
    },
}

/// Fail-closed preflight errors.  None is a retail return value: the retail function is
/// `void`, while these errors prevent an incomplete headless adapter from writing state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WallEjectPlanError {
    InvalidObjectKey(ObjectKey),
    NotBuildCarrier,
    CallerCanCarryAir,
    CallerHasNoContents,
    InvalidPassengerType(i32),
    ImmediatePassengerListEmpty,
    WrongObservedHead {
        expected: Option<ObjectKey>,
        observed: ObjectKey,
    },
    RepeatedPassenger(ObjectKey),
    NoProgress(ObjectKey),
    DrainDidNotFinish(ObjectKey),
    MissingReleasedTail(ObjectKey),
    UnexpectedReleasedTail(ObjectKey),
    MissingFailedDie(ObjectKey),
    UnexpectedFailedDie(ObjectKey),
    InvalidSpecialistRoute {
        passenger_type: i32,
        route: SpecialistRoute,
    },
    AirbaseMismatch {
        expected: bool,
        observed: bool,
    },
    EmptyReceiptToken(ObjectKey),
    ReceiptScopeMismatch(ObjectKey),
    ReceiptEffectMismatch(ObjectKey),
    ReceiptContinuityMismatch(ObjectKey),
    ReceiptRngRegression(ObjectKey),
    ReceiptSeedChangedWithoutDraw(ObjectKey),
    TooManyComeOutDraws {
        passenger: ObjectKey,
        draws: u64,
    },
    ReleasedTailConsumedRng {
        passenger: ObjectKey,
        draws: u64,
    },
}

fn validate_receipt(
    receipt: EffectReceipt,
    carrier: ObjectKey,
    passenger: ObjectKey,
    effect: ReceiptEffect,
    cursor: &mut DeterminismStamp,
) -> Result<(), WallEjectPlanError> {
    if receipt.token == 0 {
        return Err(WallEjectPlanError::EmptyReceiptToken(passenger));
    }
    if receipt.carrier != carrier || receipt.passenger != passenger {
        return Err(WallEjectPlanError::ReceiptScopeMismatch(passenger));
    }
    if receipt.effect != effect {
        return Err(WallEjectPlanError::ReceiptEffectMismatch(passenger));
    }
    if receipt.before != *cursor {
        return Err(WallEjectPlanError::ReceiptContinuityMismatch(passenger));
    }
    let Some(draws) = receipt
        .after
        .rng_draws
        .checked_sub(receipt.before.rng_draws)
    else {
        return Err(WallEjectPlanError::ReceiptRngRegression(passenger));
    };
    if draws == 0 && receipt.after.rng_seed != receipt.before.rng_seed {
        return Err(WallEjectPlanError::ReceiptSeedChangedWithoutDraw(passenger));
    }
    match effect {
        ReceiptEffect::ResetAndComeOut { .. } if draws > 1 => {
            return Err(WallEjectPlanError::TooManyComeOutDraws { passenger, draws });
        }
        ReceiptEffect::ReleasedTail { .. } if draws != 0 => {
            return Err(WallEjectPlanError::ReleasedTailConsumedRng { passenger, draws });
        }
        _ => {}
    }
    *cursor = receipt.after;
    Ok(())
}

fn valid_route(type_index: i32, route: SpecialistRoute) -> bool {
    match type_index {
        // Citizens may upgrade, fall through to a city, or find neither.
        0x32 | 0x33 => match route {
            SpecialistRoute::None => true,
            SpecialistRoute::MilitiaUpgrade { type_index } => (50..414).contains(&type_index),
            SpecialistRoute::MoveToCity { city, .. } => valid_object_key(city),
        },
        // Scholars share the city arm but never the militia-conversion arm.
        0x34 | 0x35 => match route {
            SpecialistRoute::None => true,
            SpecialistRoute::MoveToCity { city, .. } => valid_object_key(city),
            SpecialistRoute::MilitiaUpgrade { .. } => false,
        },
        // All other types bypass this branch completely.
        _ => matches!(route, SpecialistRoute::None),
    }
}

fn valid_object_key(key: ObjectKey) -> bool {
    (0..=8).contains(&key.who) && key.object >= 0
}

/// Purely preflight the retail-reached `eject_contents(1,-1,0,1)` body.
pub fn plan_step8_wall_eject(
    snapshot: &WallEjectSnapshot,
) -> Result<WallEjectPlan, WallEjectPlanError> {
    let carrier = snapshot.carrier;
    if !valid_object_key(carrier.key) {
        return Err(WallEjectPlanError::InvalidObjectKey(carrier.key));
    }
    if !carrier.is_build {
        return Err(WallEjectPlanError::NotBuildCarrier);
    }
    if carrier.can_carry_air {
        return Err(WallEjectPlanError::CallerCanCarryAir);
    }
    let Some(mut current_head) = carrier.inside_head else {
        return Err(WallEjectPlanError::CallerHasNoContents);
    };
    if !valid_object_key(current_head) {
        return Err(WallEjectPlanError::InvalidObjectKey(current_head));
    }

    if !carrier.on_map {
        return Ok(WallEjectPlan::OffMapNoOp {
            revision: snapshot.revision,
            carrier: carrier.key,
        });
    }

    if !carrier.immediate_ejection {
        return Ok(WallEjectPlan::DeferredMask {
            revision: snapshot.revision,
            carrier: carrier.key,
            before: carrier.build_masks,
            after: carrier.build_masks | BUILD_EJECT_PENDING,
        });
    }

    if snapshot.passengers.is_empty() {
        return Err(WallEjectPlanError::ImmediatePassengerListEmpty);
    }

    let mut cursor = snapshot.initial_stamp;
    let mut visited = Vec::with_capacity(snapshot.passengers.len());
    let mut ops = Vec::with_capacity(snapshot.passengers.len() * 2);
    for (passenger_index, passenger) in snapshot.passengers.iter().enumerate() {
        if !valid_object_key(passenger.key) {
            return Err(WallEjectPlanError::InvalidObjectKey(passenger.key));
        }
        if !(50..414).contains(&passenger.type_index) {
            return Err(WallEjectPlanError::InvalidPassengerType(
                passenger.type_index,
            ));
        }
        if passenger.key != current_head {
            return Err(WallEjectPlanError::WrongObservedHead {
                expected: Some(current_head),
                observed: passenger.key,
            });
        }
        if visited.contains(&passenger.key) {
            return Err(WallEjectPlanError::RepeatedPassenger(passenger.key));
        }
        visited.push(passenger.key);

        let come_out_effect = ReceiptEffect::ResetAndComeOut {
            result: passenger.result,
            carrier_head_after: passenger.head_after_come_out,
        };
        validate_receipt(
            passenger.come_out,
            carrier.key,
            passenger.key,
            come_out_effect,
            &mut cursor,
        )?;
        ops.push(ImmediateOp::ResetAndComeOut(ResetAndComeOutOp {
            passenger: passenger.key,
            expected_unit_masks: passenger.unit_masks,
            unit_masks_after: passenger.unit_masks & !UNIT_RESET_MASK,
            expected_path_count: passenger.path_count,
            path_count_after: 0,
            close_orders_arg: 0,
            come_out_arg: 0,
            receipt: passenger.come_out,
        }));

        let next_head = match passenger.result {
            ComeOutResult::Released => {
                if passenger.head_after_come_out == Some(passenger.key) {
                    return Err(WallEjectPlanError::NoProgress(passenger.key));
                }
                if passenger.failed_die.is_some() {
                    return Err(WallEjectPlanError::UnexpectedFailedDie(passenger.key));
                }
                let Some(tail) = passenger.released_tail else {
                    return Err(WallEjectPlanError::MissingReleasedTail(passenger.key));
                };
                let ReceiptEffect::ReleasedTail {
                    route,
                    airbase_strafe,
                } = tail.effect
                else {
                    return Err(WallEjectPlanError::ReceiptEffectMismatch(passenger.key));
                };
                if !valid_route(passenger.type_index, route) {
                    return Err(WallEjectPlanError::InvalidSpecialistRoute {
                        passenger_type: passenger.type_index,
                        route,
                    });
                }
                if airbase_strafe != carrier.is_airbase {
                    return Err(WallEjectPlanError::AirbaseMismatch {
                        expected: carrier.is_airbase,
                        observed: airbase_strafe,
                    });
                }
                validate_receipt(
                    tail,
                    carrier.key,
                    passenger.key,
                    ReceiptEffect::ReleasedTail {
                        route,
                        airbase_strafe,
                    },
                    &mut cursor,
                )?;
                ops.push(ImmediateOp::ReleasedTail {
                    passenger: passenger.key,
                    route,
                    airbase_strafe,
                    receipt: tail,
                });
                passenger.head_after_come_out
            }
            ComeOutResult::Blocked => {
                if passenger.released_tail.is_some() {
                    return Err(WallEjectPlanError::UnexpectedReleasedTail(passenger.key));
                }
                // A failed come_out leaves this child at the head.  The forced die must
                // be the operation that advances or empties the chain.
                if passenger.head_after_come_out != Some(passenger.key) {
                    return Err(WallEjectPlanError::ReceiptEffectMismatch(passenger.key));
                }
                let Some(die) = passenger.failed_die else {
                    return Err(WallEjectPlanError::MissingFailedDie(passenger.key));
                };
                let ReceiptEffect::DieFailedRelease { carrier_head_after } = die.effect else {
                    return Err(WallEjectPlanError::ReceiptEffectMismatch(passenger.key));
                };
                if carrier_head_after == Some(passenger.key) {
                    return Err(WallEjectPlanError::NoProgress(passenger.key));
                }
                validate_receipt(
                    die,
                    carrier.key,
                    passenger.key,
                    ReceiptEffect::DieFailedRelease { carrier_head_after },
                    &mut cursor,
                )?;
                ops.push(ImmediateOp::KillFailedRelease {
                    passenger: passenger.key,
                    die_arg_1: 0,
                    die_arg_2: -1,
                    die_arg_3_bits: 0.0f32.to_bits(),
                    receipt: die,
                });
                carrier_head_after
            }
        };

        match next_head {
            Some(next) => {
                if !valid_object_key(next) {
                    return Err(WallEjectPlanError::InvalidObjectKey(next));
                }
                current_head = next;
            }
            None => {
                if passenger_index + 1 != snapshot.passengers.len() {
                    return Err(WallEjectPlanError::WrongObservedHead {
                        expected: None,
                        observed: snapshot.passengers[passenger_index + 1].key,
                    });
                }
                return Ok(WallEjectPlan::Drained {
                    revision: snapshot.revision,
                    carrier: carrier.key,
                    initial_stamp: snapshot.initial_stamp,
                    final_stamp: cursor,
                    ops,
                });
            }
        }
    }

    Err(WallEjectPlanError::DrainDidNotFinish(current_head))
}

/// Canonical owner seam.  `commit` must re-check `revision`, all object identities and
/// receipt tokens, then apply the complete plan atomically or make no writes.
pub trait Step8EjectHost {
    type Error;

    fn snapshot(&mut self, carrier: ObjectKey) -> Result<WallEjectSnapshot, Self::Error>;
    fn commit(&mut self, plan: &WallEjectPlan) -> Result<(), Self::Error>;
}

#[derive(Debug, PartialEq, Eq)]
pub enum WallEjectExecuteError<E> {
    Host(E),
    Plan(WallEjectPlanError),
}

/// Snapshot, preflight and atomically commit one fixed step-8 ejection.
///
/// Off-map is a true retail no-op and does not call the mutation adapter.
pub fn execute_step8_wall_eject<H: Step8EjectHost>(
    host: &mut H,
    carrier: ObjectKey,
) -> Result<WallEjectPlan, WallEjectExecuteError<H::Error>> {
    let snapshot = host
        .snapshot(carrier)
        .map_err(WallEjectExecuteError::Host)?;
    let plan = plan_step8_wall_eject(&snapshot).map_err(WallEjectExecuteError::Plan)?;
    if !matches!(&plan, WallEjectPlan::OffMapNoOp { .. }) {
        host.commit(&plan).map_err(WallEjectExecuteError::Host)?;
    }
    Ok(plan)
}
