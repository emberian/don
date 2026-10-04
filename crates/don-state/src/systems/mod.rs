//! One module per retail `Game::do_frame` step body. Each module exposes
//! `STATUS` (how much of the body is transcribed) and `run`, which mutates
//! only the fields its retail function writes. A `Stub` module's `run` must
//! be a no-op so the frame burn-down keeps pointing at it.

pub mod leaders_process;
pub mod objects_inc_time;
pub mod objects_process;
