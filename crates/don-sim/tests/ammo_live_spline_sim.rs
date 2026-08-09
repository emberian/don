use don_sim::systems::ammo::{
    CrashEnv, LaunchOrder, ObjView, ShooterRules, SpawnPoint, SplineVec3, FLAG_FLYING, TRAJ_SPLINE,
};
use don_sim::tick::Sim;

struct TerrainHeight {
    wcells: i32,
    z: i32,
}

impl CrashEnv for TerrainHeight {
    fn crash_world_wcells(&self) -> (i32, i32) {
        (self.wcells, self.wcells)
    }

    fn crash_terrain_z(&self, _x: i32, _y: i32) -> i32 {
        self.z
    }
}

fn launch_fixture(high_arc: bool) -> (LaunchOrder, ObjView, ObjView) {
    let shooter = ObjView {
        alive: true,
        is_unit: true,
        x: 100,
        y: 200,
        z: 200,
        guy_mark: 1,
        rules: ShooterRules {
            obj_masks: 0x0800_0000,
            to_hit: 100,
            proj_speed: 20,
            nuke_high_arc: high_arc,
            ..Default::default()
        },
        ..Default::default()
    };
    let target = ObjView {
        alive: true,
        is_unit: true,
        x: 3_500,
        y: 3_000,
        z: 500,
        guy_mark: 1,
        rules: ShooterRules::default(),
        ..Default::default()
    };
    let order = LaunchOrder {
        gpiece: 41,
        start: SpawnPoint {
            x: 100,
            y: 200,
            z: 300,
        },
        who: 0,
        o: 0,
        whom: 1,
        ox: 0,
        angle: 0,
        cruise_pose: don_sim::systems::ammo::CruiseLaunchPose {
            speed: 20,
            ..Default::default()
        },
        cosmetic: false,
    };
    (order, shooter, target)
}

#[test]
fn sim_launch_selects_and_owns_the_fixed_nuke_sidecar() {
    let mut sim = Sim::new(7, 16);
    let (order, shooter, target) = launch_fixture(true);
    let slot = sim.launch_ammo(&order, &shooter, Some(&target), 4_000, 90);

    assert_eq!(slot, 0);
    assert!(sim.ammo.slots[slot].occupied());
    assert!(sim.ammo.slots[slot].has_spline);
    assert_eq!(sim.ammo.slots[slot].w.flags & FLAG_FLYING, FLAG_FLYING);
    assert_eq!(sim.ammo.slots[slot].w.traj, TRAJ_SPLINE);
    assert_eq!(sim.ammo.slots[slot].w.total_time, 120);
    assert_eq!(
        sim.ammo.spline(slot).unwrap().control_verts[0],
        SplineVec3::new(100.0, 200.0, 300.0)
    );
    assert_eq!(sim.ammo.spline(slot).unwrap().control_verts.len(), 12);
    assert!(sim.ammo.checksum_complete().is_ok());
    let digest = sim.channel_digest();
    let path = sim.ammo.spline_slots[slot].as_mut().unwrap();
    path.spline_verts[8].z = f32::from_bits(path.spline_verts[8].z.to_bits() + 1);
    assert_ne!(
        sim.channel_digest(),
        digest,
        "the live reporting channel includes the nested slot-owned spline walk"
    );

    sim.ammo.close_slot(slot);
    assert!(sim.ammo.spline(slot).is_none());
    assert_eq!(sim.ammo.recycled_spline_count(), 1);
}

#[test]
fn sim_terrain_nuke_adapter_combines_exact_height_and_live_tile_flags() {
    let mut sim = Sim::new(11, 16);
    sim.install_crash_env(TerrainHeight {
        wcells: 16,
        z: 1_000,
    });
    // First interpolated query is (780,760), tile (4,3). Both altitude predicates match;
    // the bit-14/low-3 arm must win and lift the returned height by 1250.
    *sim.map.world.tmask_mut(4, 3) = 0x4033;
    let (order, shooter, target) = launch_fixture(false);
    let slot = sim.launch_ammo(&order, &shooter, Some(&target), 4_000, 90);

    let spline = sim.ammo.spline(slot).expect("terrain path installed");
    assert_eq!(spline.control_verts.len(), 8);
    assert_eq!(
        spline.control_verts[2],
        SplineVec3::new(780.0, 760.0, 2_250.0)
    );
    assert_eq!(
        sim.ammo.slots[slot].w.total_time as usize + 1,
        spline.spline_verts.len()
    );
    assert!(sim.ammo.checksum_complete().is_ok());
}

#[test]
fn terrain_nuke_without_exact_height_provider_fails_closed() {
    let mut sim = Sim::new(13, 16);
    let (order, shooter, target) = launch_fixture(false);
    let slot = sim.launch_ammo(&order, &shooter, Some(&target), 4_000, 90);

    assert!(!sim.ammo.slots[slot].occupied());
    assert!(!sim.ammo.slots[slot].has_spline);
    assert!(sim.ammo.spline(slot).is_none());
    assert_eq!(sim.ammo.recycled_spline_count(), 1);
}

#[test]
fn live_graphic_cruise_gate_has_priority_and_installs_its_derived_path() {
    let mut sim = Sim::new(15, 16);
    sim.ammo.install_spline_graphic(41);
    let (order, shooter, target) = launch_fixture(true);
    let slot = sim.launch_ammo(&order, &shooter, Some(&target), 4_000, 90);

    assert!(sim.ammo.slots[slot].occupied());
    assert!(sim.ammo.slots[slot].has_spline);
    let path = sim.ammo.spline(slot).expect("live cruise path");
    assert_eq!(path.control_verts.len(), 3);
    assert_eq!(path.degree, 2);
    assert_ne!(
        path.control_verts.len(),
        12,
        "graphic flag 8 must retain priority over the shooter's nuke mask"
    );
}

#[test]
fn step15_samples_then_recycles_the_pool_owned_path_on_close() {
    let mut sim = Sim::new(17, 16);
    sim.activate(1);
    sim.spawn_unit(1, 0, 3_500, 3_000, 10)
        .expect("live spline target");
    let (order, shooter, target) = launch_fixture(true);
    let slot = sim.launch_ammo(&order, &shooter, Some(&target), 4_000, 90);
    let expected = sim.ammo.spline(slot).unwrap().spline_verts[1];

    sim.do_frame();
    assert_eq!(sim.ammo.slots[slot].w.cur_time, 1);
    assert_eq!(
        (
            sim.ammo.slots[slot].w.ex,
            sim.ammo.slots[slot].w.ey,
            sim.ammo.slots[slot].w.ez
        ),
        (expected.x as i32, expected.y as i32, expected.z as i32)
    );
    assert!(sim.ammo.spline(slot).is_some());

    sim.ammo.slots[slot].w.flags = don_sim::systems::ammo::FLAG_ALIVE;
    sim.ammo.slots[slot].w.cur_time = 199;
    sim.do_frame();
    assert!(!sim.ammo.slots[slot].occupied());
    assert!(sim.ammo.spline(slot).is_none());
    assert_eq!(sim.ammo.recycled_spline_count(), 1);
}
