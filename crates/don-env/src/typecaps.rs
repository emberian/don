//! Per-`TypeIndex` capability records, and the producer → product edge list.
//!
//! # Why this is loaded rather than compiled in
//!
//! The numbers come from `ron-data/unitrules.xml` and `ron-data/buildingrules.xml`, which
//! are shipped game content and are gitignored. `crates/don-env/gen/gen_spec.py` extracts
//! them into `schema/live/env-typecaps.bin`; this module reads that file at runtime. An
//! env constructed without it still runs — broad capability flags answer "yes", while
//! evidence-dependent predicates such as `is_plane` remain false — and
//! [`TypeCaps::is_permissive`] says so. Patrol application then reports no effect instead
//! of guessing a retail order class.
//!
//! # Where the type ids come from
//!
//! The `TypeIndex` enum in the PDB partitions 806 ids: goods `0..50`, units `50..402`,
//! gaia `402..414`, buildings `414..543`, then items/techs/ages/spells/governments. The
//! partition is cross-validated by the shipped XML: `buildingrules.xml` holds exactly 129
//! `<BUILDING>` entries (`NUM_BUILDTYPES`) and `unitrules.xml` exactly 364 `<UNIT>`
//! entries (`NUM_UNITTYPES` 352 + `NUM_GAIATYPES` 12), and positionally
//! `unitrules[0]`→`PEASANTS`, `unitrules[2]`→`SCHOLARS`, `buildingrules[0]`→`VILLAGE`,
//! `buildingrules[128]`→`SPACEPROGRAM`. [measured]

use crate::generated::{NUM_COMMON, NUM_TYPES};
use std::path::Path;

pub const F_MOVE: u16 = 1 << 0;
pub const F_ATTACK: u16 = 1 << 1;
pub const F_CIVILIAN: u16 = 1 << 2;
pub const F_SIEGE: u16 = 1 << 3;
pub const F_GARR_TOWN: u16 = 1 << 4;
pub const F_GARR_FORT: u16 = 1 << 5;
pub const F_CASTER: u16 = 1 << 6;
pub const F_AIR: u16 = 1 << 7;
pub const F_SEA: u16 = 1 << 8;
pub const F_TRANSPORT: u16 = 1 << 9;
pub const F_STEALTH: u16 = 1 << 10;
pub const F_DETECT: u16 = 1 << 11;
pub const F_ANTIAIR: u16 = 1 << 12;
pub const F_PRODUCER: u16 = 1 << 13;
pub const F_BUILDING: u16 = 1 << 14;
pub const F_UNIT: u16 = 1 << 15;

pub const FLAG_NAMES: [(&str, u16); 16] = [
    ("MOVE", F_MOVE),
    ("ATTACK", F_ATTACK),
    ("CIVILIAN", F_CIVILIAN),
    ("SIEGE", F_SIEGE),
    ("GARR_TOWN", F_GARR_TOWN),
    ("GARR_FORT", F_GARR_FORT),
    ("CASTER", F_CASTER),
    ("AIR", F_AIR),
    ("SEA", F_SEA),
    ("TRANSPORT", F_TRANSPORT),
    ("STEALTH", F_STEALTH),
    ("DETECT", F_DETECT),
    ("ANTIAIR", F_ANTIAIR),
    ("PRODUCER", F_PRODUCER),
    ("BUILDING", F_BUILDING),
    ("UNIT", F_UNIT),
];

/// One `TypeIndex`'s static capability record. Field names are the shipped XML column
/// names; nothing here is renamed to a community term.
#[derive(Clone, Copy, Debug, Default)]
pub struct TypeCap {
    pub flags: u16,
    /// `ATTACK`. Display scale; `ObjectData::get_damage` consumes it ×10.
    pub attack: i16,
    pub hits: i32,
    pub armor: i16,
    /// `MOVES`.
    pub move_rate: i16,
    /// `RANGE`, in TCoords. i32 because two nuclear types ship 0-99999.
    pub range_min: i32,
    pub range_max: i32,
    pub los: i16,
    pub recharge: i16,
    pub pop: u8,
    /// The shipped `DOMAIN` value. Do not use `domain == 2` as an `is_plane`
    /// predicate: `DomainIndex` aliases `BOTH` and `AIR` at 2, and helicopters
    /// are air-domain but fail retail's `UnitData::is_plane` test.
    pub domain: u8,
    /// 0 good, 1 unit, 2 gaia, 3 building, 4 other.
    pub category: u8,
    /// `UnitData::is_plane` `0x0046CE40`: `domain == AIR &&
    /// !(unit_flags & 0x20)`. Stored in the formerly reserved byte at offset 27
    /// of `env-typecaps.bin`, derived from shipped `DOMAIN` + `FLAGS`.
    pub is_plane: bool,
    pub cost: [i32; NUM_COMMON],
    pub support: [i32; NUM_COMMON],
}

impl TypeCap {
    #[inline]
    pub fn has(&self, f: u16) -> bool {
        self.flags & f != 0
    }
}

const REC: usize = 76;
// v2 assigns the formerly reserved record byte +27 to `is_plane`. A v1 table must not
// load as all-false: that would silently route every plane patrol to GROUP_PATROL.
const MAGIC: &[u8; 8] = b"DONTYPC2";

/// The whole table, plus the producible-set bitsets that drive the `Type` head mask.
pub struct TypeCaps {
    caps: Vec<TypeCap>,
    /// For each type, a `NUM_TYPES`-bit set of the types it can produce. Bit-packed
    /// little-endian so a per-entity type mask is a `memcpy`-and-`AND` of `bitset_bytes`
    /// rather than 806 branches.
    produces: Vec<u8>,
    pub bitset_bytes: usize,
    permissive: bool,
}

impl TypeCaps {
    pub fn bitset_bytes_for(n: usize) -> usize {
        n.div_ceil(8)
    }

    /// Broad flags allowed, no exact type predicates derived. Used when
    /// `env-typecaps.bin` is absent.
    pub fn permissive() -> TypeCaps {
        let bb = Self::bitset_bytes_for(NUM_TYPES);
        TypeCaps {
            caps: vec![
                TypeCap {
                    flags: u16::MAX,
                    hits: 100,
                    move_rate: 1,
                    range_max: 1_000_000,
                    ..Default::default()
                };
                NUM_TYPES
            ],
            produces: vec![0xFF; NUM_TYPES * bb],
            bitset_bytes: bb,
            permissive: true,
        }
    }

    pub fn is_permissive(&self) -> bool {
        self.permissive
    }

    /// Heap payload reserved by the immutable capability tables, excluding allocator
    /// metadata. `Rules` is shared by every world in a [`crate::env::VecEnv`], so this
    /// amount belongs once in a batch memory report rather than once per world.
    pub fn bytes_reserved(&self) -> usize {
        self.caps.capacity() * std::mem::size_of::<TypeCap>()
            + self.produces.capacity() * std::mem::size_of::<u8>()
    }

    pub fn load(path: &Path) -> std::io::Result<TypeCaps> {
        Self::from_bytes(&std::fs::read(path)?)
    }

    /// Load from the default location relative to the repo root, falling back to
    /// [`TypeCaps::permissive`]. Returns whether a real table was found.
    pub fn load_or_permissive(path: Option<&Path>) -> (TypeCaps, bool) {
        let p = path.map(|p| p.to_path_buf()).unwrap_or_else(default_path);
        match TypeCaps::load(&p) {
            Ok(t) => (t, true),
            Err(_) => (TypeCaps::permissive(), false),
        }
    }

    pub fn from_bytes(b: &[u8]) -> std::io::Result<TypeCaps> {
        use std::io::{Error, ErrorKind};
        let bad = |m: &str| Error::new(ErrorKind::InvalidData, m.to_string());
        if b.len() < 16 || &b[..8] != MAGIC {
            return Err(bad("env-typecaps.bin: bad magic"));
        }
        let n = u32::from_le_bytes(b[8..12].try_into().unwrap()) as usize;
        let edges = u32::from_le_bytes(b[12..16].try_into().unwrap()) as usize;
        if n != NUM_TYPES {
            return Err(bad("env-typecaps.bin: NUM_TYPES mismatch — regenerate"));
        }
        if b.len() < 16 + n * REC + edges * 4 {
            return Err(bad("env-typecaps.bin: truncated"));
        }
        let rd16 = |o: usize| i16::from_le_bytes(b[o..o + 2].try_into().unwrap());
        let rd32 = |o: usize| i32::from_le_bytes(b[o..o + 4].try_into().unwrap());
        let mut caps = Vec::with_capacity(n);
        for t in 0..n {
            let o = 16 + t * REC;
            let mut c = TypeCap {
                flags: u16::from_le_bytes(b[o..o + 2].try_into().unwrap()),
                attack: rd16(o + 2),
                hits: rd32(o + 4),
                armor: rd16(o + 8),
                move_rate: rd16(o + 10),
                range_min: rd32(o + 12),
                range_max: rd32(o + 16),
                los: rd16(o + 20),
                recharge: rd16(o + 22),
                pop: b[o + 24],
                domain: b[o + 25],
                category: b[o + 26],
                is_plane: b[o + 27] != 0,
                ..Default::default()
            };
            for k in 0..NUM_COMMON {
                c.cost[k] = rd32(o + 28 + k * 4);
                c.support[k] = rd32(o + 52 + k * 4);
            }
            caps.push(c);
        }
        let bb = Self::bitset_bytes_for(n);
        let mut produces = vec![0u8; n * bb];
        let eo = 16 + n * REC;
        for e in 0..edges {
            let o = eo + e * 4;
            let a = u16::from_le_bytes(b[o..o + 2].try_into().unwrap()) as usize;
            let p = u16::from_le_bytes(b[o + 2..o + 4].try_into().unwrap()) as usize;
            if a < n && p < n {
                produces[a * bb + p / 8] |= 1 << (p % 8);
            }
        }
        Ok(TypeCaps {
            caps,
            produces,
            bitset_bytes: bb,
            permissive: false,
        })
    }

    #[inline]
    pub fn get(&self, t: u16) -> &TypeCap {
        &self.caps[(t as usize).min(NUM_TYPES - 1)]
    }

    /// Bit-packed set of types `t` can train/research. Empty for non-producers.
    #[inline]
    pub fn produces(&self, t: u16) -> &[u8] {
        let i = (t as usize).min(NUM_TYPES - 1) * self.bitset_bytes;
        &self.produces[i..i + self.bitset_bytes]
    }

    /// Types with the `BUILDING` flag — the candidate set for the `Build` verb.
    pub fn building_bitset(&self) -> Vec<u8> {
        let mut v = vec![0u8; self.bitset_bytes];
        for (t, c) in self.caps.iter().enumerate() {
            if c.has(F_BUILDING) {
                v[t / 8] |= 1 << (t % 8);
            }
        }
        v
    }
}

pub fn default_path() -> std::path::PathBuf {
    // The crate sits at <root>/crates/don-env, so the repo root is two levels up from
    // CARGO_MANIFEST_DIR. Overridable with DON_TYPECAPS.
    if let Ok(p) = std::env::var("DON_TYPECAPS") {
        return p.into();
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schema/live/env-typecaps.bin")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generated::{BUILD_TYPE_BASE, UNIT_TYPE_BASE};

    #[test]
    fn permissive_table_is_flagged_as_such() {
        let t = TypeCaps::permissive();
        assert!(t.is_permissive());
        assert!(t.get(0).has(F_ATTACK));
    }

    #[test]
    fn v2_plane_byte_is_required_and_loaded() {
        let mut b = vec![0u8; 16 + NUM_TYPES * REC];
        b[..8].copy_from_slice(MAGIC);
        b[8..12].copy_from_slice(&(NUM_TYPES as u32).to_le_bytes());
        b[16 + 289 * REC + 27] = 1;
        let t = TypeCaps::from_bytes(&b).unwrap();
        assert!(t.get(289).is_plane);
        assert!(!t.get(310).is_plane);

        b[..8].copy_from_slice(b"DONTYPC1");
        assert!(
            TypeCaps::from_bytes(&b).is_err(),
            "v1's reserved zero byte cannot be treated as an is_plane table"
        );
    }

    /// Only meaningful on a tree that has run the generator; skips otherwise so the
    /// suite is honest about what it did not check rather than vacuously green.
    #[test]
    fn real_table_agrees_with_the_typeindex_partition() {
        let (t, found) = TypeCaps::load_or_permissive(None);
        if !found {
            eprintln!("SKIP: schema/live/env-typecaps.bin absent (run gen/gen_spec.py)");
            return;
        }
        assert!(!t.is_permissive());
        // PEASANTS = 50, the first unitrules entry ("Citizen"): civilian, mobile, cheap.
        let peasant = t.get(UNIT_TYPE_BASE as u16);
        assert!(peasant.has(F_UNIT) && peasant.has(F_CIVILIAN) && peasant.has(F_MOVE));
        assert!(!peasant.is_plane);
        assert_eq!(peasant.hits, 40, "unitrules Citizen HITS");
        // PDB TypeIndex values; rule-side classification is the exact retail
        // `UnitData::is_plane` predicate, not a broad AIR capability flag.
        assert!(t.get(289).is_plane, "FIGHTER");
        assert!(!t.get(310).is_plane, "HELICOPTER");
        // VILLAGE = 414, the first buildingrules entry ("Small City").
        let village = t.get(BUILD_TYPE_BASE as u16);
        assert!(village.has(F_BUILDING));
        assert!(
            village.has(F_PRODUCER),
            "Small City trains Citizens (unit WHERE column)"
        );
        let bits = village.flags;
        assert_eq!(bits & F_UNIT, 0, "a building must not carry the UNIT flag");
        // and the producer edge really names the peasant
        let p = t.produces(BUILD_TYPE_BASE as u16);
        assert_ne!(p[UNIT_TYPE_BASE / 8] & (1 << (UNIT_TYPE_BASE % 8)), 0);
    }
}
