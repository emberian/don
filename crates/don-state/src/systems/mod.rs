//! One module per retail `Game::do_frame` step body. Each module exposes
//! `STATUS` (how much of the body is transcribed) and `run`, which mutates
//! only the fields its retail function writes. A `Stub` module's `run` must
//! be a no-op so the frame burn-down keeps pointing at it.

pub mod leaders_process;
pub mod objects_inc_time;
pub mod objects_process;
pub mod game_daemon;
pub mod armies_process;
pub mod graphic_events_process;
pub mod leaders_end_process;
pub mod orders_roads;
pub mod build_process;
pub mod misc_steps;
pub mod construct_time;
pub mod objects_query;
pub mod leader_calc_gather;
pub mod unit_work;
pub mod pathfinder;
pub mod commands;
pub mod unit_combat;
pub mod production_train;
pub mod research;
pub mod city_process;
pub mod diplomacy;
pub mod air_flight;
pub mod territory;
pub mod achieve_events;
pub mod animals;
pub mod post_rules_trailer;
pub mod img;
