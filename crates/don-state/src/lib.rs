//! `don-state`: retail-faithful container for Descent of Nations simulation
//! state, built around the engine's own `DataWalk` visitor architecture.
//!
//! One owned byte-image state tree ([`sections::Save`]) with one set of
//! literal per-class `walk_data` transcriptions implements `.svx` load,
//! `.svx` save and the fifteen lockstep checksum channels — exactly as
//! retail's `LoadGame`/`SaveGame`/`CheckSum` share each `walk_data`.
//!
//! Objects are byte images, not host-layout structs; traversal is auditable
//! against the disassembly, allocation history is preserved verbatim, and
//! unknown bodies fail closed with class/VA/offset rather than being guessed.

pub mod check_all;
pub mod container;
pub mod generated;
pub mod prim;
pub mod sections;
pub mod spandiff;
pub mod tick;
pub mod walk;

pub use check_all::{CheckSums, CHANNEL_NAMES};
pub use sections::Save;
pub use walk::{adler32, CheckSum, DataWalk, Loader, Saver, Span, WalkError};

/// Result of loading a decompressed SaveGame stream.
pub struct SaveImage<'a> {
    /// The owned state tree produced by `WalkDataGame::walk_data` over the
    /// consumed prefix — every walked byte is stored in it.
    pub state: Save,
    /// The original decompressed bytes.
    pub raw: &'a [u8],
    /// Bytes consumed by the successful prefix of `WalkDataGame::walk_data`.
    pub consumed: usize,
    /// Per-op attribution: which class/field span produced each byte.
    pub spans: Vec<Span>,
}

pub fn load(bytes: &[u8]) -> Result<SaveImage<'_>, WalkError> {
    let (state, l) = sections::load_save(bytes)?;
    Ok(SaveImage {
        state,
        raw: bytes,
        consumed: l.pos,
        spans: l.spans,
    })
}

/// Re-emit the saved stream by driving the same state tree through `Saver`.
/// Gate 1 is real here: any divergence in traversal order or grammar makes
/// the emitted bytes differ from the input.
pub fn save(state: &mut Save) -> Result<Vec<u8>, WalkError> {
    let mut s = Saver { out: Vec::new() };
    state.walk_data(&mut s)?;
    Ok(s.out)
}
