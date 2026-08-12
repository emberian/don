// SPDX-License-Identifier: GPL-3.0-or-later

//! Dispatcher integration gates for EXPLORE_TO, ATTACK_GROUND, and AIR_ATTACK_GROUND.
//!
//! These tests exercise only the typed host-receipt boundary and deterministic local tails;
//! the instruction transcription itself remains covered in `targeted_order_plans`.

use don_sim::order::{Order, OrderIndex, ORDER_FACING_TARGET};
use don_sim::systems::movement::{PathFinder, UnitWorld};
use don_sim::systems::order_dispatch::{
    do_job, AirAttackGroundHostReceipt, AirAttackGroundPhysicsReceipt, AirPatrolSearch, ArmResult,
    AttackGroundHostReceipt, AttackOutcome, DispatchCoverage, ExploreToHostReceipt,
    ExploreToPostMoveReceipt, GatherOutcome, OrderRec, TargetState, TargetedOrderHostError,
    TargetedOrderPayload, UnitWork, WorkWorld,
};
use don_sim::systems::patrol::{AirPatrolOrder, AirPatrolTarget, GroupMoveRequest};
use don_sim::systems::targeted_order_plans::{
    AirAttackGroundFacts, AirAttackGroundOrderState, AttackGroundFacts, AttackGroundOrderState,
    HostFact, OrderEffect, OBJECT_ATTACK_GROUND_ACTIVE,
};

fn known<T>(value: T) -> HostFact<T> {
    HostFact::known(value)
}

#[derive(Default)]
struct ReceiptWorld {
    frame: i32,
    explore: Option<ExploreToHostReceipt>,
    explore_post: Option<ExploreToPostMoveReceipt>,
    ground: Option<AttackGroundHostReceipt>,
    air: Option<AirAttackGroundHostReceipt>,
    air_physics: Option<AirAttackGroundPhysicsReceipt>,
    effects: Vec<OrderEffect>,
}

impl UnitWorld for ReceiptWorld {
    fn tiles_w(&self) -> i32 {
        16
    }

    fn tiles_h(&self) -> i32 {
        16
    }

    fn wcells_w(&self) -> i32 {
        4
    }

    fn invalid_loc(&self, _: i32, _: i32) -> bool {
        false
    }

    fn unit_collides(&self, _: i32, _: i32) -> bool {
        false
    }

    fn needs_transport(&self, _: i32, _: i32, _: i32, _: i32) -> i32 {
        0
    }

    fn tregion(&self, _: i32, _: i32) -> i32 {
        0
    }
}

impl WorkWorld for ReceiptWorld {
    fn frame(&self) -> i32 {
        self.frame
    }

    fn target(&self, _: i32, _: i32) -> Option<TargetState> {
        None
    }

    fn attack(&mut self, _: &UnitWork, _: &OrderRec) -> AttackOutcome {
        AttackOutcome::Impossible
    }

    fn gather(&mut self, _: &UnitWork, _: &OrderRec) -> GatherOutcome {
        GatherOutcome::Exhausted
    }

    fn draw_path_retry_delay(&mut self) -> i32 {
        6
    }

    fn patrol_think_bird(&mut self, _: &mut UnitWork, _: &mut AirPatrolOrder) {
        unreachable!("targeted-order tests do not dispatch patrol")
    }

    fn air_patrol_physics(
        &mut self,
        _: &mut UnitWork,
        _: &mut AirPatrolOrder,
        _: i32,
        _: i32,
    ) -> bool {
        unreachable!("targeted-order tests do not dispatch patrol")
    }

    fn air_patrol_unit_target(
        &mut self,
        _: &UnitWork,
        _: &AirPatrolOrder,
        _: i32,
        _: i32,
        _: AirPatrolSearch,
    ) -> Option<AirPatrolTarget> {
        unreachable!("targeted-order tests do not dispatch patrol")
    }

    fn air_patrol_building_target(
        &mut self,
        _: &UnitWork,
        _: &AirPatrolOrder,
        _: i32,
        _: i32,
    ) -> Option<AirPatrolTarget> {
        unreachable!("targeted-order tests do not dispatch patrol")
    }

    fn group_patrol_move(&mut self, _: &mut UnitWork, _: GroupMoveRequest) {
        unreachable!("targeted-order tests do not dispatch patrol")
    }

    fn patrol_actor_is_type(&self, _: &UnitWork, _: i32, _: bool) -> bool {
        unreachable!("targeted-order tests do not dispatch patrol")
    }

    fn patrol_inside_is_scramblable(&self, _: u8, _: i16) -> bool {
        unreachable!("targeted-order tests do not dispatch patrol")
    }

    fn patrol_scramble_inside(&mut self, _: &mut UnitWork, _: i16) {
        unreachable!("targeted-order tests do not dispatch patrol")
    }

    fn explore_to_preflight(
        &mut self,
        _: &UnitWork,
        _: &OrderRec,
    ) -> Result<ExploreToHostReceipt, TargetedOrderHostError> {
        self.explore
            .clone()
            .ok_or(TargetedOrderHostError::Unavailable)
    }

    fn explore_to_post_move(
        &mut self,
        _: &UnitWork,
        _: &OrderRec,
        _: &ExploreToHostReceipt,
    ) -> ExploreToPostMoveReceipt {
        self.explore_post
            .expect("successful explore preflight installs its post-move receipt")
    }

    fn attack_ground_preflight(
        &mut self,
        _: &UnitWork,
        _: &OrderRec,
    ) -> Result<AttackGroundHostReceipt, TargetedOrderHostError> {
        self.ground
            .clone()
            .ok_or(TargetedOrderHostError::Unavailable)
    }

    fn air_attack_ground_preflight(
        &mut self,
        _: &UnitWork,
        _: &OrderRec,
    ) -> Result<AirAttackGroundHostReceipt, TargetedOrderHostError> {
        self.air.ok_or(TargetedOrderHostError::Unavailable)
    }

    fn air_attack_ground_physics(
        &mut self,
        _: &mut UnitWork,
        order: &mut AirAttackGroundOrderState,
        _: &AirAttackGroundHostReceipt,
    ) -> AirAttackGroundPhysicsReceipt {
        let receipt = self
            .air_physics
            .clone()
            .expect("successful air preflight installs its physics receipt");
        *order = receipt.order;
        receipt
    }

    fn targeted_order_effect(&mut self, actor: &mut UnitWork, _: &OrderRec, effect: OrderEffect) {
        if let OrderEffect::SetAngle(angle) = &effect {
            actor.body.angle = *angle as i32;
            actor.lead_guy.angle = *angle as i32;
        }
        self.effects.push(effect);
    }
}

fn dispatch(actor: &mut UnitWork, world: &mut ReceiptWorld, kind: OrderIndex) -> ArmResult {
    let mut pathfinder = PathFinder::new();
    let mut coverage = DispatchCoverage::default();
    let result = do_job(actor, world, &mut pathfinder, &mut coverage, kind);
    assert_eq!(coverage.dispatches[kind.index()], 1);
    result
}

fn ground_facts(actor: &UnitWork, state: AttackGroundOrderState) -> AttackGroundFacts {
    AttackGroundFacts {
        actor_owner: i32::from(actor.who),
        actor_object: i32::from(actor.o),
        actor_angle: actor.body.angle as u32,
        target_x: state.att_x,
        target_y: state.att_y,
        raw_target_angle: 0x2000_0000,
        side_firing: false,
        can_attack_ground: known(true),
        attack_unit: state.attack_unit,
        terrain_owner: known(-1),
        at_peace_with_terrain_owner: HostFact::missing("unowned terrain skips peace"),
        target_is_in_range: known(true),
        recharge: actor.recharging,
        order_facing_target: HostFact::missing("not recharging"),
        current_animation: HostFact::missing("not recharging"),
        redirects_to_special_cast: false,
        directional_attack_animation: false,
        has_ammo: true,
        recharge_delay: 4,
        vector_distance: 2_000,
        range_bias: 0,
        near_range: 4,
        far_range: 8,
        nearby_spot: HostFact::missing("target already in range"),
        is_local_player: HostFact::missing("reposition did not fail"),
    }
}

#[test]
fn explore_requires_a_token_before_do_move_and_uses_post_move_receipts() {
    let mut actor = UnitWork::at(2, 0, 100, 200);
    actor.uid = 9;
    let mut order = OrderRec::move_to(500, 600, 0);
    order.kind = OrderIndex::ExploreTo;
    order.retry = 2;
    actor.orders.push_back(order.clone());

    let before = actor.orders.clone();
    let mut unavailable = ReceiptWorld::default();
    assert_eq!(
        dispatch(&mut actor, &mut unavailable, OrderIndex::ExploreTo),
        ArmResult::HostUnavailable
    );
    assert_eq!(
        actor.orders, before,
        "missing token must prevent movement mutation"
    );

    let mut world = ReceiptWorld {
        explore: Some(ExploreToHostReceipt {
            actor_who: actor.who,
            actor_o: actor.o,
            actor_uid: actor.uid,
            frame: 0,
            order: order.clone(),
        }),
        explore_post: Some(ExploreToPostMoveReceipt {
            current_order_is_same: true,
            actor_is_on_map: true,
        }),
        ..ReceiptWorld::default()
    };
    assert_eq!(
        dispatch(&mut actor, &mut world, OrderIndex::ExploreTo),
        ArmResult::Working
    );
    assert_eq!(actor.orders.front().unwrap().retry, 1);
    assert_eq!(world.effects, vec![OrderEffect::Explore]);
}

#[test]
fn attack_ground_missing_fact_is_zero_mutation_and_fire_tail_is_ordered() {
    let state = AttackGroundOrderState {
        att_x: 480,
        att_y: 960,
        accuracy: 7,
        attack_unit: 0,
    };
    let mut actor = UnitWork::at(2, 19, 100, 200);
    actor.uid = 11;
    actor.body.angle = 0x1000_0000;
    actor.lead_guy.angle = actor.body.angle;
    actor.orders.push_back(OrderRec::attack_ground(state));

    let mut missing_facts = ground_facts(&actor, state);
    missing_facts.terrain_owner = HostFact::missing("terrain owner");
    let before_orders = actor.orders.clone();
    let before_masks = actor.unit_masks;
    let mut unavailable = ReceiptWorld {
        ground: Some(AttackGroundHostReceipt {
            actor_who: actor.who,
            actor_o: actor.o,
            actor_uid: actor.uid,
            order: state,
            order_flags: actor.orders.front().unwrap().flags,
            facts: missing_facts,
        }),
        ..ReceiptWorld::default()
    };
    assert_eq!(
        dispatch(&mut actor, &mut unavailable, OrderIndex::AttackGround),
        ArmResult::HostUnavailable
    );
    assert_eq!(actor.orders, before_orders);
    assert_eq!(actor.unit_masks, before_masks);
    assert!(unavailable.effects.is_empty());

    let mut world = ReceiptWorld {
        ground: Some(AttackGroundHostReceipt {
            actor_who: actor.who,
            actor_o: actor.o,
            actor_uid: actor.uid,
            order: state,
            order_flags: actor.orders.front().unwrap().flags,
            facts: ground_facts(&actor, state),
        }),
        ..ReceiptWorld::default()
    };
    assert_eq!(
        dispatch(&mut actor, &mut world, OrderIndex::AttackGround),
        ArmResult::Working
    );
    assert_eq!(
        actor.unit_masks & OBJECT_ATTACK_GROUND_ACTIVE,
        OBJECT_ATTACK_GROUND_ACTIVE
    );
    assert_ne!(actor.orders.front().unwrap().flags & ORDER_FACING_TARGET, 0);
    assert_eq!(actor.recharging, 5);
    assert_eq!(
        world.effects,
        vec![
            OrderEffect::SetAttack {
                object: -1,
                owner: -1,
            },
            OrderEffect::SetAngle(0x2000_0000),
            OrderEffect::SetAnimation {
                anim: 11,
                b: 0,
                c: 1,
            },
            OrderEffect::FireAmmo {
                object: -1,
                owner: -1,
            },
        ]
    );
}

#[test]
fn air_attack_ground_preflights_physics_and_commits_recharge_and_mana_tail() {
    let state = AirAttackGroundOrderState {
        attack: AttackGroundOrderState {
            att_x: 700,
            att_y: 900,
            accuracy: 0,
            attack_unit: 0,
        },
        ..AirAttackGroundOrderState::default()
    };
    let mut actor = UnitWork::at(3, 8, 100, 200);
    actor.uid = 17;
    actor.orders.push_back(OrderRec::air_attack_ground(state));

    let before = actor.orders.clone();
    let mut missing = ReceiptWorld::default();
    assert_eq!(
        dispatch(&mut actor, &mut missing, OrderIndex::AirAttackGround),
        ArmResult::HostUnavailable
    );
    assert_eq!(
        actor.orders, before,
        "physics must not run before its token exists"
    );

    let facts = AirAttackGroundFacts {
        physics_complete: known(true),
        recharge: 0,
        returning: 0,
        actor_angle: 0,
        target_angle: 0,
        target_is_in_range: known(true),
        is_missile: false,
        is_jet_fighter: HostFact::missing("angle already admitted"),
        strafes: false,
        is_bomber: known(true),
        has_ammo: true,
        recharge_delay: 8,
        bombing_mana_cost: 13,
    };
    let mut world = ReceiptWorld {
        air: Some(AirAttackGroundHostReceipt {
            actor_who: actor.who,
            actor_o: actor.o,
            actor_uid: actor.uid,
            order: state,
            order_flags: actor.orders.front().unwrap().flags,
        }),
        air_physics: Some(AirAttackGroundPhysicsReceipt {
            order: state,
            facts,
        }),
        ..ReceiptWorld::default()
    };
    assert_eq!(
        dispatch(&mut actor, &mut world, OrderIndex::AirAttackGround),
        ArmResult::Working
    );
    assert_eq!(actor.recharging, 9);
    assert_eq!(actor.mana_burn, 13);
    assert_eq!(
        world.effects,
        vec![
            OrderEffect::SetAttack {
                object: -1,
                owner: -1,
            },
            OrderEffect::SetAnimation {
                anim: 12,
                b: 0,
                c: 1,
            },
        ]
    );
}

#[test]
fn typed_ground_and_legacy_air_orders_widen_without_inventing_ground_suffixes() {
    let ground_state = AttackGroundOrderState {
        att_x: 123,
        att_y: 456,
        accuracy: 0,
        attack_unit: 0,
    };
    let ground = OrderRec::from(Order::attack_ground(ground_state));
    assert_eq!(
        ground.targeted_payload,
        TargetedOrderPayload::AttackGround(ground_state)
    );

    let legacy_ground = OrderRec::from(Order {
        kind: OrderIndex::AttackGround,
        x: 123,
        y: 456,
        ..Order::default()
    });
    assert_eq!(legacy_ground.targeted_payload, TargetedOrderPayload::None);

    let air = OrderRec::from(Order {
        kind: OrderIndex::AirAttackGround,
        x: 700,
        y: 900,
        ..Order::default()
    });
    let TargetedOrderPayload::AirAttackGround(state) = air.targeted_payload else {
        panic!("AIR_ATTACK_GROUND must widen to its concrete walked payload")
    };
    assert_eq!((state.attack.att_x, state.attack.att_y), (700, 900));
    assert_eq!(
        state,
        AirAttackGroundOrderState {
            attack: state.attack,
            ..AirAttackGroundOrderState::default()
        }
    );
}
