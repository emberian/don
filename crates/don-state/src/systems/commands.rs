//! CommandPackage::process_* — applying lockstep commands onto the save state. Not yet transcribed. Leaf module: exposes free functions for the
//! owning traversal to call; never edits sibling modules.

#![allow(dead_code)]

use crate::tick::StepStatus;

pub const STATUS: StepStatus = StepStatus::Stub;
