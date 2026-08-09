//! `scenario_data` checksum channel (`ScenarioData::walk_data`, `0x00997ad0`).
//!
//! This is a checksum-only primitive. It mirrors the statically recoverable retail
//! traversal over `ScenarioData`'s global state, including the checksum-specific omission
//! of `user_warnings`. It does not execute scenario scripts or infer any state that the
//! caller did not supply.
//!
//! The module is deliberately standalone until it is wired into `don-replay`:
//!
//! ```text
//! rustc --edition 2021 --test crates/don-replay/src/scenario_channel.rs \
//!   -o /tmp/scenario-channel-test
//! /tmp/scenario-channel-test
//! ```

#![forbid(unsafe_code)]

use std::fmt;

/// First recorded value in every one of the 21 checksum-bearing corpus files.
///
/// This is a replay target, not a constant channel: scenario state is mutable and this
/// value is not returned unless the supplied state actually hashes to it. A full scan of
/// the 488,557 checksum packets found 2,458 distinct scenario-channel values.
pub const CORPUS_INITIAL_SCENARIO_CHANNEL: u32 = 0x0992_2b90;

pub const PLAYER_COUNT: usize = 8;
pub const FIND_COUNTER_COUNT: usize = 31;
pub const UNIT_TYPE_COUNT: usize = 352;
pub const BUILD_TYPE_COUNT: usize = 129;
pub const UNITS_KILLED_COUNT: usize = UNIT_TYPE_COUNT * PLAYER_COUNT;
pub const BUILDS_DESTROYED_COUNT: usize = BUILD_TYPE_COUNT * PLAYER_COUNT;
pub const COLOR_BYTES: usize = 10;

/// The metadata serialized for a non-empty retail `Array`, `ObjectArray`, or
/// `SimpleArray`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ArrayMeta {
    pub capacity: i32,
    pub increment: i16,
    pub flags: u8,
}

/// A retail array with storage details kept separate from its logical elements.
///
/// Empty arrays serialize only their zero length, so their metadata is ignored. For a
/// non-empty array, validation rejects a negative or undersized capacity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetailArray<'a, T> {
    pub meta: ArrayMeta,
    pub elements: &'a [T],
}

impl<'a, T> RetailArray<'a, T> {
    pub const fn empty() -> Self {
        Self {
            meta: ArrayMeta {
                capacity: 0,
                increment: 0,
                flags: 0,
            },
            elements: &[],
        }
    }
}

/// A retail `String`, represented by exactly the UTF-16 code units checksummed by
/// `String::walk_data` (`0x00a1b2d0`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Utf16String<'a>(pub &'a [u16]);

pub type Color = [u8; COLOR_BYTES];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptTimer<'a> {
    pub time: i32,
    pub name: Utf16String<'a>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ScenarioComponent {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub kind: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ScenarioMessage<'a> {
    pub text: Utf16String<'a>,
    pub color: Color,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyedMessage<'a> {
    /// `LLNode<ScenarioMessage, int> + 0x2c`.
    pub key: i32,
    pub message: ScenarioMessage<'a>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WCoord {
    pub x: i32,
    pub y: i32,
}

/// Checksum-visible `BitMask` state. `flags` is not walked by this routine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BitMask<'a> {
    pub bits: i32,
    /// Must be non-negative and exactly equal to `payload.len()`.
    pub size: i32,
    pub payload: &'a [u8],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ScenarioRevealPoint {
    pub x: i32,
    pub y: i32,
    pub radius: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ScenarioObjective<'a> {
    pub completed: i32,
    pub print: i32,
    /// Retail walks `sound` before `id`.
    pub sound: Utf16String<'a>,
    pub id: Utf16String<'a>,
    pub message: ScenarioMessage<'a>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObjectiveEntry<'a> {
    /// The integer returned by the pointed-to object's id virtual before its walk.
    pub object_id: i32,
    /// `PtrLinkList` node key at node `+0x0c`.
    pub key: i32,
    pub objective: ScenarioObjective<'a>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScenarioGroup<'a> {
    pub who: i32,
    pub find_counter: i32,
    pub unit_ids: RetailArray<'a, i32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NamedScenarioGroup<'a> {
    pub group: ScenarioGroup<'a>,
    pub name: Utf16String<'a>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvolvedObject {
    /// The one-byte `LinkList<int, unsigned char>` node key, walked first.
    pub key: u8,
    pub object_id: i32,
}

/// Fixed-width global fields, in the identities recovered from the shipped PDB.
#[derive(Debug, Clone, Copy)]
pub struct ScenarioDirect<'a> {
    pub load_scenario_script: i32,
    pub msg_time: i32,
    pub game_msg_time: i32,
    pub hilite_option: i32,
    pub hilite_object: i32,
    pub highlight_x: i32,
    pub highlight_y: i32,
    pub involved_who: i32,
    pub camera_init_x: [i32; PLAYER_COUNT],
    pub camera_init_y: [i32; PLAYER_COUNT],
    pub camera_init_zoom: [i32; PLAYER_COUNT],
    pub custom_time_limit: i32,
    pub find_counters: [i32; FIND_COUNTER_COUNT],
    pub last_razed: [i32; PLAYER_COUNT],
    pub city_lost_to: [i16; PLAYER_COUNT],
    /// Row-major memory order of PDB `short[352][8]`.
    pub units_killed: &'a [i16],
    /// Row-major memory order of PDB `short[129][8]`.
    pub builds_destroyed: &'a [i16],
    pub reinforcements_arrived: [u8; PLAYER_COUNT],
    /// Row-major PDB `unsigned char[8][8]`.
    pub war_blocked: [u8; PLAYER_COUNT * PLAYER_COUNT],
    pub ally_mask: [u8; PLAYER_COUNT],
    pub diplomacy_setting: [u8; PLAYER_COUNT],
    pub plunder: u8,
    pub building_unit_bonus: u8,
    pub building_resource_bonus: u8,
    pub buildings_free: u8,
    pub buildings_gather: u8,
    pub units_free: u8,
    pub techs_free: u8,
    pub speed_control_disabled: u8,
    pub pause_disabled: u8,
    pub mouse_selection_disabled: u8,
    pub hotkey_selection_disabled: u8,
    pub display_bubble_text: u8,
    pub highlight_visible: u8,
    pub highlight_active: u8,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ScenarioStrings<'a> {
    pub scenario_name: Utf16String<'a>,
    pub temp_save: Utf16String<'a>,
    pub victory_message: Utf16String<'a>,
    pub defeat_message: Utf16String<'a>,
    pub general_powers_script: Utf16String<'a>,
    pub general_powers_script_file: Utf16String<'a>,
}

/// Complete checksum input. No field is synthesized by the walker.
#[derive(Debug, Clone, Copy)]
pub struct ScenarioState<'a> {
    pub direct: ScenarioDirect<'a>,
    pub strings: ScenarioStrings<'a>,
    pub msg_color: Color,
    pub game_msg_color: Color,
    pub objective_color: Color,
    pub timers: &'a [ScriptTimer<'a>],
    pub components: RetailArray<'a, ScenarioComponent>,
    pub messages: &'a [KeyedMessage<'a>],
    /// `user_warnings` is intentionally absent: `ScenarioData::walk_data` skips it when
    /// `DataWalk+0x08 != 0`, which is the `CheckSum` case.
    pub extra_starting_locs: RetailArray<'a, WCoord>,
    pub city_lost: BitMask<'a>,
    pub objectives: [&'a [ObjectiveEntry<'a>]; PLAYER_COUNT],
    pub scenario_groups: RetailArray<'a, NamedScenarioGroup<'a>>,
    pub involved_objects: &'a [InvolvedObject],
    pub reveal_points: [RetailArray<'a, ScenarioRevealPoint>; PLAYER_COUNT],
    pub attrition_free_points: [RetailArray<'a, ScenarioRevealPoint>; PLAYER_COUNT],
    pub objects_ignoring_orders: [RetailArray<'a, i32>; PLAYER_COUNT],
    pub ignore_orders: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScenarioChecksum {
    pub checksum: u32,
    pub bytes_walked: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScenarioChannelError {
    ExactLength {
        field: &'static str,
        expected: usize,
        actual: usize,
    },
    CountOverflow {
        field: &'static str,
        actual: usize,
    },
    StringTooLong {
        field: &'static str,
        actual: usize,
    },
    NegativeCapacity {
        field: &'static str,
        capacity: i32,
    },
    CapacityBelowLength {
        field: &'static str,
        capacity: i32,
        length: usize,
    },
    BitMaskSize {
        size: i32,
        payload: usize,
    },
}

impl fmt::Display for ScenarioChannelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for ScenarioChannelError {}

trait ByteSink {
    fn update(&mut self, bytes: &[u8]);
}

#[derive(Debug, Clone, Copy)]
struct Adler32 {
    s1: u32,
    s2: u32,
    bytes: u64,
}

impl Adler32 {
    fn new() -> Self {
        Self {
            s1: 1,
            s2: 0,
            bytes: 0,
        }
    }

    fn finish(self) -> ScenarioChecksum {
        ScenarioChecksum {
            checksum: (self.s2 << 16) | self.s1,
            bytes_walked: self.bytes,
        }
    }
}

impl ByteSink for Adler32 {
    fn update(&mut self, bytes: &[u8]) {
        // The arithmetic is `don_sim::checksum::adler32`, the workspace's only
        // implementation of `0x00a46830`. This struct is only the running `CheckSum+0x10`
        // / `+0x14` pair the channel carries.
        self.bytes += bytes.len() as u64;
        let sum = don_sim::checksum::adler32((self.s2 << 16) | self.s1, bytes);
        self.s1 = sum & 0xFFFF;
        self.s2 = sum >> 16;
    }
}

fn walk_i32<S: ByteSink>(sink: &mut S, value: i32) {
    sink.update(&value.to_le_bytes());
}

fn walk_i16<S: ByteSink>(sink: &mut S, value: i16) {
    sink.update(&value.to_le_bytes());
}

fn count_i32(field: &'static str, count: usize) -> Result<i32, ScenarioChannelError> {
    i32::try_from(count).map_err(|_| ScenarioChannelError::CountOverflow {
        field,
        actual: count,
    })
}

fn validate_string(
    field: &'static str,
    value: Utf16String<'_>,
) -> Result<(), ScenarioChannelError> {
    if value.0.len() > usize::from(u16::MAX) {
        return Err(ScenarioChannelError::StringTooLong {
            field,
            actual: value.0.len(),
        });
    }
    Ok(())
}

fn walk_string<S: ByteSink>(sink: &mut S, value: Utf16String<'_>) {
    sink.update(&(value.0.len() as u32).to_le_bytes());
    for &unit in value.0 {
        sink.update(&unit.to_le_bytes());
    }
}

fn validate_array<T>(
    field: &'static str,
    array: RetailArray<'_, T>,
) -> Result<(), ScenarioChannelError> {
    let len = count_i32(field, array.elements.len())?;
    if len != 0 {
        if array.meta.capacity < 0 {
            return Err(ScenarioChannelError::NegativeCapacity {
                field,
                capacity: array.meta.capacity,
            });
        }
        if array.meta.capacity < len {
            return Err(ScenarioChannelError::CapacityBelowLength {
                field,
                capacity: array.meta.capacity,
                length: array.elements.len(),
            });
        }
    }
    Ok(())
}

fn walk_array_header<S: ByteSink, T>(sink: &mut S, array: RetailArray<'_, T>) {
    walk_i32(sink, array.elements.len() as i32);
    if !array.elements.is_empty() {
        walk_i32(sink, array.meta.capacity);
        walk_i16(sink, array.meta.increment);
        // All three retail templates clear the ownership bit before walking flags.
        sink.update(&[array.meta.flags & 0xbf]);
    }
}

fn validate_message(
    field: &'static str,
    message: ScenarioMessage<'_>,
) -> Result<(), ScenarioChannelError> {
    validate_string(field, message.text)
}

fn walk_message<S: ByteSink>(sink: &mut S, message: ScenarioMessage<'_>) {
    // ScenarioMessage's walk_test tag is a no-op for CheckSum.
    walk_string(sink, message.text);
    sink.update(&message.color);
}

fn validate_objective(objective: ScenarioObjective<'_>) -> Result<(), ScenarioChannelError> {
    validate_string("objectives[].sound", objective.sound)?;
    validate_string("objectives[].id", objective.id)?;
    validate_message("objectives[].message.text", objective.message)
}

fn walk_objective<S: ByteSink>(sink: &mut S, objective: ScenarioObjective<'_>) {
    // This non-source order is the order at 0x00999640.
    walk_i32(sink, objective.completed);
    walk_i32(sink, objective.print);
    walk_string(sink, objective.sound);
    walk_string(sink, objective.id);
    walk_message(sink, objective.message);
}

fn validate_state(state: &ScenarioState<'_>) -> Result<(), ScenarioChannelError> {
    for (field, expected, actual) in [
        (
            "units_killed",
            UNITS_KILLED_COUNT,
            state.direct.units_killed.len(),
        ),
        (
            "builds_destroyed",
            BUILDS_DESTROYED_COUNT,
            state.direct.builds_destroyed.len(),
        ),
    ] {
        if expected != actual {
            return Err(ScenarioChannelError::ExactLength {
                field,
                expected,
                actual,
            });
        }
    }

    for (field, value) in [
        ("scenario_name", state.strings.scenario_name),
        ("temp_save", state.strings.temp_save),
        ("victory_message", state.strings.victory_message),
        ("defeat_message", state.strings.defeat_message),
        ("general_powers_script", state.strings.general_powers_script),
        (
            "general_powers_script_file",
            state.strings.general_powers_script_file,
        ),
    ] {
        validate_string(field, value)?;
    }

    count_i32("timers", state.timers.len())?;
    for timer in state.timers {
        validate_string("timers[].name", timer.name)?;
    }
    validate_array("components", state.components)?;
    count_i32("messages", state.messages.len())?;
    for entry in state.messages {
        validate_message("messages[].text", entry.message)?;
    }
    validate_array("extra_starting_locs", state.extra_starting_locs)?;

    if state.city_lost.size < 0 || state.city_lost.size as usize != state.city_lost.payload.len() {
        return Err(ScenarioChannelError::BitMaskSize {
            size: state.city_lost.size,
            payload: state.city_lost.payload.len(),
        });
    }

    for objectives in state.objectives {
        count_i32("objectives[player]", objectives.len())?;
        for entry in objectives {
            validate_objective(entry.objective)?;
        }
    }

    validate_array("scenario_groups", state.scenario_groups)?;
    for entry in state.scenario_groups.elements {
        validate_array("scenario_groups[].unit_ids", entry.group.unit_ids)?;
        validate_string("scenario_groups[].name", entry.name)?;
    }
    count_i32("involved_objects", state.involved_objects.len())?;

    for array in state.reveal_points {
        validate_array("reveal_points[player]", array)?;
    }
    for array in state.attrition_free_points {
        validate_array("attrition_free_points[player]", array)?;
    }
    for array in state.objects_ignoring_orders {
        validate_array("objects_ignoring_orders[player]", array)?;
    }
    Ok(())
}

fn walk_direct<S: ByteSink>(sink: &mut S, direct: &ScenarioDirect<'_>) {
    for value in [
        direct.load_scenario_script,
        direct.msg_time,
        direct.game_msg_time,
        direct.hilite_option,
        direct.hilite_object,
        direct.highlight_x,
        direct.highlight_y,
        direct.involved_who,
    ] {
        walk_i32(sink, value);
    }
    for player in 0..PLAYER_COUNT {
        walk_i32(sink, direct.camera_init_x[player]);
        walk_i32(sink, direct.camera_init_y[player]);
        walk_i32(sink, direct.camera_init_zoom[player]);
    }
    walk_i32(sink, direct.custom_time_limit);
    for &value in &direct.find_counters {
        walk_i32(sink, value);
    }
    for &value in &direct.last_razed {
        walk_i32(sink, value);
    }
    for &value in &direct.city_lost_to {
        walk_i16(sink, value);
    }
    for &value in direct.units_killed {
        walk_i16(sink, value);
    }
    for &value in direct.builds_destroyed {
        walk_i16(sink, value);
    }
    sink.update(&direct.reinforcements_arrived);
    sink.update(&direct.war_blocked);
    sink.update(&direct.ally_mask);
    sink.update(&direct.diplomacy_setting);
    sink.update(&[
        direct.plunder,
        direct.building_unit_bonus,
        direct.building_resource_bonus,
        direct.buildings_free,
        direct.buildings_gather,
        direct.units_free,
        direct.techs_free,
        direct.speed_control_disabled,
        direct.pause_disabled,
        direct.mouse_selection_disabled,
        direct.hotkey_selection_disabled,
        direct.display_bubble_text,
        direct.highlight_visible,
        direct.highlight_active,
    ]);
}

fn walk_state<S: ByteSink>(sink: &mut S, state: &ScenarioState<'_>) {
    // The leading walk_test tag at 0x00997ae5 emits no checksum bytes.
    walk_direct(sink, &state.direct);

    for value in [
        state.strings.scenario_name,
        state.strings.temp_save,
        state.strings.victory_message,
        state.strings.defeat_message,
        state.strings.general_powers_script,
        state.strings.general_powers_script_file,
    ] {
        walk_string(sink, value);
    }
    sink.update(&state.msg_color);
    sink.update(&state.game_msg_color);
    sink.update(&state.objective_color);

    // ScriptTimers is a LinkList<String, int>: count, then time and String per node.
    walk_i32(sink, state.timers.len() as i32);
    for timer in state.timers {
        walk_i32(sink, timer.time);
        walk_string(sink, timer.name);
    }

    walk_array_header(sink, state.components);
    for component in state.components.elements {
        walk_i32(sink, component.x);
        walk_i32(sink, component.y);
        walk_i32(sink, component.z);
        walk_i32(sink, component.kind);
    }

    walk_i32(sink, state.messages.len() as i32);
    for entry in state.messages {
        walk_i32(sink, entry.key);
        walk_message(sink, entry.message);
    }

    // `user_warnings` is omitted here by the retail checksum branch.
    walk_array_header(sink, state.extra_starting_locs);
    for coord in state.extra_starting_locs.elements {
        walk_i32(sink, coord.x);
        walk_i32(sink, coord.y);
    }

    walk_i32(sink, state.city_lost.bits);
    walk_i32(sink, state.city_lost.size);
    sink.update(state.city_lost.payload);

    for objectives in state.objectives {
        walk_i32(sink, objectives.len() as i32);
        for entry in objectives {
            walk_i32(sink, entry.object_id);
            walk_i32(sink, entry.key);
            walk_objective(sink, entry.objective);
        }
    }

    walk_array_header(sink, state.scenario_groups);
    for entry in state.scenario_groups.elements {
        walk_i32(sink, entry.group.who);
        walk_i32(sink, entry.group.find_counter);
        walk_array_header(sink, entry.group.unit_ids);
        for &unit_id in entry.group.unit_ids.elements {
            walk_i32(sink, unit_id);
        }
        walk_string(sink, entry.name);
    }

    walk_i32(sink, state.involved_objects.len() as i32);
    for entry in state.involved_objects {
        sink.update(&[entry.key]);
        walk_i32(sink, entry.object_id);
    }

    for array in state.reveal_points {
        walk_array_header(sink, array);
        for point in array.elements {
            walk_i32(sink, point.x);
            walk_i32(sink, point.y);
            walk_i32(sink, point.radius);
        }
    }
    for array in state.attrition_free_points {
        walk_array_header(sink, array);
        for point in array.elements {
            walk_i32(sink, point.x);
            walk_i32(sink, point.y);
            walk_i32(sink, point.radius);
        }
    }
    for array in state.objects_ignoring_orders {
        walk_array_header(sink, array);
        for &object_id in array.elements {
            walk_i32(sink, object_id);
        }
    }
    walk_i32(sink, state.ignore_orders);
}

/// Compute retail's `scenario_data` channel for a complete supplied state.
///
/// Validation runs before the first checksum byte is consumed, so malformed arrays,
/// truncated fixed tables, oversize strings, and inconsistent bit-mask payloads cannot
/// masquerade as complete state.
pub fn scenario_checksum(
    state: &ScenarioState<'_>,
) -> Result<ScenarioChecksum, ScenarioChannelError> {
    validate_state(state)?;
    let mut adler = Adler32::new();
    walk_state(&mut adler, state);
    Ok(adler.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Bytes(Vec<u8>);

    impl ByteSink for Bytes {
        fn update(&mut self, bytes: &[u8]) {
            self.0.extend_from_slice(bytes);
        }
    }

    fn blank_state<'a>(units: &'a [i16], builds: &'a [i16]) -> ScenarioState<'a> {
        let empty_reveal = RetailArray::empty();
        let empty_i32 = RetailArray::empty();
        ScenarioState {
            direct: ScenarioDirect {
                load_scenario_script: 0,
                msg_time: 0,
                game_msg_time: 0,
                hilite_option: 0,
                hilite_object: 0,
                highlight_x: 0,
                highlight_y: 0,
                involved_who: 0,
                camera_init_x: [0; PLAYER_COUNT],
                camera_init_y: [0; PLAYER_COUNT],
                camera_init_zoom: [0; PLAYER_COUNT],
                custom_time_limit: 0,
                find_counters: [0; FIND_COUNTER_COUNT],
                last_razed: [0; PLAYER_COUNT],
                city_lost_to: [0; PLAYER_COUNT],
                units_killed: units,
                builds_destroyed: builds,
                reinforcements_arrived: [0; PLAYER_COUNT],
                war_blocked: [0; PLAYER_COUNT * PLAYER_COUNT],
                ally_mask: [0; PLAYER_COUNT],
                diplomacy_setting: [0; PLAYER_COUNT],
                plunder: 0,
                building_unit_bonus: 0,
                building_resource_bonus: 0,
                buildings_free: 0,
                buildings_gather: 0,
                units_free: 0,
                techs_free: 0,
                speed_control_disabled: 0,
                pause_disabled: 0,
                mouse_selection_disabled: 0,
                hotkey_selection_disabled: 0,
                display_bubble_text: 0,
                highlight_visible: 0,
                highlight_active: 0,
            },
            strings: ScenarioStrings::default(),
            msg_color: [0; COLOR_BYTES],
            game_msg_color: [0; COLOR_BYTES],
            objective_color: [0; COLOR_BYTES],
            timers: &[],
            components: RetailArray::empty(),
            messages: &[],
            extra_starting_locs: RetailArray::empty(),
            city_lost: BitMask {
                bits: 0,
                size: 0,
                payload: &[],
            },
            objectives: [&[]; PLAYER_COUNT],
            scenario_groups: RetailArray::empty(),
            involved_objects: &[],
            reveal_points: [empty_reveal; PLAYER_COUNT],
            attrition_free_points: [empty_reveal; PLAYER_COUNT],
            objects_ignoring_orders: [empty_i32; PLAYER_COUNT],
            ignore_orders: 0,
        }
    }

    #[test]
    fn direct_prefix_is_retail_order_and_little_endian() {
        let units = [0i16; UNITS_KILLED_COUNT];
        let builds = [0i16; BUILDS_DESTROYED_COUNT];
        let mut state = blank_state(&units, &builds);
        state.direct.load_scenario_script = 0x0102_0304;
        state.direct.msg_time = 0x1112_1314;
        state.direct.game_msg_time = 0x2122_2324;
        state.direct.hilite_option = 0x3132_3334;

        let mut bytes = Bytes::default();
        walk_state(&mut bytes, &state);
        assert_eq!(
            &bytes.0[..16],
            &[
                0x04, 0x03, 0x02, 0x01, 0x14, 0x13, 0x12, 0x11, 0x24, 0x23, 0x22, 0x21, 0x34, 0x33,
                0x32, 0x31,
            ]
        );
    }

    #[test]
    fn array_header_and_elements_have_exact_byte_order() {
        let values = [0x1122_3344i32];
        let array = RetailArray {
            meta: ArrayMeta {
                capacity: 0x0102_0304,
                increment: 0x0506,
                flags: 0xc7,
            },
            elements: &values,
        };
        validate_array("fixture", array).unwrap();
        let mut bytes = Bytes::default();
        walk_array_header(&mut bytes, array);
        for &value in array.elements {
            walk_i32(&mut bytes, value);
        }
        assert_eq!(
            bytes.0,
            [
                0x01, 0x00, 0x00, 0x00, // length
                0x04, 0x03, 0x02, 0x01, // capacity
                0x06, 0x05, // increment
                0x87, // flags with retail ownership bit 0x40 cleared
                0x44, 0x33, 0x22, 0x11, // element
            ]
        );
    }

    #[test]
    fn strings_are_u32_count_then_utf16le() {
        let mut bytes = Bytes::default();
        walk_string(&mut bytes, Utf16String(&[0x1234, 0xabcd]));
        assert_eq!(bytes.0, [2, 0, 0, 0, 0x34, 0x12, 0xcd, 0xab]);
    }

    #[test]
    fn objective_keeps_the_retail_sound_id_base_order() {
        let objective = ScenarioObjective {
            completed: 0x0102_0304,
            print: 0x1112_1314,
            sound: Utf16String(&[0x2122]),
            id: Utf16String(&[0x3132]),
            message: ScenarioMessage {
                text: Utf16String(&[0x4142]),
                color: [0x51; COLOR_BYTES],
            },
        };
        let mut bytes = Bytes::default();
        walk_objective(&mut bytes, objective);
        assert_eq!(
            bytes.0,
            [
                0x04, 0x03, 0x02, 0x01, // completed
                0x14, 0x13, 0x12, 0x11, // print
                1, 0, 0, 0, 0x22, 0x21, // sound String
                1, 0, 0, 0, 0x32, 0x31, // id String
                1, 0, 0, 0, 0x42, 0x41, // base message String
                0x51, 0x51, 0x51, 0x51, 0x51, 0x51, 0x51, 0x51, 0x51, 0x51,
            ]
        );
    }

    #[test]
    fn one_byte_mutation_changes_the_channel_but_ownership_flag_does_not() {
        let units = [0i16; UNITS_KILLED_COUNT];
        let builds = [0i16; BUILDS_DESTROYED_COUNT];
        let mut state = blank_state(&units, &builds);
        let baseline = scenario_checksum(&state).unwrap();
        state.direct.highlight_active = 1;
        let mutated = scenario_checksum(&state).unwrap();
        assert_ne!(baseline.checksum, mutated.checksum);
        assert_eq!(baseline.bytes_walked, mutated.bytes_walked);

        let component = [ScenarioComponent::default()];
        state.direct.highlight_active = 0;
        state.components = RetailArray {
            meta: ArrayMeta {
                capacity: 1,
                increment: 1,
                flags: 0,
            },
            elements: &component,
        };
        let flag_clear = scenario_checksum(&state).unwrap();
        state.components.meta.flags = 0x40;
        let ownership_set = scenario_checksum(&state).unwrap();
        assert_eq!(flag_clear, ownership_set);
        state.components.meta.flags = 1;
        assert_ne!(
            flag_clear.checksum,
            scenario_checksum(&state).unwrap().checksum
        );
    }

    #[test]
    fn incomplete_fixed_tables_are_rejected_before_hashing() {
        let units = [0i16; UNITS_KILLED_COUNT - 1];
        let builds = [0i16; BUILDS_DESTROYED_COUNT];
        let state = blank_state(&units, &builds);
        assert_eq!(
            scenario_checksum(&state),
            Err(ScenarioChannelError::ExactLength {
                field: "units_killed",
                expected: UNITS_KILLED_COUNT,
                actual: UNITS_KILLED_COUNT - 1,
            })
        );
    }

    #[test]
    fn inconsistent_dynamic_boundaries_are_rejected() {
        let units = [0i16; UNITS_KILLED_COUNT];
        let builds = [0i16; BUILDS_DESTROYED_COUNT];
        let component = [ScenarioComponent::default()];
        let mut state = blank_state(&units, &builds);
        state.components = RetailArray {
            meta: ArrayMeta {
                capacity: 0,
                increment: 0,
                flags: 0,
            },
            elements: &component,
        };
        assert!(matches!(
            scenario_checksum(&state),
            Err(ScenarioChannelError::CapacityBelowLength {
                field: "components",
                capacity: 0,
                length: 1,
            })
        ));

        state.components = RetailArray::empty();
        state.city_lost = BitMask {
            bits: 8,
            size: 2,
            payload: &[0],
        };
        assert_eq!(
            scenario_checksum(&state),
            Err(ScenarioChannelError::BitMaskSize {
                size: 2,
                payload: 1,
            })
        );
    }
}
