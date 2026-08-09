//! Exact deterministic body of `Army::do_forming(int)` at retail `0x006F43C0`.
//!
//! This file is intentionally not exported yet.  It is a source-frozen integration unit
//! for tick step 13: the shared `armies.rs` owner and `tick.rs` scheduler are left alone so
//! this recovery cannot race their active lanes.  The narrow adapter map is recorded in
//! `docs/mechanics/army-do-forming-step13.md`.
//!
//! Evidence is instruction-level (`riseofnations.exe`, 720 bytes, army.cpp:3045-3141),
//! not a retail differential oracle.  Missing host state stops at the exact read/write
//! boundary and never invents a negative fact.

/// `Army::do_forming(int)`.
pub const RETAIL_VA: u32 = 0x006F_43C0;
/// PDB procedure length, ending immediately before `Army::do_transporting`.
pub const RETAIL_SIZE: u32 = 720;
/// `ArmyData::list` is `int[16]` at `+0x54`.
pub const ARMY_GROUP_CAPACITY: usize = 16;
/// Formation spacing passed through retail `sinx`/`cosx`.
pub const GROUP_SPACING: i32 = 0x180;
/// Distance per live-prefix entry used by the initial `project` call.
pub const PROJECT_DISTANCE_PER_GROUP: i32 = 0xC0;
/// WCoord-to-Coord scale used to center the muster tile.
pub const WCOORD_SCALE: i32 = 0x300;
/// Center of one WCoord cell.
pub const WCOORD_HALF: i32 = 0x180;

/// The `ArmyData` fields read by `Army::do_forming`.
///
/// Offsets are pinned in the field comments.  The body does not write `ArmyData`; it
/// writes the member Groups and emits Group actions through [`FormingHost`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormingArmy {
    /// `+0x02`, sign-extended before it is stored into `GroupData::army +0x08`.
    pub army: i16,
    /// `+0x24`; a navy bypasses the land-target validity branch.
    pub navy: i32,
    /// `+0x30/+0x34`.
    pub target_o: i32,
    pub target_who: i32,
    /// `+0x48/+0x4C`, WCoord.
    pub muster_x: i32,
    pub muster_y: i32,
    /// `+0x50`, a 32-bit binary angle.
    pub muster_angle: i32,
    /// `+0x54`.
    pub list: [i32; ARMY_GROUP_CAPACITY],
    /// `+0x94/+0x96`.
    pub who: i16,
    pub num_groups: i16,
}

impl Default for FormingArmy {
    fn default() -> Self {
        Self {
            army: 0,
            navy: 0,
            target_o: -1,
            target_who: -1,
            muster_x: 0,
            muster_y: 0,
            muster_angle: 0,
            list: [-1; ARMY_GROUP_CAPACITY],
            who: 0,
            num_groups: 0,
        }
    }
}

/// Exact positional arguments issued to `Group::action_move_to` `0x0070FBA0`.
///
/// The names after `queue_pos` stay positional because the PDB exposes only four `int`
/// parameters.  Freezing their values is safer than assigning guessed semantics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FormingMoveOrder {
    pub group: i32,
    pub x: i32,
    pub y: i32,
    pub queue_pos: i32,
    pub arg_4: i32,
    pub angle: i32,
    pub order_index: i32,
    pub arg_7: i32,
    pub arg_8: i32,
    pub arg_9: i32,
    pub arg_10: i32,
}

impl FormingMoveOrder {
    fn retail(group: i32, x: i32, y: i32, angle: i32) -> Self {
        Self {
            group,
            x,
            y,
            queue_pos: 2,
            arg_4: 1,
            angle,
            order_index: 2,
            arg_7: 1,
            arg_8: -1,
            arg_9: -1,
            arg_10: 0,
        }
    }
}

/// Observable arguments issued to `Group::action_siege_attack_to` `0x0070D830`.
///
/// Retail reads the two Coord arguments and the final angle.  Its middle two formal
/// parameters are filled from stale stack slots at this call site and the 2,037-byte
/// callee never reads them; representing arbitrary stack residue as simulation state
/// would be less faithful than omitting those unobservable values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FormingSiegeOrder {
    pub group: i32,
    pub x: i32,
    pub y: i32,
    pub angle: i32,
}

/// An authoritative target type lookup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetTypeFact {
    /// The target object's virtual `ObjectData` has a negative `type_index +0x72`.
    NoType,
    /// Low type flags from the pointed-to Type row at `+0x04`.
    Flags(u8),
}

/// A host fact or mutation seam that was not installed.
///
/// `Missing` is not a retail return value.  It is the headless fail-closed boundary: all
/// earlier mutations remain in retail order, and no later query or write is attempted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MissingFact {
    Engagement,
    Projection,
    ArmyList {
        cursor: usize,
    },
    Group {
        cursor: usize,
        group: i32,
    },
    GroupArmyWrite {
        cursor: usize,
        group: i32,
    },
    LeaderIdentity {
        who: usize,
    },
    Diplomacy {
        who: usize,
        other: i32,
    },
    SetupRelation {
        target_who: i32,
        owner_identity: i32,
    },
    LeaderAge {
        who: usize,
    },
    MobileCount,
    StanceWrite {
        stance: i32,
    },
    BuildingSearch {
        group: i32,
    },
    EnemyRelation {
        who: usize,
        other: i32,
    },
    TargetType {
        who: i32,
        object: i32,
    },
    MoveOrder {
        group: i32,
    },
    SiegeOrder {
        group: i32,
    },
    FormationStep,
}

/// Counted proof of how far one call progressed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FormingTrace {
    pub groups_examined: u32,
    pub live_groups: u32,
    pub group_army_writes: u32,
    pub stance_three_writes: u32,
    pub stance_zero_writes: u32,
    pub move_orders: u32,
    pub siege_orders: u32,
    pub formation_steps: u32,
}

/// The three possible exits from the recovered body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormingExit {
    /// Retail return 0: `Army::is_engaged` returned non-zero at `0x006F43C9`.
    Engaged(FormingTrace),
    /// Retail return 1: the complete live prefix was visited.
    Formed(FormingTrace),
    /// Headless-only fail-closed stop; there is no fabricated retail return value.
    Missing {
        fact: MissingFact,
        trace: FormingTrace,
    },
}

impl FormingExit {
    pub fn trace(self) -> FormingTrace {
        match self {
            Self::Engaged(trace) | Self::Formed(trace) | Self::Missing { trace, .. } => trace,
        }
    }

    pub fn retail_return(self) -> Option<i32> {
        match self {
            Self::Engaged(_) => Some(0),
            Self::Formed(_) => Some(1),
            Self::Missing { .. } => None,
        }
    }
}

/// External state/calls reached by `Army::do_forming`.
///
/// Queries return `None` when the adapter lacks the authoritative fact.  Mutations return
/// `false` when their target owner is absent.  The driver maps every absence to a typed
/// [`MissingFact`] at the exact call site instead of treating it as false/empty.
pub trait FormingHost {
    /// `Army::is_engaged` `0x006F56D0`, the first call in the function.  That callee
    /// begins with `Army::normalize`, which mutates derived Army fields and can reorder
    /// `list`; the adapter must apply those writes and refresh this view before returning.
    fn is_engaged(&mut self, army: &mut FormingArmy) -> Option<bool>;
    /// `project` `0x0092CF40`: center plus exact integer `sinx/cosx` projection.
    fn project(&mut self, x: i32, y: i32, angle: i32, distance: i32) -> Option<(i32, i32)>;

    /// `GroupData::num +0x0C`; exactly zero skips the group.  Negative is still live.
    fn group_num(&mut self, group: i32) -> Option<i32>;
    /// Store sign-extended `ArmyData::army` into `GroupData::army +0x08`.
    fn set_group_army(&mut self, group: i32, army: i32) -> bool;

    /// `LeaderData +0x08`, distinct from `ArmyData::who` even though valid retail state
    /// normally makes them equal.
    fn leader_identity(&mut self, who: usize) -> Option<i32>;
    /// `LeaderData::diplo[other]` at `Leader +0x74`.
    fn diplomacy(&mut self, who: usize, other: i32) -> Option<i32>;
    /// The mirrored setup/player relation read after a non-zero Leader diplomacy value.
    fn setup_relation(&mut self, target_who: i32, owner_identity: i32) -> Option<i32>;
    /// Decoded `Leader economy +0xE8 ^ 0x63187`.
    fn leader_age(&mut self, who: usize) -> Option<i32>;
    /// `Army::count(4, 0)`; it is deliberately not cached across retail call sites.
    fn count_mobile(&mut self, army: &FormingArmy) -> Option<i32>;
    /// `Army::set_stance` `0x006F8750`, which fans to every member group.
    fn set_stance(&mut self, army: &FormingArmy, stance: i32) -> bool;

    /// Sign of `ObjectsData::find_building(x,y,3,who,0x300,0x200,9,0,0)`.
    fn building_found(&mut self, x: i32, y: i32, who: usize) -> Option<bool>;
    /// `LeaderData::is_enemy` `0x006EBAA0`.
    fn is_enemy(&mut self, who: usize, other: i32) -> Option<bool>;
    /// Target ObjectData virtual `+0xAC`, `type_index +0x72`, then Type flags `+0x04`.
    fn target_type(&mut self, who: i32, object: i32) -> Option<TargetTypeFact>;

    fn issue_move(&mut self, order: FormingMoveOrder) -> bool;
    fn issue_siege(&mut self, order: FormingSiegeOrder) -> bool;
    /// Advance the formation cursor by `sinx(angle,0x180), -cosx(angle,0x180)`.
    fn advance_cursor(&mut self, x: i32, y: i32, angle: i32, spacing: i32) -> Option<(i32, i32)>;
}

/// Recover `Army::do_forming(int)` in retail mutation and short-circuit order.
///
/// The formal `int` argument is unused by all 720 shipped bytes, so it is intentionally
/// absent.  Arithmetic uses wrapping operations wherever x86 `imul/add` can overflow.
pub fn do_forming<H: FormingHost + ?Sized>(army: &mut FormingArmy, host: &mut H) -> FormingExit {
    let mut trace = FormingTrace::default();

    macro_rules! need {
        ($value:expr, $missing:expr) => {
            match $value {
                Some(value) => value,
                None => {
                    return FormingExit::Missing {
                        fact: $missing,
                        trace,
                    }
                }
            }
        };
    }
    macro_rules! write {
        ($value:expr, $missing:expr) => {
            if !$value {
                return FormingExit::Missing {
                    fact: $missing,
                    trace,
                };
            }
        };
    }

    // 0x006F43C9..0x006F43D8: this return precedes every other read and write.
    if need!(host.is_engaged(army), MissingFact::Engagement) {
        return FormingExit::Engaged(trace);
    }

    // 0x006F43DB..0x006F4420.  The projection still runs when num_groups <= 0.
    let center_x = army
        .muster_x
        .wrapping_mul(WCOORD_SCALE)
        .wrapping_add(WCOORD_HALF);
    let center_y = army
        .muster_y
        .wrapping_mul(WCOORD_SCALE)
        .wrapping_add(WCOORD_HALF);
    let distance = i32::from(army.num_groups).wrapping_mul(PROJECT_DISTANCE_PER_GROUP);
    let (mut x, mut y) = need!(
        host.project(center_x, center_y, army.muster_angle, distance),
        MissingFact::Projection
    );

    let who = army.who as usize;
    let mut cursor = 0usize;
    while (cursor as i32) < i32::from(army.num_groups) {
        trace.groups_examined = trace.groups_examined.wrapping_add(1);
        let group = match army.list.get(cursor) {
            Some(group) => *group,
            None => {
                return FormingExit::Missing {
                    fact: MissingFact::ArmyList { cursor },
                    trace,
                }
            }
        };

        // Negative sentinels and exactly-empty Groups consume no formation spacing.
        if group >= 0 {
            let group_num = need!(host.group_num(group), MissingFact::Group { cursor, group });
            if group_num != 0 {
                trace.live_groups = trace.live_groups.wrapping_add(1);

                // 0x006F4467: this is the first loop-local mutation for a live group;
                // is_engaged's normalize has already committed its own prefix writes.
                write!(
                    host.set_group_army(group, i32::from(army.army)),
                    MissingFact::GroupArmyWrite { cursor, group }
                );
                trace.group_army_writes = trace.group_army_writes.wrapping_add(1);

                // 0x006F446E..0x006F44D9.  Preserve the two relation short-circuits and
                // call count(4,0) afresh at this exact site.
                if army.target_who >= 0 {
                    let owner_identity = need!(
                        host.leader_identity(who),
                        MissingFact::LeaderIdentity { who }
                    );
                    if army.target_who != owner_identity {
                        let diplo = need!(
                            host.diplomacy(who, army.target_who),
                            MissingFact::Diplomacy {
                                who,
                                other: army.target_who,
                            }
                        );
                        let relation_zero = if diplo == 0 {
                            true
                        } else {
                            need!(
                                host.setup_relation(army.target_who, owner_identity),
                                MissingFact::SetupRelation {
                                    target_who: army.target_who,
                                    owner_identity,
                                }
                            ) == 0
                        };
                        if relation_zero && army.navy == 0 {
                            let age = need!(host.leader_age(who), MissingFact::LeaderAge { who });
                            if age < 4
                                && need!(host.count_mobile(army), MissingFact::MobileCount) == 0
                            {
                                write!(
                                    host.set_stance(army, 3),
                                    MissingFact::StanceWrite { stance: 3 }
                                );
                                trace.stance_three_writes =
                                    trace.stance_three_writes.wrapping_add(1);
                            }
                        }
                    }
                }

                // 0x006F44DE..0x006F452C.  A found building blocks only when a fresh
                // count(4,0) is non-zero.
                let found = need!(
                    host.building_found(x, y, who),
                    MissingFact::BuildingSearch { group }
                );
                let blocked = if found {
                    need!(host.count_mobile(army), MissingFact::MobileCount) != 0
                } else {
                    false
                };

                let move_to = if blocked {
                    false
                } else if army.navy != 0 {
                    true
                } else {
                    // 0x006F4534..0x006F45A5.  A negative target owner is still passed
                    // to is_enemy before the negative target object is tested.
                    let enemy_ok = if army.target_who == i32::from(army.who) {
                        true
                    } else {
                        need!(
                            host.is_enemy(who, army.target_who),
                            MissingFact::EnemyRelation {
                                who,
                                other: army.target_who,
                            }
                        )
                    };
                    if !enemy_ok || army.target_o < 0 {
                        false
                    } else {
                        match need!(
                            host.target_type(army.target_who, army.target_o),
                            MissingFact::TargetType {
                                who: army.target_who,
                                object: army.target_o,
                            }
                        ) {
                            TargetTypeFact::NoType => false,
                            TargetTypeFact::Flags(flags) => flags & 3 == 3,
                        }
                    }
                };

                if move_to {
                    // 0x006F45A7 precedes construction of the move action.
                    write!(
                        host.set_stance(army, 0),
                        MissingFact::StanceWrite { stance: 0 }
                    );
                    trace.stance_zero_writes = trace.stance_zero_writes.wrapping_add(1);
                    write!(
                        host.issue_move(FormingMoveOrder::retail(group, x, y, army.muster_angle,)),
                        MissingFact::MoveOrder { group }
                    );
                    trace.move_orders = trace.move_orders.wrapping_add(1);
                } else {
                    write!(
                        host.issue_siege(FormingSiegeOrder {
                            group,
                            x,
                            y,
                            angle: army.muster_angle,
                        }),
                        MissingFact::SiegeOrder { group }
                    );
                    trace.siege_orders = trace.siege_orders.wrapping_add(1);
                }

                // 0x006F45E9..0x006F4662: only a live, actioned group consumes spacing.
                (x, y) = need!(
                    host.advance_cursor(x, y, army.muster_angle, GROUP_SPACING),
                    MissingFact::FormationStep
                );
                trace.formation_steps = trace.formation_steps.wrapping_add(1);
            }
        }
        cursor += 1;
    }

    FormingExit::Formed(trace)
}
