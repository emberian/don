//! Opcode 48 Unit-receiver convergence through the production runtime and Fleet receipt.

use don_sim::command::direct_entity_command_integration::carrier_implicit_unqueue::{
    TrainingQueueCounters, TRAIN_AT_FACTORY,
};
use don_sim::command::direct_entity_command_integration::{
    DirectEntityDisposition, DirectEntityFleetReceipt, DirectEntityFleetRequest,
    DirectEntityOpenTail, DirectEntityTransactionStatus,
};
use don_sim::command::{Bridge, Fleet, InlineDef, InlinePort, Package};
use don_sim::objects::Band;
use don_sim::systems::order_dispatch::OrderQueue;
use don_sim::systems::production::runtime::{
    apply_sim_carrier_unqueue_fleet_transaction, process_sim_carrier_unqueue_command,
    LiveCarrierUnqueueObjectFacts, LiveCarrierUnqueueTypeFacts, LiveProductionRuntime,
    LiveProductionType,
};
use don_sim::systems::production::{flag, BuildData};
use don_sim::tick::Sim;

const OWNER: u8 = 2;
const CARRIER_TYPE: i32 = 351;
const PAYLOAD_TYPE: i32 = 310;

struct CarrierFleet {
    sim: Sim,
    runtime: LiveProductionRuntime,
    queue: OrderQueue,
}

impl Fleet for CarrierFleet {
    fn alive(&self, _who: u8, _o: i16) -> bool {
        false
    }

    fn is_unit(&self, _who: u8, _o: i16) -> bool {
        false
    }

    fn is_building(&self, _who: u8, _o: i16) -> bool {
        false
    }

    fn group_of(&self, _who: u8, _o: i16) -> i16 {
        -1
    }

    fn set_group_of(&mut self, _who: u8, _o: i16, _slot: i16) {}

    fn uid(&self, _who: u8, _o: i16) -> u16 {
        0xffff
    }

    fn pos(&self, _who: u8, _o: i16) -> (i32, i32) {
        (0, 0)
    }

    fn orders(&self, _who: u8, _o: i16) -> Option<&OrderQueue> {
        Some(&self.queue)
    }

    fn orders_mut(&mut self, _who: u8, _o: i16) -> Option<&mut OrderQueue> {
        Some(&mut self.queue)
    }

    fn set_stance(&mut self, _who: u8, _o: i16, _stance: i8) {}

    fn disband(&mut self, _who: u8, _o: i16) {}

    fn apply_direct_entity_command_transaction(
        &mut self,
        envelope: DirectEntityFleetRequest,
    ) -> DirectEntityFleetReceipt {
        apply_sim_carrier_unqueue_fleet_transaction(&mut self.sim, &mut self.runtime, envelope)
    }
}

fn unqueue_wire(object_index: i32, selector: i32, uid: i16) -> [u8; 15] {
    let mut wire = [0; 15];
    wire[0] = 48;
    wire[1..5].copy_from_slice(&i32::from(OWNER).to_le_bytes());
    wire[5..9].copy_from_slice(&object_index.to_le_bytes());
    wire[9..13].copy_from_slice(&selector.to_le_bytes());
    wire[13..15].copy_from_slice(&uid.to_le_bytes());
    wire
}

fn carrier_fleet(num_queued: i16, with_refund_costs: bool) -> CarrierFleet {
    let mut sim = Sim::new(0x48ca_221e, 8);
    let handle = sim
        .world
        .allocate_typed_at(OWNER, CARRIER_TYPE, 0x1200, 0x1800)
        .unwrap();
    let row = sim.world.row_of(handle).unwrap();
    sim.unit_type.push(CARRIER_TYPE);
    sim.world.units.num_queued_mut()[row] = num_queued;
    sim.world.units.queue_time_mut()[row] = 91;

    let mut runtime = std::mem::take(&mut sim.production_runtime);
    let mut payload = LiveProductionType::hosted_air_unit(PAYLOAD_TYPE, 10, 1);
    payload.carrier_unqueue = Some(LiveCarrierUnqueueTypeFacts {
        object: Some(LiveCarrierUnqueueObjectFacts {
            attack: 3,
            training_site: Some(TRAIN_AT_FACTORY),
            domain: None,
        }),
        refund_costs: with_refund_costs.then_some([1, 2, 3, 4, 5, 6]),
    });
    runtime.install_type(payload);
    let leader = &mut runtime.leaders[OWNER as usize];
    leader.helicopter_current_upgrade = Some(PAYLOAD_TYPE);
    leader.queued_counts[PAYLOAD_TYPE as usize] = 4;
    leader.resources = [100, 200, 300, 400, 500, 600];
    sim.leaders[OWNER as usize].econ.stockpile = leader.resources;
    sim.step8.leaders[OWNER as usize].econ.stockpile = leader.resources;
    leader.carrier_training_queued = TrainingQueueCounters {
        barracks: 7,
        stable: 8,
        factory: 9,
        combat: 10,
        dock: 11,
        air: 12,
    };
    runtime.carrier_resource_scratch = -77;

    CarrierFleet {
        sim,
        runtime,
        queue: OrderQueue::new(),
    }
}

fn carrier_address(fleet: &CarrierFleet) -> (i32, i16, usize) {
    let row = fleet
        .sim
        .world
        .objects
        .slot(OWNER as usize)
        .band(don_sim::objects::Band::Unit)[0] as usize;
    (
        i32::from(fleet.sim.world.units.o()[row]),
        fleet.sim.world.units.uid()[row],
        row,
    )
}

#[test]
fn bridge_opcode48_commits_the_complete_carrier_receiver_through_fleet() {
    let mut fleet = carrier_fleet(2, true);
    let (object_index, uid, row) = carrier_address(&fleet);
    let mut bridge = Bridge::new();
    bridge.frame = 144;

    bridge
        .process_all(
            &mut Package::new(0, 0),
            &unqueue_wire(object_index, i32::MIN, uid),
            &mut fleet,
        )
        .unwrap();

    let receipts = bridge.take_direct_entity_receipts();
    assert_eq!(receipts.len(), 1);
    assert!(receipts[0].valid);
    let DirectEntityFleetReceipt::Entity(receipt) = &receipts[0].observed else {
        panic!("opcode 48 returned a market receipt");
    };
    assert_eq!(receipt.status, DirectEntityTransactionStatus::Complete);
    assert!(matches!(
        receipt.disposition,
        Some(DirectEntityDisposition::CompleteUnitActionUnqueue { argument: 1, .. })
    ));
    assert!(receipt.unit_unqueue.is_some());

    assert_eq!(fleet.sim.world.units.num_queued()[row], 1);
    assert_eq!(fleet.sim.world.units.queue_time()[row], 91);
    let leader = &fleet.runtime.leaders[OWNER as usize];
    assert_eq!(leader.queued_counts[PAYLOAD_TYPE as usize], 3);
    assert_eq!(leader.carrier_training_queued.factory, 8);
    assert_eq!(leader.carrier_training_queued.combat, 10);
    assert_eq!(leader.resources, [101, 202, 303, 404, 505, 606]);
    assert_eq!(fleet.runtime.carrier_resource_scratch, 606);
    assert_eq!(InlineDef::find(48).unwrap().port, InlinePort::StateWired);
}

#[test]
fn missing_reached_refund_projection_is_atomic_and_unavailable() {
    let mut fleet = carrier_fleet(2, false);
    let (object_index, uid, row) = carrier_address(&fleet);
    let before_resources = fleet.runtime.leaders[OWNER as usize].resources;
    let before_training = fleet.runtime.leaders[OWNER as usize].carrier_training_queued;
    let before_aggregate =
        fleet.runtime.leaders[OWNER as usize].queued_counts[PAYLOAD_TYPE as usize];
    let request = don_sim::command::direct_entity_command_integration::plans::
        DirectEntityCommandRequest::Unqueue {
            who: i32::from(OWNER),
            object_index,
            type_index: 0,
            uid,
        };

    let receipt =
        process_sim_carrier_unqueue_command(&mut fleet.sim, &mut fleet.runtime, request, 9);

    assert_eq!(receipt.status, DirectEntityTransactionStatus::Unavailable);
    assert_eq!(fleet.sim.world.units.num_queued()[row], 2);
    assert_eq!(fleet.sim.world.units.queue_time()[row], 91);
    assert_eq!(
        fleet.runtime.leaders[OWNER as usize].resources,
        before_resources
    );
    assert_eq!(
        fleet.runtime.leaders[OWNER as usize].carrier_training_queued,
        before_training
    );
    assert_eq!(
        fleet.runtime.leaders[OWNER as usize].queued_counts[PAYLOAD_TYPE as usize],
        before_aggregate
    );
    assert_eq!(fleet.runtime.carrier_resource_scratch, -77);
}

#[test]
fn empty_carrier_queue_keeps_upgrade_and_refund_facts_lazy() {
    let mut fleet = carrier_fleet(0, false);
    let (object_index, uid, row) = carrier_address(&fleet);
    fleet.runtime.leaders[OWNER as usize].helicopter_current_upgrade = None;
    let request = don_sim::command::direct_entity_command_integration::plans::
        DirectEntityCommandRequest::Unqueue {
            who: i32::from(OWNER),
            object_index,
            type_index: i32::MAX,
            uid,
        };

    let receipt =
        process_sim_carrier_unqueue_command(&mut fleet.sim, &mut fleet.runtime, request, 10);

    assert_eq!(receipt.status, DirectEntityTransactionStatus::Complete);
    let receiver = receipt.unit_unqueue.unwrap();
    assert_eq!(receiver.facts.unwrap(), Default::default());
    assert_eq!(fleet.sim.world.units.num_queued()[row], 0);
    assert_eq!(fleet.sim.world.units.queue_time()[row], 91);
}

#[test]
fn canonical_no_costs_projection_skips_only_the_refund_loop() {
    let mut fleet = carrier_fleet(1, false);
    let (object_index, uid, row) = carrier_address(&fleet);
    fleet.runtime.leaders[OWNER as usize]
        .gain_context
        .suppress_resource_effects = true;
    let resources_before = fleet.sim.leaders[OWNER as usize].econ.stockpile;
    let request = don_sim::command::direct_entity_command_integration::plans::
        DirectEntityCommandRequest::Unqueue {
            who: i32::from(OWNER),
            object_index,
            type_index: -900,
            uid,
        };

    let receipt =
        process_sim_carrier_unqueue_command(&mut fleet.sim, &mut fleet.runtime, request, 11);

    assert_eq!(receipt.status, DirectEntityTransactionStatus::Complete);
    assert_eq!(fleet.sim.world.units.num_queued()[row], 0);
    assert_eq!(fleet.sim.world.units.queue_time()[row], 0);
    assert_eq!(
        fleet.runtime.leaders[OWNER as usize].queued_counts[PAYLOAD_TYPE as usize],
        3
    );
    assert_eq!(
        fleet.runtime.leaders[OWNER as usize]
            .carrier_training_queued
            .factory,
        8
    );
    assert_eq!(
        fleet.sim.leaders[OWNER as usize].econ.stockpile,
        resources_before
    );
    assert_eq!(
        fleet.runtime.leaders[OWNER as usize].resources,
        resources_before
    );
    assert_eq!(fleet.runtime.carrier_resource_scratch, -77);
}

#[test]
fn active_build_receiver_stays_an_explicit_open_tail() {
    let mut fleet = carrier_fleet(0, false);
    let build_o = fleet
        .sim
        .world
        .objects
        .slot(OWNER as usize)
        .mark(Band::Build);
    let build_row = fleet.sim.spawn_build(
        OWNER as usize,
        BuildData {
            flags: flag::VALID | flag::ACTIVE,
            who: OWNER,
            uid: 88,
            ..BuildData::default()
        },
    );
    fleet.runtime.register_build(build_row, 430);
    let build_uid = fleet.sim.builds[build_row].uid;
    let mut bridge = Bridge::new();
    bridge.frame = 12;

    bridge
        .process_all(
            &mut Package::new(0, 0),
            &unqueue_wire(build_o as i32, 777, build_uid as i16),
            &mut fleet,
        )
        .unwrap();

    let receipts = bridge.take_direct_entity_receipts();
    assert_eq!(receipts.len(), 1);
    assert!(receipts[0].valid);
    let DirectEntityFleetReceipt::Entity(receipt) = &receipts[0].observed else {
        panic!("opcode 48 returned a market receipt");
    };
    assert_eq!(receipt.status, DirectEntityTransactionStatus::OpenTail);
    assert!(matches!(
        receipt.disposition,
        Some(DirectEntityDisposition::OpenTail(
            DirectEntityOpenTail::ProductionBuildActionUnqueue { selector: 777, .. }
        ))
    ));
    assert!(receipt.unit_unqueue.is_none());
    assert_eq!(InlineDef::find(48).unwrap().port, InlinePort::StateWired);
}
