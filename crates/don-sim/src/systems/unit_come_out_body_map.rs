// SPDX-License-Identifier: GPL-3.0-or-later
//! `systems::unit_come_out_body_map` — the address-space accounting that ties the four
//! `unit_come_out_*_frontier` tranches to retail `Unit::come_out` `0x00617C10`.
//!
//! # Why this module exists
//!
//! `Unit::come_out(int)` is 9,925 bytes [`S_GPROC32` size, `ron-bin/sbl/rise.pdb`]. It was
//! recovered by four independent lanes, each of which wrote a source-only planner over one
//! address interval and each of which restated its own `ObjectIdentity` / `Point` /
//! `RngStamp` types. Nothing in the tree checked that the four intervals actually *tile* the
//! body, and three of the four modules had no `mod` declaration at all — they were compiled
//! only from their own test files, so the library crate did not contain them and no gameplay
//! path could reach them.
//!
//! This module is the join. It owns
//!
//! * the measured extent of every tranche, cross-read against each tranche module's own
//!   published constants, so a sibling that edits one of those constants breaks a test here
//!   rather than silently un-tiling the body;
//! * the complete map of the **compiler-outlined virtual-call islands** at
//!   `0x0061A206..0x0061A2D5`, which MSVC placed after the sequential body and which the
//!   first tranche never accounted for;
//! * [`ComeOutBoundary`], the single typed boundary every in-tree caller of `Unit::come_out`
//!   stops at today.
//!
//! # What this module does **not** claim
//!
//! Tiling the address space is an accounting claim, not a behavioural one. It says the four
//! planners between them read every byte of the retail body; it does not say their decisions
//! are right, and none of them has been differentially tested against retail. Fidelity tier
//! is **C** throughout. Nothing here is verified in the proof-assistant sense.

use crate::systems::unit_come_out_common_release_frontier as common_release;
use crate::systems::unit_come_out_full_frontier as full;
use crate::systems::unit_come_out_gather_selection_frontier as gather_selection;
use crate::systems::unit_come_out_release_tail_frontier as release_tail;

/// Canonical identity carried across the four independently recovered planner dialects.
///
/// The retail body keeps one `(who, o)` pair alive across every tranche.  Keeping the
/// conversions here makes an adapter prove that it did not narrow, re-owner, or otherwise
/// reinterpret that pair at a source-file boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CanonicalObjectIdentity {
    pub owner: i8,
    pub object: i16,
}

impl CanonicalObjectIdentity {
    pub const fn new(owner: i8, object: i16) -> Self {
        Self { owner, object }
    }

    pub const fn valid(self) -> bool {
        self.owner >= 0 && self.object >= 0
    }
}

/// Canonical world coordinate carried across the body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CanonicalPoint {
    pub x: i32,
    pub y: i32,
}

/// Canonical game-random image carried across the body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CanonicalRngStamp {
    pub seed: u32,
    pub draws: u64,
}

macro_rules! identity_dialect {
    ($module:ident) => {
        impl From<$module::ObjectIdentity> for CanonicalObjectIdentity {
            fn from(value: $module::ObjectIdentity) -> Self {
                Self::new(value.owner, value.object)
            }
        }

        impl From<CanonicalObjectIdentity> for $module::ObjectIdentity {
            fn from(value: CanonicalObjectIdentity) -> Self {
                Self::new(value.owner, value.object)
            }
        }
    };
}

macro_rules! point_dialect {
    ($module:ident) => {
        impl From<$module::Point> for CanonicalPoint {
            fn from(value: $module::Point) -> Self {
                Self {
                    x: value.x,
                    y: value.y,
                }
            }
        }

        impl From<CanonicalPoint> for $module::Point {
            fn from(value: CanonicalPoint) -> Self {
                Self {
                    x: value.x,
                    y: value.y,
                }
            }
        }
    };
}

macro_rules! rng_dialect {
    ($module:ident) => {
        impl From<$module::RngStamp> for CanonicalRngStamp {
            fn from(value: $module::RngStamp) -> Self {
                Self {
                    seed: value.seed,
                    draws: value.draws,
                }
            }
        }

        impl From<CanonicalRngStamp> for $module::RngStamp {
            fn from(value: CanonicalRngStamp) -> Self {
                Self {
                    seed: value.seed,
                    draws: value.draws,
                }
            }
        }
    };
}

identity_dialect!(full);
identity_dialect!(common_release);
identity_dialect!(gather_selection);
identity_dialect!(release_tail);
point_dialect!(full);
point_dialect!(common_release);
point_dialect!(gather_selection);
point_dialect!(release_tail);
rng_dialect!(full);
rng_dialect!(common_release);
rng_dialect!(gather_selection);
rng_dialect!(release_tail);

/// Lossless first-to-second-tranche join.
pub fn common_continuation(
    value: full::UnitComeOutContinuation,
) -> common_release::PrefixContinuation {
    common_release::PrefixContinuation {
        resume_va: value.resume_va,
        actor: CanonicalObjectIdentity::from(value.actor).into(),
        point: CanonicalPoint::from(value.point).into(),
        z: value.z,
        direct_container: value
            .direct_container
            .map(CanonicalObjectIdentity::from)
            .map(Into::into),
        placement_container: value
            .placement_container
            .map(CanonicalObjectIdentity::from)
            .map(Into::into),
        container_gpiece: value.container_gpiece,
        rng: CanonicalRngStamp::from(value.rng).into(),
    }
}

/// A common-release GatherList continuation converted without changing its actor, container,
/// scratch Group, point, or RNG image.  A missing direct container cannot enter the retail
/// gather loop and therefore fails closed.
pub fn gather_input(
    value: common_release::CommonReleaseContinuation,
) -> Option<gather_selection::GatherSelectionInput> {
    let container = value.prefix.direct_container?;
    Some(gather_selection::GatherSelectionInput {
        resume_va: value.resume_va,
        actor: CanonicalObjectIdentity::from(value.prefix.actor).into(),
        actor_point: CanonicalPoint::from(value.prefix.point).into(),
        direct_container: CanonicalObjectIdentity::from(container).into(),
        container_gpiece: value.prefix.container_gpiece,
        scratch_group: value.scratch_group,
        rng: CanonicalRngStamp::from(value.rng).into(),
    })
}

/// `Unit::come_out` `0x00617C10` [PDB `S_GPROC32`].
pub const UNIT_COME_OUT_VA: u32 = 0x0061_7c10;
/// `int Unit::come_out(int)` — 9,925 bytes [PDB `S_GPROC32` size].
pub const UNIT_COME_OUT_BYTES: u32 = 9_925;
/// One past the last byte of the body.
pub const UNIT_COME_OUT_END_VA: u32 = UNIT_COME_OUT_VA + UNIT_COME_OUT_BYTES;

/// Which recovered planner owns a given retail instruction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Tranche {
    /// `unit_come_out_full_frontier` — entry cleanup through contained placement/unlink.
    Prefix,
    /// `unit_come_out_common_release_frontier` — the common release prologue.
    CommonRelease,
    /// `unit_come_out_gather_selection_frontier` — the gather-point selection loop.
    GatherSelection,
    /// `unit_come_out_release_tail_frontier` — the order-installation dispatcher and tail.
    ReleaseTail,
}

impl Tranche {
    pub const ALL: [Tranche; 4] = [
        Tranche::Prefix,
        Tranche::CommonRelease,
        Tranche::GatherSelection,
        Tranche::ReleaseTail,
    ];

    pub const fn module_path(self) -> &'static str {
        match self {
            Tranche::Prefix => "systems::unit_come_out_full_frontier",
            Tranche::CommonRelease => "systems::unit_come_out_common_release_frontier",
            Tranche::GatherSelection => "systems::unit_come_out_gather_selection_frontier",
            Tranche::ReleaseTail => "systems::unit_come_out_release_tail_frontier",
        }
    }
}

/// One half-open `[start, end)` retail interval owned by one tranche.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Extent {
    pub start: u32,
    pub end: u32,
    pub owner: Tranche,
    /// `true` when the interval is one of the compiler-outlined islands past the sequential
    /// body rather than part of the straight-line flow.
    pub outlined: bool,
}

impl Extent {
    pub const fn bytes(self) -> u32 {
        self.end - self.start
    }

    pub const fn contains(self, va: u32) -> bool {
        self.start <= va && va < self.end
    }
}

/// The sequential intervals, in retail address order.
///
/// Boundaries are the ones each tranche module publishes:
/// `full::PREFIX_END_VA`, `common_release::GATHER_LIST_RESUME_VA`,
/// `gather_selection::GATHER_SELECTION_END_VA`, `release_tail::OUTLINED_REGION_START_VA`.
pub const SEQUENTIAL: [Extent; 4] = [
    Extent {
        start: UNIT_COME_OUT_VA,
        end: 0x0061_86b4,
        owner: Tranche::Prefix,
        outlined: false,
    },
    Extent {
        start: 0x0061_86b4,
        end: 0x0061_8b22,
        owner: Tranche::CommonRelease,
        outlined: false,
    },
    Extent {
        start: 0x0061_8b22,
        end: 0x0061_91a5,
        owner: Tranche::GatherSelection,
        outlined: false,
    },
    Extent {
        start: 0x0061_91a5,
        end: 0x0061_a206,
        owner: Tranche::ReleaseTail,
        outlined: false,
    },
];

/// Every compiler-outlined virtual-call island, in retail address order.
///
/// [measured, capstone PE32 disassembly of `ron-bin/riseofnations.exe`] the region
/// `0x0061A206..0x0061A2D5` is 19 islands of the shape
///
/// ```text
///   mov ecx, ebx      ; optional, only when the receiver is not already in ECX
///   call eax          ; the indirect call the fast path had devirtualised
///   jmp <resume>      ; back into the sequential body
/// ```
///
/// MSVC emits one per `if (slot == <known fn>) fast(); else slot();` site. Each island is
/// owned by whichever tranche owns its `jmp` target, **not** by whichever tranche the island
/// address falls in. Seven of them resume inside the first tranche and were never accounted
/// for by it; they are listed here as `Tranche::Prefix` and counted into the prefix total.
pub const OUTLINED: [(u32, u32, u32, Tranche); 19] = [
    (0x0061_a206, 0x0061_a20f, 0x0061_7cc7, Tranche::Prefix),
    (0x0061_a20f, 0x0061_a216, 0x0061_7e7b, Tranche::Prefix),
    // The only island that does not resume: it pops the frame and returns 1.
    (0x0061_a216, 0x0061_a226, 0x0061_a216, Tranche::Prefix),
    (0x0061_a226, 0x0061_a22f, 0x0061_802c, Tranche::Prefix),
    (0x0061_a22f, 0x0061_a236, 0x0061_80a5, Tranche::Prefix),
    (0x0061_a236, 0x0061_a23d, 0x0061_80d7, Tranche::Prefix),
    (0x0061_a23d, 0x0061_a24e, 0x0061_81dc, Tranche::Prefix),
    (0x0061_a24e, 0x0061_a25f, 0x0061_87f9, Tranche::CommonRelease),
    (0x0061_a25f, 0x0061_a268, 0x0061_885d, Tranche::CommonRelease),
    (0x0061_a268, 0x0061_a271, 0x0061_891d, Tranche::CommonRelease),
    (0x0061_a271, 0x0061_a278, 0x0061_8e18, Tranche::GatherSelection),
    (0x0061_a278, 0x0061_a281, 0x0061_9052, Tranche::GatherSelection),
    (0x0061_a281, 0x0061_a288, 0x0061_947b, Tranche::ReleaseTail),
    (0x0061_a288, 0x0061_a28f, 0x0061_95d3, Tranche::ReleaseTail),
    (0x0061_a28f, 0x0061_a298, 0x0061_962d, Tranche::ReleaseTail),
    (0x0061_a298, 0x0061_a29f, 0x0061_9774, Tranche::ReleaseTail),
    (0x0061_a29f, 0x0061_a2a8, 0x0061_9e17, Tranche::ReleaseTail),
    (0x0061_a2a8, 0x0061_a2b1, 0x0061_a0f0, Tranche::ReleaseTail),
    (0x0061_a2b1, 0x0061_a2ba, 0x0061_a137, Tranche::ReleaseTail),
];

/// The three islands past `OUTLINED`'s last entry, split out only because a 19-element and a
/// 3-element literal read better than one 22-element one. Same shape, same rules.
pub const OUTLINED_TAIL: [(u32, u32, u32, Tranche); 3] = [
    (0x0061_a2ba, 0x0061_a2c3, 0x0061_a15d, Tranche::ReleaseTail),
    (0x0061_a2c3, 0x0061_a2cc, 0x0061_a184, Tranche::ReleaseTail),
    (0x0061_a2cc, 0x0061_a2d5, 0x0061_a1aa, Tranche::ReleaseTail),
];

/// First byte of the outlined-island region.
pub const OUTLINED_REGION_START_VA: u32 = 0x0061_a206;

/// Total bytes owned by `tranche`, sequential plus its outlined islands.
pub fn tranche_bytes(tranche: Tranche) -> u32 {
    let sequential: u32 = SEQUENTIAL
        .iter()
        .filter(|e| e.owner == tranche)
        .map(|e| e.bytes())
        .sum();
    let outlined: u32 = OUTLINED
        .iter()
        .chain(OUTLINED_TAIL.iter())
        .filter(|(_, _, _, owner)| *owner == tranche)
        .map(|(start, end, _, _)| end - start)
        .sum();
    sequential + outlined
}

/// Which tranche owns `va`, or `None` when `va` is outside the body.
///
/// Outlined islands are attributed to the tranche that resumes them, so this is a
/// *behavioural* ownership query, not a "which interval is the address in" query.
pub fn owner_of(va: u32) -> Option<Tranche> {
    for (start, end, _, owner) in OUTLINED.iter().chain(OUTLINED_TAIL.iter()) {
        if *start <= va && va < *end {
            return Some(*owner);
        }
    }
    for extent in SEQUENTIAL.iter() {
        if extent.contains(va) {
            return Some(extent.owner);
        }
    }
    None
}

/// The single point at which the recovered family stops being able to run itself.
///
/// Every tranche is a *planner*: it decides, records the ordered host calls it needs, and
/// returns a plan. None of them mutates a world. A caller that wants the effect of
/// `Unit::come_out` must satisfy the plan against a host, and no such host exists in this
/// crate yet — `production_runtime`, `unit_inctime`, `gathering`, `leader_set_diplo` and
/// `step8_eject_contents` each stop here with their own local stub or receipt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ComeOutBoundary {
    /// `Unit::come_out(mode)` has a complete decision transcription across four tranches but
    /// no host in this crate can apply the resulting plan.
    NoHost {
        /// Always [`UNIT_COME_OUT_VA`].
        retail_va: u32,
        /// The `int` argument. Retail's two live values are `0` (ordinary release) and `1`.
        mode: i32,
    },
}

impl ComeOutBoundary {
    pub const fn no_host(mode: i32) -> Self {
        ComeOutBoundary::NoHost {
            retail_va: UNIT_COME_OUT_VA,
            mode,
        }
    }
}

/// Errors [`check_body_map`] can report. Each one is a real un-tiling of the retail body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BodyMapError {
    /// A sequential extent does not start where the previous one ended.
    SequentialGap { after: u32, next: u32 },
    /// The sequential extents do not start at the function entry.
    WrongEntry { got: u32 },
    /// An outlined island overlaps its neighbour or leaves a hole.
    OutlinedGap { after: u32, next: u32 },
    /// The outlined region does not begin where the sequential body ends.
    OutlinedRegionMisplaced { sequential_end: u32, region: u32 },
    /// The outlined region does not end at the last byte of the body.
    OutlinedRegionShort { got: u32, want: u32 },
    /// An island resumes outside the body.
    ResumeOutOfBody { island: u32, resume: u32 },
    /// The tranche totals do not sum to the PDB body size.
    TotalMismatch { got: u32, want: u32 },
    /// A tranche module's own published extent disagrees with this map.
    TrancheDisagrees {
        tranche: Tranche,
        map_says: u32,
        module_says: u32,
    },
}

/// Verify the whole map: contiguity, coverage, and agreement with each tranche module's own
/// published constants.
///
/// This is the load-bearing function of the module. It is deliberately written against the
/// sibling modules' constants rather than against copies, so that a lane which retunes, say,
/// `common_release::GATHER_LIST_RESUME_VA` gets a red test here instead of a body that
/// quietly stops tiling.
pub fn check_body_map() -> Result<(), BodyMapError> {
    if SEQUENTIAL[0].start != UNIT_COME_OUT_VA {
        return Err(BodyMapError::WrongEntry {
            got: SEQUENTIAL[0].start,
        });
    }
    for pair in SEQUENTIAL.windows(2) {
        if pair[0].end != pair[1].start {
            return Err(BodyMapError::SequentialGap {
                after: pair[0].end,
                next: pair[1].start,
            });
        }
    }
    let sequential_end = SEQUENTIAL[SEQUENTIAL.len() - 1].end;
    if sequential_end != OUTLINED_REGION_START_VA {
        return Err(BodyMapError::OutlinedRegionMisplaced {
            sequential_end,
            region: OUTLINED_REGION_START_VA,
        });
    }

    let islands: Vec<(u32, u32, u32, Tranche)> = OUTLINED
        .iter()
        .chain(OUTLINED_TAIL.iter())
        .copied()
        .collect();
    if islands[0].0 != OUTLINED_REGION_START_VA {
        return Err(BodyMapError::OutlinedRegionMisplaced {
            sequential_end,
            region: islands[0].0,
        });
    }
    for pair in islands.windows(2) {
        if pair[0].1 != pair[1].0 {
            return Err(BodyMapError::OutlinedGap {
                after: pair[0].1,
                next: pair[1].0,
            });
        }
    }
    let last = islands[islands.len() - 1].1;
    if last != UNIT_COME_OUT_END_VA {
        return Err(BodyMapError::OutlinedRegionShort {
            got: last,
            want: UNIT_COME_OUT_END_VA,
        });
    }
    for (start, _, resume, _) in islands.iter() {
        if *resume < UNIT_COME_OUT_VA || *resume >= UNIT_COME_OUT_END_VA {
            return Err(BodyMapError::ResumeOutOfBody {
                island: *start,
                resume: *resume,
            });
        }
    }

    let total: u32 = Tranche::ALL.iter().copied().map(tranche_bytes).sum();
    if total != UNIT_COME_OUT_BYTES {
        return Err(BodyMapError::TotalMismatch {
            got: total,
            want: UNIT_COME_OUT_BYTES,
        });
    }

    // Cross-read against each tranche module's own boundary constants.
    let claims = [
        (Tranche::Prefix, full::PREFIX_END_VA, SEQUENTIAL[0].end),
        (
            Tranche::CommonRelease,
            common_release::GATHER_LIST_RESUME_VA,
            SEQUENTIAL[1].end,
        ),
        (
            Tranche::GatherSelection,
            gather_selection::GATHER_SELECTION_END_VA,
            SEQUENTIAL[2].end,
        ),
        (
            Tranche::ReleaseTail,
            release_tail::OUTLINED_REGION_START_VA,
            SEQUENTIAL[3].end,
        ),
    ];
    for (tranche, module_says, map_says) in claims {
        if module_says != map_says {
            return Err(BodyMapError::TrancheDisagrees {
                tranche,
                map_says,
                module_says,
            });
        }
    }
    if common_release::COMMON_RELEASE_START_VA != SEQUENTIAL[1].start {
        return Err(BodyMapError::TrancheDisagrees {
            tranche: Tranche::CommonRelease,
            map_says: SEQUENTIAL[1].start,
            module_says: common_release::COMMON_RELEASE_START_VA,
        });
    }
    if gather_selection::GATHER_SELECTION_START_VA != SEQUENTIAL[2].start {
        return Err(BodyMapError::TrancheDisagrees {
            tranche: Tranche::GatherSelection,
            map_says: SEQUENTIAL[2].start,
            module_says: gather_selection::GATHER_SELECTION_START_VA,
        });
    }
    if release_tail::RELEASE_TAIL_START_VA != SEQUENTIAL[3].start {
        return Err(BodyMapError::TrancheDisagrees {
            tranche: Tranche::ReleaseTail,
            map_says: SEQUENTIAL[3].start,
            module_says: release_tail::RELEASE_TAIL_START_VA,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_four_tranches_tile_the_retail_body_exactly() {
        assert_eq!(check_body_map(), Ok(()));
    }

    #[test]
    fn every_byte_of_the_body_has_exactly_one_owner() {
        // Walk the whole 9,925-byte body one byte at a time. This is the assertion the
        // arithmetic totals cannot make on their own: a pair of extents could overlap by
        // exactly as much as another pair gaps and still sum correctly.
        let mut unowned = Vec::new();
        for va in UNIT_COME_OUT_VA..UNIT_COME_OUT_END_VA {
            if owner_of(va).is_none() {
                unowned.push(va);
            }
        }
        assert!(unowned.is_empty(), "unowned retail bytes: {unowned:x?}");

        let mut seen = 0u32;
        for extent in SEQUENTIAL.iter() {
            seen += extent.bytes();
        }
        for (start, end, _, _) in OUTLINED.iter().chain(OUTLINED_TAIL.iter()) {
            seen += end - start;
        }
        assert_eq!(seen, UNIT_COME_OUT_BYTES);
    }

    #[test]
    fn the_measured_tranche_sizes_are_the_ones_the_lanes_reported() {
        // 2,724 sequential + 72 bytes of outlined islands the first lane never counted.
        assert_eq!(tranche_bytes(Tranche::Prefix), 2_724 + 72);
        assert_eq!(
            tranche_bytes(Tranche::Prefix),
            full::PREFIX_BYTES + release_tail::REATTRIBUTED_PREFIX_ISLAND_BYTES
        );
        // The common-release lane published 1,134 + 35.
        assert_eq!(tranche_bytes(Tranche::CommonRelease), 1_169);
        assert_eq!(
            tranche_bytes(Tranche::CommonRelease),
            common_release::LOGICAL_TRANCHE_BYTES
        );
        // The gather-selection lane published 1,667 + 16.
        assert_eq!(tranche_bytes(Tranche::GatherSelection), 1_683);
        assert_eq!(
            tranche_bytes(Tranche::GatherSelection),
            gather_selection::LOGICAL_TRANCHE_BYTES
        );
        // The fourth tranche's own behaviour is 4,277 bytes. It plus the 72 island bytes
        // reattributed to the prefix are exactly the 4,349-byte residual the
        // gather-selection lane reported, which is the arithmetic that says the body is now
        // closed.
        assert_eq!(tranche_bytes(Tranche::ReleaseTail), 4_277);
        assert_eq!(
            tranche_bytes(Tranche::ReleaseTail),
            release_tail::LOGICAL_TRANCHE_BYTES
        );
        assert_eq!(
            tranche_bytes(Tranche::ReleaseTail) + release_tail::REATTRIBUTED_PREFIX_ISLAND_BYTES,
            gather_selection::RESIDUAL_BYTES_AFTER_TRANCHE
        );
        assert_eq!(release_tail::RESIDUAL_BYTES_AFTER_TRANCHE, 0);
    }

    #[test]
    fn outlined_islands_are_owned_by_their_resume_target_not_their_address() {
        // 0x0061A24E sits in the fourth tranche's address neighbourhood but resumes at
        // 0x006187F9, inside the common-release prologue. Ownership follows the resume.
        assert_eq!(owner_of(0x0061_a24e), Some(Tranche::CommonRelease));
        assert_eq!(owner_of(0x0061_a271), Some(Tranche::GatherSelection));
        assert_eq!(owner_of(0x0061_a206), Some(Tranche::Prefix));
        assert_eq!(owner_of(0x0061_a281), Some(Tranche::ReleaseTail));
        for (start, _, resume, owner) in OUTLINED.iter().chain(OUTLINED_TAIL.iter()) {
            // The single non-resuming island points at itself by construction.
            if resume == start {
                continue;
            }
            assert_eq!(
                owner_of(*resume),
                Some(*owner),
                "island {start:#010x} resumes at {resume:#010x} outside its owner"
            );
        }
    }

    #[test]
    fn addresses_outside_the_body_have_no_owner() {
        assert_eq!(owner_of(UNIT_COME_OUT_VA - 1), None);
        assert_eq!(owner_of(UNIT_COME_OUT_END_VA), None);
    }

    #[test]
    fn the_boundary_names_the_retail_va() {
        assert_eq!(
            ComeOutBoundary::no_host(0),
            ComeOutBoundary::NoHost {
                retail_va: 0x0061_7c10,
                mode: 0
            }
        );
    }
}
