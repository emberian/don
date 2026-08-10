// SPDX-License-Identifier: GPL-3.0-or-later
#[path = "../src/systems/air_patrol_unit_search_frontier.rs"]
mod frontier;

use std::collections::{BTreeMap, VecDeque};

use frontier::*;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Call {
    Find(FinderQuery),
    Object(ObjectRef),
    Valid(ObjectRef),
    Compare(ObjectRef),
}

#[derive(Default)]
struct Host {
    batches: VecDeque<(SearchPass, Vec<ObjectRef>)>,
    objects: BTreeMap<ObjectRef, ObjectFacts>,
    valid: BTreeMap<ObjectRef, bool>,
    priority: BTreeMap<ObjectRef, i32>,
    calls: Vec<Call>,
}

impl Host {
    fn batch(&mut self, pass: SearchPass, objects: &[ObjectRef]) {
        self.batches.push_back((pass, objects.to_vec()));
    }

    fn object_row(&mut self, o: i32, who: i32, uid: u16, x: i32, y: i32, domain: i32) {
        let target = ObjectRef { o, who };
        self.objects.insert(
            target,
            ObjectFacts {
                target,
                uid,
                x,
                y,
                domain,
            },
        );
        self.valid.insert(target, true);
        self.priority.insert(target, 100);
    }

    fn passes(&self) -> Vec<SearchPass> {
        self.calls
            .iter()
            .filter_map(|call| match call {
                Call::Find(FinderQuery::Units(query)) => Some(query.pass),
                Call::Find(FinderQuery::Builds(query)) => Some(query.pass),
                _ => None,
            })
            .collect()
    }
}

impl UnitSearchHost for Host {
    fn find(&mut self, query: FinderQuery) -> Result<Vec<ObjectRef>, MissingSearchFact> {
        let pass = match query {
            FinderQuery::Units(query) => query.pass,
            FinderQuery::Builds(query) => query.pass,
        };
        self.calls.push(Call::Find(query));
        let Some((expected, objects)) = self.batches.pop_front() else {
            return Err(MissingSearchFact::Finder(pass));
        };
        assert_eq!(pass, expected);
        Ok(objects)
    }

    fn object(&mut self, target: ObjectRef) -> Result<ObjectFacts, MissingSearchFact> {
        self.calls.push(Call::Object(target));
        self.objects
            .get(&target)
            .copied()
            .ok_or(MissingSearchFact::Object(target))
    }

    fn valid_target(&mut self, target: ObjectRef) -> Result<bool, MissingSearchFact> {
        self.calls.push(Call::Valid(target));
        Ok(self.valid.get(&target).copied().unwrap_or(false))
    }

    fn compare_target(&mut self, target: ObjectRef) -> Result<i32, MissingSearchFact> {
        self.calls.push(Call::Compare(target));
        self.priority
            .get(&target)
            .copied()
            .ok_or(MissingSearchFact::CompareTarget(target))
    }
}

fn actor() -> ActorFacts {
    ActorFacts {
        o: 7,
        who: 2,
        x: 1_000,
        y: 2_000,
        object_flags: 0,
    }
}

fn scan(actor_is_bomber: bool) -> PatrolUnitScanInput {
    PatrolUnitScanInput {
        actor: actor(),
        frame: 9,
        is_animal: false,
        returning: 0,
        primary_x: 1_500,
        primary_y: 2_300,
        actor_is_bomber,
        game_option_flags: 0,
        at_last_waypoint: false,
        ranges: RespondRanges::default(),
    }
}

#[test]
fn pdb_extents_rules_and_cadence_are_frozen() {
    assert_eq!(UNIT_DO_AIR_PATROL_VA, 0x005e_a620);
    assert_eq!(UNIT_DO_AIR_PATROL_BYTES, 1_248);
    assert_eq!(FIND_NEW_BOMBER_TARGET_VA, 0x005e_b960);
    assert_eq!(FIND_NEW_BOMBER_TARGET_BYTES, 781);
    assert_eq!(FIND_NEW_AIR_TARGET_VA, 0x005e_bc70);
    assert_eq!(FIND_NEW_AIR_TARGET_BYTES, 873);
    assert_eq!(RespondRanges::default().aircraft, 10);
    assert_eq!(RespondRanges::default().bomber, 12);
    assert!(patrol_unit_scan_due(&scan(false)));
    let mut not_due = scan(false);
    not_due.frame += 1;
    assert!(!patrol_unit_scan_due(&not_due));
    not_due.frame -= 1;
    not_due.is_animal = true;
    assert!(!patrol_unit_scan_due(&not_due));
    not_due.is_animal = false;
    not_due.returning = 1;
    assert!(!patrol_unit_scan_due(&not_due));
}

#[test]
fn air_search_uses_local_then_general_and_keeps_first_equal_score() {
    let a = ObjectRef { o: 3, who: 4 };
    let b = ObjectRef { o: 5, who: 4 };
    let mut host = Host::default();
    host.object_row(a.o, a.who, 30, 1_510, 2_300, DOMAIN_AIR);
    host.object_row(b.o, b.who, 50, 1_490, 2_300, DOMAIN_AIR);
    host.priority.insert(a, 700);
    host.priority.insert(b, 700);
    host.batch(SearchPass::AirActorLocal, &[]);
    host.batch(SearchPass::AirGeneral, &[a, b]);

    let result = find_new_air_target(&mut host, actor(), 1_500, 2_300, -1, -1, 10).unwrap();
    assert_eq!(
        result,
        RawSearchResult {
            target_o: a.o,
            target_who_write: Some(a.who)
        }
    );
    assert_eq!(
        host.passes(),
        vec![SearchPass::AirActorLocal, SearchPass::AirGeneral]
    );

    let Call::Find(FinderQuery::Units(local)) = host.calls[0] else {
        panic!("first finder call is not the local unit pass")
    };
    assert_eq!((local.x, local.y), (actor().x, actor().y));
    assert_eq!(local.max_dist, 10 * WORLD_UNITS_PER_TILE);
    assert_eq!(
        (local.filter, local.filter_data, local.filter_data2),
        (13, 2, 0)
    );
    assert_eq!(local.unread_arg, RawArg::Value(actor().y));

    let general = host
        .calls
        .iter()
        .find_map(|call| match call {
            Call::Find(FinderQuery::Units(query)) if query.pass == SearchPass::AirGeneral => {
                Some(*query)
            }
            _ => None,
        })
        .unwrap();
    assert_eq!((general.x, general.y), (1_500, 2_300));
    assert_eq!(general.filter, 0);
    assert_eq!(general.unread_arg, RawArg::RegisterResidue);
}

#[test]
fn guarded_air_pass_uses_five_tile_scale_and_skips_redundant_distance_gate() {
    let far = ObjectRef { o: 8, who: 5 };
    let mut host = Host::default();
    // Far beyond the broad radius: the finder owns this exclusion. The fold must not add a
    // distance check that retail does not have on the guarded pass.
    host.object_row(far.o, far.who, 80, 99_000, 99_000, DOMAIN_AIR);
    host.batch(SearchPass::AirActorLocal, &[]);
    host.batch(SearchPass::AirGuarded, &[far]);

    let result = find_new_air_target(&mut host, actor(), 1_500, 2_300, 42, 6, 10).unwrap();
    assert_eq!(result.target_o, far.o);
    assert_eq!(
        host.passes(),
        vec![SearchPass::AirActorLocal, SearchPass::AirGuarded]
    );
    let guarded = host
        .calls
        .iter()
        .find_map(|call| match call {
            Call::Find(FinderQuery::Units(query)) if query.pass == SearchPass::AirGuarded => {
                Some(*query)
            }
            _ => None,
        })
        .unwrap();
    assert_eq!(guarded.max_dist, 10 * BROAD_RADIUS_SCALE);
    assert_eq!(
        (guarded.filter, guarded.filter_data, guarded.filter_data2),
        (11, 42, 6)
    );
    assert_eq!(guarded.unread_arg, RawArg::Value(10 * BROAD_RADIUS_SCALE));
}

#[test]
fn air_local_pass_precedes_the_distant_origin_early_return() {
    let target = ObjectRef { o: 12, who: 3 };
    let mut host = Host::default();
    host.object_row(target.o, target.who, 120, 1_010, 2_010, DOMAIN_AIR);
    host.batch(SearchPass::AirActorLocal, &[target]);
    let result = find_new_air_target(&mut host, actor(), 100_000, 100_000, -1, -1, 10).unwrap();
    assert_eq!(
        result,
        RawSearchResult {
            target_o: target.o,
            target_who_write: Some(target.who)
        }
    );

    let mut miss = Host::default();
    miss.batch(SearchPass::AirActorLocal, &[]);
    let result = find_new_air_target(&mut miss, actor(), 100_000, 100_000, -1, -1, 10).unwrap();
    assert_eq!(
        result,
        RawSearchResult {
            target_o: -1,
            target_who_write: None
        }
    );
    assert_eq!(miss.passes(), vec![SearchPass::AirActorLocal]);
}

#[test]
fn bomber_divides_priority_by_whole_tile_distance_and_preserves_ties() {
    let near = ObjectRef { o: 1, who: 4 };
    let far = ObjectRef { o: 2, who: 4 };
    let tie_late = ObjectRef { o: 3, who: 4 };
    let mut host = Host::default();
    host.object_row(near.o, near.who, 10, 1_000, 2_000, 0);
    host.object_row(far.o, far.who, 20, 1_384, 2_000, 0);
    host.object_row(tie_late.o, tie_late.who, 30, 1_000, 2_000, 0);
    host.priority.insert(near, 100);
    host.priority.insert(far, 240); // 240 / (384/192 + 1) = 80
    host.priority.insert(tie_late, 100);
    host.batch(SearchPass::BomberShort, &[near, far, tie_late]);

    let result = find_new_bomber_target(&mut host, actor(), 1_000, 2_000, -1, 99, 12).unwrap();
    assert_eq!(
        result,
        RawSearchResult {
            target_o: near.o,
            target_who_write: Some(near.who)
        }
    );
    let Call::Find(FinderQuery::Builds(query)) = host.calls[0] else {
        panic!("first finder call is not the short build pass")
    };
    assert_eq!(query.max_dist, 12 * WORLD_UNITS_PER_TILE);
    assert_eq!(query.search_mask, 0);
    assert_eq!(query.filter, 0);
    assert_eq!(query.add_to_list, 0);
}

#[test]
fn option_fallback_inverts_search_and_moves_origin_to_actor() {
    let air = ObjectRef { o: 9, who: 6 };
    let mut host = Host::default();
    host.object_row(air.o, air.who, 90, 1_040, 2_000, DOMAIN_AIR);
    host.batch(SearchPass::AirActorLocal, &[]);
    host.batch(SearchPass::AirGeneral, &[]);
    host.batch(SearchPass::BomberShort, &[air]);
    let mut input = scan(false);
    input.game_option_flags = INVERSE_SEARCH_FALLBACK_OPTION;

    let result = run_patrol_unit_scan(&mut host, input).unwrap();
    assert_eq!(result.primary, PatrolSearchKind::Air);
    assert_eq!(result.fallback_called, Some(PatrolSearchKind::Bomber));
    assert_eq!(
        result.action,
        PatrolUnitScanAction::InsertStrafe {
            target: host.objects[&air],
            mandatory: 0,
        }
    );
    assert_eq!(
        host.passes(),
        vec![
            SearchPass::AirActorLocal,
            SearchPass::AirGeneral,
            SearchPass::BomberShort,
        ]
    );
    let fallback = host
        .calls
        .iter()
        .find_map(|call| match call {
            Call::Find(FinderQuery::Builds(query)) => Some(*query),
            _ => None,
        })
        .unwrap();
    assert_eq!((fallback.x, fallback.y), (actor().x, actor().y));
}

#[test]
fn bomber_primary_falls_back_to_air_only_when_option_bit_is_set() {
    let target = ObjectRef { o: 17, who: 5 };
    let mut host = Host::default();
    host.object_row(target.o, target.who, 170, 1_020, 2_020, DOMAIN_AIR);
    host.batch(SearchPass::BomberShort, &[]);
    host.batch(SearchPass::AirActorLocal, &[target]);
    let mut input = scan(true);
    input.game_option_flags = INVERSE_SEARCH_FALLBACK_OPTION;
    let result = run_patrol_unit_scan(&mut host, input).unwrap();
    assert_eq!(result.primary, PatrolSearchKind::Bomber);
    assert_eq!(result.fallback_called, Some(PatrolSearchKind::Air));
    assert!(matches!(
        result.action,
        PatrolUnitScanAction::InsertStrafe { .. }
    ));

    let mut no_option = Host::default();
    no_option.batch(SearchPass::BomberShort, &[]);
    let result = run_patrol_unit_scan(&mut no_option, scan(true)).unwrap();
    assert_eq!(result.fallback_called, None);
    assert_eq!(result.action, PatrolUnitScanAction::NoValidTarget);
    assert_eq!(no_option.passes(), vec![SearchPass::BomberShort]);
}

#[test]
fn non_air_target_is_accepted_only_at_last_waypoint() {
    let ground = ObjectRef { o: 21, who: 7 };
    let mut host = Host::default();
    host.object_row(ground.o, ground.who, 210, 1_010, 2_010, 0);
    host.batch(SearchPass::AirActorLocal, &[ground]);
    let result = run_patrol_unit_scan(&mut host, scan(false)).unwrap();
    assert_eq!(
        result.action,
        PatrolUnitScanAction::RejectGroundBeforeLast {
            target: host.objects[&ground]
        }
    );

    let mut host = Host::default();
    host.object_row(ground.o, ground.who, 210, 1_010, 2_010, 0);
    host.batch(SearchPass::AirActorLocal, &[ground]);
    let mut input = scan(false);
    input.at_last_waypoint = true;
    let result = run_patrol_unit_scan(&mut host, input).unwrap();
    assert!(matches!(
        result.action,
        PatrolUnitScanAction::InsertStrafe { .. }
    ));
}

#[test]
fn force_actor_origin_flag_rewrites_search_function_origin() {
    let mut forced = actor();
    forced.object_flags |= FORCE_ACTOR_ORIGIN_FLAG;
    let mut host = Host::default();
    host.batch(SearchPass::AirActorLocal, &[]);
    host.batch(SearchPass::AirGeneral, &[]);
    let result = find_new_air_target(&mut host, forced, 90_000, 80_000, -1, -1, 10).unwrap();
    assert_eq!(result.target_who_write, Some(-1));
    let general = host
        .calls
        .iter()
        .find_map(|call| match call {
            Call::Find(FinderQuery::Units(query)) if query.pass == SearchPass::AirGeneral => {
                Some(*query)
            }
            _ => None,
        })
        .unwrap();
    assert_eq!((general.x, general.y), (forced.x, forced.y));
}

#[test]
fn missing_host_fact_is_not_an_empty_search() {
    let mut host = Host::default();
    assert_eq!(
        find_new_air_target(&mut host, actor(), 1_500, 2_300, -1, -1, 10),
        Err(MissingSearchFact::Finder(SearchPass::AirActorLocal))
    );
}
