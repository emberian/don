//! Machine-readable static closure inventory, sourced from the executable tables.

use don_replay::checksum::{CHANNELS, CHANNEL_NAMES, NUM_WALKED};
use don_replay::state::{SimBridge, CHANNEL_ELEMENT_CLASS, CHANNEL_WALKER_SYMBOL};
use don_replay::wire::{classify, CommandClass};
use don_sim::command::{InlineDef, InlinePort, Port, Receiver, WireLen, GROUP_ACTIONS, OPCODES};
use don_sim::deviations::{ModeConfig, Surface};
use don_sim::order::{ArmStatus, EXECUTORS};
use don_sim::schedule::{StepStatus, DO_FRAME};
use std::collections::BTreeSet;

fn clean(s: &str) -> String {
    s.replace(['\t', '\n', '\r'], " ")
}

fn inline_status(port: InlinePort) -> &'static str {
    match port {
        InlinePort::Complete => "complete",
        InlinePort::StateWired => "state_wired",
    }
}

fn main() {
    println!("META\tdon.simulation-closure.static.v1");
    for x in DO_FRAME {
        let status = match x.status {
            StepStatus::Implemented => "implemented",
            StepStatus::Stub => "stub",
            StepStatus::OutOfScope => "out_of_scope",
        };
        println!(
            "TICK\t{}\t{}\t{}\t{}\t{}\t{}",
            x.idx,
            clean(x.name),
            x.va.unwrap_or(""),
            clean(x.source),
            status,
            clean(x.note)
        );
    }
    for x in EXECUTORS {
        let status = match x.status {
            ArmStatus::Implemented => "implemented",
            ArmStatus::FaithfullyEmpty => "faithfully_empty",
            ArmStatus::Unimplemented => "unimplemented",
        };
        println!(
            "ORDER\t{}\t{}\t{}\t{}\t{}\t{}",
            x.order.index(),
            x.order.name(),
            x.va.unwrap_or(""),
            clean(x.symbol),
            status,
            clean(x.note)
        );
    }
    for x in GROUP_ACTIONS {
        let status = match x.port {
            Port::Complete => "complete",
            Port::Orders => "orders_partial",
            Port::State => "state_partial",
            Port::StateWired => "state_wired",
            Port::Todo => "unimplemented",
            Port::NotOnTheWire => "not_on_wire",
        };
        println!(
            "ACTION\t{}\t0x{:08x}\t{}\t{}\t{}\t{}\t{}",
            x.name,
            x.va,
            x.size,
            x.call_sites,
            x.installs
                .iter()
                .map(|o| o.name())
                .collect::<Vec<_>>()
                .join(","),
            x.delegates.join(","),
            status
        );
    }
    for x in OPCODES {
        let class = match classify(x.op) {
            CommandClass::Sim => "sim",
            CommandClass::Lockstep => "lockstep",
            CommandClass::Presentation => "presentation",
        };
        let receiver = match x.receiver {
            Receiver::Group => "group",
            Receiver::Leader => "leader",
            Receiver::Unit => "unit",
            Receiver::Game => "game",
            Receiver::None => "none",
        };
        let bridge = if x.op == 0 {
            "selection_partial"
        } else if let Some(inline) = InlineDef::find(x.op) {
            inline_status(inline.port)
        } else if let Some(a) = x
            .action
            .and_then(|name| GROUP_ACTIONS.iter().find(|a| a.name == name))
        {
            match a.port {
                Port::Complete => "complete",
                Port::Orders => "orders_partial",
                Port::State => "state_partial",
                Port::StateWired => "state_wired",
                Port::Todo => "unimplemented",
                Port::NotOnTheWire => "not_on_wire",
            }
        } else {
            "inert"
        };
        let wire = match x.wire {
            WireLen::Fixed(n) => n.to_string(),
            WireLen::Variable => "variable".to_string(),
        };
        println!(
            "OPCODE\t{}\t{}\t{}\t0x{:08x}\t{}\t{}\t{}\t{}\t{}",
            x.op,
            x.name,
            x.method,
            x.method_va,
            receiver,
            x.action.unwrap_or(""),
            wire,
            class,
            bridge
        );
    }
    for i in 0..NUM_WALKED {
        let source = if SimBridge::PRODUCES.contains(&CHANNELS[i]) {
            "sim_bridge"
        } else if CHANNEL_NAMES[i] == "rules" {
            "replay_initial_only"
        } else {
            "absent"
        };
        println!(
            "CHECKSUM\t{}\t{}\t{}\t{}\t{}",
            i,
            CHANNEL_NAMES[i],
            clean(CHANNEL_WALKER_SYMBOL[i]),
            CHANNEL_ELEMENT_CLASS[i].unwrap_or(""),
            source
        );
    }
    let mut seen = BTreeSet::new();
    for cfg in [ModeConfig::fidelity(), ModeConfig::improved()] {
        for blocker in cfg.readiness_blockers(Surface::ProductRelease) {
            let d = blocker.deviation();
            if seen.insert(d.slug()) {
                let e = d.entry();
                println!(
                    "BLOCKER\t{}\t{}\t{:?}\t{}\t{}",
                    e.slug,
                    clean(e.title),
                    e.implementation,
                    clean(e.seam),
                    clean(&e.derived_from.join("; "))
                );
            }
        }
    }
    println!(
        "COUNTS\t{}\t{}\t{}\t{}\t{}\t{}",
        DO_FRAME.len(),
        EXECUTORS.len(),
        GROUP_ACTIONS.len(),
        OPCODES.len(),
        NUM_WALKED,
        seen.len()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_inventory_cardinalities() {
        assert_eq!((DO_FRAME.len(), EXECUTORS.len()), (29, 28));
        assert_eq!((GROUP_ACTIONS.len(), OPCODES.len()), (42, 82));
        assert_eq!(don_sim::command::INLINE_COMMANDS.len(), 26);
        assert_eq!(NUM_WALKED, 15);
    }

    #[test]
    fn every_group_opcode_resolves() {
        for op in OPCODES
            .iter()
            .filter(|x| matches!(x.receiver, Receiver::Group))
        {
            let action = op.action.expect("group opcode must name its action");
            assert!(GROUP_ACTIONS.iter().any(|x| x.name == action), "{action}");
        }
    }

    #[test]
    fn inline_command_statuses_preserve_complete_vs_state_wired() {
        for op in [
            34, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 65, 66, 67, 68, 69, 72,
            74, 79, 81,
        ] {
            assert_eq!(inline_status(InlineDef::find(op).unwrap().port), "complete");
        }
        assert_eq!(
            inline_status(InlineDef::find(76).unwrap().port),
            "state_wired"
        );
    }
}
