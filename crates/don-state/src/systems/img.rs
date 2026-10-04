//! Shared byte-image accessors for the `systems/*` step bodies.
//!
//! Every retail object is a byte image addressed by PDB offset, and every
//! serialized `sections::*` record stores *pieces* of that image as separate
//! `Vec<u8>` sub-ranges (the `walk_data` gated bodies). Each step module so
//! far re-implemented the same "image offset → sub-vector + index" mapping
//! (`build_process::Img`, `objects_inc_time::BImg`, `objects_process::UImg`,
//! `armies_process::ArmyRef`, `leaders_process::ld_i32`, …), the same
//! `TypeImg`/`type_rec` reader over `Rules.types`, the same `constant()`
//! reader and the same `get_i32`/`put_i32` byte helpers. This module is the
//! single correct copy.
//!
//! # Migration note (for module owners)
//!
//! Nothing here is wired into an existing module yet; each owner migrates
//! their own file. The intended one-line replacements:
//!
//! ```text
//!   struct Img<'a>(&'a mut Build) + impl    →  use super::img::Img;  Img(&mut *b)
//!   struct UImg<'a>(&'a mut Unit)           →  Img(&mut *u)           (same method names)
//!   struct BImg<'a>(&'a mut Build)          →  Img(&mut *b)
//!   fn constant(save, off)                  →  img::constant(save, off)
//!   struct TypeImg / fn type_rec(save, idx) →  img::type_rec(save, idx)   (kind-aware, see below)
//!   fn get_i32/put_i32/get_i16/put_i16/u8   →  img::get_i32 …
//!   fn leader_u8/i16/i32(save, who, off)    →  img::leader_u8 …  /  Img(&mut leader)
//!   ld_i32(l, img) / ld_put_i32             →  Img(&*l).i32(img) / Img(&mut *l).set_i32(img, v)
//!   fn frame / unit_mark / build_mark / wall_mark / game_random  →  img::… (identical bodies)
//! ```
//!
//! Behavioural differences from the per-module copies, all intentional:
//!
//! * `Img` is one generic wrapper over any [`Layout`] implementor (`Build`,
//!   `Unit`, `Animal`, `Leader`, `City`, `Group`, `Army`) and works over
//!   `&T` (reads) or `&mut T` (reads + writes). Reads cross-check that the
//!   whole scalar lies inside one serialized piece; a read that straddles a
//!   gap panics (`try_*` variants return `None`) instead of silently reading
//!   the next field.
//! * `Build` writes to `+0x7f` (`founder`) / `+0x83` (`max_age`) keep the
//!   duplicated `head[0..2]` in sync, as `build_process::Img::set_u8` did —
//!   but for every write width, not only `set_u8`.
//! * [`TypeImg`] is **kind-aware**. `TypeRec.ext` is a different image range
//!   per [`TypeRuleKind`], and for *unit* types it is piecewise:
//!   `[0x2b4..0x2cc) ++ [0x2d4..0x2dc) ++ [0x2dc..0x2e0) ++ [0x2e0..0x5d4)`
//!   — there is an 8-byte gap at `0x2cc..0x2d4` that `Type::walk_rules_data`
//!   does not emit (found by the `objects_inc_time` lane; `objects_process`
//!   applied the shift, `build_process` did not and would mis-read any unit
//!   type offset ≥ 0x2cc). Tech/Spell `ext` starts at image `0x1c8`, not
//!   `0x2b4`. The gap itself and unserialized offsets read as `None`.
//! * `constant()` prefers the typed `Rules.constants` image and falls back to
//!   the earlier direct `Save.constants` walk of the same object
//!   (`game_daemon`/`objects_process` behaviour; `build_process` read only the
//!   direct copy — both hold the same bytes on a well-formed save).

// Nothing is wired in until the module owners migrate; drop this once the
// first caller lands.
#![allow(dead_code)]

use crate::sections::{Animal, Army, Build, City, Group, Leader, Obj, Save, TypeRec, TypeRuleKind, Unit};
use std::ops::{Deref, DerefMut};

// ---------------------------------------------------------------------------
// Little-endian scalar helpers over any byte buffer
// ---------------------------------------------------------------------------

pub(crate) fn get_u8(buf: &[u8], off: usize) -> u8 {
    buf[off]
}

pub(crate) fn get_i16(buf: &[u8], off: usize) -> i16 {
    i16::from_le_bytes([buf[off], buf[off + 1]])
}

pub(crate) fn get_u16(buf: &[u8], off: usize) -> u16 {
    get_i16(buf, off) as u16
}

pub(crate) fn get_i32(buf: &[u8], off: usize) -> i32 {
    i32::from_le_bytes(buf[off..off + 4].try_into().unwrap())
}

pub(crate) fn get_u32(buf: &[u8], off: usize) -> u32 {
    get_i32(buf, off) as u32
}

pub(crate) fn put_u8(buf: &mut [u8], off: usize, v: u8) {
    buf[off] = v;
}

pub(crate) fn put_i16(buf: &mut [u8], off: usize, v: i16) {
    buf[off..off + 2].copy_from_slice(&v.to_le_bytes());
}

pub(crate) fn put_u16(buf: &mut [u8], off: usize, v: u16) {
    put_i16(buf, off, v as i16)
}

pub(crate) fn put_i32(buf: &mut [u8], off: usize, v: i32) {
    buf[off..off + 4].copy_from_slice(&v.to_le_bytes());
}

pub(crate) fn put_u32(buf: &mut [u8], off: usize, v: u32) {
    put_i32(buf, off, v as i32)
}

/// Bounds-checked `get_i32`.
pub(crate) fn try_i32(buf: &[u8], off: usize) -> Option<i32> {
    buf.get(off..off + 4).map(|b| i32::from_le_bytes(b.try_into().unwrap()))
}

/// Bounds-checked `get_i16`.
pub(crate) fn try_i16(buf: &[u8], off: usize) -> Option<i16> {
    buf.get(off..off + 2).map(|b| i16::from_le_bytes([b[0], b[1]]))
}

// ---------------------------------------------------------------------------
// Image layouts: retail image offset → serialized sub-vector + index
// ---------------------------------------------------------------------------

/// A serialized record whose byte pieces can be addressed by retail image
/// offset. `slot` maps an offset onto the owning piece; pieces are
/// contiguous image ranges, so a bounds check on the returned slice is the
/// "does this scalar lie inside one piece" check.
///
/// `extra_byte` covers bytes that the walker stores as *typed* fields rather
/// than byte pieces (`SubObject.flags` u8 at +0x08, `LeaderData.flags` i32
/// at +0x00, …) so they are still readable by image offset; writes to them
/// go through `set_extra_byte`.
pub(crate) trait Layout {
    /// PDB class name for diagnostics.
    const CLASS: &'static str;
    /// All gated pieces present at their walked length.
    fn complete(&self) -> bool;
    fn slot(&self, off: usize) -> Option<(&[u8], usize)>;
    fn slot_mut(&mut self, off: usize) -> Option<(&mut [u8], usize)>;
    fn extra_byte(&self, _off: usize) -> Option<u8> {
        None
    }
    fn set_extra_byte(&mut self, _off: usize, _x: u8) -> bool {
        false
    }
    /// Called after every write so duplicated fields stay in sync.
    fn after_write(&mut self) {}
}

/// Object-plane records share the `SubObject` (+0x08..+0x1c) and
/// `ObjectData` (+0x20..+0x42) prefix; this marker enables the common
/// `who/o/z/x/y/ptype/uid` accessors on [`Img`].
pub(crate) trait ObjLayout: Layout {}

/// Build / Wall image (`BuildData`/`WallData`, `Build::walk_data`):
///
/// ```text
///   +0x08         SubObject.flags                          base.sub.flags (extra)
///   +0x09..+0x1c  SubObject  who,o,z,x,y,ptype_index       base.sub.body   (19 B)
///   +0x20..+0x42  ObjectData myhits..launch_frames         base.mid        (34 B)
///   +0x48..+0x66  WallData   job_counter..demolition       wall_body       (30 B)
///   +0x6c         orig_type                                orig_type (typed i32, not here)
///   +0x70..+0x86  BuildData  gather_down..infiltrate2      body            (22 B)
///   +0x7f,+0x83   founder, max_age (also emitted ahead)    head[0], head[1] (mirrored)
///   queue rows    QueueItem 18 B each                      queue (use `queue_row`)
/// ```
impl Layout for Build {
    const CLASS: &'static str = "Build";
    fn complete(&self) -> bool {
        self.base.sub.body.len() == 19 && self.base.mid.len() == 34 && self.wall_body.len() == 30 && self.body.len() == 22
    }
    fn slot(&self, off: usize) -> Option<(&[u8], usize)> {
        Some(match off {
            0x09..=0x1b => (&self.base.sub.body[..], off - 0x09),
            0x20..=0x41 => (&self.base.mid[..], off - 0x20),
            0x48..=0x65 => (&self.wall_body[..], off - 0x48),
            0x70..=0x85 => (&self.body[..], off - 0x70),
            _ => return None,
        })
    }
    fn slot_mut(&mut self, off: usize) -> Option<(&mut [u8], usize)> {
        Some(match off {
            0x09..=0x1b => (&mut self.base.sub.body[..], off - 0x09),
            0x20..=0x41 => (&mut self.base.mid[..], off - 0x20),
            0x48..=0x65 => (&mut self.wall_body[..], off - 0x48),
            0x70..=0x85 => (&mut self.body[..], off - 0x70),
            _ => return None,
        })
    }
    fn extra_byte(&self, off: usize) -> Option<u8> {
        (off == 0x08).then_some(self.base.sub.flags)
    }
    fn set_extra_byte(&mut self, off: usize, x: u8) -> bool {
        if off == 0x08 {
            self.base.sub.flags = x;
            return true;
        }
        false
    }
    fn after_write(&mut self) {
        // founder (+0x7f) and max_age (+0x83) are serialized twice.
        if self.body.len() == 22 && self.head.len() == 2 {
            self.head[0] = self.body[0x7f - 0x70];
            self.head[1] = self.body[0x83 - 0x70];
        }
    }
}
impl ObjLayout for Build {}

/// Unit image (`UnitData`, `Unit::walk_data` / FUN_0060cf40):
///
/// ```text
///   +0x08         SubObject.flags                          base.sub.flags (extra)
///   +0x09..+0x1c  SubObject  who,o,z,x,y,ptype             base.sub.body   (19 B)
///   +0x20..+0x42  ObjectData myhits..launch_frames         base.mid        (34 B)
///   +0x48..+0xb7  UnitData   collide_frame..play           body            (111 B)
///   Guy rows      GuyData +0x08..+0xa3 (155 B)             guys.elems[i].data (use `guy_row`)
/// ```
impl Layout for Unit {
    const CLASS: &'static str = "Unit";
    fn complete(&self) -> bool {
        self.base.sub.body.len() == 19 && self.base.mid.len() == 34 && self.body.len() == 111
    }
    fn slot(&self, off: usize) -> Option<(&[u8], usize)> {
        Some(match off {
            0x09..=0x1b => (&self.base.sub.body[..], off - 0x09),
            0x20..=0x41 => (&self.base.mid[..], off - 0x20),
            0x48..=0xb6 => (&self.body[..], off - 0x48),
            _ => return None,
        })
    }
    fn slot_mut(&mut self, off: usize) -> Option<(&mut [u8], usize)> {
        Some(match off {
            0x09..=0x1b => (&mut self.base.sub.body[..], off - 0x09),
            0x20..=0x41 => (&mut self.base.mid[..], off - 0x20),
            0x48..=0xb6 => (&mut self.body[..], off - 0x48),
            _ => return None,
        })
    }
    fn extra_byte(&self, off: usize) -> Option<u8> {
        (off == 0x08).then_some(self.base.sub.flags)
    }
    fn set_extra_byte(&mut self, off: usize, x: u8) -> bool {
        if off == 0x08 {
            self.base.sub.flags = x;
            return true;
        }
        false
    }
}
impl ObjLayout for Unit {}

/// Animal image: the inherited [`Unit`] layout plus `AnimalData`
/// `+0x150..+0x155` (`ox` i16, `whom` i16, `aid` i8; `Animal::walk_data`
/// 0x005d7ea0) in `tail`.
impl Layout for Animal {
    const CLASS: &'static str = "Animal";
    fn complete(&self) -> bool {
        self.unit.complete() && self.tail.len() == 5
    }
    fn slot(&self, off: usize) -> Option<(&[u8], usize)> {
        match off {
            0x150..=0x154 => Some((&self.tail[..], off - 0x150)),
            _ => self.unit.slot(off),
        }
    }
    fn slot_mut(&mut self, off: usize) -> Option<(&mut [u8], usize)> {
        match off {
            0x150..=0x154 => Some((&mut self.tail[..], off - 0x150)),
            _ => self.unit.slot_mut(off),
        }
    }
    fn extra_byte(&self, off: usize) -> Option<u8> {
        self.unit.extra_byte(off)
    }
    fn set_extra_byte(&mut self, off: usize, x: u8) -> bool {
        self.unit.set_extra_byte(off, x)
    }
}
impl ObjLayout for Animal {}

/// `LeaderData` image (`LeaderData::walk_data` 0x006d6750; slots at
/// 0x00e3a390 stride 0x6eec):
///
/// ```text
///   +0x00..+0x04  flags   (typed i32 field; readable here, write via field)
///   +0x04..+0x08  flags2  (typed i32 field; readable here, write via field)
///   +0x08..+0x692a body                                      body (0x6922 B)
/// ```
/// `data_encrypted` (62 dwords) is not image-addressed; see
/// `leaders_process::enc_i32`.
impl Layout for Leader {
    const CLASS: &'static str = "LeaderData";
    fn complete(&self) -> bool {
        self.body.len() == 0x6922
    }
    fn slot(&self, off: usize) -> Option<(&[u8], usize)> {
        (off >= 8).then(|| (&self.body[..], off - 8))
    }
    fn slot_mut(&mut self, off: usize) -> Option<(&mut [u8], usize)> {
        (off >= 8).then(|| (&mut self.body[..], off - 8))
    }
    fn extra_byte(&self, off: usize) -> Option<u8> {
        match off {
            0..=3 => Some(self.flags.to_le_bytes()[off]),
            4..=7 => Some(self.flags2.to_le_bytes()[off - 4]),
            _ => None,
        }
    }
}

/// `CityData` image (`City::walk_data` 0x00735410): `flags` u16 at +0x04
/// (typed field, readable here), `pod` = `+0x06..+0x72` (108 B).
impl Layout for City {
    const CLASS: &'static str = "CityData";
    fn complete(&self) -> bool {
        self.pod.len() == 108
    }
    fn slot(&self, off: usize) -> Option<(&[u8], usize)> {
        (off >= 6).then(|| (&self.pod[..], off - 6))
    }
    fn slot_mut(&mut self, off: usize) -> Option<(&mut [u8], usize)> {
        (off >= 6).then(|| (&mut self.pod[..], off - 6))
    }
    fn extra_byte(&self, off: usize) -> Option<u8> {
        match off {
            4..=5 => Some(self.flags.to_le_bytes()[off - 4]),
            _ => None,
        }
    }
}

/// `Group` image (`Group::walk_data` 0x00708400): the 72-byte header at
/// `+0x00..+0x48` is `hdr`; the per-member arrays (`list`, `off_x`, …) are
/// pointer targets in retail and are typed fields here.
impl Layout for Group {
    const CLASS: &'static str = "Group";
    fn complete(&self) -> bool {
        self.hdr.len() == 72
    }
    fn slot(&self, off: usize) -> Option<(&[u8], usize)> {
        Some((&self.hdr[..], off))
    }
    fn slot_mut(&mut self, off: usize) -> Option<(&mut [u8], usize)> {
        Some((&mut self.hdr[..], off))
    }
}

/// `Army` image (`Army::walk_data` 0x006f3700): `valid` i16 at +0x00 (typed
/// field, readable here), `body` = `+0x02..+0x98` (150 B).
impl Layout for Army {
    const CLASS: &'static str = "Army";
    fn complete(&self) -> bool {
        self.body.len() == 150
    }
    fn slot(&self, off: usize) -> Option<(&[u8], usize)> {
        (off >= 2).then(|| (&self.body[..], off - 2))
    }
    fn slot_mut(&mut self, off: usize) -> Option<(&mut [u8], usize)> {
        (off >= 2).then(|| (&mut self.body[..], off - 2))
    }
    fn extra_byte(&self, off: usize) -> Option<u8> {
        match off {
            0..=1 => Some(self.valid.to_le_bytes()[off]),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Img: the accessor wrapper
// ---------------------------------------------------------------------------

/// Byte-image view of a serialized record addressed by retail offset.
/// `Img(&rec)` reads; `Img(&mut rec)` reads and writes.
pub(crate) struct Img<P>(pub P);

impl<P: Deref> Img<P>
where
    P::Target: Layout,
{
    pub(crate) fn complete(&self) -> bool {
        self.0.complete()
    }

    /// `n` bytes at `off` if the whole scalar lies inside one serialized
    /// piece (or is assembled from typed-field bytes).
    pub(crate) fn try_bytes<const N: usize>(&self, off: usize) -> Option<[u8; N]> {
        if let Some((v, i)) = self.0.slot(off) {
            return v.get(i..i + N).map(|b| b.try_into().unwrap());
        }
        let mut out = [0u8; N];
        for (k, o) in out.iter_mut().enumerate() {
            *o = self.0.extra_byte(off + k)?;
        }
        Some(out)
    }

    fn bytes<const N: usize>(&self, off: usize) -> [u8; N] {
        self.try_bytes(off).unwrap_or_else(|| {
            panic!("{} image offset {off:#x} ({N} B) is not serialized", <P::Target as Layout>::CLASS)
        })
    }

    pub(crate) fn try_u8(&self, off: usize) -> Option<u8> {
        self.try_bytes::<1>(off).map(|b| b[0])
    }
    pub(crate) fn try_i16(&self, off: usize) -> Option<i16> {
        self.try_bytes(off).map(i16::from_le_bytes)
    }
    pub(crate) fn try_i32(&self, off: usize) -> Option<i32> {
        self.try_bytes(off).map(i32::from_le_bytes)
    }

    pub(crate) fn u8(&self, off: usize) -> u8 {
        self.bytes::<1>(off)[0]
    }
    pub(crate) fn i8(&self, off: usize) -> i8 {
        self.u8(off) as i8
    }
    pub(crate) fn i16(&self, off: usize) -> i16 {
        i16::from_le_bytes(self.bytes(off))
    }
    pub(crate) fn u16(&self, off: usize) -> u16 {
        self.i16(off) as u16
    }
    pub(crate) fn i32(&self, off: usize) -> i32 {
        i32::from_le_bytes(self.bytes(off))
    }
    pub(crate) fn u32(&self, off: usize) -> u32 {
        self.i32(off) as u32
    }
}

impl<P: DerefMut> Img<P>
where
    P::Target: Layout,
{
    fn write(&mut self, off: usize, bytes: &[u8]) {
        match self.0.slot_mut(off) {
            Some((v, i)) if i + bytes.len() <= v.len() => v[i..i + bytes.len()].copy_from_slice(bytes),
            Some(_) => panic!(
                "{} image offset {off:#x} ({} B) straddles a serialized piece",
                <P::Target as Layout>::CLASS,
                bytes.len()
            ),
            None => {
                for (k, &b) in bytes.iter().enumerate() {
                    if !self.0.set_extra_byte(off + k, b) {
                        panic!("{} image offset {:#x} is not writable by image offset", <P::Target as Layout>::CLASS, off + k);
                    }
                }
            }
        }
        self.0.after_write();
    }

    pub(crate) fn set_u8(&mut self, off: usize, x: u8) {
        self.write(off, &[x])
    }
    pub(crate) fn set_i8(&mut self, off: usize, x: i8) {
        self.set_u8(off, x as u8)
    }
    pub(crate) fn set_i16(&mut self, off: usize, x: i16) {
        self.write(off, &x.to_le_bytes())
    }
    pub(crate) fn set_u16(&mut self, off: usize, x: u16) {
        self.set_i16(off, x as i16)
    }
    pub(crate) fn set_i32(&mut self, off: usize, x: i32) {
        self.write(off, &x.to_le_bytes())
    }
    pub(crate) fn set_u32(&mut self, off: usize, x: u32) {
        self.set_i32(off, x as i32)
    }
}

/// `SubObject` / `ObjectData` prefix shared by Unit, Animal, Build.
impl<P: Deref> Img<P>
where
    P::Target: ObjLayout,
{
    /// `SubObject.flags` (+0x08): bit 1 live, bit 2 started
    /// (`WallData::is_started` 0x00472360), bit 4 active
    /// (`WallData::is_active` 0x00472350), bit 0x20 city.
    pub(crate) fn flags(&self) -> u8 {
        self.u8(0x08)
    }
    /// `SubObject.who` (+0x09).
    pub(crate) fn who(&self) -> u8 {
        self.u8(0x09)
    }
    /// `SubObject.o` (+0x0a, i16) — slot index in the owner list.
    pub(crate) fn o(&self) -> i32 {
        self.i16(0x0a) as i32
    }
    /// `SubObject.z` (+0x0c).
    pub(crate) fn z(&self) -> i32 {
        self.i32(0x0c)
    }
    /// `SubObject.x_internal` (+0x10) — stored XOR `0x63637`; every retail
    /// reader applies the mask.
    pub(crate) fn x(&self) -> i32 {
        self.i32(0x10) ^ COORD_XOR
    }
    /// `SubObject.y_internal` (+0x14), XOR `0x63637`.
    pub(crate) fn y(&self) -> i32 {
        self.i32(0x14) ^ COORD_XOR
    }
    /// `SubObject.ptype_index` (+0x18).
    pub(crate) fn ptype(&self) -> i32 {
        self.i32(0x18)
    }
    /// `ObjectData.myhits` (+0x20).
    pub(crate) fn myhits(&self) -> i32 {
        self.i32(0x20)
    }
    /// `ObjectData.uid` (+0x30, i16).
    pub(crate) fn uid(&self) -> i16 {
        self.i16(0x30)
    }
}

impl<P: DerefMut> Img<P>
where
    P::Target: ObjLayout,
{
    pub(crate) fn set_x(&mut self, x: i32) {
        self.set_i32(0x10, x ^ COORD_XOR)
    }
    pub(crate) fn set_y(&mut self, y: i32) {
        self.set_i32(0x14, y ^ COORD_XOR)
    }
}

/// `x_internal`/`y_internal` obfuscation mask.
pub(crate) const COORD_XOR: i32 = 0x63637;

impl<P: Deref<Target = Build>> Img<P> {
    /// `QueueItem` row `slot` (18 serialized bytes of the 0x14 runtime item:
    /// `job_counter` i32 at +0, `type` i16 at +4).
    pub(crate) fn queue_row(&self, slot: usize) -> Option<&[u8]> {
        self.0.queue.chunks_exact(18).nth(slot)
    }
    /// `BuildData.build_masks` (+0x60, u16).
    pub(crate) fn build_masks(&self) -> u16 {
        self.u16(0x60)
    }
}

impl<P: DerefMut<Target = Build>> Img<P> {
    pub(crate) fn set_build_masks(&mut self, m: u16) {
        self.set_u16(0x60, m)
    }
}

impl<P: Deref<Target = Unit>> Img<P> {
    pub(crate) fn guys_len(&self) -> usize {
        self.0.guys.elems.len()
    }
}

/// Mutable `GuyData` row `i` of a unit (155 B = `+0x08..+0xa3`;
/// `GuyData::walk_data` 0x005e0210), only when fully walked.
pub(crate) fn guy_row(u: &mut Unit, i: usize) -> Option<&mut Vec<u8>> {
    u.guys.elems.get_mut(i).and_then(|g| g.as_mut()).map(|g| &mut g.data).filter(|d| d.len() == GUY_ROW_LEN)
}

/// Serialized `GuyData` row length (`+0x08..+0xa3`).
pub(crate) const GUY_ROW_LEN: usize = 155;

/// Image base of a `GuyData` row: `row[i]` is `GuyData + 8 + i`.
pub(crate) const GUY_BASE: usize = 8;

/// Byte-image view of one serialized `GuyData` row addressed by retail
/// offset (`+0x08..+0xa3`).
pub(crate) struct GuyImg<P>(pub P);

impl<P: Deref<Target = [u8]>> GuyImg<P> {
    pub(crate) fn u8(&self, off: usize) -> u8 {
        self.0[off - GUY_BASE]
    }
    pub(crate) fn i8(&self, off: usize) -> i8 {
        self.u8(off) as i8
    }
    pub(crate) fn i16(&self, off: usize) -> i16 {
        get_i16(&self.0, off - GUY_BASE)
    }
    pub(crate) fn u16(&self, off: usize) -> u16 {
        self.i16(off) as u16
    }
    pub(crate) fn i32(&self, off: usize) -> i32 {
        get_i32(&self.0, off - GUY_BASE)
    }
    pub(crate) fn u32(&self, off: usize) -> u32 {
        self.i32(off) as u32
    }
}

impl<P: DerefMut<Target = [u8]>> GuyImg<P> {
    pub(crate) fn set_u8(&mut self, off: usize, x: u8) {
        self.0[off - GUY_BASE] = x;
    }
    pub(crate) fn set_u16(&mut self, off: usize, x: u16) {
        put_u16(&mut self.0, off - GUY_BASE, x)
    }
    pub(crate) fn set_i32(&mut self, off: usize, x: i32) {
        put_i32(&mut self.0, off - GUY_BASE, x)
    }
    pub(crate) fn set_u32(&mut self, off: usize, x: u32) {
        self.set_i32(off, x as i32)
    }
}

// ---------------------------------------------------------------------------
// Objects: look up a slot as an image, whatever its concrete kind
// ---------------------------------------------------------------------------

/// `ObjectData.uid` (+0x30) of `Objects.lists[who][o]`, any kind.
pub(crate) fn object_uid(save: &Save, who: i32, o: i32) -> Option<i16> {
    let l = save.objects.lists.get(usize::try_from(who).ok()?)?;
    match l.elems.get(usize::try_from(o).ok()?)? {
        Some(Obj::Unit(u)) => Img(&**u).try_i16(0x30),
        Some(Obj::Animal(a)) => Img(&**a).try_i16(0x30),
        Some(Obj::Build(b)) => Img(&**b).try_i16(0x30),
        None => None,
    }
}

// ---------------------------------------------------------------------------
// Leaders
// ---------------------------------------------------------------------------

/// Image base of `Leader.body` (`LeaderData` +0x08).
pub(crate) const LEADER_BODY_BASE: usize = 0x08;

/// `LeaderData.flags` (+0x00) of slot `who`; `0` for a missing slot.
/// Bit 1 = slot in play (the `process_all` traversal gate).
pub(crate) fn leader_flags(save: &Save, who: usize) -> i32 {
    save.leaders.slots.get(who).map(|l| l.flags).unwrap_or(0)
}

/// `LeaderData.flags2` (+0x04) of slot `who`; `0` for a missing slot.
pub(crate) fn leader_flags2(save: &Save, who: usize) -> i32 {
    save.leaders.slots.get(who).map(|l| l.flags2).unwrap_or(0)
}

/// `LeaderData` byte at image offset `off` (`flags` +0, `flags2` +4, body
/// +0x08..+0x692a). `None` when the slot has no walked body.
pub(crate) fn leader_u8(save: &Save, who: usize, off: usize) -> Option<u8> {
    Img(save.leaders.slots.get(who)?).try_u8(off)
}

pub(crate) fn leader_i16(save: &Save, who: usize, off: usize) -> Option<i16> {
    Img(save.leaders.slots.get(who)?).try_i16(off)
}

pub(crate) fn leader_i32(save: &Save, who: usize, off: usize) -> Option<i32> {
    Img(save.leaders.slots.get(who)?).try_i32(off)
}

/// Mutable `LeaderData` image for slot `who` (only when its body is walked).
pub(crate) fn leader_mut(save: &mut Save, who: usize) -> Option<Img<&mut Leader>> {
    save.leaders.slots.get_mut(who).filter(|l| l.complete()).map(Img)
}

/// `Leader.data_encrypted[idx]` (62 dwords walked last by
/// `LeaderData::walk_data`).
pub(crate) fn leader_enc_i32(l: &Leader, idx: usize) -> Option<i32> {
    try_i32(&l.data_encrypted, idx * 4)
}

pub(crate) fn leader_enc_put_i32(l: &mut Leader, idx: usize, v: i32) {
    put_i32(&mut l.data_encrypted, idx * 4, v)
}

// ---------------------------------------------------------------------------
// Rules: Constants and type records
// ---------------------------------------------------------------------------

/// `Constants` i32 at image offset `off` (`[0x00c061f0]` / the const alias
/// `[0x00c061e4]`, 0xd40 B). Prefers the typed `Rules.constants` image and
/// falls back to the earlier direct `Save.constants` walk of the same
/// object. `None` if neither holds the offset.
pub(crate) fn try_constant(save: &Save, off: usize) -> Option<i32> {
    try_i32(&save.rules_tail.rules.constants, off).or_else(|| try_i32(&save.constants, off))
}

/// [`try_constant`] that panics on a missing offset — a `Constants` read
/// outside both walked images is a loader bug, not a game state.
pub(crate) fn constant(save: &Save, off: usize) -> i32 {
    try_constant(save, off).unwrap_or_else(|| panic!("Constants offset {off:#x} is not serialized"))
}

/// Type record image reader over `Rules.types[idx]` (`Type::walk_rules_data`
/// 0x00663190 + `ObjectType` 0x0065fba0 + per-kind tails). Image ranges:
///
/// ```text
///   head      image[0x04..0x5e)   every kind                    (90 B)
///   obj_mid   image[0x1e4..0x27c) Good/Unit/Build/Object        (152 B)
///   arr0/arr1 is_list (+0x280) / is_strict_list (+0x29c)        SimpleArray<u16>
///   ext       Unit:  [0x2b4..0x2cc) ++ [0x2d4..0x2dc) ++ [0x2dc..0x2e0) ++ [0x2e0..0x5d4)
///                    (792 B; the 8 B at 0x2cc..0x2d4 are NOT emitted)
///             Build: [0x2b4..0x2e5) (49 B)
///             Good:  [0x2b4..0x2f8) (68 B)
///             Tech:  [0x1c8..0x1e3) (27 B)   Spell: [0x1c8..0x1f8) (48 B)
///             Object (slot 543) / Type: none
/// ```
#[derive(Clone, Copy)]
pub(crate) struct TypeImg<'a> {
    pub rec: &'a TypeRec,
    pub kind: TypeRuleKind,
}

impl<'a> TypeImg<'a> {
    pub(crate) fn new(rec: &'a TypeRec, slot: usize) -> Self {
        TypeImg { rec, kind: TypeRuleKind::for_slot(slot) }
    }

    /// Index into `rec.ext` for image offset `off`, per kind.
    fn ext_index(&self, off: usize) -> Option<usize> {
        Some(match (self.kind, off) {
            (TypeRuleKind::Unit, 0x2b4..=0x2cb) => off - 0x2b4,
            (TypeRuleKind::Unit, 0x2d4..=0x2db) => 24 + (off - 0x2d4),
            (TypeRuleKind::Unit, 0x2dc..=0x2df) => 32 + (off - 0x2dc),
            (TypeRuleKind::Unit, 0x2e0..=0x5d3) => 36 + (off - 0x2e0),
            (TypeRuleKind::Build, 0x2b4..=0x2e4) => off - 0x2b4,
            (TypeRuleKind::Good, 0x2b4..=0x2f7) => off - 0x2b4,
            (TypeRuleKind::Tech, 0x1c8..=0x1e2) => off - 0x1c8,
            (TypeRuleKind::Spell, 0x1c8..=0x1f7) => off - 0x1c8,
            _ => return None,
        })
    }

    /// `(piece, index)` for image offset `off`; `None` for the unit-tail gap
    /// and anything not serialized for this kind.
    fn slot(&self, off: usize) -> Option<(&'a [u8], usize)> {
        match off {
            0x04..=0x5d => Some((&self.rec.head, off - 4)),
            0x1e4..=0x27b if matches!(
                self.kind,
                TypeRuleKind::Good | TypeRuleKind::Unit | TypeRuleKind::Build | TypeRuleKind::Object
            ) =>
            {
                Some((&self.rec.obj_mid, off - 0x1e4))
            }
            _ => Some((&self.rec.ext, self.ext_index(off)?)),
        }
    }

    /// `N` bytes at `off`, only when every byte maps and they are
    /// contiguous in the serialized piece (so no read spans the 0x2cc gap).
    pub(crate) fn bytes<const N: usize>(&self, off: usize) -> Option<[u8; N]> {
        let (v, i) = self.slot(off)?;
        let (v2, j) = self.slot(off + N - 1)?;
        if !std::ptr::eq(v, v2) || j != i + N - 1 {
            return None;
        }
        v.get(i..i + N).map(|b| b.try_into().unwrap())
    }

    pub(crate) fn u8(&self, off: usize) -> Option<u8> {
        self.bytes::<1>(off).map(|b| b[0])
    }
    pub(crate) fn i16(&self, off: usize) -> Option<i16> {
        self.bytes(off).map(i16::from_le_bytes)
    }
    pub(crate) fn i32(&self, off: usize) -> Option<i32> {
        self.bytes(off).map(i32::from_le_bytes)
    }
    pub(crate) fn u32(&self, off: usize) -> Option<u32> {
        self.i32(off).map(|v| v as u32)
    }

    /// `is_list` (+0x280/+0x28c) and `is_strict_list` (+0x29c/+0x2a8): the
    /// two `SimpleArray<u16>` after the object body.
    pub(crate) fn list(&self, strict: bool) -> impl Iterator<Item = u16> + 'a {
        let a = if strict { &self.rec.arr1 } else { &self.rec.arr0 };
        a.data.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]]))
    }
}

/// `Rules.types[idx]` as a kind-aware [`TypeImg`]; `None` for a negative or
/// out-of-range index.
pub(crate) fn type_rec(save: &Save, idx: i32) -> Option<TypeImg<'_>> {
    let i = usize::try_from(idx).ok()?;
    save.rules_tail.rules.types.get(i).map(|rec| TypeImg::new(rec, i))
}

// ---------------------------------------------------------------------------
// Game / Objects scalars shared by the traversals
// ---------------------------------------------------------------------------

/// `Game::frame` (`Game+0x550`, first dword of `Game.scalars`).
pub(crate) fn frame(save: &Save) -> i32 {
    get_i32(&save.game.scalars, 0)
}

/// `Objects` scalar block: `[0x1f4..0x1fc) [0x154..0x15c) unit_mark[9]
/// build_mark[9] wall_mark[9] obj_ctr[9]` (`Objects::walk_data`
/// 0x006541e0). Owners `>= 9` read as `0`.
pub(crate) fn unit_mark(save: &Save, owner: usize) -> i32 {
    objects_mark(save, 0, owner)
}

pub(crate) fn build_mark(save: &Save, owner: usize) -> i32 {
    objects_mark(save, 36, owner)
}

pub(crate) fn wall_mark(save: &Save, owner: usize) -> i32 {
    objects_mark(save, 72, owner)
}

fn objects_mark(save: &Save, block: usize, owner: usize) -> i32 {
    if owner >= 9 {
        return 0;
    }
    get_i32(&save.objects.scalars, 16 + block + owner * 4)
}

/// `GameAccess::game_random` seed inside the post-World direct block.
pub(crate) const GAME_RANDOM: usize = 0x28;

/// `Random::get(min, max)` 0x00a39d70 on `GameAccess::game_random`:
/// `seed = seed*0x19660d + 0x3c6ef35f; ((seed & 0xffff) * (max-min) >> 16) + min`.
/// Consumes one LCG step unless `min == max`.
pub(crate) fn game_random(save: &mut Save, min: i32, max: i32) -> i32 {
    let (lo, hi) = if max < min { (max, min) } else { (min, max) };
    if lo == hi {
        return min;
    }
    let seed = get_u32(&save.post_world, GAME_RANDOM);
    let seed = crate::tick::rng_step(seed);
    put_u32(&mut save.post_world, GAME_RANDOM, seed);
    (((seed & 0xffff) as i32).wrapping_mul(hi - lo) as u32 >> 16) as i32 + lo
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sections::{ObjBase, SubObj};

    /// Byte `k` of a piece whose image base is `base` is `(base + k) & 0xff`,
    /// so any correctly mapped read of offset `o` returns `o & 0xff`.
    fn stamped(base: usize, len: usize) -> Vec<u8> {
        (0..len).map(|k| ((base + k) & 0xff) as u8).collect()
    }

    fn obj_base() -> ObjBase {
        ObjBase {
            sub: SubObj { tag: 0, flags: 0x05, gate: 1, body: stamped(0x09, 19) },
            tag: 0,
            gate: 1,
            mid: stamped(0x20, 34),
            launch: 0,
            launching: Default::default(),
        }
    }

    fn build() -> Build {
        Build {
            head: vec![0, 0],
            base: obj_base(),
            wall_body: stamped(0x48, 30),
            body: stamped(0x70, 22),
            queue: (0..36u8).collect(),
            ..Default::default()
        }
    }

    fn unit() -> Unit {
        Unit { base: obj_base(), body: stamped(0x48, 111), ..Default::default() }
    }

    fn le32(off: usize) -> i32 {
        i32::from_le_bytes([off as u8, (off + 1) as u8, (off + 2) as u8, (off + 3) as u8])
    }

    #[test]
    fn build_offsets_land_in_the_right_piece() {
        let b = build();
        let v = Img(&b);
        assert!(v.complete());
        assert_eq!(v.flags(), 0x05);
        assert_eq!(v.who(), 0x09);
        assert_eq!(v.u8(0x1b), 0x1b); // last SubObject byte
        assert_eq!(v.u8(0x20), 0x20); // first ObjectData byte
        assert_eq!(v.u8(0x41), 0x41);
        assert_eq!(v.u8(0x48), 0x48); // WallData
        assert_eq!(v.u8(0x65), 0x65);
        assert_eq!(v.u8(0x70), 0x70); // BuildData
        assert_eq!(v.u8(0x85), 0x85);
        assert_eq!(v.i32(0x18), le32(0x18));
        assert_eq!(v.ptype(), le32(0x18));
        assert_eq!(v.x(), le32(0x10) ^ COORD_XOR);
        assert_eq!(v.i16(0x30), i16::from_le_bytes([0x30, 0x31]));
        assert_eq!(v.uid(), i16::from_le_bytes([0x30, 0x31]));
        assert_eq!(v.build_masks(), u16::from_le_bytes([0x60, 0x61]));
        assert_eq!(v.queue_row(1).map(|r| r[0]), Some(18));
        // Gaps are not serialized.
        assert_eq!(v.try_u8(0x1c), None);
        assert_eq!(v.try_u8(0x42), None);
        assert_eq!(v.try_u8(0x66), None);
        assert_eq!(v.try_u8(0x6c), None);
        assert_eq!(v.try_u8(0x86), None);
        // A read that would straddle wall_body's end is refused.
        assert_eq!(v.try_i32(0x64), None);
    }

    #[test]
    fn build_writes_mirror_founder_and_max_age_into_head() {
        let mut b = build();
        let mut m = Img(&mut b);
        m.set_u8(0x7f, 0xaa);
        m.set_i32(0x80, i32::from_le_bytes([1, 2, 3, 0xbb]));
        m.set_u8(0x08, 0x25);
        m.set_build_masks(0xbeef);
        assert_eq!(m.u8(0x7f), 0xaa);
        assert_eq!(m.u8(0x83), 0xbb);
        assert_eq!(m.flags(), 0x25);
        assert_eq!(m.build_masks(), 0xbeef);
        assert_eq!(b.head, vec![0xaa, 0xbb]);
        assert_eq!(b.base.sub.flags, 0x25);
        assert_eq!(b.body[0x7f - 0x70], 0xaa);
    }

    #[test]
    #[should_panic(expected = "0x6c")]
    fn build_unserialized_offset_panics() {
        let b = build();
        Img(&b).i32(0x6c);
    }

    #[test]
    fn unit_and_animal_offsets() {
        let mut u = unit();
        {
            let v = Img(&u);
            assert!(v.complete());
            assert_eq!(v.u8(0x48), 0x48);
            assert_eq!(v.u8(0xb6), 0xb6);
            assert_eq!(v.try_u8(0xb7), None);
            assert_eq!(v.i32(0x50), le32(0x50));
            assert_eq!(v.o(), i16::from_le_bytes([0x0a, 0x0b]) as i32);
        }
        Img(&mut u).set_x(1234);
        assert_eq!(Img(&u).x(), 1234);
        assert_eq!(Img(&u).i32(0x10), 1234 ^ COORD_XOR);

        let mut a = Animal { unit: u, tail: stamped(0x150, 5), ..Default::default() };
        let v = Img(&a);
        assert!(v.complete());
        assert_eq!(v.u8(0x48), 0x48);
        assert_eq!(v.i16(0x150), i16::from_le_bytes([0x50, 0x51])); // ox
        assert_eq!(v.i16(0x152), i16::from_le_bytes([0x52, 0x53])); // whom
        assert_eq!(v.i8(0x154), 0x54); // aid
        assert_eq!(v.try_u8(0x155), None);
        assert_eq!(v.try_u8(0x14f), None);
        Img(&mut a).set_i16(0x152, -7);
        assert_eq!(Img(&a).i16(0x152), -7);
        assert_eq!(a.tail[2..4], (-7i16).to_le_bytes());
    }

    #[test]
    fn leader_city_army_group_bases() {
        let l = Leader { flags: 0x0403_0201, flags2: 0x0807_0605, body: stamped(0x08, 0x6922), ..Default::default() };
        let v = Img(&l);
        assert!(v.complete());
        assert_eq!(v.u8(0x00), 0x01);
        assert_eq!(v.u8(0x03), 0x04);
        assert_eq!(v.i32(0x00), 0x0403_0201);
        assert_eq!(v.i32(0x04), 0x0807_0605);
        assert_eq!(v.u8(0x08), 0x08);
        assert_eq!(v.i32(0x3f8), le32(0x3f8)); // LeaderData.city_num
        assert_eq!(v.u8(0x6929), 0x29);
        assert_eq!(v.try_u8(0x692a), None);

        let c = City { flags: 0x0201, pod: stamped(0x06, 108), ..Default::default() };
        let v = Img(&c);
        assert!(v.complete());
        assert_eq!(v.u8(0x04), 0x01);
        assert_eq!(v.u8(0x06), 0x06);
        assert_eq!(v.i8(0x5e), 0x5e); // CityData.who
        assert_eq!(v.u8(0x71), 0x71);
        assert_eq!(v.try_u8(0x72), None);
        assert_eq!(v.try_u8(0x03), None);

        let a = Army { valid: 0x0201, body: stamped(0x02, 150), ..Default::default() };
        let v = Img(&a);
        assert!(v.complete());
        assert_eq!(v.i16(0x00), 0x0201);
        assert_eq!(v.i16(0x02), i16::from_le_bytes([0x02, 0x03])); // Army.army
        assert_eq!(v.i32(0x04), le32(0x04)); // Army.status
        assert_eq!(v.i16(0x96), i16::from_le_bytes([0x96, 0x97])); // num_groups
        assert_eq!(v.try_u8(0x98), None);

        let g = Group { hdr: stamped(0, 72), ..Default::default() };
        let v = Img(&g);
        assert!(v.complete());
        assert_eq!(v.i32(8), le32(8)); // Group.num
        assert_eq!(v.u8(71), 71);
        assert_eq!(v.try_u8(72), None);
    }

    #[test]
    fn leader_helpers_over_save() {
        let mut save = Save::default();
        save.leaders.slots = vec![Leader { flags: 1, flags2: 9, body: stamped(0x08, 0x6922), ..Default::default() }, Leader::default()];
        assert_eq!(leader_flags(&save, 0), 1);
        assert_eq!(leader_flags2(&save, 0), 9);
        assert_eq!(leader_flags(&save, 7), 0);
        assert_eq!(leader_u8(&save, 0, 0x00), Some(1));
        assert_eq!(leader_i16(&save, 0, 0x10), Some(i16::from_le_bytes([0x10, 0x11])));
        assert_eq!(leader_i32(&save, 0, 0x3f8), Some(le32(0x3f8)));
        // Slot 1 has flags & 1 == 0: no walked body.
        assert_eq!(leader_i32(&save, 1, 0x3f8), None);
        assert_eq!(leader_u8(&save, 1, 0x00), Some(0));
        assert!(leader_mut(&mut save, 1).is_none());
        leader_mut(&mut save, 0).unwrap().set_i32(0x3f8, 42);
        assert_eq!(leader_i32(&save, 0, 0x3f8), Some(42));
        assert_eq!(save.leaders.slots[0].body[0x3f0..0x3f4], 42i32.to_le_bytes());

        let l = &mut save.leaders.slots[0];
        l.data_encrypted = vec![0; 62 * 4];
        leader_enc_put_i32(l, 3, -5);
        assert_eq!(leader_enc_i32(l, 3), Some(-5));
        assert_eq!(leader_enc_i32(l, 62), None);
    }

    #[test]
    fn constant_prefers_rules_image_then_direct_walk() {
        let mut save = Save::default();
        assert_eq!(try_constant(&save, 0x08), None);
        save.constants = stamped(0, 0xd40);
        assert_eq!(constant(&save, 0x08), le32(0x08));
        assert_eq!(constant(&save, 0xcd8), le32(0xcd8));
        save.rules_tail.rules.constants = vec![0x11; 0xd40];
        assert_eq!(constant(&save, 0x08), 0x1111_1111);
        assert_eq!(try_constant(&save, 0xd40), None);
    }

    #[test]
    #[should_panic(expected = "0xd40")]
    fn constant_out_of_range_panics() {
        let save = Save::default();
        constant(&save, 0xd40);
    }

    fn type_rec_for(kind: TypeRuleKind) -> TypeRec {
        let mut rec = TypeRec { head: stamped(4, 90), ..Default::default() };
        match kind {
            TypeRuleKind::Unit => {
                rec.obj_mid = stamped(0x1e4, 152);
                // Piecewise: the serialized ext is the image minus 0x2cc..0x2d4.
                let mut ext = stamped(0x2b4, 24);
                ext.extend(stamped(0x2d4, 8));
                ext.extend(stamped(0x2dc, 4));
                ext.extend(stamped(0x2e0, 756));
                rec.ext = ext;
            }
            TypeRuleKind::Build => {
                rec.obj_mid = stamped(0x1e4, 152);
                rec.ext = stamped(0x2b4, 49);
            }
            TypeRuleKind::Good => {
                rec.obj_mid = stamped(0x1e4, 152);
                rec.ext = stamped(0x2b4, 68);
            }
            TypeRuleKind::Object => rec.obj_mid = stamped(0x1e4, 152),
            TypeRuleKind::Tech => rec.ext = stamped(0x1c8, 27),
            TypeRuleKind::Spell => rec.ext = stamped(0x1c8, 48),
            TypeRuleKind::Type => {}
        }
        rec
    }

    #[test]
    fn type_img_unit_tail_skips_the_0x2cc_gap() {
        let rec = type_rec_for(TypeRuleKind::Unit);
        let t = TypeImg { rec: &rec, kind: TypeRuleKind::Unit };
        assert_eq!(rec.ext.len(), 792);
        assert_eq!(t.i32(0x04), Some(le32(0x04))); // type_index
        assert_eq!(t.i32(0x3c), Some(le32(0x3c))); // from
        assert_eq!(t.i32(0x1e4), Some(le32(0x1e4))); // obj_masks
        assert_eq!(t.i32(0x218), Some(le32(0x218))); // kind
        assert_eq!(t.i32(0x25c), Some(le32(0x25c))); // graft
        assert_eq!(t.i32(0x2b4), Some(le32(0x2b4))); // flags
        assert_eq!(t.i32(0x2b8), Some(le32(0x2b8))); // flags2
        assert_eq!(t.i32(0x2c4), Some(le32(0x2c4))); // turn
        assert_eq!(t.i32(0x2c8), Some(le32(0x2c8))); // last dword before the gap
        assert_eq!(t.i32(0x2cc), None); // the gap
        assert_eq!(t.i32(0x2d0), None);
        assert_eq!(t.i32(0x2ca), None); // straddles the gap
        assert_eq!(t.i32(0x2d4), Some(le32(0x2d4)));
        assert_eq!(t.i32(0x2dc), Some(le32(0x2dc)));
        assert_eq!(t.i32(0x2e0), Some(le32(0x2e0)));
        assert_eq!(t.i32(0x304), Some(le32(0x304))); // num_guys
        assert_eq!(t.i32(0x5d0), Some(le32(0x5d0)));
        assert_eq!(t.i32(0x5d4), None);
        // Nothing between head and object body, or before head.
        assert_eq!(t.i32(0x5e), None);
        assert_eq!(t.i32(0x00), None);
        assert_eq!(t.i32(0x27c), None);
    }

    #[test]
    fn type_img_other_kinds() {
        let rec = type_rec_for(TypeRuleKind::Build);
        let t = TypeImg { rec: &rec, kind: TypeRuleKind::Build };
        assert_eq!(t.i32(0x2b4), Some(le32(0x2b4)));
        assert_eq!(t.u8(0x2e4), Some(0xe4));
        assert_eq!(t.i32(0x2e4), None);
        assert_eq!(t.i32(0x2d4), Some(le32(0x2d4))); // contiguous for Build
        assert_eq!(t.i32(0x25c), Some(le32(0x25c)));

        let rec = type_rec_for(TypeRuleKind::Good);
        let t = TypeImg { rec: &rec, kind: TypeRuleKind::Good };
        assert_eq!(t.i32(0x2f4), Some(le32(0x2f4)));
        assert_eq!(t.i32(0x2f8), None);

        let rec = type_rec_for(TypeRuleKind::Tech);
        let t = TypeImg { rec: &rec, kind: TypeRuleKind::Tech };
        assert_eq!(t.i32(0x1c8), Some(le32(0x1c8))); // TechType::age
        assert_eq!(t.u8(0x1e2), Some(0xe2));
        assert_eq!(t.i32(0x1e4), None); // tech has no object body
        assert_eq!(t.i32(0x2b4), None);

        let rec = type_rec_for(TypeRuleKind::Spell);
        let t = TypeImg { rec: &rec, kind: TypeRuleKind::Spell };
        assert_eq!(t.i32(0x1f4), Some(le32(0x1f4)));
        assert_eq!(t.i32(0x1f8), None);

        let rec = type_rec_for(TypeRuleKind::Object);
        let t = TypeImg { rec: &rec, kind: TypeRuleKind::Object };
        assert_eq!(t.i32(0x1e4), Some(le32(0x1e4)));
        assert_eq!(t.i32(0x2b4), None);

        let rec = type_rec_for(TypeRuleKind::Type);
        let t = TypeImg { rec: &rec, kind: TypeRuleKind::Type };
        assert_eq!(t.i32(0x04), Some(le32(0x04)));
        assert_eq!(t.i32(0x1e4), None);
    }

    #[test]
    fn type_rec_picks_kind_from_slot() {
        let mut save = Save::default();
        save.rules_tail.rules.types.resize_with(806, TypeRec::default);
        save.rules_tail.rules.types[0x32] = type_rec_for(TypeRuleKind::Unit);
        save.rules_tail.rules.types[0x19e] = type_rec_for(TypeRuleKind::Build);
        save.rules_tail.rules.types[544] = type_rec_for(TypeRuleKind::Tech);
        let mut strict = type_rec_for(TypeRuleKind::Unit);
        strict.arr1.data = vec![0x34, 0x12, 0x78, 0x56];
        save.rules_tail.rules.types[0x33] = strict;

        assert!(matches!(type_rec(&save, 0x32).unwrap().kind, TypeRuleKind::Unit));
        assert_eq!(type_rec(&save, 0x32).unwrap().i32(0x304), Some(le32(0x304)));
        assert!(matches!(type_rec(&save, 0x19e).unwrap().kind, TypeRuleKind::Build));
        assert_eq!(type_rec(&save, 0x19e).unwrap().i32(0x2d4), Some(le32(0x2d4)));
        assert_eq!(type_rec(&save, 544).unwrap().i32(0x1c8), Some(le32(0x1c8)));
        assert_eq!(type_rec(&save, 0x33).unwrap().list(true).collect::<Vec<_>>(), vec![0x1234, 0x5678]);
        assert_eq!(type_rec(&save, 0x33).unwrap().list(false).count(), 0);
        assert!(type_rec(&save, -1).is_none());
        assert!(type_rec(&save, 806).is_none());
    }

    #[test]
    fn marks_frame_and_random_match_module_copies() {
        let mut save = Save::default();
        save.game.scalars = stamped(0, 0x6e4 - 0x550);
        assert_eq!(frame(&save), le32(0));
        save.objects.scalars = stamped(0, 16 + 36 * 3 + 18);
        assert_eq!(unit_mark(&save, 0), le32(16));
        assert_eq!(unit_mark(&save, 8), le32(16 + 32));
        assert_eq!(unit_mark(&save, 9), 0);
        assert_eq!(build_mark(&save, 2), le32(16 + 36 + 8));
        assert_eq!(wall_mark(&save, 2), le32(16 + 72 + 8));

        save.post_world = vec![0; 44];
        put_u32(&mut save.post_world, GAME_RANDOM, 12345);
        assert_eq!(game_random(&mut save, 7, 7), 7);
        assert_eq!(get_u32(&save.post_world, GAME_RANDOM), 12345);
        let seed = crate::tick::rng_step(12345);
        let want = (((seed & 0xffff) as i32).wrapping_mul(0xffff) as u32 >> 16) as i32;
        assert_eq!(game_random(&mut save, 0, 0xffff), want);
        assert_eq!(get_u32(&save.post_world, GAME_RANDOM), seed);
    }

    #[test]
    fn object_uid_any_kind_and_guy_rows() {
        let mut save = Save::default();
        save.objects.lists.resize_with(1, Default::default);
        let mut u = unit();
        put_i16(&mut u.base.mid, 0x10, -321);
        let mut b = build();
        put_i16(&mut b.base.mid, 0x10, 77);
        save.objects.lists[0].elems = vec![Some(Obj::Unit(Box::new(u))), None, Some(Obj::Build(Box::new(b)))];
        assert_eq!(object_uid(&save, 0, 0), Some(-321));
        assert_eq!(object_uid(&save, 0, 1), None);
        assert_eq!(object_uid(&save, 0, 2), Some(77));
        assert_eq!(object_uid(&save, 1, 0), None);
        assert_eq!(object_uid(&save, -1, 0), None);

        let mut u = unit();
        u.guys.elems = vec![Some(crate::prim::Row { data: stamped(GUY_BASE, GUY_ROW_LEN) }), None, Some(crate::prim::Row { data: vec![0; 3] })];
        assert_eq!(Img(&u).guys_len(), 3);
        assert!(guy_row(&mut u, 1).is_none());
        assert!(guy_row(&mut u, 2).is_none(), "short row is not a walked GuyData");
        let row = guy_row(&mut u, 0).unwrap();
        let mut g = GuyImg(&mut row[..]);
        assert_eq!(g.u8(0x08), 0x08);
        assert_eq!(g.i32(0x10), le32(0x10));
        assert_eq!(g.u8(0xa2), 0xa2);
        g.set_u16(0x20, 0xabcd);
        assert_eq!(g.u16(0x20), 0xabcd);
    }

    #[test]
    fn scalar_helpers() {
        let mut buf = vec![0u8; 8];
        put_i32(&mut buf, 0, -2);
        put_i16(&mut buf, 4, -3);
        put_u8(&mut buf, 6, 0xfe);
        assert_eq!(get_i32(&buf, 0), -2);
        assert_eq!(get_u32(&buf, 0), 0xffff_fffe);
        assert_eq!(get_i16(&buf, 4), -3);
        assert_eq!(get_u16(&buf, 4), 0xfffd);
        assert_eq!(get_u8(&buf, 6), 0xfe);
        assert_eq!(try_i32(&buf, 5), None);
        assert_eq!(try_i16(&buf, 7), None);
        assert_eq!(try_i16(&buf, 6), Some(0x00fe));
    }
}
