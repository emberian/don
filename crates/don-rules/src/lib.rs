//! Parsers for Rise of Nations rule data.
//!
//! Everything here is derived from the shipped data files or from the binary, per
//! `docs/CHARTER.md`. Community documentation is never a source.

pub mod value;

pub use value::{Number, RuleValue};
