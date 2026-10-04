//! steps 0-3, 9-10, 18, 24, 26-28 (0x00000000). Step misc. Not yet transcribed.

use crate::tick::StepStatus;
use crate::Save;

pub const STATUS: StepStatus = StepStatus::Stub;

pub fn run(_save: &mut Save, _effects: &mut Vec<String>) {}
