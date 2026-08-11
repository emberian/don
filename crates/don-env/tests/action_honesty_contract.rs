//! Adversarial contract between the emitted Verb masks and action application.
//!
//! The generated taxonomy deliberately retains every retail gameplay opcode. That does
//! not make every opcode executable in the ordinary environment: policy masks are the
//! narrower runtime contract and may advertise only verbs with an effectful body whose
//! mandatory host is present.

use std::sync::Arc;

use don_env::action::{apply_player, apply_unit, ApplyStats, PlayerAction, UnitAction};
use don_env::generated as g;
use don_env::mask::MaskWriter;
use don_env::spec::{get_bit, EnvConfig, MaskLayout};
use don_env::state::{EnvWorld, Rules};
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
    (world, actor)
}

fn first_legal(record: &[u8], layout: &MaskLayout, head: usize) -> u16 {
    let bytes = &record[layout.offsets[head]..];
    (0..layout.sizes[head])
        .find(|&value| get_bit(bytes, value))
        .expect("the mask writer promises a non-empty head") as u16
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
        stance: first_legal(record, &writer.unit, g::UnitHead::Stance as usize),
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

#[test]
fn every_advertised_unit_verb_applies_without_accepted_no_effect() {
    let Some(rules) = real_rules() else {
        return;
    };
    let cfg = EnvConfig {
        grid_w: 16,
        grid_h: 16,
        max_entities: 8,
        max_controlled: 2,
        ..Default::default()
    };
    let mut advertised = [false; g::N_UNIT_VERBS];

    // Exercise the full TypeIndex domain. This catches a capability flag whose mask gate
    // is looser than the corresponding application precondition, rather than validating
    // only the reset scenario's peasant and small-city rows.
    for actor_type in 0..g::NUM_TYPES as u16 {
        for (verb, action) in masked_unit_actions(rules.clone(), &cfg, actor_type) {
            advertised[verb] = true;
            let (mut world, actor) = fixture(rules.clone(), &cfg, actor_type);
            let mut stats = ApplyStats::default();
            apply_unit(&mut world, &cfg, 0, actor, action, &mut stats);
            assert_eq!(
                stats,
                ApplyStats {
                    applied: 1,
                    ..Default::default()
                },
                "masked unit verb {} rejected/no-op'd for TypeIndex {actor_type}",
                g::UNIT_VERBS[verb].name
            );
        }
    }

    let expected = [
        g::uv::STANCE,
        g::uv::ATTACK,
        g::uv::MOVE_TO,
        g::uv::MOVE_NEAR,
        g::uv::PATROL,
        g::uv::HALT,
        g::uv::DISBAND,
        g::uv::QUEUE_UP,
        g::uv::BUILD,
    ];
    for (verb, def) in g::UNIT_VERBS.iter().enumerate() {
        assert_eq!(
            advertised[verb],
            expected.contains(&verb),
            "unexpected ordinary-environment mask status for {}",
            def.name
        );
    }
    assert!(
        !advertised[g::uv::GATHER],
        "GATHER needs authoritative terrain/capacity, occupancy, evaluator, and payout hosts"
    );
    assert!(
        !advertised[g::uv::LAUNCH_PATROL],
        "AIR_PATROL needs the mandatory air-physics/type/target-search host"
    );
    assert!(
        !advertised[g::uv::FORM],
        "Group::action_form 0x00707220 always delegates to action_move_near, whose \
         installed destination is still a UCoord cell index rather than the centred Coord \
         Unit::add_move_facing_order 0x005E55C0 stores"
    );
    assert!(
        !advertised[g::uv::SIEGE_ATTACK] && !advertised[g::uv::SWARM_AROUND],
        "both receivers are OpenActionTail frontier opcodes in don-sim and neither has an \
         add_*_order row; advertising them as ordinary ATTACK is a substitution"
    );
}

#[test]
fn every_advertised_player_verb_applies_without_accepted_no_effect() {
    let Some(rules) = real_rules() else {
        return;
    };
    let cfg = EnvConfig::default();
    let world = EnvWorld::new(rules.clone(), 4, 7, cfg.grid_w, cfg.grid_h);
    let writer = MaskWriter::new(&cfg);
    let mut out = vec![0; writer.player.record_bytes];
    writer.write_player_mask(&world, 0, &mut out);
    let base = PlayerAction {
        target_player: first_legal(&out, &writer.player, g::PlayerHead::TargetPlayer as usize),
        good: first_legal(&out, &writer.player, g::PlayerHead::Good as usize),
        amount: first_legal(&out, &writer.player, g::PlayerHead::Amount as usize),
        treaty: first_legal(&out, &writer.player, g::PlayerHead::Treaty as usize),
        ..Default::default()
    };
    let verb_bytes = &out[writer.player.offsets[g::PlayerHead::Verb as usize]..];
    let mut advertised = [false; g::N_PLAYER_VERBS];
    for verb in 0..g::N_PLAYER_VERBS {
        if !get_bit(verb_bytes, verb + 1) {
            continue;
        }
        advertised[verb] = true;
        let mut fresh = EnvWorld::new(rules.clone(), 4, 7, cfg.grid_w, cfg.grid_h);
        let mut stats = ApplyStats::default();
        apply_player(
            &mut fresh,
            0,
            PlayerAction {
                verb: (verb + 1) as u16,
                ..base
            },
            &mut stats,
        );
        assert_eq!(
            stats,
            ApplyStats {
                applied: 1,
                ..Default::default()
            },
            "masked player verb {} rejected/no-op'd",
            g::PLAYER_VERBS[verb].name
        );
    }

    let expected = [g::pv::TREATY, g::pv::DECLARE, g::pv::TRIBUTE, g::pv::RESIGN];
    for (verb, def) in g::PLAYER_VERBS.iter().enumerate() {
        assert_eq!(
            advertised[verb],
            expected.contains(&verb),
            "unexpected ordinary-environment mask status for {}",
            def.name
        );
    }
}

#[test]
fn deterministic_masked_rollout_has_zero_accepted_no_effect() {
    let cfg = EnvConfig {
        grid_w: 24,
        grid_h: 24,
        max_entities: 32,
        max_controlled: 16,
        num_agents: 2,
        start_units: 8,
        max_steps: 0,
        seed: 0x51A7_E,
        ..Default::default()
    };
    let mut env = don_env::VecEnv::new(8, cfg, None, None, 4).unwrap();
    if env
        .provenance()
        .iter()
        .any(|(key, value)| key == "typecaps" && value.starts_with("ABSENT"))
    {
        eprintln!("SKIP: schema/live/env-typecaps.bin absent (run gen/gen_spec.py)");
        return;
    }
    for _ in 0..128 {
        env.sample_masked();
        let unit = env.sampled_unit_actions().to_vec();
        let player = env.sampled_player_actions().to_vec();
        env.step(&unit, &player);
    }
    assert_eq!(env.apply_stats.accepted_no_effect, 0);
    assert_eq!(env.apply_stats.illegal, 0);
    assert!(
        env.apply_stats.applied > 0,
        "rollout must not pass vacuously"
    );
}
