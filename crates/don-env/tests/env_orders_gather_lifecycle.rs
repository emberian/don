//! Transactional lifecycle coverage for the explicit Farm Gather provider.
//!
//! These are integration invariants over Tier-C recovered primitives, not retail
//! differential evidence. The provider remains explicit and the ordinary VecEnv mask
//! remains closed.

use don_env::action::{apply_unit, ApplyStats, UnitAction};
use don_env::generated as g;
use don_env::spec::EnvConfig;
use don_env::state::{
    EnvFarmGatherOrder, EnvWorld, GatherHost, GatherHostBoundary, GatherHostError,
    GatherLeaderFrame, Rules,
};
use don_sim::command::QueuePos;
use don_sim::systems::collision::DOMAIN_LAND;
use don_sim::systems::economy::{CapGates, DoGatherContext, GatherInputs};
use don_sim::systems::gather_lifecycle::{OrdinaryGatherKind, OrdinaryGatherTarget};
use don_sim::systems::gathering::{
    num_gatherers, GatherCount, GatherNearbyPoint, GatherSite, GatherTile,
};
use don_sim::systems::map_terrain::Coord;
use don_sim::world::SUBTILE;

#[derive(Clone)]
struct FarmHost {
    target: OrdinaryGatherTarget,
    game_gate: i32,
}

impl GatherHost for FarmHost {
    fn preflight(&mut self, _world: &EnvWorld) -> Result<(), GatherHostError> {
        Ok(())
    }

    fn farm_target(
        &mut self,
        _world: &EnvWorld,
        _worker_row: usize,
        _target_row: usize,
    ) -> Result<OrdinaryGatherTarget, GatherHostError> {
        Ok(self.target)
    }

    fn farm_first_tick(
        &mut self,
        _world: &EnvWorld,
        _order: &EnvFarmGatherOrder,
    ) -> Result<(i32, i32), GatherHostError> {
        Ok((1, self.game_gate))
    }

    fn farm_per_worker_gross(
        &mut self,
        _world: &EnvWorld,
        _site: &GatherSite,
    ) -> Result<[i32; g::NUM_COMMON], GatherHostError> {
        Ok([0; g::NUM_COMMON])
    }

    fn leader_frame(
        &mut self,
        _world: &EnvWorld,
        _who: u8,
    ) -> Result<GatherLeaderFrame, GatherHostError> {
        Ok(GatherLeaderFrame {
            inputs: GatherInputs::default(),
            cap_gates: CapGates::default(),
            payout: DoGatherContext::default(),
        })
    }
}

struct Fixture {
    cfg: EnvConfig,
    world: EnvWorld,
    worker: don_sim::Handle,
    farm: don_sim::Handle,
    second_farm: don_sim::Handle,
}

fn fixture() -> Fixture {
    let (rules, _, _) = Rules::load(None, None);
    let cfg = EnvConfig {
        grid_w: 16,
        grid_h: 16,
        max_entities: 8,
        max_controlled: 4,
        ..Default::default()
    };
    let mut world = EnvWorld::new(rules, 12, 0x6a71, cfg.grid_w, cfg.grid_h);
    let worker = world.spawn(0, 0x32, 4 * SUBTILE, 4 * SUBTILE).unwrap();
    let farm = world
        .spawn(
            0,
            don_sim::systems::tech_cities::ty::FARM as u16,
            5 * SUBTILE,
            4 * SUBTILE,
        )
        .unwrap();
    let second_farm = world
        .spawn(
            0,
            don_sim::systems::tech_cities::ty::FARM as u16,
            8 * SUBTILE,
            4 * SUBTILE,
        )
        .unwrap();
    world.obs_ents[0] = vec![worker, farm, second_farm];
    Fixture {
        cfg,
        world,
        worker,
        farm,
        second_farm,
    }
}

fn target_for(world: &EnvWorld, farm: don_sim::Handle) -> OrdinaryGatherTarget {
    let row = world.sim.row_of(farm).unwrap();
    OrdinaryGatherTarget {
        kind: OrdinaryGatherKind::Farm,
        centre: GatherNearbyPoint {
            x: Coord(world.sim.pos_x()[row]),
            y: Coord(world.sim.pos_y()[row]),
        },
        corner: GatherTile { tx: 18, ty: 14 },
        x_size: 4,
        y_size: 4,
        domain: DOMAIN_LAND,
        completed: true,
    }
}

fn install(f: &mut Fixture, farm: don_sim::Handle) {
    let worker_row = f.world.sim.row_of(f.worker).unwrap();
    let farm_row = f.world.sim.row_of(farm).unwrap();
    let target = target_for(&f.world, farm);
    f.world
        .install_farm_gather(worker_row, farm_row, target, QueuePos::New)
        .unwrap();
}

fn assigned_at(world: &EnvWorld, farm: don_sim::Handle) -> i32 {
    let row = world.sim.row_of(farm).unwrap();
    let owner = world.sim.owner()[row] as u8;
    let object = world.sim.units.o()[row];
    let site = world
        .gather
        .sites
        .iter()
        .find(|site| site.owner == owner && site.build_o == object)
        .unwrap();
    num_gatherers(site, &world.gather.workers, GatherCount::Assigned, 0).unwrap()
}

#[test]
fn halt_retires_farm_order_attachment_and_income_eligibility() {
    let mut f = fixture();
    let farm = f.farm;
    install(&mut f, farm);
    assert_eq!(assigned_at(&f.world, f.farm), 1);

    let mut stats = ApplyStats::default();
    apply_unit(
        &mut f.world,
        &f.cfg,
        0,
        f.worker,
        UnitAction {
            verb: (g::uv::HALT + 1) as u16,
            ..Default::default()
        },
        &mut stats,
    );
    assert_eq!(stats.applied, 1);
    let worker_row = f.world.sim.row_of(f.worker).unwrap();
    assert_eq!(f.world.order[worker_row], g::OrderIndex::None as u8);
    assert!(f.world.gather.farm_orders.is_empty());
    assert_eq!(assigned_at(&f.world, f.farm), 0);
    assert!(f.world.gather.workers[0].assignment.is_none());

    let econ = f.world.players[0].econ;
    let mut host = FarmHost {
        target: target_for(&f.world, f.farm),
        game_gate: 1,
    };
    for _ in 0..16 {
        f.world.frame_with_gather_host(&mut host).unwrap();
    }
    assert_eq!(f.world.players[0].econ, econ);
}

#[test]
fn queue_new_order_replacement_runs_gather_retirement_first() {
    let mut f = fixture();
    let farm = f.farm;
    install(&mut f, farm);
    let mut stats = ApplyStats::default();
    apply_unit(
        &mut f.world,
        &f.cfg,
        0,
        f.worker,
        UnitAction {
            verb: (g::uv::MOVE_TO + 1) as u16,
            target_x: 10,
            target_y: 9,
            queue_pos: QueuePos::New as u16,
            ..Default::default()
        },
        &mut stats,
    );
    assert_eq!(stats.applied, 1);
    let worker_row = f.world.sim.row_of(f.worker).unwrap();
    assert_eq!(f.world.order[worker_row], g::OrderIndex::MoveTo as u8);
    assert!(f.world.gather.farm_orders.is_empty());
    assert_eq!(assigned_at(&f.world, f.farm), 0);
}

#[test]
fn gather_replacement_detaches_old_site_before_attaching_new_site() {
    let mut f = fixture();
    let farm = f.farm;
    let second_farm = f.second_farm;
    install(&mut f, farm);
    install(&mut f, second_farm);
    assert_eq!(assigned_at(&f.world, f.farm), 0);
    assert_eq!(assigned_at(&f.world, f.second_farm), 1);
    assert_eq!(f.world.gather.farm_orders.len(), 1);
    let target_o = f.world.sim.units.o()[f.world.sim.row_of(f.second_farm).unwrap()];
    assert_eq!(f.world.gather.farm_orders[0].target_o, target_o);
}

#[test]
fn worker_and_target_despawn_each_retire_the_other_side_transactionally() {
    let mut worker_case = fixture();
    let worker_farm = worker_case.farm;
    install(&mut worker_case, worker_farm);
    assert_eq!(worker_case.world.try_despawn(worker_case.worker), Ok(true));
    assert!(worker_case.world.gather.farm_orders.is_empty());
    assert!(worker_case.world.gather.workers.is_empty());
    assert_eq!(assigned_at(&worker_case.world, worker_case.farm), 0);

    let mut target_case = fixture();
    let target_farm = target_case.farm;
    install(&mut target_case, target_farm);
    assert_eq!(target_case.world.try_despawn(target_case.farm), Ok(true));
    assert!(target_case.world.gather.farm_orders.is_empty());
    assert!(target_case.world.gather.sites.is_empty());
    assert!(target_case.world.gather.workers[0].assignment.is_none());
    let worker_row = target_case.world.sim.row_of(target_case.worker).unwrap();
    assert_eq!(
        target_case.world.order[worker_row],
        g::OrderIndex::None as u8
    );
}

#[test]
fn rejected_capacity_transaction_restores_every_gather_and_order_record() {
    let mut f = fixture();
    let farm = f.farm;
    install(&mut f, farm);
    let second_worker = f.world.spawn(0, 0x32, 3 * SUBTILE, 4 * SUBTILE).unwrap();
    let second_row = f.world.sim.row_of(second_worker).unwrap();
    let farm_row = f.world.sim.row_of(f.farm).unwrap();
    let before_gather = f.world.gather.clone();
    let before_order = f.world.orders[second_row].debug_image();
    let before_mirror = f.world.order[second_row];
    let before_dirty = f.world.players[0].gather_dirty;
    let target = target_for(&f.world, f.farm);
    let error = f
        .world
        .install_farm_gather(second_row, farm_row, target, QueuePos::New)
        .unwrap_err();
    assert_eq!(
        error,
        GatherHostError::InvalidState("Farm's single authoritative gather slot is occupied")
    );
    assert_eq!(f.world.gather, before_gather);
    assert_eq!(f.world.orders[second_row].debug_image(), before_order);
    assert_eq!(f.world.order[second_row], before_mirror);
    assert_eq!(f.world.players[0].gather_dirty, before_dirty);
}

#[test]
fn invalid_retirement_chain_refuses_halt_without_partial_cleanup() {
    let mut f = fixture();
    let farm = f.farm;
    install(&mut f, farm);
    let worker_row = f.world.sim.row_of(f.worker).unwrap();
    f.world.gather.sites[0].gather_down = 123;
    let before_gather = f.world.gather.clone();
    let before_order = f.world.orders[worker_row].clone();
    let before_mirror = f.world.order[worker_row];
    let before_dirty = f.world.players[0].gather_dirty;
    assert_eq!(
        f.world.clear_orders(worker_row),
        Err(GatherHostError::InvalidState(
            "Farm Gather retirement chain is invalid"
        ))
    );
    assert_eq!(f.world.gather, before_gather);
    assert_eq!(f.world.orders[worker_row], before_order);
    assert_eq!(f.world.order[worker_row], before_mirror);
    assert_eq!(f.world.players[0].gather_dirty, before_dirty);
}

#[test]
fn late_gather_host_error_restores_rng_phase_order_and_economy() {
    let mut f = fixture();
    let farm = f.farm;
    install(&mut f, farm);
    let worker_row = f.world.sim.row_of(f.worker).unwrap();
    let worker_o = f.world.sim.units.o()[worker_row];
    // worker_o=0 for the first owner-local object, so game_gate=0 crosses the measured
    // one-in-256 movement arm after setting been_there and consuming two RNG draws.
    assert_eq!(worker_o, 0);
    let before_gather = f.world.gather.clone();
    let before_queue = f.world.orders[worker_row].debug_image();
    let before_rng = f.world.sim.random.state();
    let before_frame = f.world.sim.frame;
    let before_econ = f.world.players[0].leader_econ.image();
    let mut host = FarmHost {
        target: target_for(&f.world, f.farm),
        game_gate: 0,
    };
    assert_eq!(
        f.world.frame_with_gather_host(&mut host),
        Err(GatherHostError::Unavailable(GatherHostBoundary::GatherMove))
    );
    assert_eq!(f.world.gather, before_gather);
    assert_eq!(f.world.orders[worker_row].debug_image(), before_queue);
    assert_eq!(f.world.sim.random.state(), before_rng);
    assert_eq!(f.world.sim.frame, before_frame);
    assert_eq!(f.world.players[0].leader_econ.image(), before_econ);
}
