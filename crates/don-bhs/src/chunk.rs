//! Reader for the shipped BHS chunk container.
//!
//! `ChunkWrite::chunk_begin` (`0x00a48390`) writes an eight-byte header
//! `[size: u32, tag: u16, children: u16]`; `size` includes that header. The retail
//! compiler emits one tag-0 root containing the leaf chunks dispatched by
//! `ScriptFile::read_script_chunk` (`0x009c5440`). This module implements the exact
//! pointer-free subset needed by scalar scripts and rejects the still-global struct
//! type registry (tag 9) and unresolved includes rather than manufacturing state.

use std::fmt;

use crate::program::{
    ArrayWalkMeta, Program, ProgramWalkMeta, Script, ScriptFile, ScriptFileWalkMeta,
    ScriptWalkMeta, ValueWalkMeta,
};
use crate::value::{ScriptTy, Value};

const HEADER_LEN: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChunkError {
    Truncated {
        at: usize,
        needed: usize,
        available: usize,
    },
    InvalidChunkSize {
        at: usize,
        size: usize,
    },
    RootSizeMismatch {
        declared: usize,
        actual: usize,
    },
    BadRootTag(u16),
    ChildCountMismatch {
        declared: u16,
        actual: usize,
    },
    LeafHasChildren {
        tag: u16,
        children: u16,
    },
    DuplicateSingletonTag(u16),
    UnsupportedTag(u16),
    UnsupportedStructTypes,
    UnsupportedConstantType(u32),
    UnresolvedLinks(usize),
    InvalidUtf16,
    CountExceedsInput {
        what: &'static str,
        count: usize,
        input_len: usize,
    },
    ContainerTooLarge(usize),
    ScriptIndexOutOfOrder {
        expected: usize,
        actual: usize,
    },
    MissingCurrentScript,
    IndexOutOfRange {
        what: &'static str,
        index: usize,
        count: usize,
    },
    TrailingPayload {
        tag: u16,
        remaining: usize,
    },
}

impl fmt::Display for ChunkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for ChunkError {}

#[derive(Debug, Clone, Copy)]
struct Header {
    size: usize,
    tag: u16,
    children: u16,
}

fn header_at(bytes: &[u8], at: usize) -> Result<Header, ChunkError> {
    let end = at.checked_add(HEADER_LEN).ok_or(ChunkError::Truncated {
        at,
        needed: HEADER_LEN,
        available: bytes.len().saturating_sub(at),
    })?;
    let raw = bytes.get(at..end).ok_or(ChunkError::Truncated {
        at,
        needed: HEADER_LEN,
        available: bytes.len().saturating_sub(at),
    })?;
    Ok(Header {
        size: u32::from_le_bytes(raw[0..4].try_into().unwrap()) as usize,
        tag: u16::from_le_bytes(raw[4..6].try_into().unwrap()),
        children: u16::from_le_bytes(raw[6..8].try_into().unwrap()),
    })
}

struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
    base: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8], base: usize) -> Self {
        Self { bytes, at: 0, base }
    }

    fn remaining(&self) -> usize {
        self.bytes.len() - self.at
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8], ChunkError> {
        let end = self.at.checked_add(len).ok_or(ChunkError::Truncated {
            at: self.base + self.at,
            needed: len,
            available: self.remaining(),
        })?;
        let out = self.bytes.get(self.at..end).ok_or(ChunkError::Truncated {
            at: self.base + self.at,
            needed: len,
            available: self.remaining(),
        })?;
        self.at = end;
        Ok(out)
    }

    fn u32(&mut self) -> Result<u32, ChunkError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn string(&mut self) -> Result<String, ChunkError> {
        // `String::write` (0x00a18240): a u32 UTF-16-unit count followed by exactly
        // that many little-endian units, with no terminator in the chunk.
        let count = self.u32()? as usize;
        let byte_len = count.checked_mul(2).ok_or(ChunkError::Truncated {
            at: self.base + self.at,
            needed: usize::MAX,
            available: self.remaining(),
        })?;
        let raw = self.take(byte_len)?;
        let units = raw
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        String::from_utf16(&units).map_err(|_| ChunkError::InvalidUtf16)
    }

    fn finish(self, tag: u16) -> Result<(), ChunkError> {
        if self.remaining() == 0 {
            Ok(())
        } else {
            Err(ChunkError::TrailingPayload {
                tag,
                remaining: self.remaining(),
            })
        }
    }
}

struct Loader {
    file: ScriptFile,
    current_script: Option<usize>,
    input_len: usize,
    saw_code: bool,
    saw_links: bool,
    saw_line_info: bool,
}

impl Loader {
    fn new(source_file: String, input_len: usize) -> Self {
        Self {
            file: ScriptFile {
                source_file,
                ..Default::default()
            },
            current_script: None,
            input_len,
            saw_code: false,
            saw_links: false,
            saw_line_info: false,
        }
    }

    fn bounded_count(&self, what: &'static str, value: u32) -> Result<usize, ChunkError> {
        let count = value as usize;
        if count > self.input_len {
            Err(ChunkError::CountExceedsInput {
                what,
                count,
                input_len: self.input_len,
            })
        } else {
            Ok(count)
        }
    }

    fn load(&mut self, tag: u16, payload: &[u8], base: usize) -> Result<(), ChunkError> {
        let mut cursor = Cursor::new(payload, base);
        match tag {
            0 => {}
            2 => self.load_script_info(&mut cursor)?,
            3 => self.load_const(&mut cursor)?,
            4 => {
                if self.saw_code {
                    return Err(ChunkError::DuplicateSingletonTag(4));
                }
                self.saw_code = true;
                self.file.code.clear();
                self.file.code.extend_from_slice(payload);
                cursor.at = payload.len();
            }
            5 => self.load_trigger(&mut cursor)?,
            6 => {
                if self.saw_links {
                    return Err(ChunkError::DuplicateSingletonTag(6));
                }
                self.saw_links = true;
                self.load_links(&mut cursor)?;
            }
            7 => {
                if self.saw_line_info {
                    return Err(ChunkError::DuplicateSingletonTag(7));
                }
                self.saw_line_info = true;
                self.load_line_info(&mut cursor)?;
            }
            8 => self.load_variable(&mut cursor)?,
            9 => return Err(ChunkError::UnsupportedStructTypes),
            other => return Err(ChunkError::UnsupportedTag(other)),
        }
        cursor.finish(tag)
    }

    fn load_script_info(&mut self, cursor: &mut Cursor<'_>) -> Result<(), ChunkError> {
        // LocalScriptType::write (0x009da900) and load_script_info (0x009c51c0):
        // index, name, entry, script_type, return_type, trigger_count, param_count,
        // then [SymType.type, is_ref] for every parameter.
        let index = self.bounded_count("script index", cursor.u32()?)?;
        if index != self.file.scripts.len() {
            return Err(ChunkError::ScriptIndexOutOfOrder {
                expected: self.file.scripts.len(),
                actual: index,
            });
        }
        let name = cursor.string()?;
        let entry = cursor.u32()?;
        let script_type = cursor.u32()?;
        let return_type = cursor.u32()?;
        let trigger_count = self.bounded_count("trigger count", cursor.u32()?)?;
        let param_count = self.bounded_count("parameter count", cursor.u32()?)?;
        if param_count > cursor.remaining() / 8 {
            return Err(ChunkError::Truncated {
                at: cursor.base + cursor.at,
                needed: param_count.saturating_mul(8),
                available: cursor.remaining(),
            });
        }
        let mut params = Vec::with_capacity(param_count);
        let mut refs = Vec::with_capacity(param_count);
        for _ in 0..param_count {
            params.push(cursor.u32()?);
            refs.push(cursor.u32()? as u8);
        }
        let trigger_count_i32 =
            i32::try_from(trigger_count).map_err(|_| ChunkError::CountExceedsInput {
                what: "trigger count",
                count: trigger_count,
                input_len: self.input_len,
            })?;
        let trigger_bytes = trigger_count.saturating_add(7) / 8;
        self.file.scripts.push(Script {
            name,
            arity: param_count,
            params,
            refs,
            entry,
            return_type,
            script_type,
            trigger_names: vec![String::new(); trigger_count],
            trigger_bits: vec![0xff; trigger_bytes],
            trigger_count: trigger_count_i32,
            ..Default::default()
        });
        self.current_script = Some(index);
        Ok(())
    }

    fn load_const(&mut self, cursor: &mut Cursor<'_>) -> Result<(), ChunkError> {
        let tag = cursor.u32()?;
        let value = match ScriptTy::from_tag(tag) {
            Some(ScriptTy::Int) => Value::Int(cursor.u32()? as i32),
            Some(ScriptTy::Real) => Value::Real(f32::from_bits(cursor.u32()?)),
            Some(ScriptTy::Str) => Value::str(cursor.string()?),
            _ => return Err(ChunkError::UnsupportedConstantType(tag)),
        };
        self.file.const_pool.push(value);
        Ok(())
    }

    fn load_trigger(&mut self, cursor: &mut Cursor<'_>) -> Result<(), ChunkError> {
        let script = self.bounded_count("trigger script index", cursor.u32()?)?;
        let trigger = self.bounded_count("trigger index", cursor.u32()?)?;
        let name = cursor.string()?;
        let script_count = self.file.scripts.len();
        let target = self
            .file
            .scripts
            .get_mut(script)
            .ok_or(ChunkError::IndexOutOfRange {
                what: "trigger script index",
                index: script,
                count: script_count,
            })?;
        let trigger_count = target.trigger_names.len();
        let slot = target
            .trigger_names
            .get_mut(trigger)
            .ok_or(ChunkError::IndexOutOfRange {
                what: "trigger index",
                index: trigger,
                count: trigger_count,
            })?;
        *slot = name;
        Ok(())
    }

    fn load_links(&mut self, cursor: &mut Cursor<'_>) -> Result<(), ChunkError> {
        let count = self.bounded_count("linked file count", cursor.u32()?)?;
        let mut links = Vec::with_capacity(count);
        for _ in 0..count {
            links.push(cursor.string()?);
        }
        if !links.is_empty() {
            // ScriptFile::init resolves these names through the global loaded-file
            // table. A standalone Program has no lawful substitute for that table.
            return Err(ChunkError::UnresolvedLinks(links.len()));
        }
        self.file.linked_file_names = links;
        Ok(())
    }

    fn load_line_info(&mut self, cursor: &mut Cursor<'_>) -> Result<(), ChunkError> {
        let count = self.bounded_count("line-info count", cursor.u32()?)?;
        if count > cursor.remaining() / 4 {
            return Err(ChunkError::Truncated {
                at: cursor.base + cursor.at,
                needed: count.saturating_mul(4),
                available: cursor.remaining(),
            });
        }
        self.file.line_to_op.clear();
        self.file.line_to_op.reserve(count);
        for _ in 0..count {
            self.file.line_to_op.push(cursor.u32()? as i32);
        }
        Ok(())
    }

    fn load_variable(&mut self, cursor: &mut Cursor<'_>) -> Result<(), ChunkError> {
        let raw = cursor.u32()?;
        let name = cursor.string()?;
        let script = self
            .current_script
            .ok_or(ChunkError::MissingCurrentScript)?;
        let target = &mut self.file.scripts[script];
        if raw & 0x4000_0000 != 0 {
            let index = (raw & 0xbfff_ffff) as usize;
            if index > self.input_len {
                return Err(ChunkError::CountExceedsInput {
                    what: "static variable index",
                    count: index,
                    input_len: self.input_len,
                });
            }
            if target.static_var_names.len() <= index {
                target.static_var_names.resize(index + 1, String::new());
            }
            target.static_var_names[index] = name;
        } else {
            let index = raw as usize;
            if index > self.input_len {
                return Err(ChunkError::CountExceedsInput {
                    what: "local variable index",
                    count: index,
                    input_len: self.input_len,
                });
            }
            if target.var_names.len() <= index {
                target.var_names.resize(index + 1, String::new());
            }
            target.var_names[index] = name;
        }
        Ok(())
    }

    fn finish(self) -> Result<Program, ChunkError> {
        fn shape(count: usize) -> ArrayWalkMeta {
            ArrayWalkMeta {
                // Every supported-image loader capture has capacity exactly count.
                capacity: i32::try_from(count).expect("input-bounded count fits i32"),
                grow: u16::MAX,
                flags: 0,
            }
        }

        let script_meta = self
            .file
            .scripts
            .iter()
            .map(|script| ScriptWalkMeta {
                statics: Vec::new(),
                params: shape(script.params.len()),
                refs: shape(script.refs.len()),
                trigger_names: shape(script.trigger_names.len()),
                var_names: shape(script.var_names.len()),
                static_var_names: shape(script.static_var_names.len()),
            })
            .collect();
        let file_meta = ScriptFileWalkMeta {
            code: shape(self.file.code.len()),
            scripts: shape(self.file.scripts.len()),
            script_meta,
            const_pool: self
                .file
                .const_pool
                .iter()
                .map(|_| Some(ValueWalkMeta::scalar(2, 0)))
                .collect(),
            linked_files: shape(0),
            linked_file_indices: Vec::new(),
        };
        Ok(Program::single(self.file).with_walk_meta(ProgramWalkMeta {
            files: vec![file_meta],
        }))
    }
}

/// Load one scalar/no-include compiled ScriptFile rooted at a retail tag-0 chunk.
///
/// The resulting [`Program`] includes the same complete channel-15 sidecar as the
/// normal source compiler path. Tag 9 and non-empty tag 6 require global registries
/// not represented by a standalone Program and therefore return explicit errors.
pub fn load_program(bytes: &[u8], source_file: impl Into<String>) -> Result<Program, ChunkError> {
    if bytes.len() > i32::MAX as usize {
        return Err(ChunkError::ContainerTooLarge(bytes.len()));
    }
    let root = header_at(bytes, 0)?;
    if root.size < HEADER_LEN {
        return Err(ChunkError::InvalidChunkSize {
            at: 0,
            size: root.size,
        });
    }
    if root.size != bytes.len() {
        return Err(ChunkError::RootSizeMismatch {
            declared: root.size,
            actual: bytes.len(),
        });
    }
    if root.tag != 0 {
        return Err(ChunkError::BadRootTag(root.tag));
    }

    let mut loader = Loader::new(source_file.into(), bytes.len());
    let mut at = HEADER_LEN;
    let mut children = 0usize;
    while at < root.size {
        let header = header_at(bytes, at)?;
        if header.size < HEADER_LEN {
            return Err(ChunkError::InvalidChunkSize {
                at,
                size: header.size,
            });
        }
        if header.children != 0 {
            return Err(ChunkError::LeafHasChildren {
                tag: header.tag,
                children: header.children,
            });
        }
        let end = at
            .checked_add(header.size)
            .filter(|&end| end <= root.size)
            .ok_or(ChunkError::Truncated {
                at,
                needed: header.size,
                available: root.size - at,
            })?;
        loader.load(header.tag, &bytes[at + HEADER_LEN..end], at + HEADER_LEN)?;
        at = end;
        children += 1;
    }
    if children != root.children as usize {
        return Err(ChunkError::ChildCountMismatch {
            declared: root.children,
            actual: children,
        });
    }
    loader.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::NullHost;
    use crate::vm::Vm;

    fn push_u32(out: &mut Vec<u8>, value: u32) {
        out.extend_from_slice(&value.to_le_bytes());
    }

    fn push_string(out: &mut Vec<u8>, value: &str) {
        let units = value.encode_utf16().collect::<Vec<_>>();
        push_u32(out, units.len() as u32);
        for unit in units {
            out.extend_from_slice(&unit.to_le_bytes());
        }
    }

    fn chunk(tag: u16, payload: Vec<u8>) -> Vec<u8> {
        let mut out = Vec::with_capacity(HEADER_LEN + payload.len());
        push_u32(&mut out, (HEADER_LEN + payload.len()) as u32);
        out.extend_from_slice(&tag.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&payload);
        out
    }

    fn root(children: &[Vec<u8>]) -> Vec<u8> {
        let size = HEADER_LEN + children.iter().map(Vec::len).sum::<usize>();
        let mut out = Vec::with_capacity(size);
        push_u32(&mut out, size as u32);
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&(children.len() as u16).to_le_bytes());
        for child in children {
            out.extend_from_slice(child);
        }
        out
    }

    fn decode_hex(value: &str) -> Vec<u8> {
        value
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                let digit = |byte: u8| match byte {
                    b'0'..=b'9' => byte - b'0',
                    b'a'..=b'f' => byte - b'a' + 10,
                    _ => panic!("non-hex byte"),
                };
                digit(pair[0]) << 4 | digit(pair[1])
            })
            .collect()
    }

    fn static_int_container() -> Vec<u8> {
        let mut script = Vec::new();
        push_u32(&mut script, 0);
        push_string(&mut script, "static_int");
        push_u32(&mut script, 0); // entry
        push_u32(&mut script, 0); // script_type
        push_u32(&mut script, ScriptTy::Int.tag());
        push_u32(&mut script, 0); // trigger_count
        push_u32(&mut script, 0); // param_count

        let mut constant = Vec::new();
        push_u32(&mut constant, ScriptTy::Int.tag());
        push_u32(&mut constant, 1);

        let mut variable = Vec::new();
        push_u32(&mut variable, 0x4000_0000);
        push_string(&mut variable, "value");

        root(&[
            chunk(2, script),
            chunk(3, constant),
            chunk(
                4,
                decode_hex("47000000004400000040180000002600000020320000004028ad7b05003e"),
            ),
            chunk(8, variable),
        ])
    }

    #[test]
    fn retail_static_int_chunks_load_execute_and_carry_walk_metadata() {
        let bytes = static_int_container();
        // Root header emitted by ChunkWrite::chunk_begin: size, tag 0, four children.
        assert_eq!(
            u32::from_le_bytes(bytes[0..4].try_into().unwrap()),
            bytes.len() as u32
        );
        assert_eq!(&bytes[4..8], &[0, 0, 4, 0]);

        let mut program = load_program(&bytes, "static_int.bhs").unwrap();
        assert_eq!(program.files[0].scripts[0].name, "static_int");
        assert_eq!(program.files[0].scripts[0].static_var_names, ["value"]);
        assert_eq!(program.files[0].const_pool, [Value::Int(1)]);
        let meta = &program.walk_meta().unwrap().files[0];
        assert_eq!(meta.code.capacity as usize, program.files[0].code.len());
        assert_eq!(meta.code.grow, u16::MAX);
        assert_eq!(meta.const_pool, [Some(ValueWalkMeta::scalar(2, 0))]);

        let mut host = NullHost;
        assert_eq!(
            Vm::new(&mut program, &mut host)
                .run_script(0, "static_int")
                .unwrap()
                .returned,
            Some(Value::Int(0))
        );
        assert_eq!(
            program.walk_meta().unwrap().files[0].script_meta[0].statics,
            [Some(ValueWalkMeta::scalar(3, 0))]
        );
    }

    #[test]
    fn every_supported_leaf_uses_the_recovered_writer_layout() {
        let mut script = Vec::new();
        push_u32(&mut script, 0);
        push_string(&mut script, "leaf_shapes");
        push_u32(&mut script, 7); // entry
        push_u32(&mut script, 0); // script_type
        push_u32(&mut script, ScriptTy::Real.tag());
        push_u32(&mut script, 1); // trigger_count
        push_u32(&mut script, 2); // param_count
        push_u32(&mut script, ScriptTy::Int.tag());
        push_u32(&mut script, 0);
        push_u32(&mut script, ScriptTy::Str.tag());
        push_u32(&mut script, 1);

        let mut real = Vec::new();
        push_u32(&mut real, ScriptTy::Real.tag());
        push_u32(&mut real, 1.5f32.to_bits());
        let mut string = Vec::new();
        push_u32(&mut string, ScriptTy::Str.tag());
        push_string(&mut string, "retail");
        let mut trigger = Vec::new();
        push_u32(&mut trigger, 0);
        push_u32(&mut trigger, 0);
        push_string(&mut trigger, "gate");
        let mut links = Vec::new();
        push_u32(&mut links, 0);
        let mut lines = Vec::new();
        push_u32(&mut lines, 2);
        push_u32(&mut lines, 10);
        push_u32(&mut lines, 20);
        let mut variable = Vec::new();
        push_u32(&mut variable, 0);
        push_string(&mut variable, "value");

        let bytes = root(&[
            chunk(2, script),
            chunk(3, real),
            chunk(3, string),
            chunk(4, vec![0x47]),
            chunk(5, trigger),
            chunk(6, links),
            chunk(7, lines),
            chunk(8, variable),
        ]);
        let program = load_program(&bytes, "leaf_shapes.bhs").unwrap();
        let file = &program.files[0];
        let script = &file.scripts[0];
        assert_eq!(script.entry, 7);
        assert_eq!(script.return_type, ScriptTy::Real.tag());
        assert_eq!(script.params, [ScriptTy::Int.tag(), ScriptTy::Str.tag()]);
        assert_eq!(script.refs, [0, 1]);
        assert_eq!(script.trigger_names, ["gate"]);
        assert_eq!(script.trigger_bits, [0xff]);
        assert_eq!(script.var_names, ["value"]);
        assert_eq!(file.const_pool, [Value::Real(1.5), Value::str("retail")]);
        assert_eq!(file.code, [0x47]);
        assert_eq!(file.line_to_op, [10, 20]);
        assert_eq!(
            program.walk_meta().unwrap().files[0].const_pool,
            [
                Some(ValueWalkMeta::scalar(2, 0)),
                Some(ValueWalkMeta::scalar(2, 0))
            ]
        );
    }

    #[test]
    fn one_byte_header_mutations_fail_closed() {
        let valid = static_int_container();

        let mut wrong_size = valid.clone();
        wrong_size[0] ^= 1;
        assert!(matches!(
            load_program(&wrong_size, "static_int.bhs"),
            Err(ChunkError::RootSizeMismatch { .. })
        ));

        let mut wrong_children = valid;
        wrong_children[6] ^= 1;
        assert!(matches!(
            load_program(&wrong_children, "static_int.bhs"),
            Err(ChunkError::ChildCountMismatch { .. })
        ));

        let mut undersized_leaf = static_int_container();
        undersized_leaf[8..12].copy_from_slice(&7u32.to_le_bytes());
        assert!(matches!(
            load_program(&undersized_leaf, "static_int.bhs"),
            Err(ChunkError::InvalidChunkSize { at: 8, size: 7 })
        ));
    }

    #[test]
    fn global_struct_and_link_state_are_explicitly_unsupported() {
        let struct_root = root(&[chunk(9, Vec::new())]);
        assert!(matches!(
            load_program(&struct_root, "struct.bhs"),
            Err(ChunkError::UnsupportedStructTypes)
        ));

        let mut links = Vec::new();
        push_u32(&mut links, 1);
        push_string(&mut links, "included.bhs");
        let links_root = root(&[chunk(6, links)]);
        assert!(matches!(
            load_program(&links_root, "root.bhs"),
            Err(ChunkError::UnresolvedLinks(1))
        ));
    }
}
