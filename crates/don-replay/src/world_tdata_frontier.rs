//! Exact section-6/7 visibility producer at `World::wipe`.
//!
//! `World::walk_data` section 6 is `TData[tile_size]`, followed by the three
//! `fog_size` byte planes `seen`, `seen2`, and `seen3`.  The first producer at
//! procedural-map entry is not a terrain-group heuristic: `World::wipe`
//! `0x006b2c00` writes every TData word to zero and clears all three fog planes.
//! Its `World::clear_seen` call also clears section-7 `wcoord_seen`. This adapter
//! executes those five checksum-visible writes transactionally. It deliberately
//! makes no whole-`World::wipe` completeness claim.

#![forbid(unsafe_code)]

use don_sim::checksum::{adler32, ByteSink};
use don_sim::systems::map_terrain::{World, WorldSection};

pub const SHIPPED_EXE_SHA256: &str =
    "30478a44b577cb11ebcbbbf53d3e93ba02fd2aacf3bdefa6552c9b6449625079";
pub const WORLD_WIPE_VA: u32 = 0x006b_2c00;
pub const WORLD_WIPE_RETURN_VA: u32 = 0x006b_2dd7;
pub const WORLD_WIPE_BODY_BYTES: usize = 471;
pub const WORLD_WIPE_BODY_SHA256: &str =
    "12f0886dfc5bff4dafb834a8e9d0f3743c4ba0ec8ba6024003712d81b01138f2";
pub const PROOF_DOCUMENT: &str = "docs/assembly/replay-world-tdata-frontier.md";
pub const TDATA_ZERO_LOOP_WRITE_VA: u32 = 0x006b_2d43;
pub const CLEAR_SEEN_CALL_VA: u32 = 0x006b_2d5f;
pub const WORLD_CLEAR_SEEN_VA: u32 = 0x006b_2250;
pub const WORLD_CLEAR_WCOORD_SEEN_MEMSET_CALL_VA: u32 = 0x006b_22be;
pub const CLEAR_SEEN2_CALL_VA: u32 = 0x006b_2d66;
pub const WORLD_CLEAR_SEEN2_VA: u32 = 0x006b_2160;
pub const CLEAR_SEEN3_MEMSET_VA: u32 = 0x006b_2d75;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ByteRange {
    pub start: usize,
    pub end: usize,
}

impl ByteRange {
    pub const fn len(self) -> usize {
        self.end - self.start
    }
}

/// Exact byte windows inside `World::walk_data(section = 6)`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TDataFogLayout {
    pub tdata: ByteRange,
    pub seen: ByteRange,
    pub seen2: ByteRange,
    pub seen3: ByteRange,
}

impl TDataFogLayout {
    pub const fn bytes(self) -> usize {
        self.seen3.end
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TDataFogPlane {
    TData,
    Seen,
    Seen2,
    Seen3,
    WCoordSeen,
}

impl TDataFogPlane {
    pub const fn section(self) -> WorldSection {
        match self {
            Self::TData | Self::Seen | Self::Seen2 | Self::Seen3 => WorldSection::TDataAndFog,
            Self::WCoordSeen => WorldSection::WCoordSeen,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TDataFogWrittenRange {
    pub plane: TDataFogPlane,
    pub range: ByteRange,
    /// The shipped store or call proving that this complete range is written.
    pub producer_va: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TDataFogWipeError {
    InvalidDimensions {
        xs: i32,
        ys: i32,
    },
    DerivedDimensionMismatch {
        field: &'static str,
        expected: i32,
        actual: i32,
    },
    PlaneLengthMismatch {
        plane: TDataFogPlane,
        expected: usize,
        actual: usize,
    },
    SectionWalkLengthMismatch {
        expected: usize,
        actual: usize,
    },
    NonzeroPostcondition {
        offset: usize,
        value: u8,
    },
}

/// Receipt for the exact section-6 and section-7 slices of one executed
/// `World::wipe`.
///
/// `section_bytes_written` counts stores, including stores which rewrite zero.
/// `section_bytes_changed` counts only byte values which differ across the
/// transaction.  The former is the producer-coverage quantity; using only the
/// latter would incorrectly leave already-zero bytes unsourced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TDataFogWipeReceipt {
    pub entry_va: u32,
    pub resume_va: u32,
    pub layout: TDataFogLayout,
    pub written_ranges: [TDataFogWrittenRange; 5],
    /// Coalesced byte-value differences. These are diagnostic only and are
    /// never the source of producer ownership.
    pub changed_ranges: Vec<ByteRange>,
    /// Coalesced section-7 differences, also diagnostic only.
    pub wcoord_seen_changed_ranges: Vec<ByteRange>,
    pub tdata_cells_written: usize,
    pub fog_cells_written_per_plane: usize,
    pub section_bytes_written: usize,
    pub wcoord_seen_bytes_written: usize,
    pub total_bytes_written: usize,
    pub section_bytes_changed: usize,
    pub wcoord_seen_bytes_changed: usize,
    pub total_bytes_changed: usize,
    pub rewritten_zero_bytes: usize,
    pub nonzero_tdata_words_before: usize,
    pub nonzero_seen_bytes_before: usize,
    pub nonzero_seen2_bytes_before: usize,
    pub nonzero_seen3_bytes_before: usize,
    pub nonzero_wcoord_seen_bytes_before: usize,
    pub section_adler_before: u32,
    pub section_adler_after: u32,
    pub wcoord_seen_adler_before: u32,
    pub wcoord_seen_adler_after: u32,
    pub rng_draws: u32,
}

fn checked_shape(world: &World) -> Result<TDataFogLayout, TDataFogWipeError> {
    if world.xs <= 0 || world.ys <= 0 {
        return Err(TDataFogWipeError::InvalidDimensions {
            xs: world.xs,
            ys: world.ys,
        });
    }
    let size = world
        .xs
        .checked_mul(world.ys)
        .ok_or(TDataFogWipeError::InvalidDimensions {
            xs: world.xs,
            ys: world.ys,
        })?;
    let tile_xs = world
        .xs
        .checked_mul(4)
        .ok_or(TDataFogWipeError::InvalidDimensions {
            xs: world.xs,
            ys: world.ys,
        })?;
    let tile_ys = world
        .ys
        .checked_mul(4)
        .ok_or(TDataFogWipeError::InvalidDimensions {
            xs: world.xs,
            ys: world.ys,
        })?;
    let tile_size = tile_xs
        .checked_mul(tile_ys)
        .ok_or(TDataFogWipeError::InvalidDimensions {
            xs: world.xs,
            ys: world.ys,
        })?;
    let fog_xs = tile_xs
        .checked_mul(2)
        .ok_or(TDataFogWipeError::InvalidDimensions {
            xs: world.xs,
            ys: world.ys,
        })?
        / 4;
    let fog_ys = tile_ys
        .checked_mul(2)
        .ok_or(TDataFogWipeError::InvalidDimensions {
            xs: world.xs,
            ys: world.ys,
        })?
        / 4;
    let fog_size = fog_xs
        .checked_mul(fog_ys)
        .ok_or(TDataFogWipeError::InvalidDimensions {
            xs: world.xs,
            ys: world.ys,
        })?;

    for (field, expected, actual) in [
        ("size", size, world.size),
        ("tile_xs", tile_xs, world.tile_xs),
        ("tile_ys", tile_ys, world.tile_ys),
        ("tile_size", tile_size, world.tile_size),
        ("fog_xs", fog_xs, world.fog_xs),
        ("fog_ys", fog_ys, world.fog_ys),
        ("fog_size", fog_size, world.fog_size),
    ] {
        if expected != actual {
            return Err(TDataFogWipeError::DerivedDimensionMismatch {
                field,
                expected,
                actual,
            });
        }
    }

    let tile_cells = usize::try_from(tile_size).expect("positive checked tile size");
    let fog_cells = usize::try_from(fog_size).expect("positive checked fog size");
    for (plane, expected, actual) in [
        (TDataFogPlane::TData, tile_cells, world.tdata.len()),
        (TDataFogPlane::Seen, fog_cells, world.seen.len()),
        (TDataFogPlane::Seen2, fog_cells, world.seen2.len()),
        (TDataFogPlane::Seen3, fog_cells, world.seen3.len()),
        (
            TDataFogPlane::WCoordSeen,
            usize::try_from(size).expect("positive checked world size"),
            world.wcoord_seen.len(),
        ),
    ] {
        if expected != actual {
            return Err(TDataFogWipeError::PlaneLengthMismatch {
                plane,
                expected,
                actual,
            });
        }
    }

    let tdata_end = tile_cells
        .checked_mul(2)
        .expect("validated i32 tile size fits usize bytes");
    let seen_end = tdata_end + fog_cells;
    let seen2_end = seen_end + fog_cells;
    let seen3_end = seen2_end + fog_cells;
    Ok(TDataFogLayout {
        tdata: ByteRange {
            start: 0,
            end: tdata_end,
        },
        seen: ByteRange {
            start: tdata_end,
            end: seen_end,
        },
        seen2: ByteRange {
            start: seen_end,
            end: seen2_end,
        },
        seen3: ByteRange {
            start: seen2_end,
            end: seen3_end,
        },
    })
}

pub fn tdata_and_fog_image(world: &World) -> Vec<u8> {
    let mut sink = ByteSink::new();
    world.walk_section(&mut sink, WorldSection::TDataAndFog as i32);
    sink.0
}

pub fn wcoord_seen_image(world: &World) -> Vec<u8> {
    let mut sink = ByteSink::new();
    world.walk_section(&mut sink, WorldSection::WCoordSeen as i32);
    sink.0
}

fn changed_ranges(before: &[u8], after: &[u8]) -> Vec<ByteRange> {
    let mut ranges = Vec::new();
    let mut offset = 0usize;
    while offset < before.len() {
        if before[offset] == after[offset] {
            offset += 1;
            continue;
        }
        let start = offset;
        offset += 1;
        while offset < before.len() && before[offset] != after[offset] {
            offset += 1;
        }
        ranges.push(ByteRange { start, end: offset });
    }
    ranges
}

/// Execute and receipt the first section-6 producer at procedural-map entry.
///
/// The input is checked before mutation, and the call runs on a staged clone;
/// a shape or postcondition failure therefore leaves `world` unchanged.  No
/// recorded replay checksum is accepted as an input.
pub fn execute_tdata_and_fog_wipe(
    world: &mut World,
) -> Result<TDataFogWipeReceipt, TDataFogWipeError> {
    let layout = checked_shape(world)?;
    let before = tdata_and_fog_image(world);
    let wcoord_seen_before = wcoord_seen_image(world);
    if before.len() != layout.bytes() {
        return Err(TDataFogWipeError::SectionWalkLengthMismatch {
            expected: layout.bytes(),
            actual: before.len(),
        });
    }

    let nonzero_tdata_words_before = world.tdata.iter().filter(|&&word| word != 0).count();
    let nonzero_seen_bytes_before = world.seen.iter().filter(|&&byte| byte != 0).count();
    let nonzero_seen2_bytes_before = world.seen2.iter().filter(|&&byte| byte != 0).count();
    let nonzero_seen3_bytes_before = world.seen3.iter().filter(|&&byte| byte != 0).count();
    let nonzero_wcoord_seen_bytes_before =
        world.wcoord_seen.iter().filter(|&&byte| byte != 0).count();

    let mut staged = world.clone();
    staged.wipe();
    let after = tdata_and_fog_image(&staged);
    let wcoord_seen_after = wcoord_seen_image(&staged);
    if after.len() != layout.bytes() {
        return Err(TDataFogWipeError::SectionWalkLengthMismatch {
            expected: layout.bytes(),
            actual: after.len(),
        });
    }
    if let Some((offset, &value)) = after.iter().enumerate().find(|(_, value)| **value != 0) {
        return Err(TDataFogWipeError::NonzeroPostcondition { offset, value });
    }
    if let Some((offset, &value)) = wcoord_seen_after
        .iter()
        .enumerate()
        .find(|(_, value)| **value != 0)
    {
        return Err(TDataFogWipeError::NonzeroPostcondition {
            offset: layout.bytes() + offset,
            value,
        });
    }

    let tdata_changed_ranges = changed_ranges(&before, &after);
    let wcoord_seen_changed_ranges = changed_ranges(&wcoord_seen_before, &wcoord_seen_after);
    let section_bytes_changed = tdata_changed_ranges.iter().map(|range| range.len()).sum();
    let wcoord_seen_bytes_changed = wcoord_seen_changed_ranges
        .iter()
        .map(|range| range.len())
        .sum();
    let section_bytes_written = layout.bytes();
    let wcoord_seen_bytes_written = world.wcoord_seen.len();
    let total_bytes_written = section_bytes_written + wcoord_seen_bytes_written;
    let total_bytes_changed = section_bytes_changed + wcoord_seen_bytes_changed;
    let receipt = TDataFogWipeReceipt {
        entry_va: WORLD_WIPE_VA,
        resume_va: WORLD_WIPE_RETURN_VA,
        layout,
        written_ranges: [
            TDataFogWrittenRange {
                plane: TDataFogPlane::TData,
                range: layout.tdata,
                producer_va: TDATA_ZERO_LOOP_WRITE_VA,
            },
            TDataFogWrittenRange {
                plane: TDataFogPlane::Seen,
                range: layout.seen,
                producer_va: CLEAR_SEEN_CALL_VA,
            },
            TDataFogWrittenRange {
                plane: TDataFogPlane::Seen2,
                range: layout.seen2,
                producer_va: CLEAR_SEEN2_CALL_VA,
            },
            TDataFogWrittenRange {
                plane: TDataFogPlane::Seen3,
                range: layout.seen3,
                producer_va: CLEAR_SEEN3_MEMSET_VA,
            },
            TDataFogWrittenRange {
                plane: TDataFogPlane::WCoordSeen,
                range: ByteRange {
                    start: 0,
                    end: wcoord_seen_bytes_written,
                },
                producer_va: WORLD_CLEAR_WCOORD_SEEN_MEMSET_CALL_VA,
            },
        ],
        changed_ranges: tdata_changed_ranges,
        wcoord_seen_changed_ranges,
        tdata_cells_written: world.tdata.len(),
        fog_cells_written_per_plane: world.seen.len(),
        section_bytes_written,
        wcoord_seen_bytes_written,
        total_bytes_written,
        section_bytes_changed,
        wcoord_seen_bytes_changed,
        total_bytes_changed,
        rewritten_zero_bytes: total_bytes_written - total_bytes_changed,
        nonzero_tdata_words_before,
        nonzero_seen_bytes_before,
        nonzero_seen2_bytes_before,
        nonzero_seen3_bytes_before,
        nonzero_wcoord_seen_bytes_before,
        section_adler_before: adler32(1, &before),
        section_adler_after: adler32(1, &after),
        wcoord_seen_adler_before: adler32(1, &wcoord_seen_before),
        wcoord_seen_adler_after: adler32(1, &wcoord_seen_after),
        rng_draws: 0,
    };
    *world = staged;
    Ok(receipt)
}
