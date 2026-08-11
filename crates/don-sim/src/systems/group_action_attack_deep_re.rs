//! Instruction-grounded, side-effect-free owner for retail
//! `Group::action_attack` (`0x00712490`, 3,833 bytes).
//!
//! This file is intentionally not registered in `systems/mod.rs`.  It is the receipt
//! boundary for the future canonical `Sim` Groups/OrderList adapter: an authoritative host
//! captures the reads below, [`plan_action_attack`] emits the retail-ordered transaction,
//! and the host revalidates every identity/revision before applying any step.

pub const ACTION_ATTACK_VA: u32 = 0x0071_2490;
pub const ACTION_ATTACK_SIZE: u32 = 3_833;
pub const GROUP_IS_ON_MAP_VA: u32 = 0x0070_c450;
pub const GROUP_ACTION_BEGIN_VA: u32 = 0x0071_4100;
pub const GROUP_KILL_VA: u32 = 0x0071_4110;
pub const GROUP_FIND_LEADER_VA: u32 = 0x0070_ccb0;
pub const GROUP_GET_LOC_TO_VA: u32 = 0x0070_c5d0;
pub const GROUP_SET_UP_INSERT_VA: u32 = 0x0070_e520;
pub const GROUP_ACTION_HALT_VA: u32 = 0x0070_d0c0;
pub const GROUP_FINISH_INSERT_VA: u32 = 0x0070_e620;
pub const GROUP_ACTION_MOVE_TO_VA: u32 = 0x0070_fba0;
pub const UNIT_FIND_ATTACK_POS_VA: u32 = 0x0060_1280;
pub const UNIT_FIND_MELEE_TARGET_VA: u32 = 0x005f_f9c0;
pub const UNIT_CLEAR_ORDERS_VA: u32 = 0x005e_3860;
pub const UNIT_ADD_CAST_ORDER_VA: u32 = 0x005e_4a60;
pub const UNIT_ADD_ATTACK_ORDER_VA: u32 = 0x005e_5410;
pub const UNIT_ADD_MOVE_ORDER_VA: u32 = 0x0061_6ed0;
pub const BUILD_CHECK_CAPTURE_VA: u32 = 0x0062_76a0;
pub const OBJECT_IS_IN_RANGE_VA: u32 = 0x0064_86b0;
pub const OBJECT_IS_IN_RANGE_SHORT_VA: u32 = 0x0064_8d70;
pub const RANDOM_GET_VA_IN_FIND_ATTACK_POS: u32 = 0x0060_2124;
pub const RANDOM_GET_VA: u32 = 0x00a3_9d70;

pub const SPELL_PACK: i32 = 0x28b;
pub const SPELL_DEPLOY: i32 = 0x28c;
pub const TYPE_FILTER_0X3A: i32 = 0x3a;

/// The body has no direct RNG call.  Its only reachable RNG seam is the accepted-candidate
/// arm inside `Unit::find_attack_pos`, whose call instruction is `0x00602124`.
pub const DIRECT_RNG_CALLS: &[u32] = &[];

/// Every state-bearing host read made by this owner, in first-access order.  Repeated reads
/// inside the domain/member loops are represented once here and chronologically in each
/// member receipt.
pub const HOST_READS: &[HostRead] = &[
    HostRead::new(0x0071_24b0, "GroupData::is_on_map 0x0070C450"),
    HostRead::new(0x0071_24bd, "argument ox >= 0"),
    HostRead::new(
        0x0071_24c8,
        "ScenarioData::ignore_orders + owner prune list",
    ),
    HostRead::new(0x0071_252f, "GroupData::num +0x0C"),
    HostRead::new(0x0071_2540, "GroupData::buildings +0x49"),
    HostRead::new(0x0071_2561, "GroupData::list +0x8CC (ordered i16 prefix)"),
    HostRead::new(0x0071_27b0, "GroupData::find_leader 0x0070CCB0"),
    HostRead::new(
        0x0071_27dc,
        "leader ObjectData x/y +0x10/+0x14 (xor 0x63637)",
    ),
    HostRead::new(
        0x0071_2808,
        "GroupData::get_loc_to 0x0070C5D0 when mandatory == 1",
    ),
    HostRead::new(0x0071_2866, "ObjectData::is_in_range 0x006486B0"),
    HostRead::new(0x0071_28a5, "Unit::find_attack_pos 0x00601280"),
    HostRead::new(0x0071_2959, "target virtual is_build +0x20"),
    HostRead::new(0x0071_2973, "ObjectData::is_active_build 0x0046FB40"),
    HostRead::new(0x0071_2999, "BuildData::check_capture_eligible 0x0062D1D0"),
    HostRead::new(
        0x0071_2a2a,
        "target virtual is_seen +0x48; move_only = !seen",
    ),
    HostRead::new(0x0071_2a8e, "member virtual is_valid +0x08"),
    HostRead::new(0x0071_2a9e, "member virtual is_on_map +0xBC"),
    HostRead::new(0x0071_2ad0, "UnitTypeData::is_defense virtual +0x10C"),
    HostRead::new(0x0071_2b1a, "UnitData::is_special virtual +0xD4"),
    HostRead::new(0x0071_2b60, "ObjectData::is(0x3A, 0) virtual +0xB8"),
    HostRead::new(0x0071_2b83, "UnitData::is_plane virtual +0xC0"),
    HostRead::new(0x0071_2b97, "UnitTypeData domain +0x218 / flags +0x2B4"),
    HostRead::new(
        0x0071_2be0,
        "UnitData::get_action 0x00608450 and AttackOrder payload",
    ),
    HostRead::new(0x0071_2d68, "UnitData::is_packing_or_unpacking 0x0060A410"),
    HostRead::new(
        0x0071_2e22,
        "UnitData::mana 0x00609A50 and instance +0x96 reserve",
    ),
    HostRead::new(0x0071_2e57, "UnitData::order_type 0x00616E80"),
    HostRead::new(0x0071_2e7c, "Unit::update_order / StrafeOrder payload"),
    HostRead::new(
        0x0071_32a9,
        "vector_dist, WorldData +0x18, target wallbuild predicate",
    ),
    HostRead::new(
        0x0071_3300,
        "Unit::find_melee_target 0x005FF9C0 when mandatory == 0",
    ),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostRead {
    pub first_read_va: u32,
    pub what: &'static str,
}

impl HostRead {
    pub const fn new(first_read_va: u32, what: &'static str) -> Self {
        Self {
            first_read_va,
            what,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ObjectKey {
    pub who: u8,
    pub o: i32,
    /// Canonical sparse-slot generation/revision supplied by the future Sim owner.
    pub revision: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GroupKey {
    pub who: u8,
    pub slot: u8,
    pub id: i32,
    pub revision: u64,
    pub digest: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Coord {
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(i32)]
pub enum QueuePos {
    #[default]
    First = 0,
    Last = 1,
    New = 2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum OrderKind {
    MoveTo = 1,
    AttackTo = 2,
    ExploreTo = 3,
    FleeTo = 4,
    Attack = 10,
    CastSpell = 14,
    Strafe = 16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RngDraw {
    pub call_va: u32,
    pub rng_va: u32,
    pub low: i32,
    pub high: i32,
    pub value: i32,
    pub state_before: u32,
    pub state_after: u32,
}

/// Result of the single `find_attack_pos` call site at `0x007128A5`.
///
/// `rng` is in actual invocation order.  The delegated body calls
/// `Random::get(0, 0xFFFF)` at `0x00602124` for each accepted candidate; zero candidates
/// means zero draws.  The outer owner performs no other draw.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttackPositionReceipt {
    /// Leader position, or the `get_loc_to` result when `mandatory == 1`.
    pub origin: Coord,
    pub initial_in_range: bool,
    pub called_find_attack_pos: bool,
    pub find_succeeded: bool,
    pub to: Coord,
    pub rng: Vec<RngDraw>,
    pub world_digest: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MeleeTargetReceipt {
    pub requested_distance: i32,
    pub world_dimension_times_0x240: i32,
    /// Literal final argument: `2` for wallbuild targets, `1` otherwise.
    pub target_class_arg: i32,
    pub result: Option<ObjectKey>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExistingAttackReceipt {
    pub target: ObjectKey,
    pub active_byte_0x1c: bool,
    pub unit_counter_0xd8: i32,
    /// Result of the first `ObjectData::is_in_range` probe at `0x00712C73`.
    pub primary_in_range: bool,
    pub current_order_is_attack: bool,
    /// Result of the payload-bounds probe at `0x00712D41`.
    pub payload_in_range: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StrafeReceipt {
    pub order_revision: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemberSnapshot {
    pub key: ObjectKey,
    pub position: Coord,
    pub valid: bool,
    /// Separate from `valid`: the buildings-only prefix calls virtual `is_build +0x20`.
    pub is_build: bool,
    pub on_map: bool,
    pub type_is_defense: bool,
    pub is_special: bool,
    pub is_type_0x3a: bool,
    pub is_plane: bool,
    /// Whether vslot `+0xC0` is the default `UnitData::is_plane` implementation.  Retail
    /// devirtualizes that exact pointer into the domain/flag test; overrides are called.
    pub is_plane_vslot_is_default: bool,
    pub type_domain: i32,
    pub type_flags_0x2b4: u8,
    pub packing_or_unpacking: bool,
    pub packing: bool,
    pub mana: i32,
    pub mana_reserve_0x96: i16,
    pub order_kind: Option<OrderKind>,
    pub strafe: Option<StrafeReceipt>,
    pub existing_attack: Option<ExistingAttackReceipt>,
    /// Result returned by every short `is_in_range` call for this immutable receipt.
    pub target_in_range: bool,
    /// Presentation-only range warning used by the buildings branch.  It never changes
    /// synchronized state, but keeping it here makes the control-flow receipt complete.
    pub building_warning_invalid_range: bool,
    /// Present exactly when the retail mandatory-zero non-build arm reaches
    /// `Unit::find_melee_target`.
    pub melee_target: Option<MeleeTargetReceipt>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TargetSnapshot {
    pub key: ObjectKey,
    pub position: Coord,
    pub uid_0x30: i16,
    pub is_unit: bool,
    pub is_wallbuild: bool,
    pub is_build: bool,
    pub is_active_build: bool,
    pub capture_eligible: bool,
    pub seen_by_group: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupSnapshot {
    pub key: GroupKey,
    pub on_map: bool,
    pub buildings: bool,
    pub is_local_player: bool,
    pub disband: i32,
    pub order_num: i32,
    /// The exact `num` field, kept separate so a torn list receipt is rejected.
    pub num: i32,
    pub members: Vec<MemberSnapshot>,
    pub leader: Option<ObjectKey>,
    pub leader_position: Coord,
    pub mandatory_one_loc_to: Option<Coord>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScenarioSnapshot {
    pub ignore_orders: bool,
    /// The owner row at `0x00ED6580`, in exact list order.  Negative ids are ignored.
    pub prune_objects: Vec<i32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttackRequest {
    pub group: GroupSnapshot,
    pub ox: i32,
    pub whom: i32,
    pub mandatory: i32,
    pub queued: QueuePos,
    pub ignore: i32,
    pub scenario: ScenarioSnapshot,
    pub target: Option<TargetSnapshot>,
    pub attack_position: Option<AttackPositionReceipt>,
    /// If a producer cannot answer a reached read, it records the first unavailable read
    /// here and planning fails closed before any mutation is returned.
    pub first_unavailable: Option<ExternalBoundary>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExternalBoundary {
    pub call_va: u32,
    pub callee_va: u32,
    pub symbol: &'static str,
    pub detail: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlanError {
    External(ExternalBoundary),
    TornGroupReceipt {
        num: i32,
        members: usize,
    },
    MissingTarget,
    TargetIdentityMismatch {
        requested: ObjectKey,
        supplied: ObjectKey,
    },
    MissingAttackPositionReceipt,
    InvalidRngReceipt {
        draw_index: usize,
    },
    MissingMeleeTargetReceipt {
        member: ObjectKey,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Exit {
    GroupOffMap,
    NegativeTarget,
    EmptyGroup,
    NoLeader,
    BuildingsHandled,
    QueueFirstDelegated,
    CaptureMoveDelegated,
    MembersHandled,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttackPlan {
    pub group: GroupKey,
    pub target: Option<ObjectKey>,
    pub steps: Vec<PlanStep>,
    pub exit: Exit,
    /// A commit adapter must compare this complete read set before executing `steps[0]`.
    pub revalidate_objects: Vec<ObjectKey>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlanStep {
    ScenarioKill {
        o: i32,
        who: u8,
        arg2: i32,
        arg3: i32,
    },
    /// Exact eight-byte `Group::action_begin`: `GroupData::disband +0x28 = 0`.
    SetGroupDisband(i32),
    BuildingAttack {
        building: ObjectKey,
        or_flags_0x60: u16,
        target_o_0x7c: i16,
        target_who_0x81: u8,
    },
    MixedBuildingUnitAssertion {
        member: ObjectKey,
    },
    PresentationAttackRangeWarning {
        member: ObjectKey,
        string_offset: u32,
        sound_category: i32,
    },
    QueueFirstDance {
        set_up_insert_va: u32,
        halt_arg: i32,
        recursive_queued: QueuePos,
        finish_insert_va: u32,
    },
    AttackPositionProbe(AttackPositionReceipt),
    CheckCapture {
        building: ObjectKey,
        attacker: ObjectKey,
    },
    DelegateMoveTo {
        destination: Coord,
        queue: QueuePos,
        order: OrderKind,
        raw_tail: [i32; 6],
    },
    RetargetStrafe {
        member: ObjectKey,
        order_revision: u64,
        target: ObjectKey,
        target_uid: i16,
        field_0x3c: i32,
        domain_pass: i32,
    },
    ClearOrders {
        member: ObjectKey,
    },
    InstallCast {
        member: ObjectKey,
        spell: i32,
        queue: QueuePos,
        raw_first_four: [i32; 4],
        raw_last: i32,
    },
    InstallMove {
        member: ObjectKey,
        kind: OrderKind,
        destination: Coord,
        queue: QueuePos,
        raw_arg4: i32,
        raw_arg6: i32,
        raw_arg7_is_member_slot: bool,
        raw_tail: [i32; 2],
    },
    InstallAttack {
        member: ObjectKey,
        target: ObjectKey,
        queue: QueuePos,
        mandatory: i32,
        raw_last: i32,
    },
    MeleeTargetProbe {
        member: ObjectKey,
        receipt: MeleeTargetReceipt,
    },
    IncrementGroupOrderNum {
        before: i32,
        after: i32,
    },
}

fn finish(
    request: &AttackRequest,
    target: Option<ObjectKey>,
    steps: Vec<PlanStep>,
    exit: Exit,
) -> AttackPlan {
    let mut objects = request
        .group
        .members
        .iter()
        .map(|member| member.key)
        .collect::<Vec<_>>();
    if let Some(target) = target {
        if !objects.contains(&target) {
            objects.push(target);
        }
    }
    AttackPlan {
        group: request.group.key,
        target,
        steps,
        exit,
        revalidate_objects: objects,
    }
}

fn validate_attack_position(receipt: &AttackPositionReceipt) -> Result<(), PlanError> {
    if receipt.initial_in_range && receipt.called_find_attack_pos {
        return Err(PlanError::MissingAttackPositionReceipt);
    }
    if !receipt.initial_in_range && !receipt.called_find_attack_pos {
        return Err(PlanError::MissingAttackPositionReceipt);
    }
    if !receipt.called_find_attack_pos && !receipt.rng.is_empty() {
        return Err(PlanError::InvalidRngReceipt { draw_index: 0 });
    }
    for (draw_index, draw) in receipt.rng.iter().enumerate() {
        if draw.call_va != RANDOM_GET_VA_IN_FIND_ATTACK_POS
            || draw.rng_va != RANDOM_GET_VA
            || draw.low != 0
            || draw.high != 0xffff
            || !(0..=0xffff).contains(&draw.value)
        {
            return Err(PlanError::InvalidRngReceipt { draw_index });
        }
        if draw_index != 0 && receipt.rng[draw_index - 1].state_after != draw.state_before {
            return Err(PlanError::InvalidRngReceipt { draw_index });
        }
    }
    Ok(())
}

fn should_filter(member: &MemberSnapshot, ignore: i32) -> bool {
    ((ignore & 4) != 0 && member.type_is_defense)
        || ((ignore & 2) != 0 && member.is_special)
        || ((ignore & 1) != 0 && member.is_type_0x3a)
}

fn add_cast(steps: &mut Vec<PlanStep>, member: ObjectKey, spell: i32) {
    steps.push(PlanStep::InstallCast {
        member,
        spell,
        queue: QueuePos::New,
        raw_first_four: [-1; 4],
        raw_last: 0,
    });
}

fn add_move(steps: &mut Vec<PlanStep>, member: ObjectKey, destination: Coord, queue: QueuePos) {
    steps.push(PlanStep::InstallMove {
        member,
        kind: OrderKind::MoveTo,
        destination,
        queue,
        raw_arg4: 0,
        raw_arg6: 0,
        raw_arg7_is_member_slot: true,
        raw_tail: [-1, -1],
    });
}

fn add_attack(
    steps: &mut Vec<PlanStep>,
    member: ObjectKey,
    target: ObjectKey,
    queue: QueuePos,
    mandatory: i32,
) {
    steps.push(PlanStep::InstallAttack {
        member,
        target,
        queue,
        mandatory,
        raw_last: 1,
    });
}

/// Produce the complete retail-ordered action transaction without mutating the host.
///
/// The result deliberately contains call intents for `Group::kill`, `Build::check_capture`,
/// queue insertion, movement delegation, and `Unit::add_*_order`.  Those callees own their
/// mutations; this body owns their exact reachability, parameters, and chronology.
pub fn plan_action_attack(request: &AttackRequest) -> Result<AttackPlan, PlanError> {
    if let Some(boundary) = &request.first_unavailable {
        return Err(PlanError::External(boundary.clone()));
    }
    if request.group.num != request.group.members.len() as i32 {
        return Err(PlanError::TornGroupReceipt {
            num: request.group.num,
            members: request.group.members.len(),
        });
    }
    if !request.group.on_map {
        return Ok(finish(request, None, Vec::new(), Exit::GroupOffMap));
    }
    if request.ox < 0 {
        return Ok(finish(request, None, Vec::new(), Exit::NegativeTarget));
    }

    let mut steps = Vec::new();
    if request.scenario.ignore_orders && request.group.key.who < 8 {
        for &o in &request.scenario.prune_objects {
            if o >= 0 {
                steps.push(PlanStep::ScenarioKill {
                    o,
                    who: request.group.key.who,
                    arg2: 0,
                    arg3: 0,
                });
            }
        }
    }
    if request.group.num <= 0 {
        return Ok(finish(request, None, steps, Exit::EmptyGroup));
    }

    // vslot +0x14 => Group::action_begin 0x00714100.
    steps.push(PlanStep::SetGroupDisband(0));

    if request.group.buildings {
        let mut warned = false;
        for member in &request.group.members {
            if !member.is_build {
                steps.push(PlanStep::MixedBuildingUnitAssertion { member: member.key });
                continue;
            }
            steps.push(PlanStep::BuildingAttack {
                building: member.key,
                or_flags_0x60: 4,
                target_o_0x7c: request.ox as i16,
                target_who_0x81: request.whom as u8,
            });
            if request.group.is_local_player && !warned && member.building_warning_invalid_range {
                steps.push(PlanStep::PresentationAttackRangeWarning {
                    member: member.key,
                    string_offset: 0x837c,
                    sound_category: 0x40,
                });
                warned = true;
            }
        }
        return Ok(finish(
            request,
            request.target.as_ref().map(|target| target.key),
            steps,
            Exit::BuildingsHandled,
        ));
    }

    if request.queued == QueuePos::First {
        steps.push(PlanStep::QueueFirstDance {
            set_up_insert_va: GROUP_SET_UP_INSERT_VA,
            halt_arg: request.ignore,
            recursive_queued: QueuePos::New,
            finish_insert_va: GROUP_FINISH_INSERT_VA,
        });
        return Ok(finish(
            request,
            request.target.as_ref().map(|target| target.key),
            steps,
            Exit::QueueFirstDelegated,
        ));
    }

    let leader = match request.group.leader {
        Some(leader) => leader,
        None => {
            return Ok(finish(
                request,
                request.target.as_ref().map(|target| target.key),
                steps,
                Exit::NoLeader,
            ));
        }
    };
    let requested = ObjectKey {
        who: request.whom as u8,
        o: request.ox,
        revision: request.target.as_ref().map_or(0, |t| t.key.revision),
    };
    let target = request.target.as_ref().ok_or(PlanError::MissingTarget)?;
    if target.key.who != requested.who || target.key.o != requested.o {
        return Err(PlanError::TargetIdentityMismatch {
            requested,
            supplied: target.key,
        });
    }
    let attack_position = request
        .attack_position
        .as_ref()
        .ok_or(PlanError::MissingAttackPositionReceipt)?;
    validate_attack_position(attack_position)?;
    let expected_origin = if request.mandatory == 1 {
        request
            .group
            .mandatory_one_loc_to
            .unwrap_or(request.group.leader_position)
    } else {
        request.group.leader_position
    };
    if attack_position.origin != expected_origin {
        return Err(PlanError::MissingAttackPositionReceipt);
    }
    steps.push(PlanStep::AttackPositionProbe(attack_position.clone()));

    if target.is_build && target.is_active_build && target.capture_eligible {
        steps.push(PlanStep::CheckCapture {
            building: target.key,
            attacker: leader,
        });
        steps.push(PlanStep::DelegateMoveTo {
            destination: target.position,
            queue: request.queued,
            order: OrderKind::MoveTo,
            // args 4..10 at 0x007129CD..0x007129E2.
            raw_tail: [0, 0, 1, 1, -1, -1],
        });
        return Ok(finish(
            request,
            Some(target.key),
            steps,
            Exit::CaptureMoveDelegated,
        ));
    }

    let move_only = !target.seen_by_group;
    for domain_pass in 0..=2 {
        for member in &request.group.members {
            if !member.valid || !member.on_map || should_filter(member, request.ignore) {
                continue;
            }

            // This branch precedes the domain comparison.  Qualifying planes therefore
            // reach it in all three outer passes, exactly as the machine code does.
            let plane_strafe_path = member.is_plane
                && (!member.is_plane_vslot_is_default
                    || (member.type_domain == 2 && (member.type_flags_0x2b4 & 0x20) == 0));
            if plane_strafe_path {
                let available_mana = member
                    .mana
                    .wrapping_sub(i32::from(member.mana_reserve_0x96))
                    .max(0);
                if available_mana != 0 && member.order_kind == Some(OrderKind::Strafe) {
                    if let Some(strafe) = member.strafe {
                        steps.push(PlanStep::RetargetStrafe {
                            member: member.key,
                            order_revision: strafe.order_revision,
                            target: target.key,
                            target_uid: target.uid_0x30,
                            field_0x3c: 0,
                            domain_pass,
                        });
                    }
                }
                continue;
            }
            if member.type_domain != domain_pass {
                continue;
            }

            if let Some(existing) = member.existing_attack {
                if existing.target.who == target.key.who
                    && existing.target.o == target.key.o
                    && existing.active_byte_0x1c
                    && existing.unit_counter_0xd8 <= 2
                    && !existing.primary_in_range
                {
                    if existing.unit_counter_0xd8 == 1
                        || (existing.current_order_is_attack && existing.payload_in_range)
                    {
                        continue;
                    }
                }
            }

            if member.packing_or_unpacking && request.queued == QueuePos::New {
                if move_only {
                    if target.is_build && member.target_in_range {
                        steps.push(PlanStep::CheckCapture {
                            building: target.key,
                            attacker: member.key,
                        });
                    }
                    if member.packing {
                        add_cast(&mut steps, member.key, SPELL_PACK);
                    }
                    add_move(&mut steps, member.key, target.position, QueuePos::Last);
                } else {
                    steps.push(PlanStep::ClearOrders { member: member.key });
                    if member.packing {
                        if !member.target_in_range {
                            add_cast(&mut steps, member.key, SPELL_PACK);
                        }
                    } else if member.target_in_range {
                        add_cast(&mut steps, member.key, SPELL_DEPLOY);
                    }
                    add_attack(
                        &mut steps,
                        member.key,
                        target.key,
                        QueuePos::Last,
                        request.mandatory,
                    );
                }
                continue;
            }

            if target.is_build {
                if move_only {
                    if member.target_in_range {
                        steps.push(PlanStep::CheckCapture {
                            building: target.key,
                            attacker: member.key,
                        });
                    }
                    add_move(&mut steps, member.key, target.position, request.queued);
                } else {
                    add_attack(
                        &mut steps,
                        member.key,
                        target.key,
                        request.queued,
                        request.mandatory,
                    );
                }
                continue;
            }

            if move_only {
                add_move(&mut steps, member.key, target.position, request.queued);
            }
            let attack_target = if request.mandatory == 0 {
                let receipt = member
                    .melee_target
                    .ok_or(PlanError::MissingMeleeTargetReceipt { member: member.key })?;
                let expected_class = if target.is_wallbuild { 2 } else { 1 };
                if receipt.target_class_arg != expected_class
                    || receipt.requested_distance > receipt.world_dimension_times_0x240
                {
                    return Err(PlanError::MissingMeleeTargetReceipt { member: member.key });
                }
                steps.push(PlanStep::MeleeTargetProbe {
                    member: member.key,
                    receipt,
                });
                receipt.result.unwrap_or(target.key)
            } else {
                target.key
            };
            add_attack(
                &mut steps,
                member.key,
                attack_target,
                request.queued,
                request.mandatory,
            );
        }
    }

    steps.push(PlanStep::IncrementGroupOrderNum {
        before: request.group.order_num,
        after: request.group.order_num.wrapping_add(1),
    });
    Ok(finish(
        request,
        Some(target.key),
        steps,
        Exit::MembersHandled,
    ))
}

/// Kinds that `Unit::add_move_facing_order` can allocate.  All three call sites owned by
/// `action_attack` pass literal selector `1`, so only `MOVE_TO` is reachable here; the other
/// variants belong to the delegated movement owner and must not be inferred from intent.
pub const MOVE_FAMILY_KINDS: [OrderKind; 4] = [
    OrderKind::MoveTo,
    OrderKind::AttackTo,
    OrderKind::ExploreTo,
    OrderKind::FleeTo,
];
