//! Unit::do_attack / Object::take_damage 0x00652020 — the 31-stage integer damage chain, armor, overkill. Not yet transcribed. Leaf module: exposes free functions for the
//! owning traversal to call; never edits sibling modules.

#![allow(dead_code)]

use crate::tick::StepStatus;

pub const STATUS: StepStatus = StepStatus::Stub;
