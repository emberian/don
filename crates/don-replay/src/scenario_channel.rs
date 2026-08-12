//! `scenario_data` checksum channel (`ScenarioData::walk_data`, `0x00997ad0`).
//!
//! This is a checksum-only primitive. It mirrors the statically recoverable retail
//! traversal over `ScenarioData`'s global state, including the checksum-specific omission
//! of `user_warnings`. It does not execute scenario scripts or infer any state that the
//! caller did not supply.
//!
//! [`InitialScenarioChannel`] closes the loop: it combines the traversal with the state
//! `ScenarioFuncSet::init` `0x00a03c30` leaves behind and the two shipped
//! `internal_strings.xml` ordinals that initializer installs, and is what
//! [`crate::state::SimBridge::populate_scenario_initial`] puts on the channel.

#![forbid(unsafe_code)]

use std::fmt;
use std::path::{Path, PathBuf};

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

// ---------------------------------------------------------------------------
// The state retail actually starts a game in
// ---------------------------------------------------------------------------
//
// `ScenarioFuncSet::init` `0x00a03c30` is the whole initializer, and its only caller is
// `Game::init` (`ScenarioFuncSet::close` `0x00a03650` is the end-of-game path and writes
// a *different* state — zeroed cameras, for one). Every constant below was read off the
// instruction stream, not the decompiler's reconstruction; see
// `docs/assembly/scenario-initial-state.md` for the address-by-address table.
//
// Two of the six checksum-visible `String`s are assigned from the runtime `StringTable`
// `int_str_array` (`0x00c06378`) by fixed byte offset into its 20-byte `String` array:
//
//     0x00a04022  mov ecx, 0xe8d46c        ; ScenarioData::general_powers_script_file
//     0x00a04027  call String::operator=   ; = int_str_array[0x1d178 / 0x14 = 5958]
//     0x00a0423a  call String::operator=   ; temp_save = int_str_array[0x1d18c/0x14 = 5959]
//
// Those two values live in shipped `internal_strings.xml`. The extraction protocol is in
// `docs/assembly/scenario-initial-state.md` §7; the file itself is gitignored proprietary
// game data, so [`InitialScenarioChannel::load_from_ron_data`] reads it from the local
// install at run time and fails closed when it is absent. This module never guesses the
// two strings and never hard-codes their text.

/// `ScenarioData::msg_color` source constant at `0x00c8d260`, ten bytes.
/// `0x00a03dfd movq [0xe8fe34], xmm0` plus `0x00a03e31 mov [0xe8fe3c], eax`.
pub const RETAIL_INIT_MSG_COLOR: Color =
    [0x00, 0x00, 0x00, 0xff, 0x00, 0x00, 0x00, 0x00, 0x02, 0x00];

/// `ScenarioData::game_msg_color` and `objective_color` share one source constant at
/// `0x00c8d26c` (`0x00a03e29`/`0x00a03e36` store the same `xmm1`, `0x00a03e0c`/`0x00a03e12`
/// the same `ecx`).
pub const RETAIL_INIT_OBJECTIVE_COLOR: Color =
    [0xff, 0xff, 0xff, 0xff, 0xff, 0x7f, 0xff, 0xff, 0x02, 0x00];

/// `int_str_array` ordinal assigned to `general_powers_script_file` (`+0x1d178`, 20-byte
/// `String` stride).
pub const INTERNAL_STRING_ORDINAL_GENERAL_POWERS_SCRIPT_FILE: u32 = 0x1d178 / 0x14;
/// `int_str_array` ordinal assigned to `temp_save` (`+0x1d18c`).
pub const INTERNAL_STRING_ORDINAL_TEMP_SAVE: u32 = 0x1d18c / 0x14;

/// The two shipped-data strings `ScenarioFuncSet::init` installs. Both are required:
/// leaving one out is a different state, not a smaller one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ScenarioInitialStrings<'a> {
    /// `int_str_array[5958]`.
    pub general_powers_script_file: Utf16String<'a>,
    /// `int_str_array[5959]`.
    pub temp_save: Utf16String<'a>,
}

/// Owner for the two large zero tables `ScenarioFuncSet::init` clears, so the borrowed
/// [`ScenarioState`] can be built without allocating them at every call site.
///
/// `0x00a04194 rep stosd` with `ecx = 0xb0` clears 704 bytes of `units_killed` per player
/// (`0x00a041b0 add [ebp-0x18], 0x2c0`), and `ecx = 0x40` plus a trailing `stosw` clears
/// 258 bytes of `builds_destroyed` per player (`add [ebp-0x1c], 0x102`).
#[derive(Debug, Clone)]
pub struct RetailInitialScenario {
    units_killed: [i16; UNITS_KILLED_COUNT],
    builds_destroyed: [i16; BUILDS_DESTROYED_COUNT],
    /// `memset(&city_lost.payload, 0, (bits + 7) >> 3)` at `0x00a041fd`, with
    /// `BitMask<8>` giving `bits = 8` and `size = 1`.
    city_lost_payload: [u8; 1],
}

impl Default for RetailInitialScenario {
    fn default() -> Self {
        RetailInitialScenario {
            units_killed: [0; UNITS_KILLED_COUNT],
            builds_destroyed: [0; BUILDS_DESTROYED_COUNT],
            city_lost_payload: [0],
        }
    }
}

impl RetailInitialScenario {
    pub fn new() -> Self {
        Self::default()
    }

    /// The complete checksum-visible `ScenarioData` a retail `Game::init` leaves behind.
    ///
    /// `load_scenario_script` and `custom_time_limit` are the two scalars `init` does not
    /// write; they are zero-initialized data (`.data` beyond the raw size, so BSS) and
    /// `ScenarioFuncSet::close` `0x00a03aec`/its `[0xcc21b0] = 0` writes them back to zero
    /// between games, so zero is their value at the first `Game::init` and after every
    /// completed game. `scenario_name`, `victory_message` and `defeat_message` are
    /// likewise untouched by `init` and cleared to `EMPTY_STRING` by `close`.
    pub fn state<'a>(&'a self, strings: ScenarioInitialStrings<'a>) -> ScenarioState<'a> {
        ScenarioState {
            direct: ScenarioDirect {
                load_scenario_script: 0,
                msg_time: 200,         // 0x00a03e48 mov [0xcc21ec], 0xc8
                game_msg_time: 12_000, // 0x00a03e3e mov [0xcc2290], 0x2ee0
                hilite_option: -1,     // 0x00a03e52
                hilite_object: -1,     // 0x00a03e5c
                highlight_x: -1,       // [0xcc02fc]
                highlight_y: -1,       // [0xcc2188]
                involved_who: -1,      // 0x00a04243 mov [0xcc228c], 0xffffffff
                camera_init_x: [-1; PLAYER_COUNT],
                camera_init_y: [-1; PLAYER_COUNT],
                camera_init_zoom: [5; PLAYER_COUNT], // 0x00a03f2c mov [0xcc21c0], 5
                custom_time_limit: 0,
                // 0x00a03ed5..0x00a03f0e: seven `movaps` of the all-ones .rdata constant
                // at 0x00b69d40, one `movq`, one `mov dword`, covering 0xcc2210..0xcc228c.
                find_counters: [-1; FIND_COUNTER_COUNT],
                last_razed: [-1; PLAYER_COUNT], // 0x00a041ca [esi*4 + 0xcc2190]
                city_lost_to: [-1; PLAYER_COUNT], // 0x00a0410d mov [esi*2+0xcc0320], ax=0xffff
                units_killed: &self.units_killed,
                builds_destroyed: &self.builds_destroyed,
                reinforcements_arrived: [0; PLAYER_COUNT], // 0x00a041be
                war_blocked: [0; PLAYER_COUNT * PLAYER_COUNT], // 0x00a0419e movq [eax], 0
                ally_mask: [0; PLAYER_COUNT],              // 0x00a04141
                diplomacy_setting: [0; PLAYER_COUNT],      // 0x00a0413a
                plunder: 1,                                // 0x00cb195a
                building_unit_bonus: 1,                    // 0x00cb195b
                building_resource_bonus: 1,                // 0x00cb4ba9
                buildings_free: 0,                         // 0x00cb4bab
                buildings_gather: 1,                       // 0x00cbe329
                units_free: 0,                             // 0x00cb7df9
                techs_free: 0,                             // 0x00cb4baa
                speed_control_disabled: 0,                 // 0x00cbe32b
                pause_disabled: 0,                         // 0x00cbe5af
                mouse_selection_disabled: 0,               // 0x00cb7dfb
                hotkey_selection_disabled: 0,              // 0x00cb7dfa
                display_bubble_text: 1,                    // 0x00a04008 mov [0xcbb0d9], 1
                highlight_visible: 0,                      // 0x00cbb0da
                highlight_active: 0,                       // 0x00cbb0db
            },
            strings: ScenarioStrings {
                scenario_name: Utf16String(&[]),
                temp_save: strings.temp_save,
                victory_message: Utf16String(&[]),
                defeat_message: Utf16String(&[]),
                // 0x00a03e18 mov ecx, 0xe8d41c ; = EMPTY_STRING (0x00eb437c)
                general_powers_script: Utf16String(&[]),
                general_powers_script_file: strings.general_powers_script_file,
            },
            msg_color: RETAIL_INIT_MSG_COLOR,
            game_msg_color: RETAIL_INIT_OBJECTIVE_COLOR,
            objective_color: RETAIL_INIT_OBJECTIVE_COLOR,
            timers: &[],
            components: RetailArray::empty(),
            messages: &[],
            extra_starting_locs: RetailArray::empty(),
            city_lost: BitMask {
                bits: 8,
                size: 1,
                payload: &self.city_lost_payload,
            },
            objectives: [&[]; PLAYER_COUNT],
            scenario_groups: RetailArray::empty(),
            involved_objects: &[],
            reveal_points: [RetailArray::empty(); PLAYER_COUNT],
            attrition_free_points: [RetailArray::empty(); PLAYER_COUNT],
            objects_ignoring_orders: [RetailArray::empty(); PLAYER_COUNT],
            ignore_orders: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Binding the two shipped strings, and the channel value that follows
// ---------------------------------------------------------------------------

/// Shipped file name of the internal string table. Retail loads it once at startup
/// (`StringTable::init` `0x00A28520`) and never reloads it; see `docs/tracks/mod-story.md`
/// §2.5.
pub const INTERNAL_STRINGS_FILE: &str = don_content::string_table::INTERNAL_STRINGS;

/// Why the two shipped ordinals could not be turned into a channel value.
///
/// Every variant is a refusal, not a fallback. There is no default string, because a
/// wrong string produces a plausible-looking 32-bit number that is not retail's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InternalStringsError {
    /// `ron-data/internal_strings.xml` is not in the local extraction.
    NotExtracted(PathBuf),
    Read {
        path: PathBuf,
        message: String,
    },
    /// The bytes are not the UTF-8 the supported install ships. `don-content`'s decoder
    /// also accepts UTF-8 BOM and UTF-16LE BOM, but its entry point is private, so this
    /// path names the encoding instead of guessing at it.
    Encoding {
        path: PathBuf,
        message: String,
    },
    Parse {
        path: PathBuf,
        message: String,
    },
    /// The table parsed but is shorter than the ordinal `ScenarioFuncSet::init` indexes.
    OrdinalMissing {
        ordinal: u32,
        entries: usize,
    },
    /// The derived state failed its own structural validation. Unreachable unless the
    /// initializer table is edited into an inconsistent shape.
    Checksum(ScenarioChannelError),
}

impl fmt::Display for InternalStringsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotExtracted(path) => write!(
                f,
                "{} is not extracted; channel 14 has no shipped string source",
                path.display()
            ),
            Self::Read { path, message } => write!(f, "reading {}: {message}", path.display()),
            Self::Encoding { path, message } => write!(f, "decoding {}: {message}", path.display()),
            Self::Parse { path, message } => write!(f, "parsing {}: {message}", path.display()),
            Self::OrdinalMissing { ordinal, entries } => {
                write!(f, "ordinal {ordinal} beyond {entries} STRING entries")
            }
            Self::Checksum(e) => write!(f, "derived initial state is malformed: {e}"),
        }
    }
}

impl std::error::Error for InternalStringsError {}

/// The `scenario_data` value a retail client carries from `Game::init` until the first
/// event that moves a scenario counter.
///
/// Nothing here is fitted. The byte image is [`RetailInitialScenario`], derived field by
/// field from `ScenarioFuncSet::init`'s instruction stream; the only two free inputs are
/// read positionally out of shipped `internal_strings.xml` at the ordinals that
/// initializer's own `add eax, 0x1d178` / `0x1d18c` name. The recorded wire checksum is
/// never an input — [`CORPUS_INITIAL_SCENARIO_CHANNEL`] is a target to compare against,
/// and the comparison is allowed to fail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitialScenarioChannel {
    pub checksum: u32,
    pub bytes_walked: u64,
    /// `int_str_array[5958]`, as read from the shipped table.
    pub general_powers_script_file: String,
    /// `int_str_array[5959]`.
    pub temp_save: String,
}

impl InitialScenarioChannel {
    /// Bind the two ordinals out of an already-parsed retail string table.
    pub fn from_table(
        table: &don_content::RetailStringTable,
    ) -> Result<InitialScenarioChannel, InternalStringsError> {
        let pick = |ordinal: u32| -> Result<String, InternalStringsError> {
            table.get(ordinal as usize).map(str::to_owned).ok_or(
                InternalStringsError::OrdinalMissing {
                    ordinal,
                    entries: table.len(),
                },
            )
        };
        let general_powers_script_file = pick(INTERNAL_STRING_ORDINAL_GENERAL_POWERS_SCRIPT_FILE)?;
        let temp_save = pick(INTERNAL_STRING_ORDINAL_TEMP_SAVE)?;

        let gp: Vec<u16> = general_powers_script_file.encode_utf16().collect();
        let ts: Vec<u16> = temp_save.encode_utf16().collect();
        let owner = RetailInitialScenario::new();
        let checksum = scenario_checksum(&owner.state(ScenarioInitialStrings {
            general_powers_script_file: Utf16String(&gp),
            temp_save: Utf16String(&ts),
        }))
        .map_err(InternalStringsError::Checksum)?;

        Ok(InitialScenarioChannel {
            checksum: checksum.checksum,
            bytes_walked: checksum.bytes_walked,
            general_powers_script_file,
            temp_save,
        })
    }

    /// Read `<root>/internal_strings.xml` and bind the two ordinals.
    ///
    /// The parse is `don_content::string_table::parse_string_table_xml`, the crate's one
    /// retail-derived positional binder — deliberately not a second parser here. A regex
    /// over `<STRING hash="…">` misses the eight self-closing empty entries and shifts
    /// every ordinal after 5950 by eight, which is exactly the window these two live in.
    pub fn load_from_ron_data(root: &Path) -> Result<InitialScenarioChannel, InternalStringsError> {
        let path = root.join(INTERNAL_STRINGS_FILE);
        if !path.is_file() {
            return Err(InternalStringsError::NotExtracted(path));
        }
        let bytes = std::fs::read(&path).map_err(|error| InternalStringsError::Read {
            path: path.clone(),
            message: error.to_string(),
        })?;
        let text = std::str::from_utf8(&bytes)
            .map_err(|error| InternalStringsError::Encoding {
                path: path.clone(),
                message: error.to_string(),
            })?
            .trim_start_matches('\u{feff}');
        let table = don_content::string_table::parse_string_table_xml(text).map_err(|error| {
            InternalStringsError::Parse {
                path: path.clone(),
                message: format!("{error}"),
            }
        })?;
        Self::from_table(&table)
    }

    /// Whether this derivation reproduces the value every checksum-bearing recording in
    /// the corpus carries on its first checksummed turn.
    pub fn matches_corpus_initial(&self) -> bool {
        self.checksum == CORPUS_INITIAL_SCENARIO_CHANNEL
    }
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

    /// The instruction-derived `ScenarioFuncSet::init` state, pinned. Every one of these
    /// numbers comes from a named store in `0x00a03c30`; if someone edits a field the
    /// byte count or the value moves and this fails.
    #[test]
    fn the_retail_initial_state_is_the_derived_byte_image() {
        let owner = RetailInitialScenario::new();
        let state = owner.state(ScenarioInitialStrings::default());
        let mut bytes = Bytes::default();
        walk_state(&mut bytes, &state);

        // Fixed block: 8,102 bytes, then six String headers, three colors, and the
        // fifteen empty containers plus the `BitMask<8>`.
        assert_eq!(bytes.0.len(), 8_321);
        assert_eq!(
            &bytes.0[..16],
            &[
                0, 0, 0, 0, // load_scenario_script
                0xc8, 0, 0, 0, // msg_time = 200
                0xe0, 0x2e, 0, 0, // game_msg_time = 12000
                0xff, 0xff, 0xff, 0xff, // hilite_option = -1
            ]
        );
        let checksum = scenario_checksum(&state).unwrap();
        assert_eq!(checksum.bytes_walked, 8_321);
        assert_eq!(checksum.checksum, 0xba9c_1111);
    }

    /// The two ordinals are the ones `ScenarioFuncSet::init` indexes, and they are the
    /// only free inputs left in the state.
    ///
    /// With both strings empty the derived state does **not** reproduce the value all 21
    /// checksum-bearing recordings carry — that measurement is retained, because it is
    /// what says the shipped strings are load-bearing rather than decorative. Supplying
    /// them is the whole remaining gap.
    #[test]
    fn the_two_internal_strings_are_the_only_free_inputs() {
        assert_eq!(INTERNAL_STRING_ORDINAL_GENERAL_POWERS_SCRIPT_FILE, 5958);
        assert_eq!(INTERNAL_STRING_ORDINAL_TEMP_SAVE, 5959);

        let owner = RetailInitialScenario::new();
        let empty_strings = scenario_checksum(&owner.state(ScenarioInitialStrings::default()))
            .unwrap()
            .checksum;
        assert_ne!(
            empty_strings, CORPUS_INITIAL_SCENARIO_CHANNEL,
            "the derived state must not reach the corpus value without the shipped strings"
        );

        let name: Vec<u16> = "general_powers".encode_utf16().collect();
        let moved = scenario_checksum(&owner.state(ScenarioInitialStrings {
            general_powers_script_file: Utf16String(&name),
            temp_save: Utf16String(&[]),
        }))
        .unwrap();
        assert_ne!(moved.checksum, empty_strings);
        assert_eq!(moved.bytes_walked, 8_321 + 2 * name.len() as u64);
    }

    /// The binder, without needing the shipped file: a synthetic table whose ordinals
    /// 5958/5959 hold known text must produce exactly the traversal's value for that
    /// text, and neither ordinal may be silently defaulted when the table is short.
    #[test]
    fn the_table_binding_is_positional_and_refuses_a_short_table() {
        let mut xml = String::from("<ROOT internal=\"1\" xml:space=\"preserve\">");
        for ordinal in 0..=INTERNAL_STRING_ORDINAL_TEMP_SAVE {
            let text = match ordinal {
                INTERNAL_STRING_ORDINAL_GENERAL_POWERS_SCRIPT_FILE => "gp/file.bhs",
                INTERNAL_STRING_ORDINAL_TEMP_SAVE => "scratch.svx",
                _ => "",
            };
            xml.push_str(&format!(
                "<STRING hash=\"{ordinal}\" needed=\"1\">{text}</STRING>"
            ));
        }
        let short = don_content::string_table::parse_string_table_xml(&format!("{xml}</ROOT>"))
            .expect("synthetic table parses");
        assert_eq!(
            short.len() as u32,
            INTERNAL_STRING_ORDINAL_TEMP_SAVE + 1,
            "the short table stops one entry past general_powers_script_file"
        );

        let bound = InitialScenarioChannel::from_table(&short).unwrap();
        assert_eq!(bound.general_powers_script_file, "gp/file.bhs");
        assert_eq!(bound.temp_save, "scratch.svx");

        let owner = RetailInitialScenario::new();
        let gp: Vec<u16> = "gp/file.bhs".encode_utf16().collect();
        let ts: Vec<u16> = "scratch.svx".encode_utf16().collect();
        let direct = scenario_checksum(&owner.state(ScenarioInitialStrings {
            general_powers_script_file: Utf16String(&gp),
            temp_save: Utf16String(&ts),
        }))
        .unwrap();
        assert_eq!(bound.checksum, direct.checksum);
        assert_eq!(bound.bytes_walked, direct.bytes_walked);
        assert!(!bound.matches_corpus_initial());

        xml.truncate(xml.rfind("<STRING").unwrap());
        let truncated = don_content::string_table::parse_string_table_xml(&format!("{xml}</ROOT>"))
            .expect("truncated table parses");
        assert_eq!(
            InitialScenarioChannel::from_table(&truncated),
            Err(InternalStringsError::OrdinalMissing {
                ordinal: INTERNAL_STRING_ORDINAL_TEMP_SAVE,
                entries: INTERNAL_STRING_ORDINAL_TEMP_SAVE as usize,
            })
        );
    }

    #[test]
    fn an_unextracted_string_table_is_a_refusal_not_an_empty_channel() {
        let missing = Path::new(env!("CARGO_MANIFEST_DIR")).join("no-such-ron-data");
        assert_eq!(
            InitialScenarioChannel::load_from_ron_data(&missing),
            Err(InternalStringsError::NotExtracted(
                missing.join(INTERNAL_STRINGS_FILE)
            ))
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
