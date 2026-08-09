//! Farm-only provider integration for the recovered Gather transaction.
//!
//! This is structural/integration evidence, not a retail differential test. The fixture
//! supplies every still-external target/update/evaluator/leader input explicitly; EnvWorld
//! owns the persistent order, intrusive occupancy, main RNG, exact payout arithmetic and
//! checksum-visible byte images.

use don_env::action::{apply_unit_with_gather_host, ApplyStats, UnitAction};
use don_env::generated as g;
use don_env::spec::EnvConfig;
use don_env::state::{
    EnvFarmGatherOrder, EnvWorld, GatherHost, GatherHostError, GatherLeaderFrame, Rules,
};
use don_sim::command::QueuePos;
use don_sim::systems::collision::DOMAIN_LAND;
use don_sim::systems::economy::{CapGates, DoGatherContext, EconRules, GatherInputs, RES_FOOD};
use don_sim::systems::gather_lifecycle::{OrdinaryGatherKind, OrdinaryGatherTarget};
use don_sim::systems::gathering::{
    base_worker_gross, num_gatherers, GatherCount, GatherNearbyPoint, GatherSite, GatherTile,
};
use don_sim::systems::map_terrain::Coord;
use don_sim::world::SUBTILE;

#[derive(Clone)]
struct ExactFarmFixture {
    target: OrdinaryGatherTarget,
    per_worker: [i32; g::NUM_COMMON],
}

impl GatherHost for ExactFarmFixture {
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
        // FarmData::update's admitted result is 1. Gate value 1 keeps this deterministic
        // fixture on the measured 255/256 no-move arm, so no missing path host is crossed.
        Ok((1, 1))
    }

    fn farm_per_worker_gross(
        &mut self,
        _world: &EnvWorld,
        _site: &GatherSite,
    ) -> Result<[i32; g::NUM_COMMON], GatherHostError> {
        Ok(self.per_worker)
    }

    fn leader_frame(
        &mut self,
        _world: &EnvWorld,
        _who: u8,
    ) -> Result<GatherLeaderFrame, GatherHostError> {
        Ok(GatherLeaderFrame {
            // Explicit no-other-object/no-tech/no-wonder scenario. The Farm contribution
            // is added by EnvWorld from its active intrusive occupancy.
            inputs: GatherInputs::default(),
            cap_gates: CapGates::default(),
            payout: DoGatherContext::default(),
        })
    }
}

struct Rollout {
    order_after_install: [u8; 31],
    order_after_arrival: [u8; 31],
    econ_before: [u8; 0xf4],
    econ_after_arrival: [u8; 0xf4],
    econ_after_payout: [u8; 0xf4],
    sim_digest: u64,
    random_state: i32,
    food: i32,
    collected_food: i32,
    accumulator_food: i32,
    assigned: i32,
    active: i32,
    stats: ApplyStats,
}

fn rollout() -> Option<Rollout> {
    let (rules, caps_real, _) = Rules::load(None, None);
    if !caps_real {
        eprintln!("SKIP: schema/live/env-typecaps.bin absent (run gen/gen_spec.py)");
        return None;
    }
    let cfg = EnvConfig {
        grid_w: 16,
        grid_h: 16,
        max_entities: 8,
        max_controlled: 4,
        ..Default::default()
    };
    let mut world = EnvWorld::new(rules, 8, 0x6157, cfg.grid_w, cfg.grid_h);
    let worker = world.spawn(0, 0x32, 4 * SUBTILE, 4 * SUBTILE).unwrap();
    let farm = world
        .spawn(
            0,
            don_sim::systems::tech_cities::ty::FARM as u16,
            5 * SUBTILE,
            4 * SUBTILE,
        )
        .unwrap();
    world.obs_ents[0] = vec![worker, farm];
    let worker_row = world.sim.row_of(worker).unwrap();
    let worker_o = world.sim.units.o()[worker_row];
    let farm_row = world.sim.row_of(farm).unwrap();
    let target = OrdinaryGatherTarget {
        kind: OrdinaryGatherKind::Farm,
        centre: GatherNearbyPoint {
            x: Coord(world.sim.pos_x()[farm_row]),
            y: Coord(world.sim.pos_y()[farm_row]),
        },
        corner: GatherTile { tx: 18, ty: 14 },
        x_size: 4,
        y_size: 4,
        domain: DOMAIN_LAND,
        completed: true,
    };
    let mut per_worker = [0; g::NUM_COMMON];
    per_worker[RES_FOOD] = base_worker_gross(&EconRules::shipped(), false);
    assert_eq!(per_worker[RES_FOOD], 160);
    let mut host = ExactFarmFixture { target, per_worker };

    let econ_before = world.players[0].leader_econ.image();
    let mut stats = ApplyStats::default();
    apply_unit_with_gather_host(
        &mut world,
        &cfg,
        0,
        worker,
        UnitAction {
            verb: (g::uv::GATHER + 1) as u16,
            target_entity: 2,
            queue_pos: QueuePos::New as u16,
            ..Default::default()
        },
        &mut host,
        &mut stats,
    )
    .unwrap();
    assert_eq!(
        stats,
        ApplyStats {
            applied: 1,
            ..Default::default()
        }
    );
    assert_eq!(world.order[worker_row], g::OrderIndex::Gather as u8);
    let order_after_install = world.farm_gather_order(0, worker_o).unwrap().walk().image();
    assert_eq!(order_after_install[30], 0, "been_there starts clear");

    let site = &world.gather.sites[0];
    let assigned = num_gatherers(site, &world.gather.workers, GatherCount::Assigned, 0).unwrap();
    assert_eq!(assigned, 1);
    assert_eq!(
        num_gatherers(site, &world.gather.workers, GatherCount::Active, 0).unwrap(),
        0
    );

    world.frame_with_gather_host(&mut host).unwrap();
    let order_after_arrival = world.farm_gather_order(0, worker_o).unwrap().walk().image();
    assert_eq!(order_after_arrival[30], 1, "arrival is checksum-visible");
    assert_ne!(order_after_install, order_after_arrival);
    let site = &world.gather.sites[0];
    let active = num_gatherers(site, &world.gather.workers, GatherCount::Active, 0).unwrap();
    assert_eq!(active, 1);
    // Dirty recomputation is staggered: frame 0 is explicitly excluded and owner 0 first
    // recomputes when frame % 8 == 0. Payout still runs and writes the cap block meanwhile.
    assert_eq!(world.players[0].leader_econ.accumulator[RES_FOOD], 0);
    let econ_after_arrival = world.players[0].leader_econ.image();
    assert_ne!(econ_before, econ_after_arrival);

    for _ in 1..9 {
        world.frame_with_gather_host(&mut host).unwrap();
    }
    assert_eq!(world.players[0].leader_econ.gross[RES_FOOD], 160);
    assert_eq!(world.players[0].leader_econ.commerce_cap[RES_FOOD], 70);
    assert_eq!(world.players[0].leader_econ.accumulator[RES_FOOD], 70);
    // Frames 8..110 inclusive supply 103 payouts after the shipped age-0 commerce cap:
    // 70 * 103 == (GATHER_RATE 450 * 16) + 10.
    for _ in 9..111 {
        world.frame_with_gather_host(&mut host).unwrap();
    }
    assert_eq!(world.players[0].econ[RES_FOOD], 201);
    assert_eq!(world.players[0].collected[RES_FOOD], 1);
    assert_eq!(world.players[0].leader_econ.accumulator[RES_FOOD], 10);
    let econ_after_payout = world.players[0].leader_econ.image();
    assert_ne!(econ_after_arrival, econ_after_payout);

    Some(Rollout {
        order_after_install,
        order_after_arrival,
        econ_before,
        econ_after_arrival,
        econ_after_payout,
        sim_digest: world.sim.digest(),
        random_state: world.sim.random.state(),
        food: world.players[0].econ[RES_FOOD],
        collected_food: world.players[0].collected[RES_FOOD],
        accumulator_food: world.players[0].leader_econ.accumulator[RES_FOOD],
        assigned,
        active,
        stats,
    })
}

#[test]
fn explicit_farm_provider_changes_order_occupancy_income_and_checksum_images() {
    let Some(a) = rollout() else {
        return;
    };
    let b = rollout().expect("the same installed rules remain available");
    assert_eq!(a.order_after_install, b.order_after_install);
    assert_eq!(a.order_after_arrival, b.order_after_arrival);
    assert_eq!(a.econ_before, b.econ_before);
    assert_eq!(a.econ_after_arrival, b.econ_after_arrival);
    assert_eq!(a.econ_after_payout, b.econ_after_payout);
    assert_eq!(a.sim_digest, b.sim_digest);
    assert_eq!(a.random_state, b.random_state);
    assert_eq!((a.food, a.collected_food, a.accumulator_food), (201, 1, 10));
    assert_eq!((a.assigned, a.active), (1, 1));
    assert_eq!(a.stats.accepted_no_effect, 0);
}
