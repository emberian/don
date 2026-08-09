#[path = "../src/systems/army_do_forming.rs"]
mod army_do_forming;

use army_do_forming::{
    do_forming, FormingArmy, FormingExit, FormingHost, FormingMoveOrder, FormingSiegeOrder,
    MissingFact, TargetTypeFact,
};

#[derive(Clone, Debug, PartialEq, Eq)]
enum Event {
    Engaged,
    Project(i32, i32, i32, i32),
    GroupNum(i32),
    GroupArmy(i32, i32),
    LeaderIdentity(usize),
    Diplomacy(usize, i32),
    SetupRelation(i32, i32),
    Age(usize),
    Mobile,
    Stance(i32),
    FindBuilding(i32, i32, usize),
    Enemy(usize, i32),
    TargetType(i32, i32),
    Move(FormingMoveOrder),
    Siege(FormingSiegeOrder),
    Advance(i32, i32, i32, i32),
}

struct Host {
    events: Vec<Event>,
    engaged: Option<bool>,
    normalized_list: Option<[i32; 16]>,
    projected: Option<(i32, i32)>,
    group_nums: [Option<i32>; 8],
    leader_identity: Option<i32>,
    diplomacy: Option<i32>,
    setup_relation: Option<i32>,
    age: Option<i32>,
    mobile: Option<i32>,
    building: Option<bool>,
    enemy: Option<bool>,
    target_type: Option<TargetTypeFact>,
    writes_work: bool,
    step: Option<(i32, i32)>,
}

impl Default for Host {
    fn default() -> Self {
        Self {
            events: Vec::new(),
            engaged: Some(false),
            normalized_list: None,
            projected: Some((1_000, 2_000)),
            group_nums: [Some(1); 8],
            leader_identity: Some(0),
            diplomacy: Some(2),
            setup_relation: Some(2),
            age: Some(4),
            mobile: Some(1),
            building: Some(false),
            enemy: Some(true),
            target_type: Some(TargetTypeFact::Flags(3)),
            writes_work: true,
            step: Some((10, -20)),
        }
    }
}

impl FormingHost for Host {
    fn is_engaged(&mut self, army: &mut FormingArmy) -> Option<bool> {
        self.events.push(Event::Engaged);
        if let Some(list) = self.normalized_list {
            army.list = list;
        }
        self.engaged
    }

    fn project(&mut self, x: i32, y: i32, angle: i32, distance: i32) -> Option<(i32, i32)> {
        self.events.push(Event::Project(x, y, angle, distance));
        self.projected
    }

    fn group_num(&mut self, group: i32) -> Option<i32> {
        self.events.push(Event::GroupNum(group));
        self.group_nums[group as usize]
    }

    fn set_group_army(&mut self, group: i32, army: i32) -> bool {
        self.events.push(Event::GroupArmy(group, army));
        self.writes_work
    }

    fn leader_identity(&mut self, who: usize) -> Option<i32> {
        self.events.push(Event::LeaderIdentity(who));
        self.leader_identity
    }

    fn diplomacy(&mut self, who: usize, other: i32) -> Option<i32> {
        self.events.push(Event::Diplomacy(who, other));
        self.diplomacy
    }

    fn setup_relation(&mut self, target_who: i32, owner_identity: i32) -> Option<i32> {
        self.events
            .push(Event::SetupRelation(target_who, owner_identity));
        self.setup_relation
    }

    fn leader_age(&mut self, who: usize) -> Option<i32> {
        self.events.push(Event::Age(who));
        self.age
    }

    fn count_mobile(&mut self, _army: &FormingArmy) -> Option<i32> {
        self.events.push(Event::Mobile);
        self.mobile
    }

    fn set_stance(&mut self, _army: &FormingArmy, stance: i32) -> bool {
        self.events.push(Event::Stance(stance));
        self.writes_work
    }

    fn building_found(&mut self, x: i32, y: i32, who: usize) -> Option<bool> {
        self.events.push(Event::FindBuilding(x, y, who));
        self.building
    }

    fn is_enemy(&mut self, who: usize, other: i32) -> Option<bool> {
        self.events.push(Event::Enemy(who, other));
        self.enemy
    }

    fn target_type(&mut self, who: i32, object: i32) -> Option<TargetTypeFact> {
        self.events.push(Event::TargetType(who, object));
        self.target_type
    }

    fn issue_move(&mut self, order: FormingMoveOrder) -> bool {
        self.events.push(Event::Move(order));
        self.writes_work
    }

    fn issue_siege(&mut self, order: FormingSiegeOrder) -> bool {
        self.events.push(Event::Siege(order));
        self.writes_work
    }

    fn advance_cursor(&mut self, x: i32, y: i32, angle: i32, spacing: i32) -> Option<(i32, i32)> {
        self.events.push(Event::Advance(x, y, angle, spacing));
        self.step.map(|(dx, dy)| (x + dx, y + dy))
    }
}

fn one_group() -> FormingArmy {
    let mut army = FormingArmy {
        army: 7,
        target_o: 11,
        target_who: 1,
        muster_x: 2,
        muster_y: 3,
        muster_angle: 0x1234_5678,
        who: 0,
        num_groups: 1,
        ..FormingArmy::default()
    };
    army.list[0] = 2;
    army
}

#[test]
fn engagement_is_the_no_write_return_zero() {
    let mut army = one_group();
    let mut host = Host {
        engaged: Some(true),
        ..Host::default()
    };
    let out = do_forming(&mut army, &mut host);
    assert_eq!(out.retail_return(), Some(0));
    assert_eq!(host.events, vec![Event::Engaged]);
    assert_eq!(out.trace(), Default::default());
}

#[test]
fn formation_walk_observes_the_list_order_written_by_is_engaged_normalize() {
    let mut army = one_group();
    let mut normalized = [-1; 16];
    normalized[0] = 3;
    let mut host = Host {
        normalized_list: Some(normalized),
        ..Host::default()
    };
    let out = do_forming(&mut army, &mut host);
    assert!(matches!(out, FormingExit::Formed(_)));
    assert!(host.events.contains(&Event::GroupNum(3)));
    assert!(!host.events.contains(&Event::GroupNum(2)));
    assert_eq!(army.list[0], 3);
}

#[test]
fn land_target_moves_in_exact_mutation_order_and_freezes_all_arguments() {
    let mut army = one_group();
    let mut host = Host::default();
    let out = do_forming(&mut army, &mut host);
    assert_eq!(out.retail_return(), Some(1));

    let order = FormingMoveOrder {
        group: 2,
        x: 1_000,
        y: 2_000,
        queue_pos: 2,
        arg_4: 1,
        angle: army.muster_angle,
        order_index: 2,
        arg_7: 1,
        arg_8: -1,
        arg_9: -1,
        arg_10: 0,
    };
    assert_eq!(
        host.events,
        vec![
            Event::Engaged,
            Event::Project(1_920, 2_688, army.muster_angle, 0xC0),
            Event::GroupNum(2),
            Event::GroupArmy(2, 7),
            Event::LeaderIdentity(0),
            Event::Diplomacy(0, 1),
            Event::SetupRelation(1, 0),
            Event::FindBuilding(1_000, 2_000, 0),
            Event::Enemy(0, 1),
            Event::TargetType(1, 11),
            Event::Stance(0),
            Event::Move(order),
            Event::Advance(1_000, 2_000, army.muster_angle, 0x180),
        ]
    );
    let trace = out.trace();
    assert_eq!((trace.live_groups, trace.move_orders), (1, 1));
    assert_eq!((trace.group_army_writes, trace.formation_steps), (1, 1));
}

#[test]
fn force_stance_three_precedes_building_probe_and_move_stance() {
    let mut army = one_group();
    let mut host = Host {
        diplomacy: Some(0),
        age: Some(3),
        mobile: Some(0),
        ..Host::default()
    };
    let out = do_forming(&mut army, &mut host);
    assert!(matches!(out, FormingExit::Formed(_)));
    assert_eq!(
        host.events
            .iter()
            .filter(|event| matches!(event, Event::Mobile))
            .count(),
        1,
        "building_found=false short-circuits the second count(4,0)"
    );
    let stance_three = host
        .events
        .iter()
        .position(|event| *event == Event::Stance(3))
        .unwrap();
    let building = host
        .events
        .iter()
        .position(|event| matches!(event, Event::FindBuilding(..)))
        .unwrap();
    let stance_zero = host
        .events
        .iter()
        .position(|event| *event == Event::Stance(0))
        .unwrap();
    assert!(stance_three < building && building < stance_zero);
    assert!(!host
        .events
        .iter()
        .any(|event| matches!(event, Event::SetupRelation(..))));
}

#[test]
fn blocking_building_issues_siege_and_skips_all_target_queries() {
    let mut army = one_group();
    let mut host = Host {
        building: Some(true),
        mobile: Some(2),
        ..Host::default()
    };
    let out = do_forming(&mut army, &mut host);
    assert!(matches!(out, FormingExit::Formed(_)));
    assert!(host.events.contains(&Event::Siege(FormingSiegeOrder {
        group: 2,
        x: 1_000,
        y: 2_000,
        angle: army.muster_angle,
    })));
    assert!(!host
        .events
        .iter()
        .any(|event| matches!(event, Event::Enemy(..) | Event::TargetType(..))));
}

#[test]
fn navy_bypasses_enemy_and_target_object_facts() {
    let mut army = one_group();
    army.navy = 1;
    army.target_o = -1;
    army.target_who = -1;
    let mut host = Host {
        enemy: None,
        target_type: None,
        ..Host::default()
    };
    let out = do_forming(&mut army, &mut host);
    assert_eq!(out.trace().move_orders, 1);
    assert!(!host
        .events
        .iter()
        .any(|event| matches!(event, Event::Enemy(..) | Event::TargetType(..))));
}

#[test]
fn negative_target_owner_is_queried_before_negative_target_object() {
    let mut army = one_group();
    army.target_who = -1;
    army.target_o = -1;
    let mut host = Host {
        enemy: Some(false),
        ..Host::default()
    };
    let out = do_forming(&mut army, &mut host);
    assert_eq!(out.trace().siege_orders, 1);
    assert!(host.events.contains(&Event::Enemy(0, -1)));
    assert!(!host
        .events
        .iter()
        .any(|event| matches!(event, Event::TargetType(..))));
}

#[test]
fn sentinels_and_zero_groups_do_not_consume_spacing() {
    let mut army = one_group();
    army.num_groups = 3;
    army.list[..3].copy_from_slice(&[-1, 2, 3]);
    let mut host = Host::default();
    host.group_nums[2] = Some(0);
    host.group_nums[3] = Some(-1);
    let out = do_forming(&mut army, &mut host);
    let trace = out.trace();
    assert_eq!((trace.groups_examined, trace.live_groups), (3, 1));
    assert_eq!(trace.formation_steps, 1);
    assert_eq!(
        host.events
            .iter()
            .filter(|event| matches!(event, Event::Advance(..)))
            .count(),
        1
    );
}

#[test]
fn missing_fact_stops_after_the_group_army_write() {
    let mut army = one_group();
    let mut host = Host {
        leader_identity: None,
        ..Host::default()
    };
    let out = do_forming(&mut army, &mut host);
    assert!(matches!(
        out,
        FormingExit::Missing {
            fact: MissingFact::LeaderIdentity { who: 0 },
            ..
        }
    ));
    assert_eq!(out.trace().group_army_writes, 1);
    assert!(host.events.contains(&Event::GroupArmy(2, 7)));
    assert!(!host.events.iter().any(|event| matches!(
        event,
        Event::FindBuilding(..) | Event::Move(..) | Event::Siege(..) | Event::Advance(..)
    )));
}

#[test]
fn impossible_live_prefix_fails_closed_instead_of_clamping() {
    let mut army = one_group();
    army.num_groups = 17;
    army.list = [-1; 16];
    let mut host = Host::default();
    let out = do_forming(&mut army, &mut host);
    assert!(matches!(
        out,
        FormingExit::Missing {
            fact: MissingFact::ArmyList { cursor: 16 },
            ..
        }
    ));
    assert_eq!(out.trace().groups_examined, 17);
}
