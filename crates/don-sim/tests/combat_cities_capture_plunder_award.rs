use don_sim::systems::combat::{build_check_capture, cities_capture_prefix, damage_world};

#[path = "../src/systems/combat/cities_capture_plunder_award.rs"]
mod cities_capture_plunder_award;
#[path = "../src/systems/combat/cities_capture_plunder_gate.rs"]
mod cities_capture_plunder_gate;
#[path = "../src/systems/combat/cities_capture_swap_fork.rs"]
mod cities_capture_swap_fork;

use build_check_capture::CityKey;
use cities_capture_plunder_award::{
    apply_cities_capture_plunder_award, elimination_plunder_amount,
    plan_cities_capture_plunder_award, thedespot_scaled_plunder, BucketAddReceipt,
    BucketAddRequest, CapitalPlunderRuleReceipt, CitiesCapturePlunderAwardApplyError,
    CitiesCapturePlunderAwardContinuation, CitiesCapturePlunderAwardEvent,
    CitiesCapturePlunderAwardWorld, ConsoleWhoReceipt, EliminationPlunderSizingReceipt,
    OldOwnerLeaderFlagsReceipt, OldOwnerLeaderFlagsRequest, OrOldOwnerLeaderFlagReceipt,
    OrOldOwnerLeaderFlagRequest, TeamStyleReadPhase, TeamStyleReadReceipt, TeamStyleReadRequest,
    TheDespotPlunderRuleReceipt, TypeAvailabilityReceipt, TypeAvailabilityRequest,
    CAPITAL_CITY_FLAG, CITIES_CAPTURE_PLUNDER_AWARD_END,
    CITIES_CAPTURE_PLUNDER_AWARD_RESIDUAL_SIZE, CITIES_CAPTURE_PLUNDER_AWARD_SIZE,
    CITIES_CAPTURE_PLUNDER_AWARD_START, OLD_OWNER_CAPITAL_CAPTURED_FLAG, TYPE_AVAIL_MODE,
};
use cities_capture_plunder_gate::{
    CitiesCapturePlunderGateContinuation, CitiesCapturePlunderGateReceipt,
};
use cities_capture_swap_fork::{CitiesCaptureSwapForkContinuation, CitiesCaptureSwapForkReceipt};
use damage_world::ObjectKey;

const OLD_CITY: CityKey = CityKey { who: 1, city: 6 };
const NEW_CITY: CityKey = CityKey { who: 2, city: 9 };

fn prior_swap(plunder: i32) -> CitiesCaptureSwapForkReceipt {
    CitiesCaptureSwapForkReceipt {
        old_city_objects: vec![ObjectKey { who: 1, o: 40 }],
        new_city_objects: vec![ObjectKey { who: 2, o: 70 }],
        new_city: Some(NEW_CITY),
        plunder_accumulator: plunder,
        continuation: CitiesCaptureSwapForkContinuation::Converged0x00733d67,
        mutations: vec![],
    }
}

fn prior_gate(
    plunder: i32,
    qualifying_general: bool,
    russian_plunder_steal: bool,
) -> CitiesCapturePlunderGateReceipt {
    CitiesCapturePlunderGateReceipt {
        prior: prior_swap(plunder),
        old_city: OLD_CITY,
        new_owner: 2,
        captured_own_capital: false,
        qualifying_general,
        russian_plunder_steal,
        notification_raised: false,
        city_flags: CAPITAL_CITY_FLAG,
        capture_stamp: Some(0),
        continuation: CitiesCapturePlunderGateContinuation::EnterPlunder0x00733fac,
        events: vec![],
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HostEvent {
    Flags(OldOwnerLeaderFlagsRequest),
    Style(TeamStyleReadRequest),
    OrFlag(OrOldOwnerLeaderFlagRequest),
    CapitalRule,
    EliminationSizing,
    DespotRule,
    Availability(TypeAvailabilityRequest),
    Bucket(BucketAddRequest),
    ConsoleWho,
}

struct World {
    events: Vec<HostEvent>,
    old_flags: u32,
    eligibility_style: u8,
    sizing_style: u8,
    capital_plunder: i32,
    elimination: EliminationPlunderSizingReceipt,
    thedespot_percent: i32,
    new_availability: [i32; 6],
    old_availability: [i32; 6],
    buckets: [i32; 6],
    console_who: i32,
    corrupt_flag_receipt: bool,
    corrupt_bucket: Option<i32>,
}

impl Default for World {
    fn default() -> Self {
        Self {
            events: vec![],
            old_flags: 0,
            eligibility_style: 0,
            sizing_style: 0,
            capital_plunder: 500,
            elimination: EliminationPlunderSizingReceipt {
                exact_read_order_attested: true,
                num_nations: 8,
                leader_flags: [0; 8],
                capital_plunder_assassin: 500,
            },
            thedespot_percent: 100,
            new_availability: [2; 6],
            old_availability: [4; 6],
            buckets: [1_000; 6],
            console_who: 2,
            corrupt_flag_receipt: false,
            corrupt_bucket: None,
        }
    }
}

impl CitiesCapturePlunderAwardWorld for World {
    fn read_old_owner_leader_flags(
        &mut self,
        request: OldOwnerLeaderFlagsRequest,
    ) -> Option<OldOwnerLeaderFlagsReceipt> {
        self.events.push(HostEvent::Flags(request));
        Some(OldOwnerLeaderFlagsReceipt {
            request,
            leader_flags: self.old_flags,
        })
    }

    fn read_team_style(&mut self, request: TeamStyleReadRequest) -> Option<TeamStyleReadReceipt> {
        self.events.push(HostEvent::Style(request));
        let team_style = match request.phase {
            TeamStyleReadPhase::Eligibility0x00733fd2 => self.eligibility_style,
            TeamStyleReadPhase::Sizing0x00733fe2 => self.sizing_style,
        };
        Some(TeamStyleReadReceipt {
            request,
            team_style,
        })
    }

    fn or_old_owner_leader_flag(
        &mut self,
        request: OrOldOwnerLeaderFlagRequest,
    ) -> Option<OrOldOwnerLeaderFlagReceipt> {
        self.events.push(HostEvent::OrFlag(request));
        let before = self.old_flags;
        self.old_flags |= request.mask;
        Some(OrOldOwnerLeaderFlagReceipt {
            request,
            flags_before: before,
            flags_after: if self.corrupt_flag_receipt {
                self.old_flags ^ 1
            } else {
                self.old_flags
            },
            mutation_applied: true,
        })
    }

    fn read_capital_plunder_rule(&mut self) -> Option<CapitalPlunderRuleReceipt> {
        self.events.push(HostEvent::CapitalRule);
        Some(CapitalPlunderRuleReceipt {
            capital_plunder: self.capital_plunder,
        })
    }

    fn read_elimination_plunder_sizing(&mut self) -> Option<EliminationPlunderSizingReceipt> {
        self.events.push(HostEvent::EliminationSizing);
        Some(self.elimination)
    }

    fn read_thedespot_plunder_rule(&mut self) -> Option<TheDespotPlunderRuleReceipt> {
        self.events.push(HostEvent::DespotRule);
        Some(TheDespotPlunderRuleReceipt {
            thedespot_plunder_percent: self.thedespot_percent,
        })
    }

    fn read_type_availability(
        &mut self,
        request: TypeAvailabilityRequest,
    ) -> Option<TypeAvailabilityReceipt> {
        self.events.push(HostEvent::Availability(request));
        let table = if request.owner == 2 {
            self.new_availability
        } else {
            self.old_availability
        };
        Some(TypeAvailabilityReceipt {
            request,
            raw_availability: table[request.type_index as usize],
        })
    }

    fn bucket_add(&mut self, request: BucketAddRequest) -> Option<BucketAddReceipt> {
        self.events.push(HostEvent::Bucket(request));
        let index = request.bucket as usize;
        let before = self.buckets[index];
        self.buckets[index] = before.wrapping_add(request.amount);
        let after = if self.corrupt_bucket == Some(request.bucket) {
            self.buckets[index].wrapping_add(1)
        } else {
            self.buckets[index]
        };
        Some(BucketAddReceipt {
            request,
            balance_before: before,
            balance_after: after,
            mutation_applied: true,
        })
    }

    fn read_console_who(&mut self) -> Option<ConsoleWhoReceipt> {
        self.events.push(HostEvent::ConsoleWho);
        Some(ConsoleWhoReceipt {
            who: self.console_who,
        })
    }
}

#[test]
fn exact_award_prefix_and_residual_addresses_are_frozen() {
    assert_eq!(CITIES_CAPTURE_PLUNDER_AWARD_START, 0x0073_3FAC);
    assert_eq!(CITIES_CAPTURE_PLUNDER_AWARD_END, 0x0073_4152);
    assert_eq!(CITIES_CAPTURE_PLUNDER_AWARD_SIZE, 0x1A6);
    assert_eq!(CITIES_CAPTURE_PLUNDER_AWARD_RESIDUAL_SIZE, 0x116C);
    assert_eq!(OLD_OWNER_CAPITAL_CAPTURED_FLAG, 0x0040_0000);
}

#[test]
fn ordinary_capital_plunder_orders_flag_sizing_availability_and_buckets() {
    let plan = plan_cities_capture_plunder_award(prior_gate(200, false, false)).unwrap();
    let mut world = World::default();
    let receipt = apply_cities_capture_plunder_award(plan, &mut world).unwrap();

    assert_eq!(receipt.sized_capital_plunder, Some(500));
    assert_eq!(receipt.new_owner_award, Some(500));
    assert_eq!(receipt.old_owner_refund, Some(0));
    assert_eq!(
        receipt.continuation,
        CitiesCapturePlunderAwardContinuation::LocalAwardNotification0x00734152
    );
    assert_eq!(world.old_flags, OLD_OWNER_CAPITAL_CAPTURED_FLAG);
    assert_eq!(world.buckets, [1_500, 1_500, 1_500, 1_000, 1_500, 1_500]);
    assert!(matches!(
        receipt.events.as_slice(),
        [
            CitiesCapturePlunderAwardEvent::ReadOldOwnerLeaderFlags(_),
            CitiesCapturePlunderAwardEvent::ReadTeamStyle(_),
            CitiesCapturePlunderAwardEvent::OrOldOwnerLeaderFlag(_),
            CitiesCapturePlunderAwardEvent::ReadTeamStyle(_),
            CitiesCapturePlunderAwardEvent::ReadCapitalPlunderRule(_),
            CitiesCapturePlunderAwardEvent::WriteSizedCapitalPlunder(500),
            CitiesCapturePlunderAwardEvent::InitializeOldOwnerRefund(0),
            ..,
            CitiesCapturePlunderAwardEvent::ReadConsoleWho(_),
        ]
    ));
    let bucket_types: Vec<_> = world
        .events
        .iter()
        .filter_map(|event| match event {
            HostEvent::Bucket(request) => Some(request.bucket),
            _ => None,
        })
        .collect();
    assert_eq!(bucket_types, vec![0, 1, 2, 4, 5]);
}

#[test]
fn old_flag_and_barbarians_style_take_distinct_pre_mutation_forward_edges() {
    let mut flagged = World {
        old_flags: OLD_OWNER_CAPITAL_CAPTURED_FLAG,
        ..World::default()
    };
    let receipt = apply_cities_capture_plunder_award(
        plan_cities_capture_plunder_award(prior_gate(200, false, false)).unwrap(),
        &mut flagged,
    )
    .unwrap();
    assert_eq!(
        receipt.continuation,
        CitiesCapturePlunderAwardContinuation::AlternatePlunder0x00734547
    );
    assert_eq!(flagged.events.len(), 1);

    let mut barbarians = World {
        eligibility_style: 3,
        ..World::default()
    };
    let receipt = apply_cities_capture_plunder_award(
        plan_cities_capture_plunder_award(prior_gate(200, false, false)).unwrap(),
        &mut barbarians,
    )
    .unwrap();
    assert_eq!(
        receipt.continuation,
        CitiesCapturePlunderAwardContinuation::AlternatePlunder0x00734547
    );
    assert!(matches!(
        barbarians.events.as_slice(),
        [HostEvent::Flags(_), HostEvent::Style(_)]
    ));
    assert_eq!(barbarians.old_flags, 0);
}

#[test]
fn elimination_style_and_russian_split_can_skip_the_resource_loop() {
    let mut world = World {
        eligibility_style: 2,
        sizing_style: 2,
        ..World::default()
    };
    world.elimination.leader_flags = [1, 1, 1, 1, 1, 0, 0, 0];
    assert_eq!(elimination_plunder_amount(world.elimination), 2_500);
    let receipt = apply_cities_capture_plunder_award(
        plan_cities_capture_plunder_award(prior_gate(200, false, true)).unwrap(),
        &mut world,
    )
    .unwrap();
    assert_eq!(receipt.sized_capital_plunder, Some(2_500));
    assert_eq!(receipt.old_owner_refund, Some(2_500));
    assert_eq!(receipt.new_owner_award, Some(0));
    assert_eq!(
        receipt.continuation,
        CitiesCapturePlunderAwardContinuation::OldOwnerRefund0x0073432d
    );
    assert!(!world
        .events
        .iter()
        .any(|event| matches!(event, HostEvent::Availability(_) | HostEvent::Bucket(_))));
}

#[test]
fn thedespot_percent_uses_wrapping_product_and_signed_truncation() {
    assert_eq!(thedespot_scaled_plunder(333, 50), 166);
    let mut world = World {
        capital_plunder: 100,
        thedespot_percent: 50,
        console_who: -1,
        ..World::default()
    };
    let receipt = apply_cities_capture_plunder_award(
        plan_cities_capture_plunder_award(prior_gate(333, true, false)).unwrap(),
        &mut world,
    )
    .unwrap();
    assert_eq!(receipt.sized_capital_plunder, Some(333));
    assert_eq!(receipt.new_owner_award, Some(166));
    assert_eq!(
        receipt.continuation,
        CitiesCapturePlunderAwardContinuation::OldOwnerRefund0x0073432d
    );
}

#[test]
fn availability_short_circuits_old_owner_but_still_queries_excluded_type_three() {
    let mut world = World::default();
    world.new_availability[1] = 0;
    world.old_availability[2] = 0;
    let receipt = apply_cities_capture_plunder_award(
        plan_cities_capture_plunder_award(prior_gate(500, false, false)).unwrap(),
        &mut world,
    )
    .unwrap();
    assert_eq!(receipt.new_owner_award, Some(500));
    let availability: Vec<_> = world
        .events
        .iter()
        .filter_map(|event| match event {
            HostEvent::Availability(request) => Some((request.owner, request.type_index)),
            _ => None,
        })
        .collect();
    assert!(availability.contains(&(2, 3)));
    assert!(availability.contains(&(1, 3)));
    assert!(!availability.contains(&(1, 1)));
    assert_eq!(world.buckets, [1_500, 1_000, 1_000, 1_000, 1_500, 1_500]);
}

#[test]
fn mutated_flag_and_bucket_receipts_are_rejected_at_their_commit_points() {
    let mut bad_flag = World {
        corrupt_flag_receipt: true,
        ..World::default()
    };
    assert_eq!(
        apply_cities_capture_plunder_award(
            plan_cities_capture_plunder_award(prior_gate(200, false, false)).unwrap(),
            &mut bad_flag,
        ),
        Err(CitiesCapturePlunderAwardApplyError::FlagMutationEffectsIncomplete)
    );

    let mut bad_bucket = World {
        corrupt_bucket: Some(0),
        ..World::default()
    };
    assert_eq!(
        apply_cities_capture_plunder_award(
            plan_cities_capture_plunder_award(prior_gate(200, false, false)).unwrap(),
            &mut bad_bucket,
        ),
        Err(CitiesCapturePlunderAwardApplyError::BucketAddEffectsIncomplete)
    );
    assert!(bad_bucket.events.iter().any(|event| matches!(
        event,
        HostEvent::Availability(TypeAvailabilityRequest {
            owner: 2,
            type_index: 0,
            mode: TYPE_AVAIL_MODE,
        })
    )));
}
