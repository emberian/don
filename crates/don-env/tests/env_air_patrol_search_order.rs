//! AIR_PATROL advances its waypoint before invoking either due target search.
//!
//! The test host supplies no target-selection approximation. It records only the query
//! transaction handed across the explicit host seam.

use don_env::spec::EnvConfig;
use don_env::state::{AirPatrolHost, AirPatrolHostError, EnvWorld, Rules};
use don_sim::command::QueuePos;
use don_sim::systems::order_dispatch::AirPatrolSearch;
use don_sim::systems::patrol::{AirPatrolOrder, AirPatrolTarget};

#[derive(Default)]
struct QueryRecorder {
    unit_target: Option<AirPatrolTarget>,
    building_cursor: Option<i32>,
    building_origin: Option<(i32, i32)>,
}

impl AirPatrolHost for QueryRecorder {
    fn preflight(&mut self, _world: &EnvWorld) -> Result<(), AirPatrolHostError> {
        Ok(())
    }

    fn think_bird(
        &mut self,
        _world: &mut EnvWorld,
        _row: usize,
        _order: &mut AirPatrolOrder,
    ) -> Result<(), AirPatrolHostError> {
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
        Ok(self.unit_target)
    }

    fn find_building_target(
        &mut self,
        _world: &EnvWorld,
        _row: usize,
        order: &AirPatrolOrder,
        search_x: i32,
        search_y: i32,
    ) -> Result<Option<AirPatrolTarget>, AirPatrolHostError> {
        self.building_cursor = Some(order.points.waypoint);
        self.building_origin = Some((search_x, search_y));
        Ok(None)
    }
}

fn due_air_world() -> Option<(EnvWorld, usize)> {
    let (rules, caps_real, _) = Rules::load(None, None);
    if !caps_real {
        eprintln!("SKIP: schema/live/env-typecaps.bin absent (run gen/gen_spec.py)");
        return None;
    }
    let cfg = EnvConfig::default();
    let mut world = EnvWorld::new(rules, 4, 19, cfg.grid_w, cfg.grid_h);
    let actor = world.spawn(0, 289, 24, 24).unwrap();
    let row = world.sim.row_of(actor).unwrap();
    assert_eq!(world.sim.units.o()[row], 0);
    assert_eq!(world.sim.frame, 0, "both retail scan cadences are due");

    world
        .install_air_patrol_order(row, 24, 24, QueuePos::New)
        .unwrap();
    world
        .install_air_patrol_order(row, 240, 336, QueuePos::Last)
        .unwrap();
    Some((world, row))
}

#[test]
fn arrival_advances_cursor_before_due_building_query() {
    let Some((mut world, _row)) = due_air_world() else {
        return;
    };

    let mut host = QueryRecorder::default();
    world.frame_with_air_patrol_host(&mut host).unwrap();

    assert_eq!(host.building_cursor, Some(1));
    assert_eq!(
        host.building_origin,
        Some((240, 336)),
        "the mod-32 query must read the newly advanced waypoint"
    );
}

#[test]
fn accepted_unit_target_returns_before_building_host() {
    let Some((mut world, row)) = due_air_world() else {
        return;
    };
    let accepted = AirPatrolTarget {
        o: 3,
        who: 1,
        uid: 9,
        x: 480,
        y: 528,
        domain: don_sim::systems::air::DOMAIN_AIR,
        ever_seen_by_actor: false,
    };
    let mut host = QueryRecorder {
        unit_target: Some(accepted),
        ..QueryRecorder::default()
    };
    world.frame_with_air_patrol_host(&mut host).unwrap();

    assert_eq!(
        host.building_cursor, None,
        "retail returns after accepted mod-16 STRAFE insertion"
    );
    let front = world.orders[row].front().expect("inserted STRAFE");
    assert_eq!(front.kind, don_sim::order::OrderIndex::Strafe);
    assert_eq!(front.target_o, accepted.o);
    assert_eq!(front.target_who, accepted.who);
}
