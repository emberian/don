use don_sim::systems::combat::damage_fallthrough::{
    apply_building_fallthrough, apply_capture_zero, plan_building_fallthrough, plan_capture_zero,
    ArmyChargeRequest, BuildingFallthroughFacts, BuildingFallthroughMutation,
    BuildingFallthroughWorld, CaptureZeroDisposition, CaptureZeroFacts, CaptureZeroMutation,
    CaptureZeroWorld, CityKey, CityRaidState, DockEjectRequest, LakotaBountyRules,
    LeaderBountyState, RaidEventRequest, RaidMessageSide, AIR_DOMAIN, DEFENSIVE_RAID_EVENT_KIND,
    FIRE_RAFT_TYPE, FOOD_GOOD, HEAVY_FIRE_RAFT_TYPE, METAL_GOOD, OFFENSIVE_RAID_EVENT_KIND,
    RAID_EVENT_DURATION, TIMBER_GOOD,
};
use don_sim::systems::combat::damage_world::ObjectKey;
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;

const ATTACKER: ObjectKey = ObjectKey { who: 1, o: 17 };
const VICTIM: ObjectKey = ObjectKey { who: 3, o: 41 };

#[derive(Clone)]
struct BuildingFacts {
    victim_build: bool,
    sunkawakan: bool,
    gather_type: bool,
    gather_good: i32,
    gather_enhancer: bool,
    enhancing_good: i32,
    bounty: LeaderBountyState,
    dock: bool,
    attacker_type: i32,
    attacker_domain: Option<i32>,
    bounty_reads: Cell<usize>,
    domain_reads: Cell<usize>,
}

impl Default for BuildingFacts {
    fn default() -> Self {
        Self {
            victim_build: true,
            sunkawakan: true,
            gather_type: true,
            gather_good: FOOD_GOOD,
            gather_enhancer: false,
            enhancing_good: TIMBER_GOOD,
            bounty: LeaderBountyState::default(),
            dock: false,
            attacker_type: 99,
            attacker_domain: Some(0),
            bounty_reads: Cell::new(0),
            domain_reads: Cell::new(0),
        }
    }
}

impl BuildingFallthroughFacts for BuildingFacts {
    fn victim_is_build(&self, _: ObjectKey) -> Option<bool> {
        Some(self.victim_build)
    }
    fn attacker_is_sunkawakan(&self, _: ObjectKey) -> Option<bool> {
        Some(self.sunkawakan)
    }
    fn victim_is_gather_type(&self, _: ObjectKey) -> Option<bool> {
        Some(self.gather_type)
    }
    fn victim_gather_good(&self, _: ObjectKey) -> Option<i32> {
        Some(self.gather_good)
    }
    fn victim_is_gather_enhancer(&self, _: ObjectKey) -> Option<bool> {
        Some(self.gather_enhancer)
    }
    fn victim_enhancing_good(&self, _: ObjectKey) -> Option<i32> {
        Some(self.enhancing_good)
    }
    fn attacker_bounty_state(&self, _: u8) -> Option<LeaderBountyState> {
        self.bounty_reads.set(self.bounty_reads.get() + 1);
        Some(self.bounty)
    }
    fn victim_is_dock(&self, _: ObjectKey) -> Option<bool> {
        Some(self.dock)
    }
    fn attacker_type(&self, _: ObjectKey) -> Option<i32> {
        Some(self.attacker_type)
    }
    fn attacker_domain(&self, _: ObjectKey) -> Option<i32> {
        self.domain_reads.set(self.domain_reads.get() + 1);
        self.attacker_domain
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BuildingEvent {
    Coins(ObjectKey),
    Eject(DockEjectRequest),
}

#[derive(Default)]
struct BuildingWorld {
    events: Vec<BuildingEvent>,
}

impl BuildingFallthroughWorld for BuildingWorld {
    fn emit_gold_coin_particles(&mut self, victim: ObjectKey) {
        self.events.push(BuildingEvent::Coins(victim));
    }
    fn eject_dock_contents(&mut self, request: DockEjectRequest) {
        self.events.push(BuildingEvent::Eject(request));
    }
}

#[test]
fn lakota_bounty_redeems_each_threshold_before_particles_and_dock_eject() {
    let mut before = LeaderBountyState::default();
    before.bucket[FOOD_GOOD as usize] = 7;
    before.leftover[FOOD_GOOD as usize] = 100;
    let facts = BuildingFacts {
        bounty: before,
        dock: true,
        attacker_type: FIRE_RAFT_TYPE,
        attacker_domain: None,
        ..BuildingFacts::default()
    };
    let plan =
        plan_building_fallthrough(&facts, ATTACKER, VICTIM, 10, LakotaBountyRules::shipped())
            .unwrap();
    assert_eq!(
        facts.domain_reads.get(),
        0,
        "fire raft bypasses the domain read"
    );

    let mut leaders = vec![LeaderBountyState::default(); 4];
    leaders[usize::from(ATTACKER.who)] = before;
    let mut world = BuildingWorld::default();
    let receipt = apply_building_fallthrough(plan, &mut leaders, &mut world).unwrap();

    assert_eq!(leaders[1].leftover[0], 2_406);
    assert_eq!(leaders[1].bucket[0], 10);
    assert_eq!(
        receipt.mutations,
        vec![
            BuildingFallthroughMutation::AddBountyLeftover {
                good: FOOD_GOOD,
                delta: 23_906,
            },
            BuildingFallthroughMutation::RedeemBountyThreshold {
                good: FOOD_GOOD,
                ordinal: 0,
            },
            BuildingFallthroughMutation::RedeemBountyThreshold {
                good: FOOD_GOOD,
                ordinal: 1,
            },
            BuildingFallthroughMutation::RedeemBountyThreshold {
                good: FOOD_GOOD,
                ordinal: 2,
            },
            BuildingFallthroughMutation::AddBountyBucket {
                good: FOOD_GOOD,
                amount: 3,
            },
            BuildingFallthroughMutation::EmitGoldCoinParticles { victim: VICTIM },
            BuildingFallthroughMutation::EjectDockContents {
                request: DockEjectRequest {
                    victim: VICTIM,
                    domain: 1,
                    whom: -1,
                    force: 0,
                    damage_eject: 1,
                },
            },
        ]
    );
    assert_eq!(
        world.events,
        vec![
            BuildingEvent::Coins(VICTIM),
            BuildingEvent::Eject(DockEjectRequest {
                victim: VICTIM,
                domain: 1,
                whom: -1,
                force: 0,
                damage_eject: 1,
            }),
        ]
    );
}

#[test]
fn invalid_good_skips_bounty_state_and_particles_but_keeps_the_dock_arm() {
    let facts = BuildingFacts {
        gather_good: 5,
        dock: true,
        attacker_type: 777,
        attacker_domain: Some(AIR_DOMAIN),
        ..BuildingFacts::default()
    };
    let plan =
        plan_building_fallthrough(&facts, ATTACKER, VICTIM, 100, LakotaBountyRules::shipped())
            .unwrap();
    assert_eq!(facts.bounty_reads.get(), 0);
    assert_eq!(facts.domain_reads.get(), 1);
    let mut leaders = vec![LeaderBountyState::default(); 4];
    let mut world = BuildingWorld::default();
    let receipt = apply_building_fallthrough(plan, &mut leaders, &mut world).unwrap();
    assert!(matches!(
        receipt.mutations.as_slice(),
        [BuildingFallthroughMutation::EjectDockContents { .. }]
    ));
    assert!(matches!(world.events.as_slice(), [BuildingEvent::Eject(_)]));
}

#[test]
fn gather_enhancer_fallback_accepts_only_food_timber_and_metal() {
    for good in [FOOD_GOOD, TIMBER_GOOD, METAL_GOOD] {
        let facts = BuildingFacts {
            gather_type: false,
            gather_enhancer: true,
            enhancing_good: good,
            ..BuildingFacts::default()
        };
        let plan =
            plan_building_fallthrough(&facts, ATTACKER, VICTIM, 0, LakotaBountyRules::shipped())
                .unwrap();
        let mut leaders = vec![LeaderBountyState::default(); 4];
        let mut world = BuildingWorld::default();
        let receipt = apply_building_fallthrough(plan, &mut leaders, &mut world).unwrap();
        assert!(receipt.mutations.iter().any(|mutation| matches!(
            mutation,
            BuildingFallthroughMutation::EmitGoldCoinParticles { .. }
        )));
    }
}

#[test]
fn stale_lakota_snapshot_rejects_before_any_presentation_or_eject_mutation() {
    let facts = BuildingFacts {
        dock: true,
        attacker_type: HEAVY_FIRE_RAFT_TYPE,
        ..BuildingFacts::default()
    };
    let plan = plan_building_fallthrough(&facts, ATTACKER, VICTIM, 1, LakotaBountyRules::shipped())
        .unwrap();
    let mut leaders = vec![LeaderBountyState::default(); 4];
    leaders[1].bucket[0] = 1;
    let mut world = BuildingWorld::default();
    assert!(apply_building_fallthrough(plan, &mut leaders, &mut world).is_err());
    assert!(world.events.is_empty());
}

#[derive(Clone)]
struct CaptureFacts {
    build_flag: bool,
    active: bool,
    max_hits: i32,
    current_hits: i32,
    city: i16,
    leader_flags: u32,
    siege: bool,
    unit_masks: u32,
    army: i32,
    prehit_full: bool,
    frame: i32,
    city_state: CityRaidState,
    console: u8,
    victim_allied: bool,
    attacker_allied: bool,
    seen_results: RefCell<VecDeque<bool>>,
    visibility_reads: Cell<usize>,
    xy: (i32, i32),
    team_color: u8,
}

impl Default for CaptureFacts {
    fn default() -> Self {
        Self {
            build_flag: true,
            active: true,
            max_hits: 100,
            current_hits: 100,
            city: 2,
            leader_flags: 0,
            siege: true,
            unit_masks: 0x0004_0000,
            army: 7,
            prehit_full: true,
            frame: 500,
            city_state: CityRaidState {
                key: CityKey { who: 3, city: 2 },
                raid_stamp: 100,
            },
            console: ATTACKER.who,
            victim_allied: false,
            attacker_allied: true,
            seen_results: RefCell::new(VecDeque::from([true, true, true])),
            visibility_reads: Cell::new(0),
            xy: (12_345, -6_789),
            team_color: 4,
        }
    }
}

impl CaptureZeroFacts for CaptureFacts {
    fn victim_has_build_flag(&self, _: ObjectKey) -> Option<bool> {
        Some(self.build_flag)
    }
    fn victim_is_active(&self, _: ObjectKey) -> Option<bool> {
        Some(self.active)
    }
    fn victim_max_hits(&self, _: ObjectKey) -> Option<i32> {
        Some(self.max_hits)
    }
    fn victim_current_hits(&self, _: ObjectKey) -> Option<i32> {
        Some(self.current_hits)
    }
    fn victim_city(&self, _: ObjectKey) -> Option<i16> {
        Some(self.city)
    }
    fn attacker_leader_flags(&self, _: u8) -> Option<u32> {
        Some(self.leader_flags)
    }
    fn attacker_is_siege(&self, _: ObjectKey) -> Option<bool> {
        Some(self.siege)
    }
    fn attacker_unit_masks(&self, _: ObjectKey) -> Option<u32> {
        Some(self.unit_masks)
    }
    fn attacker_army(&self, _: ObjectKey) -> Option<i32> {
        Some(self.army)
    }
    fn victim_was_full_before_hit(&self, _: ObjectKey) -> Option<bool> {
        Some(self.prehit_full)
    }
    fn current_frame(&self) -> Option<i32> {
        Some(self.frame)
    }
    fn city_raid_state(&self, _: CityKey) -> Option<CityRaidState> {
        Some(self.city_state)
    }
    fn console_who(&self) -> Option<u8> {
        Some(self.console)
    }
    fn console_allied_to(&self, who: u8) -> Option<bool> {
        if who == VICTIM.who {
            Some(self.victim_allied)
        } else if who == ATTACKER.who {
            Some(self.attacker_allied)
        } else {
            Some(false)
        }
    }
    fn victim_seen_by_console(&self, _: ObjectKey, _: u8) -> Option<bool> {
        self.visibility_reads.set(self.visibility_reads.get() + 1);
        self.seen_results.borrow_mut().pop_front()
    }
    fn victim_xy(&self, _: ObjectKey) -> Option<(i32, i32)> {
        Some(self.xy)
    }
    fn victim_team_color(&self, _: u8) -> Option<u8> {
        Some(self.team_color)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CaptureEvent {
    Charge(ArmyChargeRequest),
    Raid(RaidEventRequest),
}

#[derive(Default)]
struct CaptureWorld {
    events: Vec<CaptureEvent>,
}

impl CaptureZeroWorld for CaptureWorld {
    fn army_charge(&mut self, request: ArmyChargeRequest) {
        self.events.push(CaptureEvent::Charge(request));
    }
    fn emit_raid_event(&mut self, request: RaidEventRequest) {
        self.events.push(CaptureEvent::Raid(request));
    }
}

#[test]
fn capture_zero_orders_siege_charge_before_city_stamp_and_offensive_event() {
    let facts = CaptureFacts::default();
    let plan = plan_capture_zero(&facts, ATTACKER, VICTIM).unwrap();
    let expected_charge = ArmyChargeRequest {
        army_who: ATTACKER.who,
        army: 7,
        target: VICTIM,
    };
    let expected_event = RaidEventRequest {
        victim: VICTIM,
        x: 12_345,
        y: -6_789,
        victim_team_color: 4,
        bubble_who: VICTIM.who,
        side: RaidMessageSide::Offensive,
        event_kind: OFFENSIVE_RAID_EVENT_KIND,
        duration: RAID_EVENT_DURATION,
    };
    let mut city = facts.city_state;
    let mut world = CaptureWorld::default();
    let receipt = apply_capture_zero(plan, Some(&mut city), &mut world).unwrap();
    assert_eq!(
        receipt.mutations,
        vec![
            CaptureZeroMutation::ArmyCharge(expected_charge),
            CaptureZeroMutation::StampCityRaid {
                key: facts.city_state.key,
                frame: 500,
            },
            CaptureZeroMutation::EmitRaidEvent(expected_event),
        ]
    );
    assert_eq!(city.raid_stamp, 500);
    assert_eq!(
        world.events,
        vec![
            CaptureEvent::Charge(expected_charge),
            CaptureEvent::Raid(expected_event),
        ]
    );
}

#[test]
fn raid_cooldown_still_issues_the_earlier_siege_army_charge() {
    let facts = CaptureFacts {
        prehit_full: false,
        frame: 350,
        ..CaptureFacts::default()
    };
    let plan = plan_capture_zero(&facts, ATTACKER, VICTIM).unwrap();
    assert_eq!(plan.disposition, CaptureZeroDisposition::RaidCooldown);
    assert!(plan.charge.is_some());
    assert!(plan.stamp.is_none());
    let mut world = CaptureWorld::default();
    let receipt = apply_capture_zero(plan, None, &mut world).unwrap();
    assert!(matches!(
        receipt.mutations.as_slice(),
        [CaptureZeroMutation::ArmyCharge(_)]
    ));
}

#[test]
fn no_local_audience_still_stamps_the_city_but_emits_no_event() {
    let facts = CaptureFacts {
        console: 7,
        victim_allied: false,
        attacker_allied: false,
        ..CaptureFacts::default()
    };
    let plan = plan_capture_zero(&facts, ATTACKER, VICTIM).unwrap();
    assert_eq!(plan.disposition, CaptureZeroDisposition::NoLocalAudience);
    assert!(plan.stamp.is_some());
    assert!(plan.event.is_none());
    let mut city = facts.city_state;
    let mut world = CaptureWorld::default();
    let receipt = apply_capture_zero(plan, Some(&mut city), &mut world).unwrap();
    assert_eq!(city.raid_stamp, facts.frame);
    assert!(!receipt
        .mutations
        .iter()
        .any(|mutation| matches!(mutation, CaptureZeroMutation::EmitRaidEvent(_))));
}

#[test]
fn retail_repeats_visibility_for_audience_and_message_side_selection() {
    let facts = CaptureFacts {
        console: 7,
        victim_allied: true,
        attacker_allied: true,
        seen_results: RefCell::new(VecDeque::from([false, true, true])),
        ..CaptureFacts::default()
    };
    let plan = plan_capture_zero(&facts, ATTACKER, VICTIM).unwrap();
    assert_eq!(facts.visibility_reads.get(), 3);
    let event = plan
        .event
        .expect("third visibility read selects offensive event");
    assert_eq!(event.side, RaidMessageSide::Offensive);
    assert_eq!(event.event_kind, OFFENSIVE_RAID_EVENT_KIND);
}

#[test]
fn victim_side_notification_uses_the_defensive_event_kind() {
    let facts = CaptureFacts {
        console: VICTIM.who,
        attacker_allied: false,
        ..CaptureFacts::default()
    };
    let plan = plan_capture_zero(&facts, ATTACKER, VICTIM).unwrap();
    let event = plan.event.unwrap();
    assert_eq!(event.side, RaidMessageSide::Defensive);
    assert_eq!(event.event_kind, DEFENSIVE_RAID_EVENT_KIND);
}

#[test]
fn stale_city_stamp_is_rejected_before_the_earlier_charge_call() {
    let facts = CaptureFacts::default();
    let plan = plan_capture_zero(&facts, ATTACKER, VICTIM).unwrap();
    let mut city = facts.city_state;
    city.raid_stamp += 1;
    let mut world = CaptureWorld::default();
    assert!(apply_capture_zero(plan, Some(&mut city), &mut world).is_err());
    assert!(world.events.is_empty());
}
