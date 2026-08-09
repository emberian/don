//! PyO3 bindings.
//!
//! # The zero-copy contract, concretely
//!
//! `step` takes the action arrays as Python buffers and reads them in place; it returns
//! nothing. Observations, masks, rewards and masks are read through [`PyVecEnv::buffers`],
//! which hands Python a `(pointer, shape, dtype)` triple per array. `python/don_env`
//! wraps each in a `numpy` view over that memory. No array crosses the boundary by value,
//! at any point, in either direction.
//!
//! The cost is a lifetime rule the Python layer enforces: a view is valid until the next
//! `step`, and anything kept must be copied. That is the same contract EnvPool and the
//! Gymnasium vector envs use, and it is what makes a batched step free of allocation.

use crate::action::ApplyStats;
use crate::env::VecEnv;
use crate::generated as g;
use crate::obs;
use crate::reward;
use crate::spec::{self, EnvConfig};
use pyo3::buffer::PyBuffer;
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

fn err<E: std::fmt::Display>(e: E) -> PyErr {
    PyValueError::new_err(e.to_string())
}

/// Read an i32 numpy array as a plain slice without copying.
fn i32_slice<'a>(obj: &Bound<'a, PyAny>, want: usize, what: &str) -> PyResult<&'a [i32]> {
    let buf: PyBuffer<i32> = PyBuffer::get(obj)?;
    if !buf.is_c_contiguous() {
        return Err(err(format!("{what} must be C-contiguous int32")));
    }
    if buf.item_count() != want {
        return Err(err(format!(
            "{what} has {} elements, expected {want}",
            buf.item_count()
        )));
    }
    // Safe: the buffer is alive for the duration of the call (the caller holds the array),
    // C-contiguous, and typed i32 by `PyBuffer::get`.
    Ok(unsafe { std::slice::from_raw_parts(buf.buf_ptr() as *const i32, want) })
}

#[pyclass(name = "VecEnv", unsendable)]
pub struct PyVecEnv {
    inner: VecEnv,
    n: usize,
}

#[pymethods]
impl PyVecEnv {
    #[new]
    #[pyo3(signature = (num_envs, num_agents=2, grid_w=64, grid_h=64, max_entities=64,
                        max_controlled=32, frames_per_step=1, max_steps=4096,
                        start_units=16, fog=false, seed=0x5EED, threads=0,
                        typecaps_path=None, balance_path=None))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        num_envs: usize,
        num_agents: usize,
        grid_w: usize,
        grid_h: usize,
        max_entities: usize,
        max_controlled: usize,
        frames_per_step: u32,
        max_steps: u32,
        start_units: usize,
        fog: bool,
        seed: u64,
        threads: usize,
        typecaps_path: Option<String>,
        balance_path: Option<String>,
    ) -> PyResult<Self> {
        let cfg = EnvConfig {
            grid_w,
            grid_h,
            max_entities,
            max_controlled,
            num_agents,
            frames_per_step,
            max_steps,
            start_units,
            fog,
            seed,
        };
        let threads = if threads == 0 {
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4)
        } else {
            threads
        };
        let inner = VecEnv::new(
            num_envs,
            cfg,
            typecaps_path.as_deref().map(std::path::Path::new),
            balance_path.as_deref().map(std::path::Path::new),
            threads,
        )
        .map_err(err)?;
        Ok(PyVecEnv { inner, n: num_envs })
    }

    fn reset(&mut self) {
        self.inner.reset_all();
    }

    /// Draw a uniform action from under the current masks into the sampler buffers, which
    /// `buffers()` exposes as `sampled_unit_actions` / `sampled_player_actions`. The
    /// buffers can be passed straight back to `step` with no copy.
    fn sample_masked(&mut self) {
        self.inner.sample_masked();
    }

    /// Apply actions and advance. Both arrays are read in place.
    ///
    /// `unit_actions`: `(num_envs, num_agents, max_controlled, 10)` int32.
    /// `player_actions`: `(num_envs, num_agents, 5)` int32.
    fn step(
        &mut self,
        unit_actions: &Bound<'_, PyAny>,
        player_actions: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        let c = &self.inner.cfg;
        let want_u = self.n * c.num_agents * c.max_controlled * g::N_UNIT_HEADS;
        let want_p = self.n * c.num_agents * g::N_PLAYER_HEADS;
        let ua = i32_slice(unit_actions, want_u, "unit_actions")?;
        let pa = i32_slice(player_actions, want_p, "player_actions")?;
        self.inner.step(ua, pa);
        Ok(())
    }

    /// `{name: (ptr, shape, dtype)}` for every output array. Valid until the next `step`.
    fn buffers<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let c = self.inner.cfg.clone();
        let (n, a) = (self.n, c.num_agents);
        let d = PyDict::new(py);
        let put = |name: &str, ptr: usize, shape: Vec<usize>, dtype: &str| -> PyResult<()> {
            d.set_item(name, (ptr, shape, dtype))
        };
        put(
            "spatial",
            self.inner.spatial().as_ptr() as usize,
            vec![n, a, obs::N_PLANES, c.grid_h, c.grid_w],
            "float32",
        )?;
        put(
            "entities",
            self.inner.entities().as_ptr() as usize,
            vec![n, a, c.max_entities, obs::N_ENTITY_FEATURES],
            "float32",
        )?;
        put(
            "globals",
            self.inner.globals().as_ptr() as usize,
            vec![n, a, obs::N_GLOBAL_FEATURES],
            "float32",
        )?;
        put(
            "unit_masks",
            self.inner.unit_masks().as_ptr() as usize,
            vec![
                n,
                a,
                c.max_controlled,
                self.inner.unit_mask_layout.record_bytes,
            ],
            "uint8",
        )?;
        put(
            "player_masks",
            self.inner.player_masks().as_ptr() as usize,
            vec![n, a, self.inner.player_mask_layout.record_bytes],
            "uint8",
        )?;
        put(
            "rewards",
            self.inner.rewards().as_ptr() as usize,
            vec![n, a],
            "float32",
        )?;
        put(
            "reward_terms",
            self.inner.reward_terms().as_ptr() as usize,
            vec![n, a, reward::N_TERMS],
            "float32",
        )?;
        put(
            "dones",
            self.inner.dones().as_ptr() as usize,
            vec![n],
            "uint8",
        )?;
        put(
            "truncateds",
            self.inner.truncateds().as_ptr() as usize,
            vec![n],
            "uint8",
        )?;
        put(
            "entity_rows",
            self.inner.entity_rows().as_ptr() as usize,
            vec![n, a, c.max_entities],
            "int32",
        )?;
        put(
            "sampled_unit_actions",
            self.inner.sampled_unit_actions().as_ptr() as usize,
            vec![n, a, c.max_controlled, g::N_UNIT_HEADS],
            "int32",
        )?;
        put(
            "sampled_player_actions",
            self.inner.sampled_player_actions().as_ptr() as usize,
            vec![n, a, g::N_PLAYER_HEADS],
            "int32",
        )?;
        Ok(d)
    }

    /// Everything the Python side needs to build spaces and unpack masks.
    fn spec<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let c = &self.inner.cfg;
        let d = PyDict::new(py);
        d.set_item("num_envs", self.n)?;
        d.set_item("num_agents", c.num_agents)?;
        d.set_item("grid", (c.grid_w, c.grid_h))?;
        d.set_item("max_entities", c.max_entities)?;
        d.set_item("max_controlled", c.max_controlled)?;
        d.set_item("frames_per_step", c.frames_per_step)?;
        d.set_item("max_steps", c.max_steps)?;
        d.set_item("fog", c.fog)?;
        d.set_item("tick_ms", g::TICK_MS_NORMAL)?;
        d.set_item("unit_head_names", g::UNIT_HEAD_NAMES.to_vec())?;
        d.set_item("unit_head_sizes", spec::unit_head_sizes(c).to_vec())?;
        d.set_item(
            "unit_mask_offsets",
            self.inner.unit_mask_layout.offsets.clone(),
        )?;
        d.set_item(
            "unit_mask_record_bytes",
            self.inner.unit_mask_layout.record_bytes,
        )?;
        d.set_item("player_head_names", g::PLAYER_HEAD_NAMES.to_vec())?;
        d.set_item("player_head_sizes", spec::player_head_sizes(c).to_vec())?;
        d.set_item(
            "player_mask_offsets",
            self.inner.player_mask_layout.offsets.clone(),
        )?;
        d.set_item(
            "player_mask_record_bytes",
            self.inner.player_mask_layout.record_bytes,
        )?;
        d.set_item("unit_verbs", verb_list(py, true)?)?;
        d.set_item("player_verbs", verb_list(py, false)?)?;
        d.set_item("spatial_planes", spec::SPATIAL_PLANES.to_vec())?;
        d.set_item("live_planes", obs::LIVE_PLANES.to_vec())?;
        d.set_item("entity_features", spec::ENTITY_FEATURES.to_vec())?;
        d.set_item("global_features", spec::GLOBAL_FEATURES.to_vec())?;
        d.set_item("reward_terms", reward::TERMS.to_vec())?;
        d.set_item("score_terms", crate::state::ScoreTerms::NAMES.to_vec())?;
        d.set_item("score_terms_live", crate::state::ScoreTerms::LIVE.to_vec())?;
        d.set_item("amount_buckets", spec::AMOUNT_BUCKETS.to_vec())?;
        d.set_item("count_buckets", spec::COUNT_BUCKETS.to_vec())?;
        d.set_item("num_types", g::NUM_TYPES)?;
        d.set_item(
            "agent_ids",
            (0..c.num_agents)
                .map(|i| format!("player_{i}"))
                .collect::<Vec<_>>(),
        )?;
        Ok(d)
    }

    /// `[(kind, text)]` — what is derived and what is scaffolding.
    fn provenance<'py>(&self, py: Python<'py>) -> Bound<'py, PyList> {
        PyList::new(py, self.inner.provenance()).expect("list")
    }

    /// `[(verb, count)]` for verbs accepted with no dynamics behind them.
    fn unimplemented<'py>(&self, py: Python<'py>) -> Bound<'py, PyList> {
        PyList::new(py, self.inner.unimplemented_counts()).expect("list")
    }

    fn apply_stats<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let s: ApplyStats = self.inner.apply_stats;
        let d = PyDict::new(py);
        d.set_item("noop", s.noop)?;
        d.set_item("applied", s.applied)?;
        d.set_item("stale", s.stale)?;
        d.set_item("incoherent", s.incoherent)?;
        d.set_item("accepted_no_effect", s.accepted_no_effect)?;
        d.set_item("illegal", s.illegal)?;
        Ok(d)
    }

    /// Set one reward-shaping weight. Term names come from `spec()["reward_terms"]`.
    fn set_reward_weight(&mut self, name: &str, weight: f32) -> PyResult<()> {
        if self.inner.reward_spec.set(name, weight) {
            Ok(())
        } else {
            Err(PyRuntimeError::new_err(format!("no reward term {name:?}")))
        }
    }

    fn reward_weights<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let d = PyDict::new(py);
        for (i, t) in reward::TERMS.iter().enumerate() {
            d.set_item(*t, self.inner.reward_spec.weights[i])?;
        }
        Ok(d)
    }

    /// `(num_envs, num_agents, 11)` engine score components, plus our unweighted total as
    /// column 11. Copied — it is small and not on the hot path.
    fn scores(&self) -> Vec<Vec<Vec<i32>>> {
        self.inner
            .worlds
            .iter()
            .map(|w| {
                (0..self.inner.cfg.num_agents)
                    .map(|k| {
                        let s = w.players[k].score;
                        let mut v = s.as_array().to_vec();
                        v.push(s.total());
                        v
                    })
                    .collect()
            })
            .collect()
    }

    #[getter]
    fn num_envs(&self) -> usize {
        self.n
    }
    #[getter]
    fn steps_taken(&self) -> u64 {
        self.inner.steps_taken
    }
}

fn verb_list<'py>(py: Python<'py>, unit: bool) -> PyResult<Bound<'py, PyList>> {
    let rows: Vec<(usize, &str, u8, u16, Vec<&str>)> = if unit {
        g::UNIT_VERBS
            .iter()
            .enumerate()
            .map(|(i, v)| (i + 1, v.name, v.opcode, v.wire_size, v.unsupplied.to_vec()))
            .collect()
    } else {
        g::PLAYER_VERBS
            .iter()
            .enumerate()
            .map(|(i, v)| (i + 1, v.name, v.opcode, v.wire_size, v.unsupplied.to_vec()))
            .collect()
    };
    PyList::new(py, rows)
}

#[pymodule]
#[pyo3(name = "_don_env")]
fn don_env_module(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyVecEnv>()?;
    m.add(
        "__doc__",
        "Native core of the Descent of Nations RL environment.",
    )?;
    Ok(())
}
