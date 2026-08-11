//! Mechanical enforcement of `mask::MaskWriter`'s invariants 1 and 4.
//!
//! `action_honesty_contract.rs` already asserts that every advertised verb reports
//! `applied` rather than `accepted_no_effect`. That is not enough: an arm can report
//! `applied` and write nothing the engine can see. FORM did exactly that — it wrote the
//! environment's private `form` mirror, left the walked `UnitData::form` at its
//! `Unit::init` `0x00612100` value, and installed no order — so a policy was trained on a
//! formation that existed only in the observation buffer.
//!
//! Two gates close that hole, and both are able to fail:
//!
//! 1. **Effect.** Applying an advertised verb must move authoritative state: the walked
//!    `don-sim` unit image (`World::digest`), the live row count, the executable order
//!    queues, or the leader economy. A private mirror write alone is not an effect.
//! 2. **Coherence.** After any advertised verb, every environment column that mirrors a
//!    walked `don-sim` field must still equal that field.
//!
//! The fixture gives the actor one queued `MOVE_TO` before acting, because retail HALT on
//! an idle unit is genuinely a no-op and the gate must not be satisfied by accident.

use std::sync::Arc;

use don_env::action::{apply_unit, ApplyStats, UnitAction};
use don_env::generated as g;
use don_env::mask::MaskWriter;
use don_env::spec::{get_bit, EnvConfig, MaskLayout};
use don_env::state::{EnvWorld, Rules};
use don_sim::command::QueuePos;
use don_sim::systems::order_dispatch::OrderRec;
use don_sim::world::SUBTILE;

fn real_rules() -> Option<Arc<Rules>> {
    let (rules, real, _) = Rules::load(None, None);
    if !real {
        eprintln!("SKIP: schema/live/env-typecaps.bin absent (run gen/gen_spec.py)");
        None
    } else {
        Some(rules)
    }
}

fn cfg() -> EnvConfig {
    EnvConfig {
        grid_w: 16,
        grid_h: 16,
        max_entities: 8,
        max_controlled: 2,
        ..Default::default()
    }
}

/// Actor, one friendly and one hostile, with a queued ordinary move on the actor.
fn fixture(rules: Arc<Rules>, cfg: &EnvConfig, actor_type: u16) -> (EnvWorld, don_sim::Handle) {
    let mut world = EnvWorld::new(rules, 16, 0xA11C_E57, cfg.grid_w, cfg.grid_h);
    let actor = world
        .spawn(0, actor_type, 4 * SUBTILE, 4 * SUBTILE)
        .unwrap();
    let friendly = world
        .spawn(0, g::UNIT_TYPE_BASE as u16, 5 * SUBTILE, 4 * SUBTILE)
        .unwrap();
    let hostile = world
        .spawn(1, g::UNIT_TYPE_BASE as u16, 6 * SUBTILE, 4 * SUBTILE)
        .unwrap();
    world.obs_ents[0] = vec![actor, friendly, hostile];
    let row = world.sim.row_of(actor).unwrap();
    // A pre-existing order so that a verb whose only effect is retiring the queue (HALT)
    // has something authoritative to retire.
    world
        .install_order(
            row,
            OrderRec::move_to(7 * SUBTILE, 4 * SUBTILE, 0),
            QueuePos::New,
        )
        .expect("the fixture actor accepts one ordinary move");
    (world, actor)
}

fn legal_values(record: &[u8], layout: &MaskLayout, head: usize) -> Vec<u16> {
    let bytes = &record[layout.offsets[head]..];
    (0..layout.sizes[head])
        .filter(|&value| get_bit(bytes, value))
        .map(|value| value as u16)
        .collect()
}

fn first_legal(record: &[u8], layout: &MaskLayout, head: usize) -> u16 {
    *legal_values(record, layout, head)
        .first()
        .expect("the mask writer promises a non-empty head")
}

/// The last legal value, used for Stance so the probe cannot pass by writing the value the
/// actor already carries from `spawn`.
fn last_legal(record: &[u8], layout: &MaskLayout, head: usize) -> u16 {
    *legal_values(record, layout, head)
        .last()
        .expect("the mask writer promises a non-empty head")
}

fn masked_unit_actions(
    rules: Arc<Rules>,
    cfg: &EnvConfig,
    actor_type: u16,
) -> Vec<(usize, UnitAction)> {
    let (world, actor) = fixture(rules, cfg, actor_type);
    let row = world.sim.row_of(actor).unwrap();
    let entity_rows = world.obs_ents[0]
        .iter()
        .map(|handle| world.sim.row_of(*handle).unwrap())
        .collect::<Vec<_>>();
    let mut writer = MaskWriter::new(cfg);
    let mut out = vec![0; cfg.max_controlled * writer.unit.record_bytes];
    writer.write_unit_masks(&world, cfg, 0, &[row], &entity_rows, &mut out);
    let record = &out[..writer.unit.record_bytes];

    let base = UnitAction {
        target_x: first_legal(record, &writer.unit, g::UnitHead::TargetX as usize),
        target_y: first_legal(record, &writer.unit, g::UnitHead::TargetY as usize),
        target_entity: first_legal(record, &writer.unit, g::UnitHead::TargetEntity as usize),
        type_index: first_legal(record, &writer.unit, g::UnitHead::Type as usize),
        queue_pos: first_legal(record, &writer.unit, g::UnitHead::QueuePos as usize),
        stance: last_legal(record, &writer.unit, g::UnitHead::Stance as usize),
        form: first_legal(record, &writer.unit, g::UnitHead::Form as usize),
        order_mods: first_legal(record, &writer.unit, g::UnitHead::OrderMods as usize),
        count: first_legal(record, &writer.unit, g::UnitHead::Count as usize),
        ..Default::default()
    };
    let verb_bytes = &record[writer.unit.offsets[g::UnitHead::Verb as usize]..];
    (0..g::N_UNIT_VERBS)
        .filter(|&verb| get_bit(verb_bytes, verb + 1))
        .map(|verb| {
            (
                verb,
                UnitAction {
                    verb: (verb + 1) as u16,
                    ..base
                },
            )
        })
        .collect()
}

/// Everything an action is allowed to count as an effect. Deliberately excludes the
/// environment's private mirror columns (`form`, `stance`, `order`, `dest_*`, `target`).
#[derive(Clone, PartialEq, Eq, Debug)]
struct AuthoritativeProbe {
    /// `World::digest` over every walked `UnitData` field of every live row.
    unit_image: u64,
    live_rows: u32,
    /// `UnitData::orderlist`: node counts and the current node's `OrderIndex`, per row.
    order_queues: Vec<(usize, u8)>,
    /// `LeaderData` stockpile, population and the outcome counters rewards read.
    leaders: Vec<(i32, i32, i32, i32, i32)>,
}

fn probe(world: &EnvWorld) -> AuthoritativeProbe {
    let n = world.sim.live_count() as usize;
    AuthoritativeProbe {
        unit_image: world.sim.digest(),
        live_rows: world.sim.live_count(),
        order_queues: (0..n)
            .map(|row| {
                let queue = &world.orders[row];
                (
                    queue.len(),
                    queue.front().map_or(0, |order| order.kind as u8),
                )
            })
            .collect(),
        leaders: world
            .players
            .iter()
            .map(|player| {
                (
                    player.econ.iter().sum::<i32>(),
                    player.pop,
                    player.units_built,
                    player.buildings_built,
                    i32::from(player.alive),
                )
            })
            .collect(),
    }
}

/// Invariant 4: the observation columns that mirror a walked field must not drift from it.
fn assert_mirrors_agree(world: &EnvWorld, context: &str) {
    for row in 0..world.sim.live_count() as usize {
        assert_eq!(
            world.form[row] as i8,
            world.sim.units.form()[row],
            "{context}: env form mirror diverged from UnitData::form at row {row}"
        );
        assert_eq!(
            i32::from(world.stance[row]),
            i32::from(world.sim.units.stance()[row]),
            "{context}: env stance mirror diverged from UnitData::stance at row {row}"
        );
    }
}

#[test]
fn every_advertised_unit_verb_moves_authoritative_state() {
    let Some(rules) = real_rules() else {
        return;
    };
    let cfg = cfg();
    let mut exercised = [false; g::N_UNIT_VERBS];

    for actor_type in 0..g::NUM_TYPES as u16 {
        for (verb, action) in masked_unit_actions(rules.clone(), &cfg, actor_type) {
            let (mut world, actor) = fixture(rules.clone(), &cfg, actor_type);
            let before = probe(&world);
            let mut stats = ApplyStats::default();
            apply_unit(&mut world, &cfg, 0, actor, action, &mut stats);
            let after = probe(&world);
            assert_eq!(
                stats,
                ApplyStats {
                    applied: 1,
                    ..Default::default()
                },
                "advertised verb {} was not applied for TypeIndex {actor_type}",
                g::UNIT_VERBS[verb].name
            );
            assert_ne!(
                before,
                after,
                "advertised verb {} reported applied but moved no authoritative state for \
                 TypeIndex {actor_type}",
                g::UNIT_VERBS[verb].name
            );
            exercised[verb] = true;
        }
    }

    assert!(
        exercised.iter().any(|seen| *seen),
        "the sweep must exercise at least one advertised verb"
    );
}

#[test]
fn no_advertised_unit_verb_leaves_a_mirror_column_lying() {
    let Some(rules) = real_rules() else {
        return;
    };
    let cfg = cfg();

    for actor_type in 0..g::NUM_TYPES as u16 {
        for (verb, action) in masked_unit_actions(rules.clone(), &cfg, actor_type) {
            let (mut world, actor) = fixture(rules.clone(), &cfg, actor_type);
            assert_mirrors_agree(&world, "fixture");
            let mut stats = ApplyStats::default();
            apply_unit(&mut world, &cfg, 0, actor, action, &mut stats);
            assert_mirrors_agree(
                &world,
                &format!(
                    "after {} on TypeIndex {actor_type}",
                    g::UNIT_VERBS[verb].name
                ),
            );
        }
    }
}

/// The gate above is only meaningful if a mirror-only write is actually detectable. This
/// reproduces the exact defect FORM used to have and shows the checker rejects it.
#[test]
fn the_mirror_gate_rejects_a_mirror_only_write() {
    let Some(rules) = real_rules() else {
        return;
    };
    let cfg = cfg();
    let (mut world, actor) = fixture(rules, &cfg, g::UNIT_TYPE_BASE as u16);
    let row = world.sim.row_of(actor).unwrap();
    let authoritative = world.sim.units.form()[row];
    world.form[row] = (authoritative as u8).wrapping_add(1);
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        assert_mirrors_agree(&world, "injected");
    }))
    .is_err();
    assert!(
        caught,
        "a form write that never reached UnitData::form must fail the coherence gate"
    );
}
