#[path = "../src/systems/bhs_create_unit_frontier.rs"]
mod bhs_create_unit_frontier;

use std::collections::BTreeMap;

use bhs_create_unit_frontier::*;

#[derive(Default)]
struct Facts {
    names: BTreeMap<String, i32>,
    canonical: BTreeMap<i32, String>,
    leader_flags: [u32; 8],
    upgrades: BTreeMap<(usize, i32), i32>,
    grafts: BTreeMap<(usize, i32), i32>,
    is_unit: BTreeMap<i32, bool>,
    transport_relation: BTreeMap<i32, bool>,
    domains: BTreeMap<i32, i32>,
    unit_flags: BTreeMap<i32, u32>,
    world_valid: Option<bool>,
    ocean: Option<bool>,
    can_transport: [Option<bool>; 8],
}

impl Facts {
    fn insert_type(&mut self, id: i32, name: &str, domain: i32) {
        self.names.insert(name.to_ascii_lowercase(), id);
        self.canonical.insert(id, name.to_owned());
        self.is_unit.insert(id, true);
        self.transport_relation.insert(id, false);
        self.domains.insert(id, domain);
        self.unit_flags.insert(id, 0);
    }
}

impl CreateUnitFacts for Facts {
    fn resolve_type(&self, name: &str) -> Option<i32> {
        self.names.get(&name.to_ascii_lowercase()).copied()
    }

    fn canonical_type_name(&self, type_id: i32) -> Option<&str> {
        self.canonical.get(&type_id).map(String::as_str)
    }

    fn leader_flags(&self, leader_slot: usize) -> Option<u32> {
        self.leader_flags.get(leader_slot).copied()
    }

    fn current_upgrade(&self, leader_slot: usize, type_id: i32) -> Option<i32> {
        self.upgrades.get(&(leader_slot, type_id)).copied()
    }

    fn leader_graft(&self, leader_slot: usize, type_id: i32) -> Option<i32> {
        self.grafts.get(&(leader_slot, type_id)).copied()
    }

    fn is_unit_type(&self, type_id: i32) -> Option<bool> {
        self.is_unit.get(&type_id).copied()
    }

    fn is_transport_barge_relation(&self, type_id: i32) -> Option<bool> {
        self.transport_relation.get(&type_id).copied()
    }

    fn domain(&self, type_id: i32) -> Option<i32> {
        self.domains.get(&type_id).copied()
    }

    fn unit_flags(&self, type_id: i32) -> Option<u32> {
        self.unit_flags.get(&type_id).copied()
    }

    fn world_valid(&self, _world_x: i32, _world_y: i32) -> Option<bool> {
        self.world_valid
    }

    fn world_is_ocean(&self, _world_x: i32, _world_y: i32) -> Option<bool> {
        self.ocean
    }

    fn can_transport(&self, leader_slot: usize) -> Option<bool> {
        self.can_transport.get(leader_slot).copied().flatten()
    }
}

fn fixture() -> Facts {
    let mut facts = Facts {
        world_valid: Some(true),
        ocean: Some(false),
        ..Default::default()
    };
    facts.insert_type(50, "Infantry", DOMAIN_GROUND);
    facts.insert_type(51, "Modern Infantry", DOMAIN_GROUND);
    facts.insert_type(52, "Fighter", DOMAIN_AIR);
    facts.insert_type(53, "Cruiser", DOMAIN_SEA);
    facts.leader_flags[0] = 3;
    facts.upgrades.insert((0, 50), 51);
    for ty in [50, 51, 52, 53] {
        facts.grafts.insert((0, ty), ty + 100);
    }
    facts.can_transport[0] = Some(true);
    facts
}

fn request<'a>(name: &'a str) -> CreateUnitRequest<'a> {
    CreateUnitRequest {
        who: 1,
        x: 20,
        y: 12,
        requested_type_name: name,
        count: 3,
    }
}

#[test]
fn registrations_handlers_and_census_are_frozen() {
    let expected = [
        (508, 0x009f_4c50, 37, 548, 91),
        (509, 0x00a0_30e0, 116, 3_046, 182),
        (510, 0x009f_4c80, 165, 1_678, 109),
    ];
    for (row, expected) in CREATE_UNIT_CENSUS.iter().zip(expected) {
        assert_eq!(row.builtin.registration(), expected.0);
        assert_eq!(row.builtin.retail_va(), expected.1);
        assert_eq!(row.builtin.retail_bytes(), expected.2);
        assert_eq!(row.calls, expected.3);
        assert_eq!(row.files, expected.4);
    }
    assert_eq!(ADD_UNIT_VA, 0x009e_2220);
    assert_eq!(ADD_UNIT_BYTES, 1_428);
    assert_eq!(
        CREATE_UNIT_CENSUS.iter().map(|row| row.calls).sum::<u32>(),
        CREATE_UNIT_COHORT_CALLS
    );
    assert_eq!(CREATE_UNIT_COHORT_CALLS, 5_272);
    assert_eq!(CREATE_UNIT_COHORT_FILES, 232);
    assert!((potential_corpus_percentage_points() - 13.194_184).abs() < 0.000_001);
}

#[test]
fn direct_keeps_requested_name_and_freezes_distinct_group_keys() {
    let facts = fixture();
    let plan = plan_create_unit_prefix(&facts, CreateUnitBuiltin::CreateUnit, &request("iNfAnTrY"))
        .unwrap();
    assert_eq!(plan.leader_slot, 0);
    assert_eq!(plan.effective_type_name, "iNfAnTrY");
    assert_eq!(plan.effective_type, 50);
    assert_eq!(plan.graft_type, 150);
    assert_eq!(plan.clear_group_key, Some(0));
    assert_eq!(plan.group_append_key, 1);
    assert_eq!(plan.route, CreateUnitRoute::DirectGround);
}

#[test]
fn upgrade_reset_and_append_substitute_the_canonical_current_upgrade_name() {
    let facts = fixture();
    let reset = plan_create_unit_prefix(
        &facts,
        CreateUnitBuiltin::CreateUnitUpgrade,
        &request("INFANTRY"),
    )
    .unwrap();
    let append = plan_create_unit_prefix(
        &facts,
        CreateUnitBuiltin::CreateUnitInGroup,
        &request("infantry"),
    )
    .unwrap();
    assert_eq!(reset.effective_type_name, "Modern Infantry");
    assert_eq!(reset.effective_type, 51);
    assert_eq!(reset.clear_group_key, Some(0));
    assert_eq!(reset.group_append_key, 1);
    assert_eq!(append.effective_type_name, "Modern Infantry");
    assert_eq!(append.clear_group_key, None);
    assert_eq!(append.group_append_key, 1);
}

#[test]
fn wrapper_and_core_leader_gates_are_distinct_and_ordered() {
    let mut facts = fixture();
    let mut bad_player = request("missing");
    bad_player.who = 9;
    assert_eq!(
        plan_create_unit_prefix(&facts, CreateUnitBuiltin::CreateUnitUpgrade, &bad_player),
        Err(CreateUnitPrefixError::RequestedTypeMissing)
    );
    assert_eq!(
        plan_create_unit_prefix(&facts, CreateUnitBuiltin::CreateUnit, &bad_player),
        Err(CreateUnitPrefixError::PlayerOutOfRange)
    );

    facts.leader_flags[0] = 1;
    assert_eq!(
        plan_create_unit_prefix(
            &facts,
            CreateUnitBuiltin::CreateUnitUpgrade,
            &request("Infantry")
        ),
        Err(CreateUnitPrefixError::CoreLeaderNotActive)
    );
    facts.leader_flags[0] = 2;
    assert_eq!(
        plan_create_unit_prefix(
            &facts,
            CreateUnitBuiltin::CreateUnitUpgrade,
            &request("Infantry")
        ),
        Err(CreateUnitPrefixError::WrapperLeaderNotInGame)
    );
}

#[test]
fn count_gate_is_unsigned_and_includes_zero_and_two_thousand() {
    let facts = fixture();
    for count in [0, 2_000] {
        let mut req = request("Infantry");
        req.count = count;
        assert_eq!(
            plan_create_unit_prefix(&facts, CreateUnitBuiltin::CreateUnit, &req)
                .unwrap()
                .count,
            count as u32
        );
    }
    for count in [-1, 2_001] {
        let mut req = request("Infantry");
        req.count = count;
        assert_eq!(
            plan_create_unit_prefix(&facts, CreateUnitBuiltin::CreateUnit, &req),
            Err(CreateUnitPrefixError::CountOutOfRange)
        );
    }
}

#[test]
fn coordinate_derivation_keeps_x86_shift_and_wrapping_rules() {
    assert_eq!(
        CreateUnitCoords::from_script(20, 12),
        CreateUnitCoords {
            world_x: 5,
            world_y: 3,
            coord_x: 3_936,
            coord_y: 2_400,
        }
    );
    assert_eq!(CreateUnitCoords::from_script(-1, -5).world_x, -1);
    assert_eq!(CreateUnitCoords::from_script(-1, -5).world_y, -2);
    assert_eq!(
        CreateUnitCoords::from_script(i32::MAX, 0).coord_x,
        i32::MAX.wrapping_mul(192).wrapping_add(96)
    );
}

#[test]
fn domain_ocean_matrix_and_air_strafe_flag_are_exact() {
    let mut facts = fixture();
    assert_eq!(
        plan_create_unit_prefix(&facts, CreateUnitBuiltin::CreateUnit, &request("Infantry"))
            .unwrap()
            .route,
        CreateUnitRoute::DirectGround
    );

    facts.ocean = Some(true);
    assert_eq!(
        plan_create_unit_prefix(&facts, CreateUnitBuiltin::CreateUnit, &request("Infantry"))
            .unwrap()
            .route,
        CreateUnitRoute::GroundViaTransport
    );
    assert_eq!(
        plan_create_unit_prefix(&facts, CreateUnitBuiltin::CreateUnit, &request("Cruiser"))
            .unwrap()
            .route,
        CreateUnitRoute::DirectSea
    );

    facts.ocean = Some(false);
    assert_eq!(
        plan_create_unit_prefix(&facts, CreateUnitBuiltin::CreateUnit, &request("Cruiser"))
            .unwrap()
            .route,
        CreateUnitRoute::RejectAfterGroupPolicy
    );
    assert_eq!(
        plan_create_unit_prefix(&facts, CreateUnitBuiltin::CreateUnit, &request("Fighter"))
            .unwrap()
            .route,
        CreateUnitRoute::DirectAir {
            install_strafe_order: true
        }
    );
    facts.unit_flags.insert(52, AIR_NO_STRAFE_FLAG);
    assert_eq!(
        plan_create_unit_prefix(&facts, CreateUnitBuiltin::CreateUnit, &request("Fighter"))
            .unwrap()
            .route,
        CreateUnitRoute::DirectAir {
            install_strafe_order: false
        }
    );
}

#[test]
fn transport_relation_rejection_is_after_reset_policy_selection() {
    let mut facts = fixture();
    facts.transport_relation.insert(50, true);
    facts.transport_relation.insert(51, true);
    let reset =
        plan_create_unit_prefix(&facts, CreateUnitBuiltin::CreateUnit, &request("Infantry"))
            .unwrap();
    let append = plan_create_unit_prefix(
        &facts,
        CreateUnitBuiltin::CreateUnitInGroup,
        &request("Infantry"),
    )
    .unwrap();
    assert_eq!(reset.route, CreateUnitRoute::RejectAfterGroupPolicy);
    assert_eq!(reset.clear_group_key, Some(0));
    assert_eq!(append.route, CreateUnitRoute::RejectAfterGroupPolicy);
    assert_eq!(append.clear_group_key, None);
}

#[test]
fn return_is_last_allocation_result_without_rollback() {
    assert_eq!(retail_last_init_result([]), -1);
    assert_eq!(retail_last_init_result([41]), 41);
    assert_eq!(retail_last_init_result([41, 42]), 42);
    assert_eq!(retail_last_init_result([41, -1]), -1);
    assert_eq!(retail_last_init_result([-1, 42]), 42);
}
