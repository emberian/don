//! `rontoy-core` turns coherent, versioned game telemetry into conservative
//! coaching cards.
//!
//! The crate deliberately contains no process attachment, clocks, network I/O,
//! UI, or game mutation.  Callers supply an explicit observation time, so an
//! identical history always produces identical cards and lifecycle events.
//! Recommendations are evidence-gated observations, not claims of optimal play.

#![forbid(unsafe_code)]

mod engine;
mod model;

pub use engine::{AdviceEngine, CoachConfig};
pub use model::*;
