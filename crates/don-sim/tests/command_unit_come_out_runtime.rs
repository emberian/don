//! Real Bridge coverage for opcode 49's canonical four-tranche host.

use don_sim::command::direct_entity_command_integration::{
    DirectEntityDisposition, DirectEntityFleetReceipt, DirectEntityFleetRequest,
    DirectEntityTransactionStatus,
};
use don_sim::command::unit_come_out_runtime::{
    apply_sim_come_out_fleet_transaction, UnitComeOutResumePayload, UnitComeOutRuntime,
};
use don_sim::command::{Bridge, Fleet, InlineDef, InlinePort, Package};
use don_sim::order::{Order, OrderIndex};
use don_sim::systems::groups_guys::{GuyData, UnitGuys};
use don_sim::systems::movement::{PathData, PathStack};
use don_sim::systems::movement_live::{LiveCollisionGuy, LiveCollisionSource};
use don_sim::systems::order_dispatch::OrderQueue;
use don_sim::systems::production::{self, BuildData};
use don_sim::systems::unit_come_out_body_map::{
    CanonicalObjectIdentity, CanonicalPoint, CanonicalRngStamp,
};
use don_sim::systems::unit_come_out_full_frontier::ExitConstants;
use don_sim::tick::{CrashUnitSource, Sim};

const OWNER: u8 = 1;
const TYPE_INDEX: i32 = 50;
const CONTAINER_TYPE: i32 = 700;
const GROUP_SLOT: i16 = 2;
const START: (i32, i32) = (800, 900);
const RELEASE: (i32, i32) = (1_280, 1_344);
const BUILD_POINT: (i32, i32) = (1_152, 1_216);
const CONTAINER_ANGLE: i32 = 0x4000_0000;

struct ComeOutFleet<'a> {
    sim: &'a mut Sim,
    runtime: &'a mut UnitComeOutRuntime,
    queue: OrderQueue,
}

impl Fleet for ComeOutFleet<'_> {
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
        request: DirectEntityFleetRequest,
    ) -> DirectEntityFleetReceipt {
        apply_sim_come_out_fleet_transaction(self.sim, self.runtime, request)
    }
}

fn write_build_identity_and_position(build: &mut BuildData, object: i16, point: (i32, i32)) {
    build.other[production::off::OBJECT_ID..production::off::OBJECT_ID + 2]
        .copy_from_slice(&object.to_le_bytes());
    build.other[production::off::X_INTERNAL..production::off::X_INTERNAL + 4]
        .copy_from_slice(&(point.0 ^ 0x63637).to_le_bytes());
    build.other[production::off::Y_INTERNAL..production::off::Y_INTERNAL + 4]
        .copy_from_slice(&(point.1 ^ 0x63637).to_le_bytes());
}

fn collision_source() -> LiveCollisionSource {
    LiveCollisionSource {
        domain: 0,
        block_radius: 1,
        big_radius: 2,
        push_size: 0,
        push_circles: 0,
        unit_flags: 0,
        unit_flags2: 0,
        attack_value: 0,
        spell_id: 0,
        unpacking: false,
        captain: true,
        moving: true,
        searching: false,
        action: OrderIndex::MoveTo as i32,
        invalid_tiles: Vec::new(),
        guys: vec![LiveCollisionGuy {
            x: START.0,
            y: START.1,
            angle: 0x1111_1111,
            block_radius: 1,
        }],
    }
}

fn graphics_source(object: i16) -> CrashUnitSource {
    let mut guy = GuyData::default();
    guy.ty = TYPE_INDEX;
    guy.who = OWNER as i8;
    guy.o = object;
    guy.x = START.0;
    guy.y = START.1;
    guy.z = 17;
    guy.angle = 0x1111_1111;
    guy.last_x = 700;
    guy.last_y = 701;
    guy.last_z = 16;
    guy.last_angle = 0x0101_0101;
    guy.des_x = 1_000;
    guy.des_y = 1_001;
    guy.des_angle = 0x0202_0202;
    CrashUnitSource {
        guys: UnitGuys {
            guys: vec![Some(guy)],
            size: 1,
            increment: 0,
            flags: 0,
            guy_mark: 1,
        },
        gpiece: Some(9),
    }
}

fn setup(with_graphics: bool) -> (Sim, UnitComeOutResumePayload, usize, i16) {
    let mut sim = Sim::new(0x49_617c10, 8);
    sim.step8.leaders[OWNER as usize].flags = 4;
    let handle = sim
        .spawn_unit(OWNER as usize, TYPE_INDEX, START.0, START.1, 1)
        .expect("unit");
    let row = sim.world.row_of(handle).unwrap();
    let object = sim.world.units.o()[row];
    let uid = sim.world.units.get_uid(row);
    sim.world.units.z_internal_mut()[row] = 17;
    sim.world.units.angle_mut()[row] = 0x1111_1111;
    sim.world.units.unit_masks_mut()[row] = 0x0400_0042;
    sim.world.orders_mut(row).push(Order {
        kind: OrderIndex::MoveTo,
        x: 2_000,
        y: 2_100,
        ..Order::default()
    });
    sim.paths[row] = PathStack::new();
    sim.paths[row].push(PathData::default());

    let group = sim.groups.get_mut(OWNER as usize, GROUP_SLOT as usize);
    group.id = GROUP_SLOT as i32;
    assert!(group.add(object, OWNER, false, 0, 0));
    sim.world.units.group_mut()[row] = GROUP_SLOT;

    let mut build = BuildData {
        flags: production::flag::VALID | production::flag::ACTIVE,
        uid: 900,
        orig_type: CONTAINER_TYPE,
        gpiece: 321,
        city: -1,
        ..BuildData::default()
    };
    let container_object = production::BUILD_BAND_BASE as i16;
    write_build_identity_and_position(&mut build, container_object, BUILD_POINT);
    assert_eq!(sim.spawn_build(OWNER as usize, build), 0);
    sim.world.units.inside_up_mut()[row] = container_object;
    sim.world.units.inside_up_who_mut()[row] = OWNER as i8;
    sim.movement_collision
        .install_contained(&sim.world, &sim.map.world, handle, collision_source())
        .expect("contained collision source");
    if with_graphics {
        assert!(sim.install_crash_unit_source(handle, graphics_source(object)));
    }

    let payload = UnitComeOutResumePayload {
        actor: CanonicalObjectIdentity::new(OWNER as i8, object),
        uid,
        type_index: TYPE_INDEX,
        container: CanonicalObjectIdentity::new(OWNER as i8, container_object),
        container_uid: 900,
        container_type_index: CONTAINER_TYPE,
        object_epoch: 401,
        order_epoch: 402,
        containment_epoch: 403,
        guy_epoch: 404,
        leader_epoch: 405,
        actor_obj_masks: 0,
        actor_block_radius: 1,
        actor_big_radius: 2,
        actor_unit_type_flags: 0,
        actor_x_size: 1,
        actor_y_size: 1,
        actor_uber_size: 1,
        actor_unit_flags2: 0,
        actor_attack: 0,
        actor_where_type: -1,
        actor_domain_query: 0,
        actor_type_movement_query: 0,
        container_angle: CONTAINER_ANGLE,
        container_block_radius: 64,
        container_x_size: 2,
        container_y_size: 2,
        container_inside_down: -1,
        container_matches_university: false,
        container_matches_oil_platform: false,
        container_gather_inside: false,
        constants: ExitConstants {
            land_min: 64,
            land_max: 384,
            water_min: 64,
            water_max: 384,
            ordinary_padding: 320,
        },
        release_point: CanonicalPoint {
            x: RELEASE.0,
            y: RELEASE.1,
        },
        terrain_z: 29,
        rng: CanonicalRngStamp {
            seed: sim.world.random.state() as u32,
            draws: 0,
        },
        guy_hint_lengths: vec![7],
        options_rebuild: false,
    };
    (sim, payload, row, object)
}

fn come_out_wire(object: i16, uid: u16) -> Vec<u8> {
    let mut wire = vec![49];
    wire.extend_from_slice(&(OWNER as i32).to_le_bytes());
    wire.extend_from_slice(&(object as i32).to_le_bytes());
    wire.extend_from_slice(&(uid as i16).to_le_bytes());
    wire
}

#[test]
fn saved_payload_resumes_and_real_bridge_packet_commits_all_four_tranches() {
    let (mut sim, payload, row, object) = setup(true);
    let mut first_runtime = UnitComeOutRuntime::default();
    assert!(first_runtime.install(payload.clone()));
    let bytes = first_runtime.save(payload.actor).expect("saved payload");
    assert_eq!(
        UnitComeOutResumePayload::decode(&bytes),
        Ok(payload.clone())
    );
    assert!(UnitComeOutResumePayload::decode(&bytes[..bytes.len() - 1]).is_err());

    let mut runtime = UnitComeOutRuntime::default();
    runtime.resume(&bytes).expect("resume payload");
    assert_eq!(
        runtime.save(payload.actor).as_deref(),
        Some(bytes.as_slice())
    );
    let mut bridge = Bridge::new();
    bridge.frame = 144;
    let mut package = Package::new(0, 0);
    let wire = come_out_wire(object, payload.uid);
    {
        let mut fleet = ComeOutFleet {
            sim: &mut sim,
            runtime: &mut runtime,
            queue: OrderQueue::new(),
        };
        bridge.process_all(&mut package, &wire, &mut fleet).unwrap();
    }

    let records = bridge.take_direct_entity_receipts();
    assert_eq!(records.len(), 1);
    assert!(records[0].valid);
    let DirectEntityFleetReceipt::Entity(receipt) = &records[0].observed else {
        panic!("entity receipt")
    };
    assert_eq!(receipt.status, DirectEntityTransactionStatus::Complete);
    assert!(matches!(
        receipt.disposition,
        Some(DirectEntityDisposition::CompleteUnitActionComeOut { argument: 0, .. })
    ));
    let body = receipt
        .unit_come_out
        .as_ref()
        .expect("four-tranche receipt");
    assert!(body.validates());
    let mut tampered = body.clone();
    tampered.after.angle ^= 1;
    assert!(!tampered.validates());
    assert!(!body.prefix_plan.steps.is_empty());
    assert!(!body.common_plan.steps.is_empty());
    assert!(!body.gather_plan.steps.is_empty());
    assert!(!body.tail_plan.steps.is_empty());

    assert_eq!(
        (
            sim.world.units.x_internal()[row],
            sim.world.units.y_internal()[row]
        ),
        RELEASE
    );
    assert_eq!(sim.world.units.z_internal()[row], 29);
    assert_eq!(sim.world.units.angle()[row], CONTAINER_ANGLE);
    assert_eq!(sim.world.units.inside_up()[row], -1);
    assert_eq!(sim.world.units.inside_up_who()[row], -1);
    assert_eq!(sim.world.units.get_unit_masks(row), 0x42);
    assert_eq!(sim.world.units.group()[row], -1);
    assert!(!sim
        .groups
        .get(OWNER as usize, GROUP_SLOT as usize)
        .member(object));
    assert!(sim.world.orders(row).is_empty());
    assert!(sim.paths[row].is_empty());
    let movement = sim
        .movement_source_state(sim.world.handle_at_row(row).unwrap())
        .unwrap();
    assert_eq!(movement.revision, 1);
    assert!(!movement.moving);
    assert_eq!(movement.action, OrderIndex::None as i32);
    let guy = sim.crash_units[row].as_ref().unwrap().guys.guys[0]
        .as_ref()
        .unwrap();
    assert_eq!((guy.x, guy.y, guy.z), (RELEASE.0, RELEASE.1, 29));
    assert_eq!(
        (guy.last_x, guy.last_y, guy.last_z),
        (RELEASE.0, RELEASE.1, 29)
    );
    assert_eq!((guy.des_x, guy.des_y), RELEASE);
    assert_eq!(guy.angle, CONTAINER_ANGLE);
    assert!(runtime.options_rebuild);
    assert!(runtime.save(payload.actor).is_none());
    assert_eq!(bridge.stats.by_opcode[49], 1);
    assert_eq!(InlineDef::find(49).unwrap().port, InlinePort::StateWired);
}

#[test]
fn missing_graphics_owner_fails_before_any_canonical_mutation() {
    let (mut sim, payload, row, object) = setup(false);
    let before = (
        sim.world.units.x_internal()[row],
        sim.world.units.y_internal()[row],
        sim.world.units.inside_up()[row],
        sim.world.units.get_unit_masks(row),
        sim.world.orders(row).len(),
        sim.paths[row].len(),
    );
    let mut runtime = UnitComeOutRuntime::default();
    assert!(runtime.install(payload.clone()));
    let mut bridge = Bridge::new();
    bridge.frame = 144;
    let mut package = Package::new(0, 0);
    {
        let mut fleet = ComeOutFleet {
            sim: &mut sim,
            runtime: &mut runtime,
            queue: OrderQueue::new(),
        };
        bridge
            .process_all(
                &mut package,
                &come_out_wire(object, payload.uid),
                &mut fleet,
            )
            .unwrap();
    }
    let records = bridge.take_direct_entity_receipts();
    assert_eq!(records.len(), 1);
    assert!(records[0].valid);
    let DirectEntityFleetReceipt::Entity(receipt) = &records[0].observed else {
        panic!("entity receipt")
    };
    assert_eq!(receipt.status, DirectEntityTransactionStatus::Unavailable);
    assert_eq!(
        before,
        (
            sim.world.units.x_internal()[row],
            sim.world.units.y_internal()[row],
            sim.world.units.inside_up()[row],
            sim.world.units.get_unit_masks(row),
            sim.world.orders(row).len(),
            sim.paths[row].len(),
        )
    );
    assert!(!runtime.options_rebuild);
}
