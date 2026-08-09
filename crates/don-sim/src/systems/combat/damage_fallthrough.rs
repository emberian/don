//! The two deterministic `Object::do_damage` tails around the splash walk.
//!
//! This module recovers the complete building-only block at
//! `0x0064BEB7..0x0064C10C` and the continuation taken when
//! `Build::check_capture` returns zero at `0x0064C558..0x0064C86B`.
//! Presentation calls are represented as typed requests; checksum-visible
//! economy/city state and `Army::charge` remain ordered world mutations.

use super::damage_world::ObjectKey;

pub const SUNKAWAKAN_TYPE: i32 = 0xCD;
pub const FIRE_RAFT_TYPE: i32 = 0x14E;
pub const HEAVY_FIRE_RAFT_TYPE: i32 = 0x14F;
pub const AIR_DOMAIN: i32 = 2;

pub const FOOD_GOOD: i32 = 0;
pub const TIMBER_GOOD: i32 = 1;
pub const METAL_GOOD: i32 = 4;

pub const SHIPPED_GATHER_RATE: i32 = 450;
pub const SHIPPED_LAKOTA_CAV_DAMAGE_BONUS: i32 = 85;

/// Plain projection of `LeaderDataEncrypt::bucket` and `::leftover`.
///
/// Retail stores both arrays XOR-obfuscated. The XORs cancel around each
/// read/modify/write and are deliberately absent from this logical state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LeaderBountyState {
    pub bucket: [i32; 6],
    pub leftover: [i32; 6],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LakotaBountyRules {
    /// `Constants +0x27C`, `_wtoi`; shipped value 450.
    pub gather_rate: i32,
    /// `Constants +0x85C`, 8.8 fixed point; shipped value 85.
    pub cav_damage_bonus: i32,
}

impl LakotaBountyRules {
    pub const fn shipped() -> Self {
        Self {
            gather_rate: SHIPPED_GATHER_RATE,
            cav_damage_bonus: SHIPPED_LAKOTA_CAV_DAMAGE_BONUS,
        }
    }
}

/// Pure facts read by `0x0064BEB7..0x0064C10C` in retail short-circuit order.
pub trait BuildingFallthroughFacts {
    /// Victim object virtual `is_build()` (`vt +0x20`). False exits `do_damage`.
    fn victim_is_build(&self, victim: ObjectKey) -> Option<bool>;
    /// `attacker->is(SUNKAWAKAN, 0)`.
    fn attacker_is_sunkawakan(&self, attacker: ObjectKey) -> Option<bool>;
    /// Victim `BuildTypeData::is_gather_type()` (`vt +0x90`).
    fn victim_is_gather_type(&self, victim: ObjectKey) -> Option<bool>;
    /// `BuildTypeData::get_good()`; read only for a gather type.
    fn victim_gather_good(&self, victim: ObjectKey) -> Option<i32>;
    /// Read only when `is_gather_type()` was false.
    fn victim_is_gather_enhancer(&self, victim: ObjectKey) -> Option<bool>;
    /// `BuildTypeData::get_enhancing_good()`.
    fn victim_enhancing_good(&self, victim: ObjectKey) -> Option<i32>;
    /// Snapshot of the attacker's encrypted economy projection.
    fn attacker_bounty_state(&self, attacker_who: u8) -> Option<LeaderBountyState>;
    /// Victim `ObjectTypeData::is_dock()` (`vt +0x108`).
    fn victim_is_dock(&self, victim: ObjectKey) -> Option<bool>;
    /// The attacker's exact `TypeIndex`.
    fn attacker_type(&self, attacker: ObjectKey) -> Option<i32>;
    /// Attacker `ObjectTypeData +0x218`; read only for non-fire-raft types.
    fn attacker_domain(&self, attacker: ObjectKey) -> Option<i32>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MissingBuildingFallthroughFact {
    VictimBuildClass,
    AttackerSunkawakanClass,
    VictimGatherClass,
    VictimGatherGood,
    VictimGatherEnhancerClass,
    VictimEnhancingGood,
    AttackerBountyState,
    VictimDockClass,
    AttackerType,
    AttackerDomain,
    NonPositiveGatherThreshold(i32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LakotaBountyPlan {
    pub attacker_who: u8,
    pub victim: ObjectKey,
    pub good: i32,
    pub threshold: i32,
    pub delta: i32,
    pub crossings: i32,
    pub before: LeaderBountyState,
    pub after: LeaderBountyState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DockEjectRequest {
    pub victim: ObjectKey,
    /// `Object::eject_contents(1, -1, 0, 1)`.
    pub domain: i32,
    pub whom: i32,
    pub force: i32,
    pub damage_eject: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildingFallthroughPlan {
    /// Retail jumps straight to the function epilogue at `0x0064BECA`.
    ExitVictimNotBuilding,
    Continue {
        bounty: Option<LakotaBountyPlan>,
        dock_eject: Option<DockEjectRequest>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildingFallthroughMutation {
    AddBountyLeftover { good: i32, delta: i32 },
    RedeemBountyThreshold { good: i32, ordinal: i32 },
    AddBountyBucket { good: i32, amount: i32 },
    EmitGoldCoinParticles { victim: ObjectKey },
    EjectDockContents { request: DockEjectRequest },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildingFallthroughReceipt {
    pub exits_do_damage: bool,
    pub mutations: Vec<BuildingFallthroughMutation>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildingFallthroughApplyError {
    MissingAttackerLeader(u8),
    StaleAttackerBountyState {
        who: u8,
        expected: LeaderBountyState,
        actual: LeaderBountyState,
    },
}

pub trait BuildingFallthroughWorld {
    /// `GraphicPieces::emit_gold_coin_particles(victim.o, victim.who)`.
    fn emit_gold_coin_particles(&mut self, victim: ObjectKey);
    fn eject_dock_contents(&mut self, request: DockEjectRequest);
}

fn qualifying_bounty_good(good: i32) -> bool {
    matches!(good, FOOD_GOOD | TIMBER_GOOD | METAL_GOOD)
}

fn plan_lakota_bounty(
    attacker: ObjectKey,
    victim: ObjectKey,
    good: i32,
    damage: i32,
    rules: LakotaBountyRules,
    before: LeaderBountyState,
) -> Result<LakotaBountyPlan, MissingBuildingFallthroughFact> {
    let threshold = rules.gather_rate.wrapping_shl(4);
    if threshold <= 0 {
        return Err(MissingBuildingFallthroughFact::NonPositiveGatherThreshold(
            threshold,
        ));
    }

    // `imul; imul; cdq; and edx,0xff; add; sar 8`: wrapping products followed by
    // signed division by 256, truncating toward zero.
    let product = rules
        .cav_damage_bonus
        .wrapping_mul(threshold)
        .wrapping_mul(damage);
    let delta = product / 256;
    let good_i = good as usize;
    let after_add = before.leftover[good_i].wrapping_add(delta);
    let crossings = if after_add >= threshold {
        after_add / threshold
    } else {
        0
    };
    let mut after = before;
    after.leftover[good_i] = after_add.wrapping_sub(threshold.wrapping_mul(crossings));
    after.bucket[good_i] = after.bucket[good_i].wrapping_add(crossings);

    Ok(LakotaBountyPlan {
        attacker_who: attacker.who,
        victim,
        good,
        threshold,
        delta,
        crossings,
        before,
        after,
    })
}

/// Recover the full building-only pre-splash tail.
///
/// `damage` is the post-scale `ObjectData::get_damage` result retained in
/// `[ebp-0x24]`. A qualifying Lakota hit emits coin particles even when it did
/// not cross a gather threshold.
pub fn plan_building_fallthrough<F: BuildingFallthroughFacts + ?Sized>(
    facts: &F,
    attacker: ObjectKey,
    victim: ObjectKey,
    damage: i32,
    rules: LakotaBountyRules,
) -> Result<BuildingFallthroughPlan, MissingBuildingFallthroughFact> {
    if !facts
        .victim_is_build(victim)
        .ok_or(MissingBuildingFallthroughFact::VictimBuildClass)?
    {
        return Ok(BuildingFallthroughPlan::ExitVictimNotBuilding);
    }

    let mut bounty = None;
    if facts
        .attacker_is_sunkawakan(attacker)
        .ok_or(MissingBuildingFallthroughFact::AttackerSunkawakanClass)?
    {
        let good = if facts
            .victim_is_gather_type(victim)
            .ok_or(MissingBuildingFallthroughFact::VictimGatherClass)?
        {
            Some(
                facts
                    .victim_gather_good(victim)
                    .ok_or(MissingBuildingFallthroughFact::VictimGatherGood)?,
            )
        } else if facts
            .victim_is_gather_enhancer(victim)
            .ok_or(MissingBuildingFallthroughFact::VictimGatherEnhancerClass)?
        {
            Some(
                facts
                    .victim_enhancing_good(victim)
                    .ok_or(MissingBuildingFallthroughFact::VictimEnhancingGood)?,
            )
        } else {
            None
        };
        if let Some(good) = good.filter(|good| qualifying_bounty_good(*good)) {
            let before = facts
                .attacker_bounty_state(attacker.who)
                .ok_or(MissingBuildingFallthroughFact::AttackerBountyState)?;
            bounty = Some(plan_lakota_bounty(
                attacker, victim, good, damage, rules, before,
            )?);
        }
    }

    let dock_eject = if facts
        .victim_is_dock(victim)
        .ok_or(MissingBuildingFallthroughFact::VictimDockClass)?
    {
        let attacker_type = facts
            .attacker_type(attacker)
            .ok_or(MissingBuildingFallthroughFact::AttackerType)?;
        let burns_dock = matches!(attacker_type, FIRE_RAFT_TYPE | HEAVY_FIRE_RAFT_TYPE)
            || facts
                .attacker_domain(attacker)
                .ok_or(MissingBuildingFallthroughFact::AttackerDomain)?
                == AIR_DOMAIN;
        burns_dock.then_some(DockEjectRequest {
            victim,
            domain: 1,
            whom: -1,
            force: 0,
            damage_eject: 1,
        })
    } else {
        None
    };

    Ok(BuildingFallthroughPlan::Continue { bounty, dock_eject })
}

/// Apply the pre-splash tail in retail instruction order.
pub fn apply_building_fallthrough<W: BuildingFallthroughWorld + ?Sized>(
    plan: BuildingFallthroughPlan,
    leaders: &mut [LeaderBountyState],
    world: &mut W,
) -> Result<BuildingFallthroughReceipt, BuildingFallthroughApplyError> {
    let BuildingFallthroughPlan::Continue { bounty, dock_eject } = plan else {
        return Ok(BuildingFallthroughReceipt {
            exits_do_damage: true,
            mutations: Vec::new(),
        });
    };

    if let Some(bounty) = bounty {
        let Some(actual) = leaders.get(usize::from(bounty.attacker_who)) else {
            return Err(BuildingFallthroughApplyError::MissingAttackerLeader(
                bounty.attacker_who,
            ));
        };
        if *actual != bounty.before {
            return Err(BuildingFallthroughApplyError::StaleAttackerBountyState {
                who: bounty.attacker_who,
                expected: bounty.before,
                actual: *actual,
            });
        }
    }

    let mut mutations = Vec::new();
    if let Some(bounty) = bounty {
        leaders[usize::from(bounty.attacker_who)] = bounty.after;
        mutations.push(BuildingFallthroughMutation::AddBountyLeftover {
            good: bounty.good,
            delta: bounty.delta,
        });
        for ordinal in 0..bounty.crossings {
            mutations.push(BuildingFallthroughMutation::RedeemBountyThreshold {
                good: bounty.good,
                ordinal,
            });
        }
        mutations.push(BuildingFallthroughMutation::AddBountyBucket {
            good: bounty.good,
            amount: bounty.crossings,
        });
        world.emit_gold_coin_particles(bounty.victim);
        mutations.push(BuildingFallthroughMutation::EmitGoldCoinParticles {
            victim: bounty.victim,
        });
    }
    if let Some(request) = dock_eject {
        world.eject_dock_contents(request);
        mutations.push(BuildingFallthroughMutation::EjectDockContents { request });
    }

    Ok(BuildingFallthroughReceipt {
        exits_do_damage: false,
        mutations,
    })
}

// ==========================================================================================
// Build::check_capture returned zero: 0x0064C558..0x0064C86B
// ==========================================================================================

pub const RAID_EVENT_COOLDOWN_FRAMES: i32 = 0x12C;
pub const OFFENSIVE_RAID_EVENT_KIND: i32 = -7;
pub const DEFENSIVE_RAID_EVENT_KIND: i32 = -6;
pub const RAID_EVENT_DURATION: i32 = 0xC00;
pub const OFFENSIVE_RAID_MESSAGE_SLOT: u32 = 0x00C9_9F50;
pub const DEFENSIVE_RAID_MESSAGE_SLOT: u32 = 0x00C9_9F64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CityKey {
    pub who: u8,
    pub city: i16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CityRaidState {
    pub key: CityKey,
    /// `CityData +0x18`.
    pub raid_stamp: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArmyChargeRequest {
    pub army_who: u8,
    pub army: i32,
    pub target: ObjectKey,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RaidMessageSide {
    Offensive,
    Defensive,
}

impl RaidMessageSide {
    pub const fn event_kind(self) -> i32 {
        match self {
            Self::Offensive => OFFENSIVE_RAID_EVENT_KIND,
            Self::Defensive => DEFENSIVE_RAID_EVENT_KIND,
        }
    }

    pub const fn retail_message_slot(self) -> u32 {
        match self {
            Self::Offensive => OFFENSIVE_RAID_MESSAGE_SLOT,
            Self::Defensive => DEFENSIVE_RAID_MESSAGE_SLOT,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RaidEventRequest {
    pub victim: ObjectKey,
    pub x: i32,
    pub y: i32,
    pub victim_team_color: u8,
    pub bubble_who: u8,
    pub side: RaidMessageSide,
    pub event_kind: i32,
    pub duration: i32,
}

/// Facts consumed after `Build::check_capture` has returned zero.
pub trait CaptureZeroFacts {
    fn victim_has_build_flag(&self, victim: ObjectKey) -> Option<bool>;
    fn victim_is_active(&self, victim: ObjectKey) -> Option<bool>;
    fn victim_max_hits(&self, victim: ObjectKey) -> Option<i32>;
    fn victim_current_hits(&self, victim: ObjectKey) -> Option<i32>;
    fn victim_city(&self, victim: ObjectKey) -> Option<i16>;

    fn attacker_leader_flags(&self, attacker_who: u8) -> Option<u32>;
    fn attacker_is_siege(&self, attacker: ObjectKey) -> Option<bool>;
    fn attacker_unit_masks(&self, attacker: ObjectKey) -> Option<u32>;
    fn attacker_army(&self, attacker: ObjectKey) -> Option<i32>;

    /// `[ebp-0x6C]`, captured before `take_damage`: active build and hits >= max hits.
    fn victim_was_full_before_hit(&self, victim: ObjectKey) -> Option<bool>;
    fn current_frame(&self) -> Option<i32>;
    fn city_raid_state(&self, key: CityKey) -> Option<CityRaidState>;

    fn console_who(&self) -> Option<u8>;
    fn console_allied_to(&self, who: u8) -> Option<bool>;
    /// `victim->is_seen(console_who, 0)`; retail may issue this read more than once.
    fn victim_seen_by_console(&self, victim: ObjectKey, console_who: u8) -> Option<bool>;
    fn victim_xy(&self, victim: ObjectKey) -> Option<(i32, i32)>;
    fn victim_team_color(&self, victim_who: u8) -> Option<u8>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MissingCaptureZeroFact {
    VictimBuildFlag,
    VictimActiveState,
    VictimMaximumHits,
    VictimCurrentHits,
    VictimCity,
    AttackerLeaderFlags,
    AttackerSiegeClass,
    AttackerUnitMasks,
    AttackerArmy,
    VictimPreHitFullState,
    CurrentFrame,
    CityRaidState,
    CityRaidIdentityMismatch { expected: CityKey, actual: CityKey },
    ConsoleWho,
    ConsoleVictimDiplomacy,
    ConsoleAttackerDiplomacy,
    VictimVisibility,
    VictimCoordinates,
    VictimTeamColor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureZeroDisposition {
    VictimMissingBuildFlag,
    VictimInactive,
    VictimBelowMaximumHits,
    RaidCooldown,
    NoLocalAudience,
    RaidEvent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CityRaidStampPlan {
    pub before: CityRaidState,
    pub after: CityRaidState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaptureZeroPlan {
    pub disposition: CaptureZeroDisposition,
    pub charge: Option<ArmyChargeRequest>,
    pub stamp: Option<CityRaidStampPlan>,
    pub event: Option<RaidEventRequest>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureZeroMutation {
    ArmyCharge(ArmyChargeRequest),
    StampCityRaid { key: CityKey, frame: i32 },
    EmitRaidEvent(RaidEventRequest),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaptureZeroReceipt {
    pub disposition: CaptureZeroDisposition,
    pub mutations: Vec<CaptureZeroMutation>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureZeroApplyError {
    MissingCityRaidState,
    StaleCityRaidState {
        expected: CityRaidState,
        actual: CityRaidState,
    },
}

pub trait CaptureZeroWorld {
    fn army_charge(&mut self, request: ArmyChargeRequest);
    fn emit_raid_event(&mut self, request: RaidEventRequest);
}

fn no_capture_zero_effect(disposition: CaptureZeroDisposition) -> CaptureZeroPlan {
    CaptureZeroPlan {
        disposition,
        charge: None,
        stamp: None,
        event: None,
    }
}

/// Plan the complete capture-return-zero continuation without mutating the world.
pub fn plan_capture_zero<F: CaptureZeroFacts + ?Sized>(
    facts: &F,
    attacker: ObjectKey,
    victim: ObjectKey,
) -> Result<CaptureZeroPlan, MissingCaptureZeroFact> {
    if !facts
        .victim_has_build_flag(victim)
        .ok_or(MissingCaptureZeroFact::VictimBuildFlag)?
    {
        return Ok(no_capture_zero_effect(
            CaptureZeroDisposition::VictimMissingBuildFlag,
        ));
    }
    if !facts
        .victim_is_active(victim)
        .ok_or(MissingCaptureZeroFact::VictimActiveState)?
    {
        return Ok(no_capture_zero_effect(
            CaptureZeroDisposition::VictimInactive,
        ));
    }
    let max_hits = facts
        .victim_max_hits(victim)
        .ok_or(MissingCaptureZeroFact::VictimMaximumHits)?;
    let current_hits = facts
        .victim_current_hits(victim)
        .ok_or(MissingCaptureZeroFact::VictimCurrentHits)?;
    if current_hits < max_hits {
        return Ok(no_capture_zero_effect(
            CaptureZeroDisposition::VictimBelowMaximumHits,
        ));
    }

    let city = facts
        .victim_city(victim)
        .ok_or(MissingCaptureZeroFact::VictimCity)?;
    let mut charge = None;
    if attacker.o >= 0 {
        let leader_flags = facts
            .attacker_leader_flags(attacker.who)
            .ok_or(MissingCaptureZeroFact::AttackerLeaderFlags)?;
        if leader_flags & 0x4 == 0
            && facts
                .attacker_is_siege(attacker)
                .ok_or(MissingCaptureZeroFact::AttackerSiegeClass)?
            && facts
                .attacker_unit_masks(attacker)
                .ok_or(MissingCaptureZeroFact::AttackerUnitMasks)?
                & 0x0004_0000
                != 0
        {
            let army = facts
                .attacker_army(attacker)
                .ok_or(MissingCaptureZeroFact::AttackerArmy)?;
            if army >= 0 {
                charge = Some(ArmyChargeRequest {
                    army_who: attacker.who,
                    army,
                    target: victim,
                });
            }
        }
    }

    let prehit_full = facts
        .victim_was_full_before_hit(victim)
        .ok_or(MissingCaptureZeroFact::VictimPreHitFullState)?;
    let frame = facts
        .current_frame()
        .ok_or(MissingCaptureZeroFact::CurrentFrame)?;
    let city_key = CityKey {
        who: victim.who,
        city,
    };
    let city_before = facts
        .city_raid_state(city_key)
        .ok_or(MissingCaptureZeroFact::CityRaidState)?;
    if city_before.key != city_key {
        return Err(MissingCaptureZeroFact::CityRaidIdentityMismatch {
            expected: city_key,
            actual: city_before.key,
        });
    }
    if !prehit_full && frame.wrapping_sub(city_before.raid_stamp) < RAID_EVENT_COOLDOWN_FRAMES {
        return Ok(CaptureZeroPlan {
            disposition: CaptureZeroDisposition::RaidCooldown,
            charge,
            stamp: None,
            event: None,
        });
    }
    let stamp = CityRaidStampPlan {
        before: city_before,
        after: CityRaidState {
            raid_stamp: frame,
            ..city_before
        },
    };

    let console = facts
        .console_who()
        .ok_or(MissingCaptureZeroFact::ConsoleWho)?;
    let audience = if victim.who == console {
        true
    } else {
        let allied_to_victim = facts
            .console_allied_to(victim.who)
            .ok_or(MissingCaptureZeroFact::ConsoleVictimDiplomacy)?;
        if allied_to_victim
            && facts
                .victim_seen_by_console(victim, console)
                .ok_or(MissingCaptureZeroFact::VictimVisibility)?
        {
            true
        } else if attacker.who == console {
            true
        } else {
            facts
                .console_allied_to(attacker.who)
                .ok_or(MissingCaptureZeroFact::ConsoleAttackerDiplomacy)?
                && facts
                    .victim_seen_by_console(victim, console)
                    .ok_or(MissingCaptureZeroFact::VictimVisibility)?
        }
    };
    if !audience {
        return Ok(CaptureZeroPlan {
            disposition: CaptureZeroDisposition::NoLocalAudience,
            charge,
            stamp: Some(stamp),
            event: None,
        });
    }

    // Retail repeats the attacker-alliance/visibility reads after its audience gate to
    // select the offensive versus defensive message string.
    let side = if attacker.who == console
        || (facts
            .console_allied_to(attacker.who)
            .ok_or(MissingCaptureZeroFact::ConsoleAttackerDiplomacy)?
            && facts
                .victim_seen_by_console(victim, console)
                .ok_or(MissingCaptureZeroFact::VictimVisibility)?)
    {
        RaidMessageSide::Offensive
    } else {
        RaidMessageSide::Defensive
    };
    let (x, y) = facts
        .victim_xy(victim)
        .ok_or(MissingCaptureZeroFact::VictimCoordinates)?;
    let victim_team_color = facts
        .victim_team_color(victim.who)
        .ok_or(MissingCaptureZeroFact::VictimTeamColor)?;
    let event = RaidEventRequest {
        victim,
        x,
        y,
        victim_team_color,
        bubble_who: victim.who,
        side,
        event_kind: side.event_kind(),
        duration: RAID_EVENT_DURATION,
    };

    Ok(CaptureZeroPlan {
        disposition: CaptureZeroDisposition::RaidEvent,
        charge,
        stamp: Some(stamp),
        event: Some(event),
    })
}

/// Execute the continuation in address order: army charge, raid stamp, presentation event.
pub fn apply_capture_zero<W: CaptureZeroWorld + ?Sized>(
    plan: CaptureZeroPlan,
    city: Option<&mut CityRaidState>,
    world: &mut W,
) -> Result<CaptureZeroReceipt, CaptureZeroApplyError> {
    if let Some(stamp) = plan.stamp {
        let actual = city
            .as_deref()
            .ok_or(CaptureZeroApplyError::MissingCityRaidState)?;
        if *actual != stamp.before {
            return Err(CaptureZeroApplyError::StaleCityRaidState {
                expected: stamp.before,
                actual: *actual,
            });
        }
    }

    let mut mutations = Vec::new();
    if let Some(charge) = plan.charge {
        world.army_charge(charge);
        mutations.push(CaptureZeroMutation::ArmyCharge(charge));
    }
    if let Some(stamp) = plan.stamp {
        *city.expect("city state was admitted before the first mutation") = stamp.after;
        mutations.push(CaptureZeroMutation::StampCityRaid {
            key: stamp.after.key,
            frame: stamp.after.raid_stamp,
        });
    }
    if let Some(event) = plan.event {
        world.emit_raid_event(event);
        mutations.push(CaptureZeroMutation::EmitRaidEvent(event));
    }

    Ok(CaptureZeroReceipt {
        disposition: plan.disposition,
        mutations,
    })
}
