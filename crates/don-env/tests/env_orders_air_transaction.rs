//! Hosted AIR_PATROL failures restore the environment transaction.
//!
//! This does not close any of the three product blockers: the fake host exists only to
//! force a late error after mutating the airframe, proving that external-host failure does
//! not leave the RL world half advanced.

use don_env::spec::EnvConfig;
use don_env::state::{AirPatrolHost, AirPatrolHostBoundary, AirPatrolHostError, EnvWorld, Rules};
use don_sim::command::QueuePos;
use don_sim::systems::order_dispatch::AirPatrolSearch;
use don_sim::systems::patrol::{AirPatrolOrder, AirPatrolTarget};

struct LateUnitSearchFailure;

impl AirPatrolHost for LateUnitSearchFailure {
    fn preflight(&mut self, _world: &EnvWorld) -> Result<(), AirPatrolHostError> {
        Ok(())
    }

    fn think_bird(
        &mut self,
        world: &mut EnvWorld,
        row: usize,
        order: &mut AirPatrolOrder,
    ) -> Result<(), AirPatrolHostError> {
        world.spell_time[row] = 77;
        order.points.waypoint = 1;
        Ok(())
    }

    fn do_air_physics(
        &mut self,
        world: &mut EnvWorld,
        row: usize,
        _order: &mut AirPatrolOrder,
        target_x: i32,
        target_y: i32,
    ) -> Result<bool, AirPatrolHostError> {
        world.sim.set_pos(row, target_x, target_y);
        Ok(true)
    }

    fn actor_is_type(
        &mut self,
        _world: &EnvWorld,
        _row: usize,
        _type_id: i32,
        _strict: bool,
    ) -> Result<bool, AirPatrolHostError> {
        Ok(false)
    }

    fn find_unit_target(
        &mut self,
        _world: &EnvWorld,
        _row: usize,
        _order: &AirPatrolOrder,
        _search_x: i32,
        _search_y: i32,
        _search: AirPatrolSearch,
    ) -> Result<Option<AirPatrolTarget>, AirPatrolHostError> {
        Err(AirPatrolHostError::Unavailable(
            AirPatrolHostBoundary::UnitTargetSearch,
        ))
    }

    fn find_building_target(
        &mut self,
        _world: &EnvWorld,
        _row: usize,
        _order: &AirPatrolOrder,
        _search_x: i32,
        _search_y: i32,
    ) -> Result<Option<AirPatrolTarget>, AirPatrolHostError> {
        Ok(None)
    }
}

#[test]
fn late_air_host_error_restores_airframe_order_and_frame() {
    let (rules, _, _) = Rules::load(None, None);
    let cfg = EnvConfig::default();
    let mut world = EnvWorld::new(rules, 4, 17, cfg.grid_w, cfg.grid_h);
    let actor = world.spawn(0, 289, 2, 3).unwrap();
    let row = world.sim.row_of(actor).unwrap();
    // First owner-local object: (o + frame) is divisible by 16, so the failing unit search
    // runs after think_bird and air physics have both mutated the world.
    assert_eq!(world.sim.units.o()[row], 0);
    world
        .install_air_patrol_order(row, 900, 700, QueuePos::New)
        .unwrap();

    let before_order = world.orders[row].clone();
    let before_position = (world.sim.pos_x()[row], world.sim.pos_y()[row]);
    let before_spell = world.spell_time[row];
    let before_frame = world.sim.frame;
    assert_eq!(
        world.frame_with_air_patrol_host(&mut LateUnitSearchFailure),
        Err(AirPatrolHostError::Unavailable(
            AirPatrolHostBoundary::UnitTargetSearch
        ))
    );
    assert_eq!(world.orders[row], before_order);
    assert_eq!(
        (world.sim.pos_x()[row], world.sim.pos_y()[row]),
        before_position
    );
    assert_eq!(world.spell_time[row], before_spell);
    assert_eq!(world.sim.frame, before_frame);
}
