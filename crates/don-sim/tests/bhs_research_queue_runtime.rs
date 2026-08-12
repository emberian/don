//! Canonical Sim transaction used by BHS builtin 357.

use don_sim::objects::BUILD_BAND_BASE;
use don_sim::systems::production::runtime::{
    apply_sim_single_library_research_transaction, LiveProductionRuntime, LiveProductionType,
    SingleLibraryResearchQueueResult, SingleLibraryResearchRequest, SingleLibraryResearchStatus,
    SingleLibraryResearchStep,
};
use don_sim::systems::production::{flag, off, BuildData, BuildQueue, BuildQueueEntry};
use don_sim::tick::Sim;

const OWNER: usize = 0;
const LIBRARY: i32 = 435;
const WRITTEN_WORD: i32 = 551;
const COST: [i32; 6] = [0, 12, 5, 0, 0, 0];

fn installed() -> (
    Sim,
    LiveProductionRuntime,
    usize,
    SingleLibraryResearchRequest,
) {
    let mut sim = Sim::new(0x357, 8);
    let object = BUILD_BAND_BASE;
    let mut build = BuildData {
        flags: flag::VALID | flag::ACTIVE,
        who: OWNER as u8,
        queue: BuildQueue {
            queued: 0,
            entries: vec![BuildQueueEntry::default(); 2],
        },
        ..BuildData::default()
    };
    build.other[off::OBJECT_ID..off::OBJECT_ID + 2].copy_from_slice(&(object as i16).to_le_bytes());
    let row = sim.spawn_build(OWNER, build);

    let mut runtime = std::mem::take(&mut sim.production_runtime);
    runtime.register_build(row, LIBRARY);
    let mut library = LiveProductionType::in_place_building(LIBRARY, 1);
    library.is_library = true;
    runtime.install_type(library);
    let mut research = LiveProductionType::research(WRITTEN_WORD, 200);
    research.repeat_cost = Some(COST);
    runtime.install_type(research);
    runtime.leaders[OWNER].resources = [100; 6];
    sim.leaders[OWNER].econ.stockpile = [100; 6];
    sim.step8.leaders[OWNER].econ.stockpile = [100; 6];
    sim.vic_leaders.slots[OWNER].economy.bucket = [100; 6];

    (
        sim,
        runtime,
        row,
        SingleLibraryResearchRequest {
            owner: OWNER as u8,
            object_index: object as i16,
            producer_type: LIBRARY,
            research_type: WRITTEN_WORD,
            cost: COST,
        },
    )
}

#[test]
fn written_word_commits_group_queue_counters_and_all_resource_mirrors() {
    let (mut sim, mut runtime, row, request) = installed();
    let receipt = apply_sim_single_library_research_transaction(&mut sim, &mut runtime, request);

    assert_eq!(receipt.status, SingleLibraryResearchStatus::Applied);
    assert_eq!(
        receipt.queue_result,
        Some(SingleLibraryResearchQueueResult::Enqueued)
    );
    assert!(receipt.validates(request));
    assert_eq!(receipt.group_slot, Some(1));
    assert_eq!(sim.groups.last_group[OWNER], 1);
    assert_eq!(
        sim.groups.list[1].id, 1,
        "push_group preserves the fixed slot id"
    );
    assert_eq!(sim.groups.list[1].who, OWNER as u8);
    assert_eq!(sim.groups.list[1].num, 1);
    assert_eq!(sim.groups.list[1].buildings, 1);
    assert_eq!(sim.groups.list[1].list[0], BUILD_BAND_BASE as i16);
    assert_eq!(
        receipt.steps,
        [
            SingleLibraryResearchStep::BuildCanQueue,
            SingleLibraryResearchStep::TemporaryGroupClear,
            SingleLibraryResearchStep::TemporaryGroupAdd,
            SingleLibraryResearchStep::GroupsPushGroup {
                slot: 1,
                reused: false,
            },
            SingleLibraryResearchStep::GroupActionQueueUp,
            SingleLibraryResearchStep::BuildActionQueue { queue_slot: 0 },
        ]
    );

    assert_eq!(sim.builds[row].queue.queued, 1);
    let entry = sim.builds[row].queue.entries[0];
    assert_eq!(entry.type_index, WRITTEN_WORD as i16);
    assert_eq!(entry.res, [1, 2, -1]);
    assert_eq!(entry.amt, [12, 5, 0]);
    assert_eq!(
        runtime.leaders[OWNER].queued_counts[WRITTEN_WORD as usize],
        1
    );
    assert_eq!(runtime.leaders[OWNER].ages_queued, 0);
    assert_eq!(runtime.leaders[OWNER].epochs_queued, 1);
    assert_eq!(
        runtime.leaders[OWNER].resources,
        [100, 88, 95, 100, 100, 100]
    );
    assert_eq!(
        sim.leaders[OWNER].econ.stockpile,
        runtime.leaders[OWNER].resources
    );
    assert_eq!(
        sim.step8.leaders[OWNER].econ.stockpile,
        runtime.leaders[OWNER].resources
    );
    assert_eq!(
        sim.vic_leaders.slots[OWNER].economy.bucket,
        runtime.leaders[OWNER].resources
    );
    assert_eq!(
        sim.vic_leaders.slots[OWNER].num_queued[WRITTEN_WORD as usize],
        1
    );
}

#[test]
fn can_pay_refusal_precedes_cursor_group_and_queue_mutation() {
    let (mut sim, mut runtime, row, request) = installed();
    runtime.leaders[OWNER].resources = [0; 6];
    sim.leaders[OWNER].econ.stockpile = [0; 6];
    sim.step8.leaders[OWNER].econ.stockpile = [0; 6];
    sim.vic_leaders.slots[OWNER].economy.bucket = [0; 6];
    sim.groups.list[1].army = 77;
    sim.groups.list[1].form = 9;
    sim.groups.list[1].order_num = 23;
    sim.groups.list[1].disband = 1;
    let groups_before = sim.groups.clone();

    let receipt = apply_sim_single_library_research_transaction(&mut sim, &mut runtime, request);

    assert_eq!(receipt.status, SingleLibraryResearchStatus::Unavailable);
    assert_eq!(receipt.queue_result, None);
    assert_eq!(receipt.queue_slot, None);
    assert!(receipt.validates(request));
    assert_eq!(sim.groups.list, groups_before.list);
    assert_eq!(sim.groups.last_group, groups_before.last_group);
    assert_eq!(sim.builds[row].queue.queued, 0);
    assert_eq!(
        runtime.leaders[OWNER].queued_counts[WRITTEN_WORD as usize],
        0
    );
    assert_eq!(runtime.leaders[OWNER].ages_queued, 0);
    assert_eq!(runtime.leaders[OWNER].epochs_queued, 0);
    assert_eq!(runtime.leaders[OWNER].resources, [0; 6]);
    assert_eq!(
        sim.vic_leaders.slots[OWNER].num_queued[WRITTEN_WORD as usize],
        0
    );
}

#[test]
fn six_good_payment_stores_only_the_first_three_nonzero_cost_cells() {
    let (mut sim, mut runtime, row, mut request) = installed();
    request.cost = [1, 1, 1, 1, 0, 0];
    runtime.types[WRITTEN_WORD as usize]
        .as_mut()
        .unwrap()
        .repeat_cost = Some(request.cost);
    let receipt = apply_sim_single_library_research_transaction(&mut sim, &mut runtime, request);
    assert_eq!(receipt.status, SingleLibraryResearchStatus::Applied);
    assert!(receipt.validates(request));
    assert_eq!(sim.builds[row].queue.queued, 1);
    assert_eq!(sim.builds[row].queue.entries[0].res, [0, 1, 2]);
    assert_eq!(sim.builds[row].queue.entries[0].amt, [1, 1, 1]);
    assert_eq!(runtime.leaders[OWNER].resources, [99, 99, 99, 99, 100, 100]);
    assert_eq!(
        runtime.leaders[OWNER].queued_counts[WRITTEN_WORD as usize],
        1
    );
    assert_eq!(runtime.leaders[OWNER].ages_queued, 0);
    assert_eq!(runtime.leaders[OWNER].epochs_queued, 1);
}

#[test]
fn duplicate_or_desynchronised_research_refuses_before_group_allocation() {
    for defect in 0..5 {
        let (mut sim, mut runtime, row, request) = installed();
        match defect {
            0 => runtime.leaders[OWNER].tech.tech.set(WRITTEN_WORD, true),
            1 => runtime.leaders[OWNER].queued_counts[WRITTEN_WORD as usize] = 1,
            2 => sim.step8.leaders[OWNER].econ.stockpile[0] = 99,
            3 => sim.vic_leaders.slots[OWNER].economy.bucket[0] = 99,
            4 => sim.vic_leaders.slots[OWNER].num_queued[WRITTEN_WORD as usize] = 1,
            _ => unreachable!(),
        }
        let last_before = sim.groups.last_group;
        let receipt =
            apply_sim_single_library_research_transaction(&mut sim, &mut runtime, request);
        assert_eq!(receipt.status, SingleLibraryResearchStatus::Unavailable);
        assert_eq!(sim.groups.last_group, last_before);
        assert_eq!(sim.builds[row].queue.queued, 0);
    }
}
