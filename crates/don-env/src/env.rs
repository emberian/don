//! The vectorised environment: N independent worlds, stepped in parallel, writing into
//! buffers the env owns for its lifetime.
//!
//! # Buffer ownership is the zero-copy contract
//!
//! Every observation, mask and reward array is allocated once at construction and
//! rewritten in place. The Python layer takes a pointer and a shape and builds a `numpy`
//! view; there is no serialisation step anywhere in `step`. The cost of that is a hard
//! rule: a returned array is only valid until the next `step`, and the Python side copies
//! if it wants to keep one.
//!
//! # Parallelism
//!
//! Worlds share nothing, so the batch dimension is embarrassingly parallel — the same
//! property `don_sim::Batch` rests on. Workers pull chunks of worlds from a shared cursor,
//! and buffer slices are partitioned by world, so no two workers touch the same bytes.

use crate::action::{apply_player, apply_unit, ApplyStats, PlayerAction, UnitAction};
use crate::generated as g;
use crate::mask::MaskWriter;
use crate::obs;
use crate::reward::{self, RewardSpec, N_TERMS};
use crate::spec::{player_head_sizes, unit_head_sizes, EnvConfig, MaskLayout};
use crate::state::{EnvWorld, Rules};
use std::sync::Arc;

pub struct VecEnv {
    pub cfg: EnvConfig,
    pub rules: Arc<Rules>,
    pub worlds: Vec<EnvWorld>,
    pub reward_spec: RewardSpec,

    pub unit_mask_layout: MaskLayout,
    pub player_mask_layout: MaskLayout,

    // ---- owned output buffers, shape documented on each accessor -------------------
    spatial: Vec<f32>,
    entities: Vec<f32>,
    globals: Vec<f32>,
    unit_masks: Vec<u8>,
    player_masks: Vec<u8>,
    rewards: Vec<f32>,
    terms: Vec<f32>,
    dones: Vec<u8>,
    truncs: Vec<u8>,
    /// For each (env, agent, slot) the entity row that slot refers to; -1 when empty.
    /// Exposed so a policy can join observation rows back to engine entities.
    entity_rows: Vec<i32>,
    /// Reference masked sampler output, same shapes as the action inputs. Sampling in
    /// Rust rather than numpy is not a convenience: the `Type` head is 806 wide, so
    /// unpacking every mask to bool in numpy costs an order of magnitude more than the
    /// env step itself and would make any reported steps/s a measurement of numpy.
    sample_unit: Vec<i32>,
    sample_player: Vec<i32>,
    sample_rng: u64,

    threads: usize,
    caps_real: bool,
    balance_real: bool,
    pub steps_taken: u64,
    pub apply_stats: ApplyStats,
}

impl VecEnv {
    pub fn new(
        n: usize,
        cfg: EnvConfig,
        typecaps: Option<&std::path::Path>,
        balance: Option<&std::path::Path>,
        threads: usize,
    ) -> Result<VecEnv, String> {
        cfg.validate()?;
        let (rules, caps_real, balance_real) = Rules::load(typecaps, balance);
        let cap = (cfg.start_units + 8) * g::NUM_PLAYERS + 256;
        let worlds: Vec<EnvWorld> = (0..n)
            .map(|i| {
                EnvWorld::new(
                    rules.clone(),
                    cap,
                    cfg.seed ^ (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15),
                    cfg.grid_w,
                    cfg.grid_h,
                )
            })
            .collect();
        let a = cfg.num_agents;
        let uml = MaskLayout::new(&unit_head_sizes(&cfg));
        let pml = MaskLayout::new(&player_head_sizes(&cfg));
        let mut e = VecEnv {
            spatial: vec![0.0; n * a * obs::N_PLANES * cfg.grid_h * cfg.grid_w],
            entities: vec![0.0; n * a * cfg.max_entities * obs::N_ENTITY_FEATURES],
            globals: vec![0.0; n * a * obs::N_GLOBAL_FEATURES],
            unit_masks: vec![0; n * a * cfg.max_controlled * uml.record_bytes],
            player_masks: vec![0; n * a * pml.record_bytes],
            rewards: vec![0.0; n * a],
            terms: vec![0.0; n * a * N_TERMS],
            dones: vec![0; n],
            truncs: vec![0; n],
            entity_rows: vec![-1; n * a * cfg.max_entities],
            sample_unit: vec![0; n * a * cfg.max_controlled * g::N_UNIT_HEADS],
            sample_player: vec![0; n * a * g::N_PLAYER_HEADS],
            sample_rng: cfg.seed | 1,
            unit_mask_layout: uml,
            player_mask_layout: pml,
            reward_spec: RewardSpec::default(),
            worlds,
            rules,
            cfg,
            threads: threads.max(1),
            caps_real,
            balance_real,
            steps_taken: 0,
            apply_stats: ApplyStats::default(),
        };
        e.reset_all();
        Ok(e)
    }

    pub fn len(&self) -> usize {
        self.worlds.len()
    }
    pub fn is_empty(&self) -> bool {
        self.worlds.is_empty()
    }

    /// Honest description of what is derived and what is scaffolding, so a training run
    /// can print it into its own log instead of a human remembering.
    pub fn provenance(&self) -> Vec<(String, String)> {
        let mut v = vec![
            (
                "action_space".into(),
                format!(
                    "{} unit verbs + {} player verbs, from the 82 CommandTypes opcodes \
                      [measured, schema/command-wire.json]",
                    g::N_UNIT_VERBS,
                    g::N_PLAYER_VERBS
                ),
            ),
            (
                "tick_ms".into(),
                format!(
                    "{} at Normal [measured, TurnControl::timings 0x00AFC4A4]",
                    g::TICK_MS_NORMAL
                ),
            ),
            (
                "owner_rotation".into(),
                "(frame + i) % 10, as Objects::process_all [measured]".into(),
            ),
            (
                "typecaps".into(),
                if self.caps_real {
                    "derived from ron-data/unitrules.xml + buildingrules.xml [measured]".into()
                } else {
                    "ABSENT — masks are PERMISSIVE. Run crates/don-env/gen/gen_spec.py".to_string()
                },
            ),
            (
                "balance_table".into(),
                if self.balance_real {
                    "schema/live/balance-real.bin, 493x493 int16 from combat_table+4 [measured]"
                        .into()
                } else {
                    "ABSENT — damage uses a flat 100% balance term".to_string()
                },
            ),
            (
                "damage".into(),
                "don_sim::mechanics::damage (ObjectData::get_damage 0x00644130) with default \
              predicates: spine only, guarded terms unreached"
                    .into(),
            ),
        ];
        let scaffold = [
            "movement: straight-line integer approach, NOT Unit::move_step / PathFinder::astar_path",
            "pathfinding: absent",
            "gathering / economy rates: absent, base_rate is always 0",
            "build queue timing: absent, QueueUp and Build complete instantly",
            "tech tree / prerequisites: absent, PREQ columns unread",
            "fog of war: LOS-radius reveal from unitrules LOS; the engine's fog model is underived",
            "terrain: no map, so 5 of 12 spatial planes are structurally zero",
            "start positions: placeholder ring, not the map generator",
        ];
        for s in scaffold {
            v.push(("scaffolding".into(), s.into()));
        }
        for gap in crate::mask::CROSS_HEAD_GAPS {
            v.push(("mask_gap".into(), gap.into()));
        }
        v
    }

    /// Total verbs accepted with no dynamics behind them, summed over the batch.
    pub fn unimplemented_counts(&self) -> Vec<(String, u64)> {
        let mut out = Vec::new();
        for (i, vd) in g::UNIT_VERBS.iter().enumerate() {
            let n: u64 = self.worlds.iter().map(|w| w.unimplemented.unit[i]).sum();
            if n > 0 {
                out.push((format!("unit.{}", vd.name), n));
            }
        }
        for (i, vd) in g::PLAYER_VERBS.iter().enumerate() {
            let n: u64 = self.worlds.iter().map(|w| w.unimplemented.player[i]).sum();
            if n > 0 {
                out.push((format!("player.{}", vd.name), n));
            }
        }
        out.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
        out
    }

    pub fn reset_all(&mut self) {
        let (agents, start, seed) = (self.cfg.num_agents, self.cfg.start_units, self.cfg.seed);
        for (i, w) in self.worlds.iter_mut().enumerate() {
            w.reset(
                agents,
                start,
                seed ^ (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15),
            );
        }
        self.dones.fill(0);
        self.truncs.fill(0);
        self.rewards.fill(0.0);
        self.terms.fill(0.0);
        self.refresh_observations();
    }

    /// Advance every world by `frames_per_step`, applying `unit_actions` and
    /// `player_actions`, then rewrite every observation and mask.
    ///
    /// `unit_actions` is `(n_envs, n_agents, max_controlled, 10)` row-major i32.
    /// `player_actions` is `(n_envs, n_agents, 5)`.
    pub fn step(&mut self, unit_actions: &[i32], player_actions: &[i32]) {
        let cfg = self.cfg.clone();
        let a = cfg.num_agents;
        let ua_stride = cfg.max_controlled * g::N_UNIT_HEADS;
        let pa_stride = g::N_PLAYER_HEADS;
        let rules_terms = N_TERMS;
        let spec = self.reward_spec.clone();

        let n = self.worlds.len();
        let threads = self.threads.min(n.max(1));
        let sp_stride = a * obs::N_PLANES * cfg.grid_h * cfg.grid_w;
        let en_stride = a * cfg.max_entities * obs::N_ENTITY_FEATURES;
        let gl_stride = a * obs::N_GLOBAL_FEATURES;
        let um_stride = a * cfg.max_controlled * self.unit_mask_layout.record_bytes;
        let pm_stride = a * self.player_mask_layout.record_bytes;
        let er_stride = a * cfg.max_entities;

        let uml = self.unit_mask_layout.clone();
        let pml = self.player_mask_layout.clone();

        // Partition every buffer by world so the workers never overlap.
        let mut parts: Vec<WorldPart<'_>> = Vec::with_capacity(n);
        let mut worlds = self.worlds.as_mut_slice();
        let mut spatial = self.spatial.as_mut_slice();
        let mut entities = self.entities.as_mut_slice();
        let mut globals = self.globals.as_mut_slice();
        let mut umask = self.unit_masks.as_mut_slice();
        let mut pmask = self.player_masks.as_mut_slice();
        let mut rewards = self.rewards.as_mut_slice();
        let mut terms = self.terms.as_mut_slice();
        let mut dones = self.dones.as_mut_slice();
        let mut truncs = self.truncs.as_mut_slice();
        let mut erows = self.entity_rows.as_mut_slice();
        for i in 0..n {
            let (w, rest) = worlds.split_at_mut(1);
            worlds = rest;
            let (sp, rest) = spatial.split_at_mut(sp_stride);
            spatial = rest;
            let (en, rest) = entities.split_at_mut(en_stride);
            entities = rest;
            let (gl, rest) = globals.split_at_mut(gl_stride);
            globals = rest;
            let (um, rest) = umask.split_at_mut(um_stride);
            umask = rest;
            let (pm, rest) = pmask.split_at_mut(pm_stride);
            pmask = rest;
            let (rw, rest) = rewards.split_at_mut(a);
            rewards = rest;
            let (tm, rest) = terms.split_at_mut(a * rules_terms);
            terms = rest;
            let (dn, rest) = dones.split_at_mut(1);
            dones = rest;
            let (tr, rest) = truncs.split_at_mut(1);
            truncs = rest;
            let (er, rest) = erows.split_at_mut(er_stride);
            erows = rest;
            parts.push(WorldPart {
                w: &mut w[0],
                ua: &unit_actions[i * a * ua_stride..(i + 1) * a * ua_stride],
                pa: &player_actions[i * a * pa_stride..(i + 1) * a * pa_stride],
                sp,
                en,
                gl,
                um,
                pm,
                rw,
                tm,
                dn,
                tr,
                er,
            });
        }

        let stats = std::sync::Mutex::new(ApplyStats::default());
        let chunk = parts.len().div_ceil(threads.max(1)).max(1);
        std::thread::scope(|scope| {
            for group in parts.chunks_mut(chunk) {
                let (cfg, spec, uml, pml, stats) = (&cfg, &spec, &uml, &pml, &stats);
                scope.spawn(move || {
                    let mut mw = MaskWriter::new(cfg);
                    let mut local = ApplyStats::default();
                    let mut sel: Vec<usize> = Vec::with_capacity(cfg.max_entities);
                    let mut own: Vec<usize> = Vec::with_capacity(cfg.max_controlled);
                    for p in group.iter_mut() {
                        step_one(
                            p, cfg, spec, uml, pml, &mut mw, &mut sel, &mut own, &mut local,
                        );
                    }
                    stats.lock().expect("worker panicked").add(&local);
                });
            }
        });
        self.apply_stats
            .add(&stats.into_inner().expect("worker panicked"));
        self.steps_taken += 1;
    }

    fn refresh_observations(&mut self) {
        let zero_u =
            vec![
                0i32;
                self.worlds.len() * self.cfg.num_agents * self.cfg.max_controlled * g::N_UNIT_HEADS
            ];
        let zero_p = vec![0i32; self.worlds.len() * self.cfg.num_agents * g::N_PLAYER_HEADS];
        let before = self.steps_taken;
        let saved = std::mem::take(&mut self.apply_stats);
        // A NOOP step: applies nothing, and writes every observation and mask.
        self.step(&zero_u, &zero_p);
        self.steps_taken = before;
        self.apply_stats = saved;
        // The NOOP step still advanced the sim by one frame; undo the episode counter so
        // `reset` is observationally a reset.
        for w in &mut self.worlds {
            w.step_index = 0;
        }
        self.rewards.fill(0.0);
        self.terms.fill(0.0);
    }

    // ---- buffer accessors ------------------------------------------------------------
    /// `(n_envs, n_agents, N_PLANES, grid_h, grid_w)` f32.
    pub fn spatial(&self) -> &[f32] {
        &self.spatial
    }
    /// `(n_envs, n_agents, max_entities, N_ENTITY_FEATURES)` f32.
    pub fn entities(&self) -> &[f32] {
        &self.entities
    }
    /// `(n_envs, n_agents, N_GLOBAL_FEATURES)` f32.
    pub fn globals(&self) -> &[f32] {
        &self.globals
    }
    /// `(n_envs, n_agents, max_controlled, unit_mask_record_bytes)` u8, bit-packed LSB-first.
    pub fn unit_masks(&self) -> &[u8] {
        &self.unit_masks
    }
    /// `(n_envs, n_agents, player_mask_record_bytes)` u8, bit-packed LSB-first.
    pub fn player_masks(&self) -> &[u8] {
        &self.player_masks
    }
    /// `(n_envs, n_agents)` f32.
    pub fn rewards(&self) -> &[f32] {
        &self.rewards
    }
    /// `(n_envs, n_agents, N_TERMS)` f32.
    pub fn reward_terms(&self) -> &[f32] {
        &self.terms
    }
    /// `(n_envs,)` u8.
    pub fn dones(&self) -> &[u8] {
        &self.dones
    }
    pub fn truncateds(&self) -> &[u8] {
        &self.truncs
    }
    /// `(n_envs, n_agents, max_entities)` i32 — entity row per observation slot, -1 empty.
    pub fn entity_rows(&self) -> &[i32] {
        &self.entity_rows
    }
    pub fn sampled_unit_actions(&self) -> &[i32] {
        &self.sample_unit
    }
    pub fn sampled_player_actions(&self) -> &[i32] {
        &self.sample_player
    }

    /// Draw a uniform action from under the current masks, into the sampler buffers.
    ///
    /// Reservoir sampling straight over the packed bitsets: whole zero bytes are skipped,
    /// so the 806-wide `Type` head costs 101 byte loads rather than 806 elements. This is
    /// the reference behaviour a masked policy must reproduce, and the opponent a scripted
    /// league slot can use without leaving Rust.
    pub fn sample_masked(&mut self) {
        let a = self.cfg.num_agents;
        let urec = self.unit_mask_layout.record_bytes;
        let prec = self.player_mask_layout.record_bytes;
        let n = self.worlds.len();
        let threads = self.threads.min(n.max(1));

        // Chunk by world, matching how the masks themselves were written, so the sampler
        // touches the same cache lines the mask writer just left hot.
        let per_world_recs = a * self.cfg.max_controlled;
        let uml = &self.unit_mask_layout;
        let pml = &self.player_mask_layout;
        let seed = self.sample_rng;
        let chunk = n.div_ceil(threads.max(1)).max(1);
        let umasks = &self.unit_masks;
        let pmasks = &self.player_masks;
        let mut u_out = self
            .sample_unit
            .chunks_mut(chunk * per_world_recs * g::N_UNIT_HEADS);
        let mut p_out = self.sample_player.chunks_mut(chunk * a * g::N_PLAYER_HEADS);
        std::thread::scope(|scope| {
            let mut w0 = 0usize;
            while w0 < n {
                let w1 = (w0 + chunk).min(n);
                let uo = u_out.next().expect("chunk");
                let po = p_out.next().expect("chunk");
                let um = &umasks[w0 * per_world_recs * urec..w1 * per_world_recs * urec];
                let pm = &pmasks[w0 * a * prec..w1 * a * prec];
                let s = seed ^ ((w0 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
                scope.spawn(move || {
                    let mut rng = s | 1;
                    let mut next = move || {
                        rng ^= rng << 13;
                        rng ^= rng >> 7;
                        rng ^= rng << 17;
                        rng
                    };
                    for (i, rec) in um.chunks_exact(urec).enumerate() {
                        let o = i * g::N_UNIT_HEADS;
                        for h in 0..g::N_UNIT_HEADS {
                            uo[o + h] = pick(&rec[uml.offsets[h]..], uml.sizes[h], &mut next);
                        }
                    }
                    for (i, rec) in pm.chunks_exact(prec).enumerate() {
                        let o = i * g::N_PLAYER_HEADS;
                        for h in 0..g::N_PLAYER_HEADS {
                            po[o + h] = pick(&rec[pml.offsets[h]..], pml.sizes[h], &mut next);
                        }
                    }
                });
                w0 = w1;
            }
        });
        self.sample_rng = self
            .sample_rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1);
    }
}

/// Uniform choice among the set bits of `buf[..n]`: popcount to a total, one draw, then
/// select the k-th set bit. Two branch-free byte sweeps instead of reservoir sampling's
/// per-bit division, which is what the 806-wide `Type` head made expensive.
///
/// Returns 0 if nothing is set, which cannot happen for a mask this crate wrote
/// (`mask.rs` invariant 2); bits at or past `n` are never set by any writer here.
#[inline]
fn pick(buf: &[u8], n: usize, next: &mut impl FnMut() -> u64) -> i32 {
    let nb = n.div_ceil(8);
    let bytes = &buf[..nb];
    let mut total = 0u32;
    for b in bytes {
        total += b.count_ones();
    }
    if total <= 1 {
        // The overwhelmingly common case for the small heads, and it needs no draw.
        return match total {
            0 => 0,
            _ => bytes
                .iter()
                .position(|b| *b != 0)
                .map(|i| (i * 8 + bytes[i].trailing_zeros() as usize) as i32)
                .unwrap_or(0),
        };
    }
    let mut k = (next() % total as u64) as u32;
    for (i, &b) in bytes.iter().enumerate() {
        let c = b.count_ones();
        if k < c {
            let mut bb = b;
            for _ in 0..k {
                bb &= bb - 1;
            }
            return (i * 8 + bb.trailing_zeros() as usize) as i32;
        }
        k -= c;
    }
    0
}

struct WorldPart<'a> {
    w: &'a mut EnvWorld,
    ua: &'a [i32],
    pa: &'a [i32],
    sp: &'a mut [f32],
    en: &'a mut [f32],
    gl: &'a mut [f32],
    um: &'a mut [u8],
    pm: &'a mut [u8],
    rw: &'a mut [f32],
    tm: &'a mut [f32],
    dn: &'a mut [u8],
    tr: &'a mut [u8],
    er: &'a mut [i32],
}

#[allow(clippy::too_many_arguments)]
fn step_one(
    p: &mut WorldPart<'_>,
    cfg: &EnvConfig,
    spec: &RewardSpec,
    uml: &MaskLayout,
    pml: &MaskLayout,
    mw: &mut MaskWriter,
    sel: &mut Vec<usize>,
    own: &mut Vec<usize>,
    stats: &mut ApplyStats,
) {
    let a = cfg.num_agents;
    let snaps: Vec<_> = (0..a).map(|k| reward::snapshot(p.w, k as u8)).collect();

    // Apply actions. Agents are visited in the engine's rotated owner order so the
    // scheduler bias matches `Objects::process_all` rather than a fixed 0..n sweep.
    let f = p.w.sim.frame as usize;
    for i in 0..a {
        let who = ((f + i) % a) as u8;
        // Actions index the handle list captured when the observation was written, so
        // slot k means the same entity the policy saw even after another agent's action
        // has compacted the SoA rows.
        let n_ctrl = p.w.ctrl[who as usize].len();
        let base = who as usize * cfg.max_controlled * g::N_UNIT_HEADS;
        for k in 0..n_ctrl.min(cfg.max_controlled) {
            let h = p.w.ctrl[who as usize][k];
            let o = base + k * g::N_UNIT_HEADS;
            let act = UnitAction::from_slice(&p.ua[o..o + g::N_UNIT_HEADS]);
            apply_unit(p.w, cfg, who, h, act, stats);
        }
        let po = who as usize * g::N_PLAYER_HEADS;
        let pact = PlayerAction::from_slice(&p.pa[po..po + g::N_PLAYER_HEADS]);
        apply_player(p.w, who, pact, stats);
    }

    for _ in 0..cfg.frames_per_step.max(1) {
        p.w.frame();
    }
    p.w.step_index += 1;

    // Reward, termination.
    let mut any_done = false;
    for k in 0..a {
        let t = &mut p.tm[k * N_TERMS..(k + 1) * N_TERMS];
        let done = reward::write_terms(p.w, k as u8, &snaps[k], t);
        p.rw[k] = spec.combine(t);
        any_done |= done;
    }
    let truncated = cfg.max_steps > 0 && p.w.step_index >= cfg.max_steps;
    p.dn[0] = u8::from(any_done);
    p.tr[0] = u8::from(truncated);
    if any_done || truncated {
        let seed = (p.w.sim.frame as u64) ^ (p.w.step_index as u64).wrapping_mul(0x9E37_79B9);
        p.w.reset(cfg.num_agents, cfg.start_units, seed | 1);
    }

    // Observations and masks, after any autoreset, so the returned observation is the
    // first observation of the new episode — Gymnasium's `autoreset_mode=NextStep`
    // semantics with the final observation available through `reward_terms`.
    let plane = obs::N_PLANES * cfg.grid_h * cfg.grid_w;
    for k in 0..a {
        let who = k as u8;
        obs::select_entities(p.w, cfg, who, sel);
        controlled_rows(p.w, cfg, who, own);
        obs::write_spatial(p.w, cfg, who, &mut p.sp[k * plane..(k + 1) * plane]);
        obs::write_entities(
            p.w,
            cfg,
            who,
            sel,
            own.len(),
            &mut p.en[k * cfg.max_entities * obs::N_ENTITY_FEATURES
                ..(k + 1) * cfg.max_entities * obs::N_ENTITY_FEATURES],
        );
        obs::write_global(
            p.w,
            cfg,
            who,
            &mut p.gl[k * obs::N_GLOBAL_FEATURES..(k + 1) * obs::N_GLOBAL_FEATURES],
        );
        let er = &mut p.er[k * cfg.max_entities..(k + 1) * cfg.max_entities];
        er.fill(-1);
        for (i, &r) in sel.iter().enumerate() {
            er[i] = r as i32;
        }
        let ctrl_h: Vec<_> = own.iter().map(|&r| p.w.handle_at(r)).collect();
        let obs_h: Vec<_> = sel.iter().map(|&r| p.w.handle_at(r)).collect();
        p.w.ctrl[k] = ctrl_h;
        p.w.obs_ents[k] = obs_h;
        let um = &mut p.um[k * cfg.max_controlled * uml.record_bytes
            ..(k + 1) * cfg.max_controlled * uml.record_bytes];
        mw.write_unit_masks(p.w, cfg, who, own, sel, um);
        let pm = &mut p.pm[k * pml.record_bytes..(k + 1) * pml.record_bytes];
        mw.write_player_mask(p.w, who, pm);
    }
}

/// The rows an agent may command this step: its own entities, row order, truncated to
/// `max_controlled`. Row order is stable within a frame and is what the observation's
/// `controllable` flag marks.
fn controlled_rows(w: &EnvWorld, cfg: &EnvConfig, who: u8, out: &mut Vec<usize>) {
    out.clear();
    let n = w.sim.live_count() as usize;
    for row in 0..n {
        if w.sim.owner()[row] == who as i8 {
            out.push(row);
            if out.len() == cfg.max_controlled {
                return;
            }
        }
    }
}
