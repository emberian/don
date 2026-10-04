//! The retail `DataWalk` visitor, mirrored.
//!
//! `DataWalk` is a two-method pure-virtual class (vftable 0x00b2bcd8):
//! `walk_function(begin, end)` and `walk_test(&flag)`. `SaveGame`, `LoadGame`
//! and `CheckSum` are its three concrete implementations, so one literal
//! `walk_data` transcription per class implements load, save and lockstep
//! checksum. Vftable-adjacent fields used by the walkers: +4 direction
//! (loading), +8 checksum flag, +0xc section mask, +0x10 running adler, +0x14
//! bytes walked.

use std::fmt;

#[derive(Debug)]
pub struct WalkError {
    /// Class or section being walked, e.g. "GraphicEvents" or "Unit[3].orders".
    pub class: &'static str,
    /// Retail VA of the walk_data body being transcribed, when known.
    pub va: u32,
    /// Stream offset where the walk stopped.
    pub offset: usize,
    pub detail: String,
    /// Path of the last recorded span (the field that tripped the error).
    pub last_span: String,
}

impl WalkError {
    pub fn new(class: &'static str, va: u32, offset: usize, detail: String) -> Self {
        WalkError { class, va, offset, detail: detail.into(), last_span: String::new() }
    }
}

impl fmt::Display for WalkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} (walk_data {:#010x}) stopped at stream offset {:#x} after {}: {}",
            self.class, self.va, self.offset, self.last_span, self.detail
        )
    }
}

impl std::error::Error for WalkError {}

/// One attributed span of the file: which class/field produced these bytes.
#[derive(Debug, Clone)]
pub struct Span {
    pub path: String,
    pub offset: usize,
    pub len: usize,
    /// VA of the owning walk_data, when known.
    pub va: u32,
}

/// The two-method virtual interface every walk_data calls into.
///
/// `walk_bytes` mirrors `walk_function(begin, end)`; `walk_tag` mirrors
/// `walk_test(&byte)`: Save writes the byte, Load reads and preserves it,
/// CheckSum ignores it entirely.
pub trait DataWalk {
    fn walk_bytes(&mut self, path: &str, buf: &mut [u8]) -> Result<(), WalkError>;
    /// `walk_test`: the tag value is produced by the caller on save; on load
    /// the observed byte is returned for the caller to check/preserve.
    fn walk_tag(&mut self, path: &str, tag: &mut u8) -> Result<(), WalkError>;
    fn is_loading(&self) -> bool;
    fn is_checksum(&self) -> bool;
    fn mask(&self) -> u32;
    fn pos(&self) -> usize;
    /// Bytes left in the stream; only meaningful for [`Loader`].
    fn remaining(&self) -> usize { 0 }
    fn fail(&self, class: &'static str, va: u32, detail: String) -> WalkError {
        WalkError::new(class, va, self.pos(), detail)
    }
}

/// LoadGame: reads a byte slice; every op is recorded as a [`Span`].
pub struct Loader<'a> {
    pub buf: &'a [u8],
    pub pos: usize,
    pub spans: Vec<Span>,
}

impl<'a> Loader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Loader { buf, pos: 0, spans: Vec::new() }
    }

    pub fn take(&mut self, path: &str, va: u32, n: usize, class: &'static str) -> Result<&'a [u8], WalkError> {
        if self.pos + n > self.buf.len() {
            let mut e = WalkError::new(
                class,
                va,
                self.pos,
                format!("walk of {n} bytes overruns {}-byte stream", self.buf.len()),
            );
            e.last_span = self.spans.last().map(|s| s.path.clone()).unwrap_or_default();
            return Err(e);
        }
        let s = &self.buf[self.pos..self.pos + n];
        self.spans.push(Span { path: path.to_string(), offset: self.pos, len: n, va });
        self.pos += n;
        Ok(s)
    }

    pub fn u8(&mut self, path: &str, va: u32, class: &'static str) -> Result<u8, WalkError> {
        Ok(self.take(path, va, 1, class)?[0])
    }
    pub fn u16(&mut self, path: &str, va: u32, class: &'static str) -> Result<u16, WalkError> {
        Ok(u16::from_le_bytes(self.take(path, va, 2, class)?.try_into().unwrap()))
    }
    pub fn i16(&mut self, path: &str, va: u32, class: &'static str) -> Result<i16, WalkError> {
        Ok(i16::from_le_bytes(self.take(path, va, 2, class)?.try_into().unwrap()))
    }
    pub fn u32(&mut self, path: &str, va: u32, class: &'static str) -> Result<u32, WalkError> {
        Ok(u32::from_le_bytes(self.take(path, va, 4, class)?.try_into().unwrap()))
    }
    pub fn i32(&mut self, path: &str, va: u32, class: &'static str) -> Result<i32, WalkError> {
        Ok(i32::from_le_bytes(self.take(path, va, 4, class)?.try_into().unwrap()))
    }
    pub fn f32(&mut self, path: &str, va: u32, class: &'static str) -> Result<f32, WalkError> {
        Ok(f32::from_le_bytes(self.take(path, va, 4, class)?.try_into().unwrap()))
    }

    /// `String::walk_data` 0x00a1b2d0: u32 UTF-16 code-unit count, then data.
    /// Returns the raw code units; the stream bytes are preserved verbatim.
    pub fn wstr(&mut self, path: &str, va: u32, class: &'static str) -> Result<Vec<u16>, WalkError> {
        let n = self.u32(path, va, class)? as usize;
        if n > (1 << 20) {
            return Err(self.fail(class, va, format!("absurd String length {n}")));
        }
        let raw = self.take(path, va, 2 * n, class)?;
        Ok(raw.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect())
    }
}

impl<'a> DataWalk for Loader<'a> {
    fn walk_bytes(&mut self, path: &str, buf: &mut [u8]) -> Result<(), WalkError> {
        let n = buf.len();
        if self.pos + n > self.buf.len() {
            return Err(WalkError::new("Loader", 0, self.pos, format!("overrun reading {n} bytes")));
        }
        buf.copy_from_slice(&self.buf[self.pos..self.pos + n]);
        self.spans.push(Span { path: path.to_string(), offset: self.pos, len: n, va: 0 });
        self.pos += n;
        Ok(())
    }
    fn walk_tag(&mut self, path: &str, tag: &mut u8) -> Result<(), WalkError> {
        if self.pos >= self.buf.len() {
            return Err(WalkError::new("Loader", 0, self.pos, "overrun reading tag".to_string()));
        }
        *tag = self.buf[self.pos];
        self.spans.push(Span { path: format!("{path}<tag>"), offset: self.pos, len: 1, va: 0 });
        self.pos += 1;
        Ok(())
    }
    fn is_loading(&self) -> bool { true }
    fn is_checksum(&self) -> bool { false }
    fn mask(&self) -> u32 { 0 }
    fn pos(&self) -> usize { self.pos }
    fn remaining(&self) -> usize { self.buf.len() - self.pos }
    fn fail(&self, class: &'static str, va: u32, detail: String) -> WalkError {
        let mut e = WalkError::new(class, va, self.pos(), detail.into());
        e.last_span = self.spans.last().map(|s| s.path.clone()).unwrap_or_default();
        e
    }
}

/// SaveGame: writes the byte image. With byte-image state the emitted stream
/// is byte-identical to the loaded input by construction; the traversal order
/// is what must be right, and that is what the walkers exercise.
pub struct Saver {
    pub out: Vec<u8>,
}

impl DataWalk for Saver {
    fn walk_bytes(&mut self, _path: &str, buf: &mut [u8]) -> Result<(), WalkError> {
        self.out.extend_from_slice(buf);
        Ok(())
    }
    fn walk_tag(&mut self, _path: &str, tag: &mut u8) -> Result<(), WalkError> {
        self.out.push(*tag);
        Ok(())
    }
    fn is_loading(&self) -> bool { false }
    fn is_checksum(&self) -> bool { false }
    fn mask(&self) -> u32 { 0 }
    fn pos(&self) -> usize { self.out.len() }
}

/// Retail adler-32 (zlib flavour, mod 65521) as used by `CheckSum::walk_function`.
pub fn adler32(adler: u32, data: &[u8]) -> u32 {
    let mut a = adler & 0xffff;
    let mut b = (adler >> 16) & 0xffff;
    for chunk in data.chunks(5552) {
        for &x in chunk {
            a += x as u32;
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    (b << 16) | a
}

/// CheckSum visitor: +0x10 adler (reset to 1 per channel), +0x14 byte count.
/// `walk_test` is a no-op, and checksum-only omissions are gated by
/// `is_checksum()`.
#[derive(Default)]
pub struct CheckSum {
    pub adler: u32,
    pub bytes: u64,
    pub pos: usize,
    pub mask: u32,
}

impl CheckSum {
    pub fn new(mask: u32) -> Self {
        CheckSum { adler: 1, bytes: 0, pos: 0, mask }
    }

    /// Feed bytes into the checksum without counting them as walked — retail
    /// does this where check_* calls `FUN_005089d0` (raw adler32 on +0x10)
    /// directly instead of `walk_function` (e.g. check_groups' last_group).
    pub fn feed(&mut self, buf: &[u8]) {
        self.adler = adler32(self.adler, buf);
    }
}

impl DataWalk for CheckSum {
    fn walk_bytes(&mut self, _path: &str, buf: &mut [u8]) -> Result<(), WalkError> {
        self.adler = adler32(self.adler, buf);
        self.bytes += buf.len() as u64;
        self.pos += buf.len();
        Ok(())
    }
    fn walk_tag(&mut self, _path: &str, _tag: &mut u8) -> Result<(), WalkError> {
        Ok(())
    }
    fn is_loading(&self) -> bool { false }
    fn is_checksum(&self) -> bool { true }
    fn mask(&self) -> u32 { self.mask }
    fn pos(&self) -> usize { self.pos }
}
