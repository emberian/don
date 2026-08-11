//! Real opcode-0 selection plus opcode-24 packet through the canonical Sim queue host.

use don_sim::command::queue_up_action::{QueueUpActionReceipt, QueueUpActionRequest};
use don_sim::command::{Bridge, Fleet, Package};
use don_sim::objects::{Band, BUILD_BAND_BASE};
use don_sim::systems::order_dispatch::OrderQueue;
use don_sim::systems::production::runtime::{
    apply_sim_queue_up_fleet_transaction, LiveCarrierUnqueueObjectFacts,
    LiveCarrierUnqueueTypeFacts, LiveProductionRuntime, LiveProductionType,
};
use don_sim::systems::production::{flag, BuildData, BuildQueue, BuildQueueEntry};
use don_sim::tick::Sim;

const OWNER: u8 = 2;
const PRODUCER_TYPE: i32 = 430;
const UNIT_TYPE: i32 = 100;

struct QueueUpFleet {
    sim: Sim,
    runtime: LiveProductionRuntime,
    groups: Vec<i16>,
    queue: OrderQueue,
}

impl QueueUpFleet {
    fn build_row(&self, who: u8, o: i16) -> Option<usize> {
        let slot = usize::try_from(o)
            .ok()?
            .checked_sub(BUILD_BAND_BASE as usize)?;
        self.sim
            .world
            .objects
            .slot(who as usize)
            .band(Band::Build)
            .get(slot)
            .map(|&row| row as usize)
    }
}

impl Fleet for QueueUpFleet {
    fn alive(&self, who: u8, o: i16) -> bool {
        self.build_row(who, o)
            .and_then(|row| self.sim.builds.get(row))
            .is_some_and(BuildData::is_valid)
    }

    fn is_unit(&self, _who: u8, _o: i16) -> bool {
        false
    }

    fn is_building(&self, who: u8, o: i16) -> bool {
        self.build_row(who, o).is_some()
    }

    fn group_of(&self, who: u8, o: i16) -> i16 {
        self.build_row(who, o)
            .and_then(|row| self.groups.get(row).copied())
            .unwrap_or(-1)
    }

    fn set_group_of(&mut self, who: u8, o: i16, group: i16) {
        if let Some(row) = self.build_row(who, o) {
            self.groups[row] = group;
        }
    }

    fn uid(&self, who: u8, o: i16) -> u16 {
        self.build_row(who, o)
            .and_then(|row| self.sim.builds.get(row))
            .map_or(u16::MAX, |build| build.uid)
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

    fn apply_queue_up_action_transaction(
        &mut self,
        request: QueueUpActionRequest,
    ) -> QueueUpActionReceipt {
        apply_sim_queue_up_fleet_transaction(&mut self.sim, &mut self.runtime, request)
    }
}

fn group_wire(objects: &[i16]) -> Vec<u8> {
    let mut wire = vec![0, objects.len() as u8, OWNER];
    for &object in objects {
        wire.extend_from_slice(&object.to_le_bytes());
    }
    wire
}

fn queue_up_wire(type_index: i32, num: i32) -> [u8; 9] {
    let mut wire = [0; 9];
    wire[0] = 24;
    wire[1..5].copy_from_slice(&type_index.to_le_bytes());
    wire[5..9].copy_from_slice(&num.to_le_bytes());
    wire
}

fn build(queue_len: u8, uid: u16) -> BuildData {
    BuildData {
        flags: flag::VALID | flag::ACTIVE,
        who: OWNER,
        uid,
        queue: BuildQueue {
            queued: queue_len,
            entries: vec![BuildQueueEntry::default(); 3],
        },
        ..BuildData::default()
    }
}

#[test]
fn bridge_opcode24_sorts_and_commits_real_build_queues() {
    let mut sim = Sim::new(0x24_6f_dbb0, 8);
    let row0 = sim.spawn_build(OWNER as usize, build(1, 70));
    let row1 = sim.spawn_build(OWNER as usize, build(0, 71));
    assert_eq!(row0, 0);
    assert_eq!(row1, 1);

    let mut runtime = std::mem::take(&mut sim.production_runtime);
    runtime.register_build(row0, PRODUCER_TYPE);
    runtime.register_build(row1, PRODUCER_TYPE);
    runtime.install_type(LiveProductionType::in_place_building(PRODUCER_TYPE, 1));
    let mut unit = LiveProductionType::ordinary_unit(UNIT_TYPE, 20, 0);
    unit.repeat_cost = Some([10, 5, 0, 0, 0, 0]);
    unit.carrier_unqueue = Some(LiveCarrierUnqueueTypeFacts {
        object: Some(LiveCarrierUnqueueObjectFacts {
            attack: 1,
            training_site: Some(PRODUCER_TYPE),
            domain: None,
        }),
        refund_costs: Some([10, 5, 0, 0, 0, 0]),
    });
    runtime.install_type(unit);
    runtime.leaders[OWNER as usize].resources = [100, 100, 0, 0, 0, 0];
    sim.leaders[OWNER as usize].econ.stockpile = [100, 100, 0, 0, 0, 0];
    sim.step8.leaders[OWNER as usize].econ.stockpile = [100, 100, 0, 0, 0, 0];

    let mut fleet = QueueUpFleet {
        sim,
        runtime,
        groups: vec![-1; 2],
        queue: OrderQueue::new(),
    };
    let objects = [BUILD_BAND_BASE as i16, BUILD_BAND_BASE as i16 + 1];
    let mut package = Package::new(OWNER as i32, 0);
    let mut bridge = Bridge::new();
    bridge
        .process_all(&mut package, &group_wire(&objects), &mut fleet)
        .unwrap();
    assert!(package.group >= 0);
    bridge.groups.get_mut(package.group).unwrap().disband = 91;

    bridge
        .process_all(&mut package, &queue_up_wire(UNIT_TYPE, 2), &mut fleet)
        .unwrap();

    assert_eq!(bridge.groups.get(package.group).unwrap().disband, 0);
    assert_eq!(fleet.sim.builds[row0].queue.queued, 3);
    assert_eq!(fleet.sim.builds[row1].queue.queued, 2);
    assert_eq!(
        fleet.sim.builds[row0].queue.entries[1].type_index,
        UNIT_TYPE as i16
    );
    assert_eq!(
        fleet.sim.builds[row1].queue.entries[0].type_index,
        UNIT_TYPE as i16
    );
    assert_eq!(fleet.sim.builds[row1].queue.entries[0].res, [0, 1, -1]);
    assert_eq!(fleet.sim.builds[row1].queue.entries[0].amt, [10, 5, 0]);
    assert_eq!(
        fleet.runtime.leaders[OWNER as usize].resources,
        [60, 80, 0, 0, 0, 0]
    );
    assert_eq!(
        fleet.runtime.leaders[OWNER as usize].queued_counts[UNIT_TYPE as usize],
        4
    );
    assert_eq!(
        fleet.runtime.leaders[OWNER as usize]
            .carrier_training_queued
            .factory,
        4
    );
    assert_eq!(bridge.stats.acted, 1);
    assert_eq!(bridge.stats.open_group_action_tails, 0);
    assert_eq!(bridge.stats.unported, 0);
}

#[test]
fn missing_cost_projection_leaves_every_owner_unchanged() {
    let mut sim = Sim::new(0x24_0000_0001, 8);
    let row = sim.spawn_build(OWNER as usize, build(0, 80));
    let mut runtime = std::mem::take(&mut sim.production_runtime);
    runtime.register_build(row, PRODUCER_TYPE);
    runtime.install_type(LiveProductionType::in_place_building(PRODUCER_TYPE, 1));
    runtime.install_type(LiveProductionType::ordinary_unit(UNIT_TYPE, 20, 0));
    let before = sim.builds[row].clone();
    let mut fleet = QueueUpFleet {
        sim,
        runtime,
        groups: vec![-1],
        queue: OrderQueue::new(),
    };
    let mut package = Package::new(OWNER as i32, 0);
    let mut bridge = Bridge::new();
    bridge.process_one(
        &mut package,
        &group_wire(&[BUILD_BAND_BASE as i16]),
        &mut fleet,
    );
    bridge.process_one(&mut package, &queue_up_wire(UNIT_TYPE, 1), &mut fleet);

    assert_eq!(fleet.sim.builds[row].queue.queued, before.queue.queued);
    assert_eq!(fleet.sim.builds[row].queue.entries, before.queue.entries);
    assert_eq!(bridge.stats.open_group_action_tails, 1);
    assert_eq!(bridge.stats.unported, 1);
}
