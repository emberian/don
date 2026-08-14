// SPDX-License-Identifier: GPL-3.0-or-later

use don_sim::systems::armies::{
    ArmyData, ST_DEFENDING, ST_FORMING, ST_MARCHING, ST_MUSTERING, ST_TRANSPORTING,
};
use don_sim::systems::army_do_mustering::{
    do_mustering, MusteringCity, MusteringExit, MusteringHost, MusteringMissingFact,
};

#[derive(Clone, Debug, PartialEq, Eq)]
enum Event {
    Release,
    City(usize, i32),
    Find(i32, i32, i32),
    Strategy(usize, i32),
    Difficulty(usize),
    Flags(usize),
}

struct Host {
    events: Vec<Event>,
    released: Option<bool>,
    city: Option<MusteringCity>,
    find: Option<bool>,
    strategy: Option<u16>,
    difficulty: Option<i32>,
    flags: Option<u32>,
    find_mutation: Option<i32>,
}

impl Default for Host {
    fn default() -> Self {
        Self {
            events: Vec::new(),
            released: Some(true),
            city: Some(MusteringCity {
                flags: 1,
                object: 17,
            }),
            find: Some(false),
            strategy: Some(0),
            difficulty: Some(4),
            flags: Some(1),
            find_mutation: None,
        }
    }
}

impl MusteringHost for Host {
    fn release_mustering(&mut self, _army: &ArmyData) -> Option<bool> {
        self.events.push(Event::Release);
        self.released
    }

    fn city(&mut self, who: usize, city: i32) -> Option<MusteringCity> {
        self.events.push(Event::City(who, city));
        self.city
    }

    fn find_muster_spot(
        &mut self,
        army: &mut ArmyData,
        object: i32,
        who: i32,
        forced: i32,
    ) -> Option<bool> {
        self.events.push(Event::Find(object, who, forced));
        if let Some(x) = self.find_mutation {
            army.muster_x = x;
        }
        self.find
    }

    fn strategy(&mut self, who: usize, region: i32) -> Option<u16> {
        self.events.push(Event::Strategy(who, region));
        self.strategy
    }

    fn difficulty(&mut self, who: usize) -> Option<i32> {
        self.events.push(Event::Difficulty(who));
        self.difficulty
    }

    fn leader_flags(&mut self, who: usize) -> Option<u32> {
        self.events.push(Event::Flags(who));
        self.flags
    }
}

fn army() -> ArmyData {
    ArmyData {
        valid: 1,
        status: ST_MUSTERING,
        reg: 6,
        city: 4,
        muster_x: 2,
        muster_y: 3,
        muster_angle: 0x1234_5678,
        who: 2,
        ..ArmyData::default()
    }
}

#[test]
fn released_navy_skips_every_land_fact_and_rewrites_the_rally() {
    let mut army = army();
    army.navy = 1;
    let mut host = Host {
        city: None,
        strategy: None,
        difficulty: None,
        flags: None,
        ..Host::default()
    };
    let out = do_mustering(&mut army, &mut host);
    assert_eq!(out.retail_return(), Some(1));
    assert_eq!(host.events, vec![Event::Release]);
    assert_eq!(army.status, ST_MARCHING);
    assert_eq!(army.city, -1);
    assert_eq!((army.x, army.y), (0x780, 0xa80));
    assert_eq!(army.angle, army.muster_angle);
}

#[test]
fn released_land_strategy_four_uses_difficulty_before_defending() {
    let mut army = army();
    let mut host = Host {
        strategy: Some(4),
        difficulty: Some(2),
        ..Host::default()
    };
    let out = do_mustering(&mut army, &mut host);
    assert!(matches!(out, MusteringExit::Dispatched(_)));
    assert_eq!(army.status, ST_DEFENDING);
    assert_eq!(
        host.events,
        vec![Event::Release, Event::Strategy(2, 6), Event::Difficulty(2)]
    );
}

#[test]
fn released_land_strategy_eight_and_ai_flags_transport() {
    let mut army = army();
    let mut host = Host {
        strategy: Some(8),
        flags: Some(0x100),
        ..Host::default()
    };
    do_mustering(&mut army, &mut host);
    assert_eq!(army.status, ST_TRANSPORTING);
    assert_eq!(
        host.events,
        vec![Event::Release, Event::Strategy(2, 6), Event::Flags(2)]
    );
}

#[test]
fn successful_muster_search_sets_forming_and_skips_common_rewrite() {
    let mut army = army();
    let before = army.clone();
    let mut host = Host {
        released: Some(false),
        find: Some(true),
        strategy: None,
        ..Host::default()
    };
    let out = do_mustering(&mut army, &mut host);
    assert!(matches!(out, MusteringExit::Forming(_)));
    assert_eq!(army.status, ST_MUSTERING | ST_FORMING);
    assert_eq!(army.city, before.city);
    assert_eq!(
        (army.x, army.y, army.angle),
        (before.x, before.y, before.angle)
    );
    assert_eq!(
        host.events,
        vec![Event::Release, Event::City(2, 4), Event::Find(17, 2, 1)]
    );
}

#[test]
fn inactive_city_skips_find_and_falls_through_to_marching() {
    let mut army = army();
    let mut host = Host {
        released: Some(false),
        city: Some(MusteringCity {
            flags: 0,
            object: 17,
        }),
        ..Host::default()
    };
    do_mustering(&mut army, &mut host);
    assert_eq!(army.status, ST_MARCHING);
    assert_eq!(
        host.events,
        vec![Event::Release, Event::City(2, 4), Event::Strategy(2, 6)]
    );
}

#[test]
fn unreleased_large_force_transports_only_after_strategy_and_flag_tests() {
    let mut army = army();
    army.city = -1;
    army.num_captains = 8;
    let mut host = Host {
        released: Some(false),
        strategy: Some(8),
        flags: Some(0x200),
        ..Host::default()
    };
    do_mustering(&mut army, &mut host);
    assert_eq!(army.status, ST_TRANSPORTING);
    assert_eq!(
        host.events,
        vec![Event::Release, Event::Strategy(2, 6), Event::Flags(2)]
    );
}

#[test]
fn missing_fact_stops_after_find_muster_spot_prefix_mutations() {
    let mut army = army();
    let mut host = Host {
        released: Some(false),
        find: Some(false),
        find_mutation: Some(91),
        strategy: None,
        ..Host::default()
    };
    let out = do_mustering(&mut army, &mut host);
    assert!(matches!(
        out,
        MusteringExit::Missing {
            fact: MusteringMissingFact::Strategy { who: 2, region: 6 },
            ..
        }
    ));
    assert_eq!(army.muster_x, 91);
    assert_eq!(army.status, ST_MUSTERING);
    assert_eq!(army.city, 4);
}
