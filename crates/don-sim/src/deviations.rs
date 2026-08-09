//! The fidelity / improved mode split, and the registry of every deliberate deviation.
//!
//! # Why this module exists
//!
//! *Descent of Nations* has two jobs that pull in opposite directions. It is a
//! **reimplementation** whose progress is measured by how many turns of a real retail
//! replay its checksums survive — which requires reproducing retail exactly, bugs
//! included. And it is an **edition of the game people will play**, which requires being
//! allowed to fix what is broken.
//!
//! Those coexist only if every divergence is a *decision with a name*. Undeclared drift
//! is what turns a fidelity claim into a lie, and it is invisible from the inside: the
//! code compiles, the local tests pass, and the replay scoreboard just quietly stops
//! climbing without saying why.
//!
//! So: [`Mode::Fidelity`] is the default, and in fidelity mode
//! [`ModeConfig::is_active`] returns `false` for **every** registry entry, unconditionally
//! and by construction. [`Mode::Improved`] may switch entries on individually. Every entry
//! carries what retail does, what we do instead, why, and the address the retail behaviour
//! was read from.
//!
//! # What is *not* in here
//!
//! A deviation is a place where we know retail's behaviour and choose a different one.
//! It is not:
//!
//! * **A gap.** Unported mechanics are counted by `docs/mechanics/COVERAGE.md`; a missing
//!   `do_job` arm is absence, not divergence.
//! * **A rules change.** Retuning `rules.xml` is data, and data is already
//!   swappable — see the "Balance, and why it is not here" section of
//!   `docs/tracks/deviations.md`.
//! * **A performance choice that produces identical bits.** SIMD kernels are asserted
//!   bit-identical to their scalar references; that is not a deviation.
//!
//! [`Kind::Drift`] entries are the exception that keeps the register honest: places where
//! our code already differs from retail *without* anyone having decided to. They are
//! recorded so the divergence cannot hide, they are not toggleable, and they block every
//! shipped execution surface they can reach — which is exactly why they are debt.
//! Research-only models are labelled separately: they may stay useful while incomplete,
//! but their output can never be promoted to a playable or fidelity result.
//!
//! # Using it
//!
//! ```
//! use don_sim::deviations::{behaviour, Deviation, Mode, ModeConfig};
//!
//! // The default is fidelity, and fidelity cannot carry an active deviation.
//! let retail = ModeConfig::default();
//! assert_eq!(retail.mode(), Mode::Fidelity);
//! assert!(!retail.is_active(Deviation::AiGatherHandicap));
//! assert_eq!(behaviour::gather_handicap_pct(&retail, 0), -35);
//!
//! // Improved mode turns on the entries that default on, individually revocable.
//! let mut ours = ModeConfig::improved();
//! assert_eq!(behaviour::gather_handicap_pct(&ours, 0), 0);
//! ours.disable(Deviation::AiGatherHandicap);
//! assert_eq!(behaviour::gather_handicap_pct(&ours, 0), -35);
//! ```
//!
//! The human-readable changelog is `docs/tracks/deviations.md`; the design notes and the
//! measurements behind the seeded entries are in `docs/tracks/dual-mode.md`. The binary
//! `don-deviations` prints this registry and is what `tools/replay-validate.sh` runs to
//! prove its numbers were produced in fidelity mode.

use std::fmt;

// =======================================================================================
// Mode
// =======================================================================================

/// Which edition of the simulation is running.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Mode {
    /// Reproduce retail, including its bugs. **The default**, and the only mode under
    /// which a replay-validation or oracle number may be reported.
    #[default]
    Fidelity,
    /// Our edition. Entries in the registry may be switched on individually.
    Improved,
}

impl Mode {
    /// The name used on the command line and in `DON_MODE`.
    pub const fn slug(self) -> &'static str {
        match self {
            Mode::Fidelity => "fidelity",
            Mode::Improved => "improved",
        }
    }

    /// Parse a mode name. Case-insensitive; `retail` is accepted for `fidelity`.
    pub fn from_slug(s: &str) -> Result<Mode, ModeError> {
        match s.trim().to_ascii_lowercase().as_str() {
            "fidelity" | "retail" => Ok(Mode::Fidelity),
            "improved" => Ok(Mode::Improved),
            other => Err(ModeError::UnknownMode(other.to_string())),
        }
    }
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.slug())
    }
}

// =======================================================================================
// The registry
// =======================================================================================

/// What sort of registry entry this is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    /// A deliberate improvement. Toggleable, and only ever active in [`Mode::Improved`].
    Fix,
    /// A divergence that already exists in our code and that **nobody chose**. Not
    /// toggleable, active in both modes, and a live contaminant of any fidelity claim
    /// that touches it. Recorded here so it cannot hide; the fix is to remove it.
    Drift,
    /// A candidate that was investigated and turned out **not** to be a deviation.
    /// Recorded so it is not resurrected. Never active.
    Rejected,
}

/// A claim-bearing way of executing the project.
///
/// This is deliberately about *entrypoint reachability*, not global repository coverage.
/// An unfinished campaign importer must not block replay validation if the validator never
/// calls it. Conversely, a known approximation on the playable path must not hide behind a
/// green unit-test suite.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Surface {
    /// Replay/oracle comparison against the retail executable. Requires fidelity mode.
    ReplayValidation,
    /// The game a person can launch and play.
    PlayableEdition,
    /// The reinforcement-learning environment exposed to agents.
    RlEnvironment,
    /// Aggregate release gate over every non-research product surface.
    ProductRelease,
}

impl Surface {
    pub const ALL: [Surface; 4] = [
        Surface::ReplayValidation,
        Surface::PlayableEdition,
        Surface::RlEnvironment,
        Surface::ProductRelease,
    ];

    pub const fn slug(self) -> &'static str {
        match self {
            Surface::ReplayValidation => "replay",
            Surface::PlayableEdition => "playable",
            Surface::RlEnvironment => "rl-env",
            Surface::ProductRelease => "product",
        }
    }

    pub fn from_slug(s: &str) -> Result<Surface, ModeError> {
        match s.trim().to_ascii_lowercase().as_str() {
            "replay" | "validation" | "oracle" => Ok(Surface::ReplayValidation),
            "playable" | "game" => Ok(Surface::PlayableEdition),
            "rl-env" | "env" | "rl" => Ok(Surface::RlEnvironment),
            "product" | "release" => Ok(Surface::ProductRelease),
            other => Err(ModeError::UnknownSurface(other.to_string())),
        }
    }

    const fn requires_fidelity(self) -> bool {
        matches!(self, Surface::ReplayValidation)
    }
}

impl fmt::Display for Surface {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.slug())
    }
}

/// Whether an entry's implementation is admissible on the surfaces it can reach.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ImplementationStatus {
    /// A `Fix` is called by its real subsystem through the named seam.
    Wired,
    /// A `Fix` exists in the registry but its subsystem does not call it yet.
    Unwired,
    /// A known approximation executes on one or more product surfaces.
    KnownDrift,
    /// An intentionally bounded research model; never a product or fidelity claim.
    ResearchOnly,
    /// A rejected candidate has no runtime implementation.
    NotApplicable,
}

impl Kind {
    /// Whether entries of this kind can be switched on at all.
    pub const fn toggleable(self) -> bool {
        matches!(self, Kind::Fix)
    }

    pub const fn slug(self) -> &'static str {
        match self {
            Kind::Fix => "fix",
            Kind::Drift => "drift",
            Kind::Rejected => "rejected",
        }
    }
}

/// Every catalogued deviation. The discriminant is the registry index.
///
/// Adding a variant means adding an [`Entry`] at the same index in [`REGISTRY`]; the
/// tests in this module and in `tests/fidelity_mode.rs` check that correspondence.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(usize)]
pub enum Deviation {
    // -- Fix -----------------------------------------------------------------------------
    /// The AI difficulty income cheat.
    AiGatherHandicap = 0,
    /// The income cheat's C truncation, which makes low difficulties harsher than nominal.
    GatherHandicapTruncation = 1,
    /// `economic.bhs` testing `== 0` where the engine returns `-1`.
    BhsPrereqResultTest = 2,
    /// The shipped `"Citizens"` unit-type name, which does not exist.
    BhsCitizensTypo = 3,
    /// Tikal's border bonus reading `TIKAL_TEMPLE_HP`.
    TikalBorderRuleSlot = 4,
    /// The unqueue refund charging the player.
    RefundChargesPlayer = 5,
    /// The unqueue refund rewriting its own record, so repeats compound.
    RefundRepeatCompounding = 6,
    /// The caravan A\* mode-A heuristic passing `-goalY`.
    CaravanHeuristicGoalY = 7,
    /// `CityData::refinery` stored as a literal zero, making `REFINERY_BONUS` dead.
    RefineryBonusDead = 8,

    // -- Drift ---------------------------------------------------------------------------
    /// `don-env` has no authoritative `Unit::do_air_physics` host.
    EnvAirPatrolPhysics = 9,
    /// `don-env` has no authoritative patrol air/bomber target-search host.
    EnvAirPatrolUnitTargetSearch = 10,
    /// `don-env` has no authoritative patrol building target-search host.
    EnvAirPatrolBuildingTargetSearch = 11,
    /// `don-ai`'s six numbered model simplifications.
    AiModelSimplifications = 12,
    /// Arena MODEL 2a: no persistent retail construction schedule/identity host.
    ArenaConstructionScheduleModel = 13,
    /// Arena MODEL 2b: no authoritative construction placement transaction.
    ArenaConstructionPlacementModel = 14,
    /// Arena MODEL 2c: no authoritative construction lifecycle transaction.
    ArenaConstructionLifecycleModel = 15,
    /// Arena MODEL 2d: no authoritative construction interruption transaction.
    ArenaConstructionInterruptionModel = 16,
    /// Arena MODEL 3a: no authoritative gathering-capacity evaluator.
    ArenaGatherCapacityModel = 17,
    /// Arena MODEL 3b: no persistent retail gathering-occupancy host.
    ArenaGatherOccupancyModel = 18,
    /// Arena MODEL 3c: no authoritative gathering-terrain reservation lifecycle.
    ArenaGatherReservationModel = 19,
    /// Arena MODEL 3d: no authoritative gathering-payout transaction.
    ArenaGatherPayoutModel = 20,
    /// Arena MODEL 4: incomplete retail target-acquisition host.
    ArenaTargetAcquisitionModel = 21,
    /// Arena roster prerequisite: graphics-turret Guys are not materialized.
    ArenaGuyTurretModel = 22,
    /// Arena MODEL 6a: no retail water terrain/pathing host.
    ArenaWaterModel = 23,
    /// Arena MODEL 6b: no retail naval object/order/production host.
    ArenaNavalModel = 24,
    /// Arena MODEL 6c: no complete retail airframe host.
    ArenaAirModel = 25,
    /// Arena MODEL 6d: no complete retail diplomacy state/side-effect host.
    ArenaDiplomacyModel = 26,
    /// Arena MODEL 6e: no wired retail attrition state/damage host.
    ArenaAttritionModel = 27,
    /// Arena MODEL 6f: no wired retail supply-query/state host.
    ArenaSupplyModel = 28,

    // -- Rejected ------------------------------------------------------------------------
    /// "`attack_dir` does not mean what its name says" — it does.
    AttackDirSemantics = 29,
    /// "The gather-enhancer tables are off by one" — they are 1-based by design.
    GatherEnhancerTableBase = 30,
}

/// One registry entry: the whole justification for a divergence, in one place.
#[derive(Clone, Copy, Debug)]
pub struct Entry {
    /// The identity.
    pub id: Deviation,
    /// Stable kebab-case name, used on the command line and in `DON_DEVIATIONS`.
    pub slug: &'static str,
    /// One line, human-facing.
    pub title: &'static str,
    /// [`Kind::Fix`] / [`Kind::Drift`] / [`Kind::Rejected`].
    pub kind: Kind,
    /// What the retail game does.
    pub retail: &'static str,
    /// What we do instead when this entry is active. For [`Kind::Drift`], what we do
    /// today in *both* modes. For [`Kind::Rejected`], why there is nothing to do.
    pub ours: &'static str,
    /// Why the change is worth making.
    pub why: &'static str,
    /// The addresses, shipped-data lines, or source sites the retail behaviour was read
    /// from. Never empty — a deviation without a derivation is folklore.
    pub derived_from: &'static [&'static str],
    /// Where the measurement lives, and at what confidence.
    pub evidence: &'static str,
    /// Whether [`ModeConfig::improved`] switches it on. `false` means "available, but we
    /// do not ship it on" — normally because the blast radius is not yet measured.
    pub default_in_improved: bool,
    /// Whether activating it can change a checksum channel, i.e. whether it would make a
    /// replay diverge. Everything true here is also a reason fidelity mode exists.
    pub affects_checksum: bool,
    /// The Rust seam that consults this entry, if one exists yet.
    pub seam: &'static str,
    /// Product surfaces whose execution can reach this behaviour. An empty slice is only
    /// valid for [`ImplementationStatus::ResearchOnly`] and rejected candidates.
    pub surfaces: &'static [Surface],
    /// Whether the real runtime path is admissible on those surfaces.
    pub implementation: ImplementationStatus,
}

impl Entry {
    /// Whether this behaviour can execute on `surface`.
    pub fn reaches(&self, surface: Surface) -> bool {
        match surface {
            Surface::ProductRelease => {
                !self.surfaces.is_empty()
                    && self.implementation != ImplementationStatus::ResearchOnly
            }
            _ => self.surfaces.contains(&surface),
        }
    }
}

/// A concrete reason an execution surface is not ready for a claim or release.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadinessBlocker {
    /// Improved mode selected a documented fix whose real subsystem does not call it.
    UnwiredImprovement(Deviation),
    /// A knowingly non-retail approximation executes on this surface.
    KnownDrift(Deviation),
}

impl ReadinessBlocker {
    pub const fn deviation(self) -> Deviation {
        match self {
            ReadinessBlocker::UnwiredImprovement(d) | ReadinessBlocker::KnownDrift(d) => d,
        }
    }
}

impl fmt::Display for ReadinessBlocker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReadinessBlocker::UnwiredImprovement(d) => {
                write!(f, "active improvement `{d}` is registered but not wired")
            }
            ReadinessBlocker::KnownDrift(d) => {
                write!(f, "known approximation `{d}` executes on this surface")
            }
        }
    }
}

/// The registry. Index == `Deviation as usize`.
pub static REGISTRY: [Entry; Deviation::COUNT] = [
    // ---------------------------------------------------------------------------------
    Entry {
        id: Deviation::AiGatherHandicap,
        slug: "ai-gather-handicap",
        title: "The AI difficulty setting is an income cheat, not a smarter opponent",
        kind: Kind::Fix,
        retail: "`LeaderData::get_gather_handicap` returns a signed percentage by \
                 difficulty — Easiest -35, Easy -15, Moderate -7, Tough 0, Tougher +25, \
                 Toughest +50 — and `Leader::do_gather` applies it to every resource \
                 every frame as `income = (100 + pct) * income / 100`, after the \
                 displayed gather rate has already been cached, so it never appears in \
                 the AI's own economy readout.",
        ours: "The handicap is 0 at every difficulty. Difficulty is expressed by how well \
               the opponent plays, not by how much it is given.",
        why: "Measured end to end on six identical starting towns with no AI running at \
              all, so the only difference between rows was this function: Toughest \
              gathers 1.49x what Tough gathers and Easiest 0.65x, a 2.29x spread across \
              the ladder. A player who beats Toughest has beaten a richer opponent, not \
              a better one, and an RL agent trained against it learns to beat an economy \
              bonus. This is the single change that most affects whether the game is \
              worth playing on hard.",
        derived_from: &[
            "LeaderData::get_gather_handicap 0x006D66A0",
            "Leader::do_gather 0x006CE450",
            "display-before-handicap ordering: 0x006CE723 precedes 0x006CE72C",
        ],
        evidence: "docs/tracks/ron-ai-impl.md §2 (income probe, six difficulties); \
                   docs/mechanics/economy.md §4.5. Table is Tier C (decompiled, not yet \
                   oracle-checked); the 2.29x spread is [measured] in our own runner.",
        default_in_improved: true,
        affects_checksum: true,
        seam: "don_sim::deviations::behaviour::select_gather_handicap",
        surfaces: &[Surface::PlayableEdition, Surface::RlEnvironment],
        implementation: ImplementationStatus::Wired,
    },
    // ---------------------------------------------------------------------------------
    Entry {
        id: Deviation::GatherHandicapTruncation,
        slug: "gather-handicap-truncation",
        title: "The income handicap truncates, so penalties are harsher than they read",
        kind: Kind::Fix,
        retail: "`income = (100 + pct) * income / 100` is an integer divide truncating \
                 toward zero. At negative `pct` the discarded fraction is always lost, so \
                 the effective penalty is strictly worse than the table says; at positive \
                 `pct` the bonus is merely rounded down.",
        ours: "Round to nearest instead of always toward zero.",
        why: "On the measured timber column, whose per-frame income is a single-digit \
              number, Easiest lands on 0.60x where the table promises 0.65x and Easy on \
              0.80x against 0.85x. The bias is largest when income is smallest, i.e. in \
              the opening, i.e. exactly when a low difficulty is supposed to be gentle. \
              This entry is inert while `ai-gather-handicap` is active (a 0% handicap \
              divides exactly); it exists for anyone who wants the handicap ladder back \
              without its arithmetic bias.",
        derived_from: &["Leader::do_gather 0x006CE450"],
        evidence: "docs/tracks/ron-ai-impl.md §2, ratio columns; docs/mechanics/economy.md \
                   §4.5. [measured] in our runner — it fell out of running the derived \
                   code, which is why hand-computing an expectation would have hidden it.",
        default_in_improved: true,
        affects_checksum: true,
        seam: "don_sim::deviations::behaviour::apply_gather_handicap_pct",
        surfaces: &[Surface::PlayableEdition, Surface::RlEnvironment],
        implementation: ImplementationStatus::Wired,
    },
    // ---------------------------------------------------------------------------------
    Entry {
        id: Deviation::BhsPrereqResultTest,
        slug: "bhs-prereq-result-test",
        title: "economic.bhs step 20 spins forever on an unmet prerequisite",
        kind: Kind::Fix,
        retail: "`place_building_with_cost` returns the engine tri-state: >0 placed, 0 \
                 refused-but-affordable, -1 invalid (which is what an unmet PREQ \
                 returns). `economic.bhs` step 20 tests `== 0` before giving up, so on -1 \
                 it falls through to `old_step = 0` and retries the same illegal \
                 placement forever — and `old_step = 0` is precisely the signal that \
                 stops the 300-second hang watchdog from arming.",
        ours: "Treat any result `<= 0` as a failed placement, so the script yields the \
               cycle exactly as it does for a refusal.",
        why: "Observed generating 657 invalid orders for the Greeks in a single 30-minute \
              match: step 20 places a University whose PREQ0 is Classical Age, before \
              holding it. In retail this is survivable only because of the every-30-ticks \
              age-tech fast lane in `Leader::plan_strategy`, which sits outside the \
              production cycle; anything that stubs that lane leaves the AI permanently \
              stuck. A shipped script bug that is still there 23 years later, and one an \
              opponent worth playing cannot have.",
        derived_from: &[
            "ron-data/ai-scripts/economic.bhs:638 and :642 (`... == 0`)",
            "Leader::production_ai script tri-state, see crates/don-ai/src/orders.rs",
        ],
        evidence: "docs/tracks/ron-ai-impl.md §4.3, docs/tracks/bhs-what-we-know.md. \
                   [measured] in the don-ai match runner: 657 invalid orders, one call site.",
        default_in_improved: true,
        affects_checksum: true,
        seam: "don_sim::deviations::behaviour::bhs_order_failed",
        surfaces: &[Surface::PlayableEdition, Surface::RlEnvironment],
        implementation: ImplementationStatus::Wired,
    },
    // ---------------------------------------------------------------------------------
    Entry {
        id: Deviation::BhsCitizensTypo,
        slug: "bhs-citizens-typo",
        title: "economic.bhs trains a unit type that does not exist",
        kind: Kind::Fix,
        retail: "Five `train_unit_with_need(who, needed_citizens, \"Citizens\")` calls \
                 name a type that is absent, case-sensitively, from the shipped data \
                 files — the type is `Citizen`. The maintenance call at economic.bhs:297 \
                 uses the correct singular with the *same* `needed_citizens`, so the \
                 next script invocation partly cleans up after the failed one.",
        ours: "Resolve the shipped `\"Citizens\"` to `\"Citizen\"` at the type lookup.",
        why: "The five worker-training steps of the shipped opening silently do nothing. \
              Partly masked is not fixed: the opening trains its citizens a cycle late, \
              every time.",
        derived_from: &[
            "ron-data/ai-scripts/economic.bhs:475, :510, :568, :674, :762",
            "ron-data/unitrules.xml (`Citizen`; note `Fishermen` really is plural)",
        ],
        evidence: "docs/tracks/AUDIT-frontier.md V17 — verified case-sensitively absent \
                   from both shipped data files. [measured].",
        default_in_improved: true,
        affects_checksum: true,
        seam: "don_sim::deviations::behaviour::bhs_unit_type_name",
        surfaces: &[Surface::PlayableEdition, Surface::RlEnvironment],
        implementation: ImplementationStatus::Wired,
    },
    // ---------------------------------------------------------------------------------
    Entry {
        id: Deviation::TikalBorderRuleSlot,
        slug: "tikal-border-rule-slot",
        title: "Tikal's border bonus reads the wrong rules constant",
        kind: Kind::Fix,
        retail: "Under `has_wonder(0x214)` the border code loads `[ecx + 0x4a0]`, which \
                 XML declaration order puts at `TIKAL_TEMPLE_HP`. `TIKAL_TEMPLE_BORDERS` \
                 is at `+0x498` and is never read by the border path.",
        ours: "Read `TIKAL_TEMPLE_BORDERS` for the border bonus.",
        why: "Both constants ship as `50%`, so no shipped number moves and no retail \
              replay can tell the difference — but a modder who edits \
              `TIKAL_TEMPLE_BORDERS` sees no effect, and one who edits \
              `TIKAL_TEMPLE_HP` silently changes borders. The Workshop mod library is a \
              stated goal; a rules file whose names lie to you is a bad foundation for it.",
        derived_from: &[
            "border scoring at 0x006B0DC9 (`mov ecx, [ecx + 0x4a0]`)",
            "rule offsets: TIKAL_TEMPLE_BORDERS +0x498, TIKAL_TEMPLE_RANGE +0x49C, \
             TIKAL_TEMPLE_HP +0x4A0",
        ],
        evidence: "docs/mechanics/borders-fog.md §4.2(ii). [measured] at the instruction \
                   level. Checksum-neutral on shipped data because both constants are 50.",
        default_in_improved: true,
        affects_checksum: true,
        seam: "don_sim::deviations::behaviour::tikal_border_percent",
        surfaces: &[Surface::PlayableEdition, Surface::RlEnvironment],
        implementation: ImplementationStatus::Wired,
    },
    // ---------------------------------------------------------------------------------
    Entry {
        id: Deviation::RefundChargesPlayer,
        slug: "refund-charges-player",
        title: "Cancelling a queued item can take resources away from you",
        kind: Kind::Fix,
        retail: "`Build::refund_cost` computes an age-adjusted `adj` and credits \
                 `stockpile += amt - adj`. With `TECH_SCIENCE_DISCOUNT = 10` and zero \
                 ages elapsed, `adj` for `amt = 100` is 110, so the credit is **-10**: \
                 cancelling costs you even at the same age, and more as ages pass.",
        ours: "Clamp the credit at zero. Cancelling can refund nothing; it can never bill \
               you.",
        why: "A negative refund is not a difficulty knob, it is a trap — invisible in the \
              UI, triggered by a routine action, and worse the longer the game runs. \
              Nothing about the game is more improved by leaving it in.",
        derived_from: &["Build::refund_cost 0x00620490 (build.cpp:6849)"],
        evidence: "docs/mechanics/production.md §3.6; the retail arithmetic is reproduced \
                   literally in crates/don-sim/src/systems/production.rs::refund_amount. \
                   [measured].",
        default_in_improved: true,
        affects_checksum: true,
        seam: "don_sim::deviations::behaviour::refund_slot",
        surfaces: &[Surface::PlayableEdition, Surface::RlEnvironment],
        implementation: ImplementationStatus::Wired,
    },
    // ---------------------------------------------------------------------------------
    Entry {
        id: Deviation::RefundRepeatCompounding,
        slug: "refund-repeat-compounding",
        title: "The refund rewrites its own record, so a second cancel compounds",
        kind: Kind::Fix,
        retail: "`refund_cost` writes `adj` back over `amt[i]` in the queue record. \
                 Re-entering through `Build::action_unqueue` therefore adjusts an already \
                 adjusted amount, and the charge grows each time.",
        ours: "Leave the recorded amount alone.",
        why: "Separate from the negative credit and separately toggleable, because the two \
              are separate mistakes: one is a sign, the other is state. Retail's own \
              reachability of this path is narrow, which is exactly why it never got \
              noticed.",
        derived_from: &[
            "Build::refund_cost 0x00620490",
            "Build::action_unqueue 0x00620280",
        ],
        evidence: "docs/mechanics/production.md §3.6. [measured] that the write-back \
                   exists; the compounding is the consequence.",
        default_in_improved: true,
        affects_checksum: true,
        seam: "don_sim::deviations::behaviour::refund_slot",
        surfaces: &[Surface::PlayableEdition, Surface::RlEnvironment],
        implementation: ImplementationStatus::Wired,
    },
    // ---------------------------------------------------------------------------------
    Entry {
        id: Deviation::CaravanHeuristicGoalY,
        slug: "caravan-heuristic-goal-y",
        title: "The caravan A* mode-A heuristic ignores the node's own y",
        kind: Kind::Fix,
        retail: "At 0x00685FD9 the child-node heuristic is `pf_dist(child.x - goalX, \
                 -goalY)` — `neg edx`, verified in raw bytes, where `child.y - goalY` was \
                 plainly intended. `pf_dist` takes `abs` of both arguments, so `h` is a \
                 large per-search constant plus a term of order `dx^2 / goalY`. A \
                 constant added to every node's `h` does not change A* ordering: **mode A \
                 is, to within a negligible term, plain Dijkstra**.",
        ours: "Pass `child.y - goalY`.",
        why: "It is a real heuristic instead of an accidental one — fewer node \
              expansions for the same path, which is throughput on the batch simulator's \
              hottest search.",
        derived_from: &[
            "PathFinder::astar_caravan_road 0x00685990",
            "mode-A child heuristic 0x00685FD9 (raw bytes `f7 da` = neg edx)",
        ],
        evidence: "docs/derivation/pathfinding.md, 'The heuristic — and a retail bug worth \
                   replicating'. [measured] that the instructions are these; [inference] \
                   that it is a mistake. **Default off even in improved mode**: it changes \
                   which path is found, not merely how fast, and the blast radius on unit \
                   behaviour is unmeasured. Turn it on deliberately, with a measurement.",
        default_in_improved: false,
        affects_checksum: true,
        seam: "don_sim::deviations::behaviour::caravan_h_mode_a_dy",
        surfaces: &[Surface::PlayableEdition, Surface::RlEnvironment],
        implementation: ImplementationStatus::Unwired,
    },
    // ---------------------------------------------------------------------------------
    Entry {
        id: Deviation::RefineryBonusDead,
        slug: "refinery-bonus-dead",
        title: "REFINERY_BONUS is loaded and then never used",
        kind: Kind::Fix,
        retail: "`City::calc_gather` writes `granary`, `lumber_mill` and `smelter` from \
                 their rule tables and stores `CityData::refinery` as a literal 0, in \
                 every path. `REFINERY_BONUS = 33%` is parsed out of the rules and is dead \
                 in the city path.",
        ours: "Apply `REFINERY_BONUS` the way the other three enhancers are applied.",
        why: "A shipped constant that does nothing is either a bug or an unannounced \
              balance decision, and we cannot yet tell which — a Refinery may well be \
              intended to earn its keep elsewhere. **Default off in improved mode**: this \
              one changes balance, and balance changes need a measurement and an argument, \
              not a toggle. It is registered so the question is asked out loud.",
        derived_from: &["City::calc_gather 0x00737C60 (literal 0 store)"],
        evidence: "docs/mechanics/tech-cities.md §3.1 — recorded there as a fact, \
                   explicitly not as a bug to work around. [measured].",
        default_in_improved: false,
        affects_checksum: true,
        seam: "don_sim::deviations::behaviour::refinery_bonus_pct",
        surfaces: &[Surface::PlayableEdition, Surface::RlEnvironment],
        implementation: ImplementationStatus::Unwired,
    },
    // ---------------------------------------------------------------------------------
    Entry {
        id: Deviation::EnvAirPatrolPhysics,
        slug: "env-air-patrol-physics",
        title: "don-env lacks the authoritative AIR_PATROL airframe transaction",
        kind: Kind::Drift,
        retail: "Unit::do_air_patrol calls Unit::do_air_physics before it advances a waypoint \
                 or searches for a target; a zero result stops the executor for the frame.",
        ours: "Routing, dynamic queue ownership and patrol-local transitions are exact. EnvWorld \
               now leaves AIR_PATROL stationary unless a mandatory AirPatrolHost supplies the \
               complete physics transaction; its straight-line mover is no longer reused.",
        why: "Moving an aircraft through the ground movement scaffold changes position, facing, \
              altitude, fuel, hosting and later scan timing while appearing plausible.",
        derived_from: &[
            "Unit::do_air_patrol 0x005EA620",
            "Unit::do_air_physics 0x005E86D0",
            "crates/don-env/src/state.rs::AirPatrolHost",
        ],
        evidence: "docs/mechanics/air.md and docs/assembly/command-bridge.md §Agreement \
                   checks. The call boundary is measured; the physics body is not ported.",
        default_in_improved: false,
        affects_checksum: true,
        seam: "",
        surfaces: &[Surface::RlEnvironment],
        implementation: ImplementationStatus::KnownDrift,
    },
    // ---------------------------------------------------------------------------------
    Entry {
        id: Deviation::EnvAirPatrolUnitTargetSearch,
        slug: "env-air-patrol-unit-target-search",
        title: "don-env lacks AIR_PATROL's air/bomber target searches",
        kind: Kind::Drift,
        retail: "On the actor-indexed mod-16 cadence, patrol chooses find_new_air_target or \
                 find_new_bomber_target first, then applies the game-option fallback.",
        ours: "The exact cadence, search origin, fighter-bomber home-relative transform and \
               StrafeOrder insertion are recovered, but EnvWorld has no spatial search host.",
        why: "Always returning no target silently removes opportunistic interception; nearest \
              entity or unordered iteration would choose a different target.",
        derived_from: &[
            "Unit::do_air_patrol 0x005EA6EC",
            "find_new_air_target",
            "find_new_bomber_target",
            "crates/don-env/src/state.rs::AirPatrolHost::find_unit_target",
        ],
        evidence: "docs/mechanics/air.md §patrol cadence and \
                   crates/don-sim/src/systems/order_dispatch.rs::do_air_patrol. [measured].",
        default_in_improved: false,
        affects_checksum: true,
        seam: "",
        surfaces: &[Surface::RlEnvironment],
        implementation: ImplementationStatus::KnownDrift,
    },
    // ---------------------------------------------------------------------------------
    Entry {
        id: Deviation::EnvAirPatrolBuildingTargetSearch,
        slug: "env-air-patrol-building-target-search",
        title: "don-env lacks AIR_PATROL's building spatial search",
        kind: Kind::Drift,
        retail: "On the actor-indexed mod-32 cadence, patrol calls ObjectsData::find_building_at \
                 with SearchIndexBH(3) and tests the returned type's owner-target bit.",
        ours: "The cadence, waypoint-derived origin, acceptance bit and mandatory StrafeOrder \
               insertion are exact, but EnvWorld has no ordered building spatial query.",
        why: "Returning no building erases attacks; a nearest-building stand-in leaks different \
              visibility, diplomacy, spatial-order and type-bit behavior.",
        derived_from: &[
            "Unit::do_air_patrol 0x005EA84C",
            "ObjectsData::find_building_at",
            "crates/don-env/src/state.rs::AirPatrolHost::find_building_target",
        ],
        evidence: "docs/mechanics/air.md §patrol cadence and \
                   crates/don-sim/src/systems/order_dispatch.rs::do_air_patrol. [measured].",
        default_in_improved: false,
        affects_checksum: true,
        seam: "",
        surfaces: &[Surface::RlEnvironment],
        implementation: ImplementationStatus::KnownDrift,
    },
    // ---------------------------------------------------------------------------------
    Entry {
        id: Deviation::AiModelSimplifications,
        slug: "ai-model-simplifications",
        title: "don-ai's match runner is a model, and it says so in six places",
        kind: Kind::Drift,
        retail: "Placement can fail spatially; `BuildData::gather_max` is a real byte; \
                 which resource a gatherer yields is decided by the map; construction \
                 needs a builder present; there is combat and diplomacy; and the ten \
                 compiled production stages have bodies.",
        ours: "`crates/don-ai/src/game.rs` marks six numbered `DEVIATION`s at their sites: \
               placement never fails spatially, worker slots come from a table, gather \
               targets come from a table, builders are assumed present, there is no \
               combat or diplomacy, and the compiled stages are stubs.",
        why: "These are model gaps in a research harness, not choices we would ship in the \
              playable edition. They are listed here so that `don-ai` output is never \
              mistaken for a fidelity result — the six are already honestly marked at \
              their sites, and this entry is the index to them.",
        derived_from: &["crates/don-ai/src/game.rs:22-40 (the six numbered DEVIATIONs)"],
        evidence: "docs/tracks/ron-ai-impl.md §3. Self-declared by that lane, not \
                   independently measured here.",
        default_in_improved: false,
        affects_checksum: false,
        seam: "",
        surfaces: &[],
        implementation: ImplementationStatus::ResearchOnly,
    },
    // ---------------------------------------------------------------------------------
    Entry {
        id: Deviation::ArenaConstructionScheduleModel,
        slug: "arena-construction-schedule-model",
        title: "Arena lacks retail construction state, identity and scheduler order",
        kind: Kind::Drift,
        retail: "A persistent BuildData site is processed before live builders, whose BuildAt \
                 orders retain (who,o,uid) and execute in retail object traversal order.",
        ours: "The construction state machine and fail-closed adapter exist, but Arena does not \
               persist their full site/order state or call them from the retail scheduler.",
        why: "Builder order is load-bearing because each contributor receives the next harmonic \
              share; a timer, set, or EntId-only target changes progress and slot-reuse behavior.",
        derived_from: &[
            "Unit::add_build_order 0x005E5210",
            "Unit::do_build 0x005EEBF0",
            "crates/don-sim/src/systems/construction.rs",
        ],
        evidence: "docs/mechanics/construction.md §§4-5; Tier C instruction/PDB recovery.",
        default_in_improved: false,
        affects_checksum: true,
        seam: "",
        surfaces: &[Surface::PlayableEdition],
        implementation: ImplementationStatus::KnownDrift,
    },
    // ---------------------------------------------------------------------------------
    Entry {
        id: Deviation::ArenaConstructionPlacementModel,
        slug: "arena-construction-placement-model",
        title: "Arena lacks the authoritative blocked-site transaction",
        kind: Kind::Drift,
        retail: "BuildTypeData::blocked_site resolves terrain, territory, cliffs/water, \
                 adjacency, dock, city and wonder-capacity dependencies before start.",
        ours: "The recovered lifecycle requires a mandatory blocked_site callback; Arena has no \
               complete implementation and may not substitute its custom placement predicate.",
        why: "Accepting a site retail rejects changes obstruction, ownership, economy and combat.",
        derived_from: &[
            "BuildTypeData::blocked_site",
            "BuildTypeData::blocked_location 0x006375B0",
            "BuildTypeData::blocked_tcoord 0x00636DB0",
        ],
        evidence: "docs/mechanics/construction.md §§4-5; callback is fail-closed in \
                   systems/construction.rs.",
        default_in_improved: false,
        affects_checksum: true,
        seam: "",
        surfaces: &[Surface::PlayableEdition],
        implementation: ImplementationStatus::KnownDrift,
    },
    // ---------------------------------------------------------------------------------
    Entry {
        id: Deviation::ArenaConstructionLifecycleModel,
        slug: "arena-construction-lifecycle-model",
        title: "Arena lacks exact construction start, activation, rejection and completion",
        kind: Kind::Drift,
        retail: "Lazy Build::start, successful Build::activate, rejected Object::disband and \
                 Unit::build_done/reassignment are full world transactions.",
        ours: "The local progress/call ordering is recovered, but Arena implements none of the \
               mandatory ownership, city, terrain, event, queue, RNG and checksum effects.",
        why: "Flags-only completion would leave a plausible building with the wrong world graph.",
        derived_from: &[
            "Build::start",
            "Build::activate",
            "Object::disband",
            "Unit::build_done 0x00603BF0",
        ],
        evidence: "docs/mechanics/construction.md §§4-5; RUNTIME_FIDELITY_READY is false.",
        default_in_improved: false,
        affects_checksum: true,
        seam: "",
        surfaces: &[Surface::PlayableEdition],
        implementation: ImplementationStatus::KnownDrift,
    },
    // ---------------------------------------------------------------------------------
    Entry {
        id: Deviation::ArenaConstructionInterruptionModel,
        slug: "arena-construction-interruption-model",
        title: "Arena lacks exact construction death and cancellation transactions",
        kind: Kind::Drift,
        retail: "Builder death/cancel closes the unit/order transaction; target destruction \
                 closes/disbands the target and builders observe invalid identity lazily.",
        ours: "The interruption boundary is recovered, but Arena does not route death, cancel or \
               target destruction through a complete object/order host.",
        why: "Dropping a timer or eager-visiting builders loses retained work and order side effects.",
        derived_from: &[
            "Unit::do_build 0x005EEBF0",
            "crates/don-sim/src/systems/construction.rs::interrupt_builder",
        ],
        evidence: "docs/mechanics/construction.md §§4-5; Tier C instruction/PDB recovery.",
        default_in_improved: false,
        affects_checksum: true,
        seam: "",
        surfaces: &[Surface::PlayableEdition],
        implementation: ImplementationStatus::KnownDrift,
    },
    // ---------------------------------------------------------------------------------
    Entry {
        id: Deviation::ArenaGatherCapacityModel,
        slug: "arena-gather-capacity-model",
        title: "Arena lacks the authoritative gathering-capacity evaluator",
        kind: Kind::Drift,
        retail: "BuildTypeData::calc_gather evaluates the ordered MiningList, real terrain, \
                 access, ownership/diplomacy and player modifiers into signed gather_max.",
        ours: "Signed-byte storage is recovered, but Arena still has no complete evaluator and \
               may not infer slots from a building table, radius or gather_from length.",
        why: "Invented capacity changes the opening economy that arena results rank.",
        derived_from: &[
            "BuildTypeData::calc_gather 0x00639E40",
            "BuildTypeData::max_gatherers 0x0063C430",
        ],
        evidence: "docs/mechanics/gathering.md §Capacity comes from the world, not a type table.",
        default_in_improved: false,
        affects_checksum: true,
        seam: "",
        surfaces: &[Surface::PlayableEdition],
        implementation: ImplementationStatus::KnownDrift,
    },
    // ---------------------------------------------------------------------------------
    Entry {
        id: Deviation::ArenaGatherOccupancyModel,
        slug: "arena-gather-occupancy-model",
        title: "Arena lacks persistent retail gathering occupancy and order identity",
        kind: Kind::Drift,
        retail: "BuildData and UnitData keep an owner-local intrusive gather_down chain; a live \
                 GatherOrder retains (whom,ox,uid), arrival state and exact detach/close paths.",
        ours: "Exact attach, prune, detach and UID checks have an adapter, but Arena does not own \
               the persistent object-table links or execute the gather command/order lifecycle.",
        why: "Inferred membership or stale object identity changes crowding and payout.",
        derived_from: &[
            "Build::add_gatherer 0x0062F640",
            "Build::check_gatherers 0x0062F710",
            "Build::remove_gatherer 0x0062F8D0",
        ],
        evidence: "docs/mechanics/gathering.md §§The state model, Generational targets.",
        default_in_improved: false,
        affects_checksum: true,
        seam: "",
        surfaces: &[Surface::PlayableEdition],
        implementation: ImplementationStatus::KnownDrift,
    },
    // ---------------------------------------------------------------------------------
    Entry {
        id: Deviation::ArenaGatherReservationModel,
        slug: "arena-gather-reservation-model",
        title: "Arena lacks the ordered gathering-terrain reservation lifecycle",
        kind: Kind::Drift,
        retail: "find_gather_tiles and verify_gather_tiles maintain an ordered weighted \
                 MiningList and TData 0x1000 claims; non-flat gathering rotates entries.",
        ours: "Atomic writes for already-authoritative tiles are recovered, but Arena has no \
               selection, verification, invalidation or non-flat rotation transaction.",
        why: "The reservation bit is world state, not a depletion counter or optional hint.",
        derived_from: &[
            "Build::find_gather_tiles 0x00623350",
            "WorldData::is_gathered_from",
            "Unit::do_non_flat_gather",
        ],
        evidence: "docs/mechanics/gathering.md §Terrain claims and non-depletion.",
        default_in_improved: false,
        affects_checksum: true,
        seam: "",
        surfaces: &[Surface::PlayableEdition],
        implementation: ImplementationStatus::KnownDrift,
    },
    // ---------------------------------------------------------------------------------
    Entry {
        id: Deviation::ArenaGatherPayoutModel,
        slug: "arena-gather-payout-model",
        title: "Arena lacks the authoritative gathering-payout transaction",
        kind: Kind::Drift,
        retail: "The terrain/type evaluator produces six-slot per-worker gross, active occupancy \
                 scales it, and Leader::do_gather applies carries, caps and expenses.",
        ours: "Gross composition and carry kernels are recovered, but Arena has neither the \
               authoritative evaluator nor the exact leader income/checksum transaction.",
        why: "A custom resource-kind or direct credit path changes income values and timing.",
        derived_from: &[
            "BuildTypeData::calc_gather 0x00639E40",
            "Leader::do_gather",
            "crates/don-sim/src/systems/gathering.rs",
        ],
        evidence: "docs/mechanics/gathering.md §Rates, caps, and credit.",
        default_in_improved: false,
        affects_checksum: true,
        seam: "",
        surfaces: &[Surface::PlayableEdition],
        implementation: ImplementationStatus::KnownDrift,
    },
    // ---------------------------------------------------------------------------------
    Entry {
        id: Deviation::ArenaTargetAcquisitionModel,
        slug: "arena-target-acquisition-model",
        title: "Arena MODEL 4 has an incomplete retail target-acquisition host",
        kind: Kind::Drift,
        retail: "Retail walks spatial cells in stable order and applies diplomacy, visibility, \
                 pre-fog cloak/detection, validity, region, priority, crowding, stance and \
                 building-territory gates.",
        ours: "The focused unit-target path is recovered, but the full Arena/Marshal product \
               behavior is not yet green. Arena also lacks authoritative seen3 detector/cloak \
               state and ordinary building admission still lacks WData territory ownership.",
        why: "A partially integrated scan changes combat choices and bot economy. The coarse \
              blocker remains until full Arena/Marshal gates pass; only then may the residual \
              building-territory transaction be split out independently.",
        derived_from: &[
            "Object::find_auto_target 0x0064DDA0",
            "Object::check_target building tail",
            "crates/don-ai/src/arena/world.rs::ArenaTargetAdapter",
        ],
        evidence: "docs/assembly/target-selection.md. Focused non-cloaked direct-land target \
                   integration is green, but full Arena/Marshal validation currently regresses; \
                   cloak/detection and building-territory hosts remain explicit residuals.",
        default_in_improved: false,
        affects_checksum: true,
        seam: "",
        surfaces: &[Surface::PlayableEdition],
        implementation: ImplementationStatus::KnownDrift,
    },
    // ---------------------------------------------------------------------------------
    Entry {
        id: Deviation::ArenaGuyTurretModel,
        slug: "arena-guy-turret-model",
        title: "Arena does not materialize graphics-turret Guys",
        kind: Kind::Drift,
        retail: "`Guy::init_real` selects each Guy's gpiece and sets `GUY_FLAG_TURRETS` \
                 when that selected graphics graph has pivot restrictions; initial and live \
                 angles come from the loaded hierarchy before `Unit::fight` plans a volley.",
        ours: "The supported installed XML catalog, transactional Guy graphics profile \
               installer and exact pivot-aim arithmetic are recovered, but Arena does not \
               yet supply the required retail/.bh3 hierarchy extractor and position provider.",
        why: "The exact supported-roster flank path is complete. Silently treating a turret \
              type as an ordinary Guy would create a different approximation, so the roster \
              prerequisite remains independently product-blocking.",
        derived_from: &[
            "Unit::fight 0x005FD4D0",
            "Guy::init_real 0x005DB6B0",
            "Guy::set_all_pivots 0x005D8BC0",
            "don_sim::systems::fight::direct_land_volley_plan",
            "don_sim::systems::graphics_turret",
        ],
        evidence: "docs/mechanics/graphics-turrets.md; installed-file catalog smoke test and \
                   fail-closed materialization/aim tests in graphics_turret.rs.",
        default_in_improved: false,
        affects_checksum: true,
        seam: "",
        surfaces: &[Surface::PlayableEdition],
        implementation: ImplementationStatus::KnownDrift,
    },
    // ---------------------------------------------------------------------------------
    Entry {
        id: Deviation::ArenaWaterModel,
        slug: "arena-water-model",
        title: "Arena MODEL 6a has no retail water terrain/pathing host",
        kind: Kind::Drift,
        retail: "Maps carry water/WATERHALF cells, water regions and the tile/water A* domains.",
        ours: "The playable arena generator is land-only and cannot execute a retail water path.",
        why: "A water capability flag or body-radius route would be a new simplified model, not \
              the missing retail terrain and pathfinder.",
        derived_from: &[
            "WorldData water fields and astar_path 0x00683770",
            "crates/don-ai/src/arena/world.rs MODEL 6 declaration",
        ],
        evidence: "docs/mechanics/map-terrain.md and docs/mechanics/movement.md; exact missing \
                   prerequisites listed in arena::retail_systems::MODEL6_INVENTORY.",
        default_in_improved: false,
        affects_checksum: true,
        seam: "",
        surfaces: &[Surface::PlayableEdition],
        implementation: ImplementationStatus::KnownDrift,
    },
    // ---------------------------------------------------------------------------------
    Entry {
        id: Deviation::ArenaNavalModel,
        slug: "arena-naval-model",
        title: "Arena MODEL 6b has no retail naval object/order/production host",
        kind: Kind::Drift,
        retail: "Naval play uses find_wpath, dock lifecycle, queues, boarding, containment, \
                 rendezvous, unloading, fishing, naval territory and supply.",
        ours: "The arena admits no naval runtime; the recovered naval module remains Tier C and \
               labels its incomplete route/board APIs as proxies.",
        why: "Calling a proxy or spawning one ship would replace the omission with a knowingly \
              simplified naval simulation.",
        derived_from: &[
            "crates/don-sim/src/systems/naval.rs",
            "crates/don-ai/src/arena/world.rs MODEL 6 declaration",
        ],
        evidence: "docs/mechanics/naval.md and arena MODEL6 integration inventory.",
        default_in_improved: false,
        affects_checksum: true,
        seam: "",
        surfaces: &[Surface::PlayableEdition],
        implementation: ImplementationStatus::KnownDrift,
    },
    // ---------------------------------------------------------------------------------
    Entry {
        id: Deviation::ArenaAirModel,
        slug: "arena-air-model",
        title: "Arena MODEL 6c has no complete retail airframe host",
        kind: Kind::Drift,
        retail: "Aircraft execute do_air_physics, live order/host scans, fuel, capacity and the \
                 anti-air gate at Ammo::init's main-RNG position.",
        ours: "Live AirTypeData and exact local air adapters now exist, but the playable arena \
               has no airframe/ammo/order/checksum caller.",
        why: "The adapter is fail-closed scaffolding; it does not turn isolated primitives into \
              an air simulation claim.",
        derived_from: &[
            "Unit::do_air_physics",
            "Ammo::init 0x0067BBF0",
            "crates/don-ai/src/arena/retail_systems.rs",
        ],
        evidence: "docs/mechanics/air.md and arena MODEL6 integration inventory.",
        default_in_improved: false,
        affects_checksum: true,
        seam: "",
        surfaces: &[Surface::PlayableEdition],
        implementation: ImplementationStatus::KnownDrift,
    },
    // ---------------------------------------------------------------------------------
    Entry {
        id: Deviation::ArenaDiplomacyModel,
        slug: "arena-diplomacy-model",
        title: "Arena MODEL 6d has no complete retail diplomacy host",
        kind: Kind::Drift,
        retail: "Diplomacy stores bilateral declarations, resolves their mutual minimum, and \
                 applies retargeting, vision, event/chat and strategy side effects.",
        ours: "The exact explicit relation table adapter is available, but arena matches still \
               expose no declaration command or runtime side-effect channel.",
        why: "Two players beginning at war does not make diplomacy complete, and relation state \
              may not be hidden inside a bot.",
        derived_from: &[
            "LeaderData::get_diplo 0x006EBA50",
            "Leader::set_diplo 0x006EC6A0",
            "Leader::diplomacy 0x006BC950",
        ],
        evidence: "docs/mechanics/victory-score.md and arena MODEL6 integration inventory.",
        default_in_improved: false,
        affects_checksum: true,
        seam: "",
        surfaces: &[Surface::PlayableEdition],
        implementation: ImplementationStatus::KnownDrift,
    },
    // ---------------------------------------------------------------------------------
    Entry {
        id: Deviation::ArenaAttritionModel,
        slug: "arena-attrition-model",
        title: "Arena MODEL 6e does not execute retail attrition state/damage",
        kind: Kind::Drift,
        retail: "Retail recomputes each unit's period from territory/leader/type predicates, \
                 phases it by object id, supply-gates it and applies the exact damage shape.",
        ours: "Exact arithmetic and a prerequisite-explicit adapter exist, but arena units do \
               not own or tick the required attrition state.",
        why: "Applying a generic damage-over-time constant would be a deliberate simplification.",
        derived_from: &[
            "Unit::process_attrition 0x005E11A0",
            "Unit::suffer_attrition 0x005E1A10",
            "UnitData::get_attrition 0x00608FD0",
        ],
        evidence: "docs/mechanics/borders-fog.md and arena MODEL6 integration inventory.",
        default_in_improved: false,
        affects_checksum: true,
        seam: "",
        surfaces: &[Surface::PlayableEdition],
        implementation: ImplementationStatus::KnownDrift,
    },
    // ---------------------------------------------------------------------------------
    Entry {
        id: Deviation::ArenaSupplyModel,
        slug: "arena-supply-model",
        title: "Arena MODEL 6f does not execute retail supply queries/state",
        kind: Kind::Drift,
        retail: "Unit::process_supply short-circuits exact type/bonus gates then queries live \
                 supply sources and three building classes in order; supply gates attrition and \
                 affects reload/healing.",
        ours:
            "The predicate adapter requires every query result explicitly; Supplies::find_supply \
               and arena source/building queries are absent, so no supply result is fabricated.",
        why: "Always-in-supply is not neutral: it disables attrition and changes combat pacing.",
        derived_from: &[
            "Unit::process_supply 0x005E0560",
            "Supplies::find_supply 0x0073ABA0",
        ],
        evidence: "docs/mechanics/borders-fog.md and arena MODEL6 integration inventory.",
        default_in_improved: false,
        affects_checksum: true,
        seam: "",
        surfaces: &[Surface::PlayableEdition],
        implementation: ImplementationStatus::KnownDrift,
    },
    // ---------------------------------------------------------------------------------
    Entry {
        id: Deviation::AttackDirSemantics,
        slug: "attack-dir-semantics",
        title: "attack_dir means what its name says — do not 'fix' it",
        kind: Kind::Rejected,
        retail: "`Unit::fight` computes `attack_dir = find_angle(target.x - attacker.x, \
                 target.y - attacker.y)`: the direction the attack *travels*, attacker to \
                 target. `attack_dir == defender_facing` therefore means the attack is \
                 going the way the defender is looking, i.e. the attacker is behind it.",
        ours: "Nothing. Same convention, same arcs.",
        why: "This was carried for a while as a suspected mis-named argument, because with \
              `defender_facing = 0` an `attack_dir` of 0 scores flank tier 1 and a half \
              turn scores 0 — which looks backwards until the convention is pinned. \
              Substituting `bearing = attack_dir - half turn` gives the arcs you would \
              expect: front +-60 degrees no bonus, sides 2 x 75 degrees tier 2, rear +-45 \
              degrees tier 1. Sides worth twice the rear, front worth nothing, exactly as \
              the rules.xml comment describes. Changing this would make a frontal charge \
              earn the flanking bonus.",
        derived_from: &[
            "Unit::fight 0x005FE872..0x005FE89B (dx/dy order, then find_angle)",
            "flank pre-guard 0x00644B1D",
        ],
        evidence: "docs/assembly/target-selection.md, '`attack_dir`, settled'. [measured] \
                   in capstone. Supersedes the open question in \
                   docs/tracks/web-spectator.md §1.3.",
        default_in_improved: false,
        affects_checksum: false,
        seam: "",
        surfaces: &[],
        implementation: ImplementationStatus::NotApplicable,
    },
    // ---------------------------------------------------------------------------------
    Entry {
        id: Deviation::GatherEnhancerTableBase,
        slug: "gather-enhancer-table-base",
        title: "The gather-enhancer tables are 1-based by design — do not 'fix' them",
        kind: Kind::Rejected,
        retail: "`City::calc_gather` pushes base `+0x2B8` against a `granary_bonus[5]` \
                 that starts at `+700`, and likewise for lumber mill and smelter; \
                 `SCHOLAR_RATE` is read as `RULES[0x284 + (level-1)*4]`. The level \
                 accessors are 1-based, with 0 meaning 'no such building' and \
                 short-circuited to a zero bonus by the building flags.",
        ours: "Nothing. Index by `level - 1`, as retail does.",
        why: "Two lanes found this independently and both were briefly tempted to call it \
              an off-by-one. It is not: a port that indexes by `level` reads the next \
              tier's number for every enhancer building in the game. Registered so the \
              third lane to notice it does not 'fix' it.",
        derived_from: &[
            "City::calc_gather 0x00737C60",
            "SCHOLAR_RATE read at 0x006D5754",
        ],
        evidence: "docs/mechanics/tech-cities.md §3.1 and docs/mechanics/economy.md §4.4 \
                   — two independent [measured] confirmations.",
        default_in_improved: false,
        affects_checksum: false,
        seam: "",
        surfaces: &[],
        implementation: ImplementationStatus::NotApplicable,
    },
];

impl Deviation {
    /// The number of registry entries.
    pub const COUNT: usize = 31;

    /// Every deviation, in registry order.
    pub const ALL: [Deviation; Deviation::COUNT] = [
        Deviation::AiGatherHandicap,
        Deviation::GatherHandicapTruncation,
        Deviation::BhsPrereqResultTest,
        Deviation::BhsCitizensTypo,
        Deviation::TikalBorderRuleSlot,
        Deviation::RefundChargesPlayer,
        Deviation::RefundRepeatCompounding,
        Deviation::CaravanHeuristicGoalY,
        Deviation::RefineryBonusDead,
        Deviation::EnvAirPatrolPhysics,
        Deviation::EnvAirPatrolUnitTargetSearch,
        Deviation::EnvAirPatrolBuildingTargetSearch,
        Deviation::AiModelSimplifications,
        Deviation::ArenaConstructionScheduleModel,
        Deviation::ArenaConstructionPlacementModel,
        Deviation::ArenaConstructionLifecycleModel,
        Deviation::ArenaConstructionInterruptionModel,
        Deviation::ArenaGatherCapacityModel,
        Deviation::ArenaGatherOccupancyModel,
        Deviation::ArenaGatherReservationModel,
        Deviation::ArenaGatherPayoutModel,
        Deviation::ArenaTargetAcquisitionModel,
        Deviation::ArenaGuyTurretModel,
        Deviation::ArenaWaterModel,
        Deviation::ArenaNavalModel,
        Deviation::ArenaAirModel,
        Deviation::ArenaDiplomacyModel,
        Deviation::ArenaAttritionModel,
        Deviation::ArenaSupplyModel,
        Deviation::AttackDirSemantics,
        Deviation::GatherEnhancerTableBase,
    ];

    /// Registry index.
    pub const fn index(self) -> usize {
        self as usize
    }

    /// This deviation's registry entry.
    pub fn entry(self) -> &'static Entry {
        &REGISTRY[self.index()]
    }

    pub fn slug(self) -> &'static str {
        self.entry().slug
    }

    pub fn kind(self) -> Kind {
        self.entry().kind
    }

    /// Look a deviation up by slug.
    pub fn from_slug(s: &str) -> Result<Deviation, ModeError> {
        let s = s.trim();
        Deviation::ALL
            .into_iter()
            .find(|d| d.slug() == s)
            .ok_or_else(|| ModeError::UnknownSlug(s.to_string()))
    }
}

impl fmt::Display for Deviation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.slug())
    }
}

// =======================================================================================
// ModeConfig
// =======================================================================================

/// The mode, plus which registry entries are switched on.
///
/// The fidelity invariant is structural rather than remembered: the active set is a
/// bitmask that [`ModeConfig::fidelity`] leaves empty, [`ModeConfig::enable`] refuses to
/// touch in fidelity mode, and [`ModeConfig::is_active`] short-circuits to `false` in
/// fidelity mode regardless. Three independent barriers, because the failure this guards
/// against — a fidelity number quietly produced under a fix — is silent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModeConfig {
    mode: Mode,
    /// Bit `d.index()` set == entry active. Always 0 when `mode == Fidelity`.
    active: u32,
}

impl Default for ModeConfig {
    /// Fidelity, nothing active. **This is the project default and it is load-bearing.**
    fn default() -> Self {
        ModeConfig::fidelity()
    }
}

impl ModeConfig {
    /// Retail behaviour, bugs included. No entry can be active.
    pub const fn fidelity() -> ModeConfig {
        ModeConfig {
            mode: Mode::Fidelity,
            active: 0,
        }
    }

    /// Our edition, with every [`Entry::default_in_improved`] fix switched on.
    pub fn improved() -> ModeConfig {
        let mut active = 0u32;
        let mut i = 0;
        while i < Deviation::COUNT {
            let e = &REGISTRY[i];
            if e.kind.toggleable() && e.default_in_improved {
                active |= 1 << i;
            }
            i += 1;
        }
        ModeConfig {
            mode: Mode::Improved,
            active,
        }
    }

    /// Improved mode with nothing switched on — the base for building a set by hand.
    pub const fn improved_bare() -> ModeConfig {
        ModeConfig {
            mode: Mode::Improved,
            active: 0,
        }
    }

    pub const fn mode(&self) -> Mode {
        self.mode
    }

    pub const fn is_fidelity(&self) -> bool {
        matches!(self.mode, Mode::Fidelity)
    }

    /// Whether this deviation is in force.
    ///
    /// Returns `false` for every entry in fidelity mode, and for every non-[`Kind::Fix`]
    /// entry in any mode.
    pub fn is_active(&self, d: Deviation) -> bool {
        if self.is_fidelity() {
            return false;
        }
        self.active & (1 << d.index()) != 0
    }

    /// Switch an entry on. Fails in fidelity mode, and for entries that are not fixes.
    pub fn enable(&mut self, d: Deviation) -> Result<(), ModeError> {
        if self.is_fidelity() {
            return Err(ModeError::FidelityIsImmutable(d));
        }
        if !d.kind().toggleable() {
            return Err(ModeError::NotToggleable(d));
        }
        self.active |= 1 << d.index();
        Ok(())
    }

    /// Switch an entry off. Always allowed — turning things off can never contaminate a
    /// fidelity claim.
    pub fn disable(&mut self, d: Deviation) {
        self.active &= !(1 << d.index());
    }

    /// Every active entry, in registry order.
    pub fn active(&self) -> impl Iterator<Item = Deviation> + '_ {
        Deviation::ALL.into_iter().filter(|d| self.is_active(*d))
    }

    /// How many entries are active.
    pub fn active_count(&self) -> usize {
        self.active().count()
    }

    /// `Ok(())` iff this config is safe to produce a fidelity number under.
    ///
    /// This is what `tools/replay-validate.sh` asserts, through the `don-deviations`
    /// binary, before it is allowed to write a scoreboard.
    pub fn assert_fidelity(&self) -> Result<(), ModeError> {
        if !self.is_fidelity() {
            return Err(ModeError::NotFidelity(self.mode));
        }
        match self.active().next() {
            Some(d) => Err(ModeError::ActiveInFidelity(d)),
            None => Ok(()),
        }
    }

    /// Every known reason `surface` is not admissible for a fidelity/product claim.
    ///
    /// This gate is scoped to code the entrypoint can actually execute. Research-only
    /// models therefore remain available without pretending they are complete, while a
    /// playable or RL path that knowingly substitutes a simplified model fails closed.
    pub fn readiness_blockers(
        &self,
        surface: Surface,
    ) -> impl Iterator<Item = ReadinessBlocker> + '_ {
        Deviation::ALL.into_iter().filter_map(move |d| {
            let e = d.entry();
            if !e.reaches(surface) {
                return None;
            }
            match (e.kind, e.implementation) {
                (Kind::Fix, ImplementationStatus::Unwired) if self.is_active(d) => {
                    Some(ReadinessBlocker::UnwiredImprovement(d))
                }
                (Kind::Drift, ImplementationStatus::KnownDrift) => {
                    Some(ReadinessBlocker::KnownDrift(d))
                }
                _ => None,
            }
        })
    }

    /// Refuse a claim-bearing entrypoint unless its reachable behaviour is admissible.
    ///
    /// Replay validation additionally requires fidelity mode. Playable and RL entrypoints
    /// may run either edition, but may not run a known approximation; improved mode also
    /// may not advertise an active fix until the real subsystem calls its seam.
    pub fn assert_ready(&self, surface: Surface) -> Result<(), ModeError> {
        if surface.requires_fidelity() {
            self.assert_fidelity()?;
        }
        match self.readiness_blockers(surface).next() {
            Some(ReadinessBlocker::UnwiredImprovement(d)) => {
                Err(ModeError::UnwiredImprovement(surface, d))
            }
            Some(ReadinessBlocker::KnownDrift(d)) => Err(ModeError::KnownDrift(surface, d)),
            None => Ok(()),
        }
    }

    /// Build a config from the process environment.
    ///
    /// * `DON_MODE` — `fidelity` (default, also `retail`) or `improved`.
    /// * `DON_DEVIATIONS` — comma-separated `slug`, `+slug` or `-slug`, applied in order
    ///   on top of the mode's defaults.
    ///
    /// Every failure is loud. In particular, asking for `+slug` while in fidelity mode is
    /// an **error**, not a silent no-op: someone who typed it meant it, and the only safe
    /// way to answer is to refuse and say why.
    pub fn from_env() -> Result<ModeConfig, ModeError> {
        let mode = std::env::var("DON_MODE").ok();
        let list = std::env::var("DON_DEVIATIONS").ok();
        ModeConfig::from_env_parts(mode.as_deref(), list.as_deref())
    }

    /// The testable half of [`ModeConfig::from_env`].
    pub fn from_env_parts(mode: Option<&str>, list: Option<&str>) -> Result<ModeConfig, ModeError> {
        let mode = match mode {
            None => Mode::Fidelity,
            Some(s) if s.trim().is_empty() => Mode::Fidelity,
            Some(s) => Mode::from_slug(s)?,
        };
        let mut cfg = match mode {
            Mode::Fidelity => ModeConfig::fidelity(),
            Mode::Improved => ModeConfig::improved(),
        };
        for tok in list.unwrap_or("").split(',') {
            let tok = tok.trim();
            if tok.is_empty() {
                continue;
            }
            let (on, name) = match tok.as_bytes()[0] {
                b'+' => (true, &tok[1..]),
                b'-' | b'!' => (false, &tok[1..]),
                _ => (true, tok),
            };
            let d = Deviation::from_slug(name)?;
            if on {
                cfg.enable(d)?;
            } else {
                cfg.disable(d);
            }
        }
        Ok(cfg)
    }

    /// One line per active entry, for logs and for the `don-deviations` banner.
    pub fn describe(&self) -> String {
        let mut s = format!("mode={} active={}", self.mode, self.active_count());
        for d in self.active() {
            s.push_str("\n  + ");
            s.push_str(d.slug());
            s.push_str(" — ");
            s.push_str(d.entry().title);
        }
        s
    }
}

// =======================================================================================
// Errors
// =======================================================================================

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModeError {
    /// A deviation was asked to be enabled while in fidelity mode.
    FidelityIsImmutable(Deviation),
    /// A `Drift` or `Rejected` entry was asked to be enabled.
    NotToggleable(Deviation),
    /// `DON_DEVIATIONS` named something that is not in the registry.
    UnknownSlug(String),
    /// `DON_MODE` named something that is not a mode.
    UnknownMode(String),
    /// A readiness target named something unsupported.
    UnknownSurface(String),
    /// [`ModeConfig::assert_fidelity`] on a non-fidelity config.
    NotFidelity(Mode),
    /// [`ModeConfig::assert_fidelity`] found an active entry. Should be unreachable —
    /// it exists so that if the invariant is ever broken, the gate says so rather than
    /// passing.
    ActiveInFidelity(Deviation),
    /// Improved mode advertised a fix whose real subsystem has not adopted its seam.
    UnwiredImprovement(Surface, Deviation),
    /// A knowingly approximate implementation can execute on the requested surface.
    KnownDrift(Surface, Deviation),
}

impl fmt::Display for ModeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModeError::FidelityIsImmutable(d) => write!(
                f,
                "cannot enable `{d}` in fidelity mode: fidelity reproduces retail, bugs \
                 included. Set DON_MODE=improved if that is what you meant."
            ),
            ModeError::NotToggleable(d) => write!(
                f,
                "`{d}` is a {} entry, not a fix; it has no toggle",
                d.kind().slug()
            ),
            ModeError::UnknownSlug(s) => {
                write!(f, "no deviation named `{s}` (see `don-deviations --list`)")
            }
            ModeError::UnknownMode(s) => {
                write!(f, "no such mode `{s}` (expected `fidelity` or `improved`)")
            }
            ModeError::UnknownSurface(s) => write!(
                f,
                "no readiness surface `{s}` (expected `replay`, `playable`, `rl-env`, or `product`)"
            ),
            ModeError::NotFidelity(m) => write!(
                f,
                "this measurement may only be produced in fidelity mode; DON_MODE is `{m}`"
            ),
            ModeError::ActiveInFidelity(d) => {
                write!(f, "INVARIANT BROKEN: `{d}` reports active in fidelity mode")
            }
            ModeError::UnwiredImprovement(surface, d) => write!(
                f,
                "{surface} is not ready: active improvement `{d}` is registered but its real \
                 subsystem does not call the seam"
            ),
            ModeError::KnownDrift(surface, d) => write!(
                f,
                "{surface} is not ready: known approximation `{d}` can execute on this surface"
            ),
        }
    }
}

impl std::error::Error for ModeError {}

// =======================================================================================
// Behaviour — the seams
// =======================================================================================

/// One function per deviation, each holding both branches.
///
/// The point of routing a divergence through a named function is that the *retail* branch
/// stays written down next to ours. A subsystem that consults these keeps its derivation
/// intact and gains a switch; a subsystem that forks its own copy loses both.
pub mod behaviour {
    use super::{Deviation, ModeConfig};

    /// `LeaderData::get_gather_handicap` `0x006D66A0`, indexed by stored difficulty
    /// 0..=5 (Easiest..Toughest). Tier C — read from the decompilation, not yet
    /// oracle-checked.
    pub const GATHER_HANDICAP_PCT: [i32; 6] = [-35, -15, -7, 0, 25, 50];

    /// The signed income percentage for a stored difficulty.
    ///
    /// The caller owns the question of *whether* the engine applies a handicap to this
    /// player at all — retail gives humans 0 unless the no-rush option is set, and that
    /// gate lives in `Leader::do_gather`, not here. Out-of-range difficulties return 0.
    pub fn gather_handicap_pct(cfg: &ModeConfig, difficulty: u8) -> i32 {
        let retail = GATHER_HANDICAP_PCT
            .get(difficulty as usize)
            .copied()
            .unwrap_or(0);
        select_gather_handicap(cfg, retail)
    }

    /// Select between the percentage already produced by retail's caller and DoN's fair
    /// difficulty policy. Keeping this separate from the difficulty table preserves the
    /// human/no-rush gates that `LeaderData::get_gather_handicap` applies before payout.
    pub fn select_gather_handicap(cfg: &ModeConfig, retail_pct: i32) -> i32 {
        if cfg.is_active(Deviation::AiGatherHandicap) {
            0
        } else {
            retail_pct
        }
    }

    /// One application of the handicap: `Leader::do_gather` `0x006CE450` computes
    /// `income = (100 + pct) * income / 100` with a 32-bit `imul`/`idiv`, truncating
    /// toward zero.
    pub fn apply_gather_handicap(cfg: &ModeConfig, income: i32, difficulty: u8) -> i32 {
        let pct = gather_handicap_pct(cfg, difficulty);
        apply_gather_handicap_pct(cfg, income, pct)
    }

    /// Apply an already selected percentage at the real `Leader::do_gather` seam.
    pub fn apply_gather_handicap_pct(cfg: &ModeConfig, income: i32, pct: i32) -> i32 {
        let scaled = (100i32.wrapping_add(pct)).wrapping_mul(income);
        if cfg.is_active(Deviation::GatherHandicapTruncation) {
            div_round_nearest(scaled, 100)
        } else {
            scaled.wrapping_div(100)
        }
    }

    /// Round half away from zero, which is what "not biased downward" means for a value
    /// that can be negative.
    fn div_round_nearest(n: i32, d: i32) -> i32 {
        let half = d / 2;
        if (n >= 0) == (d >= 0) {
            n.wrapping_add(half).wrapping_div(d)
        } else {
            n.wrapping_sub(half).wrapping_div(d)
        }
    }

    /// The engine's order tri-state, as the BHS host returns it.
    pub const ORDER_INVALID: i32 = -1;
    /// Refused, but the player could have paid — the only failure the shipped script tests.
    pub const ORDER_REFUSED: i32 = 0;

    /// Did this order fail in the way that should make the script yield its cycle?
    ///
    /// Retail: `result == 0`, which is what `economic.bhs` writes at :638 and :642 —
    /// so `-1` (invalid, e.g. unmet prerequisite) falls through and the step retries
    /// forever with its watchdog disarmed.
    pub fn bhs_order_failed(cfg: &ModeConfig, result: i32) -> bool {
        if cfg.is_active(Deviation::BhsPrereqResultTest) {
            result <= 0
        } else {
            result == ORDER_REFUSED
        }
    }

    /// Resolve a unit-type name written by a shipped script.
    ///
    /// Retail passes `"Citizens"` straight through and finds nothing.
    pub fn bhs_unit_type_name<'a>(cfg: &ModeConfig, name: &'a str) -> &'a str {
        if cfg.is_active(Deviation::BhsCitizensTypo) && name == "Citizens" {
            "Citizen"
        } else {
            name
        }
    }

    /// Tikal's border bonus percentage.
    ///
    /// Retail loads `[rules + 0x4A0]` = `TIKAL_TEMPLE_HP` at `0x006B0DC9`;
    /// `TIKAL_TEMPLE_BORDERS` is `[rules + 0x498]` and is never read here. Both ship as
    /// 50, so on shipped data the two branches agree — pass both and let the caller stay
    /// honest about which slot it meant.
    pub fn tikal_border_percent(
        cfg: &ModeConfig,
        tikal_temple_borders: i32,
        tikal_temple_hp: i32,
    ) -> i32 {
        if cfg.is_active(Deviation::TikalBorderRuleSlot) {
            tikal_temple_borders
        } else {
            tikal_temple_hp
        }
    }

    /// What one resource slot of `Build::refund_cost` does to the world.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct RefundOutcome {
        /// Added to the stockpile. Negative in retail whenever `adjusted > amt`.
        pub credit: i32,
        /// Written back over the queue record's amount.
        pub new_amt: i32,
    }

    /// The tail of `Build::refund_cost` `0x00620490`, given the amount originally paid
    /// and the age-adjusted amount (`production::refund_amount`, which is unchanged in
    /// both modes — this seam owns only what is *done* with it).
    pub fn refund_slot(cfg: &ModeConfig, amt: i32, adjusted: i32) -> RefundOutcome {
        let raw = amt.wrapping_sub(adjusted);
        let credit = if cfg.is_active(Deviation::RefundChargesPlayer) {
            raw.max(0)
        } else {
            raw
        };
        let new_amt = if cfg.is_active(Deviation::RefundRepeatCompounding) {
            amt
        } else {
            adjusted
        };
        RefundOutcome { credit, new_amt }
    }

    /// The second argument the caravan A\* mode-A child heuristic passes to the integer
    /// distance kernel.
    ///
    /// Retail passes `-goalY` (`neg edx` at `0x00685FD9`); the kernel takes `abs` of both
    /// arguments, so the node's own `y` never reaches the heuristic.
    pub fn caravan_h_mode_a_dy(cfg: &ModeConfig, child_y: i32, goal_y: i32) -> i32 {
        if cfg.is_active(Deviation::CaravanHeuristicGoalY) {
            child_y.wrapping_sub(goal_y)
        } else {
            goal_y.wrapping_neg()
        }
    }

    /// The refinery gather enhancer, as `City::calc_gather` `0x00737C60` stores it.
    ///
    /// Retail stores a literal 0 regardless of the rules value.
    pub fn refinery_bonus_pct(cfg: &ModeConfig, rules_refinery_bonus: i32) -> i32 {
        if cfg.is_active(Deviation::RefineryBonusDead) {
            rules_refinery_bonus
        } else {
            0
        }
    }
}

// =======================================================================================
// Tests
// =======================================================================================

#[cfg(test)]
mod tests {
    use super::behaviour::*;
    use super::*;

    // -- the invariant ------------------------------------------------------------------

    #[test]
    fn the_default_is_fidelity() {
        assert_eq!(ModeConfig::default().mode(), Mode::Fidelity);
        assert!(ModeConfig::default().is_fidelity());
        assert_eq!(Mode::default(), Mode::Fidelity);
    }

    #[test]
    fn no_entry_is_ever_active_in_fidelity_mode() {
        let cfg = ModeConfig::fidelity();
        for d in Deviation::ALL {
            assert!(!cfg.is_active(d), "{d} active in fidelity mode");
        }
        assert_eq!(cfg.active_count(), 0);
        assert_eq!(cfg.assert_fidelity(), Ok(()));
    }

    #[test]
    fn fidelity_refuses_to_be_switched() {
        let mut cfg = ModeConfig::fidelity();
        for d in Deviation::ALL {
            assert_eq!(
                cfg.enable(d),
                Err(ModeError::FidelityIsImmutable(d)),
                "{d} was allowed on in fidelity mode"
            );
            assert!(!cfg.is_active(d));
        }
    }

    #[test]
    fn drift_and_rejected_entries_have_no_toggle() {
        let mut cfg = ModeConfig::improved();
        for d in Deviation::ALL {
            if d.kind().toggleable() {
                continue;
            }
            assert_eq!(cfg.enable(d), Err(ModeError::NotToggleable(d)));
            assert!(!cfg.is_active(d));
        }
    }

    #[test]
    fn improved_activates_exactly_the_defaults() {
        let cfg = ModeConfig::improved();
        for d in Deviation::ALL {
            let e = d.entry();
            let want = e.kind.toggleable() && e.default_in_improved;
            assert_eq!(cfg.is_active(d), want, "{d}");
        }
        assert!(cfg.assert_fidelity().is_err());
    }

    #[test]
    fn entries_are_individually_revocable() {
        let mut cfg = ModeConfig::improved();
        assert!(cfg.is_active(Deviation::RefundChargesPlayer));
        cfg.disable(Deviation::RefundChargesPlayer);
        assert!(!cfg.is_active(Deviation::RefundChargesPlayer));
        // and nothing else moved
        assert!(cfg.is_active(Deviation::RefundRepeatCompounding));
        cfg.enable(Deviation::RefundChargesPlayer).unwrap();
        assert!(cfg.is_active(Deviation::RefundChargesPlayer));
    }

    // -- registry hygiene ---------------------------------------------------------------

    #[test]
    fn registry_indices_match_the_enum() {
        assert_eq!(REGISTRY.len(), Deviation::COUNT);
        assert_eq!(Deviation::ALL.len(), Deviation::COUNT);
        for (i, d) in Deviation::ALL.into_iter().enumerate() {
            assert_eq!(d.index(), i, "{d} is at the wrong index");
            assert_eq!(REGISTRY[i].id, d, "registry slot {i} holds the wrong id");
        }
    }

    #[test]
    fn every_entry_carries_its_justification() {
        for d in Deviation::ALL {
            let e = d.entry();
            assert!(!e.slug.is_empty(), "{d}: empty slug");
            assert!(
                e.slug
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'),
                "{d}: slug `{}` is not kebab-case",
                e.slug
            );
            assert!(!e.title.is_empty(), "{d}: no title");
            assert!(e.retail.len() > 40, "{d}: retail behaviour not described");
            assert!(e.ours.len() > 10, "{d}: our behaviour not described");
            assert!(e.why.len() > 40, "{d}: no rationale");
            assert!(
                !e.derived_from.is_empty(),
                "{d}: no derivation — a deviation without an address is folklore"
            );
            assert!(!e.evidence.is_empty(), "{d}: no evidence pointer");
            if e.kind.toggleable() {
                assert!(!e.seam.is_empty(), "{d}: a fix with no seam to consult it");
            } else {
                assert!(
                    !e.default_in_improved,
                    "{d}: non-fix entries cannot default on"
                );
            }
            match (e.kind, e.implementation) {
                (Kind::Fix, ImplementationStatus::Wired | ImplementationStatus::Unwired) => {
                    assert!(!e.surfaces.is_empty(), "{d}: fix has no execution surface");
                }
                (Kind::Drift, ImplementationStatus::KnownDrift) => {
                    assert!(
                        !e.surfaces.is_empty(),
                        "{d}: product drift has no blocking surface"
                    );
                }
                (Kind::Drift, ImplementationStatus::ResearchOnly) => {
                    assert!(
                        e.surfaces.is_empty(),
                        "{d}: research-only model leaked onto a product surface"
                    );
                }
                (Kind::Rejected, ImplementationStatus::NotApplicable) => {
                    assert!(
                        e.surfaces.is_empty(),
                        "{d}: rejected candidate has a surface"
                    );
                }
                pair => panic!("{d}: inconsistent kind/status pair {pair:?}"),
            }
            for surface in e.surfaces {
                assert_ne!(
                    *surface,
                    Surface::ProductRelease,
                    "{d}: product is an aggregate target, not a direct execution surface"
                );
            }
        }
    }

    #[test]
    fn slugs_are_unique_and_round_trip() {
        for (i, a) in Deviation::ALL.into_iter().enumerate() {
            assert_eq!(Deviation::from_slug(a.slug()), Ok(a));
            for b in Deviation::ALL.into_iter().skip(i + 1) {
                assert_ne!(a.slug(), b.slug(), "duplicate slug {}", a.slug());
            }
        }
        assert!(matches!(
            Deviation::from_slug("no-such-thing"),
            Err(ModeError::UnknownSlug(_))
        ));
    }

    // -- environment --------------------------------------------------------------------

    #[test]
    fn env_defaults_to_fidelity() {
        let cfg = ModeConfig::from_env_parts(None, None).unwrap();
        assert!(cfg.is_fidelity());
        assert_eq!(cfg.assert_fidelity(), Ok(()));
        assert!(ModeConfig::from_env_parts(Some(""), Some(""))
            .unwrap()
            .is_fidelity());
    }

    #[test]
    fn env_cannot_smuggle_a_fix_into_fidelity() {
        let e = ModeConfig::from_env_parts(None, Some("+ai-gather-handicap")).unwrap_err();
        assert_eq!(
            e,
            ModeError::FidelityIsImmutable(Deviation::AiGatherHandicap)
        );
        // and a bare slug is treated as `+`, so it is refused the same way
        assert!(ModeConfig::from_env_parts(Some("fidelity"), Some("ai-gather-handicap")).is_err());
    }

    #[test]
    fn env_composes_mode_and_overrides() {
        let cfg = ModeConfig::from_env_parts(
            Some("improved"),
            Some("-ai-gather-handicap, +caravan-heuristic-goal-y"),
        )
        .unwrap();
        assert!(!cfg.is_active(Deviation::AiGatherHandicap));
        assert!(cfg.is_active(Deviation::CaravanHeuristicGoalY));
        assert!(cfg.is_active(Deviation::BhsPrereqResultTest));

        assert!(matches!(
            ModeConfig::from_env_parts(Some("sideways"), None),
            Err(ModeError::UnknownMode(_))
        ));
        // `retail` is a synonym, because someone will type it
        assert!(ModeConfig::from_env_parts(Some("RETAIL"), None)
            .unwrap()
            .is_fidelity());
    }

    // -- behaviour: retail branch is what the derivations say ---------------------------

    #[test]
    fn fidelity_reproduces_the_income_cheat_table() {
        let f = ModeConfig::fidelity();
        assert_eq!(
            (0..6)
                .map(|d| gather_handicap_pct(&f, d))
                .collect::<Vec<_>>(),
            vec![-35, -15, -7, 0, 25, 50]
        );
        // The end-to-end spread the difficulty lane measured: Toughest / Easiest on the
        // same income. 150/65 = 2.307 nominal; the runner measured 2.29 after truncation.
        let easiest = apply_gather_handicap(&f, 1000, 0);
        let toughest = apply_gather_handicap(&f, 1000, 5);
        assert_eq!((easiest, toughest), (650, 1500));
    }

    #[test]
    fn improved_flattens_the_income_cheat() {
        let i = ModeConfig::improved();
        for d in 0..6u8 {
            assert_eq!(gather_handicap_pct(&i, d), 0);
            assert_eq!(apply_gather_handicap(&i, 1000, d), 1000);
        }
    }

    #[test]
    fn the_truncation_is_asymmetric_and_the_fix_is_isolated() {
        let f = ModeConfig::fidelity();
        // A small income is where it bites: 0.65 x 7 = 4.55, retail keeps 4.
        assert_eq!(apply_gather_handicap(&f, 7, 0), 4);
        // Handicap ladder kept, truncation fixed.
        let mut t = ModeConfig::improved_bare();
        t.enable(Deviation::GatherHandicapTruncation).unwrap();
        assert_eq!(
            gather_handicap_pct(&t, 0),
            -35,
            "handicap must be untouched"
        );
        assert_eq!(apply_gather_handicap(&t, 7, 0), 5);
        // Positive handicaps are only rounded down, so the fix moves them less.
        assert_eq!(apply_gather_handicap(&f, 7, 5), 10);
        assert_eq!(apply_gather_handicap(&t, 7, 5), 11);
    }

    #[test]
    fn step20_spins_in_fidelity_and_yields_in_improved() {
        let f = ModeConfig::fidelity();
        // -1 is what an unmet PREQ returns, and retail's `== 0` does not see it.
        assert!(!bhs_order_failed(&f, ORDER_INVALID));
        assert!(bhs_order_failed(&f, ORDER_REFUSED));
        assert!(!bhs_order_failed(&f, 1));

        let i = ModeConfig::improved();
        assert!(bhs_order_failed(&i, ORDER_INVALID));
        assert!(bhs_order_failed(&i, ORDER_REFUSED));
        assert!(!bhs_order_failed(&i, 1));
    }

    #[test]
    fn the_citizens_typo_survives_fidelity() {
        let f = ModeConfig::fidelity();
        assert_eq!(bhs_unit_type_name(&f, "Citizens"), "Citizens");
        let i = ModeConfig::improved();
        assert_eq!(bhs_unit_type_name(&i, "Citizens"), "Citizen");
        // and nothing else is rewritten
        assert_eq!(bhs_unit_type_name(&i, "Fishermen"), "Fishermen");
        assert_eq!(bhs_unit_type_name(&i, "Citizen"), "Citizen");
    }

    #[test]
    fn tikal_reads_hp_in_fidelity_and_borders_in_improved() {
        let f = ModeConfig::fidelity();
        let i = ModeConfig::improved();
        // Shipped data: both 50, so the two modes agree and no replay can tell.
        assert_eq!(tikal_border_percent(&f, 50, 50), 50);
        assert_eq!(tikal_border_percent(&i, 50, 50), 50);
        // A modder who edits the borders constant: retail ignores them, we do not.
        assert_eq!(tikal_border_percent(&f, 90, 50), 50);
        assert_eq!(tikal_border_percent(&i, 90, 50), 90);
    }

    #[test]
    fn the_refund_charges_you_in_fidelity() {
        let f = ModeConfig::fidelity();
        // production.md §3.6: amt 100, TECH_SCIENCE_DISCOUNT 10, same age -> adj 110.
        let r = refund_slot(&f, 100, 110);
        assert_eq!(r.credit, -10, "retail bills you for cancelling");
        assert_eq!(
            r.new_amt, 110,
            "and rewrites the record so a repeat compounds"
        );

        let i = ModeConfig::improved();
        let r = refund_slot(&i, 100, 110);
        assert_eq!(r.credit, 0);
        assert_eq!(r.new_amt, 100);

        // The two halves are separate mistakes and separately toggleable.
        let mut only_sign = ModeConfig::improved_bare();
        only_sign.enable(Deviation::RefundChargesPlayer).unwrap();
        let r = refund_slot(&only_sign, 100, 110);
        assert_eq!((r.credit, r.new_amt), (0, 110));

        let mut only_state = ModeConfig::improved_bare();
        only_state
            .enable(Deviation::RefundRepeatCompounding)
            .unwrap();
        let r = refund_slot(&only_state, 100, 110);
        assert_eq!((r.credit, r.new_amt), (-10, 100));

        // A genuine refund is untouched by either.
        assert_eq!(refund_slot(&f, 100, 60).credit, 40);
        assert_eq!(refund_slot(&i, 100, 60).credit, 40);
    }

    #[test]
    fn the_caravan_heuristic_ignores_child_y_in_fidelity() {
        let f = ModeConfig::fidelity();
        // Two different nodes, same goal: retail gives them the same y term, which is
        // why mode A degenerates to Dijkstra.
        assert_eq!(caravan_h_mode_a_dy(&f, 10, 400), -400);
        assert_eq!(caravan_h_mode_a_dy(&f, 390, 400), -400);

        let mut i = ModeConfig::improved();
        i.enable(Deviation::CaravanHeuristicGoalY).unwrap();
        assert_eq!(caravan_h_mode_a_dy(&i, 10, 400), -390);
        assert_eq!(caravan_h_mode_a_dy(&i, 390, 400), -10);
        // Not on by default even in improved mode — it changes which path is found.
        assert!(!ModeConfig::improved().is_active(Deviation::CaravanHeuristicGoalY));
    }

    #[test]
    fn the_refinery_bonus_is_dead_in_both_modes_by_default() {
        let f = ModeConfig::fidelity();
        let i = ModeConfig::improved();
        assert_eq!(refinery_bonus_pct(&f, 33), 0);
        assert_eq!(refinery_bonus_pct(&i, 33), 0);
        let mut on = ModeConfig::improved_bare();
        on.enable(Deviation::RefineryBonusDead).unwrap();
        assert_eq!(refinery_bonus_pct(&on, 33), 33);
    }

    #[test]
    fn describe_lists_what_is_on() {
        let d = ModeConfig::fidelity().describe();
        assert!(d.starts_with("mode=fidelity active=0"), "{d}");
        assert!(!d.contains('+'));
        let d = ModeConfig::improved().describe();
        assert!(d.contains("ai-gather-handicap"), "{d}");
        assert!(!d.contains("caravan-heuristic-goal-y"), "{d}");
    }
}
