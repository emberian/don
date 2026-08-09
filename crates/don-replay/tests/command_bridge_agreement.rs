//! Three crates independently describe the same 82 opcodes. This asserts they agree.
//!
//! * `don-net::opcodes::COMMAND_SIZES` + `Command::wire_len` — the decoder that walks a
//!   real `.rcx` payload.
//! * `don-replay::wire::{COMMAND_SIZEOF, COMMAND_STRUCT, COMMAND_METHOD}` — generated from
//!   `schema/command-wire.json`, i.e. from the PDB *type* stream.
//! * `don-sim::command::OPCODES` — generated from the PDB type stream **and** from the
//!   disassembled `CommandPackage::process_*` handlers, whose return value is the length
//!   the engine itself advances by.
//!
//! The third is the interesting one: it is the only table derived from code rather than
//! from types, so agreement here is a real cross-check rather than two views of one file.
//! Written by the `command-bridge` lane; this file is additive and owns no existing path.

use don_sim::command::{self as cb, WireLen};

/// Every opcode: the struct name and handler name must be the same string in all three.
#[test]
fn names_agree_across_don_net_don_replay_and_don_sim() {
    assert_eq!(cb::NUM_OPCODES, don_replay::wire::NUM_OPCODES);
    assert_eq!(cb::NUM_OPCODES, don_net::COMMAND_SIZES.len());
    for d in cb::OPCODES.iter() {
        let i = d.op as usize;
        assert_eq!(
            d.name,
            don_replay::wire::COMMAND_STRUCT[i],
            "opcode {i} struct name"
        );
        assert_eq!(
            d.method,
            don_replay::wire::COMMAND_METHOD[i],
            "opcode {i} handler name"
        );
        assert_eq!(d.name, don_net::COMMAND_STRUCTS[i], "opcode {i} vs don-net");
    }
}

/// The measured `process_*` return value equals the PDB `sizeof` for every fixed-length
/// opcode, and `don-net`'s decoder table carries the same number.
#[test]
fn fixed_lengths_agree_with_the_pdb_sizeof_and_with_don_nets_decoder() {
    for d in cb::OPCODES.iter() {
        let i = d.op as usize;
        match d.wire {
            WireLen::Fixed(n) => {
                assert_eq!(
                    n,
                    don_replay::wire::COMMAND_SIZEOF[i],
                    "opcode {i} {} : process_* returns {n}, PDB sizeof says {}",
                    d.name,
                    don_replay::wire::COMMAND_SIZEOF[i]
                );
                assert_eq!(
                    don_net::COMMAND_SIZES[i],
                    Some(n),
                    "opcode {i} {} : don-net decoder table",
                    d.name
                );
            }
            WireLen::Variable => {
                assert_eq!(
                    don_net::COMMAND_SIZES[i],
                    None,
                    "opcode {i} {} : don-net must treat this as variable",
                    d.name
                );
            }
        }
    }
}

/// The variable-length trio, agreed byte for byte against `don-net`'s own decoder over
/// the whole range each handler accepts.
#[test]
fn variable_lengths_agree_with_don_nets_decoder() {
    for num in 0u8..=64 {
        let b = [0u8, num, 1];
        assert_eq!(
            cb::wire_len(&b).unwrap(),
            don_net::Command::wire_len(&b).unwrap(),
            "GroupCommand num={num}"
        );
    }
    for len in 0u16..=64 {
        let mut b = vec![51u8, 0, 0, 0, 0, 0];
        b[4..6].copy_from_slice(&len.to_le_bytes());
        assert_eq!(
            cb::wire_len(&b).unwrap(),
            don_net::Command::wire_len(&b).unwrap(),
            "SplineCommand len={len}"
        );
    }
    for len in 0i32..=64 {
        let mut b = vec![0u8; 19];
        b[0] = 68;
        b[13..17].copy_from_slice(&len.to_le_bytes());
        assert_eq!(
            cb::wire_len(&b).unwrap(),
            don_net::Command::wire_len(&b).unwrap(),
            "ChatCommand len={len}"
        );
    }
}

/// A real payload walks identically under both length functions.
#[test]
fn a_packet_walks_to_the_same_boundaries_under_both_decoders() {
    use don_sim::command::{build, QueuePos};
    let mut payload = build::group(1, &[3, 4, 5]);
    payload.extend_from_slice(&build::move_to(1200, 800, QueuePos::New, 0));
    payload.extend_from_slice(&build::attack(9, 2, QueuePos::Last));
    payload.extend_from_slice(&build::halt());
    payload.extend_from_slice(&build::stance(3));

    let mut ours = Vec::new();
    let mut i = 0;
    while i < payload.len() {
        let l = cb::wire_len(&payload[i..]).unwrap();
        ours.push((payload[i], i, l));
        i += l;
    }
    assert_eq!(i, payload.len(), "our walk left residue");

    let theirs: Vec<(u8, usize, usize)> = {
        let mut v = Vec::new();
        let mut j = 0;
        while j < payload.len() {
            let l = don_net::Command::wire_len(&payload[j..]).unwrap();
            v.push((payload[j], j, l));
            j += l;
        }
        assert_eq!(j, payload.len(), "don-net's walk left residue");
        v
    };
    assert_eq!(ours, theirs);
    assert_eq!(ours.len(), 5);
}

/// `don-replay`'s `wire::Order` decodes the same fields the bridge reads, for the four
/// opcodes it models. This is the seam where a field-offset drift would show up.
#[test]
fn don_replays_order_decode_reads_the_same_fields_the_bridge_reads() {
    use don_replay::wire::{CommandView, Order};
    use don_sim::command::{build, QueuePos};

    let mv = build::move_to(-1234, 5678, QueuePos::Last, 0);
    let v = CommandView::new(7, &mv);
    assert_eq!(v.get("to_x"), Some(-1234));
    assert_eq!(v.get("to_y"), Some(5678));
    assert!(matches!(
        Order::decode(&v),
        Order::MoveTo {
            to_x: -1234,
            to_y: 5678,
            queued: 1
        }
    ));

    let mn = build::move_near(7, 9, 48, QueuePos::New, 0);
    let v = CommandView::new(8, &mn);
    assert!(matches!(
        Order::decode(&v),
        Order::MoveNear {
            to_x: 7,
            to_y: 9,
            tolerance: 48
        }
    ));

    let at = build::attack(11, 3, QueuePos::First);
    let v = CommandView::new(4, &at);
    assert!(matches!(
        Order::decode(&v),
        Order::Attack {
            ox: 11,
            whom: 3,
            ignore: 0
        }
    ));

    let st = build::stance(2);
    let v = CommandView::new(2, &st);
    assert!(matches!(Order::decode(&v), Order::Stance { stance: 2 }));
}

/// End to end: decode a payload with `don-net`, run it through the bridge, and check that
/// a unit ends up holding the order the command asked for.
///
/// This is the shortest path that exercises all three layers — wire framing, opcode
/// dispatch, and order installation — in one call.
#[test]
fn a_don_net_decoded_stream_makes_a_unit_hold_an_order() {
    use don_net::{decode_commands, Obfuscation};
    use don_sim::command::{build, Bridge, Fleet, ObjectTable, Package, QueuePos, Slot};
    use don_sim::order::OrderIndex;

    let mut payload = build::group(1, &[0, 1]);
    payload.extend_from_slice(&build::move_to(4096, 2048, QueuePos::New, 0));
    payload.extend_from_slice(&build::attack(6, 2, QueuePos::Last));

    let cmds = decode_commands(&payload, &mut Obfuscation::none()).expect("don-net decode");
    assert_eq!(cmds.len(), 3);

    let mut fleet = ObjectTable::new(4);
    fleet.put(1, 0, Slot::unit(10, 0, 0));
    fleet.put(1, 1, Slot::unit(11, 32, 0));

    let mut bridge = Bridge::new();
    let mut pkg = Package::new(1, 0);
    for c in &cmds {
        bridge.process_one(&mut pkg, c.bytes, &mut fleet);
    }

    assert!(pkg.group >= 0, "the selection did not intern a group");
    assert_eq!(bridge.stats.orders_installed, 4);
    for o in [0i16, 1] {
        let l = fleet.orders(1, o).unwrap();
        assert_eq!(l.len(), 2);
        let kinds: Vec<OrderIndex> = l.iter().map(|x| x.kind).collect();
        assert_eq!(kinds, vec![OrderIndex::MoveTo, OrderIndex::Attack]);
        assert_eq!(
            (l.current().unwrap().x, l.current().unwrap().y),
            (4096, 2048)
        );
    }
}
