//! Binary oracle: map `riseofnations.exe` into a live 32-bit process and call retail
//! functions directly, so our Rust reimplementation can be differentially tested against
//! the actual shipped code.
//!
//! The crate is a library plus two binaries so that the *models* and the *mapping* exist
//! exactly once. Two copies of a model is two things that can be right about each other
//! while both being wrong about the binary.
//!
//! * [`registry`] — the declarative case list. Adding a differential case is data.
//! * [`run`] — executes the registry under fork isolation and emits the JSON record.
//! * [`image`] — mapping, relocating, calling, hashing; the fork helper; the selftest.
//! * [`models`] — Rust models for targets that have no `don-sim` implementation *yet*.
//! * [`damage_env`] / [`damage_test`] — the fabricated world `FUN_00644130` needs.
//!
//! Builds only for 32-bit x86; it executes the retail image in-process. It is excluded
//! from the workspace, so `cargo test` at the repo root never touches it — **a green
//! `cargo test` is evidence about the Rust crates only, never about a fidelity claim.**

pub mod damage_env;
pub mod damage_test;
pub mod image;
pub mod models;
pub mod registry;
pub mod run;
