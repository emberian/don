//! The unit-versus-unit balance table.
//!
//! # What this is
//!
//! `Balance combat_table` lives at `0x00C12BF0`; the array inside it is
//! `Balance::final_balance_table` at **`0x00C12BF4`**, and the PDB type record says
//! `short[493][493]` — 486,098 bytes. `ObjectData::get_damage` reads
//! `(i32)(i16) table[attacker_type_id * 493 + defender_type_id]` at `0x0064418E` and
//! feeds it into the chain as a percentage. [`crate::mechanics::balance_index`] is that
//! address arithmetic.
//!
//! The bytes are **not embedded here**. They are captured game content and live in
//! `schema/live/balance-real.bin`, which is gitignored; this module loads that file.
//! Loading rather than embedding also keeps a stale copy from silently outliving a
//! recapture.
//!
//! # A trap this module exists to prevent
//!
//! An earlier capture used `0x00C06AFC`, which is a **bias-folded base**:
//! `0x00C12BF4 - 0x00C06AFC = 49,400 = 2 * (50*493 + 50)`, because unit ids start at 50.
//! Capturing there reads 49,400 bytes early and produces the "unexplained negatives"
//! that were chased for a day. [`BalanceTable::from_bytes`] therefore rejects a table
//! containing negative entries by default: the real one has none.

use std::path::{Path, PathBuf};

/// Both dimensions of `short[493][493]`.
pub const DIM: usize = 493;
/// Byte length of the whole table.
pub const BYTES: usize = DIM * DIM * 2;
/// Type ids below this are not units. From the 49,400-byte bias above.
pub const FIRST_UNIT_TYPE_ID: i32 = 50;

/// The 493x493 `int16` table.
#[derive(Clone)]
pub struct BalanceTable {
    data: Vec<i16>,
}

#[derive(Debug)]
pub enum LoadError {
    Io(std::io::Error),
    /// The file was not exactly [`BYTES`] long.
    WrongSize { got: usize },
    /// Negative entries: almost certainly captured at the bias-folded base.
    Negative { count: usize, first_index: usize },
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::Io(e) => write!(f, "{e}"),
            LoadError::WrongSize { got } => {
                write!(f, "balance table must be {BYTES} bytes (493x493 i16), got {got}")
            }
            LoadError::Negative { count, first_index } => write!(
                f,
                "{count} negative entries (first at linear index {first_index}); \
                 the real table has none — this looks like a capture at the \
                 bias-folded base 0x00C06AFC instead of 0x00C12BF4"
            ),
        }
    }
}

impl std::error::Error for LoadError {}

impl BalanceTable {
    /// Where the captured table lives, relative to the workspace.
    pub fn default_path() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../schema/live/balance-real.bin")
    }

    /// Load the captured table. Returns `Err` rather than a zero table if it is missing,
    /// because a silently-empty balance table is a simulation that looks like it works.
    pub fn load(path: &Path) -> Result<BalanceTable, LoadError> {
        let raw = std::fs::read(path).map_err(LoadError::Io)?;
        BalanceTable::from_bytes(&raw)
    }

    /// Load from [`BalanceTable::default_path`].
    pub fn load_default() -> Result<BalanceTable, LoadError> {
        BalanceTable::load(&BalanceTable::default_path())
    }

    pub fn from_bytes(raw: &[u8]) -> Result<BalanceTable, LoadError> {
        if raw.len() != BYTES {
            return Err(LoadError::WrongSize { got: raw.len() });
        }
        let data: Vec<i16> = raw
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]))
            .collect();
        if let Some(i) = data.iter().position(|&v| v < 0) {
            let count = data.iter().filter(|&&v| v < 0).count();
            return Err(LoadError::Negative { count, first_index: i });
        }
        Ok(BalanceTable { data })
    }

    /// `(i32)(i16) table[atk * 493 + def]`, the value `get_damage` reads at `0x0064418E`.
    ///
    /// Returns `None` for an index outside the array rather than reading whatever the
    /// engine would read out of bounds — retail has no check here, but silently
    /// returning adjacent memory would be a divergence we could not see.
    #[inline]
    pub fn get(&self, attacker_type_id: i32, defender_type_id: i32) -> Option<i32> {
        let i = crate::mechanics::balance_index(attacker_type_id, defender_type_id);
        if i < 0 {
            return None;
        }
        self.data.get(i as usize).map(|&v| v as i32)
    }

    /// The raw linear array, for anyone doing their own indexing.
    pub fn raw(&self) -> &[i16] {
        &self.data
    }

    /// (min, max, distinct values, non-zero count) — the shape check the capture lane
    /// reported, recomputed here so a swapped file is caught on load.
    pub fn stats(&self) -> (i16, i16, usize, usize) {
        let mut seen = std::collections::BTreeSet::new();
        let mut nz = 0usize;
        for &v in &self.data {
            seen.insert(v);
            if v != 0 {
                nz += 1;
            }
        }
        (
            *self.data.iter().min().unwrap_or(&0),
            *self.data.iter().max().unwrap_or(&0),
            seen.len(),
            nz,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_a_short_file() {
        assert!(matches!(
            BalanceTable::from_bytes(&[0u8; 16]),
            Err(LoadError::WrongSize { .. })
        ));
    }

    #[test]
    fn rejects_negatives_because_the_real_table_has_none() {
        let mut raw = vec![0u8; BYTES];
        raw[0] = 0xFF;
        raw[1] = 0xFF; // -1
        assert!(matches!(BalanceTable::from_bytes(&raw), Err(LoadError::Negative { .. })));
    }

    #[test]
    fn indexing_matches_the_damage_pipeline() {
        let mut raw = vec![0u8; BYTES];
        let i = crate::mechanics::balance_index(50, 51) as usize;
        raw[i * 2] = 0x39;
        raw[i * 2 + 1] = 0x05; // 1337
        let t = BalanceTable::from_bytes(&raw).unwrap();
        assert_eq!(t.get(50, 51), Some(1337));
        assert_eq!(t.get(50, 52), Some(0));
    }

    #[test]
    fn out_of_domain_indices_are_none_not_garbage() {
        let t = BalanceTable::from_bytes(&vec![0u8; BYTES]).unwrap();
        assert_eq!(t.get(492, 492), Some(0));
        assert_eq!(t.get(493, 0), None);
        assert_eq!(t.get(-1, 0), None);
    }

    /// Only runs where the captured file is present (it is gitignored game content).
    #[test]
    fn the_captured_table_loads_and_has_the_reported_shape() {
        let p = BalanceTable::default_path();
        if !p.exists() {
            eprintln!("skipping: {} not present", p.display());
            return;
        }
        let t = BalanceTable::load(&p).expect("captured table must load");
        let (min, max, distinct, nz) = t.stats();
        assert_eq!(t.raw().len(), DIM * DIM);
        // [measured] here, and matching what the capture lane reported: 367 distinct
        // values spanning 5..=2574, no zeros and no negatives anywhere in the array.
        assert_eq!((min, max, distinct), (5, 2574, 367));
        assert_eq!(nz, DIM * DIM, "the real table has no zero entries at all");
    }
}
