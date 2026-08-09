//! Exact mountain-range list randomization used by common map generation.
//!
//! Provenance is the shipped PDB plus the retail instruction stream:
//!
//! - `Mountains::randomize_mountains`, `0x0089ca70`--`0x0089cb4d`;
//! - `Mountains::get_range`, `0x0089cb50`;
//! - `LinkListBase<int,u8>::seek_index`, `0x0046f0e0`.
//!
//! `MountainsData` contains three 24-byte `LinkList<int,u8>` fields at offsets
//! `+4`, `+28`, and `+52`.  The native list caches the current node's data and
//! metric.  This representation replaces pointers with a closed-list index but
//! preserves the observable head-relative seek and return-then-advance cursor.

use crate::rng::Random;

/// PDB `enum MountainRangeSize`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MountainRangeSize {
    Small = 1,
    Medium = 2,
    Large = 3,
}

/// One `LLNode<int,u8>` payload.  Pointer links are represented by vector order.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MountainRangeEntry {
    pub data: i32,
    pub metric: u8,
}

impl MountainRangeEntry {
    pub const fn new(data: i32, metric: u8) -> Self {
        Self { data, metric }
    }
}

/// Pointer-free state of one closed `LinkList<int,u8>`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MountainRangeList {
    entries: Vec<MountainRangeEntry>,
    current_index: Option<usize>,
    current_data: i32,
    current_metric: u8,
}

impl MountainRangeList {
    /// Constructs the supported closed-list domain.  A non-empty native list's
    /// current cache is its head after `head`/`close`; an empty list caches zero.
    pub fn new(entries: Vec<MountainRangeEntry>) -> Self {
        match entries.first().copied() {
            Some(head) => Self {
                entries,
                current_index: Some(0),
                current_data: head.data,
                current_metric: head.metric,
            },
            None => Self::default(),
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    #[inline]
    pub const fn current_index(&self) -> Option<usize> {
        self.current_index
    }

    #[inline]
    pub const fn current_data(&self) -> i32 {
        self.current_data
    }

    #[inline]
    pub const fn current_metric(&self) -> u8 {
        self.current_metric
    }

    /// `seek_index` (`0x0046f0e0`).  The retail helper always restarts at the
    /// head.  Randomization supplies `0 <= index < length`, so its native early
    /// stop at the tail is equivalent to this direct index in the supported
    /// closed-list domain.  Empty lists are untouched and return zero.
    fn seek_index(&mut self, index: usize) -> usize {
        let Some(entry) = self.entries.get(index).copied() else {
            return 0;
        };
        self.current_index = Some(index);
        self.current_data = entry.data;
        self.current_metric = entry.metric;
        index
    }

    /// Native `next` portion embedded in `Mountains::get_range`: return the old
    /// cached data, then advance the cursor once if the list is non-empty.
    fn take_and_advance(&mut self) -> i32 {
        let result = self.current_data;
        if !self.entries.is_empty() {
            let next = (self.current_index.unwrap_or(0) + 1) % self.entries.len();
            self.seek_index(next);
        }
        result
    }
}

/// The three list/cursor fields used by `Mountains::randomize_mountains`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Mountains {
    pub small_ranges: MountainRangeList,
    pub medium_ranges: MountainRangeList,
    pub large_ranges: MountainRangeList,
}

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct MountainRandomizeReceipt {
    /// Head-relative indices selected in SMALL, MEDIUM, LARGE order.
    pub selected_indices: [usize; 3],
    /// Number of main-RNG draws consumed.  This is one per list of length > 1.
    pub draws: u8,
    pub rng_state_after: i32,
}

impl Mountains {
    /// Complete `Mountains::randomize_mountains` (`0x0089ca70`).
    ///
    /// The operation is one ordered three-list transaction: SMALL, MEDIUM, then
    /// LARGE.  A zero- or one-element list seeks index zero without drawing.  A
    /// larger list consumes exactly one `Random::get(0, 0xffff)` and takes the
    /// signed native remainder by its positive length.
    pub fn randomize_mountains(&mut self, random: &mut Random) -> MountainRandomizeReceipt {
        let mut selected_indices = [0; 3];
        let mut draws = 0;

        for (slot, ranges) in [
            &mut self.small_ranges,
            &mut self.medium_ranges,
            &mut self.large_ranges,
        ]
        .into_iter()
        .enumerate()
        {
            let index = if ranges.len() > 1 {
                draws += 1;
                (random.get(0, 0xffff) as usize) % ranges.len()
            } else {
                0
            };
            selected_indices[slot] = ranges.seek_index(index);
        }

        MountainRandomizeReceipt {
            selected_indices,
            draws,
            rng_state_after: random.state(),
        }
    }

    /// Complete deterministic body of `Mountains::get_range` (`0x0089cb50`) for
    /// the PDB-valid enum domain.  Retail diagnoses invalid integer enum values
    /// and returns zero; the typed Rust API makes them unrepresentable.
    pub fn get_range(&mut self, size: MountainRangeSize) -> i32 {
        match size {
            MountainRangeSize::Small => self.small_ranges.take_and_advance(),
            MountainRangeSize::Medium => self.medium_ranges.take_and_advance(),
            MountainRangeSize::Large => self.large_ranges.take_and_advance(),
        }
    }
}
