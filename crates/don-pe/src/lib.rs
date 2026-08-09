//! Minimal PE32 reader and image mapper.
//!
//! This is the first half of the stage-3 oracle (`docs/oracle-architecture.md`): map
//! `riseofnations.exe` into memory ourselves, apply its base relocations, and hand out
//! callable addresses by RVA, so that individual retail functions can be invoked with
//! controlled inputs and differentially tested against our reimplementation.
//!
//! Deliberately *not* `LoadLibrary`. Loading an EXE as a library runs its entry point as
//! `DllMain(DLL_PROCESS_ATTACH)`, which is wrong, and drags in the loader's import, TLS and
//! CFG processing whether we want it or not. Mapping by hand keeps the harness honest about
//! exactly what state a called function can see.
//!
//! # Scope
//!
//! Parsing and mapping are architecture-independent and are tested on any host. *Calling*
//! into the mapped image requires an i686 process (see `docs/oracle-architecture.md`); this
//! crate stops at producing a correctly relocated image and resolving RVAs.
//!
//! Reachability still governs what may be called: a function that touches imports or live
//! globals cannot be driven from fabricated inputs no matter how well we map it.

use std::fmt;

pub const IMAGE_DOS_SIGNATURE: u16 = 0x5A4D; // "MZ"
pub const IMAGE_NT_SIGNATURE: u32 = 0x0000_4550; // "PE\0\0"
pub const IMAGE_FILE_MACHINE_I386: u16 = 0x014C;
pub const PE32_MAGIC: u16 = 0x010B;

/// Index of the base-relocation entry in the data directory.
const DIR_BASERELOC: usize = 5;

#[derive(Debug)]
pub enum PeError {
    Truncated {
        what: &'static str,
        need: usize,
        have: usize,
    },
    BadDosSignature(u16),
    BadNtSignature(u32),
    UnsupportedMachine(u16),
    UnsupportedMagic(u16),
    NoRelocations,
    BadRelocBlock {
        at: usize,
    },
}

impl fmt::Display for PeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PeError::Truncated { what, need, have } => {
                write!(
                    f,
                    "truncated reading {what}: need {need} bytes, have {have}"
                )
            }
            PeError::BadDosSignature(v) => write!(f, "bad DOS signature {v:#06x}"),
            PeError::BadNtSignature(v) => write!(f, "bad NT signature {v:#010x}"),
            PeError::UnsupportedMachine(v) => write!(f, "unsupported machine {v:#06x}"),
            PeError::UnsupportedMagic(v) => write!(f, "unsupported optional-header magic {v:#06x}"),
            PeError::NoRelocations => write!(f, "image has no base-relocation directory"),
            PeError::BadRelocBlock { at } => write!(f, "malformed relocation block at {at:#x}"),
        }
    }
}

impl std::error::Error for PeError {}

#[derive(Debug, Clone)]
pub struct Section {
    pub name: String,
    pub virtual_size: u32,
    pub virtual_address: u32,
    pub size_of_raw_data: u32,
    pub pointer_to_raw_data: u32,
    pub characteristics: u32,
}

impl Section {
    pub fn is_executable(&self) -> bool {
        self.characteristics & 0x2000_0000 != 0
    }
    pub fn is_writable(&self) -> bool {
        self.characteristics & 0x8000_0000 != 0
    }
    pub fn contains_rva(&self, rva: u32) -> bool {
        rva >= self.virtual_address && rva < self.virtual_address + self.virtual_size.max(1)
    }
}

#[derive(Debug, Clone)]
pub struct PeImage {
    pub machine: u16,
    pub image_base: u32,
    pub entry_point_rva: u32,
    pub size_of_image: u32,
    pub size_of_headers: u32,
    pub section_alignment: u32,
    pub file_alignment: u32,
    pub characteristics: u16,
    pub dll_characteristics: u16,
    pub sections: Vec<Section>,
    /// (rva, size) per data directory entry.
    pub data_directories: Vec<(u32, u32)>,
}

fn rd_u16(b: &[u8], off: usize, what: &'static str) -> Result<u16, PeError> {
    b.get(off..off + 2)
        .map(|s| u16::from_le_bytes([s[0], s[1]]))
        .ok_or(PeError::Truncated {
            what,
            need: off + 2,
            have: b.len(),
        })
}

fn rd_u32(b: &[u8], off: usize, what: &'static str) -> Result<u32, PeError> {
    b.get(off..off + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
        .ok_or(PeError::Truncated {
            what,
            need: off + 4,
            have: b.len(),
        })
}

impl PeImage {
    /// Parse the headers of a PE32 image. Does not copy section data.
    pub fn parse(bytes: &[u8]) -> Result<PeImage, PeError> {
        let dos = rd_u16(bytes, 0, "DOS signature")?;
        if dos != IMAGE_DOS_SIGNATURE {
            return Err(PeError::BadDosSignature(dos));
        }
        let e_lfanew = rd_u32(bytes, 0x3C, "e_lfanew")? as usize;
        let sig = rd_u32(bytes, e_lfanew, "NT signature")?;
        if sig != IMAGE_NT_SIGNATURE {
            return Err(PeError::BadNtSignature(sig));
        }

        let coff = e_lfanew + 4;
        let machine = rd_u16(bytes, coff, "machine")?;
        if machine != IMAGE_FILE_MACHINE_I386 {
            return Err(PeError::UnsupportedMachine(machine));
        }
        let num_sections = rd_u16(bytes, coff + 2, "num sections")? as usize;
        let size_of_optional = rd_u16(bytes, coff + 16, "size of optional header")? as usize;
        let characteristics = rd_u16(bytes, coff + 18, "characteristics")?;

        let opt = coff + 20;
        let magic = rd_u16(bytes, opt, "optional magic")?;
        if magic != PE32_MAGIC {
            return Err(PeError::UnsupportedMagic(magic));
        }
        let entry_point_rva = rd_u32(bytes, opt + 16, "entry point")?;
        let image_base = rd_u32(bytes, opt + 28, "image base")?;
        let section_alignment = rd_u32(bytes, opt + 32, "section alignment")?;
        let file_alignment = rd_u32(bytes, opt + 36, "file alignment")?;
        let size_of_image = rd_u32(bytes, opt + 56, "size of image")?;
        let size_of_headers = rd_u32(bytes, opt + 60, "size of headers")?;
        let dll_characteristics = rd_u16(bytes, opt + 70, "dll characteristics")?;
        let num_dirs = rd_u32(bytes, opt + 92, "number of rva and sizes")? as usize;

        let mut data_directories = Vec::with_capacity(num_dirs);
        for i in 0..num_dirs.min(16) {
            let base = opt + 96 + i * 8;
            data_directories.push((
                rd_u32(bytes, base, "data dir rva")?,
                rd_u32(bytes, base + 4, "data dir size")?,
            ));
        }

        let sec_table = opt + size_of_optional;
        let mut sections = Vec::with_capacity(num_sections);
        for i in 0..num_sections {
            let s = sec_table + i * 40;
            let raw_name = bytes.get(s..s + 8).ok_or(PeError::Truncated {
                what: "section name",
                need: s + 8,
                have: bytes.len(),
            })?;
            let name = String::from_utf8_lossy(raw_name)
                .trim_end_matches('\0')
                .to_string();
            sections.push(Section {
                name,
                virtual_size: rd_u32(bytes, s + 8, "section vsize")?,
                virtual_address: rd_u32(bytes, s + 12, "section vaddr")?,
                size_of_raw_data: rd_u32(bytes, s + 16, "section rawsize")?,
                pointer_to_raw_data: rd_u32(bytes, s + 20, "section rawptr")?,
                characteristics: rd_u32(bytes, s + 36, "section chars")?,
            });
        }

        Ok(PeImage {
            machine,
            image_base,
            entry_point_rva,
            size_of_image,
            size_of_headers,
            section_alignment,
            file_alignment,
            characteristics,
            dll_characteristics,
            sections,
            data_directories,
        })
    }

    /// True when the image carries a base-relocation directory, i.e. it can be mapped at
    /// an address other than its preferred base. Required for the map-and-call oracle.
    pub fn has_relocations(&self) -> bool {
        self.data_directories
            .get(DIR_BASERELOC)
            .is_some_and(|(rva, size)| *rva != 0 && *size != 0)
    }

    /// Translate an RVA to an offset in the on-disk file, if it falls in a section's raw data.
    pub fn rva_to_file_offset(&self, rva: u32) -> Option<usize> {
        for s in &self.sections {
            if s.contains_rva(rva) {
                let delta = rva - s.virtual_address;
                if delta < s.size_of_raw_data {
                    return Some((s.pointer_to_raw_data + delta) as usize);
                }
                return None; // in a BSS-style tail with no backing bytes
            }
        }
        None
    }

    pub fn section_containing(&self, rva: u32) -> Option<&Section> {
        self.sections.iter().find(|s| s.contains_rva(rva))
    }

    /// Build the mapped image: a `size_of_image`-sized buffer with headers and each
    /// section placed at its virtual address, zero-filled elsewhere (so uninitialised
    /// data reads as zero, as the real loader guarantees).
    pub fn map(&self, bytes: &[u8]) -> Result<Vec<u8>, PeError> {
        let mut image = vec![0u8; self.size_of_image as usize];

        let hdr = (self.size_of_headers as usize).min(bytes.len());
        image[..hdr].copy_from_slice(&bytes[..hdr]);

        for s in &self.sections {
            let raw_start = s.pointer_to_raw_data as usize;
            let raw_len = (s.size_of_raw_data as usize).min(bytes.len().saturating_sub(raw_start));
            if raw_len == 0 {
                continue;
            }
            let va = s.virtual_address as usize;
            let copy_len = raw_len.min(image.len().saturating_sub(va));
            image[va..va + copy_len].copy_from_slice(&bytes[raw_start..raw_start + copy_len]);
        }
        Ok(image)
    }

    /// Apply base relocations to a mapped image so it is valid at `actual_base`.
    ///
    /// Returns the number of fixups applied. PE32 images use `IMAGE_REL_BASED_HIGHLOW`
    /// (type 3) almost exclusively; type 0 is padding and is skipped.
    pub fn relocate(&self, image: &mut [u8], actual_base: u32) -> Result<usize, PeError> {
        let (dir_rva, dir_size) = *self
            .data_directories
            .get(DIR_BASERELOC)
            .filter(|(r, s)| *r != 0 && *s != 0)
            .ok_or(PeError::NoRelocations)?;

        let delta = actual_base.wrapping_sub(self.image_base);
        if delta == 0 {
            return Ok(0);
        }

        let mut applied = 0usize;
        let mut off = dir_rva as usize;
        let end = off + dir_size as usize;

        while off + 8 <= end.min(image.len()) {
            let page_rva = rd_u32(image, off, "reloc page rva")?;
            let block_size = rd_u32(image, off + 4, "reloc block size")? as usize;
            if block_size < 8 {
                if block_size == 0 {
                    break; // terminator
                }
                return Err(PeError::BadRelocBlock { at: off });
            }
            let entries = (block_size - 8) / 2;
            for i in 0..entries {
                let e = rd_u16(image, off + 8 + i * 2, "reloc entry")?;
                let kind = e >> 12;
                let fixup = page_rva as usize + (e & 0x0FFF) as usize;
                match kind {
                    0 => {} // ABSOLUTE: padding
                    3 => {
                        // HIGHLOW: add delta to the 32-bit value at the fixup
                        if fixup + 4 <= image.len() {
                            let cur = rd_u32(image, fixup, "reloc target")?;
                            image[fixup..fixup + 4]
                                .copy_from_slice(&cur.wrapping_add(delta).to_le_bytes());
                            applied += 1;
                        }
                    }
                    _ => {} // other kinds do not occur in PE32 x86 images we target
                }
            }
            off += block_size;
        }
        Ok(applied)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The real binary. Absent from the repo (copyrighted); tests that need it skip.
    fn load() -> Option<Vec<u8>> {
        let p = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../ron-bin/riseofnations.exe"
        );
        std::fs::read(p).ok()
    }

    #[test]
    fn rejects_non_pe() {
        assert!(matches!(
            PeImage::parse(b"not a pe at all"),
            Err(PeError::Truncated { .. }) | Err(PeError::BadDosSignature(_))
        ));
    }

    #[test]
    fn parses_headers_matching_measured_ground_truth() {
        let Some(b) = load() else {
            eprintln!("skipping: binary absent");
            return;
        };
        let pe = PeImage::parse(&b).expect("parse");
        // Values independently measured with pefile; see docs/binary-ground-truth.md.
        assert_eq!(pe.machine, IMAGE_FILE_MACHINE_I386);
        assert_eq!(pe.image_base, 0x0040_0000);
        assert_eq!(pe.entry_point_rva, 0x0015_d699);
        assert_eq!(pe.sections.len(), 8);
        assert!(pe.has_relocations(), "map-and-call oracle needs .reloc");
        let text = &pe.sections[0];
        assert_eq!(text.name, ".text");
        assert_eq!(text.virtual_address, 0x1000);
        assert_eq!(text.virtual_size, 7_090_736);
        assert!(text.is_executable());
    }

    #[test]
    fn rva_translation_round_trips_against_known_addresses() {
        let Some(b) = load() else { return };
        let pe = PeImage::parse(&b).expect("parse");
        // FUN_0065fc00 is the combat-stats loader; its RVA must land inside .text.
        let rva = 0x0065_fc00 - pe.image_base;
        let sec = pe.section_containing(rva).expect("in a section");
        assert_eq!(sec.name, ".text");
        assert!(pe.rva_to_file_offset(rva).is_some());
    }

    #[test]
    fn maps_and_relocates() {
        let Some(b) = load() else { return };
        let pe = PeImage::parse(&b).expect("parse");
        let mut image = pe.map(&b).expect("map");
        assert_eq!(image.len(), pe.size_of_image as usize);

        // The mapped bytes at a known code RVA must equal the file bytes there.
        let rva = (0x0065_fc00u32) - pe.image_base;
        let fo = pe.rva_to_file_offset(rva).unwrap();
        assert_eq!(&image[rva as usize..rva as usize + 16], &b[fo..fo + 16]);

        // Relocating to the preferred base is a no-op...
        assert_eq!(pe.relocate(&mut image, pe.image_base).unwrap(), 0);

        // ...and relocating elsewhere applies a large number of HIGHLOW fixups.
        let before = image.clone();
        let applied = pe.relocate(&mut image, 0x1000_0000).expect("relocate");
        assert!(applied > 100_000, "expected many fixups, got {applied}");
        assert_ne!(before, image, "relocation must change the image");
    }

    #[test]
    fn relocation_is_reversible() {
        // Relocating to B then back to the original base must restore the image exactly.
        // This is the strongest cheap check that our fixup arithmetic is right.
        let Some(b) = load() else { return };
        let pe = PeImage::parse(&b).expect("parse");
        let mut image = pe.map(&b).expect("map");
        let pristine = image.clone();

        pe.relocate(&mut image, 0x2000_0000).expect("relocate away");
        assert_ne!(pristine, image);

        // relocate() computes delta from the *preferred* base, so undo by hand: applying
        // the inverse delta requires pretending the current base is the target.
        let inverse = PeImage {
            image_base: 0x2000_0000,
            ..pe.clone()
        };
        inverse
            .relocate(&mut image, pe.image_base)
            .expect("relocate back");
        assert_eq!(pristine, image, "relocation round-trip must be lossless");
    }
}
