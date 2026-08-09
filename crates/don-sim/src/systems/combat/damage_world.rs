//! Address-bounded `Object::do_damage` post-`take_damage` world transactions.
//!
//! This is the address-bounded slice `0x0064BA18..0x0064BBFA` plus the adjacent
//! survivor-only `Armies::emergency` gate at `0x0064BBFD..0x0064BC17`, followed by the
//! flamethrower entrench/eject transaction at `0x0064BC17..0x0064BEB7`, and the bounded
//! post-splash capture-attempt arm at `0x0064C4E3..0x0064C558`.
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

// ===========================================================================================
// Flamethrower special hit: 0x0064BC17..0x0064BEB7
// ===========================================================================================

/// `FLAMETHROWER` in the shipped TypeIndex table.
pub const FLAMETHROWER_TYPE: i32 = 0x83;
/// The generic `CITIZENS` TypeIndex selected before nation/tribe specialization.
pub const BASE_CITIZEN_TYPE: i32 = 0x32;
/// `ObjectData::can_carry(2)` — air-domain cargo suppresses the building-ejection arm.
pub const AIR_DOMAIN: i32 = 2;
/// `ObjectData::num_inside(1)` — the saved land-occupant count.
pub const LAND_DOMAIN: i32 = 1;

/// Facts needed by the contiguous flamethrower special-hit branch.
///
/// The planner preserves all retail short circuits. A non-flamethrower needs no victim
/// facts; a unit needs no containment/death-ring facts; an empty non-unit needs no spawn
/// facts. Every fact required by the chosen executor path is resolved before ejection or
/// mask mutation.
pub trait SpecialHitFacts {
    /// `attacker->is(FLAMETHROWER, 0)` at `0x0064BC17..0x0064BC3B`.
    fn attacker_is_flamethrower(&self, attacker: ObjectKey) -> Option<bool>;
    /// Target virtual `is_unit()` at `0x0064BC43..0x0064BC56`.
    fn victim_is_unit(&self, victim: ObjectKey) -> Option<bool>;
    /// The checksum-visible `UnitData +0x68` value, read before the entrenchment gate.
    fn victim_unit_masks(&self, victim: ObjectKey) -> Option<u32>;
    /// The checksum-visible `UnitData +0x6C` value, needed only for entrenched units.
    fn victim_unit_masks2(&self, victim: ObjectKey) -> Option<u32>;
    /// `ObjectData::can_carry(AIR_DOMAIN)`.
    fn victim_can_carry_air(&self, victim: ObjectKey) -> Option<bool>;
    /// `ObjectData::num_inside(LAND_DOMAIN)`, read before `eject_contents`.
    fn victim_land_inside(&self, victim: ObjectKey) -> Option<i32>;
    /// Victim type `x_size +0x234`; the spawn bound is signed `x_size >> 1`.
    fn victim_type_x_size(&self, victim: ObjectKey) -> Option<i32>;
    /// The exact current death-ring order plus the global gpiece threshold at
    /// `[ObjectsOut+0xD48]`.
    fn death_visual_scan(&self) -> Option<DeathVisualScan>;
    /// Unmasked victim coordinates, reused for every synthesized Citizen.
    fn victim_xy(&self, victim: ObjectKey) -> Option<(i32, i32)>;
    /// `leaders[victim].tribe_can_type(types[CITIZENS])`.
    fn victim_tribe_can_base_citizen(&self, victim_who: u8) -> Option<bool>;
    /// `types[CITIZENS]->vt+0x68(leaders[victim].nation)` fallback.
    fn victim_nation_citizen_type(&self, victim_who: u8) -> Option<i32>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MissingSpecialHitFact {
    AttackerType,
    VictimClass,
    VictimUnitMasks,
    VictimUnitMasks2,
    VictimAirCarry,
    VictimLandInside,
    VictimTypeXSize,
    DeathVisualScan,
    VictimCoordinates,
    VictimTribeCitizenGate,
    VictimNationCitizenType,
}

/// Minimal death-ring projection consumed by `0x0064BD47..0x0064BD96`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeathVisual {
    /// `DeathObjData +0x00`.
    pub valid: i32,
    /// `DeathObjData +0x20`.
    pub gpiece: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeathVisualScan {
    pub records: Vec<DeathVisual>,
    pub gpiece_threshold: i32,
}

impl DeathVisualScan {
    /// Count valid records whose `gpiece` is strictly greater than the global threshold.
    /// The retail loop scans every slot in array order and uses a signed comparison.
    pub fn qualifying_count(&self) -> i32 {
        let mut count = 0i32;
        for record in &self.records {
            if record.valid != 0 && record.gpiece > self.gpiece_threshold {
                count = count.wrapping_add(1);
            }
        }
        count
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EntrenchClearPlan {
    pub victim: ObjectKey,
    pub expected_unit_masks: u32,
    pub expected_unit_masks2: u32,
    pub unit_masks_after: u32,
    pub unit_masks2_after: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CitizenBurnSpawnPlan {
    pub victim: ObjectKey,
    pub citizen_type: i32,
    pub x: i32,
    pub y: i32,
    pub spawn_count: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildingEjectPlan {
    pub victim: ObjectKey,
    /// Saved before ejection. Retail only tests zero/non-zero after the call.
    pub land_inside_before_eject: i32,
    pub burn_spawns: Option<CitizenBurnSpawnPlan>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpecialHitPlan {
    NotFlamethrower,
    UnitNoEntrenchment,
    ClearEntrenchment(EntrenchClearPlan),
    NonUnitCanCarryAir,
    EjectBuilding(BuildingEjectPlan),
}

/// Exact external operations used by the special-hit branch.
pub trait SpecialHitWorld {
    fn remove_entrench(&mut self, victim: ObjectKey);
    /// `Object::eject_contents(1, -1, 0, 1)`.
    fn eject_contents(&mut self, victim: ObjectKey);
    /// `Objects::init_unit(victim.who, type, x, y, -1, -1, -1)`; negative means failure.
    fn init_burning_citizen(&mut self, request: BurningCitizenRequest) -> i32;
    /// `Unit::go_inside(victim.o, victim.who, 0)`.
    fn burning_citizen_go_inside(&mut self, spawned_o: i32, victim: ObjectKey);
    /// `Unit::come_out(0)`.
    fn burning_citizen_come_out(&mut self, spawned_o: i32, spawned_who: u8);
    /// Object virtual `close(4, -1, 0)`, issued in a second loop.
    fn close_burning_citizen(&mut self, spawned_o: i32, spawned_who: u8);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BurningCitizenRequest {
    pub who: u8,
    pub type_index: i32,
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpecialHitMutation {
    ClearEntrenchMasks,
    RemoveEntrenchGraphics,
    EjectContents,
    InitCitizen { iteration: i32, returned_o: i32 },
    CitizenGoInside { o: i32 },
    CitizenComeOut { o: i32 },
    CloseCitizen { o: i32 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpecialHitReceipt {
    pub mutations: Vec<SpecialHitMutation>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpecialHitApplyError {
    MissingUnitState,
    StaleUnitMasks {
        expected: (u32, u32),
        actual: (u32, u32),
    },
}

/// Pure, fail-closed recovery of `0x0064BC17..0x0064BEB7`.
pub fn plan_special_hit<F: SpecialHitFacts + ?Sized>(
    facts: &F,
    attacker: ObjectKey,
    victim: ObjectKey,
) -> Result<SpecialHitPlan, MissingSpecialHitFact> {
    if !facts
        .attacker_is_flamethrower(attacker)
        .ok_or(MissingSpecialHitFact::AttackerType)?
    {
        return Ok(SpecialHitPlan::NotFlamethrower);
    }

    if facts
        .victim_is_unit(victim)
        .ok_or(MissingSpecialHitFact::VictimClass)?
    {
        let unit_masks = facts
            .victim_unit_masks(victim)
            .ok_or(MissingSpecialHitFact::VictimUnitMasks)?;
        if unit_masks & 0x0200_0000 == 0 {
            return Ok(SpecialHitPlan::UnitNoEntrenchment);
        }
        let unit_masks2 = facts
            .victim_unit_masks2(victim)
            .ok_or(MissingSpecialHitFact::VictimUnitMasks2)?;
        return Ok(SpecialHitPlan::ClearEntrenchment(EntrenchClearPlan {
            victim,
            expected_unit_masks: unit_masks,
            expected_unit_masks2: unit_masks2,
            unit_masks_after: unit_masks & !0x0200_0000,
            unit_masks2_after: unit_masks2 & !0x0000_1000 & !0x0002_0000,
        }));
    }

    if facts
        .victim_can_carry_air(victim)
        .ok_or(MissingSpecialHitFact::VictimAirCarry)?
    {
        return Ok(SpecialHitPlan::NonUnitCanCarryAir);
    }

    let land_inside = facts
        .victim_land_inside(victim)
        .ok_or(MissingSpecialHitFact::VictimLandInside)?;
    if land_inside == 0 {
        return Ok(SpecialHitPlan::EjectBuilding(BuildingEjectPlan {
            victim,
            land_inside_before_eject: 0,
            burn_spawns: None,
        }));
    }

    let x_size = facts
        .victim_type_x_size(victim)
        .ok_or(MissingSpecialHitFact::VictimTypeXSize)?;
    let spawn_count = x_size >> 1;
    let death_scan = facts
        .death_visual_scan()
        .ok_or(MissingSpecialHitFact::DeathVisualScan)?;
    if death_scan.qualifying_count() >= spawn_count {
        return Ok(SpecialHitPlan::EjectBuilding(BuildingEjectPlan {
            victim,
            land_inside_before_eject: land_inside,
            burn_spawns: None,
        }));
    }

    let (x, y) = facts
        .victim_xy(victim)
        .ok_or(MissingSpecialHitFact::VictimCoordinates)?;
    let citizen_type = if facts
        .victim_tribe_can_base_citizen(victim.who)
        .ok_or(MissingSpecialHitFact::VictimTribeCitizenGate)?
    {
        BASE_CITIZEN_TYPE
    } else {
        facts
            .victim_nation_citizen_type(victim.who)
            .ok_or(MissingSpecialHitFact::VictimNationCitizenType)?
    };

    Ok(SpecialHitPlan::EjectBuilding(BuildingEjectPlan {
        victim,
        land_inside_before_eject: land_inside,
        burn_spawns: Some(CitizenBurnSpawnPlan {
            victim,
            citizen_type,
            x,
            y,
            spawn_count,
        }),
    }))
}

/// Execute the special-hit transaction with exact mutation and loop order.
///
/// Successful allocations are contained and released immediately, but all successful slots
/// are closed only after the entire allocation loop finishes. Failed allocations still
/// consume an iteration and never appear in the close loop.
pub fn apply_special_hit<W: SpecialHitWorld + ?Sized>(
    plan: SpecialHitPlan,
    victim_unit: Option<&mut super::UnitCombatState>,
    world: &mut W,
) -> Result<SpecialHitReceipt, SpecialHitApplyError> {
    let mut mutations = Vec::new();
    match plan {
        SpecialHitPlan::NotFlamethrower
        | SpecialHitPlan::UnitNoEntrenchment
        | SpecialHitPlan::NonUnitCanCarryAir => {}
        SpecialHitPlan::ClearEntrenchment(clear) => {
            let unit = victim_unit.ok_or(SpecialHitApplyError::MissingUnitState)?;
            let actual = (unit.unit_masks, unit.unit_masks2);
            let expected = (clear.expected_unit_masks, clear.expected_unit_masks2);
            if actual != expected {
                return Err(SpecialHitApplyError::StaleUnitMasks { expected, actual });
            }
            unit.unit_masks = clear.unit_masks_after;
            unit.unit_masks2 = clear.unit_masks2_after;
            mutations.push(SpecialHitMutation::ClearEntrenchMasks);
            world.remove_entrench(clear.victim);
            mutations.push(SpecialHitMutation::RemoveEntrenchGraphics);
        }
        SpecialHitPlan::EjectBuilding(eject) => {
            world.eject_contents(eject.victim);
            mutations.push(SpecialHitMutation::EjectContents);
            if let Some(spawns) = eject.burn_spawns {
                let mut successful = Vec::new();
                for iteration in 0..spawns.spawn_count {
                    let returned_o = world.init_burning_citizen(BurningCitizenRequest {
                        who: spawns.victim.who,
                        type_index: spawns.citizen_type,
                        x: spawns.x,
                        y: spawns.y,
                    });
                    mutations.push(SpecialHitMutation::InitCitizen {
                        iteration,
                        returned_o,
                    });
                    if returned_o < 0 {
                        continue;
                    }
                    successful.push(returned_o);
                    world.burning_citizen_go_inside(returned_o, spawns.victim);
                    mutations.push(SpecialHitMutation::CitizenGoInside { o: returned_o });
                    world.burning_citizen_come_out(returned_o, spawns.victim.who);
                    mutations.push(SpecialHitMutation::CitizenComeOut { o: returned_o });
                }
                for o in successful {
                    world.close_burning_citizen(o, spawns.victim.who);
                    mutations.push(SpecialHitMutation::CloseCitizen { o });
                }
            }
        }
    }
    Ok(SpecialHitReceipt { mutations })
}

// ===========================================================================================
// Post-splash capture attempt: 0x0064C4E3..0x0064C558
// ===========================================================================================

/// The one type fact read before `Object::do_damage` decides whether to call
/// `Build::check_capture`.
pub trait CaptureAttemptFacts {
    /// Victim type virtual `BuildTypeData::is_city()` (`vt +0x64`) at
    /// `0x0064C4F7..0x0064C51C`.
    fn victim_is_city(&self, victim: ObjectKey) -> Option<bool>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MissingCaptureAttemptFact {
    VictimCityClassification,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaptureCheckRequest {
    /// `this`: the victim `BuildData` returned by the object virtual at `+0xAC`.
    pub victim: ObjectKey,
    /// `Build::check_capture(attacker.o, attacker.who)`.
    pub attacker: ObjectKey,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureAttemptPlan {
    VictimNotCity,
    SameOwner,
    CheckCapture(CaptureCheckRequest),
}

/// Identity-bound proof of the synchronous `Build::check_capture` call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaptureCheckReceipt {
    pub request: CaptureCheckRequest,
    /// Retail tests `eax != 0` at `0x0064C550`; non-zero jumps directly to the
    /// `Object::do_damage` epilogue at `0x0064C86B`.
    pub returned_nonzero: bool,
}

/// Atomic adapter around the mutating `Build::check_capture` routine.
///
/// `None` means the adapter could not admit the call and guarantees that it performed no
/// mutation. A returned receipt must identify the exact victim and attacker.
pub trait CaptureAttemptWorld {
    fn check_capture(&mut self, request: CaptureCheckRequest) -> Option<CaptureCheckReceipt>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureAttemptReceipt {
    VictimNotCity,
    SameOwner,
    Checked {
        request: CaptureCheckRequest,
        /// Whether the caller must take retail's immediate function-exit edge.
        stop_post_damage: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureAttemptApplyError {
    MissingAtomicReceipt,
    ReceiptIdentityMismatch {
        expected: CaptureCheckRequest,
        actual: CaptureCheckRequest,
    },
}

/// Resolve retail's two gates in address order.
///
/// `BuildTypeData::is_city()` is evaluated before the same-owner comparison, so a non-city
/// victim never needs a meaningful attacker owner and a missing city classification always
/// fails closed.
pub fn plan_capture_attempt<F: CaptureAttemptFacts + ?Sized>(
    facts: &F,
    attacker: ObjectKey,
    victim: ObjectKey,
) -> Result<CaptureAttemptPlan, MissingCaptureAttemptFact> {
    if !facts
        .victim_is_city(victim)
        .ok_or(MissingCaptureAttemptFact::VictimCityClassification)?
    {
        return Ok(CaptureAttemptPlan::VictimNotCity);
    }
    if victim.who == attacker.who {
        return Ok(CaptureAttemptPlan::SameOwner);
    }
    Ok(CaptureAttemptPlan::CheckCapture(CaptureCheckRequest {
        victim,
        attacker,
    }))
}

/// Execute the bounded capture-attempt arm and expose retail's branch result.
pub fn apply_capture_attempt<W: CaptureAttemptWorld + ?Sized>(
    plan: CaptureAttemptPlan,
    world: &mut W,
) -> Result<CaptureAttemptReceipt, CaptureAttemptApplyError> {
    match plan {
        CaptureAttemptPlan::VictimNotCity => Ok(CaptureAttemptReceipt::VictimNotCity),
        CaptureAttemptPlan::SameOwner => Ok(CaptureAttemptReceipt::SameOwner),
        CaptureAttemptPlan::CheckCapture(request) => {
            let receipt = world
                .check_capture(request)
                .ok_or(CaptureAttemptApplyError::MissingAtomicReceipt)?;
            if receipt.request != request {
                return Err(CaptureAttemptApplyError::ReceiptIdentityMismatch {
                    expected: request,
                    actual: receipt.request,
                });
            }
            Ok(CaptureAttemptReceipt::Checked {
                request,
                stop_post_damage: receipt.returned_nonzero,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

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

    #[derive(Debug)]
    struct SpecialFacts {
        flamethrower: Option<bool>,
        unit: Option<bool>,
        unit_masks: Option<u32>,
        unit_masks2: Option<u32>,
        can_carry_air: Option<bool>,
        land_inside: Option<i32>,
        x_size: Option<i32>,
        death_scan: Option<DeathVisualScan>,
        xy: Option<(i32, i32)>,
        tribe_citizen: Option<bool>,
        nation_citizen: Option<i32>,
        calls: RefCell<Vec<&'static str>>,
    }

    impl Default for SpecialFacts {
        fn default() -> Self {
            Self {
                flamethrower: Some(true),
                unit: Some(false),
                unit_masks: Some(0),
                unit_masks2: Some(0),
                can_carry_air: Some(false),
                land_inside: Some(1),
                x_size: Some(6),
                death_scan: Some(DeathVisualScan {
                    records: Vec::new(),
                    gpiece_threshold: 40,
                }),
                xy: Some((123, -456)),
                tribe_citizen: Some(true),
                nation_citizen: Some(777),
                calls: RefCell::new(Vec::new()),
            }
        }
    }

    impl SpecialHitFacts for SpecialFacts {
        fn attacker_is_flamethrower(&self, _: ObjectKey) -> Option<bool> {
            self.calls.borrow_mut().push("attacker_type");
            self.flamethrower
        }
        fn victim_is_unit(&self, _: ObjectKey) -> Option<bool> {
            self.calls.borrow_mut().push("victim_is_unit");
            self.unit
        }
        fn victim_unit_masks(&self, _: ObjectKey) -> Option<u32> {
            self.calls.borrow_mut().push("unit_masks");
            self.unit_masks
        }
        fn victim_unit_masks2(&self, _: ObjectKey) -> Option<u32> {
            self.calls.borrow_mut().push("unit_masks2");
            self.unit_masks2
        }
        fn victim_can_carry_air(&self, _: ObjectKey) -> Option<bool> {
            self.calls.borrow_mut().push("can_carry_air");
            self.can_carry_air
        }
        fn victim_land_inside(&self, _: ObjectKey) -> Option<i32> {
            self.calls.borrow_mut().push("land_inside");
            self.land_inside
        }
        fn victim_type_x_size(&self, _: ObjectKey) -> Option<i32> {
            self.calls.borrow_mut().push("x_size");
            self.x_size
        }
        fn death_visual_scan(&self) -> Option<DeathVisualScan> {
            self.calls.borrow_mut().push("death_scan");
            self.death_scan.clone()
        }
        fn victim_xy(&self, _: ObjectKey) -> Option<(i32, i32)> {
            self.calls.borrow_mut().push("xy");
            self.xy
        }
        fn victim_tribe_can_base_citizen(&self, _: u8) -> Option<bool> {
            self.calls.borrow_mut().push("tribe_citizen");
            self.tribe_citizen
        }
        fn victim_nation_citizen_type(&self, _: u8) -> Option<i32> {
            self.calls.borrow_mut().push("nation_citizen");
            self.nation_citizen
        }
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    enum SpecialEvent {
        RemoveEntrench(ObjectKey),
        Eject(ObjectKey),
        Init(BurningCitizenRequest, i32),
        GoInside(i32, ObjectKey),
        ComeOut(i32, u8),
        Close(i32, u8),
    }

    #[derive(Default)]
    struct SpecialWorld {
        events: Vec<SpecialEvent>,
        init_results: Vec<i32>,
        next_init: usize,
    }

    impl SpecialHitWorld for SpecialWorld {
        fn remove_entrench(&mut self, victim: ObjectKey) {
            self.events.push(SpecialEvent::RemoveEntrench(victim));
        }
        fn eject_contents(&mut self, victim: ObjectKey) {
            self.events.push(SpecialEvent::Eject(victim));
        }
        fn init_burning_citizen(&mut self, request: BurningCitizenRequest) -> i32 {
            let result = self.init_results[self.next_init];
            self.next_init += 1;
            self.events.push(SpecialEvent::Init(request, result));
            result
        }
        fn burning_citizen_go_inside(&mut self, spawned_o: i32, victim: ObjectKey) {
            self.events.push(SpecialEvent::GoInside(spawned_o, victim));
        }
        fn burning_citizen_come_out(&mut self, spawned_o: i32, spawned_who: u8) {
            self.events
                .push(SpecialEvent::ComeOut(spawned_o, spawned_who));
        }
        fn close_burning_citizen(&mut self, spawned_o: i32, spawned_who: u8) {
            self.events
                .push(SpecialEvent::Close(spawned_o, spawned_who));
        }
    }

    #[test]
    fn special_hit_short_circuits_in_retail_fact_order() {
        let facts = SpecialFacts {
            flamethrower: Some(false),
            unit: None,
            ..SpecialFacts::default()
        };
        assert_eq!(
            plan_special_hit(&facts, ATTACKER, VICTIM),
            Ok(SpecialHitPlan::NotFlamethrower)
        );
        assert_eq!(*facts.calls.borrow(), vec!["attacker_type"]);

        let facts = SpecialFacts {
            unit: Some(true),
            unit_masks: Some(0x10),
            unit_masks2: None,
            ..SpecialFacts::default()
        };
        assert_eq!(
            plan_special_hit(&facts, ATTACKER, VICTIM),
            Ok(SpecialHitPlan::UnitNoEntrenchment)
        );
        assert_eq!(
            *facts.calls.borrow(),
            vec!["attacker_type", "victim_is_unit", "unit_masks"]
        );

        let facts = SpecialFacts {
            can_carry_air: Some(true),
            land_inside: None,
            ..SpecialFacts::default()
        };
        assert_eq!(
            plan_special_hit(&facts, ATTACKER, VICTIM),
            Ok(SpecialHitPlan::NonUnitCanCarryAir)
        );
        assert_eq!(
            *facts.calls.borrow(),
            vec!["attacker_type", "victim_is_unit", "can_carry_air"]
        );
    }

    #[test]
    fn entrench_clear_mutates_only_three_checksum_bits_then_removes_graphics() {
        let before_masks = 0xA200_0005;
        let before_masks2 = 0xF00A_1F0F;
        let facts = SpecialFacts {
            unit: Some(true),
            unit_masks: Some(before_masks),
            unit_masks2: Some(before_masks2),
            ..SpecialFacts::default()
        };
        let SpecialHitPlan::ClearEntrenchment(plan) =
            plan_special_hit(&facts, ATTACKER, VICTIM).unwrap()
        else {
            panic!("entrenched-unit plan");
        };
        assert_eq!(plan.unit_masks_after, before_masks & !0x0200_0000);
        assert_eq!(
            plan.unit_masks2_after,
            before_masks2 & !0x0000_1000 & !0x0002_0000
        );

        let mut unit = super::super::UnitCombatState {
            unit_masks: before_masks,
            unit_masks2: before_masks2,
            ..super::super::UnitCombatState::default()
        };
        let mut checksum_before = [0u8; super::super::UNIT_WALK_LEN];
        unit.patch_unit_range(&mut checksum_before);
        let mut world = SpecialWorld::default();
        let receipt = apply_special_hit(
            SpecialHitPlan::ClearEntrenchment(plan),
            Some(&mut unit),
            &mut world,
        )
        .unwrap();
        let mut checksum_after = [0u8; super::super::UNIT_WALK_LEN];
        unit.patch_unit_range(&mut checksum_after);

        assert_eq!(
            receipt.mutations,
            vec![
                SpecialHitMutation::ClearEntrenchMasks,
                SpecialHitMutation::RemoveEntrenchGraphics,
            ]
        );
        assert_eq!(world.events, vec![SpecialEvent::RemoveEntrench(VICTIM)]);
        let mask_i = 0x68 - super::super::UNIT_WALK_BEGIN;
        let mask2_i = 0x6C - super::super::UNIT_WALK_BEGIN;
        for i in 0..super::super::UNIT_WALK_LEN {
            if !(mask_i..mask_i + 4).contains(&i) && !(mask2_i..mask2_i + 4).contains(&i) {
                assert_eq!(checksum_after[i], checksum_before[i], "walk byte {i:#x}");
            }
        }
        assert_eq!(
            &checksum_after[mask_i..mask_i + 4],
            &plan.unit_masks_after.to_le_bytes()
        );
        assert_eq!(
            &checksum_after[mask2_i..mask2_i + 4],
            &plan.unit_masks2_after.to_le_bytes()
        );
    }

    #[test]
    fn entrench_apply_rejects_missing_or_stale_state_before_world_mutation() {
        let plan = EntrenchClearPlan {
            victim: VICTIM,
            expected_unit_masks: 0x0200_0001,
            expected_unit_masks2: 0x0002_1001,
            unit_masks_after: 1,
            unit_masks2_after: 1,
        };
        let mut world = SpecialWorld::default();
        assert_eq!(
            apply_special_hit(SpecialHitPlan::ClearEntrenchment(plan), None, &mut world),
            Err(SpecialHitApplyError::MissingUnitState)
        );
        let mut unit = super::super::UnitCombatState {
            unit_masks: 0x0200_0001,
            unit_masks2: 0,
            ..super::super::UnitCombatState::default()
        };
        assert_eq!(
            apply_special_hit(
                SpecialHitPlan::ClearEntrenchment(plan),
                Some(&mut unit),
                &mut world
            ),
            Err(SpecialHitApplyError::StaleUnitMasks {
                expected: (0x0200_0001, 0x0002_1001),
                actual: (0x0200_0001, 0),
            })
        );
        assert!(world.events.is_empty());
        assert_eq!(unit.unit_masks, 0x0200_0001);
        assert_eq!(unit.unit_masks2, 0);
    }

    #[test]
    fn empty_building_still_ejects_and_needs_no_spawn_facts() {
        let facts = SpecialFacts {
            land_inside: Some(0),
            x_size: None,
            death_scan: None,
            xy: None,
            tribe_citizen: None,
            nation_citizen: None,
            ..SpecialFacts::default()
        };
        let plan = plan_special_hit(&facts, ATTACKER, VICTIM).unwrap();
        assert_eq!(
            plan,
            SpecialHitPlan::EjectBuilding(BuildingEjectPlan {
                victim: VICTIM,
                land_inside_before_eject: 0,
                burn_spawns: None,
            })
        );
        assert_eq!(
            *facts.calls.borrow(),
            vec![
                "attacker_type",
                "victim_is_unit",
                "can_carry_air",
                "land_inside"
            ]
        );
        let mut world = SpecialWorld::default();
        let receipt = apply_special_hit(plan, None, &mut world).unwrap();
        assert_eq!(world.events, vec![SpecialEvent::Eject(VICTIM)]);
        assert_eq!(receipt.mutations, vec![SpecialHitMutation::EjectContents]);
    }

    #[test]
    fn death_ring_uses_valid_and_strict_greater_and_suppresses_at_spawn_bound() {
        let scan = DeathVisualScan {
            records: vec![
                DeathVisual {
                    valid: 0,
                    gpiece: 99,
                },
                DeathVisual {
                    valid: 1,
                    gpiece: 40,
                },
                DeathVisual {
                    valid: -1,
                    gpiece: 41,
                },
                DeathVisual {
                    valid: 2,
                    gpiece: 42,
                },
            ],
            gpiece_threshold: 40,
        };
        assert_eq!(scan.qualifying_count(), 2);
        let facts = SpecialFacts {
            x_size: Some(5),
            death_scan: Some(scan),
            xy: None,
            tribe_citizen: None,
            ..SpecialFacts::default()
        };
        let SpecialHitPlan::EjectBuilding(plan) =
            plan_special_hit(&facts, ATTACKER, VICTIM).unwrap()
        else {
            panic!("building eject plan");
        };
        assert_eq!(plan.burn_spawns, None, "5 >> 1 is exactly two");
        assert_eq!(
            *facts.calls.borrow(),
            vec![
                "attacker_type",
                "victim_is_unit",
                "can_carry_air",
                "land_inside",
                "x_size",
                "death_scan"
            ]
        );
    }

    #[test]
    fn citizen_type_gate_and_signed_odd_spawn_bound_are_exact() {
        let base = SpecialFacts {
            x_size: Some(7),
            tribe_citizen: Some(true),
            nation_citizen: None,
            ..SpecialFacts::default()
        };
        let SpecialHitPlan::EjectBuilding(base_plan) =
            plan_special_hit(&base, ATTACKER, VICTIM).unwrap()
        else {
            panic!("building eject plan");
        };
        let spawn = base_plan.burn_spawns.unwrap();
        assert_eq!(spawn.spawn_count, 3);
        assert_eq!(spawn.citizen_type, BASE_CITIZEN_TYPE);
        assert!(!base.calls.borrow().contains(&"nation_citizen"));

        let fallback = SpecialFacts {
            tribe_citizen: Some(false),
            nation_citizen: Some(912),
            ..SpecialFacts::default()
        };
        let SpecialHitPlan::EjectBuilding(fallback_plan) =
            plan_special_hit(&fallback, ATTACKER, VICTIM).unwrap()
        else {
            panic!("building eject plan");
        };
        assert_eq!(fallback_plan.burn_spawns.unwrap().citizen_type, 912);
        assert_eq!(
            &fallback.calls.borrow()[7..],
            &["tribe_citizen", "nation_citizen"]
        );
    }

    #[test]
    fn citizen_lifecycle_defers_successful_closes_until_all_allocations_finish() {
        let plan = SpecialHitPlan::EjectBuilding(BuildingEjectPlan {
            victim: VICTIM,
            land_inside_before_eject: 9,
            burn_spawns: Some(CitizenBurnSpawnPlan {
                victim: VICTIM,
                citizen_type: 912,
                x: 12,
                y: -34,
                spawn_count: 3,
            }),
        });
        let request = BurningCitizenRequest {
            who: VICTIM.who,
            type_index: 912,
            x: 12,
            y: -34,
        };
        let mut world = SpecialWorld {
            init_results: vec![12, -1, 7],
            ..SpecialWorld::default()
        };
        let receipt = apply_special_hit(plan, None, &mut world).unwrap();
        assert_eq!(
            world.events,
            vec![
                SpecialEvent::Eject(VICTIM),
                SpecialEvent::Init(request, 12),
                SpecialEvent::GoInside(12, VICTIM),
                SpecialEvent::ComeOut(12, VICTIM.who),
                SpecialEvent::Init(request, -1),
                SpecialEvent::Init(request, 7),
                SpecialEvent::GoInside(7, VICTIM),
                SpecialEvent::ComeOut(7, VICTIM.who),
                SpecialEvent::Close(12, VICTIM.who),
                SpecialEvent::Close(7, VICTIM.who),
            ]
        );
        assert_eq!(
            receipt.mutations,
            vec![
                SpecialHitMutation::EjectContents,
                SpecialHitMutation::InitCitizen {
                    iteration: 0,
                    returned_o: 12,
                },
                SpecialHitMutation::CitizenGoInside { o: 12 },
                SpecialHitMutation::CitizenComeOut { o: 12 },
                SpecialHitMutation::InitCitizen {
                    iteration: 1,
                    returned_o: -1,
                },
                SpecialHitMutation::InitCitizen {
                    iteration: 2,
                    returned_o: 7,
                },
                SpecialHitMutation::CitizenGoInside { o: 7 },
                SpecialHitMutation::CitizenComeOut { o: 7 },
                SpecialHitMutation::CloseCitizen { o: 12 },
                SpecialHitMutation::CloseCitizen { o: 7 },
            ]
        );
    }

    struct CaptureFacts {
        is_city: Option<bool>,
        calls: RefCell<Vec<ObjectKey>>,
    }

    impl CaptureAttemptFacts for CaptureFacts {
        fn victim_is_city(&self, victim: ObjectKey) -> Option<bool> {
            self.calls.borrow_mut().push(victim);
            self.is_city
        }
    }

    #[derive(Default)]
    struct CaptureWorld {
        calls: Vec<CaptureCheckRequest>,
        response: Option<CaptureCheckReceipt>,
    }

    impl CaptureAttemptWorld for CaptureWorld {
        fn check_capture(&mut self, request: CaptureCheckRequest) -> Option<CaptureCheckReceipt> {
            self.calls.push(request);
            self.response
        }
    }

    #[test]
    fn capture_city_classification_is_mandatory_even_for_same_owner() {
        let facts = CaptureFacts {
            is_city: None,
            calls: RefCell::new(Vec::new()),
        };
        let same_owner = ObjectKey {
            who: ATTACKER.who,
            o: VICTIM.o,
        };
        assert_eq!(
            plan_capture_attempt(&facts, ATTACKER, same_owner),
            Err(MissingCaptureAttemptFact::VictimCityClassification)
        );
        assert_eq!(*facts.calls.borrow(), vec![same_owner]);
    }

    #[test]
    fn capture_non_city_and_same_owner_arms_never_call_the_world() {
        let non_city = CaptureFacts {
            is_city: Some(false),
            calls: RefCell::new(Vec::new()),
        };
        let mut world = CaptureWorld::default();
        let plan = plan_capture_attempt(&non_city, ATTACKER, VICTIM).unwrap();
        assert_eq!(plan, CaptureAttemptPlan::VictimNotCity);
        assert_eq!(
            apply_capture_attempt(plan, &mut world),
            Ok(CaptureAttemptReceipt::VictimNotCity)
        );

        let same_owner_victim = ObjectKey {
            who: ATTACKER.who,
            o: VICTIM.o,
        };
        let city = CaptureFacts {
            is_city: Some(true),
            calls: RefCell::new(Vec::new()),
        };
        let plan = plan_capture_attempt(&city, ATTACKER, same_owner_victim).unwrap();
        assert_eq!(plan, CaptureAttemptPlan::SameOwner);
        assert_eq!(
            apply_capture_attempt(plan, &mut world),
            Ok(CaptureAttemptReceipt::SameOwner)
        );
        assert!(world.calls.is_empty());
    }

    #[test]
    fn capture_check_receipt_preserves_exact_identity_and_exit_edge() {
        let facts = CaptureFacts {
            is_city: Some(true),
            calls: RefCell::new(Vec::new()),
        };
        // `movsx eax, word ptr [attacker+0x0A]` at `0x0064C53C`: keep the signed
        // attacker object id all the way into Build::check_capture's first argument.
        let capture_attacker = ObjectKey { who: 1, o: -17 };
        let request = CaptureCheckRequest {
            victim: VICTIM,
            attacker: capture_attacker,
        };
        let plan = plan_capture_attempt(&facts, capture_attacker, VICTIM).unwrap();
        assert_eq!(plan, CaptureAttemptPlan::CheckCapture(request));

        for returned_nonzero in [false, true] {
            let mut world = CaptureWorld {
                response: Some(CaptureCheckReceipt {
                    request,
                    returned_nonzero,
                }),
                ..CaptureWorld::default()
            };
            assert_eq!(
                apply_capture_attempt(plan, &mut world),
                Ok(CaptureAttemptReceipt::Checked {
                    request,
                    stop_post_damage: returned_nonzero,
                })
            );
            assert_eq!(world.calls, vec![request]);
        }
    }

    #[test]
    fn capture_host_failure_and_identity_drift_are_typed_errors() {
        let request = CaptureCheckRequest {
            victim: VICTIM,
            attacker: ATTACKER,
        };
        let plan = CaptureAttemptPlan::CheckCapture(request);
        let mut missing = CaptureWorld::default();
        assert_eq!(
            apply_capture_attempt(plan, &mut missing),
            Err(CaptureAttemptApplyError::MissingAtomicReceipt)
        );

        let actual = CaptureCheckRequest {
            victim: VICTIM,
            attacker: ObjectKey { who: 2, o: 99 },
        };
        let mut mismatch = CaptureWorld {
            response: Some(CaptureCheckReceipt {
                request: actual,
                returned_nonzero: true,
            }),
            ..CaptureWorld::default()
        };
        assert_eq!(
            apply_capture_attempt(plan, &mut mismatch),
            Err(CaptureAttemptApplyError::ReceiptIdentityMismatch {
                expected: request,
                actual,
            })
        );
    }
}
