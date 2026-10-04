//! The retail container grammars shared by many walk_data bodies, as
//! byte-owning state. Every header field (length, capacity, increment, flags,
//! repeated history) is walked verbatim — allocation history is preserved,
//! never normalized.
//!
//! All helpers are direction-agnostic: the same walk serves `Loader`,
//! `Saver` and `CheckSum`. The only place the two stream directions differ is
//! `w.is_loading()`, where counts come from the stream and storage is
//! allocated before the element loop; on save the count is the Vec length.

use crate::walk::{DataWalk, WalkError};

type R = Result<(), WalkError>;

// ---------------------------------------------------------------------------
// scalar + blob leaves
// ---------------------------------------------------------------------------

macro_rules! wint {
    ($name:ident, $t:ty, $n:expr) => {
        pub fn $name(w: &mut dyn DataWalk, path: &str, _va: u32, v: &mut $t) -> R {
            let mut b = v.to_le_bytes();
            w.walk_bytes(path, &mut b)?;
            *v = <$t>::from_le_bytes(b);
            Ok(())
        }
    };
}
wint!(w_u8, u8, 1);
wint!(w_i8, i8, 1);
wint!(w_u16, u16, 2);
wint!(w_i16, i16, 2);
wint!(w_u32, u32, 4);
wint!(w_i32, i32, 4);

/// `walk_test`: Load preserves the observed byte into `tag`, Save emits the
/// stored byte, CheckSum ignores it.
pub fn tag(w: &mut dyn DataWalk, path: &str, tag: &mut u8) -> R {
    w.walk_tag(path, tag)
}

/// A fixed-size direct byte range. On load the Vec is (re)sized then filled;
/// on save its stored bytes are emitted.
pub fn take(
    w: &mut dyn DataWalk,
    path: &str,
    buf: &mut Vec<u8>,
    n: usize,
    class: &'static str,
    va: u32,
) -> R {
    if w.is_loading() {
        buf.clear();
        buf.resize(n, 0);
    } else if buf.len() != n {
        return Err(w.fail(
            class,
            va,
            format!("{path}: stored {} bytes, walk wants {n}", buf.len()),
        ));
    }
    w.walk_bytes(path, buf)
}

/// A variable direct byte range whose size was determined by a preceding
/// count the caller already walked.
pub fn take_n(
    w: &mut dyn DataWalk,
    path: &str,
    buf: &mut Vec<u8>,
    n: usize,
    class: &'static str,
    va: u32,
) -> R {
    take(w, path, buf, n, class, va)
}

/// `String::walk_data` 0x00a1d2d0: u32 UTF-16 code-unit count, then data.
/// Stored as raw code units; the stream image is preserved verbatim.
pub fn wstr(
    w: &mut dyn DataWalk,
    path: &str,
    s: &mut Vec<u16>,
    class: &'static str,
    va: u32,
) -> R {
    let mut n = if w.is_loading() { 0 } else { s.len() as u32 };
    w_u32(w, path, va, &mut n)?;
    if w.is_loading() {
        if n > (1 << 20) {
            return Err(w.fail(class, va, format!("absurd String length {n}")));
        }
        s.clear();
        s.resize(n as usize, 0);
    }
    let mut b: Vec<u8> = s.iter().flat_map(|u| u.to_le_bytes()).collect();
    take(w, path, &mut b, n as usize * 2, class, va)?;
    if w.is_loading() {
        for (i, c) in b.chunks_exact(2).enumerate() {
            s[i] = u16::from_le_bytes([c[0], c[1]]);
        }
    }
    Ok(())
}

/// `BitMask<N>` / `VariableBuffer`: i32 bits, i32 payload size, u8 data[size].
#[derive(Default, Clone)]
pub struct BitMask {
    pub bits: i32,
    pub size: i32,
    pub data: Vec<u8>,
}

impl BitMask {
    pub fn walk(&mut self, w: &mut dyn DataWalk, path: &str, class: &'static str, va: u32) -> R {
        if !w.is_loading() {
            self.size = self.data.len() as i32;
        }
        w_i32(w, path, va, &mut self.bits)?;
        w_i32(w, path, va, &mut self.size)?;
        if w.is_loading()
            && (self.bits < 0 || self.size < 0 || self.size > 1 << 20 || self.bits > self.size * 8)
        {
            return Err(w.fail(
                class,
                va,
                format!("BitMask bits={} size={}", self.bits, self.size),
            ));
        }
        take(w, path, &mut self.data, self.size as usize, class, va)
    }
}

// ---------------------------------------------------------------------------
// element protocol
// ---------------------------------------------------------------------------

/// A walked element/section body. `path` carries the span-attribution name.
pub trait Body {
    fn walk(&mut self, path: &str, w: &mut dyn DataWalk) -> R;
}

/// Fixed-size row: data is an opaque Vec<u8> of `SIZE` bytes.
#[derive(Clone)]
pub struct Row<const SIZE: usize> {
    pub data: Vec<u8>,
}

impl<const SIZE: usize> Default for Row<SIZE> {
    fn default() -> Self {
        Row { data: vec![0; SIZE] }
    }
}

impl<const SIZE: usize> Body for Row<SIZE> {
    fn walk(&mut self, path: &str, w: &mut dyn DataWalk) -> R {
        take(w, path, &mut self.data, SIZE, "Row", 0)
    }
}

// ---------------------------------------------------------------------------
// containers
// ---------------------------------------------------------------------------

/// `SimpleArray<T>`: i32 length; if nonzero, i32 capacity + i16 increment +
/// u8 flags + `length` fixed-size elements (opaque byte image).
#[derive(Default, Clone)]
pub struct SimpleVec {
    pub len: i32,
    pub cap: i32,
    pub inc: i16,
    pub flags: u8,
    pub data: Vec<u8>,
}

impl SimpleVec {
    pub fn walk(
        &mut self,
        w: &mut dyn DataWalk,
        path: &str,
        class: &'static str,
        va: u32,
        esz: usize,
    ) -> R {
        if !w.is_loading() {
            self.len = (self.data.len() / esz) as i32;
        }
        w_i32(w, path, va, &mut self.len)?;
        if w.is_loading() && !(0..=4_000_000).contains(&self.len) {
            return Err(w.fail(class, va, format!("SimpleArray length {}", self.len)));
        }
        if self.len == 0 {
            return Ok(());
        }
        w_i32(w, path, va, &mut self.cap)?;
        w_i16(w, path, va, &mut self.inc)?;
        w_u8(w, path, va, &mut self.flags)?;
        if w.is_loading() && self.cap < self.len {
            return Err(w.fail(
                class,
                va,
                format!("SimpleArray capacity {} < length {}", self.cap, self.len),
            ));
        }
        take(w, path, &mut self.data, self.len as usize * esz, class, va)
    }
}

/// `Array<T>`: same header as SimpleVec; elements are walked bodies.
#[derive(Default, Clone)]
pub struct Arr<T> {
    pub len: i32,
    pub cap: i32,
    pub inc: i16,
    pub flags: u8,
    pub elems: Vec<T>,
}

impl<T: Default + Body> Arr<T> {
    pub fn walk(&mut self, w: &mut dyn DataWalk, path: &str, class: &'static str, va: u32) -> R {
        if !w.is_loading() {
            self.len = self.elems.len() as i32;
        }
        w_i32(w, path, va, &mut self.len)?;
        if w.is_loading() && !(0..=4_000_000).contains(&self.len) {
            return Err(w.fail(class, va, format!("Array length {}", self.len)));
        }
        if self.len == 0 {
            return Ok(());
        }
        w_i32(w, path, va, &mut self.cap)?;
        w_i16(w, path, va, &mut self.inc)?;
        w_u8(w, path, va, &mut self.flags)?;
        if w.is_loading() && self.cap < self.len {
            return Err(w.fail(
                class,
                va,
                format!("Array capacity {} < length {}", self.cap, self.len),
            ));
        }
        if w.is_loading() {
            self.elems.clear();
            self.elems.resize_with(self.len as usize, T::default);
        }
        for (i, e) in self.elems.iter_mut().enumerate() {
            e.walk(&format!("{path}[{i}]"), w)?;
        }
        Ok(())
    }
}

/// `PtrArray<T>` / `ObjectArray<T>` over a presence-planed element set:
/// header, boolean presence plane, repeated capacity+increment, then a body
/// per present row. `elems` is indexed by slot; absent slots stay `None`.
#[derive(Clone)]
pub struct PtrVec<T> {
    pub len: i32,
    pub cap: i32,
    pub inc: i16,
    pub flags: u8,
    pub present: Vec<u8>,
    pub cap2: i32,
    pub inc2: i16,
    pub elems: Vec<Option<T>>,
}

impl<T> Default for PtrVec<T> {
    fn default() -> Self {
        PtrVec {
            len: 0,
            cap: 0,
            inc: 0,
            flags: 0,
            present: Vec::new(),
            cap2: 0,
            inc2: 0,
            elems: Vec::new(),
        }
    }
}

impl<T: Default + Body> PtrVec<T> {
    pub fn walk(&mut self, w: &mut dyn DataWalk, path: &str, class: &'static str, va: u32) -> R {
        if !w.is_loading() {
            self.len = self.elems.len() as i32;
        }
        w_i32(w, path, va, &mut self.len)?;
        if w.is_loading() && !(0..=4_000_000).contains(&self.len) {
            return Err(w.fail(class, va, format!("PtrArray length {}", self.len)));
        }
        if self.len == 0 {
            return Ok(());
        }
        w_i32(w, path, va, &mut self.cap)?;
        w_i16(w, path, va, &mut self.inc)?;
        w_u8(w, path, va, &mut self.flags)?;
        if w.is_loading() && self.cap < self.len {
            return Err(w.fail(
                class,
                va,
                format!("PtrArray capacity {} < length {}", self.cap, self.len),
            ));
        }
        take(w, path, &mut self.present, self.len as usize, class, va)?;
        if self.present.iter().any(|&v| v > 1) {
            return Err(w.fail(class, va, "non-boolean presence byte".to_string()));
        }
        w_i32(w, path, va, &mut self.cap2)?;
        w_i16(w, path, va, &mut self.inc2)?;
        if self.cap2 != self.cap || self.inc2 != self.inc {
            return Err(w.fail(
                class,
                va,
                format!(
                    "repeated history ({},{}) != header ({},{})",
                    self.cap2, self.inc2, self.cap, self.inc
                ),
            ));
        }
        if w.is_loading() {
            self.elems.clear();
            self.elems.resize_with(self.len as usize, || None);
        }
        for (i, &p) in self.present.clone().iter().enumerate() {
            if p != 0 {
                self.elems[i]
                    .get_or_insert_with(T::default)
                    .walk(&format!("{path}[{i}]"), w)?;
            }
        }
        Ok(())
    }
}
