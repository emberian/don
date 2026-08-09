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
    assert_eq!(group_ops.len(), 34);

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

    // Everything else group-scoped is on the unit head, plus the two Unit-scoped ones.
    for op in group_ops.iter().filter(|op| !misplaced.contains(op)) {
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

/// **Disagreement, pinned.** `don-env::action::apply_unit` routes `MOVE_TO`, `MOVE_NEAR`,
/// `PATROL` and `LAUNCH_PATROL` to one arm that installs `OrderIndex::MoveTo`.
/// `COVERAGE.md` §3.1 already records that as wrong; the bridge now says what the right
/// answer is, measured from the allocation site rather than argued:
///
/// | opcode | `Group::action_*` | `Unit::add_*_order` | `OrdersMemManager::get_obj` |
/// |---|---|---|---|
/// | 7 `MOVE_TO` | `action_move_to` → `action_move_near` | `add_move_facing_order` | `MOVE_TO` (or 2/3/4 by the `orders` byte) |
/// | 8 `MOVE_NEAR` | `action_move_near` | `add_move_facing_order` | same |
/// | 10 `PATROL` | `action_patrol` | `add_patrol_order` | **`GROUP_PATROL` (22)** |
/// | 11 `LAUNCH_PATROL` | `action_launch_patrol` | `add_air_patrol_order` | **`AIR_PATROL` (17)** |
#[test]
fn patrol_and_launch_patrol_are_not_move_to() {
    // What the bridge installs, driven through the real wire path.
    use don_sim::command::{build, Bridge, Fleet, ObjectTable, Package, QueuePos, Slot};

    let mut f = ObjectTable::new(2);
    f.put(1, 0, Slot::unit(1, 0, 0));
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

    // LaunchPatrolCommand (11): op | i32 to_x | i32 to_y | i32 queued | shift | ctrl | alt.
    let mut launch = vec![11u8];
    launch.extend_from_slice(&64i32.to_le_bytes());
    launch.extend_from_slice(&96i32.to_le_bytes());
    launch.extend_from_slice(&(QueuePos::New as i32).to_le_bytes());
    launch.extend_from_slice(&[0u8; 12]);
    b.process_all(&mut p, &launch, &mut f).unwrap();
    assert_eq!(
        f.orders(1, 0).unwrap().current().unwrap().kind,
        OrderIndex::AirPatrol
    );

    // And `OrderIndex::Patrol` (5) is never what a patrol produces.
    assert!(cb::UNCONSTRUCTED_ORDERS.contains(&OrderIndex::Patrol));
}

/// The env's `OrderIndex` and `QueuePos` enums must be numerically identical to
/// `don-sim`'s, or an action crossing the boundary changes meaning.
#[test]
fn the_shared_enums_have_identical_numbering() {
    assert_eq!(g::OrderIndex::MoveTo as u16, OrderIndex::MoveTo as u16);
    assert_eq!(g::OrderIndex::Attack as u16, OrderIndex::Attack as u16);
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
    assert_eq!(wire_reachable.len(), 34);
    let ported = wire_reachable
        .iter()
        .filter(|a| matches!(a.port, cb::Port::Orders | cb::Port::State))
        .count();
    assert_eq!(
        ported, 17,
        "ported action count changed; update docs/assembly/command-bridge.md"
    );
}
