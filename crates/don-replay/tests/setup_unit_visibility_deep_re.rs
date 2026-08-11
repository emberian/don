#![allow(dead_code)]

mod leader_initial_prefix {
    pub use don_replay::leader_initial_prefix::CHECKSUM_LEADER_SLOTS;
}

mod leaders_dynamic_children_frontier {
    pub use don_replay::leaders_dynamic_children_frontier::*;
}

mod setup_place_unit_deep_re {
    pub use don_replay::setup_place_unit_deep_re::*;
}

mod unit_init_location_deep_re {
    pub use don_replay::unit_init_location_deep_re::*;
}

#[path = "../src/unit_init_collision_tail_deep_re.rs"]
mod unit_init_collision_tail_deep_re;

#[path = "../src/setup_unit_visibility_deep_re.rs"]
mod lane;

use don_replay::leaders_dynamic_children_frontier::DynamicLeadersAuthority;
use don_sim::systems::economy::GoodNode;
use don_sim::systems::items::{Items, WGrid, DOWN_ITEM, TYPE_GOODY, WFLAG_ITEM};
use don_sim::systems::map_terrain::{land, tflag, wflag, World};
use don_sim::systems::step12_visibility_producer_frontier::UnitLosFacts;
use don_sim::systems::world_oil_goods::{OilGoodRuntime, OilGoodSlot, SUBOBJECT_COORD_XOR};
use lane::{
    produce_setup_unit_visibility_test_seam, LeaderArrayField, LeaderTypeAvailReceipt,
    NewRareDecision, RareGoodClassAuthority, RevealLeaderFacts, SetupUnitVisibilityAuthority,
    SetupUnitVisibilityError, SetupVisibilityExternalResidual, ARRAY_BASE_ADD_VA,
    LEADER_TYPE_AVAIL_CALL_VA, LEADER_TYPE_AVAIL_VA, UNIT_AUTO_EXPLORE,
};
use unit_init_collision_tail_deep_re::{
    VisibilityUpdateRequest, OBJECT_UPDATE_SEEN_CALL_VA, OBJECT_UPDATE_SEEN_VA,
};

fn world() -> World {
    let mut world = World::init(4, 4, 44, 4, 4);
    world.wdata[0].land = land::FERTILE;
    world
}

fn request(unit_masks: u32) -> VisibilityUpdateRequest {
    VisibilityUpdateRequest {
        call_va: OBJECT_UPDATE_SEEN_CALL_VA,
        body_va: OBJECT_UPDATE_SEEN_VA,
        incremental: 0,
        owner: 0,
        o: 0,
        x: 0x180,
        y: 0x300,
        angle: 0,
        source_mylos: 1,
        type_domain: 0,
        type_unit_flags2: 0,
        unit_masks,
        object_flags: 1,
        infiltrated: 0,
    }
}

fn los() -> UnitLosFacts {
    UnitLosFacts {
        mylos: 1,
        ptolemy_count: 0,
        unit_role: 0,
        has_ptolemy_general: None,
        ptolemy_los_bonus: 0,
        the_ceo_count: 0,
        unit_is_siege: None,
        has_the_ceo_general: None,
        the_ceo_unit_los: 0,
    }
}

fn leaders(flags: i32) -> [RevealLeaderFacts; 8] {
    let mut leaders = [RevealLeaderFacts::default(); 8];
    leaders[0].leader_flags = flags;
    leaders
}

fn empty_authority(flags: i32) -> SetupUnitVisibilityAuthority {
    SetupUnitVisibilityAuthority {
        los: los(),
        leaders: leaders(flags),
        object_links: Vec::new(),
        rare_goods: Vec::new(),
        type_avail_calls: Vec::new(),
    }
}

fn one_good(type_index: i32) -> OilGoodRuntime {
    OilGoodRuntime {
        slots: vec![OilGoodSlot {
            node: GoodNode {
                flags: 1,
                who: u8::MAX,
                o: 0,
                z: SUBOBJECT_COORD_XOR,
                x: 0x180 ^ SUBOBJECT_COORD_XOR,
                y: 0x180 ^ SUBOBJECT_COORD_XOR,
                type_index,
                ever_seen: 0,
            },
            ptype_present: true,
            cur_time: 0,
        }],
        capacity: 4,
        increment: -1,
        array_flags: 0,
        cur_index: 0,
        good_mark: 1,
    }
}

#[test]
fn resource_oil_and_auto_explore_branches_keep_native_order_and_sparse_ids() {
    let mut world = world();
    let mut worldc = world.clone();
    // project(0x180, 0x300, 0, 0x180) = (0x180, 0x180), fog (1,1), W cell (0,0).
    let ti = world.t_index(3, 3);
    world.tdata[ti] |= tflag::RESOURCE;
    worldc.tdata[ti] |= tflag::RESOURCE;
    world.wdata[0].flags |= wflag::OIL | WFLAG_ITEM;
    worldc.wdata[0].down = lane::DOWN_GOOD;
    worldc.wdata[0].down_who = 0;

    let mut goods = one_good(7);
    let mut items = Items::new();
    let mut dynamic = DynamicLeadersAuthority::default();
    let mut authority = empty_authority(0x0b);
    authority.rare_goods.push(RareGoodClassAuthority {
        good_slot: 0,
        type_index: 7,
        is_type_6: false,
        is_type_31: false,
    });
    authority.type_avail_calls.push(LeaderTypeAvailReceipt {
        call_va: LEADER_TYPE_AVAIL_CALL_VA,
        body_va: LEADER_TYPE_AVAIL_VA,
        leader: 0,
        type_index: 7,
        strict: 1,
        available: true,
    });

    let receipt = produce_setup_unit_visibility_test_seam(
        &mut world,
        &worldc,
        &mut goods,
        &mut items,
        &mut dynamic,
        request(UNIT_AUTO_EXPLORE),
        0x0b,
        0x1234,
        authority,
    )
    .unwrap();

    assert_eq!(receipt.newly_explored, vec![(1, 1)]);
    assert!(receipt.projected_small_los);
    assert!(receipt.update_local_seen_skipped);
    assert_eq!(receipt.rng_before, receipt.rng_after);
    assert_eq!(goods.slots[0].node.ever_seen, 1);
    assert_eq!(dynamic.rows[0].new_rares.elements, vec![0]);
    assert_eq!(dynamic.rows[0].new_rares.capacity, 4);
    assert_eq!(dynamic.rows[0].oil_patches.elements, vec![0]);
    assert_eq!(receipt.leader_array_adds.len(), 2);
    assert!(receipt
        .leader_array_adds
        .iter()
        .all(|add| add.body_va == ARRAY_BASE_ADD_VA));
    assert_eq!(
        receipt.reveals[0].new_rare_decision(0),
        Some(NewRareDecision::Added)
    );
    assert_eq!(
        receipt.reveals[0].oil.add.unwrap().field,
        LeaderArrayField::OilPatches
    );
    match receipt.next_external_residual {
        SetupVisibilityExternalResidual::GroupsAutoExplore { requests, .. } => {
            assert_eq!(requests.len(), 1);
            assert_eq!(requests[0].target_fine_x, 0x180);
            assert_eq!(requests[0].target_fine_y, 0x180);
            assert_eq!(requests[0].order_index, 3);
        }
        other => panic!("unexpected residual: {other:?}"),
    }
}

#[test]
fn item_ever_seen_is_committed_with_the_fog_plane() {
    let mut world = world();
    let mut worldc = world.clone();
    world.wdata[0].flags |= WFLAG_ITEM;
    let mut item_grid = WGrid::new(4, 4);
    let mut items = Items::new();
    let slot = items.init_item(&mut item_grid, TYPE_GOODY, 0x180, 0x180, 0);
    assert_eq!(slot, 0);
    worldc.wdata[0].down = DOWN_ITEM;
    worldc.wdata[0].down_who = 0;

    let mut goods = OilGoodRuntime::default();
    let mut dynamic = DynamicLeadersAuthority::default();
    let receipt = produce_setup_unit_visibility_test_seam(
        &mut world,
        &worldc,
        &mut goods,
        &mut items,
        &mut dynamic,
        request(0),
        0x0b,
        77,
        empty_authority(0x0b),
    )
    .unwrap();

    assert_eq!(items.get(0).unwrap().ever_seen, 1);
    assert_eq!(receipt.item_ever_seen_mutations, 1);
    assert_eq!(receipt.reveals[0].item.item_slot, Some(0));
    assert_eq!(
        receipt.next_external_residual,
        SetupVisibilityExternalResidual::None
    );
}

#[test]
fn missing_heterogeneous_chain_link_rolls_back_every_owner() {
    let mut world = world();
    let mut worldc = world.clone();
    world.wdata[0].flags |= WFLAG_ITEM;
    worldc.wdata[0].down = 4;
    worldc.wdata[0].down_who = 2;
    let worldc_before = worldc.clone();
    let before_checksum = world.checksum();
    let before_seen = world.seen.clone();
    let before_seen2 = world.seen2.clone();
    let before_wcoord_seen = world.wcoord_seen.clone();
    let mut goods = OilGoodRuntime::default();
    let before_goods = goods.clone();
    let mut items = Items::new();
    let before_items = items.clone();
    let mut dynamic = DynamicLeadersAuthority::default();
    let before_dynamic = dynamic.clone();

    let err = produce_setup_unit_visibility_test_seam(
        &mut world,
        &worldc,
        &mut goods,
        &mut items,
        &mut dynamic,
        request(0),
        0x0b,
        91,
        empty_authority(0x0b),
    )
    .unwrap_err();
    assert_eq!(
        err,
        SetupUnitVisibilityError::MissingObjectLink { who: 2, o: 4 }
    );
    assert_eq!(world.checksum(), before_checksum);
    assert_eq!(world.seen, before_seen);
    assert_eq!(world.seen2, before_seen2);
    assert_eq!(world.wcoord_seen, before_wcoord_seen);
    assert_eq!(worldc.checksum(), worldc_before.checksum());
    assert_eq!(goods, before_goods);
    assert_eq!(items, before_items);
    assert_eq!(dynamic, before_dynamic);
}

trait RevealReceiptExt {
    fn new_rare_decision(&self, leader: u8) -> Option<NewRareDecision>;
}

impl RevealReceiptExt for lane::RevealFogReceipt {
    fn new_rare_decision(&self, leader: u8) -> Option<NewRareDecision> {
        self.rare
            .new_rare
            .as_ref()?
            .candidates
            .iter()
            .find(|candidate| candidate.leader == leader)
            .map(|candidate| candidate.decision)
    }
}
