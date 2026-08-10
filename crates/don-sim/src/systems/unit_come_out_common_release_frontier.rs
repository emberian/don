//! Second source-only tranche of retail `Unit::come_out(int)`.
//!
//! The first frontier stops at `0x006186B4`.  This planner owns the common release
//! prologue through the captain/build gather-list seam at `0x00618B22`, plus the three
//! compiler-outlined virtual-call islands used by that prologue.  It records every
//! stateful or result-bearing host call as an ordered RNG receipt and returns a plan;
//! it never publishes a partial world mutation.

pub const COMMON_RELEASE_START_VA: u32 = 0x0061_86b4;
pub const GATHER_LIST_RESUME_VA: u32 = 0x0061_8b22;
pub const FALLBACK_SETUP_RESUME_VA: u32 = 0x0061_919d;
pub const SEQUENTIAL_BYTES: u32 = GATHER_LIST_RESUME_VA - COMMON_RELEASE_START_VA;
pub const OUTLINED_VIRTUAL_ISLANDS: [(u32, u32); 3] = [
    (0x0061_a24e, 0x0061_a25f),
    (0x0061_a25f, 0x0061_a268),
    (0x0061_a268, 0x0061_a271),
];
pub const OUTLINED_BYTES: u32 = 17 + 9 + 9;
pub const LOGICAL_TRANCHE_BYTES: u32 = SEQUENTIAL_BYTES + OUTLINED_BYTES;
pub const PRIOR_RESIDUAL_BYTES: u32 = 7_201;
pub const RESIDUAL_BYTES_AFTER_TRANCHE: u32 = PRIOR_RESIDUAL_BYTES - LOGICAL_TRANCHE_BYTES;

pub const CEO_POSITION_MASK: u32 = 0x0001_0000;
pub const PLANE_EXIT_FLAG: u32 = 0x20;
pub const LEADER_COMMAND_FLAG: u32 = 4;
pub const ACTIVE_OBJECT_FLAG: u8 = 1;
pub const ACTIVE_BUILD_FLAG: u8 = 0x20;
pub const CITY_WORKER_FLAG: u16 = 0x40;
pub const PATHING_UNIT_MASK: u32 = 0x0400_0000;
pub const FIRST_GUY_EXIT_Z_DELTA: i32 = 500;
pub const WORKER_EXIT_ACTION: i32 = 0x1a;
pub const EXIT_ANIM_TYPES: [i32; 2] = [0x34, 0x35];
pub const NUKE_FAMILY_TYPE: i32 = 0x13b;

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

impl WideObjectIdentity {
    pub const fn new(owner: i32, object: i32) -> Self {
        Self { owner, object }
    }

    /// Retail's order-target test is exactly `owner != -1 && object != -1`; it does
    /// not impose a non-negative/range fence at this site.
    pub const fn present_at_retail_gate(self) -> bool {
        self.owner != -1 && self.object != -1
    }

    pub const fn equals_narrow(self, other: ObjectIdentity) -> bool {
        self.owner == other.owner as i32 && self.object == other.object as i32
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

/// Structural copy of the typed seam emitted by the first frontier.  Integration must
/// convert this from `unit_come_out_full_frontier::UnitComeOutContinuation` without
/// changing any field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrefixContinuation {
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
pub enum HostCallKind {
    SetNewLocation,
    UpdateCeoPosition,
    UpdateFirstGuyZ,
    DownUnitComeOut,
    SetExitAnimation,
    CloseOrders,
    ClearPartialPath,
    ActorUpdateAction,
    ActorReadStrafeTarget,
    ScratchGroupAdd,
    ScratchGroupPush,
    WorkerUpdateAction { slot: usize },
    WorkerReadGarrisonTarget { slot: usize },
}

impl HostCallKind {
    pub const fn call_va(self) -> u32 {
        match self {
            Self::SetNewLocation => 0x0061_86bc,
            Self::UpdateCeoPosition => 0x0061_86f8,
            Self::UpdateFirstGuyZ => 0x0061_8711,
            Self::DownUnitComeOut => 0x0061_879c,
            Self::SetExitAnimation => 0x0061_87b9,
            Self::CloseOrders => 0x0061_8828,
            Self::ClearPartialPath => 0x0061_882f,
            Self::ActorUpdateAction => 0x0061_8836,
            Self::ActorReadStrafeTarget => 0x0061_88da,
            Self::ScratchGroupAdd => 0x0061_8994,
            Self::ScratchGroupPush => 0x0061_89a5,
            Self::WorkerUpdateAction { .. } => 0x0061_8aaf,
            Self::WorkerReadGarrisonTarget { .. } => 0x0061_8ab8,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostReturn {
    VoidOrIgnored,
    I32(i32),
    Target(WideObjectIdentity),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostCallReceipt {
    pub kind: HostCallKind,
    pub result: HostReturn,
    pub before: RngStamp,
    pub after: RngStamp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActorReleaseFacts {
    pub unit_masks: u32,
    pub unit_masks2: u32,
    pub domain: i32,
    pub unit_type_flags: u32,
    pub type_index: i32,
    /// True when vtable slot `+0xC0` is the base `UnitData::is_plane` fast target.
    /// Retail folds that target to the domain/type-flag test; an override is called
    /// and its boolean result is used directly.
    pub is_plane_base_slot: bool,
    pub is_plane: bool,
    pub is_captain: bool,
    /// Exact result of `Unit::is(0x13B, false)`, not an equality shortcut.
    pub matches_nuke_family: bool,
    pub uber_size: i32,
    pub path_length: i32,
    pub leader_flags: u32,
    pub nukes_launched_before: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FirstGuyFacts {
    /// `Guy::z` after `Guy::update_z()` returns at `0x00618716`.
    pub z_after_update: i32,
    pub last_z_before: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DownUnitFacts {
    pub identity: ObjectIdentity,
    /// Low `SubObjectData::flags`; bit zero admits recursive `come_out(1)`.
    pub flags_low: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OrderHeadFacts {
    pub metric: u8,
    /// The head-node cursor stores still occur when its current data is null, but
    /// retail skips `UnitOrder::get_strafe_order` in that case.
    pub current_data_present: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkerSlotFacts {
    pub active: bool,
    pub is_worker: bool,
    pub action_type: Option<i32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildExitFacts {
    pub flags_low: u8,
    pub inside_down: i16,
    pub city_index: i16,
    /// Read only after the active/uncontained/non-negative-city gates.  `None`
    /// means retail did not dereference the owner city table at this prefix.
    pub city_flags_before: Option<u16>,
    /// Total owner object slots used by the bounded retail scan.
    pub owner_object_slots: usize,
    /// The observed prefix: all slots when no match exists, or exactly through the
    /// first worker whose current action targets the containing build.
    pub worker_scan: Vec<WorkerSlotFacts>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CaptainContainerKind {
    Other,
    Build(BuildExitFacts),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaptainContainerFacts {
    pub identity: ObjectIdentity,
    pub kind: CaptainContainerKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommonReleaseFacts {
    pub prefix: PrefixContinuation,
    pub actor: ActorReleaseFacts,
    pub first_guy: Option<FirstGuyFacts>,
    pub down_unit: Option<DownUnitFacts>,
    pub order_head: Option<OrderHeadFacts>,
    pub captain_container: Option<CaptainContainerFacts>,
    pub host_receipts: Vec<HostCallReceipt>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommonReleaseStep {
    HostCall {
        call_va: u32,
        receipt: HostCallReceipt,
    },
    RaiseFirstGuyAfterPlaneExit {
        z_store_va: u32,
        last_z_store_va: u32,
        z_before: i32,
        last_z_before: i32,
        after: i32,
    },
    ClearPathingMask {
        store_va: u32,
        before: u32,
        after: u32,
    },
    ClearPathLength {
        store_va: u32,
        before: i32,
    },
    SelectOrderHead {
        first_node_store_va: u32,
        first_data_store_va: u32,
        first_metric_store_va: u32,
        repeat_node_store_va: u32,
        repeat_data_store_va: u32,
        repeat_metric_store_va: u32,
        metric: u8,
    },
    IncrementNukesLaunched {
        store_va: u32,
        before: i32,
        after: i32,
        target: WideObjectIdentity,
    },
    ClearCityWorkerFlag {
        store_va: u32,
        city_index: i16,
        before: u16,
        after: u16,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommonReleaseContinuation {
    pub resume_va: u32,
    pub prefix: PrefixContinuation,
    pub scratch_group: Option<i32>,
    pub rng: RngStamp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommonReleaseExit {
    GatherList(CommonReleaseContinuation),
    FallbackSetup(CommonReleaseContinuation),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommonReleasePlan {
    pub steps: Vec<CommonReleaseStep>,
    pub exit: CommonReleaseExit,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommonReleaseError {
    ResumeVa {
        expected: u32,
        observed: u32,
    },
    InvalidIdentity(ObjectIdentity),
    Missing(&'static str),
    Unexpected(&'static str),
    ContainerMismatch {
        expected: ObjectIdentity,
        observed: ObjectIdentity,
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
    InvalidComeOutResult(i32),
    IncompleteWorkerScan {
        expected_slots: usize,
        observed: usize,
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
        steps: &mut Vec<CommonReleaseStep>,
    ) -> Result<HostReturn, CommonReleaseError> {
        let Some(receipt) = self.receipts.get(self.index).copied() else {
            return Err(CommonReleaseError::ReceiptCount {
                consumed: self.index + 1,
                observed: self.receipts.len(),
            });
        };
        if receipt.kind != expected {
            return Err(CommonReleaseError::ReceiptKind {
                expected,
                observed: receipt.kind,
            });
        }
        if receipt.before != self.rng
            || receipt.after.draws < receipt.before.draws
            || (receipt.before.seed != receipt.after.seed
                && receipt.before.draws == receipt.after.draws)
        {
            return Err(CommonReleaseError::ReceiptContinuity);
        }
        self.index += 1;
        self.rng = receipt.after;
        steps.push(CommonReleaseStep::HostCall {
            call_va: expected.call_va(),
            receipt,
        });
        Ok(receipt.result)
    }

    fn take_void(
        &mut self,
        expected: HostCallKind,
        steps: &mut Vec<CommonReleaseStep>,
    ) -> Result<(), CommonReleaseError> {
        let observed = self.take(expected, steps)?;
        if observed != HostReturn::VoidOrIgnored {
            return Err(CommonReleaseError::ReceiptReturn {
                kind: expected,
                observed,
            });
        }
        Ok(())
    }

    fn finish(self) -> Result<RngStamp, CommonReleaseError> {
        if self.index != self.receipts.len() {
            return Err(CommonReleaseError::ReceiptCount {
                consumed: self.index,
                observed: self.receipts.len(),
            });
        }
        Ok(self.rng)
    }
}

fn ensure_no_captain_payloads(facts: &CommonReleaseFacts) -> Result<(), CommonReleaseError> {
    if facts.captain_container.is_some() {
        return Err(CommonReleaseError::Unexpected(
            "captain-container facts on a non-captain branch",
        ));
    }
    Ok(())
}

pub fn plan_unit_come_out_common_release(
    facts: &CommonReleaseFacts,
) -> Result<CommonReleasePlan, CommonReleaseError> {
    if facts.prefix.resume_va != COMMON_RELEASE_START_VA {
        return Err(CommonReleaseError::ResumeVa {
            expected: COMMON_RELEASE_START_VA,
            observed: facts.prefix.resume_va,
        });
    }
    if !facts.prefix.actor.valid() {
        return Err(CommonReleaseError::InvalidIdentity(facts.prefix.actor));
    }
    for identity in [
        facts.prefix.direct_container,
        facts.prefix.placement_container,
        facts.down_unit.map(|down| down.identity),
    ]
    .into_iter()
    .flatten()
    {
        if !identity.valid() {
            return Err(CommonReleaseError::InvalidIdentity(identity));
        }
    }
    if let Some(down) = facts.down_unit {
        if down.identity.owner != facts.prefix.actor.owner {
            return Err(CommonReleaseError::Unexpected(
                "down-unit owner differs from actor owner",
            ));
        }
    }

    let mut steps = Vec::new();
    let mut receipts = ReceiptCursor::new(&facts.host_receipts, facts.prefix.rng);

    match receipts.take(HostCallKind::SetNewLocation, &mut steps)? {
        HostReturn::I32(_) => {}
        observed => {
            return Err(CommonReleaseError::ReceiptReturn {
                kind: HostCallKind::SetNewLocation,
                observed,
            });
        }
    }

    if facts.actor.unit_masks2 & CEO_POSITION_MASK != 0 {
        receipts.take_void(HostCallKind::UpdateCeoPosition, &mut steps)?;
    }

    if facts.actor.domain == 2 {
        let guy = facts
            .first_guy
            .ok_or(CommonReleaseError::Missing("first Guy for plane exit"))?;
        receipts.take_void(HostCallKind::UpdateFirstGuyZ, &mut steps)?;
        if facts.actor.unit_type_flags & PLANE_EXIT_FLAG != 0 {
            let after = guy.z_after_update.wrapping_add(FIRST_GUY_EXIT_Z_DELTA);
            steps.push(CommonReleaseStep::RaiseFirstGuyAfterPlaneExit {
                z_store_va: 0x0061_8737,
                last_z_store_va: 0x0061_8734,
                z_before: guy.z_after_update,
                last_z_before: guy.last_z_before,
                after,
            });
        }
    } else if facts.first_guy.is_some() {
        return Err(CommonReleaseError::Unexpected(
            "first-Guy Z facts on a non-plane-domain branch",
        ));
    }

    if let Some(down) = facts.down_unit {
        if down.flags_low & ACTIVE_OBJECT_FLAG != 0 {
            let result = receipts.take(HostCallKind::DownUnitComeOut, &mut steps)?;
            let HostReturn::I32(result) = result else {
                return Err(CommonReleaseError::ReceiptReturn {
                    kind: HostCallKind::DownUnitComeOut,
                    observed: result,
                });
            };
            if !matches!(result, 0 | 1) {
                return Err(CommonReleaseError::InvalidComeOutResult(result));
            }
        }
    }

    if EXIT_ANIM_TYPES.contains(&facts.actor.type_index) {
        receipts.take_void(HostCallKind::SetExitAnimation, &mut steps)?;
    }

    let plane_allows_cleanup = if facts.actor.is_plane_base_slot {
        facts.actor.domain != 2 || facts.actor.unit_type_flags & PLANE_EXIT_FLAG != 0
    } else {
        !facts.actor.is_plane
    };
    let clear_path = facts.actor.leader_flags & LEADER_COMMAND_FLAG == 0 && plane_allows_cleanup;
    if clear_path {
        steps.push(CommonReleaseStep::ClearPathingMask {
            store_va: 0x0061_8813,
            before: facts.actor.unit_masks,
            after: facts.actor.unit_masks & !PATHING_UNIT_MASK,
        });
        steps.push(CommonReleaseStep::ClearPathLength {
            store_va: 0x0061_881e,
            before: facts.actor.path_length,
        });
        receipts.take_void(HostCallKind::CloseOrders, &mut steps)?;
        receipts.take_void(HostCallKind::ClearPartialPath, &mut steps)?;
        receipts.take_void(HostCallKind::ActorUpdateAction, &mut steps)?;
    }

    if facts.actor.matches_nuke_family {
        if let Some(head) = facts.order_head {
            steps.push(CommonReleaseStep::SelectOrderHead {
                first_node_store_va: 0x0061_887f,
                first_data_store_va: 0x0061_8888,
                first_metric_store_va: 0x0061_8891,
                repeat_node_store_va: 0x0061_88ba,
                repeat_data_store_va: 0x0061_88c3,
                repeat_metric_store_va: 0x0061_88cc,
                metric: head.metric,
            });
            if head.current_data_present {
                let result = receipts.take(HostCallKind::ActorReadStrafeTarget, &mut steps)?;
                let HostReturn::Target(target) = result else {
                    return Err(CommonReleaseError::ReceiptReturn {
                        kind: HostCallKind::ActorReadStrafeTarget,
                        observed: result,
                    });
                };
                if target.present_at_retail_gate() {
                    steps.push(CommonReleaseStep::IncrementNukesLaunched {
                        store_va: 0x0061_88fa,
                        before: facts.actor.nukes_launched_before,
                        after: facts.actor.nukes_launched_before.wrapping_add(1),
                        target,
                    });
                }
            }
        }
    } else if facts.order_head.is_some() {
        return Err(CommonReleaseError::Unexpected(
            "order-head observation outside the 0x13B family branch",
        ));
    }

    if !facts.actor.is_captain {
        ensure_no_captain_payloads(facts)?;
        let rng = receipts.finish()?;
        return Ok(CommonReleasePlan {
            steps,
            exit: CommonReleaseExit::FallbackSetup(CommonReleaseContinuation {
                resume_va: FALLBACK_SETUP_RESUME_VA,
                prefix: facts.prefix,
                scratch_group: None,
                rng,
            }),
        });
    }

    let Some(container_identity) = facts.prefix.direct_container else {
        if facts.captain_container.is_some() {
            return Err(CommonReleaseError::Unexpected(
                "container facts without a direct container",
            ));
        }
        let rng = receipts.finish()?;
        return Ok(CommonReleasePlan {
            steps,
            exit: CommonReleaseExit::FallbackSetup(CommonReleaseContinuation {
                resume_va: FALLBACK_SETUP_RESUME_VA,
                prefix: facts.prefix,
                scratch_group: None,
                rng,
            }),
        });
    };
    let container = facts
        .captain_container
        .as_ref()
        .ok_or(CommonReleaseError::Missing(
            "captain direct-container facts",
        ))?;
    if container.identity != container_identity {
        return Err(CommonReleaseError::ContainerMismatch {
            expected: container_identity,
            observed: container.identity,
        });
    }

    let CaptainContainerKind::Build(build) = &container.kind else {
        let rng = receipts.finish()?;
        return Ok(CommonReleasePlan {
            steps,
            exit: CommonReleaseExit::FallbackSetup(CommonReleaseContinuation {
                resume_va: FALLBACK_SETUP_RESUME_VA,
                prefix: facts.prefix,
                scratch_group: None,
                rng,
            }),
        });
    };

    let mut scratch_group = None;
    if facts.actor.uber_size > 1 {
        receipts.take_void(HostCallKind::ScratchGroupAdd, &mut steps)?;
        let result = receipts.take(HostCallKind::ScratchGroupPush, &mut steps)?;
        let HostReturn::I32(group) = result else {
            return Err(CommonReleaseError::ReceiptReturn {
                kind: HostCallKind::ScratchGroupPush,
                observed: result,
            });
        };
        scratch_group = Some(group);
    }

    let city_gate =
        build.flags_low & ACTIVE_BUILD_FLAG != 0 && build.inside_down < 0 && build.city_index >= 0;
    if !city_gate {
        if build.city_flags_before.is_some()
            || build.owner_object_slots != 0
            || !build.worker_scan.is_empty()
        {
            return Err(CommonReleaseError::Unexpected(
                "city/worker facts after an unreached build gate",
            ));
        }
    } else {
        let city_flags = build
            .city_flags_before
            .ok_or(CommonReleaseError::Missing("city flags"))?;
        if city_flags & CITY_WORKER_FLAG == 0 {
            if build.owner_object_slots != 0 || !build.worker_scan.is_empty() {
                return Err(CommonReleaseError::Unexpected(
                    "worker scan after a clear city-worker gate",
                ));
            }
        } else {
            if build.worker_scan.len() > build.owner_object_slots {
                return Err(CommonReleaseError::Unexpected(
                    "worker scan exceeds owner object slots",
                ));
            }
            let mut matched = false;
            for (slot, candidate) in build.worker_scan.iter().copied().enumerate() {
                if !candidate.active {
                    if candidate.is_worker || candidate.action_type.is_some() {
                        return Err(CommonReleaseError::Unexpected(
                            "worker payload on an inactive object slot",
                        ));
                    }
                    continue;
                }
                if !candidate.is_worker {
                    if candidate.action_type.is_some() {
                        return Err(CommonReleaseError::Unexpected(
                            "action type on a non-worker slot",
                        ));
                    }
                    continue;
                }
                let action_type = candidate
                    .action_type
                    .ok_or(CommonReleaseError::Missing("worker action type"))?;
                if action_type != WORKER_EXIT_ACTION {
                    continue;
                }
                receipts.take_void(HostCallKind::WorkerUpdateAction { slot }, &mut steps)?;
                let result =
                    receipts.take(HostCallKind::WorkerReadGarrisonTarget { slot }, &mut steps)?;
                let HostReturn::Target(target) = result else {
                    return Err(CommonReleaseError::ReceiptReturn {
                        kind: HostCallKind::WorkerReadGarrisonTarget { slot },
                        observed: result,
                    });
                };
                if target.equals_narrow(container_identity) {
                    matched = true;
                    if slot + 1 != build.worker_scan.len() {
                        return Err(CommonReleaseError::Unexpected(
                            "worker observations after the first matching target",
                        ));
                    }
                    break;
                }
            }
            if !matched && build.worker_scan.len() != build.owner_object_slots {
                return Err(CommonReleaseError::IncompleteWorkerScan {
                    expected_slots: build.owner_object_slots,
                    observed: build.worker_scan.len(),
                });
            }
            if !matched && facts.actor.leader_flags & LEADER_COMMAND_FLAG != 0 {
                steps.push(CommonReleaseStep::ClearCityWorkerFlag {
                    store_va: 0x0061_8b1d,
                    city_index: build.city_index,
                    before: city_flags,
                    after: city_flags & !CITY_WORKER_FLAG,
                });
            }
        }
    }

    let rng = receipts.finish()?;
    Ok(CommonReleasePlan {
        steps,
        exit: CommonReleaseExit::GatherList(CommonReleaseContinuation {
            resume_va: GATHER_LIST_RESUME_VA,
            prefix: facts.prefix,
            scratch_group,
            rng,
        }),
    })
}
