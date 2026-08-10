//! Third source-only tranche of retail `Unit::come_out(int)`.
//!
//! This planner consumes the common-release build seam at `0x00618B22` and owns the
//! complete gather-point selection loop through `0x006191A4`, plus the two compiler-
//! outlined virtual calls used by the loop. It emits an atomic plan ending immediately
//! before the selected/fallback point is consumed at `0x006191A5`.

pub const GATHER_SELECTION_START_VA: u32 = 0x0061_8b22;
pub const GATHER_SELECTION_END_VA: u32 = 0x0061_91a5;
pub const SEQUENTIAL_BYTES: u32 = GATHER_SELECTION_END_VA - GATHER_SELECTION_START_VA;
pub const OUTLINED_VIRTUAL_ISLANDS: [(u32, u32); 2] =
    [(0x0061_a271, 0x0061_a278), (0x0061_a278, 0x0061_a281)];
pub const OUTLINED_BYTES: u32 = 7 + 9;
pub const LOGICAL_TRANCHE_BYTES: u32 = SEQUENTIAL_BYTES + OUTLINED_BYTES;
pub const PRIOR_RESIDUAL_BYTES: u32 = 6_032;
pub const RESIDUAL_BYTES_AFTER_TRANCHE: u32 = PRIOR_RESIDUAL_BYTES - LOGICAL_TRANCHE_BYTES;

pub const GATHER_PROBE_RADIUS: i32 = 0x600;
pub const GATHER_PROBE_ANGLE: u32 = 0x5555_5555;
pub const GARRISON_ACTION: u8 = 1;
pub const ATTACK_ACTION: u8 = 2;
pub const UNIVERSITY_TYPE: i32 = 0x1a4;

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
pub struct WideObjectIdentity {
    pub owner: i32,
    pub object: i32,
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

/// Structural copy of the fields supplied by the landed common-release frontier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GatherSelectionInput {
    pub resume_va: u32,
    pub actor: ObjectIdentity,
    pub actor_point: Point,
    pub direct_container: ObjectIdentity,
    pub container_gpiece: i32,
    pub scratch_group: Option<i32>,
    pub rng: RngStamp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActorSelectionFacts {
    pub attack: i32,
    pub uber_size: i32,
    /// Base `UnitData::is_captain` is folded to the high-bit field test. An
    /// override is called and represented by a receipt instead.
    pub is_captain_base_slot: bool,
    pub captain_bit: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildingDirectFacts {
    pub identity: WideObjectIdentity,
    pub myhits: i32,
    pub type_index: i32,
    pub gather_max: i8,
    /// Whether vtable slot `+0xB8` is the base `ObjectData::is` fast target.
    pub university_is_base_slot: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GatherPointFacts {
    pub point: Point,
    pub action: u8,
    /// Required only after a successful positive `find_any_building_at` result.
    pub building: Option<BuildingDirectFacts>,
    /// State of `BuildData::gather.head_node` when retail advances the list cursor
    /// after this point. Ignored, and required false, on a terminal point.
    pub advance_head_present: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GatherSelectionFacts {
    pub input: GatherSelectionInput,
    pub actor: ActorSelectionFacts,
    /// Direct read of `BuildData::gather.head_node` at `0x00618B47`.
    pub has_gather_head: bool,
    /// Observed list entries. This is empty on an early gate, otherwise it must
    /// exactly match `BuildData::num_gather()`.
    pub points: Vec<GatherPointFacts>,
    pub host_receipts: Vec<HostCallReceipt>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostCallKind {
    GatherInside,
    ActorOrderType,
    NumGather,
    GatherHead,
    TypeFindNearbySpot { point: usize },
    FindAnyBuildingAt { point: usize },
    ActorIsPeasant { point: usize },
    CandidateIsWallBuild { point: usize },
    CandidateIsActive { point: usize },
    CandidateIsValidWall { point: usize },
    CandidateWallTypeIsGatherType { point: usize },
    CandidateMatchesUniversity { point: usize, base_slot: bool },
    CandidateNumGatherers { point: usize },
    CandidateIsActiveBuild { point: usize },
    CandidateOwnerIsAlly { point: usize },
    CandidateGarrisonLimitGate { point: usize },
    ActorCanGarrison { point: usize, type_index: i32 },
    CandidateGarrisonLimitCapacity { point: usize },
    CandidateNumInside { point: usize },
    ActorControlCost { point: usize },
    ActorIsWorker { point: usize, building_found: bool },
    CandidateOwnerIsEnemy { point: usize },
    ActorIsCaptain { point: usize },
    ActorFindNearbySpot { point: usize },
    FindMoveAngle { point: usize, group: bool },
    ScratchGroupMove { point: usize },
    ActorAddMoveFacingOrder { point: usize },
}

impl HostCallKind {
    pub const fn call_va(self) -> u32 {
        match self {
            Self::GatherInside => 0x0061_8b7a,
            Self::ActorOrderType => 0x0061_8b8d,
            Self::NumGather => 0x0061_8bb8,
            Self::GatherHead => 0x0061_8be4,
            Self::TypeFindNearbySpot { .. } => 0x0061_8c74,
            Self::FindAnyBuildingAt { .. } => 0x0061_8cdb,
            Self::ActorIsPeasant { .. } => 0x0061_8d04,
            Self::CandidateIsWallBuild { .. } => 0x0061_8d34,
            Self::CandidateIsActive { .. } => 0x0061_8d61,
            Self::CandidateIsValidWall { .. } => 0x0061_8da6,
            Self::CandidateWallTypeIsGatherType { .. } => 0x0061_8dce,
            Self::CandidateMatchesUniversity {
                base_slot: true, ..
            } => 0x0061_8e15,
            Self::CandidateMatchesUniversity {
                base_slot: false, ..
            } => 0x0061_a271,
            Self::CandidateNumGatherers { .. } => 0x0061_8e57,
            Self::CandidateIsActiveBuild { .. } => 0x0061_8e91,
            Self::CandidateOwnerIsAlly { .. } => 0x0061_8eb3,
            Self::CandidateGarrisonLimitGate { .. } => 0x0061_8ee5,
            Self::ActorCanGarrison { .. } => 0x0061_8f13,
            Self::CandidateGarrisonLimitCapacity { .. } => 0x0061_8f4f,
            Self::CandidateNumInside { .. } => 0x0061_8f69,
            Self::ActorControlCost { .. } => 0x0061_8f76,
            Self::ActorIsWorker {
                building_found: true,
                ..
            } => 0x0061_8fa9,
            Self::ActorIsWorker {
                building_found: false,
                ..
            } => 0x0061_901e,
            Self::CandidateOwnerIsEnemy { .. } => 0x0061_8fc6,
            Self::ActorIsCaptain { .. } => 0x0061_a27a,
            Self::ActorFindNearbySpot { .. } => 0x0061_9091,
            Self::FindMoveAngle { group: true, .. } => 0x0061_90b4,
            Self::FindMoveAngle { group: false, .. } => 0x0061_910d,
            Self::ScratchGroupMove { .. } => 0x0061_90ce,
            Self::ActorAddMoveFacingOrder { .. } => 0x0061_9129,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostReturn {
    VoidOrIgnored,
    I32(i32),
    Search { code: i32, point: Point },
    Building(WideObjectIdentity),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostCallReceipt {
    pub kind: HostCallKind,
    pub result: HostReturn,
    pub before: RngStamp,
    pub after: RngStamp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GatherSelectionStep {
    HostCall {
        call_va: u32,
        receipt: HostCallReceipt,
    },
    AdvanceGatherCursor {
        point: usize,
        current_node_store_va: u32,
        current_data_store_va: u32,
        current_metric_store_va: u32,
    },
    SetMovementBaseline {
        point: usize,
        x_store_vas: [u32; 2],
        y_store_vas: [u32; 2],
        before: Point,
        after: Point,
    },
    SelectTerminalPoint {
        point: usize,
        flag_store_va: u32,
        action_store_va: u32,
        selected: Point,
        action: u8,
    },
    SetExhaustedFallback {
        flag_store_va: u32,
        x_move_va: u32,
        y_move_va: u32,
        selected: Point,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GatherSelectionContinuation {
    pub resume_va: u32,
    pub input: GatherSelectionInput,
    pub selected: Point,
    pub action: u8,
    pub terminal_selection: bool,
    pub rng: RngStamp,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GatherSelectionPlan {
    pub steps: Vec<GatherSelectionStep>,
    pub continuation: GatherSelectionContinuation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GatherSelectionError {
    ResumeVa {
        expected: u32,
        observed: u32,
    },
    InvalidIdentity(ObjectIdentity),
    Unexpected(&'static str),
    Missing(&'static str),
    PointCount {
        expected: usize,
        observed: usize,
    },
    BuildingMismatch {
        expected: WideObjectIdentity,
        observed: WideObjectIdentity,
    },
    ReceiptCount {
        consumed: usize,
        observed: usize,
    },
    ReceiptKind {
        expected: HostCallKind,
        observed: HostCallKind,
    },
    ReceiptContinuity,
    ReceiptReturn {
        kind: HostCallKind,
        observed: HostReturn,
    },
}

struct ReceiptCursor<'a> {
    receipts: &'a [HostCallReceipt],
    index: usize,
    rng: RngStamp,
}

impl<'a> ReceiptCursor<'a> {
    fn new(receipts: &'a [HostCallReceipt], rng: RngStamp) -> Self {
        Self {
            receipts,
            index: 0,
            rng,
        }
    }

    fn take(
        &mut self,
        expected: HostCallKind,
        steps: &mut Vec<GatherSelectionStep>,
    ) -> Result<HostReturn, GatherSelectionError> {
        let Some(receipt) = self.receipts.get(self.index).copied() else {
            return Err(GatherSelectionError::ReceiptCount {
                consumed: self.index + 1,
                observed: self.receipts.len(),
            });
        };
        if receipt.kind != expected {
            return Err(GatherSelectionError::ReceiptKind {
                expected,
                observed: receipt.kind,
            });
        }
        if receipt.before != self.rng
            || receipt.after.draws < receipt.before.draws
            || (receipt.before.seed != receipt.after.seed
                && receipt.before.draws == receipt.after.draws)
        {
            return Err(GatherSelectionError::ReceiptContinuity);
        }
        self.index += 1;
        self.rng = receipt.after;
        steps.push(GatherSelectionStep::HostCall {
            call_va: expected.call_va(),
            receipt,
        });
        Ok(receipt.result)
    }

    fn i32(
        &mut self,
        kind: HostCallKind,
        steps: &mut Vec<GatherSelectionStep>,
    ) -> Result<i32, GatherSelectionError> {
        let observed = self.take(kind, steps)?;
        let HostReturn::I32(value) = observed else {
            return Err(GatherSelectionError::ReceiptReturn { kind, observed });
        };
        Ok(value)
    }

    fn void(
        &mut self,
        kind: HostCallKind,
        steps: &mut Vec<GatherSelectionStep>,
    ) -> Result<(), GatherSelectionError> {
        let observed = self.take(kind, steps)?;
        if observed != HostReturn::VoidOrIgnored {
            return Err(GatherSelectionError::ReceiptReturn { kind, observed });
        }
        Ok(())
    }

    fn search(
        &mut self,
        kind: HostCallKind,
        steps: &mut Vec<GatherSelectionStep>,
    ) -> Result<(i32, Point), GatherSelectionError> {
        let observed = self.take(kind, steps)?;
        let HostReturn::Search { code, point } = observed else {
            return Err(GatherSelectionError::ReceiptReturn { kind, observed });
        };
        Ok((code, point))
    }

    fn building(
        &mut self,
        kind: HostCallKind,
        steps: &mut Vec<GatherSelectionStep>,
    ) -> Result<WideObjectIdentity, GatherSelectionError> {
        let observed = self.take(kind, steps)?;
        let HostReturn::Building(identity) = observed else {
            return Err(GatherSelectionError::ReceiptReturn { kind, observed });
        };
        Ok(identity)
    }

    fn finish(self) -> Result<RngStamp, GatherSelectionError> {
        if self.index != self.receipts.len() {
            return Err(GatherSelectionError::ReceiptCount {
                consumed: self.index,
                observed: self.receipts.len(),
            });
        }
        Ok(self.rng)
    }
}

fn truth(value: i32) -> bool {
    value != 0
}

fn no_building_payload(point: &GatherPointFacts) -> Result<(), GatherSelectionError> {
    if point.building.is_some() {
        return Err(GatherSelectionError::Unexpected(
            "building payload on an unreached building lookup",
        ));
    }
    Ok(())
}

fn fallback_plan(
    facts: &GatherSelectionFacts,
    steps: Vec<GatherSelectionStep>,
    receipts: ReceiptCursor<'_>,
) -> Result<GatherSelectionPlan, GatherSelectionError> {
    let selected = Point {
        x: facts.input.container_gpiece,
        y: facts.input.container_gpiece,
    };
    let rng = receipts.finish()?;
    let mut steps = steps;
    steps.push(GatherSelectionStep::SetExhaustedFallback {
        flag_store_va: 0x0061_9195,
        x_move_va: 0x0061_91a1,
        y_move_va: 0x0061_919d,
        selected,
    });
    Ok(GatherSelectionPlan {
        steps,
        continuation: GatherSelectionContinuation {
            resume_va: GATHER_SELECTION_END_VA,
            input: facts.input,
            selected,
            action: 0,
            terminal_selection: false,
            rng,
        },
    })
}

fn selected_plan(
    facts: &GatherSelectionFacts,
    mut steps: Vec<GatherSelectionStep>,
    receipts: ReceiptCursor<'_>,
    point_index: usize,
    point: GatherPointFacts,
) -> Result<GatherSelectionPlan, GatherSelectionError> {
    if point.advance_head_present {
        return Err(GatherSelectionError::Unexpected(
            "cursor-advance payload on a terminal point",
        ));
    }
    steps.push(GatherSelectionStep::SelectTerminalPoint {
        point: point_index,
        flag_store_va: 0x0061_8ff5,
        action_store_va: 0x0061_9007,
        selected: point.point,
        action: point.action,
    });
    let rng = receipts.finish()?;
    Ok(GatherSelectionPlan {
        steps,
        continuation: GatherSelectionContinuation {
            resume_va: GATHER_SELECTION_END_VA,
            input: facts.input,
            selected: point.point,
            action: point.action,
            terminal_selection: true,
            rng,
        },
    })
}

fn advance(point_index: usize, point: GatherPointFacts, steps: &mut Vec<GatherSelectionStep>) {
    if point.advance_head_present {
        steps.push(GatherSelectionStep::AdvanceGatherCursor {
            point: point_index,
            current_node_store_va: 0x0061_9176,
            current_data_store_va: 0x0061_917c,
            current_metric_store_va: 0x0061_9184,
        });
    }
}

fn movement(
    facts: &GatherSelectionFacts,
    point_index: usize,
    point: GatherPointFacts,
    mut destination: Point,
    previous: Point,
    receipts: &mut ReceiptCursor<'_>,
    steps: &mut Vec<GatherSelectionStep>,
) -> Result<Point, GatherSelectionError> {
    let is_captain = if facts.actor.is_captain_base_slot {
        facts.actor.captain_bit
    } else {
        truth(receipts.i32(HostCallKind::ActorIsCaptain { point: point_index }, steps)?)
    };
    if is_captain {
        if facts.actor.uber_size > 1 && facts.input.scratch_group.is_some_and(|group| group >= 0) {
            let (_code, found) = receipts.search(
                HostCallKind::ActorFindNearbySpot { point: point_index },
                steps,
            )?;
            destination = found;
            receipts.i32(
                HostCallKind::FindMoveAngle {
                    point: point_index,
                    group: true,
                },
                steps,
            )?;
            receipts.void(HostCallKind::ScratchGroupMove { point: point_index }, steps)?;
        } else {
            receipts.i32(
                HostCallKind::FindMoveAngle {
                    point: point_index,
                    group: false,
                },
                steps,
            )?;
            receipts.void(
                HostCallKind::ActorAddMoveFacingOrder { point: point_index },
                steps,
            )?;
        }
    }
    steps.push(GatherSelectionStep::SetMovementBaseline {
        point: point_index,
        x_store_vas: [0x0061_9130, 0x0061_9134],
        y_store_vas: [0x0061_913a, 0x0061_913e],
        before: previous,
        after: destination,
    });
    advance(point_index, point, steps);
    Ok(destination)
}

pub fn plan_unit_come_out_gather_selection(
    facts: &GatherSelectionFacts,
) -> Result<GatherSelectionPlan, GatherSelectionError> {
    if facts.input.resume_va != GATHER_SELECTION_START_VA {
        return Err(GatherSelectionError::ResumeVa {
            expected: GATHER_SELECTION_START_VA,
            observed: facts.input.resume_va,
        });
    }
    for identity in [facts.input.actor, facts.input.direct_container] {
        if !identity.valid() {
            return Err(GatherSelectionError::InvalidIdentity(identity));
        }
    }
    let mut steps = Vec::new();
    let mut receipts = ReceiptCursor::new(&facts.host_receipts, facts.input.rng);

    if !facts.has_gather_head {
        if !facts.points.is_empty() {
            return Err(GatherSelectionError::Unexpected(
                "gather points after an empty-head gate",
            ));
        }
        return fallback_plan(facts, steps, receipts);
    }
    if receipts.i32(HostCallKind::GatherInside, &mut steps)? != 0 {
        if !facts.points.is_empty() {
            return Err(GatherSelectionError::Unexpected(
                "gather points after gather_inside gate",
            ));
        }
        return fallback_plan(facts, steps, receipts);
    }
    if receipts.i32(HostCallKind::ActorOrderType, &mut steps)? != 0 {
        if !facts.points.is_empty() {
            return Err(GatherSelectionError::Unexpected(
                "gather points after actor order-type gate",
            ));
        }
        return fallback_plan(facts, steps, receipts);
    }
    let count = receipts.i32(HostCallKind::NumGather, &mut steps)?;
    receipts.void(HostCallKind::GatherHead, &mut steps)?;
    if count <= 0 {
        if !facts.points.is_empty() {
            return Err(GatherSelectionError::Unexpected(
                "gather points after non-positive count",
            ));
        }
        return fallback_plan(facts, steps, receipts);
    }
    let count = count as usize;
    if facts.points.len() != count {
        return Err(GatherSelectionError::PointCount {
            expected: count,
            observed: facts.points.len(),
        });
    }

    let mut previous = facts.input.actor_point;
    for (point_index, point) in facts.points.iter().copied().enumerate() {
        let mut destination = point.point;
        if point_index != 0 {
            let (code, found) = receipts.search(
                HostCallKind::TypeFindNearbySpot { point: point_index },
                &mut steps,
            )?;
            destination = found;
            if code != 0 {
                no_building_payload(&point)?;
                advance(point_index, point, &mut steps);
                continue;
            }
        }

        // Retail accepts the final list entry before interpreting its action or
        // looking up a building, and uses the raw GatherPoint coordinates.
        if point_index + 1 == count {
            no_building_payload(&point)?;
            return selected_plan(facts, steps, receipts, point_index, point);
        }

        let mut should_move = point.action == 0;
        let mut should_skip = false;
        let mut should_select = false;

        if point.action != 0 {
            let found = receipts.building(
                HostCallKind::FindAnyBuildingAt { point: point_index },
                &mut steps,
            )?;
            if found.object < 0 {
                no_building_payload(&point)?;
                if facts.actor.attack != 0 {
                    let worker = truth(receipts.i32(
                        HostCallKind::ActorIsWorker {
                            point: point_index,
                            building_found: false,
                        },
                        &mut steps,
                    )?);
                    if !worker && point.action == ATTACK_ACTION {
                        should_skip = true;
                    } else {
                        should_move = true;
                    }
                } else {
                    should_move = true;
                }
            } else {
                let building = point
                    .building
                    .ok_or(GatherSelectionError::Missing("candidate building facts"))?;
                if building.identity != found {
                    return Err(GatherSelectionError::BuildingMismatch {
                        expected: found,
                        observed: building.identity,
                    });
                }
                let mut valid = false;
                let mut blocked = false;
                let peasant = truth(receipts.i32(
                    HostCallKind::ActorIsPeasant { point: point_index },
                    &mut steps,
                )?);
                if peasant && point.action == GARRISON_ACTION {
                    let wallbuild = truth(receipts.i32(
                        HostCallKind::CandidateIsWallBuild { point: point_index },
                        &mut steps,
                    )?);
                    if wallbuild && found.owner == facts.input.actor.owner as i32 {
                        let active = truth(receipts.i32(
                            HostCallKind::CandidateIsActive { point: point_index },
                            &mut steps,
                        )?);
                        if !active || building.myhits != 0 {
                            valid = true;
                        } else {
                            let valid_wall = truth(receipts.i32(
                                HostCallKind::CandidateIsValidWall { point: point_index },
                                &mut steps,
                            )?);
                            if valid_wall {
                                let gather_type = truth(receipts.i32(
                                    HostCallKind::CandidateWallTypeIsGatherType {
                                        point: point_index,
                                    },
                                    &mut steps,
                                )?);
                                if !gather_type {
                                    let university = truth(receipts.i32(
                                        HostCallKind::CandidateMatchesUniversity {
                                            point: point_index,
                                            base_slot: building.university_is_base_slot,
                                        },
                                        &mut steps,
                                    )?);
                                    if !university {
                                        let gatherers = receipts.i32(
                                            HostCallKind::CandidateNumGatherers {
                                                point: point_index,
                                            },
                                            &mut steps,
                                        )?;
                                        if gatherers < building.gather_max as i32 {
                                            valid = true;
                                        } else {
                                            blocked = true;
                                        }
                                    } else {
                                        blocked = true;
                                    }
                                } else {
                                    blocked = true;
                                }
                            } else {
                                blocked = true;
                            }
                        }
                    }
                }

                let active_build = truth(receipts.i32(
                    HostCallKind::CandidateIsActiveBuild { point: point_index },
                    &mut steps,
                )?);
                if active_build {
                    let ally = truth(receipts.i32(
                        HostCallKind::CandidateOwnerIsAlly { point: point_index },
                        &mut steps,
                    )?);
                    if ally {
                        let limit = receipts.i32(
                            HostCallKind::CandidateGarrisonLimitGate { point: point_index },
                            &mut steps,
                        )?;
                        if limit != 0 {
                            let can_garrison = truth(receipts.i32(
                                HostCallKind::ActorCanGarrison {
                                    point: point_index,
                                    type_index: building.type_index,
                                },
                                &mut steps,
                            )?);
                            if can_garrison && point.action == GARRISON_ACTION {
                                let capacity = receipts.i32(
                                    HostCallKind::CandidateGarrisonLimitCapacity {
                                        point: point_index,
                                    },
                                    &mut steps,
                                )?;
                                let inside = receipts.i32(
                                    HostCallKind::CandidateNumInside { point: point_index },
                                    &mut steps,
                                )?;
                                let control = receipts.i32(
                                    HostCallKind::ActorControlCost { point: point_index },
                                    &mut steps,
                                )?;
                                if inside.wrapping_add(control) <= capacity {
                                    valid = true;
                                } else {
                                    blocked = true;
                                }
                            }
                        }
                    }
                }

                let mut enemy_attack_override = false;
                if facts.actor.attack != 0 {
                    let worker = truth(receipts.i32(
                        HostCallKind::ActorIsWorker {
                            point: point_index,
                            building_found: true,
                        },
                        &mut steps,
                    )?);
                    if !worker {
                        let enemy = truth(receipts.i32(
                            HostCallKind::CandidateOwnerIsEnemy { point: point_index },
                            &mut steps,
                        )?);
                        enemy_attack_override = enemy && point.action == ATTACK_ACTION;
                    }
                }

                if valid {
                    should_skip = true;
                } else if blocked || enemy_attack_override {
                    should_select = true;
                } else {
                    should_move = true;
                }
            }
        } else if point.building.is_some() {
            return Err(GatherSelectionError::Unexpected(
                "building payload on action zero",
            ));
        }

        if should_select {
            return selected_plan(facts, steps, receipts, point_index, point);
        }
        if should_move {
            previous = movement(
                facts,
                point_index,
                point,
                destination,
                previous,
                &mut receipts,
                &mut steps,
            )?;
        } else {
            debug_assert!(should_skip);
            advance(point_index, point, &mut steps);
        }
    }

    fallback_plan(facts, steps, receipts)
}
