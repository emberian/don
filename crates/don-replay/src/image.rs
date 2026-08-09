//! Column rows → **engine-layout byte images**.
//!
//! # Why this exists
//!
//! `don-sim` stores state as planes of columns, because that is what a
//! batch-parallel simulation wants. The engine stores it as a C++ object, and
//! the checksum walks *that* object's bytes at *those* offsets. Nothing could
//! compare the two until something wrote one into the other, which is why
//! `SimBridge::populate` was a no-op and why every passing channel on the replay
//! scoreboard passed by walking zero bytes.
//!
//! This module is that conversion, and it is deliberately **table-driven**: it
//! reads [`don_sim::generated::state::FieldDesc`] — offset, size, element count,
//! width pool, plane — which is generated from the shipped PDB's own type
//! stream. There is no hand-written field list here to drift from the binary. A
//! field the columns do not materialise (a container, a pointer, a `String`) is
//! *counted*, not guessed, so an image always knows how much of itself is real.
//!
//! # The one substitution, named
//!
//! `ObjectData`'s three coordinates are stored **masked**: the engine keeps
//! `value ^ 0x00063637` in `z_internal`/`x_internal`/`y_internal` and unmasks on
//! read (`docs/derivation/*`, `crates/don-sim/src/systems/items.rs:101`,
//! `ammo.rs:88`; `GuyData`'s coordinates are *not* masked). `don-sim`'s columns
//! hold the **unmasked** value — `World::spawn` writes a plain `Coord` and
//! `World::pos_x` hands it straight to the movement kernels — so imaging applies
//! the mask on the way out. Neither convention is wrong, but they are different,
//! and the difference has to happen in exactly one visible place.
//!
//! It does not currently affect any checksum: `Unit::walk_data` walks
//! `[0x48, 0xb7)` and `Object::walk_data` walks `[0x20, 0x42)`, and the
//! coordinates at `+12/+16/+20` are in neither. It affects every *other* use of
//! an engine-layout record, so it is done properly rather than deferred.

use don_sim::generated::state::{ClassDesc, FieldDesc, Pool, Repr};

/// `Object` coordinate mask. Same constant as
/// `crates/don-sim/src/systems/items.rs`'s `COORD_XOR`; repeated here rather
/// than imported only because that module is a sibling lane's file.
pub const COORD_XOR: i32 = 0x0006_3637;

/// Fields whose stored form is `value ^ COORD_XOR`.
const MASKED_COORDS: [&str; 3] = ["z_internal", "x_internal", "y_internal"];

/// A `don-sim` column block that can be read plane by plane.
///
/// Implemented for the generated `*Cols` structs. The generator emits the same
/// four accessors for every class (omitting a pool with zero planes), so this
/// trait is the small amount of glue that lets one imaging routine serve all of
/// them.
pub trait PdbColumns {
    /// The generated description of the class these columns hold.
    fn desc() -> &'static ClassDesc;
    /// Live rows.
    fn rows(&self) -> usize;
    fn w4(&self, plane: usize) -> &[i32];
    fn w2(&self, plane: usize) -> &[i16];
    fn w1(&self, plane: usize) -> &[i8];
    fn wf(&self, _plane: usize) -> &[f32] {
        &[]
    }
}

impl PdbColumns for don_sim::generated::state::UnitCols {
    fn desc() -> &'static ClassDesc {
        &don_sim::generated::state::unit::DESC
    }
    fn rows(&self) -> usize {
        self.len()
    }
    fn w4(&self, plane: usize) -> &[i32] {
        self.w4_plane(plane)
    }
    fn w2(&self, plane: usize) -> &[i16] {
        self.w2_plane(plane)
    }
    fn w1(&self, plane: usize) -> &[i8] {
        self.w1_plane(plane)
    }
}

/// What an image is made of: how much came from a column and how much is zero
/// because nothing models it yet.
///
/// This is the honesty half of the bridge. `bytes` alone would let an image of
/// 344 zeroes look exactly like an image of 344 real bytes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ImageCoverage {
    /// `sizeof` of the class.
    pub bytes: u32,
    /// Bytes written from a materialised column.
    pub sourced: u32,
    /// Bytes of fields the columns do not materialise (aggregates, deferred
    /// arrays, bitfields). Left zero in the image.
    pub unsourced: u32,
    /// Bytes inside no declared field at all: compiler padding, and any hole the
    /// PDB field list does not cover. Also left zero.
    pub unaccounted: u32,
    /// Distinct bytes of the image the checksum visits, from the *generated
    /// traversal* — not from the per-field `walked` flag, which is computed per
    /// class and misses base-class walks.
    pub walked: u32,
    /// Of `walked`, the bytes a materialised column writes.
    pub sourced_walked: u32,
    /// Of `walked`, the bytes nothing writes. **This is the number that bounds
    /// fidelity on the channel**: every one is a byte retail hashes and we hash a
    /// zero for.
    pub unsourced_walked: u32,
    pub fields_sourced: u32,
    pub fields_unsourced: u32,
}

impl ImageCoverage {
    /// Walked bytes we can source, as a fraction of walked bytes. 1.0 does
    /// **not** mean the values are right — only that something writes them.
    pub fn walked_sourced_ratio(&self) -> f64 {
        if self.walked == 0 {
            0.0
        } else {
            self.sourced_walked as f64 / self.walked as f64
        }
    }
}

/// Static coverage of a class's imaging, independent of any row.
///
/// `mask` is the traversal's own answer to which bytes matter
/// ([`crate::walk::WalkedMask`]); pass `None` when the class has no derived
/// walker and the walked columns will read zero.
pub fn coverage_of(desc: &ClassDesc, mask: Option<&crate::walk::WalkedMask>) -> ImageCoverage {
    let mut c = ImageCoverage {
        bytes: desc.sizeof,
        ..Default::default()
    };
    let mut sourced_bytes = vec![false; desc.sizeof as usize];
    let mut declared = vec![false; desc.sizeof as usize];
    for f in desc.fields {
        let (a, b) = (
            f.offset as usize,
            ((f.offset + f.size) as usize).min(desc.sizeof as usize),
        );
        if a >= b {
            continue;
        }
        for d in &mut declared[a..b] {
            *d = true;
        }
        if f.alias_of.is_some() {
            continue;
        }
        if f.repr.materialised() {
            c.fields_sourced += 1;
            for s in &mut sourced_bytes[a..b] {
                *s = true;
            }
        } else {
            c.fields_unsourced += 1;
        }
    }
    c.sourced = sourced_bytes.iter().filter(|b| **b).count() as u32;
    c.unaccounted = declared.iter().filter(|d| !**d).count() as u32;
    c.unsourced = c.bytes - c.sourced - c.unaccounted;
    if let Some(m) = mask {
        c.walked = m.count();
        for (i, s) in sourced_bytes.iter().enumerate() {
            if *s && m.bytes.get(i).copied().unwrap_or(false) {
                c.sourced_walked += 1;
            }
        }
        c.unsourced_walked = c.walked - c.sourced_walked;
    }
    c
}

fn put(img: &mut [u8], off: usize, bytes: &[u8]) -> bool {
    if off + bytes.len() > img.len() {
        return false;
    }
    img[off..off + bytes.len()].copy_from_slice(bytes);
    true
}

/// Write one field of one row into `img`.
fn image_field<C: PdbColumns>(cols: &C, row: usize, f: &FieldDesc, img: &mut [u8]) -> u32 {
    if f.alias_of.is_some() || !f.repr.materialised() || f.count == 0 {
        return 0;
    }
    let elems = f.count as usize;
    let esz = (f.size / f.count) as usize;
    let mask = MASKED_COORDS.contains(&f.name);
    let mut wrote = 0u32;
    for i in 0..elems {
        let plane = f.plane as usize + i;
        let off = f.offset as usize + i * esz;
        let ok = match f.pool {
            Pool::W4 => {
                let v = *cols.w4(plane).get(row).unwrap_or(&0);
                let v = if mask { v ^ COORD_XOR } else { v };
                match f.repr {
                    Repr::U32 => put(img, off, &(v as u32).to_le_bytes()),
                    _ => put(img, off, &v.to_le_bytes()),
                }
            }
            Pool::W2 => {
                let v = *cols.w2(plane).get(row).unwrap_or(&0);
                put(img, off, &v.to_le_bytes())
            }
            Pool::W1 => {
                let v = *cols.w1(plane).get(row).unwrap_or(&0);
                put(img, off, &(v as u8).to_le_bytes())
            }
            Pool::WF => {
                let v = *cols.wf(plane).get(row).unwrap_or(&0.0);
                put(img, off, &v.to_le_bytes())
            }
            Pool::None => false,
        };
        if ok {
            wrote += esz as u32;
        }
    }
    wrote
}

/// Image row `row` of `cols` as the engine's object, `sizeof` bytes long.
///
/// Bytes no column materialises stay zero. That is a *state* claim, not a
/// formatting convenience — an unmodelled `Array<T>` really is walked by retail
/// and really is not produced by us — so the count comes back with the image.
pub fn image_row<C: PdbColumns>(cols: &C, row: usize) -> Vec<u8> {
    let desc = C::desc();
    let mut img = vec![0u8; desc.sizeof as usize];
    if row >= cols.rows() {
        return img;
    }
    for f in desc.fields {
        image_field(cols, row, f, &mut img);
    }
    img
}

/// The imaging coverage of a class, with the walked mask taken from the
/// generated traversal for the class named by `walk_class`.
pub fn class_coverage<C: PdbColumns>() -> ImageCoverage {
    let desc = C::desc();
    let mask = desc
        .walk_class
        .and_then(crate::walk_gen::class_index)
        .map(|k| crate::walk::WalkedMask::of_class(k, desc.sizeof as usize));
    coverage_of(desc, mask.as_ref())
}

/// Read a scalar field back out of an image, by name. The inverse of
/// [`image_row`] for one field, and the only honest way to test the forward
/// direction — a test that re-derives the offset by hand tests the test.
pub fn read_i64(desc: &ClassDesc, img: &[u8], name: &str) -> Option<i64> {
    let f = desc.field(name)?;
    let o = f.offset as usize;
    let esz = if f.count == 0 {
        f.size
    } else {
        f.size / f.count
    } as usize;
    if o + esz > img.len() {
        return None;
    }
    let b = &img[o..o + esz];
    Some(match (f.repr, esz) {
        (Repr::I8, 1) => b[0] as i8 as i64,
        (Repr::U8, 1) => b[0] as i64,
        (Repr::I16, 2) => i16::from_le_bytes([b[0], b[1]]) as i64,
        (Repr::U16, 2) => u16::from_le_bytes([b[0], b[1]]) as i64,
        (Repr::I32, 4) => i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as i64,
        (Repr::U32, 4) => u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as i64,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use don_sim::generated::state::{unit, UnitCols};

    fn unit_desc() -> &'static ClassDesc {
        &unit::DESC
    }

    /// The image is the PDB's `sizeof`, and the fields land where the PDB says.
    /// Values are read back through the *table*, so nothing here hand-counts an
    /// offset.
    #[test]
    fn a_unit_row_images_at_the_pdb_offsets() {
        let mut w = don_sim::World::with_capacity(4, 1);
        let h = w.spawn(3).expect("spawn");
        let row = w.row_of(h).expect("row");
        w.units.angle_mut()[row] = 0x1234_5678;
        w.units.myhits_mut()[row] = 4242;
        w.units.set_uid(row, 0xbeef);
        w.units.myarmor_mut()[row] = -7;

        let img = image_row(&w.units, row);
        assert_eq!(img.len(), 344, "sizeof(Unit)");

        let d = unit_desc();
        assert_eq!(read_i64(d, &img, "angle"), Some(0x1234_5678));
        assert_eq!(read_i64(d, &img, "myhits"), Some(4242));
        assert_eq!(read_i64(d, &img, "uid"), Some(0xbeef));
        assert_eq!(read_i64(d, &img, "myarmor"), Some(-7));
        assert_eq!(read_i64(d, &img, "who"), Some(3));
    }

    /// Coordinates are masked on the way into the image and only there.
    #[test]
    fn coordinates_are_stored_masked_in_the_image() {
        let mut w = don_sim::World::with_capacity(4, 7);
        let h = w.spawn(0).expect("spawn");
        let row = w.row_of(h).expect("row");
        w.set_pos(row, 0x0012_3456, 0x0065_4321);

        let img = image_row(&w.units, row);
        let d = unit_desc();
        assert_eq!(
            read_i64(d, &img, "x_internal"),
            Some((0x0012_3456i32 ^ COORD_XOR) as i64)
        );
        assert_eq!(
            read_i64(d, &img, "y_internal"),
            Some((0x0065_4321i32 ^ COORD_XOR) as i64)
        );
        // and the column itself was not disturbed
        assert_eq!(w.pos_x()[row], 0x0012_3456);
    }

    /// The imaging is not silently complete: `UnitData` carries containers we do
    /// not materialise, some of which the checksum visits. The bridge must say
    /// so, and the accounting must close.
    #[test]
    fn image_coverage_counts_what_it_cannot_source() {
        let c = class_coverage::<UnitCols>();
        assert_eq!(
            c.sourced + c.unsourced + c.unaccounted,
            c.bytes,
            "every byte accounted: {c:?}"
        );
        assert!(c.fields_unsourced > 0, "UnitData has unmaterialised fields");
        assert!(c.walked > 0, "Unit has a derived walker");
        assert_eq!(
            c.sourced_walked + c.unsourced_walked,
            c.walked,
            "walked bytes split cleanly"
        );
        // Unit::walk_data walks [72,183) itself and reaches Object/SubObject for
        // [32,66) and [8,24); all of those are materialised scalars, so the
        // sourced share is the whole walked set of the flat record.
        assert!(
            c.sourced_walked >= 111 + 34 + 16,
            "Unit 111 + Object 34 + SubObject 16 are all columns: {c:?}"
        );
    }

    /// An empty column block images zeroes rather than reading a row that is not
    /// there.
    #[test]
    fn an_out_of_range_row_images_zeroes() {
        let cols = UnitCols::with_capacity(4);
        let img = image_row(&cols, 0);
        assert!(img.iter().all(|&b| b == 0));
        assert_eq!(img.len(), 344);
    }
}
