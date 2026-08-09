//! Reinforcement-learning environment surface over the Descent of Nations simulation.
//!
//! # What this crate is
//!
//! Two deliberately different environment surfaces whose action and observation spaces are
//! read out of the engine rather than invented:
//!
//! * [`VecEnv`] is the existing factored, masked, vectorised compact backend. It remains the
//!   throughput-oriented default and keeps its existing API;
//! * [`AuthoritativeBackend`] is an opt-in, single-episode fidelity backend whose sole mutable
//!   game-state owner is `don_sim::tick::Sim`. Its admitted action surface is intentionally
//!   narrow and fail-closed.
//!
//! [`EnvironmentBackend`] makes that ownership choice explicit without inventing a lowest-
//! common-denominator `step` method between incompatible fidelity tiers.
//!
//! * the action space is the 82 `CommandTypes` opcodes and the 28 `OrderIndex` order kinds
//!   (`schema/command-wire.json`, the PDB type stream) refactored into ten independently
//!   masked heads — see [`action`];
//! * the observation space is chosen from what the engine itself checksums as sim-critical
//!   (`schema/state-schema.json`), and the entity feature columns carry the engine's own
//!   field names — see [`spec::ENTITY_FEATURES`];
//! * the reward exposes the eleven `LeaderData` score fields as a term vector and lets the
//!   trainer supply the weights — see [`reward`];
//! * capability masks come from the shipped `unitrules.xml` / `buildingrules.xml`, keyed
//!   to `TypeIndex` by a positional correspondence that the PDB's own counts confirm —
//!   see [`typecaps`].
//!
//! # What this crate is not
//!
//! Neither backend is a complete retail simulation. The compact backend contains explicitly
//! reported scaffolding; [`env::VecEnv::provenance`] prints its complete list. The
//! authoritative backend executes the retail-ordered `Sim` core, but currently admits only
//! NOOP and the completely hosted subset of MOVE_TO. Every other policy verb is a typed
//! refusal rather than accepted no-effect behavior.

pub mod action;
pub mod authoritative_backend;
pub mod authoritative_episode;
pub mod backend;
pub mod env;
pub mod eval;
pub mod generated;
pub mod mask;
pub mod obs;
pub mod reward;
pub mod spec;
pub mod state;
pub mod typecaps;

#[cfg(feature = "python")]
mod py;

pub use authoritative_backend::{
    decode_unit_heads, ApplyReceipt, ApplyRefusal, AuthoritativeBackend, CoreObservation,
    CoreReward, CoreRewardSnapshot, FactoredApplyRefusal, FactoredUnitActionSpace,
    HeadDecodeRefusal, IntegrationBoundary, OwnEntityObservation, PreparedUnitAction,
    ProjectionRefusal, QueuePosition, Relation, UnitActionRequest, VerbIntegration, VerbRoute,
    PLAYER_INTEGRATION, UNIT_INTEGRATION,
};
pub use authoritative_episode::{
    AuthoritativeEpisode, EpisodeError, ScenarioSpec, ScenarioUnit, StepReceipt,
};
pub use backend::{BackendCreateError, BackendKind, EnvironmentBackend};
pub use env::VecEnv;
pub use spec::EnvConfig;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generated as g;
    use crate::spec::get_bit;

    fn env(n: usize) -> VecEnv {
        let cfg = EnvConfig {
            grid_w: 32,
            grid_h: 32,
            max_entities: 32,
            max_controlled: 16,
            num_agents: 2,
            start_units: 8,
            max_steps: 64,
            ..Default::default()
        };
        VecEnv::new(n, cfg, None, None, 1).unwrap()
    }

    #[test]
    fn every_opcode_is_classified_exactly_once() {
        let mut seen = [0u8; 82];
        for v in g::UNIT_VERBS.iter() {
            seen[v.opcode as usize] += 1;
        }
        for v in g::PLAYER_VERBS.iter() {
            seen[v.opcode as usize] += 1;
        }
        for (op, _) in g::SELECTION_OPCODES {
            seen[op as usize] += 1;
        }
        for (op, _) in g::UI_OPCODES {
            seen[op as usize] += 1;
        }
        for (op, _) in g::ADMIN_OPCODES {
            seen[op as usize] += 1;
        }
        for (op, _) in g::CHEAT_OPCODES {
            seen[op as usize] += 1;
        }
        for (op, c) in seen.iter().enumerate() {
            assert_eq!(*c, 1, "opcode {op} classified {c} times");
        }
    }

    #[test]
    fn head_layout_matches_the_generated_head_count() {
        let cfg = EnvConfig::default();
        assert_eq!(spec::unit_head_sizes(&cfg).len(), g::N_UNIT_HEADS);
        assert_eq!(spec::player_head_sizes(&cfg).len(), g::N_PLAYER_HEADS);
    }

    /// Invariant 2 of `mask.rs`: no head of any live entity is ever entirely zero.
    #[test]
    fn no_mask_head_is_ever_all_zero() {
        let mut e = env(4);
        let ua = vec![0i32; 4 * 2 * e.cfg.max_controlled * g::N_UNIT_HEADS];
        let pa = vec![0i32; 4 * 2 * g::N_PLAYER_HEADS];
        for _ in 0..10 {
            e.step(&ua, &pa);
            let rec = e.unit_mask_layout.record_bytes;
            for (i, chunk) in e.unit_masks().chunks(rec).enumerate() {
                for h in 0..g::N_UNIT_HEADS {
                    let o = e.unit_mask_layout.offsets[h];
                    let n = e.unit_mask_layout.sizes[h].div_ceil(8);
                    let any = chunk[o..o + n].iter().any(|b| *b != 0);
                    assert!(any, "unit mask head {h} all-zero at record {i}");
                }
            }
            let prec = e.player_mask_layout.record_bytes;
            for chunk in e.player_masks().chunks(prec) {
                for h in 0..g::N_PLAYER_HEADS {
                    let o = e.player_mask_layout.offsets[h];
                    let n = e.player_mask_layout.sizes[h].div_ceil(8);
                    assert!(
                        chunk[o..o + n].iter().any(|b| *b != 0),
                        "player head {h} all-zero"
                    );
                }
            }
        }
    }

    /// Invariant 1, the part a factored mask can promise: an action sampled strictly under
    /// the mask is never rejected as illegal.
    #[test]
    fn masked_sampling_produces_no_illegal_actions() {
        let mut e = env(8);
        let n = e.len();
        let a = e.cfg.num_agents;
        let mut rng: u64 = 0xC0FFEE;
        let mut next = move || {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            rng
        };
        for _ in 0..25 {
            let mut ua = vec![0i32; n * a * e.cfg.max_controlled * g::N_UNIT_HEADS];
            let mut pa = vec![0i32; n * a * g::N_PLAYER_HEADS];
            let rec = e.unit_mask_layout.record_bytes;
            for r in 0..n * a * e.cfg.max_controlled {
                let m = &e.unit_masks()[r * rec..(r + 1) * rec];
                for h in 0..g::N_UNIT_HEADS {
                    let o = e.unit_mask_layout.offsets[h];
                    let sz = e.unit_mask_layout.sizes[h];
                    let legal: Vec<usize> = (0..sz).filter(|&b| get_bit(&m[o..], b)).collect();
                    assert!(!legal.is_empty());
                    ua[r * g::N_UNIT_HEADS + h] = legal[(next() as usize) % legal.len()] as i32;
                }
            }
            let prec = e.player_mask_layout.record_bytes;
            for r in 0..n * a {
                let m = &e.player_masks()[r * prec..(r + 1) * prec];
                for h in 0..g::N_PLAYER_HEADS {
                    let o = e.player_mask_layout.offsets[h];
                    let sz = e.player_mask_layout.sizes[h];
                    let legal: Vec<usize> = (0..sz).filter(|&b| get_bit(&m[o..], b)).collect();
                    pa[r * g::N_PLAYER_HEADS + h] = legal[(next() as usize) % legal.len()] as i32;
                }
            }
            e.step(&ua, &pa);
        }
        let s = e.apply_stats;
        assert_eq!(
            s.illegal,
            0,
            "masked sampling produced {} illegal actions out of {} (noop {}, applied {}, \
             accepted-no-effect {})",
            s.illegal,
            s.noop + s.applied + s.accepted_no_effect + s.illegal,
            s.noop,
            s.applied,
            s.accepted_no_effect
        );
        assert!(
            s.applied > 0,
            "test is vacuous unless something was applied"
        );
    }

    #[test]
    fn stepping_is_deterministic_across_thread_counts() {
        let digest = |threads: usize| {
            let cfg = EnvConfig {
                grid_w: 32,
                grid_h: 32,
                max_entities: 32,
                max_controlled: 16,
                num_agents: 2,
                start_units: 8,
                max_steps: 0,
                ..Default::default()
            };
            let mut e = VecEnv::new(6, cfg, None, None, threads).unwrap();
            let ua = vec![1i32; 6 * 2 * e.cfg.max_controlled * g::N_UNIT_HEADS];
            let pa = vec![0i32; 6 * 2 * g::N_PLAYER_HEADS];
            for _ in 0..20 {
                e.step(&ua, &pa);
            }
            let mut h: u64 = 0xcbf2_9ce4_8422_2325;
            for w in &e.worlds {
                h ^= w.sim.digest();
                h = h.wrapping_mul(0x100_0000_01B3);
            }
            for &v in e.entities().iter() {
                h ^= v.to_bits() as u64;
                h = h.wrapping_mul(0x100_0000_01B3);
            }
            h
        };
        let want = digest(1);
        for t in [2usize, 4, 8] {
            assert_eq!(digest(t), want, "thread count {t} changed the result");
        }
    }

    fn assert_float_bits(label: &str, a: &[f32], b: &[f32]) {
        assert_eq!(a.len(), b.len(), "{label} length");
        for (i, (x, y)) in a.iter().zip(b).enumerate() {
            assert_eq!(x.to_bits(), y.to_bits(), "{label}[{i}]");
        }
    }

    fn assert_env_image(a: &VecEnv, b: &VecEnv) {
        assert_eq!(a.worlds.len(), b.worlds.len());
        for (i, (x, y)) in a.worlds.iter().zip(&b.worlds).enumerate() {
            assert_eq!(x.sim.digest(), y.sim.digest(), "world {i} sim");
            assert_eq!(x.type_index, y.type_index, "world {i} types");
            assert_eq!(x.order, y.order, "world {i} orders");
            assert_eq!(x.target, y.target, "world {i} targets");
            assert_eq!(x.dest_x, y.dest_x, "world {i} dest x");
            assert_eq!(x.dest_y, y.dest_y, "world {i} dest y");
            assert_eq!(x.stance, y.stance, "world {i} stance");
            assert_eq!(x.form, y.form, "world {i} form");
            assert_eq!(x.ctrl, y.ctrl, "world {i} control handles");
            assert_eq!(x.obs_ents, y.obs_ents, "world {i} observed handles");
            assert_eq!(x.step_index, y.step_index, "world {i} step");
        }
        assert_float_bits("spatial", a.spatial(), b.spatial());
        assert_float_bits("entities", a.entities(), b.entities());
        assert_float_bits("globals", a.globals(), b.globals());
        assert_float_bits("rewards", a.rewards(), b.rewards());
        assert_float_bits("terms", a.reward_terms(), b.reward_terms());
        assert_eq!(a.unit_masks(), b.unit_masks(), "unit masks");
        assert_eq!(a.player_masks(), b.player_masks(), "player masks");
        assert_eq!(a.entity_rows(), b.entity_rows(), "entity rows");
        assert_eq!(a.dones(), b.dones(), "done flags");
        assert_eq!(a.truncateds(), b.truncateds(), "truncated flags");
        assert_eq!(a.apply_stats, b.apply_stats, "action results");
    }

    /// Worker scheduling is not allowed to enter either the simulator or the reference
    /// masked sampler's random stream. This compares all returned arrays and the mutable
    /// entity columns, not merely a high-level score.
    #[test]
    fn full_step_and_sampler_are_thread_count_independent() {
        let run = |threads| {
            let cfg = EnvConfig {
                grid_w: 24,
                grid_h: 20,
                max_entities: 40,
                max_controlled: 20,
                num_agents: 3,
                start_units: 10,
                max_steps: 0,
                ..Default::default()
            };
            let mut e = VecEnv::new(11, cfg, None, None, threads).unwrap();
            for _ in 0..12 {
                e.sample_masked();
                let ua = e.sampled_unit_actions().to_vec();
                let pa = e.sampled_player_actions().to_vec();
                e.step(&ua, &pa);
            }
            e
        };

        let serial = run(1);
        for threads in [2usize, 4, 8] {
            let parallel = run(threads);
            assert_eq!(
                serial.sampled_unit_actions(),
                parallel.sampled_unit_actions(),
                "unit sampler changed at {threads} threads"
            );
            assert_eq!(
                serial.sampled_player_actions(),
                parallel.sampled_player_actions(),
                "player sampler changed at {threads} threads"
            );
            assert_env_image(&serial, &parallel);
        }
    }

    #[test]
    fn parallel_reset_reconstructs_the_same_complete_observation() {
        let mut serial = env(13);
        let mut parallel = {
            let cfg = serial.cfg.clone();
            VecEnv::new(13, cfg, None, None, 8).unwrap()
        };
        let ua = vec![0; 13 * 2 * serial.cfg.max_controlled * g::N_UNIT_HEADS];
        let pa = vec![0; 13 * 2 * g::N_PLAYER_HEADS];
        for _ in 0..7 {
            serial.step(&ua, &pa);
            parallel.step(&ua, &pa);
        }
        serial.reset_all();
        parallel.reset_all();
        assert_env_image(&serial, &parallel);
    }

    /// The spatial digest was captured with the pre-optimization implementation at
    /// `fc7e2b0`. The mask digest was recaptured from the shipped type-capability path
    /// after the action-honesty gate stopped advertising unsupported verbs and removed
    /// padding fallback bits from live records. It was recaptured again when exact HALT
    /// and DISBAND lifecycle predicates replaced the approximate STANCE advertisement,
    /// then when captured stance types 1..3 gained their exact executable transaction.
    /// Together they catch accidental changes to plane clearing, entity occupancy, or
    /// packed-mask emission independently of the cross-thread comparison above.
    #[test]
    fn hot_output_fingerprint_matches_the_pre_optimization_capture() {
        let cfg = EnvConfig {
            grid_w: 64,
            grid_h: 64,
            max_entities: 64,
            max_controlled: 32,
            num_agents: 2,
            frames_per_step: 1,
            max_steps: 0,
            start_units: 16,
            fog: false,
            seed: 0x5EED,
        };
        let e = VecEnv::new(1, cfg, None, None, 1).unwrap();
        let spatial = e.spatial().iter().fold(0x811C_9DC5u32, |h, v| {
            (h ^ v.to_bits()).wrapping_mul(0x0100_0193)
        });
        let masks = e.unit_masks().iter().fold(0x811C_9DC5u32, |h, v| {
            (h ^ u32::from(*v)).wrapping_mul(0x0100_0193)
        });
        assert_eq!(spatial, 0xD9EA_9DC5);
        assert_eq!(masks, 0x0B6D_3EB1);
    }

    #[test]
    fn reward_defaults_to_sparse_outcome_and_shaping_is_settable() {
        let mut s = reward::RewardSpec::default();
        assert_eq!(s.weights[reward::IDX_WIN], 1.0);
        assert!(s.set("d_units_killed", 0.25));
        assert!(!s.set("no_such_term", 1.0));
    }

    #[test]
    fn observation_buffers_have_the_declared_shapes() {
        let e = env(3);
        let (n, a, c) = (3usize, e.cfg.num_agents, &e.cfg);
        assert_eq!(
            e.spatial().len(),
            n * a * obs::N_PLANES * c.grid_h * c.grid_w
        );
        assert_eq!(
            e.entities().len(),
            n * a * c.max_entities * obs::N_ENTITY_FEATURES
        );
        assert_eq!(e.globals().len(), n * a * obs::N_GLOBAL_FEATURES);
        assert_eq!(e.rewards().len(), n * a);
        assert_eq!(e.reward_terms().len(), n * a * reward::N_TERMS);
        assert_eq!(e.entity_rows().len(), n * a * c.max_entities);
    }
}
