//! Exact checksum-side traversal for the `script_run_time` channel.
//!
//! Retail provenance:
//!
//! - `CheckSums::check_all` `0x00936560` resets a fresh `CheckSum` and calls
//!   `RunTimeEnv::walk_data` `0x009c41a0` for channel 15.
//! - `RunTimeEnv::walk_data` closes transient interpreter state, hashes the signed
//!   32-bit count in the global `ScriptFile::script_files` array at preferred VA
//!   `0x00c8cba0`, and calls `ScriptFile::walk_data` `0x009c63b0` for every entry.
//! - On `CheckSum` (`DataWalk+0x08 != 0`), `ScriptFile::walk_data` includes only
//!   `code`, `scripts`, `const_pool`, and `linked_files`. Source paths, line maps,
//!   breakpoints, timestamps, and flags are deliberately absent.
//!
//! This module models that byte traversal. It does not parse or execute BHS bytecode,
//! invent interpreter state, dereference retail pointers, or reconstruct a script file
//! from source. A caller must provide every checksum-visible pointed-to payload. Missing
//! array capacity, flags, pointer presence, UTF-16 units, dynamic value payload, or bitmask
//! bytes is therefore a type/error boundary rather than a zero-filled fallback.
//!
//! The traversal is instruction-derived but has not been differentially exercised against
//! retail for generated inputs: it is below Tier B. [`checksum_program`] is the live
//! `don-bhs::Program` adapter, and [`crate::state::SimBridge::populate_script_runtime`]
//! installs its result as channel 15. Both reject missing retail-only metadata; the
//! compiler/chunk-loader producer for full shipped programs remains open.

#![forbid(unsafe_code)]

use std::fmt;

use don_bhs::program::{
    ArrayWalkMeta as ProgramArrayWalkMeta, Program as BhsProgram, Script as BhsScript,
    ScriptFile as BhsScriptFile, ScriptFileWalkMeta as BhsScriptFileWalkMeta,
    ScriptWalkMeta as BhsScriptWalkMeta, ValueWalkMeta, ValueWalkNested,
};
use don_bhs::value::Value;

/// Metadata hashed by retail's non-empty `SimpleArray`, `ObjectArray`, and `PtrArray`
/// walkers. `flags` is stored unmasked; the walk emits `flags & 0xbf` exactly as retail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArrayShape {
    pub capacity: i32,
    pub grow: u16,
    pub flags: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SimpleU8Array<'a> {
    pub shape: ArrayShape,
    pub elements: &'a [u8],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SimpleI32Array<'a> {
    pub shape: ArrayShape,
    pub elements: &'a [i32],
}

/// A retail `String` represented as its exact checksum-visible UTF-16 code units.
/// `String::walk_data` hashes a zero-extended 32-bit length followed by these units.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WalkString<'a> {
    pub utf16: &'a [u16],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StringArray<'a> {
    pub shape: ArrayShape,
    pub elements: &'a [WalkString<'a>],
}

/// The exact bytes selected by `Script::walk_data` for `DynamicBitMask`.
///
/// Retail hashes `bits`, then `size`, then `size` bytes at `ptr` when `size != 0`.
/// Keeping `size` explicit makes a torn or incomplete capture rejectable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DynamicBitMask<'a> {
    pub bits: i32,
    pub size: i32,
    pub bytes: &'a [u8],
}

/// Fields common to every non-null `ScriptType` value.
///
/// `scope` is the low 16 bits read at `ScriptType+0x08`. Retail calls vtable slot
/// `+0x24` (`is_array`) and ORs bit `0x80` into the walked copy when it returns true.
/// It then emits `ref_count` from `ScriptType+0x0c` before calling the most-derived
/// `walk_data` vtable slot. [measured, 0x009d7f2d..0x009d7f7d]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptValueBase {
    pub data_type: i32,
    pub scope: u16,
    pub is_array: bool,
    pub ref_count: u16,
}

/// Checksum-visible dynamic shapes reached through `ScriptType::walk_base` `0x009d7ea0`.
///
/// `Object` also covers subclasses which inherit `ScriptObject::walk_data`. `Base` is the
/// no-payload `ScriptType::walk_data` implementation. Floating-point state is carried as
/// raw bits so the primitive never canonicalises a NaN.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptValue<'a> {
    Null,
    Base(ScriptValueBase),
    Int {
        base: ScriptValueBase,
        value: i32,
    },
    FloatBits {
        base: ScriptValueBase,
        bits: u32,
    },
    String {
        base: ScriptValueBase,
        value: WalkString<'a>,
    },
    Object {
        base: ScriptValueBase,
        values: &'a [ScriptValue<'a>],
    },
    Array {
        base: ScriptValueBase,
        blank_base: &'a ScriptValue<'a>,
        values: &'a [ScriptValue<'a>],
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Script<'a> {
    pub static_vars: &'a [ScriptValue<'a>],
    pub trigger_bits: DynamicBitMask<'a>,
    pub params: SimpleI32Array<'a>,
    pub refs: SimpleU8Array<'a>,
    pub trigger_names: StringArray<'a>,
    pub var_names: StringArray<'a>,
    pub static_var_names: StringArray<'a>,
    pub name: WalkString<'a>,
    pub offset: i32,
    pub return_type: i32,
    pub script_type: i32,
}

/// Retail `PtrArray<Script>` state. Every `None` emits a zero presence byte; every
/// `Some` emits one. Capacity and growth are intentionally emitted twice by the shipped
/// specialization: once before the presence vector and once before the script bodies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptPtrArray<'a> {
    pub shape: ArrayShape,
    pub elements: &'a [Option<Script<'a>>],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptFile<'a> {
    pub code: SimpleU8Array<'a>,
    pub scripts: ScriptPtrArray<'a>,
    pub const_pool: &'a [ScriptValue<'a>],
    pub linked_files: SimpleI32Array<'a>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptRuntime<'a> {
    /// Entries in global `ScriptFile::script_files` order. Retail dereferences every
    /// pointer unconditionally, so this API deliberately has no nullable root entry.
    pub script_files: &'a [ScriptFile<'a>],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptChannelChecksum {
    pub checksum: u32,
    pub bytes_walked: u64,
    pub script_files: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptChannelError {
    MissingProgramWalkMetadata,
    ProjectionLengthMismatch {
        what: &'static str,
        live: usize,
        metadata: usize,
    },
    MissingValueWalkMetadata {
        what: &'static str,
    },
    UnexpectedValueWalkMetadata {
        what: &'static str,
    },
    ValueShapeMismatch {
        what: &'static str,
        live: &'static str,
        metadata: &'static str,
    },
    LinkedFileIndexMismatch {
        slot: usize,
        live: usize,
        metadata: i32,
    },
    CountDoesNotFitI32 {
        what: &'static str,
        actual: usize,
    },
    StringDoesNotFitU16 {
        actual: usize,
    },
    NegativeCapacity {
        what: &'static str,
        capacity: i32,
    },
    CapacityBelowCount {
        what: &'static str,
        capacity: i32,
        count: usize,
    },
    NegativeBitMaskSize {
        size: i32,
    },
    BitMaskLengthMismatch {
        declared: i32,
        actual: usize,
    },
}

impl fmt::Display for ScriptChannelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for ScriptChannelError {}

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

    fn update(&mut self, bytes: &[u8]) {
        // The arithmetic is `don_sim::checksum::adler32`, the workspace's only
        // implementation of `0x00a46830`. This struct is only the running `CheckSum+0x10`
        // / `+0x14` pair the channel carries.
        self.bytes += bytes.len() as u64;
        let sum = don_sim::checksum::adler32((self.s2 << 16) | self.s1, bytes);
        self.s1 = sum & 0xFFFF;
        self.s2 = sum >> 16;
    }

    fn value(self) -> u32 {
        (self.s2 << 16) | self.s1
    }
}

trait WalkSink {
    fn walk(&mut self, bytes: &[u8]);
}

impl WalkSink for Adler32 {
    fn walk(&mut self, bytes: &[u8]) {
        self.update(bytes);
    }
}

fn checked_count(what: &'static str, count: usize) -> Result<i32, ScriptChannelError> {
    i32::try_from(count).map_err(|_| ScriptChannelError::CountDoesNotFitI32 {
        what,
        actual: count,
    })
}

fn validate_shape(
    what: &'static str,
    shape: ArrayShape,
    count: usize,
) -> Result<(), ScriptChannelError> {
    let count_i32 = checked_count(what, count)?;
    if shape.capacity < 0 {
        return Err(ScriptChannelError::NegativeCapacity {
            what,
            capacity: shape.capacity,
        });
    }
    if shape.capacity < count_i32 {
        return Err(ScriptChannelError::CapacityBelowCount {
            what,
            capacity: shape.capacity,
            count,
        });
    }
    Ok(())
}

fn walk_i32<S: WalkSink>(sink: &mut S, value: i32) {
    sink.walk(&value.to_le_bytes());
}

fn walk_u16<S: WalkSink>(sink: &mut S, value: u16) {
    sink.walk(&value.to_le_bytes());
}

fn walk_array_header<S: WalkSink>(sink: &mut S, shape: ArrayShape) {
    walk_i32(sink, shape.capacity);
    walk_u16(sink, shape.grow);
    sink.walk(&[shape.flags & 0xbf]);
}

fn walk_simple_u8<S: WalkSink>(
    sink: &mut S,
    what: &'static str,
    array: SimpleU8Array<'_>,
) -> Result<(), ScriptChannelError> {
    let count = checked_count(what, array.elements.len())?;
    walk_i32(sink, count);
    if count != 0 {
        validate_shape(what, array.shape, array.elements.len())?;
        walk_array_header(sink, array.shape);
        sink.walk(array.elements);
    }
    Ok(())
}

fn walk_simple_i32<S: WalkSink>(
    sink: &mut S,
    what: &'static str,
    array: SimpleI32Array<'_>,
) -> Result<(), ScriptChannelError> {
    let count = checked_count(what, array.elements.len())?;
    walk_i32(sink, count);
    if count != 0 {
        validate_shape(what, array.shape, array.elements.len())?;
        walk_array_header(sink, array.shape);
        for &value in array.elements {
            walk_i32(sink, value);
        }
    }
    Ok(())
}

fn walk_string<S: WalkSink>(sink: &mut S, value: WalkString<'_>) -> Result<(), ScriptChannelError> {
    let len =
        u16::try_from(value.utf16.len()).map_err(|_| ScriptChannelError::StringDoesNotFitU16 {
            actual: value.utf16.len(),
        })?;
    walk_i32(sink, i32::from(len));
    for &unit in value.utf16 {
        walk_u16(sink, unit);
    }
    Ok(())
}

fn walk_string_array<S: WalkSink>(
    sink: &mut S,
    what: &'static str,
    array: StringArray<'_>,
) -> Result<(), ScriptChannelError> {
    let count = checked_count(what, array.elements.len())?;
    walk_i32(sink, count);
    if count != 0 {
        validate_shape(what, array.shape, array.elements.len())?;
        walk_array_header(sink, array.shape);
        for &value in array.elements {
            walk_string(sink, value)?;
        }
    }
    Ok(())
}

fn walk_script_values<S: WalkSink>(
    sink: &mut S,
    what: &'static str,
    values: &[ScriptValue<'_>],
) -> Result<(), ScriptChannelError> {
    walk_i32(sink, checked_count(what, values.len())?);
    for value in values {
        walk_script_value(sink, value)?;
    }
    Ok(())
}

fn walk_script_value_base<S: WalkSink>(sink: &mut S, base: ScriptValueBase) {
    walk_i32(sink, base.data_type);
    let walked_scope = if base.is_array {
        base.scope | 0x0080
    } else {
        base.scope
    };
    walk_u16(sink, walked_scope);
    walk_u16(sink, base.ref_count);
}

fn walk_script_value<S: WalkSink>(
    sink: &mut S,
    value: &ScriptValue<'_>,
) -> Result<(), ScriptChannelError> {
    match value {
        ScriptValue::Null => walk_i32(sink, 0),
        ScriptValue::Base(base) => walk_script_value_base(sink, *base),
        ScriptValue::Int { base, value } => {
            walk_script_value_base(sink, *base);
            walk_i32(sink, *value);
        }
        ScriptValue::FloatBits { base, bits } => {
            walk_script_value_base(sink, *base);
            sink.walk(&bits.to_le_bytes());
        }
        ScriptValue::String { base, value } => {
            walk_script_value_base(sink, *base);
            walk_string(sink, *value)?;
        }
        ScriptValue::Object { base, values } => {
            walk_script_value_base(sink, *base);
            walk_script_values(sink, "ScriptObject.values", values)?;
        }
        ScriptValue::Array {
            base,
            blank_base,
            values,
        } => {
            walk_script_value_base(sink, *base);
            walk_script_value(sink, blank_base)?;
            walk_script_values(sink, "ScriptArray.values", values)?;
        }
    }
    Ok(())
}

fn walk_dynamic_bit_mask<S: WalkSink>(
    sink: &mut S,
    mask: DynamicBitMask<'_>,
) -> Result<(), ScriptChannelError> {
    if mask.size < 0 {
        return Err(ScriptChannelError::NegativeBitMaskSize { size: mask.size });
    }
    if usize::try_from(mask.size).ok() != Some(mask.bytes.len()) {
        return Err(ScriptChannelError::BitMaskLengthMismatch {
            declared: mask.size,
            actual: mask.bytes.len(),
        });
    }
    walk_i32(sink, mask.bits);
    walk_i32(sink, mask.size);
    if mask.size != 0 {
        sink.walk(mask.bytes);
    }
    Ok(())
}

fn walk_script<S: WalkSink>(sink: &mut S, script: &Script<'_>) -> Result<(), ScriptChannelError> {
    // Script::walk_data 0x009c5f30, instruction order.
    walk_script_values(sink, "Script.static_vars", script.static_vars)?;
    walk_dynamic_bit_mask(sink, script.trigger_bits)?;
    walk_simple_i32(sink, "Script.params", script.params)?;
    walk_simple_u8(sink, "Script.refs", script.refs)?;
    walk_string_array(sink, "Script.trigger_names", script.trigger_names)?;
    walk_string_array(sink, "Script.var_names", script.var_names)?;
    walk_string_array(sink, "Script.static_var_names", script.static_var_names)?;
    walk_string(sink, script.name)?;
    walk_i32(sink, script.offset);
    walk_i32(sink, script.return_type);
    walk_i32(sink, script.script_type);
    Ok(())
}

fn walk_script_array<S: WalkSink>(
    sink: &mut S,
    array: ScriptPtrArray<'_>,
) -> Result<(), ScriptChannelError> {
    let count = checked_count("ScriptFile.scripts", array.elements.len())?;
    walk_i32(sink, count);
    if count == 0 {
        return Ok(());
    }

    validate_shape("ScriptFile.scripts", array.shape, array.elements.len())?;
    walk_array_header(sink, array.shape);
    for script in array.elements {
        sink.walk(&[u8::from(script.is_some())]);
    }

    // PtrArray<Script>::walk_data `0x004cccd0` walks `[array+0x08,array+0x0e)`
    // again after the presence vector. The flags byte is not repeated.
    walk_i32(sink, array.shape.capacity);
    walk_u16(sink, array.shape.grow);
    for script in array.elements.iter().flatten() {
        walk_script(sink, script)?;
    }
    Ok(())
}

fn walk_script_file<S: WalkSink>(
    sink: &mut S,
    file: &ScriptFile<'_>,
) -> Result<(), ScriptChannelError> {
    // ScriptFile::walk_data 0x009c63b0. CheckSum's is_checksum flag is one, so the
    // debug/source branch beginning at 0x009c6400 is skipped.
    walk_simple_u8(sink, "ScriptFile.code", file.code)?;
    walk_script_array(sink, file.scripts)?;
    walk_script_values(sink, "ScriptFile.const_pool", file.const_pool)?;
    walk_simple_i32(sink, "ScriptFile.linked_files", file.linked_files)?;
    Ok(())
}

fn walk_runtime<S: WalkSink>(
    sink: &mut S,
    runtime: &ScriptRuntime<'_>,
) -> Result<(), ScriptChannelError> {
    // RunTimeEnv::walk_data 0x009c41a0 walks only the global array's count before
    // dereferencing every ScriptFile pointer in order. No outer capacity/presence bytes.
    walk_i32(
        sink,
        checked_count("ScriptFile::script_files", runtime.script_files.len())?,
    );
    for file in runtime.script_files {
        walk_script_file(sink, file)?;
    }
    Ok(())
}

/// Compute the checksum channel from a complete checksum-visible runtime projection.
pub fn checksum_script_runtime(
    runtime: &ScriptRuntime<'_>,
) -> Result<ScriptChannelChecksum, ScriptChannelError> {
    let mut adler = Adler32::new();
    walk_runtime(&mut adler, runtime)?;
    Ok(ScriptChannelChecksum {
        checksum: adler.value(),
        bytes_walked: adler.bytes,
        script_files: runtime.script_files.len(),
    })
}

fn array_shape(meta: ProgramArrayWalkMeta) -> ArrayShape {
    ArrayShape {
        capacity: meta.capacity,
        grow: meta.grow,
        flags: meta.flags,
    }
}

fn require_parallel_len(
    what: &'static str,
    live: usize,
    metadata: usize,
) -> Result<(), ScriptChannelError> {
    if live == metadata {
        Ok(())
    } else {
        Err(ScriptChannelError::ProjectionLengthMismatch {
            what,
            live,
            metadata,
        })
    }
}

fn walk_rust_string<S: WalkSink>(sink: &mut S, value: &str) -> Result<(), ScriptChannelError> {
    let units: Vec<u16> = value.encode_utf16().collect();
    walk_string(sink, WalkString { utf16: &units })
}

fn walk_rust_string_array<S: WalkSink>(
    sink: &mut S,
    what: &'static str,
    shape: ProgramArrayWalkMeta,
    values: &[String],
) -> Result<(), ScriptChannelError> {
    let count = checked_count(what, values.len())?;
    walk_i32(sink, count);
    if count != 0 {
        validate_shape(what, array_shape(shape), values.len())?;
        walk_array_header(sink, array_shape(shape));
        for value in values {
            walk_rust_string(sink, value)?;
        }
    }
    Ok(())
}

fn nested_name(nested: &ValueWalkNested) -> &'static str {
    match nested {
        ValueWalkNested::Scalar => "scalar",
        ValueWalkNested::Object { .. } => "object",
        ValueWalkNested::Array { .. } => "array",
    }
}

fn walk_bhs_value<S: WalkSink>(
    sink: &mut S,
    what: &'static str,
    value: Option<&Value>,
    meta: Option<&ValueWalkMeta>,
) -> Result<(), ScriptChannelError> {
    let value = match value {
        None | Some(Value::Null) => {
            if meta.is_some() {
                return Err(ScriptChannelError::UnexpectedValueWalkMetadata { what });
            }
            walk_i32(sink, 0);
            return Ok(());
        }
        Some(value) => value,
    };
    let meta = meta.ok_or(ScriptChannelError::MissingValueWalkMetadata { what })?;
    let data_type = value.data_type();
    let scalar_base = |data_type: u32| ScriptValueBase {
        data_type: data_type as i32,
        scope: meta.scope,
        is_array: false,
        ref_count: meta.ref_count,
    };

    match value {
        Value::Int(payload) => {
            if !matches!(meta.nested, ValueWalkNested::Scalar) {
                return Err(ScriptChannelError::ValueShapeMismatch {
                    what,
                    live: "int",
                    metadata: nested_name(&meta.nested),
                });
            }
            walk_script_value_base(sink, scalar_base(data_type));
            walk_i32(sink, *payload);
        }
        Value::Real(payload) => {
            if !matches!(meta.nested, ValueWalkNested::Scalar) {
                return Err(ScriptChannelError::ValueShapeMismatch {
                    what,
                    live: "real",
                    metadata: nested_name(&meta.nested),
                });
            }
            walk_script_value_base(sink, scalar_base(data_type));
            sink.walk(&payload.to_bits().to_le_bytes());
        }
        Value::Str(payload) => {
            if !matches!(meta.nested, ValueWalkNested::Scalar) {
                return Err(ScriptChannelError::ValueShapeMismatch {
                    what,
                    live: "string",
                    metadata: nested_name(&meta.nested),
                });
            }
            walk_script_value_base(sink, scalar_base(data_type));
            walk_rust_string(sink, payload)?;
        }
        Value::Obj(value) => {
            let value = value.borrow();
            let base = ScriptValueBase {
                data_type: value.data_type as i32,
                scope: meta.scope,
                is_array: value.is_array,
                ref_count: meta.ref_count,
            };
            walk_script_value_base(sink, base);
            if value.is_array {
                let (blank_meta, value_meta) = match &meta.nested {
                    ValueWalkNested::Array { blank_base, values } => {
                        (blank_base.as_deref(), values)
                    }
                    nested => {
                        return Err(ScriptChannelError::ValueShapeMismatch {
                            what,
                            live: "array",
                            metadata: nested_name(nested),
                        });
                    }
                };
                let blank =
                    value
                        .blank_base
                        .as_deref()
                        .ok_or(ScriptChannelError::ValueShapeMismatch {
                            what,
                            live: "array-without-blank-base",
                            metadata: "array",
                        })?;
                walk_bhs_value(sink, "ScriptArray.blank_base", Some(blank), blank_meta)?;
                require_parallel_len("ScriptArray.values", value.values.len(), value_meta.len())?;
                walk_i32(
                    sink,
                    checked_count("ScriptArray.values", value.values.len())?,
                );
                for (slot, slot_meta) in value.values.iter().zip(value_meta) {
                    let slot = slot.borrow();
                    walk_bhs_value(
                        sink,
                        "ScriptArray.values[]",
                        Some(&slot),
                        slot_meta.as_ref(),
                    )?;
                }
            } else {
                let value_meta = match &meta.nested {
                    ValueWalkNested::Object { values } => values,
                    nested => {
                        return Err(ScriptChannelError::ValueShapeMismatch {
                            what,
                            live: "object",
                            metadata: nested_name(nested),
                        });
                    }
                };
                require_parallel_len("ScriptObject.values", value.values.len(), value_meta.len())?;
                walk_i32(
                    sink,
                    checked_count("ScriptObject.values", value.values.len())?,
                );
                for (slot, slot_meta) in value.values.iter().zip(value_meta) {
                    let slot = slot.borrow();
                    walk_bhs_value(
                        sink,
                        "ScriptObject.values[]",
                        Some(&slot),
                        slot_meta.as_ref(),
                    )?;
                }
            }
        }
        Value::Null => unreachable!("null returned above"),
    }
    Ok(())
}

fn walk_bhs_values<'a, S: WalkSink>(
    sink: &mut S,
    what: &'static str,
    values: impl IntoIterator<Item = Option<&'a Value>>,
    len: usize,
    metadata: &[Option<ValueWalkMeta>],
) -> Result<(), ScriptChannelError> {
    require_parallel_len(what, len, metadata.len())?;
    walk_i32(sink, checked_count(what, len)?);
    for (value, meta) in values.into_iter().zip(metadata) {
        walk_bhs_value(sink, what, value, meta.as_ref())?;
    }
    Ok(())
}

fn walk_bhs_script<S: WalkSink>(
    sink: &mut S,
    script: &BhsScript,
    meta: &BhsScriptWalkMeta,
) -> Result<(), ScriptChannelError> {
    walk_bhs_values(
        sink,
        "Script.static_vars",
        script.statics.iter().map(Option::as_ref),
        script.statics.len(),
        &meta.statics,
    )?;
    walk_dynamic_bit_mask(
        sink,
        DynamicBitMask {
            bits: script.trigger_count,
            size: checked_count("Script.trigger_bits", script.trigger_bits.len())?,
            bytes: &script.trigger_bits,
        },
    )?;

    let params: Vec<i32> = script.params.iter().map(|&value| value as i32).collect();
    walk_simple_i32(
        sink,
        "Script.params",
        SimpleI32Array {
            shape: array_shape(meta.params),
            elements: &params,
        },
    )?;
    walk_simple_u8(
        sink,
        "Script.refs",
        SimpleU8Array {
            shape: array_shape(meta.refs),
            elements: &script.refs,
        },
    )?;
    walk_rust_string_array(
        sink,
        "Script.trigger_names",
        meta.trigger_names,
        &script.trigger_names,
    )?;
    walk_rust_string_array(sink, "Script.var_names", meta.var_names, &script.var_names)?;
    walk_rust_string_array(
        sink,
        "Script.static_var_names",
        meta.static_var_names,
        &script.static_var_names,
    )?;
    walk_rust_string(sink, &script.name)?;
    walk_i32(sink, script.entry as i32);
    walk_i32(sink, script.return_type as i32);
    walk_i32(sink, script.script_type as i32);
    Ok(())
}

fn walk_bhs_file<S: WalkSink>(
    sink: &mut S,
    file: &BhsScriptFile,
    meta: &BhsScriptFileWalkMeta,
    live_link_indices: &[usize],
) -> Result<(), ScriptChannelError> {
    walk_simple_u8(
        sink,
        "ScriptFile.code",
        SimpleU8Array {
            shape: array_shape(meta.code),
            elements: &file.code,
        },
    )?;

    require_parallel_len(
        "ScriptFile.scripts",
        file.scripts.len(),
        meta.script_meta.len(),
    )?;
    let count = checked_count("ScriptFile.scripts", file.scripts.len())?;
    walk_i32(sink, count);
    if count != 0 {
        validate_shape(
            "ScriptFile.scripts",
            array_shape(meta.scripts),
            file.scripts.len(),
        )?;
        walk_array_header(sink, array_shape(meta.scripts));
        for _ in &file.scripts {
            sink.walk(&[1]);
        }
        walk_i32(sink, meta.scripts.capacity);
        walk_u16(sink, meta.scripts.grow);
        for (script, script_meta) in file.scripts.iter().zip(&meta.script_meta) {
            walk_bhs_script(sink, script, script_meta)?;
        }
    }

    walk_bhs_values(
        sink,
        "ScriptFile.const_pool",
        file.const_pool.iter().map(Some),
        file.const_pool.len(),
        &meta.const_pool,
    )?;
    walk_simple_i32(
        sink,
        "ScriptFile.linked_files",
        SimpleI32Array {
            shape: array_shape(meta.linked_files),
            elements: &meta.linked_file_indices,
        },
    )?;
    require_parallel_len(
        "ScriptFile.linked_files",
        live_link_indices.len(),
        meta.linked_file_indices.len(),
    )?;
    require_parallel_len(
        "ScriptFile.linked_file_names",
        file.linked_file_names.len(),
        live_link_indices.len(),
    )?;
    for (slot, (&live, &metadata)) in live_link_indices
        .iter()
        .zip(&meta.linked_file_indices)
        .enumerate()
    {
        if usize::try_from(metadata).ok() != Some(live) {
            return Err(ScriptChannelError::LinkedFileIndexMismatch {
                slot,
                live,
                metadata,
            });
        }
    }
    Ok(())
}

/// Channel 15 for a simulation that has loaded **no** `ScriptFile`.
///
/// This is not a fallback and not an "assume empty": it is the complete retail
/// traversal for a state whose `ScriptFile::script_files` array is empty, read off the
/// instruction stream of `RunTimeEnv::walk_data` `0x009c41a0`:
///
/// ```text
/// walk_tag(int_str_array[0x231b8 / 0x14])      ; no-op for CheckSum
/// RunTimeEnv::close()                          ; frees transient interpreter state,
///                                              ; emits nothing
/// if (DataWalk+0x04 == 0):                     ; the write/checksum direction
///     local = ScriptFile::script_files.length  ; signed 32-bit at 0x00c8cba4
///     walk(&local, &local + 4)                 ; four bytes, always
///     for i in 0..local: ScriptFile::walk_data ; 0x009c63b0, none when local == 0
/// ```
///
/// So the walk is unconditionally four bytes of the element count, and every further
/// byte is per-`ScriptFile`. `PtrArray` capacity/grow/flags are **not** hashed here —
/// only the `int` count is — which is why an empty runtime is exactly
/// `adler32(1, [0,0,0,0])`.
///
/// The claim this producer makes is falsifiable and routinely false: a recording whose
/// engine did load script files disagrees on its first checksummed turn. It carries no
/// evidence about BHS program *semantics*; it is a statement about the container header
/// and about our world holding no programs.
pub fn checksum_empty_runtime() -> ScriptChannelChecksum {
    let mut adler = Adler32::new();
    walk_i32(&mut adler, 0);
    ScriptChannelChecksum {
        checksum: adler.value(),
        bytes_walked: adler.bytes,
        script_files: 0,
    }
}

/// The value [`checksum_empty_runtime`] produces, and the value 7 of the 21
/// checksum-bearing corpus recordings carry on **every** checksummed turn. Recorded
/// independently of this code as `channels.script_run_time.expected` in
/// `schema/replay-validation.json` before any producer existed.
pub const EMPTY_RUNTIME_CHANNEL: u32 = 0x0004_0001;

/// Project the authoritative `don-bhs` program state onto retail checksum channel 15.
///
/// Logical payloads come from the live VM-owned [`BhsProgram`]. Container headers and
/// `ScriptType` ownership fields come from its independently recovered sidecar. Any
/// missing or stale parallel state is rejected before a checksum can be installed.
pub fn checksum_program(program: &BhsProgram) -> Result<ScriptChannelChecksum, ScriptChannelError> {
    let meta = program
        .walk_meta()
        .ok_or(ScriptChannelError::MissingProgramWalkMetadata)?;
    require_parallel_len("Program.files", program.files.len(), meta.files.len())?;

    let mut adler = Adler32::new();
    walk_i32(
        &mut adler,
        checked_count("ScriptFile::script_files", program.files.len())?,
    );
    for (index, (file, file_meta)) in program.files.iter().zip(&meta.files).enumerate() {
        walk_bhs_file(
            &mut adler,
            file,
            file_meta,
            program.resolved_links(index).unwrap_or(&[]),
        )?;
    }
    Ok(ScriptChannelChecksum {
        checksum: adler.value(),
        bytes_walked: adler.bytes,
        script_files: program.files.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    use don_bhs::host::NullHost;
    use don_bhs::program::{ProgramWalkMeta, ScriptFileWalkMeta, ScriptWalkMeta};
    use don_bhs::vm::Vm;
    use don_bhs_cc::sema::{self, Severity};

    #[derive(Default)]
    struct Bytes(Vec<u8>);

    impl WalkSink for Bytes {
        fn walk(&mut self, bytes: &[u8]) {
            self.0.extend_from_slice(bytes);
        }
    }

    /// The empty-runtime walk is exactly four zero bytes, and the value it produces
    /// is the one the corpus carries. Any extra header byte, or a count that is not
    /// walked at all, changes it.
    #[test]
    fn the_empty_run_time_env_walks_exactly_the_four_count_bytes() {
        let empty = checksum_empty_runtime();
        assert_eq!(empty.bytes_walked, 4);
        assert_eq!(empty.script_files, 0);
        assert_eq!(empty.checksum, EMPTY_RUNTIME_CHANNEL);
        assert_eq!(empty.checksum, crate::checksum::adler32(1, &[0, 0, 0, 0]));

        // The two adjacent mistakes this pins against: walking nothing (which is what
        // an absent producer does, and reads 1), and walking the `PtrArray` header.
        assert_ne!(empty.checksum, 1);
        assert_ne!(
            empty.checksum,
            crate::checksum::adler32(1, &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
        );
    }

    /// A single loaded file must move the channel; otherwise the count is not really
    /// being hashed and the empty agreement would be vacuous.
    #[test]
    fn one_loaded_script_file_moves_the_channel() {
        let mut program = captured_empty_main_program();
        let loaded = checksum_program(&program).expect("captured program walks");
        assert_ne!(loaded.checksum, checksum_empty_runtime().checksum);
        assert!(loaded.bytes_walked > 4);
        program.files.clear();
        if let Some(meta) = program.walk_meta_mut() {
            meta.files.clear();
        }
        assert_eq!(
            checksum_program(&program).expect("empty file list walks"),
            checksum_empty_runtime()
        );
    }

    const EMPTY_SHAPE: ArrayShape = ArrayShape {
        capacity: 0,
        grow: 0,
        flags: 0,
    };

    fn empty_u8() -> SimpleU8Array<'static> {
        SimpleU8Array {
            shape: EMPTY_SHAPE,
            elements: &[],
        }
    }

    fn empty_i32() -> SimpleI32Array<'static> {
        SimpleI32Array {
            shape: EMPTY_SHAPE,
            elements: &[],
        }
    }

    fn empty_strings() -> StringArray<'static> {
        StringArray {
            shape: EMPTY_SHAPE,
            elements: &[],
        }
    }

    #[test]
    fn empty_runtime_still_walks_the_root_i32_count() {
        let runtime = ScriptRuntime { script_files: &[] };
        let mut bytes = Bytes::default();
        walk_runtime(&mut bytes, &runtime).unwrap();
        assert_eq!(bytes.0, [0, 0, 0, 0]);

        let result = checksum_script_runtime(&runtime).unwrap();
        assert_eq!(result.bytes_walked, 4);
        assert_eq!(result.script_files, 0);
    }

    #[test]
    fn instruction_order_and_little_endian_widths_are_explicit() {
        let null = ScriptValue::Null;
        let static_vars = [null];
        let script = Script {
            static_vars: &static_vars,
            trigger_bits: DynamicBitMask {
                bits: 0x1122_3344,
                size: 1,
                bytes: &[0x5a],
            },
            params: empty_i32(),
            refs: empty_u8(),
            trigger_names: empty_strings(),
            var_names: empty_strings(),
            static_var_names: empty_strings(),
            name: WalkString { utf16: &[0x0041] },
            offset: 0x0102_0304,
            return_type: -2,
            script_type: 7,
        };
        let scripts = [Some(script)];
        let const_pool = [ScriptValue::Int {
            base: ScriptValueBase {
                data_type: 2,
                scope: 0x0102,
                is_array: true,
                ref_count: 0x3344,
            },
            value: 0x5566_7788,
        }];
        let file = ScriptFile {
            code: SimpleU8Array {
                shape: ArrayShape {
                    capacity: 2,
                    grow: 0x3344,
                    flags: 0xff,
                },
                elements: &[0xaa],
            },
            scripts: ScriptPtrArray {
                shape: ArrayShape {
                    capacity: 2,
                    grow: 0x5566,
                    flags: 0xc1,
                },
                elements: &scripts,
            },
            const_pool: &const_pool,
            linked_files: SimpleI32Array {
                shape: ArrayShape {
                    capacity: 2,
                    grow: 0x7788,
                    flags: 0x40,
                },
                elements: &[0x1020_3040],
            },
        };
        let files = [file];
        let runtime = ScriptRuntime {
            script_files: &files,
        };

        let mut bytes = Bytes::default();
        walk_runtime(&mut bytes, &runtime).unwrap();
        assert_eq!(
            bytes.0,
            [
                // root count; code SimpleArray
                1, 0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0, 0x44, 0x33, 0xbf, 0xaa,
                // scripts PtrArray header, presence, duplicated capacity/grow
                1, 0, 0, 0, 2, 0, 0, 0, 0x66, 0x55, 0x81, 1, 2, 0, 0, 0, 0x66, 0x55,
                // Script: static vars, trigger mask, params, refs, three string arrays
                1, 0, 0, 0, 0, 0, 0, 0, 0x44, 0x33, 0x22, 0x11, 1, 0, 0, 0, 0x5a, 0, 0, 0,
                0, // params count
                0, 0, 0, 0, // refs count
                0, 0, 0, 0, // trigger_names count
                0, 0, 0, 0, // var_names count
                0, 0, 0, 0, // static_var_names count
                // name and the three trailing Script ints
                1, 0, 0, 0, 0x41, 0, 4, 3, 2, 1, 0xfe, 0xff, 0xff, 0xff, 7, 0, 0, 0,
                // const pool Int: count, discriminator/base, payload
                1, 0, 0, 0, 2, 0, 0, 0, 0x82, 1, 0x44, 0x33, 0x88, 0x77, 0x66, 0x55,
                // linked_files SimpleArray; transient flag bit 0x40 is masked out
                1, 0, 0, 0, 2, 0, 0, 0, 0x88, 0x77, 0, 0x40, 0x30, 0x20, 0x10,
            ]
        );
    }

    #[test]
    fn one_checksum_visible_byte_mutation_is_observable() {
        let file_a = ScriptFile {
            code: SimpleU8Array {
                shape: ArrayShape {
                    capacity: 1,
                    grow: 1,
                    flags: 0,
                },
                elements: &[0x38],
            },
            scripts: ScriptPtrArray {
                shape: EMPTY_SHAPE,
                elements: &[],
            },
            const_pool: &[],
            linked_files: empty_i32(),
        };
        let file_b = ScriptFile {
            code: SimpleU8Array {
                elements: &[0x39],
                ..file_a.code
            },
            ..file_a
        };
        let files_a = [file_a];
        let files_b = [file_b];
        let a = checksum_script_runtime(&ScriptRuntime {
            script_files: &files_a,
        })
        .unwrap();
        let b = checksum_script_runtime(&ScriptRuntime {
            script_files: &files_b,
        })
        .unwrap();
        assert_ne!(a.checksum, b.checksum);
        assert_eq!(a.bytes_walked, b.bytes_walked);
    }

    #[test]
    fn incomplete_capture_is_rejected_instead_of_zero_filled() {
        let file = ScriptFile {
            code: SimpleU8Array {
                shape: ArrayShape {
                    capacity: 0,
                    grow: 0,
                    flags: 0,
                },
                elements: &[1],
            },
            scripts: ScriptPtrArray {
                shape: EMPTY_SHAPE,
                elements: &[],
            },
            const_pool: &[],
            linked_files: empty_i32(),
        };
        let files = [file];
        assert_eq!(
            checksum_script_runtime(&ScriptRuntime {
                script_files: &files,
            }),
            Err(ScriptChannelError::CapacityBelowCount {
                what: "ScriptFile.code",
                capacity: 0,
                count: 1,
            })
        );

        let script = Script {
            static_vars: &[],
            trigger_bits: DynamicBitMask {
                bits: 1,
                size: 2,
                bytes: &[0xff],
            },
            params: empty_i32(),
            refs: empty_u8(),
            trigger_names: empty_strings(),
            var_names: empty_strings(),
            static_var_names: empty_strings(),
            name: WalkString { utf16: &[] },
            offset: 0,
            return_type: 0,
            script_type: 0,
        };
        let scripts = [Some(script)];
        let file = ScriptFile {
            code: empty_u8(),
            scripts: ScriptPtrArray {
                shape: ArrayShape {
                    capacity: 1,
                    grow: 0,
                    flags: 0,
                },
                elements: &scripts,
            },
            const_pool: &[],
            linked_files: empty_i32(),
        };
        let files = [file];
        assert_eq!(
            checksum_script_runtime(&ScriptRuntime {
                script_files: &files,
            }),
            Err(ScriptChannelError::BitMaskLengthMismatch {
                declared: 2,
                actual: 1,
            })
        );
    }

    #[test]
    fn float_payload_preserves_raw_nan_bits() {
        let a = ScriptValue::FloatBits {
            base: ScriptValueBase {
                data_type: 3,
                scope: 0,
                is_array: false,
                ref_count: 1,
            },
            bits: 0x7fc0_0001,
        };
        let b = ScriptValue::FloatBits {
            base: ScriptValueBase {
                data_type: 3,
                scope: 0,
                is_array: false,
                ref_count: 1,
            },
            bits: 0x7fc0_0002,
        };
        let mut bytes_a = Bytes::default();
        let mut bytes_b = Bytes::default();
        walk_script_value(&mut bytes_a, &a).unwrap();
        walk_script_value(&mut bytes_b, &b).unwrap();
        assert_ne!(bytes_a.0, bytes_b.0);
        assert_eq!(&bytes_a.0[8..], &[1, 0, 0xc0, 0x7f]);
        assert_eq!(&bytes_b.0[8..], &[2, 0, 0xc0, 0x7f]);
    }

    fn captured_empty_main_program() -> BhsProgram {
        // `Compiler::compile` capture from the supported shipped image
        // sha256 30478a44...25079. The logical bytes already live in the compiler
        // differential; this adds the checksum-visible Array headers measured from
        // that same in-memory ScriptFile.
        let code = vec![0x47, 0, 0, 0, 0, 0x28, 0xad, 0x7b, 0x05, 0, 0x3e];
        BhsProgram::single(BhsScriptFile {
            code,
            scripts: vec![BhsScript {
                name: "empty_main".into(),
                return_type: 0x0005_7bad,
                ..Default::default()
            }],
            ..Default::default()
        })
        .with_walk_meta(ProgramWalkMeta {
            files: vec![ScriptFileWalkMeta {
                code: ProgramArrayWalkMeta {
                    capacity: 11,
                    grow: u16::MAX,
                    flags: 0,
                },
                scripts: ProgramArrayWalkMeta {
                    capacity: 1,
                    grow: u16::MAX,
                    flags: 0,
                },
                script_meta: vec![ScriptWalkMeta {
                    params: ProgramArrayWalkMeta {
                        grow: u16::MAX,
                        ..Default::default()
                    },
                    refs: ProgramArrayWalkMeta {
                        grow: u16::MAX,
                        ..Default::default()
                    },
                    trigger_names: ProgramArrayWalkMeta {
                        grow: u16::MAX,
                        ..Default::default()
                    },
                    var_names: ProgramArrayWalkMeta {
                        grow: u16::MAX,
                        ..Default::default()
                    },
                    static_var_names: ProgramArrayWalkMeta {
                        grow: u16::MAX,
                        ..Default::default()
                    },
                    ..Default::default()
                }],
                linked_files: ProgramArrayWalkMeta {
                    grow: u16::MAX,
                    ..Default::default()
                },
                ..Default::default()
            }],
        })
    }

    #[test]
    fn captured_program_projection_matches_the_structural_walker() {
        let program = captured_empty_main_program();
        let got = checksum_program(&program).unwrap();

        let script = Script {
            static_vars: &[],
            trigger_bits: DynamicBitMask {
                bits: 0,
                size: 0,
                bytes: &[],
            },
            params: empty_i32(),
            refs: empty_u8(),
            trigger_names: empty_strings(),
            var_names: empty_strings(),
            static_var_names: empty_strings(),
            name: WalkString {
                utf16: &[
                    b'e' as u16,
                    b'm' as u16,
                    b'p' as u16,
                    b't' as u16,
                    b'y' as u16,
                    b'_' as u16,
                    b'm' as u16,
                    b'a' as u16,
                    b'i' as u16,
                    b'n' as u16,
                ],
            },
            offset: 0,
            return_type: 0x0005_7bad,
            script_type: 0,
        };
        let scripts = [Some(script)];
        let files = [ScriptFile {
            code: SimpleU8Array {
                shape: ArrayShape {
                    capacity: 11,
                    grow: u16::MAX,
                    flags: 0,
                },
                elements: &program.files[0].code,
            },
            scripts: ScriptPtrArray {
                shape: ArrayShape {
                    capacity: 1,
                    grow: u16::MAX,
                    flags: 0,
                },
                elements: &scripts,
            },
            const_pool: &[],
            linked_files: empty_i32(),
        }];
        let expected = checksum_script_runtime(&ScriptRuntime {
            script_files: &files,
        })
        .unwrap();
        assert_eq!(got, expected);
        assert_eq!(got.bytes_walked, 120, "non-vacuous compiled image");
    }

    #[test]
    fn live_static_payload_mutation_moves_the_program_channel() {
        let mut program = BhsProgram::single(BhsScriptFile {
            scripts: vec![BhsScript {
                name: "tick".into(),
                statics: vec![Some(Value::Int(1))],
                ..Default::default()
            }],
            ..Default::default()
        })
        .with_walk_meta(ProgramWalkMeta {
            files: vec![ScriptFileWalkMeta {
                scripts: ProgramArrayWalkMeta {
                    capacity: 1,
                    grow: u16::MAX,
                    flags: 0,
                },
                script_meta: vec![ScriptWalkMeta {
                    statics: vec![Some(ValueWalkMeta::scalar(3, 1))],
                    ..Default::default()
                }],
                ..Default::default()
            }],
        });
        let before = checksum_program(&program).unwrap();
        program.files[0].scripts[0].statics[0] = Some(Value::Int(2));
        let after = checksum_program(&program).unwrap();
        assert_ne!(before.checksum, after.checksum);
        assert_eq!(before.bytes_walked, after.bytes_walked);
    }

    #[test]
    fn retail_array_lowering_executes_with_scalar_channel15_constants() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../don-bhs/oracle/fixtures/array_runtime.bhs");
        let includes = sema::IncludePath::with_roots([path.parent().unwrap().to_path_buf()]);
        let unit = sema::analyze(&path, &includes).unwrap();
        let (mut program, diags, _) = don_bhs_cc::codegen::compile(&unit);
        assert!(unit
            .diags
            .iter()
            .chain(&diags)
            .all(|diag| diag.severity != Severity::Error));
        assert_eq!(
            program.files[0].const_pool,
            [Value::Int(1), Value::Int(2), Value::Int(3), Value::Int(0)]
        );
        assert_eq!(
            program.walk_meta().unwrap().files[0].const_pool,
            vec![Some(ValueWalkMeta::scalar(2, 0)); 4]
        );

        let before = checksum_program(&program).unwrap();
        assert!(before.bytes_walked > 200, "non-vacuous compiled image");
        let mut host = NullHost;
        assert_eq!(
            Vm::new(&mut program, &mut host)
                .run_script(0, "array_runtime")
                .unwrap()
                .returned,
            Some(Value::Int(0))
        );
        // Runtime-created locals do not enter ScriptFile::walk_data. The compiled
        // scalar pool does, and a same-shape mutation must move channel 15.
        assert_eq!(checksum_program(&program).unwrap(), before);
        program.files[0].const_pool[0] = Value::Int(4);
        let mutated = checksum_program(&program).unwrap();
        assert_ne!(mutated.checksum, before.checksum);
        assert_eq!(mutated.bytes_walked, before.bytes_walked);
    }

    #[test]
    fn recursive_array_projection_hashes_blank_base_and_live_elements() {
        let program = BhsProgram::single(BhsScriptFile {
            const_pool: vec![Value::array(0x0005_7bad, Value::Int(0), 1)],
            ..Default::default()
        })
        .with_walk_meta(ProgramWalkMeta {
            files: vec![ScriptFileWalkMeta {
                const_pool: vec![Some(ValueWalkMeta {
                    scope: 2,
                    ref_count: 0,
                    nested: ValueWalkNested::Array {
                        blank_base: Some(Box::new(ValueWalkMeta::scalar(3, 1))),
                        values: vec![Some(ValueWalkMeta::scalar(3, 1))],
                    },
                })],
                ..Default::default()
            }],
        });
        let before = checksum_program(&program).unwrap();
        let Value::Obj(array) = &program.files[0].const_pool[0] else {
            unreachable!()
        };
        *array.borrow().values[0].borrow_mut() = Value::Int(1);
        let after = checksum_program(&program).unwrap();
        assert_ne!(before.checksum, after.checksum);
        assert_eq!(before.bytes_walked, after.bytes_walked);
    }

    #[test]
    fn compiled_chunk_loader_installs_a_mutation_sensitive_program_channel() {
        // Exact ChunkWrite header layout: one tag-0 root, one tag-4 bytecode leaf.
        let mut chunks = vec![
            17, 0, 0, 0, 0, 0, 1, 0, // root: size=17, tag=0, children=1
            9, 0, 0, 0, 4, 0, 0, 0,    // leaf: size=9, tag=4, children=0
            0x47, // OP_SCRIPT_MARKER
        ];
        let before_program = don_bhs::chunk::load_program(&chunks, "marker.bhs").unwrap();
        let before = checksum_program(&before_program).unwrap();
        assert!(before.bytes_walked > 4);

        chunks[16] = 0x27; // OP_POP: one compiled-image byte, same container shape.
        let after_program = don_bhs::chunk::load_program(&chunks, "marker.bhs").unwrap();
        let after = checksum_program(&after_program).unwrap();
        assert_ne!(before.checksum, after.checksum);
        assert_eq!(before.bytes_walked, after.bytes_walked);
    }

    #[test]
    fn tag9_global_type_registry_is_outside_channel15_script_file_walk() {
        fn chunk_with_type_name(name: &str) -> Vec<u8> {
            fn push_u32(out: &mut Vec<u8>, value: u32) {
                out.extend_from_slice(&value.to_le_bytes());
            }

            let units = name.encode_utf16().collect::<Vec<_>>();
            let mut type_payload = Vec::new();
            push_u32(&mut type_payload, units.len() as u32);
            for unit in units {
                type_payload.extend_from_slice(&unit.to_le_bytes());
            }

            let mut type_chunk = Vec::new();
            push_u32(&mut type_chunk, (8 + type_payload.len()) as u32);
            type_chunk.extend_from_slice(&9u16.to_le_bytes());
            type_chunk.extend_from_slice(&0u16.to_le_bytes());
            type_chunk.extend_from_slice(&type_payload);

            let code_chunk = vec![
                9, 0, 0, 0, 4, 0, 0, 0,    // tag 4, one-byte payload
                0x47, // OP_SCRIPT_MARKER
            ];
            let mut root = Vec::new();
            push_u32(&mut root, (8 + type_chunk.len() + code_chunk.len()) as u32);
            root.extend_from_slice(&0u16.to_le_bytes());
            root.extend_from_slice(&2u16.to_le_bytes());
            root.extend_from_slice(&type_chunk);
            root.extend_from_slice(&code_chunk);
            root
        }

        let pair = don_bhs::chunk::load_program(&chunk_with_type_name("Pair"), "registry_only.bhs")
            .unwrap();
        let boxed = don_bhs::chunk::load_program(&chunk_with_type_name("Box"), "registry_only.bhs")
            .unwrap();
        assert_eq!(pair.global_type_names().last().unwrap(), "Pair");
        assert_eq!(boxed.global_type_names().last().unwrap(), "Box");

        // RunTimeEnv::walk_data (0x009c41a0) walks the global ScriptFile list and
        // delegates to ScriptFile::walk_data; it never walks ScriptGameInterfaceBase's
        // process-global type-name table. Mutating only tag 9 must therefore leave
        // channel 15 byte-for-byte unchanged.
        assert_eq!(checksum_program(&pair), checksum_program(&boxed));
    }

    #[test]
    fn stale_live_link_resolution_is_rejected_before_hashing() {
        let mut program = BhsProgram::default();
        program.files = vec![
            BhsScriptFile {
                source_file: "lib.bhs".into(),
                ..Default::default()
            },
            BhsScriptFile {
                source_file: "root.bhs".into(),
                linked_file_names: vec!["lib.bhs".into()],
                ..Default::default()
            },
        ];
        program.set_walk_meta(ProgramWalkMeta {
            files: vec![
                ScriptFileWalkMeta::default(),
                ScriptFileWalkMeta {
                    linked_files: ProgramArrayWalkMeta {
                        capacity: 1,
                        grow: u16::MAX,
                        flags: 0,
                    },
                    linked_file_indices: vec![0],
                    ..Default::default()
                },
            ],
        });
        assert!(checksum_program(&program).is_ok());

        // The executable table is authoritative for OP_CALL_INCLUDE. If the checksum
        // sidecar drifts from it, channel 15 must reject the stale projection.
        program.walk_meta_mut().unwrap().files[1].linked_file_indices[0] = 1;
        assert_eq!(
            checksum_program(&program),
            Err(ScriptChannelError::LinkedFileIndexMismatch {
                slot: 0,
                live: 0,
                metadata: 1,
            })
        );
    }

    #[test]
    fn program_projection_rejects_missing_and_stale_sidecars() {
        let bare = BhsProgram::single(BhsScriptFile::default());
        assert_eq!(
            checksum_program(&bare),
            Err(ScriptChannelError::MissingProgramWalkMetadata)
        );

        let mut stale = captured_empty_main_program();
        stale.files[0].scripts.push(BhsScript::default());
        assert_eq!(
            checksum_program(&stale),
            Err(ScriptChannelError::ProjectionLengthMismatch {
                what: "ScriptFile.scripts",
                live: 2,
                metadata: 1,
            })
        );
    }
}
