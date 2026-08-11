//! Does the RL action space agree with the measured command→order bridge?
//!
//! `don-env::generated` derives its verb heads from `schema/command-wire.json` — the PDB
//! *type* stream. `don-sim::command` derives its opcode table from the same types **plus**
//! the disassembled `CommandPackage::process_*` handlers, so it knows which retail object
//! each opcode is actually handed to and which `OrderIndex` the resulting
//! `Unit::add_*_order` allocates.
//!
//! Where they agree, this file asserts it. Where they disagree, it pins the disagreement
//! as an explicit list so that closing the gap fails the test and forces the list to be
//! updated, rather than the divergence quietly persisting.
//!
//! Written by the `command-bridge` lane; additive, owns no existing path.

use don_env::generated as g;
use don_sim::command as cb;
use don_sim::order::OrderIndex;

fn opdef(op: u8) -> &'static cb::OpDef {
    &cb::OPCODES[op as usize]
}

/// Wire sizes: every verb the env can emit must carry the length the engine's own handler
/// returns, or a packet the env writes will not walk.
#[test]
fn every_env_verb_carries_the_measured_wire_size() {
    for v in g::UNIT_VERBS.iter().chain(g::PLAYER_VERBS.iter()) {
        let d = opdef(v.opcode);
        match d.wire {
            cb::WireLen::Fixed(n) => assert_eq!(
                n as usize, v.wire_size as usize,
                "{} (opcode {}) : handler returns {n}, env says {}",
                v.name, v.opcode, v.wire_size
            ),
            cb::WireLen::Variable => panic!(
                "{} (opcode {}) is variable-length; the env cannot emit a fixed-size one",
                v.name, v.opcode
            ),
        }
    }
}

/// The 49 verbs plus the four non-agent classes must partition all 82 opcodes exactly
/// once. This is the env's own claim; asserting it here keeps the two enumerations from
/// drifting apart silently.
#[test]
fn the_env_partition_covers_every_opcode_exactly_once() {
    let mut seen = [0u8; cb::NUM_OPCODES];
    for v in g::UNIT_VERBS.iter().chain(g::PLAYER_VERBS.iter()) {
        seen[v.opcode as usize] += 1;
    }
    for (op, _) in g::SELECTION_OPCODES
        .iter()
        .chain(g::UI_OPCODES.iter())
        .chain(g::ADMIN_OPCODES.iter())
        .chain(g::CHEAT_OPCODES.iter())
    {
        seen[*op as usize] += 1;
    }
    for (op, n) in seen.iter().enumerate() {
        assert_eq!(
            *n,
            1,
            "opcode {op} ({}) appears {n} times in the env partition",
            cb::OPCODES[op].name
        );
    }
}

/// **Disagreement, pinned.** 34 opcodes are handed to `groups.list[package.group]`, i.e.
/// they act on the current *selection*. `don-env` puts 31 of them on its unit head and the
/// other three on its player head, where there is no selection to act on.
///
/// `ALARM` (27) reaches `Group::action_alarm` `0x0070EC30`, which garrisons the selected
/// civilians; `UNITMASK` (32) and `BUILDMASK` (33) reach `Group::action_unitmask`
/// `0x006FCB90` / `action_buildmask` `0x006FC9A0`, which set per-group production masks.
/// All three take the group as `this` exactly the way `MOVE_TO` does [measured].
#[test]
fn three_group_scoped_opcodes_sit_on_the_env_player_head() {
    let group_ops: Vec<u8> = cb::OPCODES
        .iter()
        .filter(|d| d.is_group_action())
        .map(|d| d.op)
        .collect();
    assert_eq!(group_ops.len(), 35);

    let unit_ops: Vec<u8> = g::UNIT_VERBS.iter().map(|v| v.opcode).collect();
    let player_ops: Vec<u8> = g::PLAYER_VERBS.iter().map(|v| v.opcode).collect();

    let misplaced: Vec<u8> = group_ops
        .iter()
        .copied()
        .filter(|op| player_ops.contains(op))
        .collect();
    assert_eq!(
        misplaced,
        vec![27, 32, 33],
        "group-scoped opcodes on the player head changed; update the note in \
         docs/assembly/command-bridge.md"
    );

    // Everything else group-scoped except administrative BEGIN is on the unit head,
    // plus the two Unit-scoped ones.
    for op in group_ops
        .iter()
        .filter(|op| **op != 1 && !misplaced.contains(op))
    {
        assert!(
            unit_ops.contains(op),
            "opcode {op} ({}) is group-scoped but on no env unit head",
            cb::OPCODES[*op as usize].name
        );
    }
    for d in cb::OPCODES
        .iter()
        .filter(|d| d.receiver == cb::Receiver::Unit)
    {
        assert!(
            unit_ops.contains(&d.op),
            "{} is not an env unit verb",
            d.name
        );
    }
}

/// `don-env::action::apply_unit` now agrees with the measured allocation sites. The
/// `PATROL` command has a type-dependent branch: `Group::action_patrol` delegates true
/// planes to `action_air_patrol`, while ground units and helicopters take the group path.
/// `LAUNCH_PATROL` filters out every non-plane member instead of installing a ground
/// fallback.
///
/// | opcode | `Group::action_*` | `Unit::add_*_order` | `OrdersMemManager::get_obj` |
/// |---|---|---|---|
/// | 7 `MOVE_TO` | `action_move_to` → `action_move_near` | `add_move_facing_order` | `MOVE_TO` (or 2/3/4 by the `orders` byte) |
/// | 8 `MOVE_NEAR` | `action_move_near` | `add_move_facing_order` | same |
/// | 10 `PATROL` | `action_patrol` | `add_patrol_order` | **`GROUP_PATROL` (22)** |
/// | 11 `LAUNCH_PATROL` | `action_launch_patrol` | `add_air_patrol_order` | **`AIR_PATROL` (17)** |
#[test]
fn patrol_and_launch_patrol_are_not_move_to() {
    use don_env::action::patrol_order_for_opcode;

    assert_eq!(
        patrol_order_for_opcode(10, false),
        Some(g::OrderIndex::GroupPatrol)
    );
    assert_eq!(
        patrol_order_for_opcode(10, true),
        Some(g::OrderIndex::AirPatrol)
    );
    assert_eq!(
        patrol_order_for_opcode(11, true),
        Some(g::OrderIndex::AirPatrol)
    );
    assert_eq!(patrol_order_for_opcode(11, false), None);

    // What the bridge installs, driven through the real wire path.
    use don_sim::command::{build, Bridge, Fleet, ObjectTable, Package, QueuePos, Slot};

    let mut f = ObjectTable::new(2);
    f.put(1, 0, Slot::unit(1, 0, 0));
    f.put(1, 1, Slot::plane(2, 0, 0));
    let mut b = Bridge::new();
    let mut p = Package::new(1, 0);
    b.process_all(&mut p, &build::group(1, &[0]), &mut f)
        .unwrap();

    // PatrolCommand (10): op | i32 to_x | i32 to_y | i8 queued.
    let mut patrol = vec![10u8];
    patrol.extend_from_slice(&64i32.to_le_bytes());
    patrol.extend_from_slice(&96i32.to_le_bytes());
    patrol.push(QueuePos::New as u8);
    b.process_all(&mut p, &patrol, &mut f).unwrap();
    assert_eq!(
        f.orders(1, 0).unwrap().current().unwrap().kind,
        OrderIndex::GroupPatrol
    );

    // The same opcode delegates a true plane to AIR_PATROL.
    b.process_all(&mut p, &build::group(1, &[1]), &mut f)
        .unwrap();
    b.process_all(&mut p, &patrol, &mut f).unwrap();
    assert_eq!(
        f.orders(1, 1).unwrap().current().unwrap().kind,
        OrderIndex::AirPatrol
    );

    // LaunchPatrolCommand (11): op | i32 to_x | i32 to_y | i32 queued | shift | ctrl | alt.
    let mut launch = vec![11u8];
    launch.extend_from_slice(&64i32.to_le_bytes());
    launch.extend_from_slice(&96i32.to_le_bytes());
    launch.extend_from_slice(&(QueuePos::New as i32).to_le_bytes());
    launch.extend_from_slice(&[0u8; 12]);
    b.process_all(&mut p, &launch, &mut f).unwrap();
    assert_eq!(
        f.orders(1, 1).unwrap().current().unwrap().kind,
        OrderIndex::AirPatrol
    );

    // And `OrderIndex::Patrol` (5) is never what a patrol produces.
    assert!(cb::UNCONSTRUCTED_ORDERS.contains(&OrderIndex::Patrol));
}

/// Exercise the RL application and mask paths with the shipped type table. This catches
/// both tempting broad classifications: treating every AIR-domain unit as a plane (which
/// misroutes helicopters), and treating ordinary aircraft patrol as GROUP_PATROL.
#[test]
fn env_patrol_routing_uses_retail_is_plane() {
    use don_env::action::{apply_unit, ApplyStats, UnitAction};
    use don_env::mask::MaskWriter;
    use don_env::spec::get_bit;
    use don_env::state::{EnvWorld, Rules};
    use don_env::EnvConfig;

    let (rules, caps_real, _) = Rules::load(None, None);
    if !caps_real {
        eprintln!("SKIP: schema/live/env-typecaps.bin absent (run gen/gen_spec.py)");
        return;
    }
    let cfg = EnvConfig::default();
    let mut w = EnvWorld::new(rules, 8, 1, cfg.grid_w, cfg.grid_h);
    let citizen = w.spawn(0, 50, 0, 0).unwrap();
    let fighter = w.spawn(0, 289, 0, 0).unwrap();
    let helicopter = w.spawn(0, 310, 0, 0).unwrap();

    let apply = |w: &mut EnvWorld, h, verb| {
        let mut st = ApplyStats::default();
        apply_unit(
            w,
            &cfg,
            0,
            h,
            UnitAction {
                verb: (verb + 1) as u16,
                target_x: 3,
                target_y: 5,
                queue_pos: g::QueuePos::QueueNew as u16,
                ..Default::default()
            },
            &mut st,
        );
        st
    };

    let mut queued = ApplyStats::default();
    apply_unit(
        &mut w,
        &cfg,
        0,
        citizen,
        UnitAction {
            verb: (g::uv::PATROL + 1) as u16,
            target_x: 3,
            target_y: 5,
            queue_pos: g::QueuePos::QueueFirst as u16,
            ..Default::default()
        },
        &mut queued,
    );
    assert_eq!(queued.applied, 1);
    assert_eq!(
        w.order[w.sim.row_of(citizen).unwrap()],
        g::OrderIndex::GroupPatrol as u8,
        "retail normalizes GROUP_PATROL QUEUE_FIRST to its replacement path"
    );

    assert_eq!(apply(&mut w, citizen, g::uv::PATROL).applied, 1);
    assert_eq!(
        w.order[w.sim.row_of(citizen).unwrap()],
        g::OrderIndex::GroupPatrol as u8
    );
    assert_eq!(apply(&mut w, fighter, g::uv::PATROL).applied, 1);
    assert_eq!(
        w.order[w.sim.row_of(fighter).unwrap()],
        g::OrderIndex::AirPatrol as u8
    );
    assert_eq!(apply(&mut w, helicopter, g::uv::PATROL).applied, 1);
    assert_eq!(
        w.order[w.sim.row_of(helicopter).unwrap()],
        g::OrderIndex::GroupPatrol as u8
    );
    assert_eq!(apply(&mut w, fighter, g::uv::LAUNCH_PATROL).applied, 1);
    assert_eq!(
        w.order[w.sim.row_of(fighter).unwrap()],
        g::OrderIndex::AirPatrol as u8
    );
    assert_eq!(
        apply(&mut w, helicopter, g::uv::LAUNCH_PATROL).illegal,
        1,
        "retail ignores non-plane group members; the RL mask must not offer this pair"
    );

    let rows = vec![
        w.sim.row_of(citizen).unwrap(),
        w.sim.row_of(fighter).unwrap(),
        w.sim.row_of(helicopter).unwrap(),
    ];
    let mut masks = MaskWriter::new(&cfg);
    let rec = masks.unit.record_bytes;
    let mut out = vec![0; cfg.max_controlled * rec];
    masks.write_unit_masks(&w, &cfg, 0, &rows, &rows, &mut out);
    let verb_offset = masks.unit.offsets[g::UnitHead::Verb as usize];
    let verb_bytes = masks.unit.sizes[g::UnitHead::Verb as usize].div_ceil(8);
    let allows = |slot: usize, verb: usize| {
        let start = slot * rec + verb_offset;
        get_bit(&out[start..start + verb_bytes], verb + 1)
    };
    assert!(allows(0, g::uv::PATROL));
    assert!(!allows(0, g::uv::LAUNCH_PATROL));
    assert!(
        !allows(1, g::uv::PATROL),
        "ordinary VecEnv has no mandatory AIR_PATROL physics/search host"
    );
    assert!(!allows(1, g::uv::LAUNCH_PATROL));
    assert!(allows(2, g::uv::PATROL));
    assert!(!allows(2, g::uv::LAUNCH_PATROL));
}

#[test]
fn env_patrol_queue_and_executor_preserve_retail_transitions() {
    use don_env::spec::EnvConfig;
    use don_env::state::{AirPatrolHost, AirPatrolHostError, EnvWorld, Rules};
    use don_sim::command::QueuePos;
    use don_sim::order::OrderIndex;
    use don_sim::systems::order_dispatch::{AirPatrolSearch, PatrolPayload};
    use don_sim::systems::patrol::{AirPatrolOrder, AirPatrolTarget};

    struct ExplicitTestAirframe {
        ready: bool,
    }

    impl AirPatrolHost for ExplicitTestAirframe {
        fn preflight(&mut self, _world: &EnvWorld) -> Result<(), AirPatrolHostError> {
            if self.ready {
                Ok(())
            } else {
                Err(AirPatrolHostError::Unavailable(
                    don_env::state::AirPatrolHostBoundary::AirPhysics,
                ))
            }
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
            // This is an explicit test double, not a production movement fallback.
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
            Ok(None)
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

    let (rules, caps_real, _) = Rules::load(None, None);
    if !caps_real {
        eprintln!("SKIP: schema/live/env-typecaps.bin absent (run gen/gen_spec.py)");
        return;
    }
    let cfg = EnvConfig::default();
    let mut w = EnvWorld::new(rules, 8, 1, cfg.grid_w, cfg.grid_h);

    let ground = w.spawn(0, 50, 0, 0).unwrap();
    let grow = w.sim.row_of(ground).unwrap();
    w.install_group_patrol_order(grow, 48, 48, QueuePos::New)
        .unwrap();
    w.install_group_patrol_order(grow, 111, 222, QueuePos::Last)
        .unwrap();
    let group = match &w.orders[grow].front().unwrap().patrol_payload {
        PatrolPayload::Group(group) => group,
        other => panic!("expected concrete group-patrol body, got {other:?}"),
    };
    assert_eq!(group.points.len(), 3);
    assert_eq!(
        (group.points.x[2], group.points.y[2]),
        (111, 222),
        "QUEUE_LAST extension writes the raw command Coord"
    );
    w.install_group_patrol_order(grow, 48, 48, QueuePos::First)
        .unwrap();
    assert_eq!(w.orders[grow].len(), 1);
    let group = match &w.orders[grow].front().unwrap().patrol_payload {
        PatrolPayload::Group(group) => group,
        _ => unreachable!(),
    };
    assert_eq!(group.points.len(), 2, "QUEUE_FIRST follows replacement");

    w.frame();
    assert_eq!(w.orders[grow].len(), 2);
    let leg = w.orders[grow].front().unwrap();
    assert_eq!(leg.kind, OrderIndex::AttackTo);
    assert_eq!((leg.x, leg.y), (72, 72));
    assert_eq!(w.order[grow], OrderIndex::AttackTo as u8);
    let patrol = w.orders[grow].iter().nth(1).unwrap();
    let PatrolPayload::Group(group) = &patrol.patrol_payload else {
        unreachable!()
    };
    assert_eq!(group.points.waypoint, 1);

    let air = w.spawn(0, 289, 0, 0).unwrap();
    let arow = w.sim.row_of(air).unwrap();
    w.install_air_patrol_order(arow, 24, 24, QueuePos::New)
        .unwrap();
    w.install_air_patrol_order(arow, 900, 900, QueuePos::Last)
        .unwrap();
    assert_eq!(w.orders[arow].len(), 1);
    let PatrolPayload::Air(route) = &w.orders[arow].front().unwrap().patrol_payload else {
        unreachable!()
    };
    assert_eq!(route.points.len(), 2);

    let before = (w.sim.pos_x()[arow], w.sim.pos_y()[arow]);
    w.frame();
    let PatrolPayload::Air(route) = &w.orders[arow].front().unwrap().patrol_payload else {
        unreachable!()
    };
    assert_eq!(
        route.points.waypoint, 0,
        "AIR_PATROL must not advance through the straight-line movement scaffold"
    );
    assert_eq!((w.sim.pos_x()[arow], w.sim.pos_y()[arow]), before);
    assert!(
        w.unimplemented.unit[g::uv::PATROL] > 0,
        "the unavailable air host must remain observable"
    );

    let frame_before_preflight = w.sim.frame;
    assert_eq!(
        w.frame_with_air_patrol_host(&mut ExplicitTestAirframe { ready: false }),
        Err(AirPatrolHostError::Unavailable(
            don_env::state::AirPatrolHostBoundary::AirPhysics,
        ))
    );
    assert_eq!(
        w.sim.frame, frame_before_preflight,
        "preflight must reject before the owner-slot scheduler mutates the frame"
    );

    w.frame_with_air_patrol_host(&mut ExplicitTestAirframe { ready: true })
        .unwrap();
    let PatrolPayload::Air(route) = &w.orders[arow].front().unwrap().patrol_payload else {
        unreachable!()
    };
    assert_eq!(
        route.points.waypoint, 1,
        "the recovered post-physics transition advances only behind an explicit host"
    );
}

/// The env's `OrderIndex` and `QueuePos` enums must be numerically identical to
/// `don-sim`'s, or an action crossing the boundary changes meaning.
#[test]
fn the_shared_enums_have_identical_numbering() {
    assert_eq!(g::OrderIndex::MoveTo as u16, OrderIndex::MoveTo as u16);
    assert_eq!(g::OrderIndex::Attack as u16, OrderIndex::Attack as u16);
    assert_eq!(
        g::OrderIndex::AirPatrol as u16,
        OrderIndex::AirPatrol as u16
    );
    assert_eq!(
        g::OrderIndex::GroupPatrol as u16,
        OrderIndex::GroupPatrol as u16
    );
    assert_eq!(g::OrderIndex::Think as u16, OrderIndex::Think as u16);
    assert_eq!(g::QueuePos::QueueFirst as u8, cb::QueuePos::First as u8);
    assert_eq!(g::QueuePos::QueueLast as u8, cb::QueuePos::Last as u8);
    assert_eq!(g::QueuePos::QueueNew as u8, cb::QueuePos::New as u8);
}

/// **Disagreement, pinned.** The env classifies opcode 34 `HOTKEY` as SELECTION alongside
/// opcode 0 `GROUP`. Measured, only opcode 0 writes `CommandPackage::group`;
/// `process_hotkey` `0x009474D0` calls `HotKeyGroups::copy_group` `0x00715120` and never
/// touches the package's group field. `Group::action_hotkey` `0x006FA7A0` exists but its
/// only caller is `Console::on_key_down` — the local UI, not the wire.
#[test]
fn only_opcode_zero_is_really_a_selection_command() {
    let sel: Vec<u8> = g::SELECTION_OPCODES.iter().map(|(o, _)| *o).collect();
    assert_eq!(sel, vec![0, 34]);
    assert_eq!(opdef(0).receiver, cb::Receiver::None); // process_group writes `group` itself
    assert_eq!(opdef(34).action, None);
    assert_eq!(opdef(34).method, "process_hotkey");
    // action_hotkey is real, but off the wire.
    let h = cb::ActionDef::find("hotkey").unwrap();
    assert_eq!(h.va, 0x006FA7A0);
    assert_eq!(h.port, cb::Port::NotOnTheWire);
}

/// How much of the wire-reachable action surface the bridge reproduces, stated as a
/// number so it can only go up deliberately.
#[test]
fn the_ported_share_of_wire_reachable_actions_is_recorded() {
    let wire_reachable: Vec<&cb::ActionDef> = cb::GROUP_ACTIONS
        .iter()
        .filter(|a| a.port != cb::Port::NotOnTheWire)
        .collect();
    // 36, not 35: `return` left `NotOnTheWire` when the self-contained group-receiver lane
    // gave it a host. It is not dispatched directly by an opcode, but `recall` — which is
    // (opcode 35) — delegates to it, so it is genuinely wire-reachable. `hotkey` did NOT
    // move: its body is inlined into opcode 34's handler, which never dispatches the action.
    assert_eq!(wire_reachable.len(), 36);
    let ported = wire_reachable
        .iter()
        .filter(|a| {
            matches!(
                a.port,
                cb::Port::Complete | cb::Port::Orders | cb::Port::State | cb::Port::StateWired
            )
        })
        .count();
    // 36 for the same reason: `return` is now hosted and wire-reachable via `recall`.
    assert_eq!(
        ported, 36,
        "ported action count changed; update docs/assembly/command-bridge.md"
    );
}
