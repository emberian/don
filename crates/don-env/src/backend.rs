//! Explicit side-by-side selection between the compact and authoritative environment owners.
//!
//! Selection stops at construction and typed access. The two variants intentionally do not
//! share a `step` trait: compact `VecEnv` consumes generated factored batches, while the
//! authoritative backend accepts only transactions whose complete effects are hosted by
//! `don_sim::tick::Sim`. A common action method here would either weaken refusal semantics or
//! falsely imply observation and reward parity.

use crate::authoritative_backend::AuthoritativeBackend;
use crate::authoritative_episode::{EpisodeError, ScenarioSpec};
use crate::env::VecEnv;
use crate::spec::EnvConfig;
use std::fmt;
use std::path::Path;

/// The selected mutable game-state owner and fidelity tier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendKind {
    /// Existing batched `EnvWorld` throughput surface.
    Compact,
    /// Single-episode `don_sim::tick::Sim` ownership surface.
    Authoritative,
}

/// Construction errors preserve which backend rejected its input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BackendCreateError {
    Compact(String),
    Authoritative(EpisodeError),
}

impl fmt::Display for BackendCreateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Compact(reason) => write!(f, "compact backend construction failed: {reason}"),
            Self::Authoritative(reason) => {
                write!(f, "authoritative backend construction failed: {reason}")
            }
        }
    }
}

impl std::error::Error for BackendCreateError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Compact(_) => None,
            Self::Authoritative(reason) => Some(reason),
        }
    }
}

/// An explicit ownership selector, not a promise that both fidelity tiers have one API.
///
/// Existing callers can continue to construct [`VecEnv`] directly. New migration/evaluation
/// call sites can hold this enum and branch once on [`Self::kind`], then use the selected
/// backend's native typed surface.
pub enum EnvironmentBackend {
    Compact(VecEnv),
    Authoritative(AuthoritativeBackend),
}

impl EnvironmentBackend {
    /// Construct the unchanged compact vector backend.
    pub fn compact(
        worlds: usize,
        config: EnvConfig,
        typecaps: Option<&Path>,
        balance: Option<&Path>,
        threads: usize,
    ) -> Result<Self, BackendCreateError> {
        VecEnv::new(worlds, config, typecaps, balance, threads)
            .map(Self::Compact)
            .map_err(BackendCreateError::Compact)
    }

    /// Construct one deterministic authoritative episode from explicit scenario inputs.
    pub fn authoritative(scenario: ScenarioSpec) -> Result<Self, BackendCreateError> {
        AuthoritativeBackend::from_spec(scenario)
            .map(Self::Authoritative)
            .map_err(BackendCreateError::Authoritative)
    }

    pub fn kind(&self) -> BackendKind {
        match self {
            Self::Compact(_) => BackendKind::Compact,
            Self::Authoritative(_) => BackendKind::Authoritative,
        }
    }

    pub fn as_compact(&self) -> Option<&VecEnv> {
        match self {
            Self::Compact(backend) => Some(backend),
            Self::Authoritative(_) => None,
        }
    }

    pub fn as_compact_mut(&mut self) -> Option<&mut VecEnv> {
        match self {
            Self::Compact(backend) => Some(backend),
            Self::Authoritative(_) => None,
        }
    }

    pub fn as_authoritative(&self) -> Option<&AuthoritativeBackend> {
        match self {
            Self::Compact(_) => None,
            Self::Authoritative(backend) => Some(backend),
        }
    }

    pub fn as_authoritative_mut(&mut self) -> Option<&mut AuthoritativeBackend> {
        match self {
            Self::Compact(_) => None,
            Self::Authoritative(backend) => Some(backend),
        }
    }
}
