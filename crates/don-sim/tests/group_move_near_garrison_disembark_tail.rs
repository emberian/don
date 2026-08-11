// SPDX-License-Identifier: GPL-3.0-or-later

#[path = "../src/systems/group_move_near_garrison_disembark_tail.rs"]
mod tail;

use std::cell::Cell;

use tail::*;

#[derive(Clone)]
struct GarrisonHost {
    angle: Option<u32>,
    nearby: Option<NearbySpotAnswer>,
}

impl ArmyGarrisonHost for GarrisonHost {
    fn find_angle(&self, _dx: i32, _dy: i32) -> Option<u32> {
        self.angle
    }

    fn find_nearby_spot(&self, _request: &NearbySpotRequest) -> Option<NearbySpotAnswer> {
        self.nearby
    }
}

struct PathHost {
    epoch: Cell<u64>,
    paths: Vec<(FindWPathRequest, FindWPathAnswer)>,
    split_region_at_x: Option<i32>,
    invalid: Option<bool>,
}

impl PathHost {
    fn uniform() -> Self {
        Self {
            epoch: Cell::new(7),
            paths: Vec::new(),
            split_region_at_x: None,
            invalid: Some(false),
        }
    }
}

impl DisembarkTailHost for PathHost {
    fn epoch(&self) -> u64 {
        self.epoch.get()
    }

    fn find_wpath(&self, request: &FindWPathRequest) -> Option<FindWPathAnswer> {
        self.paths
            .iter()
            .find(|(expected, _)| expected == request)
            .map(|(_, answer)| answer.clone())
    }

    fn terrain_region(&self, request: TerrainRegionRequest) -> Option<u16> {
        Some(match self.split_region_at_x {
            Some(split) if request.x >= split => 2,
            _ => 1,
        })
    }

    fn invalid_loc(&self, _request: InvalidLocRequest) -> Option<bool> {
        self.invalid
    }
}

fn unit(object: i16, x: i32, y: i32) -> TailUnit {
    TailUnit {
        key: UnitKey::new(2, object),
        valid: true,
        on_map: true,
        plane_excluded: false,
        domain: 0,
        is_supply: false,
        is_siege: false,
        is_hero: false,
        current_order: Some(MOVE_TO),
        current_order_pause: Some(0),
        pathfinder_counter_b2: 0,
        movement_gate_607b40: 1,
        x,
        y,
        path: Vec::new(),
        version: 10,
    }
}

fn base_state() -> MoveNearTailState {
    MoveNearTailState {
        owner: 2,
        members: vec![10, 11],
        army_mode: false,
        city_lookup_ran: false,
        city: None,
        leader_flags: 0,
        orders: MOVE_TO,
        tolerance: 0x90,
        origin_x: 100,
        origin_y: 100,
        formation: FormationTail {
            leader_index: 0,
            to_x: vec![100, 200],
            to_y: vec![100, 100],
            permutation: vec![0, 1],
            resolved_form: 0,
        },
        world: TailWorldShape {
            tile_width: 16,
            tile_height: 16,
            coarse_width: 64,
            coarse_height: 64,
        },
        units: vec![unit(10, 100, 100), unit(11, 200, 100)],
        group_order_num: 41,
        form_scratch: vec![0xa5; 0xe60],
        external_epoch: 91,
        rng_epoch: 1234,
    }
}

fn army_member() -> ArmyMemberFacts {
    ArmyMemberFacts {
        actor: UnitKey::new(2, 10),
        x: 0x900,
        y: 0xa00,
        valid: true,
        on_map: true,
        plane_excluded: false,
        is_supply: true,
        is_siege: false,
        is_hero: false,
        current_order: Some(MOVE_TO),
        can_garrison_city: Some(true),
    }
}

fn city() -> CityTarget {
    CityTarget {
        key: UnitKey::new(2, 77),
        type_index: 0x1a0,
        x: 0x600,
        y: 0x500,
    }
}

#[test]
fn capstone_stack_layout_restores_the_missing_group_word() {
    let abi = MoveNearAbi::from_wire(2, 8, -1, 1);
    assert_eq!(abi.stack_tail(), [2, 1, 8, -1, 1]);
    assert_eq!(MoveNearAbi::ORDERS_STACK_OFFSET, 0x20);
    assert_eq!(MoveNearAbi::GROUP_STACK_OFFSET, 0x24);
    assert_eq!(MoveNearAbi::FORM_STACK_OFFSET, 0x28);
    assert_eq!(MoveNearAbi::WIDTH_STACK_OFFSET, 0x2c);
    assert_eq!(MoveNearAbi::DISEMBARK_STACK_OFFSET, 0x30);
    assert_eq!(DISEMBARK_SLICE_BYTES, 3_399);
    assert_eq!(GARRISON_SLICE_BYTES, 642);
    assert_eq!(FIND_WPATH_STACK_BYTES, 0x14);
}

#[test]
fn supply_siege_or_hero_can_install_the_exact_garrison_tuple() {
    let host = GarrisonHost {
        angle: None,
        nearby: None,
    };
    let decision = plan_army_city_member(true, true, Some(city()), &army_member(), &host).unwrap();
    assert_eq!(
        decision,
        ArmyMemberDecision::InstallGarrison(GarrisonInstall {
            actor: UnitKey::new(2, 10),
            target: UnitKey::new(2, 77),
            search: 0,
            queue: QUEUE_NEW,
            group: 0,
        })
    );
}

#[test]
fn failed_city_garrison_search_preserves_the_odd_destination_y_abi_word() {
    let mut member = army_member();
    member.can_garrison_city = Some(false);
    let host = GarrisonHost {
        angle: Some(0x1234_5678),
        nearby: Some(NearbySpotAnswer {
            return_value: 0,
            out_x: 0x710,
            out_y: 0x820,
        }),
    };
    let decision = plan_army_city_member(true, true, Some(city()), &member, &host).unwrap();
    let ArmyMemberDecision::InstallMove {
        install,
        angle_request,
        angle,
        nearby_request,
        ..
    } = decision
    else {
        panic!("expected move fallback")
    };
    assert_eq!(angle_request, (0x300, 0x500));
    assert_eq!(angle, 0x1234_5678);
    assert_eq!(nearby_request.min_radius, 0x300);
    assert_eq!(nearby_request.max_radius, 0x600);
    assert_eq!(nearby_request.tail, [0, 0, -1, 0, -1]);
    assert_eq!((install.x, install.y), (0x710, 0x820));
    assert_eq!(install.raw_arg7, 0x820);
    assert_eq!((install.original_x, install.original_y), (-1, -1));
}

#[test]
fn failed_nearby_search_uses_the_city_centre_and_siege_attack_is_skipped_without_a_city() {
    let mut member = army_member();
    member.can_garrison_city = Some(false);
    let host = GarrisonHost {
        angle: Some(9),
        nearby: Some(NearbySpotAnswer {
            return_value: 1,
            out_x: 99,
            out_y: 88,
        }),
    };
    let decision = plan_army_city_member(true, true, Some(city()), &member, &host).unwrap();
    let ArmyMemberDecision::InstallMove { install, .. } = decision else {
        panic!("expected move fallback")
    };
    assert_eq!(
        (install.x, install.y, install.raw_arg7),
        (0x600, 0x500, 0x500)
    );

    member.is_supply = false;
    member.is_siege = true;
    member.current_order = Some(ATTACK_ORDER);
    member.can_garrison_city = None;
    assert_eq!(
        plan_army_city_member(true, true, None, &member, &host),
        Ok(ArmyMemberDecision::Skip)
    );
}

#[test]
fn branch_lazy_garrison_facts_fail_closed() {
    let mut member = army_member();
    member.can_garrison_city = None;
    let host = GarrisonHost {
        angle: None,
        nearby: None,
    };
    assert_eq!(
        plan_army_city_member(true, true, Some(city()), &member, &host),
        Err(ArmyMemberPlanError::MissingCanGarrison)
    );
    member.is_supply = false;
    member.can_garrison_city = Some(false);
    assert_eq!(
        plan_army_city_member(false, false, None, &member, &host),
        Err(ArmyMemberPlanError::UnexpectedCanGarrison)
    );
}

#[test]
fn straight_leader_seed_scatters_to_every_member_and_consumes_no_rng() {
    let mut state = base_state();
    let host = PathHost::uniform();
    let receipt = preflight_disembark_tail(&state, &host).unwrap();
    assert!(receipt.plan.used_group_path);
    assert!(receipt.plan.straight_path);
    assert!(receipt.plan.rng_draws.is_empty());
    assert_eq!(receipt.plan.after.rng_epoch, state.rng_epoch);
    assert_eq!(
        receipt.plan.after.units[0].path,
        vec![PathData {
            to_x: 100,
            to_y: 100,
            tolerance: 0x90,
            flags: 1,
        }]
    );
    assert_eq!(
        receipt.plan.after.units[1].path,
        vec![PathData {
            to_x: 200,
            to_y: 100,
            tolerance: 0x90,
            flags: 1,
        }]
    );
    assert_eq!(receipt.plan.after.group_order_num, 42);
    assert!(receipt
        .plan
        .after
        .form_scratch
        .iter()
        .all(|&byte| byte == 0));
    assert_eq!(
        receipt
            .plan
            .host_calls
            .iter()
            .filter(|call| matches!(call, TailHostCall::FindWPath { .. }))
            .count(),
        0
    );
    commit_disembark_tail(&mut state, &receipt, &host).unwrap();
    assert_eq!(state, receipt.plan.after);
}

#[test]
fn pathfinder_stack_is_popped_then_each_complete_unit_path_is_inverted() {
    let mut state = base_state();
    state.origin_x = 0;
    state.origin_y = 0;
    state.orders = 2;
    state.formation.to_x = vec![3_000, 3_100];
    state.formation.to_y = vec![3_000, 3_000];
    state.units[0].path = vec![PathData {
        to_x: 0,
        to_y: 0,
        tolerance: 1,
        flags: 7,
    }];
    let seed = PathData {
        to_x: 3_000,
        to_y: 3_000,
        tolerance: 0x90,
        flags: 1,
    };
    let request = FindWPathRequest {
        stack: vec![seed],
        start_x: 0,
        start_y: 0,
        owner: 2,
        object: 10,
        army_path_mode: false,
        rng_epoch: 1234,
    };
    let a = PathData {
        to_x: 3_000,
        to_y: 2_000,
        tolerance: 2,
        flags: 1,
    };
    let b = PathData {
        to_x: 2_000,
        to_y: 1_000,
        tolerance: 3,
        flags: 1,
    };
    let mut host = PathHost::uniform();
    host.paths.push((
        request,
        FindWPathAnswer {
            return_value: 777,
            stack: vec![a, b],
            rng_draws: Vec::new(),
            rng_epoch_after: 1234,
            unit_mutation: None,
        },
    ));
    let receipt = preflight_disembark_tail(&state, &host).unwrap();
    assert!(receipt.plan.used_group_path);
    assert!(!receipt.plan.straight_path);
    // Existing, then popped B, then popped A, then retail inverts the whole Stack.
    assert_eq!(
        receipt.plan.after.units[0].path,
        vec![a, b, state.units[0].path[0]]
    );
    assert_eq!(
        receipt.plan.after.units[1].path,
        vec![PathData { to_x: 3_100, ..a }, PathData { to_x: 2_100, ..b },]
    );
}

#[test]
fn empty_group_path_runs_one_exact_fallback_request_per_eligible_member() {
    let mut state = base_state();
    state.units[0].valid = false;
    let seed = PathData {
        to_x: 200,
        to_y: 100,
        tolerance: 0x90,
        flags: 1,
    };
    let request = FindWPathRequest {
        stack: vec![seed],
        start_x: 100,
        start_y: 100,
        owner: 2,
        object: 11,
        army_path_mode: false,
        rng_epoch: 1234,
    };
    let mut host = PathHost::uniform();
    host.paths.push((
        request.clone(),
        FindWPathAnswer {
            return_value: -1,
            stack: Vec::new(),
            rng_draws: Vec::new(),
            rng_epoch_after: 1234,
            unit_mutation: None,
        },
    ));
    let receipt = preflight_disembark_tail(&state, &host).unwrap();
    assert!(!receipt.plan.used_group_path);
    assert_eq!(receipt.plan.after.units[0].path, Vec::<PathData>::new());
    assert_eq!(receipt.plan.after.units[1].path, vec![seed]);
    assert_eq!(
        receipt.plan.host_calls,
        vec![TailHostCall::FindWPath {
            request,
            answer: FindWPathAnswer {
                return_value: -1,
                stack: Vec::new(),
                rng_draws: Vec::new(),
                rng_epoch_after: 1234,
                unit_mutation: None,
            },
        }]
    );
}

#[test]
fn sea_members_obey_the_noncorner_leader_only_filter() {
    let mut state = base_state();
    state.origin_x = 0;
    state.origin_y = 0;
    state.orders = 2;
    state.formation.to_x = vec![3_000, 3_100];
    state.formation.to_y = vec![3_000, 3_000];
    state.units[1].domain = DOMAIN_SEA;
    let seed = PathData {
        to_x: 3_000,
        to_y: 3_000,
        tolerance: 0x90,
        flags: 1,
    };
    let noncorner = PathData {
        to_x: 2_000,
        to_y: 2_000,
        tolerance: 0,
        flags: 0,
    };
    let request = FindWPathRequest {
        stack: vec![seed],
        start_x: 0,
        start_y: 0,
        owner: 2,
        object: 10,
        army_path_mode: false,
        rng_epoch: 1234,
    };
    let mut host = PathHost::uniform();
    host.paths.push((
        request,
        FindWPathAnswer {
            return_value: 0,
            stack: vec![noncorner],
            rng_draws: Vec::new(),
            rng_epoch_after: 1234,
            unit_mutation: None,
        },
    ));
    let receipt = preflight_disembark_tail(&state, &host).unwrap();
    assert_eq!(receipt.plan.after.units[0].path.len(), 1);
    assert!(receipt.plan.after.units[1].path.is_empty());

    state.leader_flags = LEADER_FLAG_DISABLE_SEA_LEADER_ONLY;
    let receipt = preflight_disembark_tail(&state, &host).unwrap();
    assert_eq!(receipt.plan.after.units[1].path.len(), 1);
}

#[test]
fn changed_terrain_region_calls_invalid_loc_and_rehomes_to_the_path_tile() {
    let mut state = base_state();
    state.origin_x = 0;
    state.origin_y = 0;
    state.orders = 2;
    state.formation.to_x = vec![3_000, 3_800];
    state.formation.to_y = vec![3_000, 3_000];
    state.formation.permutation = vec![1, 0];
    state.formation.resolved_form = FORM_SCATTER_PERMUTATION;
    let seed = PathData {
        to_x: 3_000,
        to_y: 3_000,
        tolerance: 0x90,
        flags: 1,
    };
    let node = PathData {
        to_x: 0x320,
        to_y: 0x120,
        tolerance: 0,
        flags: 0,
    };
    let request = FindWPathRequest {
        stack: vec![seed],
        start_x: 0,
        start_y: 0,
        owner: 2,
        object: 10,
        army_path_mode: false,
        rng_epoch: 1234,
    };
    let mut host = PathHost::uniform();
    host.split_region_at_x = Some(0x500);
    host.invalid = Some(true);
    host.paths.push((
        request,
        FindWPathAnswer {
            return_value: 0,
            stack: vec![node],
            rng_draws: Vec::new(),
            rng_epoch_after: 1234,
            unit_mutation: None,
        },
    ));
    let receipt = preflight_disembark_tail(&state, &host).unwrap();
    let invalid_calls: Vec<_> = receipt
        .plan
        .host_calls
        .iter()
        .filter_map(|call| match call {
            TailHostCall::InvalidLoc { request, .. } => Some(*request),
            _ => None,
        })
        .collect();
    assert!(!invalid_calls.is_empty());
    assert!(invalid_calls
        .iter()
        .all(|request| request.raw == [1, 1, 0, 0, 0, 0]));
    // Every repaired point is moved into the same 0x300 tile as the path node.
    for unit in &receipt.plan.after.units {
        for point in &unit.path {
            assert_eq!(point.to_x / 0x300, node.to_x / 0x300);
        }
    }
}

#[test]
fn astar_rng_counter_and_current_order_pause_commit_as_one_receipt() {
    let mut state = base_state();
    state.origin_x = 0;
    state.origin_y = 0;
    state.formation.to_x = vec![3_000, 3_100];
    state.formation.to_y = vec![3_000, 3_000];
    state.units[0].pathfinder_counter_b2 = 250;
    state.units[0].current_order_pause = Some(4);
    let seed = PathData {
        to_x: 3_000,
        to_y: 3_000,
        tolerance: 0x90,
        flags: 1,
    };
    let request = FindWPathRequest {
        stack: vec![seed],
        start_x: 0,
        start_y: 0,
        owner: 2,
        object: 10,
        army_path_mode: false,
        rng_epoch: 1234,
    };
    let mutation = PathFinderUnitMutation {
        actor: UnitKey::new(2, 10),
        counter_b2_before: 250,
        counter_b2_after: 24,
        pause: Some(PathFinderPauseMutation {
            before: 4,
            draw: 10,
            after: 7,
        }),
    };
    let mut host = PathHost::uniform();
    host.paths.push((
        request,
        FindWPathAnswer {
            return_value: 0,
            stack: vec![seed],
            rng_draws: vec![10],
            rng_epoch_after: 1235,
            unit_mutation: Some(mutation),
        },
    ));

    let receipt = preflight_disembark_tail(&state, &host).unwrap();
    assert_eq!(receipt.plan.rng_draws, vec![10]);
    assert_eq!(receipt.plan.after.rng_epoch, 1235);
    assert_eq!(receipt.plan.after.units[0].pathfinder_counter_b2, 24);
    assert_eq!(receipt.plan.after.units[0].current_order_pause, Some(7));
    commit_disembark_tail(&mut state, &receipt, &host).unwrap();
    assert_eq!(state, receipt.plan.after);
}

#[test]
fn malformed_astar_pause_effect_fails_before_any_state_mutation() {
    let state = {
        let mut state = base_state();
        state.origin_x = 0;
        state.origin_y = 0;
        state.formation.to_x = vec![3_000, 3_100];
        state.formation.to_y = vec![3_000, 3_000];
        state.units[0].current_order_pause = Some(4);
        state
    };
    let seed = PathData {
        to_x: 3_000,
        to_y: 3_000,
        tolerance: 0x90,
        flags: 1,
    };
    let request = FindWPathRequest {
        stack: vec![seed],
        start_x: 0,
        start_y: 0,
        owner: 2,
        object: 10,
        army_path_mode: false,
        rng_epoch: 1234,
    };
    let mut host = PathHost::uniform();
    host.paths.push((
        request,
        FindWPathAnswer {
            return_value: 0,
            stack: vec![seed],
            rng_draws: vec![10],
            rng_epoch_after: 1235,
            unit_mutation: Some(PathFinderUnitMutation {
                actor: UnitKey::new(2, 10),
                counter_b2_before: 0,
                counter_b2_after: 30,
                pause: Some(PathFinderPauseMutation {
                    before: 4,
                    draw: 10,
                    after: 8,
                }),
            }),
        },
    ));

    assert_eq!(
        preflight_disembark_tail(&state, &host),
        Err(TailPlanError::InvalidPathFinderPause {
            actor: UnitKey::new(2, 10),
            before: 4,
            draw: 10,
            after: 8,
        })
    );
    assert_eq!(state.rng_epoch, 1234);
    assert_eq!(state.units[0].pathfinder_counter_b2, 0);
    assert_eq!(state.units[0].current_order_pause, Some(4));
}

#[test]
fn stale_state_and_stale_host_epoch_reject_without_partial_mutation() {
    let mut state = base_state();
    let host = PathHost::uniform();
    let receipt = preflight_disembark_tail(&state, &host).unwrap();

    state.units[0].version += 1;
    let changed = state.clone();
    assert_eq!(
        commit_disembark_tail(&mut state, &receipt, &host),
        Err(TailCommitError::StateChanged)
    );
    assert_eq!(state, changed);

    let mut state = receipt.before.clone();
    host.epoch.set(8);
    let before = state.clone();
    assert_eq!(
        commit_disembark_tail(&mut state, &receipt, &host),
        Err(TailCommitError::HostEpochChanged)
    );
    assert_eq!(state, before);
}

#[test]
fn malformed_identity_formation_and_scratch_shapes_are_refused() {
    let host = PathHost::uniform();
    let mut state = base_state();
    state.members[1] = state.members[0];
    assert_eq!(
        preflight_disembark_tail(&state, &host),
        Err(TailPlanError::DuplicateMember(10))
    );

    let mut state = base_state();
    state.formation.permutation = vec![0, 0];
    assert_eq!(
        preflight_disembark_tail(&state, &host),
        Err(TailPlanError::InvalidPermutation)
    );

    let mut state = base_state();
    state.form_scratch.pop();
    assert_eq!(
        preflight_disembark_tail(&state, &host),
        Err(TailPlanError::InvalidFormScratchLength(0xe5f))
    );
}
