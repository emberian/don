//! `Object::do_damage`'s first post-`take_damage` world transaction.
//!
//! This is the address-bounded slice `0x0064BA18..0x0064BBFA` plus the adjacent
//! survivor-only `Armies::emergency` gate at `0x0064BBFD..0x0064BC17`.  It deliberately
//! stops before the special-hit, containment/ejection and capture arms beginning at
//! `0x0064BC17`.
//!
//! Retail reads infallible globals. A replay host resolves every fact needed by the selected
//! branch into a [`PostDamagePlan`] before mutating leader state. Missing facts therefore
//! fail closed with no partial mutation. Once resolved, the mutation order is instruction
//! order, including `Build::plunder` between score changes and the frame-rate counters.

use super::DamageOutcome;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObjectKey {
    pub who: u8,
    pub o: i16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VictimClass {
    /// The virtual at `0x0064BA34` returned non-zero.
    Building,
    /// A non-zero `take_damage` result returns immediately at `0x0064BA39`.
    Other,
}

/// Infallible retail-global reads expressed as optional host facts.
pub trait DamageWorldFacts {
    fn victim_class(&self, victim: ObjectKey) -> Option<VictimClass>;
    /// Type virtual `+0x80`, called with the victim owner at `0x0064BADF`.
    fn victim_score_value(&self, victim: ObjectKey) -> Option<i32>;
    /// `leaders[attacker].is_ally(victim.who)`.
    fn attacker_allied_to_victim(&self, attacker: u8, victim: u8) -> Option<bool>;
    /// The attacker's `ObjectTypeData +0x218`; value 2 suppresses plunder.
    fn attacker_domain(&self, attacker: ObjectKey) -> Option<i32>;
    /// The deliberately opposite-direction diplomacy read at `0x0064BB76`.
    fn victim_allied_to_attacker(&self, victim: u8, attacker: u8) -> Option<bool>;
    /// The second `BuildData::is_build` virtual (`vt + 0x20`) at `0x0064BB8B`.
    fn victim_is_build_second_read(&self, victim: ObjectKey) -> Option<bool>;
    /// `flags&1 && is_build() && is_active()` cached at `0x0064A803..0x0064A82F`;
    /// selects rate increment 5 vs 2. `is_active` is the folded vtable entry at `+0x4C`.
    fn victim_was_active_build(&self, victim: ObjectKey) -> Option<bool>;
    /// Local `[ebp-0x2C]`, set by presentation-entangled hit-report arms.
    fn emergency_gate(&self, victim: ObjectKey) -> Option<bool>;
    /// `LeaderData::leader_flags`; bit `0x4` suppresses `Armies::emergency`.
    fn victim_leader_flags(&self, victim: u8) -> Option<u32>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MissingDamageFact {
    VictimClass,
    VictimScoreValue,
    AttackerDiplomacy,
    AttackerDomain,
    VictimDiplomacy,
    VictimSecondBuildRead,
    VictimActiveBuildState,
    EmergencyGate,
    VictimLeaderFlags,
}

/// Combat-owned `LeaderData` fields touched by this transaction.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LeaderCombatMutationState {
    /// `+0x44`.
    pub score_combat: i32,
    /// `+0x818`.
    pub buildings_razed: i32,
    /// `+0xA58`.
    pub deaths_current_frame: u16,
    /// `+0xA5A`.
    pub kills_current_frame: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlunderRequest {
    pub victim: ObjectKey,
    pub attacker_who: u8,
}

/// Infallible live adapter for the retail call at `0x0064BBB0`.
pub trait PlunderWorld {
    fn plunder(&mut self, request: PlunderRequest);
}

/// Infallible live adapter for `Armies::emergency(who)` at `0x0064BC12`.
pub trait EmergencyWorld {
    fn armies_emergency(&mut self, who: u8);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CombatScoreMutation {
    /// Enemy: wrapping add, no clamp.
    Reward(i32),
    /// Ally: wrapping subtract, then clamp negative result to zero.
    AlliedPenalty(i32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildingDeathPlan {
    pub attacker: ObjectKey,
    pub victim: ObjectKey,
    pub score: CombatScoreMutation,
    pub plunder: Option<PlunderRequest>,
    pub rate_increment: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PostDamagePlan {
    Survived { emergency_who: Option<u8> },
    RemovedNonBuilding,
    RemovedBuilding(BuildingDeathPlan),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildingDeathMutation {
    BuildingsRazed,
    CombatScore,
    Plunder,
    KillsCurrentFrame,
    DeathsCurrentFrame,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildingDeathReceipt {
    pub mutations: Vec<BuildingDeathMutation>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PostDamageReceipt {
    Survived { emergency_called: bool },
    RemovedNonBuilding,
    RemovedBuilding(BuildingDeathReceipt),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DamageWorldApplyError {
    MissingAttackerLeader(u8),
    MissingVictimLeader(u8),
}

/// Resolve the first post-`take_damage` branch without mutating world state.
///
/// `secondary` is `Object::do_damage` argument 7 (`[ebp+0x20]`). Reads preserve retail's
/// short-circuit gates: a domain-2 attacker does not require the later diplomacy/capability
/// facts.
pub fn plan_post_damage<F: DamageWorldFacts + ?Sized>(
    facts: &F,
    attacker: ObjectKey,
    victim: ObjectKey,
    outcome: DamageOutcome,
    secondary: i32,
) -> Result<PostDamagePlan, MissingDamageFact> {
    if outcome == DamageOutcome::Survived {
        let gate = facts
            .emergency_gate(victim)
            .ok_or(MissingDamageFact::EmergencyGate)?;
        if !gate {
            return Ok(PostDamagePlan::Survived {
                emergency_who: None,
            });
        }
        let flags = facts
            .victim_leader_flags(victim.who)
            .ok_or(MissingDamageFact::VictimLeaderFlags)?;
        return Ok(PostDamagePlan::Survived {
            emergency_who: (flags & 0x4 == 0).then_some(victim.who),
        });
    }

    let class = facts
        .victim_class(victim)
        .ok_or(MissingDamageFact::VictimClass)?;
    if class != VictimClass::Building {
        return Ok(PostDamagePlan::RemovedNonBuilding);
    }

    let allied = facts
        .attacker_allied_to_victim(attacker.who, victim.who)
        .ok_or(MissingDamageFact::AttackerDiplomacy)?;
    let score_value = facts
        .victim_score_value(victim)
        .ok_or(MissingDamageFact::VictimScoreValue)?;
    // Signed magic divide at `0x0064BAEF..0x0064BB00`: truncation toward zero.
    let score_tenth = score_value / 10;
    let score = if allied {
        CombatScoreMutation::AlliedPenalty(score_tenth)
    } else {
        CombatScoreMutation::Reward(score_tenth)
    };

    let domain = facts
        .attacker_domain(attacker)
        .ok_or(MissingDamageFact::AttackerDomain)?;
    let plunder = if domain == 2 || secondary != 0 {
        None
    } else {
        let victim_allied = facts
            .victim_allied_to_attacker(victim.who, attacker.who)
            .ok_or(MissingDamageFact::VictimDiplomacy)?;
        if victim_allied {
            None
        } else {
            facts
                .victim_is_build_second_read(victim)
                .ok_or(MissingDamageFact::VictimSecondBuildRead)?
                .then_some(PlunderRequest {
                    victim,
                    attacker_who: attacker.who,
                })
        }
    };

    let high_rate = facts
        .victim_was_active_build(victim)
        .ok_or(MissingDamageFact::VictimActiveBuildState)?;
    Ok(PostDamagePlan::RemovedBuilding(BuildingDeathPlan {
        attacker,
        victim,
        score,
        plunder,
        rate_increment: if high_rate { 5 } else { 2 },
    }))
}

/// Apply a resolved lethal-building transaction in retail instruction order.
pub fn apply_building_death<W: PlunderWorld + ?Sized>(
    plan: BuildingDeathPlan,
    leaders: &mut [LeaderCombatMutationState],
    world: &mut W,
) -> Result<BuildingDeathReceipt, DamageWorldApplyError> {
    let attacker_i = usize::from(plan.attacker.who);
    let victim_i = usize::from(plan.victim.who);
    // Admission is deliberately complete before the first mutation.
    if attacker_i >= leaders.len() {
        return Err(DamageWorldApplyError::MissingAttackerLeader(
            plan.attacker.who,
        ));
    }
    if victim_i >= leaders.len() {
        return Err(DamageWorldApplyError::MissingVictimLeader(plan.victim.who));
    }

    let mut mutations = Vec::with_capacity(5);
    leaders[attacker_i].buildings_razed = leaders[attacker_i].buildings_razed.wrapping_add(1);
    mutations.push(BuildingDeathMutation::BuildingsRazed);

    let old_score = leaders[attacker_i].score_combat;
    leaders[attacker_i].score_combat = match plan.score {
        CombatScoreMutation::Reward(value) => old_score.wrapping_add(value),
        CombatScoreMutation::AlliedPenalty(value) => old_score.wrapping_sub(value).max(0),
    };
    mutations.push(BuildingDeathMutation::CombatScore);

    if let Some(request) = plan.plunder {
        world.plunder(request);
        mutations.push(BuildingDeathMutation::Plunder);
    }

    leaders[attacker_i].kills_current_frame = leaders[attacker_i]
        .kills_current_frame
        .wrapping_add(plan.rate_increment);
    mutations.push(BuildingDeathMutation::KillsCurrentFrame);
    leaders[victim_i].deaths_current_frame = leaders[victim_i]
        .deaths_current_frame
        .wrapping_add(plan.rate_increment);
    mutations.push(BuildingDeathMutation::DeathsCurrentFrame);

    Ok(BuildingDeathReceipt { mutations })
}

/// Execute a fully resolved post-damage plan.
///
/// This is the composed public transaction: the survivor arm can issue the synchronous
/// same-frame army rethink, while the removed-building arm applies its exact ordered leader
/// and plunder mutations. No further host fact reads occur here.
pub fn apply_post_damage<W: PlunderWorld + EmergencyWorld + ?Sized>(
    plan: PostDamagePlan,
    leaders: &mut [LeaderCombatMutationState],
    world: &mut W,
) -> Result<PostDamageReceipt, DamageWorldApplyError> {
    match plan {
        PostDamagePlan::Survived { emergency_who } => {
            if let Some(who) = emergency_who {
                world.armies_emergency(who);
            }
            Ok(PostDamageReceipt::Survived {
                emergency_called: emergency_who.is_some(),
            })
        }
        PostDamagePlan::RemovedNonBuilding => Ok(PostDamageReceipt::RemovedNonBuilding),
        PostDamagePlan::RemovedBuilding(plan) => {
            apply_building_death(plan, leaders, world).map(PostDamageReceipt::RemovedBuilding)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug)]
    struct Facts {
        class: Option<VictimClass>,
        score: Option<i32>,
        attacker_allied: Option<bool>,
        domain: Option<i32>,
        victim_allied: Option<bool>,
        second_is_build: Option<bool>,
        active_build: Option<bool>,
        emergency: Option<bool>,
        leader_flags: Option<u32>,
    }

    impl Default for Facts {
        fn default() -> Self {
            Self {
                class: Some(VictimClass::Building),
                score: Some(99),
                attacker_allied: Some(false),
                domain: Some(0),
                victim_allied: Some(false),
                second_is_build: Some(true),
                active_build: Some(false),
                emergency: Some(true),
                leader_flags: Some(0),
            }
        }
    }

    impl DamageWorldFacts for Facts {
        fn victim_class(&self, _: ObjectKey) -> Option<VictimClass> {
            self.class
        }
        fn victim_score_value(&self, _: ObjectKey) -> Option<i32> {
            self.score
        }
        fn attacker_allied_to_victim(&self, _: u8, _: u8) -> Option<bool> {
            self.attacker_allied
        }
        fn attacker_domain(&self, _: ObjectKey) -> Option<i32> {
            self.domain
        }
        fn victim_allied_to_attacker(&self, _: u8, _: u8) -> Option<bool> {
            self.victim_allied
        }
        fn victim_is_build_second_read(&self, _: ObjectKey) -> Option<bool> {
            self.second_is_build
        }
        fn victim_was_active_build(&self, _: ObjectKey) -> Option<bool> {
            self.active_build
        }
        fn emergency_gate(&self, _: ObjectKey) -> Option<bool> {
            self.emergency
        }
        fn victim_leader_flags(&self, _: u8) -> Option<u32> {
            self.leader_flags
        }
    }

    struct World {
        plunders: Vec<PlunderRequest>,
        emergencies: Vec<u8>,
    }

    impl Default for World {
        fn default() -> Self {
            Self {
                plunders: Vec::new(),
                emergencies: Vec::new(),
            }
        }
    }

    impl PlunderWorld for World {
        fn plunder(&mut self, request: PlunderRequest) {
            self.plunders.push(request);
        }
    }

    impl EmergencyWorld for World {
        fn armies_emergency(&mut self, who: u8) {
            self.emergencies.push(who);
        }
    }

    const ATTACKER: ObjectKey = ObjectKey { who: 1, o: 17 };
    const VICTIM: ObjectKey = ObjectKey { who: 3, o: 41 };

    fn lethal(facts: &Facts, secondary: i32) -> Result<PostDamagePlan, MissingDamageFact> {
        plan_post_damage(
            facts,
            ATTACKER,
            VICTIM,
            DamageOutcome::Died { overflow: 7 },
            secondary,
        )
    }

    #[test]
    fn lethal_non_building_short_circuits_every_later_fact() {
        let facts = Facts {
            class: Some(VictimClass::Other),
            score: None,
            attacker_allied: None,
            domain: None,
            victim_allied: None,
            second_is_build: None,
            active_build: None,
            ..Facts::default()
        };
        assert_eq!(lethal(&facts, 0), Ok(PostDamagePlan::RemovedNonBuilding));
    }

    #[test]
    fn missing_fact_rejects_during_the_pure_planning_phase() {
        let facts = Facts {
            victim_allied: None,
            ..Facts::default()
        };
        assert_eq!(lethal(&facts, 0), Err(MissingDamageFact::VictimDiplomacy));
    }

    #[test]
    fn enemy_building_mutates_in_exact_order_and_wraps_retail_fields() {
        let facts = Facts {
            active_build: Some(true),
            ..Facts::default()
        };
        let PostDamagePlan::RemovedBuilding(plan) = lethal(&facts, 0).unwrap() else {
            panic!("building plan");
        };
        assert_eq!(plan.score, CombatScoreMutation::Reward(9));
        assert_eq!(plan.rate_increment, 5);

        let mut leaders = vec![LeaderCombatMutationState::default(); 4];
        leaders[1].score_combat = 10;
        leaders[1].buildings_razed = i32::MAX;
        leaders[1].kills_current_frame = u16::MAX - 2;
        leaders[3].deaths_current_frame = u16::MAX - 1;
        let mut world = World::default();
        let receipt = apply_building_death(plan, &mut leaders, &mut world).unwrap();

        assert_eq!(leaders[1].score_combat, 19);
        assert_eq!(leaders[1].buildings_razed, i32::MIN);
        assert_eq!(leaders[1].kills_current_frame, 2);
        assert_eq!(leaders[3].deaths_current_frame, 3);
        assert_eq!(
            world.plunders,
            vec![PlunderRequest {
                victim: VICTIM,
                attacker_who: 1
            }]
        );
        assert_eq!(
            receipt.mutations,
            vec![
                BuildingDeathMutation::BuildingsRazed,
                BuildingDeathMutation::CombatScore,
                BuildingDeathMutation::Plunder,
                BuildingDeathMutation::KillsCurrentFrame,
                BuildingDeathMutation::DeathsCurrentFrame,
            ]
        );
    }

    #[test]
    fn alliance_reads_are_directional_and_allied_score_clamps() {
        let facts = Facts {
            score: Some(109),
            attacker_allied: Some(true),
            victim_allied: Some(false),
            ..Facts::default()
        };
        let PostDamagePlan::RemovedBuilding(plan) = lethal(&facts, 0).unwrap() else {
            panic!("building plan");
        };
        assert_eq!(plan.score, CombatScoreMutation::AlliedPenalty(10));
        assert!(
            plan.plunder.is_some(),
            "opposite diplomacy read must stay distinct"
        );
        let mut leaders = vec![LeaderCombatMutationState::default(); 4];
        leaders[1].score_combat = 7;
        let mut world = World::default();
        apply_building_death(plan, &mut leaders, &mut world).unwrap();
        assert_eq!(leaders[1].score_combat, 0);
        assert_eq!(world.plunders.len(), 1);
    }

    #[test]
    fn plunder_gates_short_circuit_in_retail_order() {
        let no_tail = Facts {
            victim_allied: None,
            second_is_build: None,
            ..Facts::default()
        };
        let PostDamagePlan::RemovedBuilding(p) = lethal(
            &Facts {
                domain: Some(2),
                ..no_tail.clone()
            },
            0,
        )
        .unwrap() else {
            panic!("building plan");
        };
        assert_eq!(p.plunder, None);

        let PostDamagePlan::RemovedBuilding(p) = lethal(&no_tail, 1).unwrap() else {
            panic!("building plan");
        };
        assert_eq!(p.plunder, None);

        let PostDamagePlan::RemovedBuilding(p) = lethal(
            &Facts {
                victim_allied: Some(true),
                second_is_build: None,
                ..Facts::default()
            },
            0,
        )
        .unwrap() else {
            panic!("building plan");
        };
        assert_eq!(p.plunder, None);
    }

    #[test]
    fn signed_score_division_truncates_toward_zero() {
        let facts = Facts {
            score: Some(-109),
            ..Facts::default()
        };
        let PostDamagePlan::RemovedBuilding(plan) = lethal(&facts, 0).unwrap() else {
            panic!("building plan");
        };
        assert_eq!(plan.score, CombatScoreMutation::Reward(-10));
    }

    #[test]
    fn survivor_emergency_needs_no_building_facts_and_honors_flag_four() {
        let facts = Facts {
            class: None,
            score: None,
            attacker_allied: None,
            domain: None,
            victim_allied: None,
            second_is_build: None,
            active_build: None,
            emergency: Some(true),
            leader_flags: Some(0),
        };
        let plan = plan_post_damage(&facts, ATTACKER, VICTIM, DamageOutcome::Survived, 0)
            .expect("complete survivor facts");
        assert_eq!(
            plan,
            PostDamagePlan::Survived {
                emergency_who: Some(3)
            }
        );
        let mut leaders = Vec::new();
        let mut world = World::default();
        assert_eq!(
            apply_post_damage(plan, &mut leaders, &mut world),
            Ok(PostDamageReceipt::Survived {
                emergency_called: true
            })
        );
        assert_eq!(world.emergencies, vec![3]);
        assert_eq!(
            plan_post_damage(
                &Facts {
                    leader_flags: Some(0x4),
                    ..facts
                },
                ATTACKER,
                VICTIM,
                DamageOutcome::Survived,
                0,
            ),
            Ok(PostDamagePlan::Survived {
                emergency_who: None
            })
        );
    }

    #[test]
    fn apply_validates_both_leaders_before_any_mutation() {
        let PostDamagePlan::RemovedBuilding(plan) = lethal(&Facts::default(), 0).unwrap() else {
            panic!("building plan");
        };
        let mut leaders = vec![
            LeaderCombatMutationState {
                score_combat: 11,
                buildings_razed: 12,
                deaths_current_frame: 13,
                kills_current_frame: 14,
            };
            2
        ];
        let before = leaders.clone();
        let mut world = World::default();
        assert_eq!(
            apply_building_death(plan, &mut leaders, &mut world),
            Err(DamageWorldApplyError::MissingVictimLeader(3))
        );
        assert_eq!(leaders, before);
        assert!(world.plunders.is_empty());
    }

    #[test]
    fn self_owned_kill_updates_both_counters_in_one_leader_slot() {
        let facts = Facts {
            attacker_allied: Some(true),
            victim_allied: Some(true),
            ..Facts::default()
        };
        let same = ObjectKey { who: 1, o: 41 };
        let PostDamagePlan::RemovedBuilding(plan) =
            plan_post_damage(&facts, ATTACKER, same, DamageOutcome::Disbanded, 0).unwrap()
        else {
            panic!("building plan");
        };
        let mut leaders = vec![LeaderCombatMutationState::default(); 2];
        let mut world = World::default();
        apply_building_death(plan, &mut leaders, &mut world).unwrap();
        assert_eq!(leaders[1].kills_current_frame, 2);
        assert_eq!(leaders[1].deaths_current_frame, 2);
        assert!(world.plunders.is_empty());
    }
}
