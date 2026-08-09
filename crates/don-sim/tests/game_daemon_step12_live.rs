//! Real `Sim::do_frame` reachability proof for the exact step-12 shell and collision reaper.

use don_sim::systems::{game_daemon_step12::REGION_SLOTS, map_terrain::CollBlock};
use don_sim::tick::{Gap, Sim, StepRun};

#[test]
fn real_tick_runs_the_shell_reaps_the_live_world_and_retains_both_cursor_views() {
    let mut sim = Sim::new(0x12_731f90, 16);
    assert_eq!(sim.map.regions.len(), REGION_SLOTS);
    sim.map.world.wdata[0].block = Some(Box::new(CollBlock::default()));

    let first = sim.do_frame();

    assert_eq!(first.steps[12], StepRun::Executed);
    assert!(
        first.work[12] >= 2,
        "collision and Groups bodies always run"
    );
    assert!(
        sim.map.world.wdata[0].block.is_none(),
        "the reaper must mutate the authoritative map, not a compatibility copy"
    );
    assert_eq!(sim.collision_blocks.cursor(), 5);
    assert_eq!(sim.game_daemon.empty_colls, 5);
    assert_eq!(sim.groups.proc_group, 1);
    assert_eq!(sim.cover.gaps[Gap::GameDaemonProcessCollBlocks.index()], 0);
    assert_eq!(sim.cover.gaps[Gap::GameDaemonCalcDanger.index()], 1);

    let second = sim.do_frame();
    assert_eq!(second.steps[12], StepRun::Executed);
    assert_eq!(sim.collision_blocks.cursor(), 10);
    assert_eq!(sim.game_daemon.empty_colls, 10);
    assert_eq!(sim.groups.proc_group, 2);
    assert_eq!(
        sim.cover.gaps[Gap::GameDaemonCalcDanger.index()],
        1,
        "frame 1 must not charge the frame%200 danger child"
    );
}

#[test]
fn cursor_mirror_divergence_refuses_step12_before_mutation() {
    let mut sim = Sim::new(0x12_00732700, 8);
    sim.game_daemon.repaths = [12; 8];
    sim.game_daemon.empty_colls = 7;
    let before_regions: Vec<_> = sim.map.regions.iter().map(|region| region.flags).collect();

    let trace = sim.do_frame();

    assert_eq!(
        trace.steps[12],
        StepRun::Unimplemented(Gap::GameDaemonProcessCollBlocks)
    );
    assert_eq!(sim.game_daemon.repaths, [12; 8]);
    assert_eq!(sim.game_daemon.empty_colls, 7);
    assert_eq!(sim.collision_blocks.cursor(), 0);
    assert_eq!(
        sim.map
            .regions
            .iter()
            .map(|region| region.flags)
            .collect::<Vec<_>>(),
        before_regions
    );
    assert_eq!(sim.cover.gaps[Gap::GameDaemonProcessCollBlocks.index()], 1);
}
