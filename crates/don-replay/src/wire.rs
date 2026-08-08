//! Typed field access over a decoded command, and the classification that says
//! which commands the simulation has to act on.
//!
//! The field table is generated from `schema/command-wire.json` (the PDB type
//! stream for the 82 `*Command` structs), so reading `MoveToCommand::x` is a
//! table lookup, not a hand-counted offset.

pub use crate::wire_gen::{
    COMMAND_FIELDS, COMMAND_METHOD, COMMAND_SIZEOF, COMMAND_STRUCT, NUM_OPCODES,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    I8,
    U8,
    I16,
    U16,
    I32,
    U32,
    F32,
    /// A struct or array we do not decode (`QueuePos`, `wchar_t[N]`, …).
    Opaque,
}

#[derive(Debug, Clone, Copy)]
pub struct Field {
    pub off: u16,
    pub kind: FieldKind,
    pub width: u8,
    pub name: &'static str,
    pub ty: &'static str,
}

/// A decoded command with typed access to its PDB fields.
#[derive(Debug, Clone, Copy)]
pub struct CommandView<'a> {
    pub opcode: u8,
    pub bytes: &'a [u8],
}

impl<'a> CommandView<'a> {
    pub fn new(opcode: u8, bytes: &'a [u8]) -> Self {
        CommandView { opcode, bytes }
    }
    pub fn struct_name(&self) -> &'static str {
        COMMAND_STRUCT.get(self.opcode as usize).copied().unwrap_or("?")
    }
    pub fn method(&self) -> &'static str {
        COMMAND_METHOD.get(self.opcode as usize).copied().unwrap_or("?")
    }
    pub fn fields(&self) -> &'static [Field] {
        COMMAND_FIELDS.get(self.opcode as usize).copied().unwrap_or(&[])
    }

    /// Read a field as `i64`, widening from its declared type. `None` if the
    /// field is opaque or runs past the command's bytes.
    pub fn get(&self, name: &str) -> Option<i64> {
        let f = self.fields().iter().find(|f| f.name == name)?;
        self.read(f)
    }

    pub fn read(&self, f: &Field) -> Option<i64> {
        let o = f.off as usize;
        let n = f.width as usize;
        if f.kind == FieldKind::Opaque || n == 0 || o + n > self.bytes.len() {
            return None;
        }
        let b = &self.bytes[o..o + n];
        Some(match f.kind {
            FieldKind::I8 => b[0] as i8 as i64,
            FieldKind::U8 => b[0] as i64,
            FieldKind::I16 => i16::from_le_bytes([b[0], b[1]]) as i64,
            FieldKind::U16 => u16::from_le_bytes([b[0], b[1]]) as i64,
            FieldKind::I32 => i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as i64,
            FieldKind::U32 => u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as i64,
            FieldKind::F32 => f32::from_le_bytes([b[0], b[1], b[2], b[3]]) as i64,
            FieldKind::Opaque => return None,
        })
    }
}

/// What a command does to the world, from the simulation's point of view.
///
/// The split is by the engine's own handler, not by guesswork: `process_camera`
/// writes only view state, `process_check_sums` writes only the desync ledger,
/// and everything under `Order`/`Economy`/`Build` mutates checksummed state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CommandClass {
    /// Mutates simulation state the checksum walks.
    Sim,
    /// Lockstep bookkeeping: `check_sums`, `check_random`, turn control.
    Lockstep,
    /// Local presentation only: camera, selection, chat, taunts.
    Presentation,
}

/// Classification per opcode.
///
/// `Presentation` is the conservative set — an opcode is only presentation if
/// its handler is one we have read and it touches no walked state. Everything
/// unread defaults to `Sim`, so the worklist over-reports rather than
/// under-reports.
pub fn classify(op: u8) -> CommandClass {
    match op {
        // ---- lockstep bookkeeping ----
        0x39 => CommandClass::Lockstep, // CheckSumsCommand   / process_check_sums
        0x3a => CommandClass::Lockstep, // NextCheckSumCommand / process_next_check_sum
        // ---- local view / UI, no walked state ----
        0x44 => CommandClass::Presentation, // ChatCommand
        0x48 => CommandClass::Presentation, // CameraCommand (zoom/x_loc/y_loc)
        // Everything else, including every opcode whose handler we have not
        // read, counts as simulation. Over-reporting the worklist is the safe
        // direction: a missed sim command is a silent divergence, an extra one
        // is a wasted afternoon.
        _ => CommandClass::Sim,
    }
}

/// The sim-visible orders the harness can hand to a simulation, decoded from
/// the PDB field layout.
///
/// This is deliberately a *thin* enum: the point of the harness is the loop and
/// the measurement, and inventing a rich order model before `don-sim` can act
/// on one would be modelling ahead of derivation. Opcodes not listed arrive as
/// `Raw` with their field table attached, which is enough for a consumer to
/// read any field by name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Order {
    /// `MoveToCommand` (0x07): `to_x`, `to_y`, plus angle/formation bytes.
    MoveTo { to_x: i32, to_y: i32, queued: i8 },
    /// `MoveNearCommand` (0x08): as `MoveTo` with a `tolerance`.
    MoveNear { to_x: i32, to_y: i32, tolerance: i32 },
    /// `AttackCommand` (0x04): `ox` is the ordering object, `whom` the target.
    Attack { ox: i32, whom: i32, ignore: i32 },
    /// `AttackGroundCommand` (0x09).
    AttackGround { to_x: i32, to_y: i32 },
    /// `HaltCommand` (0x0c) — a one-byte command, opcode only.
    Halt,
    /// `StanceCommand` (0x02).
    Stance { stance: i32 },
    /// Anything else: opcode plus its wire length.
    Raw { opcode: u8, len: u16 },
}

impl Order {
    pub fn decode(v: &CommandView<'_>) -> Order {
        let raw = Order::Raw { opcode: v.opcode, len: v.bytes.len() as u16 };
        let g = |n: &str| v.get(n);
        match v.opcode {
            0x07 => match (g("to_x"), g("to_y"), g("queued")) {
                (Some(x), Some(y), Some(q)) => {
                    Order::MoveTo { to_x: x as i32, to_y: y as i32, queued: q as i8 }
                }
                _ => raw,
            },
            0x08 => match (g("to_x"), g("to_y"), g("tolerance")) {
                (Some(x), Some(y), Some(t)) => {
                    Order::MoveNear { to_x: x as i32, to_y: y as i32, tolerance: t as i32 }
                }
                _ => raw,
            },
            0x09 => match (g("to_x"), g("to_y")) {
                (Some(x), Some(y)) => Order::AttackGround { to_x: x as i32, to_y: y as i32 },
                _ => raw,
            },
            0x04 => match (g("ox"), g("whom"), g("ignore")) {
                (Some(o), Some(w), Some(i)) => {
                    Order::Attack { ox: o as i32, whom: w as i32, ignore: i as i32 }
                }
                _ => raw,
            },
            0x0c => Order::Halt,
            0x02 => match g("stance") {
                Some(s) => Order::Stance { stance: s as i32 },
                None => raw,
            },
            _ => raw,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_covers_every_opcode() {
        assert_eq!(NUM_OPCODES, 82);
        assert_eq!(COMMAND_FIELDS.len(), NUM_OPCODES);
        assert_eq!(COMMAND_STRUCT[0x39], "CheckSumsCommand");
        assert_eq!(COMMAND_METHOD[0x39], "process_check_sums");
        assert_eq!(COMMAND_SIZEOF[0x39], 65);
    }

    #[test]
    fn checksum_fields_are_sixteen_u32_at_the_derived_offsets() {
        let f = COMMAND_FIELDS[0x39];
        assert_eq!(f.len(), 16);
        for (i, fd) in f.iter().enumerate() {
            assert_eq!(fd.off as usize, 1 + 4 * i, "{}", fd.name);
            assert_eq!(fd.kind, FieldKind::U32);
        }
        assert_eq!(f[15].name, "all_checksum");
    }

    #[test]
    fn typed_field_read_matches_the_bytes() {
        // MoveToCommand 0x07, 22 bytes.
        let mut b = vec![0u8; COMMAND_SIZEOF[0x07] as usize];
        b[0] = 0x07;
        let names: Vec<&str> = COMMAND_FIELDS[0x07].iter().map(|f| f.name).collect();
        assert!(names.contains(&"to_x"), "MoveToCommand fields: {names:?}");
        let fx = COMMAND_FIELDS[0x07].iter().find(|f| f.name == "to_x").unwrap();
        let o = fx.off as usize;
        b[o..o + 4].copy_from_slice(&(-1234i32).to_le_bytes());
        let v = CommandView::new(0x07, &b);
        assert_eq!(v.get("to_x"), Some(-1234));
        assert!(matches!(Order::decode(&v), Order::MoveTo { to_x: -1234, .. }));
    }

    #[test]
    fn a_short_buffer_never_reads_past_the_end() {
        let b = [0x07u8, 1, 2];
        let v = CommandView::new(0x07, &b);
        assert_eq!(v.get("to_x"), None);
        assert!(matches!(Order::decode(&v), Order::Raw { opcode: 0x07, len: 3 }));
    }

    #[test]
    fn classification_puts_checksums_outside_the_sim_set() {
        assert_eq!(classify(0x39), CommandClass::Lockstep);
        assert_eq!(classify(0x48), CommandClass::Presentation);
        assert_eq!(classify(0x07), CommandClass::Sim);
    }
}
