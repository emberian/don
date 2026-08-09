//! The compiled-program representation: `ScriptFile` and `Script`.
//!
//! Layouts below are **verbatim from `ron-bin/sbl/rise.pdb`** — member names and
//! offsets are the shipped ones, not guesses. [measured]
//!
//! ```text
//! ScriptFile  +0   Buffer                 code            // bytecode; data ptr at +0x10, size at +4
//!             +28  PtrArray<Script>       scripts         // data at +0x2c
//!             +56  Array<ScriptType*>     const_pool      // count +0x3c, data +0x48
//!             +84  Array<String>          linked_file_names
//!             +108 String                 source_file
//!             +128 Array<int>             line_to_op
//!             +156 ...                    break_lines
//!             +184 Array<ScriptFile*>     linked_files    // count +0xbc, data +0xc8
//!             +212 long                   source_time_stamp
//!             +216 long                   binary_time_stamp
//!             +220 unsigned char          file_flags
//!
//! Script      +4   Array<..>              params          // count at +8 == arity
//!             +32  ...                    refs
//!             +60  Array<ScriptType*>     static_vars     // count +0x40, data +0x4c
//!             +88  BitArray               trigger_bits    // n at +0x58, bytes at +0x60
//!             +100 Array<String>          trigger_names
//!             +124 Array<String>          var_names
//!             +148 Array<String>          static_var_names
//!             +172 String                 name
//!             +192 int                    offset          // entry code offset
//!             +196 int                    return_type
//!             +200 int                    script_type
//! ```
//!
//! The **on-disk** compiled form is a chunk file. `ScriptFile::read_script_chunk`
//! (`0x009c5440`) dispatches on a 16-bit chunk tag; the chunk header is 8 bytes
//! (size, tag) since `load_bytecode` is handed `*(int*)chunk - 8`: [measured]
//!
//! | tag | loader | contents |
//! |----|--------|----------|
//! | 0 | (skip) | — |
//! | 2 | `load_script_info` `0x009c51c0` | one `Script`: name, params, statics, offset |
//! | 3 | `load_const`       `0x009c4fb0` | one constant-pool entry |
//! | 4 | `load_bytecode`    `0x009c50c0` | the raw code bytes |
//! | 5 | `load_trigger`     `0x009c4f50` | a trigger declaration |
//! | 6 | `load_links`       `0x009c5120` | `include` links |
//! | 7 | `load_line_info`   `0x009c4ec0` | line -> code offset map |
//! | 8 | `load_variable`    `0x009c4e30` | a variable name record |
//! | 9 | `load_struct_types``0x009c4d70` | struct type definitions |
//!
//! We do not yet parse that container — see `docs/tracks/bhs-engine.md`. This module
//! is the *in-memory* shape the VM runs on, which is what a compiler frontend or a
//! chunk reader would both produce.

use crate::value::Value;

/// The three checksum-visible fields of a non-empty retail Array container.
///
/// These are deliberately not reconstructed from Rust `Vec::capacity()`: retail's
/// allocator growth and its `grow`/`flags` bytes are part of channel 15.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ArrayWalkMeta {
    pub capacity: i32,
    pub grow: u16,
    pub flags: u8,
}

/// Checksum metadata parallel to one non-null [`Value`].
///
/// `data_type`, scalar payload, and the object/array distinction remain authoritative
/// in the live value. The sidecar retains only fields Rust ownership intentionally
/// erased plus recursive pointer-slot metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValueWalkMeta {
    pub scope: u16,
    pub ref_count: u16,
    pub nested: ValueWalkNested,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValueWalkNested {
    Scalar,
    Object {
        values: Vec<Option<ValueWalkMeta>>,
    },
    Array {
        blank_base: Option<Box<ValueWalkMeta>>,
        values: Vec<Option<ValueWalkMeta>>,
    },
}

impl ValueWalkMeta {
    pub const fn scalar(scope: u16, ref_count: u16) -> Self {
        Self {
            scope,
            ref_count,
            nested: ValueWalkNested::Scalar,
        }
    }
}

/// Retail-only container/value fields parallel to one [`Script`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScriptWalkMeta {
    pub statics: Vec<Option<ValueWalkMeta>>,
    pub params: ArrayWalkMeta,
    pub refs: ArrayWalkMeta,
    pub trigger_names: ArrayWalkMeta,
    pub var_names: ArrayWalkMeta,
    pub static_var_names: ArrayWalkMeta,
}

/// Retail-only checksum state parallel to one [`ScriptFile`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScriptFileWalkMeta {
    pub code: ArrayWalkMeta,
    pub scripts: ArrayWalkMeta,
    pub script_meta: Vec<ScriptWalkMeta>,
    pub const_pool: Vec<Option<ValueWalkMeta>>,
    pub linked_files: ArrayWalkMeta,
    pub linked_file_indices: Vec<i32>,
}

/// Complete sidecar required to project a live [`Program`] onto channel 15.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProgramWalkMeta {
    pub files: Vec<ScriptFileWalkMeta>,
}

/// One compiled script function.
#[derive(Debug, Clone, Default)]
pub struct Script {
    pub name: String,
    /// `Script::params.count` — the declared arity.
    pub arity: usize,
    /// Serialized `SymType.type` tag for each parameter (`Script::params`, +4).
    /// Retail writes each tag as the first dword of a parameter record and the loader
    /// restores this table before compacting the adjacent ref dword into [`Self::refs`].
    pub params: Vec<u32>,
    /// `Script::refs` (+32), one byte per parameter. Retail's chunk writer emits a
    /// dword `0` or `1` beside each parameter type and the loader compacts its low byte
    /// into this array. Runtime aliasing itself is selected by the compiled
    /// `OP_INIT`/`OP_INIT_COPY` prologue; the VM does not consult this metadata at call
    /// time.
    pub refs: Vec<u8>,
    /// Byte offset of this function's entry point in [`ScriptFile::code`]
    /// (`Script::offset`, +192).
    pub entry: u32,
    /// `Script::return_type` (+196), a `SymType` tag. `0x84048` (void) makes
    /// `VirtualMachine::init` set the void-return flag, which suppresses the
    /// return-slot reservation in `RunTimeEnv::call_script`.
    pub return_type: u32,
    /// `Script::script_type` (+200). Retail's local-script writer stores literal zero
    /// here for every `ai` / `scenario` / `conquest` qualifier; the qualifier itself is
    /// compiler-only metadata, not a runtime ID.
    pub script_type: u32,
    /// Names of the local slots, for disassembly (`Script::var_names`).
    pub var_names: Vec<String>,
    /// Names of the `static` slots (`Script::static_var_names`).
    pub static_var_names: Vec<String>,
    /// Names of inline `trigger` blocks (`Script::trigger_names`).
    pub trigger_names: Vec<String>,

    // ---- mutable runtime state, checksummed by the engine ----
    /// `Script::static_vars` (+60). **This is the entire cross-frame memory of a
    /// BHS script.** `Game::do_frame` calls `run_script` once per frame with zero
    /// arguments, so every shipped script is a state machine over these slots.
    /// A `None` slot is the uninitialised state that `OP_JUMP_IF_INITED` tests.
    pub statics: Vec<Option<Value>>,
    /// `Script::trigger_bits` (+88). Bit *set* means the trigger is enabled;
    /// `OP_JUMP_IF_BITSET` jumps *past* the trigger body when the bit is clear.
    pub trigger_bits: Vec<u8>,
    /// `Script::trigger_bits` element count, the bound `OP_BIT_SET`/`OP_BIT_UNSET`
    /// compare against (`cmp edx, [script+0x58]; jg skip`).
    pub trigger_count: i32,
}

impl Script {
    /// `Script::is_trigger_enabled(int)` (`0x009c5b60`).
    pub fn is_trigger_enabled(&self, idx: i32) -> bool {
        if idx < 0 || idx > self.trigger_count {
            return false;
        }
        let byte = (idx >> 3) as usize;
        match self.trigger_bits.get(byte) {
            Some(b) => (b >> (idx & 7)) & 1 != 0,
            None => false,
        }
    }

    /// `OP_BIT_SET` (0x3c). The handler at `0x009e1022` uses **`btr`** — it CLEARS.
    /// The enum name is inverted relative to the machine; we follow the machine.
    pub fn trigger_bit_clear(&mut self, idx: i32) {
        if idx > self.trigger_count {
            return;
        }
        let byte = (idx >> 3) as usize;
        if let Some(b) = self.trigger_bits.get_mut(byte) {
            *b &= !(1u8 << (idx & 7));
        }
    }

    /// `OP_BIT_UNSET` (0x3d). The handler at `0x009e105f` uses **`bts`** — it SETS.
    pub fn trigger_bit_set(&mut self, idx: i32) {
        if idx > self.trigger_count {
            return;
        }
        let byte = (idx >> 3) as usize;
        if let Some(b) = self.trigger_bits.get_mut(byte) {
            *b |= 1u8 << (idx & 7);
        }
    }

    /// `Script::enable_all_triggers(bool)` (`0x009c5c90`), reached from the
    /// `enable_all_triggers` / `disable_all_triggers` builtins. Unlike the opcode
    /// pair, these are named the way they behave.
    pub fn enable_all_triggers(&mut self, on: bool) {
        let fill = if on { 0xffu8 } else { 0x00 };
        for b in self.trigger_bits.iter_mut() {
            *b = fill;
        }
    }

    /// Resolve a trigger by name against `Script::trigger_names` (+100).
    pub fn find_trigger(&self, name: &str) -> Option<usize> {
        self.trigger_names
            .iter()
            .position(|n| n.eq_ignore_ascii_case(name))
    }
}

/// One compiled `.bhs` translation unit.
#[derive(Debug, Clone, Default)]
pub struct ScriptFile {
    pub source_file: String,
    /// `ScriptFile::code` — the flat bytecode array all scripts in this file share.
    pub code: Vec<u8>,
    pub scripts: Vec<Script>,
    /// `ScriptFile::const_pool` — the target of a `0x20000000`-tagged variable
    /// reference and of `OP_CASE`'s first operand.
    pub const_pool: Vec<Value>,
    /// `ScriptFile::linked_file_names`, parallel to `linked_files`.
    pub linked_file_names: Vec<String>,
    /// Code offset -> source line, from the `line_to_op` chunk. Used only for
    /// diagnostics and for lining our disassembly up with retail's `bhs.log`.
    pub line_to_op: Vec<i32>,
}

impl ScriptFile {
    pub fn find_script(&self, name: &str) -> Option<usize> {
        // `ScriptFile::find_script_index` (`0x009c4d20`) is a linear scan.
        self.scripts
            .iter()
            .position(|s| s.name.eq_ignore_ascii_case(name))
    }
}

/// A loaded program: one root file plus everything it `include`s.
///
/// `RunTimeEnv::call_script(file_idx, script_idx)` resolves `file_idx` through
/// `ScriptFile::linked_files` when non-negative, and means "the current file" when
/// negative (which is how `OP_CALL` is encoded: `call_script(-1, script_idx)`).
#[derive(Debug, Clone, Default)]
pub struct Program {
    pub files: Vec<ScriptFile>,
    walk_meta: Option<ProgramWalkMeta>,
}

impl Program {
    pub fn single(file: ScriptFile) -> Program {
        Program {
            files: vec![file],
            walk_meta: None,
        }
    }

    /// Attach an independently recovered retail checksum sidecar.
    ///
    /// The replay adapter validates every parallel length and dynamic shape before
    /// walking. Attaching metadata is therefore not enough to make an incompatible
    /// program hashable; stale or partial sidecars fail closed.
    pub fn with_walk_meta(mut self, walk_meta: ProgramWalkMeta) -> Program {
        self.walk_meta = Some(walk_meta);
        self
    }

    pub fn set_walk_meta(&mut self, walk_meta: ProgramWalkMeta) {
        self.walk_meta = Some(walk_meta);
    }

    pub fn walk_meta(&self) -> Option<&ProgramWalkMeta> {
        self.walk_meta.as_ref()
    }

    /// Mutable access for runtime operations that mirror retail ownership fields.
    ///
    /// A producer must install a complete sidecar first. The VM never fabricates a
    /// missing sidecar: without one, channel 15 remains unavailable and execution
    /// proceeds without pretending checksum fidelity.
    pub fn walk_meta_mut(&mut self) -> Option<&mut ProgramWalkMeta> {
        self.walk_meta.as_mut()
    }
}
