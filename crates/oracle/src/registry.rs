//! **The case registry.** Adding a differential case is editing data in this file.
//!
//! # What a case is
//!
//! A target VA in `riseofnations.exe`, a calling convention, an input plan, and a Rust
//! model to compare against. [`REGISTRY`] holds one [`Case`] per Tier-B claim we can
//! re-run; `run.rs` executes them and emits the machine-readable record.
//!
//! # The rule that makes this worth having
//!
//! **`model` must point at the function we ship**, not at a copy transcribed into the
//! oracle. Before this registry existed, every case in `main.rs` compared retail against
//! an inline lambda: `hash_into_range`'s model was re-typed in `difftest`, `flank_level`'s
//! in `combat_difftest`, and the balance lookup did raw pointer arithmetic rather than
//! calling `don_sim::balance_index`. Those tests could — and can only — tell you the
//! oracle's copy was right. `don-sim` could have drifted arbitrarily and every one of them
//! would still have printed PASS. Where no shipped implementation exists yet, the model
//! comes from `models.rs` and the `model` string says so out loud.
//!
//! # Trial counts
//!
//! The counts below reproduce the numbers quoted in `docs/provenance-ledger.md`, so a
//! clean run regenerates the ledger's evidence column rather than corroborating it from
//! memory. `--scale` multiplies the randomised and swept phases; exhaustive phases never
//! shrink, because a shrunk exhaustive phase is a different claim.

use crate::models;

/// One differential case.
pub struct Case {
    /// Stable identifier. Appears in the JSON and in `--only`; do not rename casually,
    /// the ledger generator keys on it.
    pub id: &'static str,
    /// Target virtual address at the image's preferred base (`0x00400000`).
    pub va: u32,
    /// Calling convention, spelled the way the disassembly justifies it.
    pub abi: &'static str,
    /// Fully-qualified path of the Rust function under test. If it does not start with
    /// `don_`, no shipped code is being tested and the string must say why.
    pub model: &'static str,
    pub subsystem: &'static str,
    /// Heading of the `docs/provenance-ledger.md` entry this case is evidence for.
    pub ledger: &'static str,
    pub derivation: &'static str,
    /// `schema/islands.jsonl` class, or the reason the function is callable anyway.
    pub reachability: &'static str,
    /// What this case does *not* establish. Copied into the JSON so a consumer of the
    /// measurements inherits the caveat with the number.
    pub caveat: &'static str,
    pub plan: Plan,
}

/// A range of column values for a grid phase.
pub enum Cols {
    Range(i32, i32),
    List(&'static [i32]),
}

pub struct Grid {
    pub label: &'static str,
    pub row_lo: i32,
    pub row_hi: i32,
    pub cols: Cols,
}

/// A named input distribution for a two-integer target.
pub struct Phase2 {
    pub dist: Dist2,
    pub description: &'static str,
}

pub enum Dist2 {
    /// Both components uniform over the whole `i32` range.
    Full { count: u32 },
    /// Both components uniform in `[-half, half]`.
    Centered { half: i32, count: u32 },
    /// Both components within `±span` of `center`, with mixed signs — for pinning a
    /// branch whose guard sits at `center`.
    Straddle { center: i32, span: i32, count: u32 },
}

/// How a case drives its target. One variant per calling convention / environment shape;
/// a new case that fits an existing shape needs no new code at all.
pub enum Plan {
    /// `__stdcall(i32, i32, i32, i32) -> i32`, no memory environment.
    Stdcall4 {
        model: fn(i32, i32, i32, i32) -> i32,
        edges: &'static [[i32; 4]],
        random: u32,
        random_distribution: &'static str,
    },
    /// Single argument in ECX, result in EAX. No stack arguments, no environment.
    Ecx1 {
        model: fn(u32) -> u32,
        edges: &'static [u32],
        sweep_start: u32,
        sweep_stride: u32,
        sweep_count: u32,
        sweep_distribution: &'static str,
    },
    /// `__thiscall` against a scratch object. `write_and_model` fills the object from a
    /// pseudo-random word and returns what retail must produce.
    ThiscallScratch {
        write_and_model: fn(*mut u8, u64) -> u32,
        random: u32,
        distribution: &'static str,
    },
    /// `__stdcall(row, col) -> i32` over a table inside the mapped image; the model is
    /// `index(row, col)` followed by a sign-extending load of `elem_bits` at
    /// `table_va + index * (elem_bits/8)`.
    Stdcall2Table {
        table_va: u32,
        elem_bits: u32,
        index: fn(i32, i32) -> i32,
        grids: &'static [Grid],
    },
    /// `Balance::return_modifier` over a **populated** `final_balance_table`.
    ///
    /// [`Plan::Stdcall2Table`] compares retail against a raw read of the *file* image,
    /// which at `0x00C06AFC` is almost entirely zero — so it pins the multiply/add and
    /// says nothing about the contents, and a zero table agrees with any indexing at all.
    /// This variant loads the captured array, writes it into the mapped image at
    /// `table_va`, and compares retail's own read against the **shipped**
    /// `don_sim::balance::BalanceTable`, at raw `TypeIndex` arguments. That is what makes
    /// the 50-row bias a thing the case can fail on.
    BalanceTable {
        /// `Balance::final_balance_table`, the array's real base.
        table_va: u32,
        /// Captured `493x493` `i16` array, relative to the oracle's working directory. A
        /// case whose capture is missing SKIPs; it never falls back to the zero image.
        capture_file: &'static str,
        /// Grids over raw `TypeIndex` values — not zero-based rows.
        grids: &'static [Grid],
    },
    /// `GuyData::turn_speed` with its owning-unit lookup and `Constants` pointer installed
    /// in a private arena. The model is the shipped `groups_guys` method.
    GuyTurnSpeed {
        random: u32,
        distribution: &'static str,
    },
    /// The pure `Guy::turn_angles` solver. Unlike `Guy::turn_towards`, this writes the
    /// proposed angle through an output pointer and never enters `Guy::do_turn`.
    GuyTurnAngles {
        random: u32,
        distribution: &'static str,
    },
    /// Stateful `Guy::turn_towards`, with animation disabled and no extra crew bodies.
    /// Compares the return and the entire 155-byte synchronized `GuyData` range.
    GuyTurnTowards {
        random: u32,
        distribution: &'static str,
    },
    /// The unmodified entry of `Map::make` through the seed writes at `0x0068bcd0`.
    /// The executor replaces the *following* instruction with a jump to the function's
    /// real epilogue, isolating the prefix without pretending to construct the full map.
    MapMakeSeedPrefix {
        random: u32,
        distribution: &'static str,
    },
    /// Complete call-free `Map::fix_diag_land`, with a patterned World/WData
    /// arena and byte-for-byte post-call comparison.
    FixDiagLand {
        random: u32,
        distribution: &'static str,
    },
    /// `WorldData::start_city_wcoord`, with the two World references and its bit plane
    /// installed in a private arena.
    StartCityWcoord {
        random: u32,
        distribution: &'static str,
    },
    /// `World::add_starting_location`, with preallocated World arrays so the
    /// isolated writer never enters the engine allocator.
    AddStartingLocation {
        random: u32,
        distribution: &'static str,
    },
    /// `WorldData::start_city_rad_wcoord`, with parallel city-coordinate arrays
    /// and `Constants::city_center_radius` installed in a private arena.
    StartCityRadWcoord {
        random: u32,
        distribution: &'static str,
    },
    /// `MapFairness::calc_distances`, including its binary32 output and extrema.
    MapFairnessCalcDistances {
        random: u32,
        distribution: &'static str,
    },
    /// Complete `Map::place_start_in_region`, with retail-built circle tables,
    /// fabricated Region/World arrays, and the real game RNG leaf.
    PlaceStartInRegion {
        random: u32,
        distribution: &'static str,
    },
    /// Complete call-free `Map::land_dist`, with retail-built circle tables and a
    /// patterned World/WData arena compared byte-for-byte after every call.
    LandDist {
        random: u32,
        distribution: &'static str,
    },
    /// The damage pipeline. Needs the fabricated world in `damage_env.rs`.
    Damage {
        seeds: &'static [u64],
        trials_per_seed: u32,
        distribution: &'static str,
    },
    /// `Random::next_float` — `__thiscall(this = &u32 state)`, result in xmm0. Compares
    /// **both** the returned float's bit pattern and the resulting state, chained.
    RngNextFloat {
        edge_seeds: &'static [u32],
        random_seeds: u32,
        total_steps: u32,
        distribution: &'static str,
    },
    /// `Random::in_range(lo, hi)` — `__thiscall` + two stdcall dwords, `ret 8`. Needs a
    /// `%fs` base installed (SEH prologue).
    RngInRange {
        edges: &'static [(u32, i32, i32)],
        random: u32,
        /// `false` keeps |bound| <= 0xFFFF so the retail warning branch is never taken;
        /// `true` goes past it and suppresses the once-per-session warning flag.
        wide: bool,
        distribution: &'static str,
    },
    /// `__fastcall(ecx, edx) -> eax`, no memory environment.
    Fastcall2 {
        model: fn(i32, i32) -> i32,
        edges: &'static [(i32, i32)],
        dists: &'static [Phase2],
    },
    /// `__fastcall(ecx = running sum, edx = buffer) + one stack dword length`, **caller
    /// cleans**. The buffer is ours; the model runs over the same bytes.
    Adler32 {
        model: fn(u32, &[u8]) -> u32,
        boundary_lens: &'static [usize],
        bufcap: usize,
        random_cases: u32,
        distribution: &'static str,
    },
    /// `RString::AsScaled(scale)` — `__thiscall`, one stack dword, `ret 4`. Patches two
    /// IAT slots so the routine's CRT leaves are ours.
    AsScaled {
        scales: &'static [i32],
        edges: &'static [&'static str],
        /// Shipped corpus to sweep, relative to the oracle's working directory. A case
        /// whose corpus file is missing is SKIPPED, never silently shortened.
        corpus_file: &'static str,
        generated: u32,
        distribution: &'static str,
    },
}

// ---------------------------------------------------------------------------------
// Input generators for the randomised phases. Kept beside the registry because the input
// distribution is part of the claim: a Tier-B number without its distribution is not a
// measurement, it is a mood.
// ---------------------------------------------------------------------------------

/// `hash_into_range`: `a` and `b` full-width, `lo`/`hi` in ±2^15 so the divisor stays a
/// plausible range width rather than almost always being astronomically large.
pub fn draw_hash_into_range(r: u64, q: u64) -> [i32; 4] {
    [
        r as i32,
        (r >> 32) as i32,
        (q as i32) >> 16,
        ((q >> 32) as i32) >> 16,
    ]
}

// ---------------------------------------------------------------------------------
// The registry.
// ---------------------------------------------------------------------------------

pub static REGISTRY: &[Case] = &[
    Case {
        id: "hash_into_range",
        va: 0x0084_6450,
        abi: "__stdcall(i32,i32,i32,i32) -> i32, ret 0x10. PDB: Doober::get_num(TCoord, \
              TCoord, int, int)",
        model: "don_sim::mechanics::hash_into_range",
        subsystem: "arithmetic (dead code in this image; not the engine RNG)",
        ledger: "§1.1 hash_into_range(a, b, lo, hi) — Doober::get_num",
        derivation: "docs/provenance-ledger.md §1.1; docs/derivation/rng.md",
        reachability: "dead code in this image — 0 rel32 targets and 0 occurrences of the LE \
                       dword 0x00846450 in any section",
        caveat: "Called by nothing in the retail image, so this case keeps the port honest \
                 without making it load-bearing. An unreferenced COMDAT can still be \
                 semantically live, and the function's PURPOSE is underived: the PDB says it \
                 takes two TCoords, so it is a coordinate-keyed number generator for dropped \
                 resources, not a general hash.",
        plan: Plan::Stdcall4 {
            model: don_sim::hash_into_range,
            edges: &[
                [0, 0, 0, 0],
                [1, 1, 0, 1],
                [-1, -1, -5, 5],
                [i32::MAX, 1, 0, 10],
                [i32::MIN, 1, 0, 10],
                [7, -3, -100, 100],
                [123456, 789, 0, 0],
                [5, 5, 10, -10],
            ],
            random: 500_000,
            random_distribution: "xorshift64 (seed per --seed): a,b uniform over i32; \
                                  lo,hi uniform over ±2^15",
        },
    },
    Case {
        id: "flank_level",
        va: 0x0092_CFE0,
        abi: "argument in ECX, result in EAX (11 instructions, preserves EDX). PDB: \
              flanking(unsigned long, unsigned long)",
        model: "don_sim::mechanics::flank_level",
        subsystem: "combat",
        ledger: "§1.3 flank_level(angle_delta) — flanking(u32, u32)",
        derivation: "docs/derivation/combat.md §3 step 20",
        reachability: "ISLAND",
        caveat: "Standalone classification only; whether the caller at 0x00644B3A feeds it \
                 the angle we think it does is pinned separately, inside the damage case. \
                 OPEN (ledger §5.6): the PDB signature takes TWO unsigned longs and this \
                 model takes one, in ECX. Every trial here passed with whatever the second \
                 argument happened to be, so the case does not establish that it is unread.",
        plan: Plan::Ecx1 {
            model: don_sim::flank_level,
            edges: &[
                0,
                1,
                0x3FFF_FFFF,
                0x4000_0000,
                0x4000_0001,
                0x5FFF_FFFF,
                0x6000_0000,
                0x6000_0001,
                0x9FFF_FFFF,
                0xA000_0000,
                0xA000_0001,
                0xD555_5554,
                0xD555_5555,
                0xD555_5556,
                0xFFFF_FFFF,
                0x8000_0000,
                0x7FFF_FFFF,
            ],
            sweep_start: 0,
            sweep_stride: 8191,
            sweep_count: 500_000,
            sweep_distribution: "every boundary the instruction sequence can distinguish, \
                                 plus neighbours; then a stride-8191 walk of the full u32 \
                                 domain (8191 is odd, so the walk never repeats within 2^32)",
        },
    },
    Case {
        id: "balance_accessor",
        va: 0x0058_1CA0,
        abi: "__stdcall(i32 atk_type, i32 def_type) -> i32, ret 8. PDB: \
              Balance::return_modifier(TypeIndex, TypeIndex)",
        model: "don_sim::mechanics::balance_index + sign-extending i16 load",
        subsystem: "combat",
        ledger: "§1.5 balance_index(atk, def) — Balance::return_modifier",
        derivation: "docs/derivation/combat.md §1, §5; ledger §4.5, §5.4",
        reachability: "WRITES_GLOBAL — reads the table through 0x00C06AFC, writes nothing",
        caveat: "Address arithmetic only, and against the FILE image: 0x00C06AFC is a \
                 bias-folded base (ledger §4.5) whose live array is final_balance_table at \
                 0x00C12BF4, so this case pins the multiply/add/movsx and says nothing about \
                 the table's contents. OPEN (ledger §5.4): the live damage hook observed \
                 defender type ids 521, 522 and 526, so the 493x493 grid is not the whole \
                 id domain even though the stride 493 is Tier-B pinned.",
        plan: Plan::Stdcall2Table {
            table_va: 0x00C0_6AFC,
            elem_bits: 16,
            index: don_sim::balance_index,
            grids: &[
                Grid {
                    label: "exhaustive over the 493x493 defined type domain",
                    row_lo: 0,
                    row_hi: 493,
                    cols: Cols::Range(0, 493),
                },
                Grid {
                    label: "rows past the type domain, exercising the multiply/add with no \
                            bounds check",
                    row_lo: 500,
                    row_hi: 1500,
                    cols: Cols::List(&[0, 1, 100, 492]),
                },
            ],
        },
    },
    Case {
        id: "balance_final_table",
        va: 0x0058_1CA0,
        abi: "__stdcall(i32 atk_type, i32 def_type) -> i32, ret 8. PDB: \
              Balance::return_modifier(TypeIndex, TypeIndex). Seven instructions, ECX \
              unread, so the __thiscall receiver is irrelevant",
        model: "don_sim::balance::BalanceTable::get (via don_sim::balance_path::table_index)",
        subsystem: "combat",
        ledger: "§1.5 balance_index(atk, def) — Balance::return_modifier",
        derivation: "docs/assembly/balance-path.md; docs/derivation/combat.md §1, §5",
        reachability: "reads .data through the folded base 0x00C06AFC and writes nothing; \
                       the capture is injected at the real base 0x00C12BF4 before the call",
        caveat: "This case pins the ACCESSOR against real contents, not the CONTENTS \
                 themselves — the array it injects is a live capture, so agreement says \
                 our loader and index reproduce retail's read of the bytes we already had, \
                 not that those bytes are what Balance::fill_tables would produce. \
                 Balance::type_damage 0x0057FB50 and Balance::compute_modifier 0x00581CC0 \
                 remain UNTESTED and unported: they run once at rules-load, inside \
                 Balance::init, and no runtime path reaches them, so a fabricated 543-entry \
                 type array with virtual dispatch would be needed to call them. Trials \
                 outside TypeIndex 50..=542 are EXCLUDED, not passed: retail reads adjacent \
                 .data there and we deliberately model no value for it.",
        plan: Plan::BalanceTable {
            table_va: 0x00C1_2BF4,
            capture_file: "data/balance-real.bin",
            grids: &[
                Grid {
                    label: "exhaustive over the whole balance type domain, TypeIndex \
                            50..=542 (Citizen .. Space Program)",
                    row_lo: 50,
                    row_hi: 543,
                    cols: Cols::Range(50, 543),
                },
                Grid {
                    label: "the guard band below the domain — GoodType ids, which have no \
                            balance row",
                    row_lo: 0,
                    row_hi: 50,
                    cols: Cols::List(&[50, 100, 542]),
                },
                Grid {
                    label: "the guard band above the domain — ItemType ids and beyond",
                    row_lo: 543,
                    row_hi: 600,
                    cols: Cols::List(&[50, 100, 542]),
                },
            ],
        },
    },
    Case {
        id: "guy_turn_speed",
        va: 0x005D_E340,
        abi: "int __thiscall GuyData::turn_speed(int) const; ECX=this, one stack dword, \
              callee `ret 4`. PDB and 202-byte body agree",
        model: "don_sim::systems::groups_guys::GuyData::turn_speed",
        subsystem: "movement / formation facing",
        ledger: "docs/mechanics/groups-guys.md §2.6 — GuyData::turn_speed",
        derivation: "docs/mechanics/groups-guys.md §2.6; re/decomp-all/005de340.c; \
                     PDB GuyData::turn_speed @0x005DE340",
        reachability: "SELF-CALL only into mapped data: reads GuyData, \
                       objects.lists[who][o], UnitData+0x18/+0x68, \
                       UnitTypeData+0x2C4/+0x304 and Constants+8/+0xC",
        caveat: "The object graph and Constants pointer are fabricated, so this pins the \
                 shipped arithmetic and exact field/width reads, not the live values fed \
                 by a match. The damped form performs an unchecked `div`; generated \
                 avg_speed values -7..=-4 make its divisor zero and are EXCLUDED and \
                 counted rather than crashing the case. Signed owner/object indexes \
                 outside their live domains are not generated.",
        plan: Plan::GuyTurnSpeed {
            random: 250_000,
            distribution: "branch-biased xorshift64: squad/crew boundary; raw/damped; \
                           tracking and fast-face early returns; both rule scales; \
                           wrapping i32 rule products; avg_speed division/floor boundaries",
        },
    },
    Case {
        id: "guy_turn_angles",
        va: 0x005D_98C0,
        abi: "unsigned long __thiscall Guy::turn_angles(unsigned long desired, \
              unsigned long *out, int raw_step, int half); ECX=this, ret 0x10. PDB and \
              134-byte body agree",
        model: "don_sim::systems::groups_guys::GuyData::turn_angles",
        subsystem: "movement / formation facing",
        ledger: "docs/mechanics/groups-guys.md §2.6 — Guy::turn_angles/turn_towards solver",
        derivation: "docs/mechanics/groups-guys.md §2.6; re/decomp-all/005d98c0.c; \
                     PDB Guy::turn_angles @0x005D98C0",
        reachability: "SELF-CALL only to GuyData::turn_speed; writes only the caller-owned \
                       output dword. Chosen instead of turn_towards because the latter \
                       enters the side-effecting Guy::do_turn tail",
        caveat: "This pins the pure solver shared with turn_towards: the 0x02222220 snap, \
                 `not` magnitude fold, shortest-direction choice, wrap, remainder and \
                 optional half-step. Retail's third argument is constrained to 1 because \
                 the shipped Rust `turn_angles` API represents the live raw-step call \
                 shape. It does NOT cover Guy::do_turn, pivots, animation, or writing the \
                 live Guy angle.",
        plan: Plan::GuyTurnAngles {
            random: 250_000,
            distribution: "branch-biased xorshift64: exact snap neighbours; 0x80000000 \
                           direction tie; clockwise/counter-clockwise wrap; step equality \
                           and one-past; full/half step; the turn_speed rule boundaries",
        },
    },
    Case {
        id: "guy_turn_towards",
        va: 0x005D_9720,
        abi: "unsigned long __thiscall Guy::turn_towards(unsigned long desired, int unread, \
              int animate); ECX=this, ret 0x0C. PDB plus the 116-byte body establish all \
              three stack slots; the middle slot is unread",
        model: "don_sim::systems::groups_guys::GuyData::turn_towards plus GuyData::walk_bytes",
        subsystem: "movement / formation facing",
        ledger: "docs/mechanics/groups-guys.md §2.6 — stateful Guy::turn_towards/do_turn",
        derivation: "docs/mechanics/groups-guys.md §2.6; re/decomp-all/005d9720.c, \
                     005d97a0.c, 005d9010.c; PDB Guy::turn_towards/Guy::do_turn/Guy::set_angle",
        reachability: "SELF-CALL chain turn_towards -> do_turn -> set_angle -> turn_speed. \
                       The fabricated unit sets total_guy_count == squad_size, so the \
                       precisely decoded crew-recursion loops are empty; animate=0 keeps \
                       the 4,723-byte Guy::set_anim branch closed",
        caveat: "This compares the return and every byte of GuyData's synchronized \
                 [this+8,this+0xA3) range, including the `guy_flags |= 2` side effect that \
                 the solver-only port previously omitted. Domain is exact and deliberate: \
                 owner 0..7, object index 0..3, nonnegative squad size, no extra crew, and \
                 animate=0. It does NOT cover pivot animation (`guy_flags & 8`, animate!=0) \
                 or recursive propagation into separately allocated crew Guys; those \
                 require the animation packet graph and a multi-Guy fixture, respectively.",
        plan: Plan::GuyTurnTowards {
            random: 250_000,
            distribution: "the turn_angles boundary distribution plus leader/nonleader and \
                           squad/crew bodies; the ABI's unread middle argument varies across \
                           zero and arbitrary nonzero dwords; full post-call GuyData image \
                           compared byte-for-byte",
        },
    },
    Case {
        id: "map_make_seed_prefix",
        va: 0x0068_BC90,
        abi: "void __thiscall Map::make(int map_arg, int seed, int mode), ret 0x0C. PDB; \
              the tested prefix is entry..0x0068bcd0 inclusive",
        model: "don_sim::systems::map_terrain::World::seed_map_generation",
        subsystem: "world generation / deterministic seeding",
        ledger: "docs/mechanics/map-terrain.md §7 — deterministic map generation seed",
        derivation: "docs/mechanics/map-terrain.md §7 and §7.1; \
                     docs/derivation/rng.md §6; PDB Map::make",
        reachability: "Map::make is the common virtual map driver in 21 map-style vtables. \
                       Its entry prefix has no calls and touches only Map+0x110, \
                       World+0x7C, game_random+0, and the SEH chain",
        caveat: "This executes the retail bytes from Map::make entry through 0x0068bcd0, \
                 then a case-local five-byte jump at the original 0x0068bcd2 instruction \
                 reaches the function's unmodified epilogue at 0x0068c84a. It proves the \
                 signed seed gate and the two state writes; it does NOT execute terrain \
                 creation, consume the RNG, select an orientation, or place starts. Full \
                 construction remains an explicitly recorded gap.",
        plan: Plan::MapMakeSeedPrefix {
            random: 100_000,
            distribution: "seven signed-gate/map-argument edges, then xorshift64: seed and \
                           initial World/RNG/map words uniform over all 32-bit patterns; \
                           exact positive/negative branch counts reported",
        },
    },
    Case {
        id: "fix_diag_land",
        va: 0x0069_C250,
        abi: "void __cdecl Map::fix_diag_land(); no arguments",
        model: "don_sim::systems::map_terrain::World::fix_diag_land",
        subsystem: "world generation / post-continent diagonal repair",
        ledger: "docs/mechanics/map-terrain.md §7.1 — executable terrain mutation",
        derivation: "docs/mechanics/map-terrain.md §7.1; PDB Map::fix_diag_land; \
                     retail 0x0069c250..0x0069c457",
        reachability: "Map::make calls this common static routine immediately after the \
                       selected map-style make_continents hook, except for map style 23. \
                       The complete 536-byte routine is call-free and touches only the \
                       World dimensions and WData land/land_sub bytes",
        caveat: "Valid generated-map domain: positive dimensions 1..32 and WData.land \
                 values 0..2. The fixture patterns the complete 372-byte World and every \
                 28-byte WData record, installs only xs/ys and the measured +0x134 WData \
                 pointer, then compares every fabricated byte. This proves the in-place \
                 x-major NW/NE/SE/SW repair, including the 16-bit land/subtype store. It \
                 does not generate the continent plane supplied to the routine.",
        plan: Plan::FixDiagLand {
            random: 100_000,
            distribution: "four corner orientations, off-map/single-cell and x-major \
                           cascade edges, then xorshift64 worlds 1..32 with dry, sparse-water, \
                           dense-water, and uniform 0..2 land planes; complete patterned \
                           World/WData post-state compared byte-for-byte",
        },
    },
    Case {
        id: "start_city_wcoord",
        va: 0x006B_30E0,
        abi: "int __thiscall WorldData::start_city_wcoord(WCoord const& x, WCoord const& y), \
              ret 8; ECX is unread",
        model: "don_sim::systems::map_terrain::World::start_city_wcoord",
        subsystem: "world generation / starting-position occupancy",
        ledger: "docs/mechanics/map-terrain.md §7.1 — executable start-placement foothold",
        derivation: "docs/mechanics/map-terrain.md §7.1; PDB \
                     WorldData::start_city_wcoord; retail 0x006b30e0..0x006b311d",
        reachability: "64-byte pure leaf once GameAccessConst::worldc, GameAccess::world, \
                       and World::start_city_locs are installed; no calls or allocations",
        caveat: "Valid-domain claim only: width 1..512, height 1..128, and x/y in bounds. \
                 Retail performs no bounds check. This proves exact row-major flattening \
                 and LSB-first bit order for the start-city occupancy plane, not how \
                 Map::place_start_in_region chooses a coordinate and not ring/radius \
                 exclusion semantics.",
        plan: Plan::StartCityWcoord {
            random: 250_000,
            distribution: "byte/row boundary edges plus xorshift64 widths 1..512, heights \
                           1..128, valid coordinates, and uniform selected-byte patterns",
        },
    },
    Case {
        id: "add_starting_location",
        va: 0x006B_2DE0,
        abi: "int __thiscall World::add_starting_location(WCoord const& x, \
              WCoord const& y), ret 8; ECX is unread",
        model: "don_sim::systems::map_terrain::World::add_starting_location",
        subsystem: "world generation / starting-position writer",
        ledger: "docs/mechanics/map-terrain.md §7.1 — executable start-placement writer",
        derivation: "docs/mechanics/map-terrain.md §7.1; PDB \
                     World::add_starting_location; retail 0x006b2de0..0x006b3019",
        reachability: "Map generators call the common World writer after selecting a start. \
                       The fixture installs World+0x80/+0x9c/+0xb8/+0xd4 arrays with \
                       measured metadata and spare capacity, keeping all allocator calls \
                       untaken while executing every append and occupancy-bit write",
        caveat: "This proves the returned start index, four array append sequences, and \
                 all four row-major LSB-first occupancy writes. The fixture deliberately \
                 preallocates capacity, so array-growth policy is established separately \
                 from SimpleArray::increase_size disassembly and sim tests. It does not \
                 choose coordinates or approximate Map::make.",
        plan: Plan::AddStartingLocation {
            random: 100_000,
            distribution: "byte/row-boundary coordinate sequences plus xorshift64 worlds \
                           2..128 cells per axis, 1..8 valid appended starts per fixture; \
                           return index, walked array metadata/elements, and full bit plane \
                           compared after every append",
        },
    },
    Case {
        id: "start_city_rad_wcoord",
        va: 0x006B_3850,
        abi: "int __thiscall WorldData::start_city_rad_wcoord(WCoord const& x, \
              WCoord const& y), ret 8; ECX is unread",
        model: "don_sim::systems::map_terrain::World::start_city_rad_wcoord",
        subsystem: "world generation / starting-position exclusion radius",
        ledger: "docs/mechanics/map-terrain.md §7.1 — executable start-placement radius",
        derivation: "docs/mechanics/map-terrain.md §7.1; PDB \
                     WorldData::start_city_rad_wcoord; retail 0x006b3850..0x006b3952",
        reachability: "Common world query used by map placement and resource-location \
                       searches. The fixture installs only World start-city arrays and \
                       Constants::city_center_radius; the 259-byte routine is call-free",
        caveat: "Valid generated-map domain: parallel start-city arrays, coordinates \
                 0..127, and city_center_radius 0..1024. This proves the complete scan, integer \
                 distance, WCoord-to-tile factor four, minus-one adjustment and strict \
                 comparison. It does not choose starts or place terrain/resources.",
        plan: Plan::StartCityRadWcoord {
            random: 250_000,
            distribution: "empty/one/many array edges and exact threshold neighbours, \
                           then xorshift64 parallel arrays of 0..32 coordinates in \
                           0..127, query coordinates 0..127, city_center_radius 0..1024",
        },
    },
    Case {
        id: "map_fairness_calc_distances",
        va: 0x0068_A1C0,
        abi: "void __thiscall MapFairness::calc_distances(WCoord const& x, \
              WCoord const& y, float scale), ret 8; scale is passed in XMM3",
        model: "don_sim::systems::map_terrain::MapFairness::calc_distances",
        subsystem: "world generation / starting-position fairness",
        ledger: "docs/mechanics/map-terrain.md §7.1 — executable start fairness scorer",
        derivation: "docs/mechanics/map-terrain.md §7.1; PDB \
                     MapFairness::calc_distances; retail 0x0068a1c0..0x0068a2da",
        reachability: "Called by map-style placement loops after candidates exist. The \
                       fixture installs World start arrays; the complete routine is \
                       call-free and writes only MapFairness::dists/lowest_dist/highest_dist",
        caveat: "Valid generated-map domain: 0..8 players, valid team/start indices, \
                 coordinates 0..127, and finite nonnegative binary32 scales. This proves \
                 the full 120-byte post-call object: exact float output bits, strict \
                 extrema updates, and preservation of every other patterned byte. It does \
                 not select a candidate or generate terrain.",
        plan: Plan::MapFairnessCalcDistances {
            random: 250_000,
            distribution: "empty/tie/zero extrema edges plus xorshift64 0..8 start arrays, \
                           shuffled valid team indices, 0..127 coordinates, and finite \
                           nonnegative f32 scales drawn across exponent/mantissa bits",
        },
    },
    Case {
        id: "place_start_in_region",
        va: 0x0068_AC00,
        abi: "int __thiscall Map::place_start_in_region(int region, WCoord* out_x, \
              WCoord* out_y, int min_dist, int unread, SimpleArray<WCoord>* prior_x, \
              SimpleArray<WCoord>* prior_y), ret 0x1c",
        model: "don_sim::systems::map_terrain::place_start_in_region",
        subsystem: "world generation / concrete starting-position selection",
        ledger: "docs/mechanics/map-terrain.md §7.1 — executable region-start selector",
        derivation: "docs/mechanics/map-terrain.md §7.1; PDB \
                     Map::place_start_in_region; retail 0x0068ac00..0x0068ae49 plus \
                     Map::is_near_ocean 0x0068b0a0..0x0068b1dc",
        reachability: "Called by map-style start-placement loops. The fixture executes \
                       retail circle_init first, then installs a measured Regions -> \
                       ObjectArray<Region> -> WCoordList graph, World WData/start arrays, \
                       and game_random. No allocator or substituted call is entered",
        caveat: "Valid generator-domain claim: one non-empty Region coordinate list, \
                 parallel prior-start arrays, worlds 16..32 cells per axis, and WData.land \
                 values 0..2. The full 592-byte selector and its complete 317-byte \
                 is_near_ocean callee execute. The case compares return/out coordinates, \
                 final RNG state, and every byte of the patterned fabricated object arena. \
                 It proves selection from supplied region/world inputs, not how Map::make \
                 generated those inputs.",
        plan: Plan::PlaceStartInRegion {
            random: 100_000,
            distribution: "dry/ocean/exact coastal-band and margin/spacing edges, then \
                           xorshift64 worlds 16..32, 1..16 valid Region coordinates, sparse \
                           or dense WData.land, 0..8 default or optional prior starts, \
                           min_dist 0..24, arbitrary unread argument and RNG seed",
        },
    },
    Case {
        id: "land_dist",
        va: 0x0069_D970,
        abi: "int __thiscall Map::land_dist(WCoord x, WCoord y, int edge_is_land), ret 0x0c. \
              The PDB marks it virtual; the 351-byte body never reads ECX, and both WCoords \
              arrive as by-value dwords at [ebp+8] / [ebp+0xc], not as references",
        model: "don_sim::systems::map_terrain::World::land_dist",
        subsystem: "world generation / continent and lake spacing",
        ledger: "docs/mechanics/map-terrain.md §7.2 — executable ring-distance-to-land leaf",
        derivation: "docs/assembly/map-great-lakes-continents.md §1; \
                     docs/mechanics/map-terrain.md §7.2; PDB Map::land_dist; \
                     retail 0x0069d970..0x0069dacc",
        reachability: "Call-free leaf. It reads only GameAccess::world (xs at +0x00, ys at \
                       +0x04, wdata at +0x134) and the canonical circle tables at \
                       0x00cb7e90 / 0x00cbb0e0 / 0x00cbe330, which the fixture fills by \
                       executing retail circle_init 0x006817f0 rather than installing a \
                       convenient ring. MapGreatLakes::make_continents applies it as the \
                       lake-seed spacing test at 0x0069a255",
        caveat: "Valid generated-map domain: an IN-BOUNDS origin. Retail bounds-checks \
                 nothing at the origin — it indexes wdata[y*xs + x] directly — so \
                 out-of-range origins are not generated and no policy is claimed for them. \
                 Worlds are 1..32 cells per axis in the randomised phase and 140x140 in the \
                 saturation edges, so both the off-map arm and the pure `d > 0x40 -> 0x41` \
                 exit execute. This proves the origin is_ocean gate (WATERHALF before the \
                 land byte), the half-open ring span retail derives from one table read at \
                 0xcbe32c and 0xcbe330, the octagon ring order, the off-map arm's dependence \
                 on the third argument, and that the leaf writes nothing. It does NOT \
                 establish how make_continents chooses candidates, what threshold it \
                 compares the distance against, or that any generated world reaches this \
                 leaf: the WData plane is fabricated. The shipped Rust takes a `bool` third \
                 argument while retail takes an int, so the case supplies arbitrary nonzero \
                 dwords and reports the branch split rather than assuming they are equivalent.",
        plan: Plan::LandDist {
            random: 100_000,
            distribution: "39 edges (origin is_ocean incl. both WATERHALF halves and \
                           out-of-enum land bytes; octagon-vs-Euclid ring membership; the \
                           first and last entry of rings 1, 2, 5, 17 and 64; a WATERHALF \
                           offset terminating a deep-water cell; six shapes of the third \
                           argument; interior saturation on a 140x140 world; land at octagon \
                           distance 65), then xorshift64 worlds 1..32 per axis with all-ocean, \
                           sparse-land, half-land, uniform-i8 and all-dry planes, WATERHALF \
                           painted at 0, 1/32 and 1/4 density, uniform random flag words, \
                           in-bounds origins biased 3:1 onto ocean, and the third argument \
                           zero one time in four and an arbitrary dword otherwise",
        },
    },
    Case {
        id: "accessor_movsx_word_0xa",
        va: 0x0047_2400,
        abi: "__thiscall, no stack args (movsx eax, word ptr [ecx+0xA])",
        model: "oracle::models::accessor_movsx_0xa — shape probe, no don-sim counterpart",
        subsystem: "harness self-evidence",
        ledger: "(none — this is a mechanism check, not a mechanic)",
        derivation: "crates/oracle/src/main.rs, original difftest",
        reachability: "ISLAND",
        caveat: "Proves the harness reproduces __thiscall field reads. It is not evidence \
                 about any shipped Rust.",
        plan: Plan::ThiscallScratch {
            write_and_model: models::accessor_movsx_0xa,
            random: 100_000,
            distribution: "xorshift64; low 16 bits written to obj+0xA, so uniform over u16",
        },
    },
    Case {
        id: "accessor_diff_0x12c_0x12a",
        va: 0x0048_F770,
        abi: "__thiscall, no stack args ([this+0x12C] - [this+0x12A], both i16)",
        model: "oracle::models::accessor_diff_12c_12a — shape probe, no don-sim counterpart",
        subsystem: "harness self-evidence",
        ledger: "(none — this is a mechanism check, not a mechanic)",
        derivation: "crates/oracle/src/main.rs, original difftest",
        reachability: "ISLAND",
        caveat: "As above: mechanism, not mechanic.",
        plan: Plan::ThiscallScratch {
            write_and_model: models::accessor_diff_12c_12a,
            random: 100_000,
            distribution: "xorshift64; two independent uniform u16 fields",
        },
    },
    Case {
        id: "damage_pipeline",
        va: 0x0064_4130,
        abi: "__thiscall + six stack dwords, ret 0x18. PDB: ObjectData::get_damage",
        model: "don_sim::mechanics::damage_traced",
        subsystem: "combat",
        ledger: "§1.2 damage() — ObjectData::get_damage, 0x00644130",
        derivation: "docs/derivation/damage-port.md; docs/derivation/combat.md §3",
        reachability: "SELF_CALL — not callable with fabricated bytes; damage_env.rs builds \
                       the world (two objects, four vtables, RULES, game, players, map, both \
                       object tables, city table) so every predicate becomes a settable dword",
        caveat: "Steps 10, 11 and 27 and the get_attack/get_armor upgrade branches are \
                 structurally unreachable here and remain UNVERIFIED. The ~30 object-graph \
                 predicates are inputs, not derivations. out_kind is written by retail and \
                 never compared: '0 mismatches' is about the return value only. \
                 REFUTED ASSUMPTION (ledger §4.3): the harness aliases [0x00C0AB84] and \
                 [0x00C0AEC0], and a live capture shows they do NOT alias — they agree on \
                 every unit defender and differ on every building defender. That does not \
                 disturb the trial count; it BOUNDS which inputs it covers, excluding \
                 building defenders. REFUTED ASSUMPTION (ledger §4.4): attack/armor are \
                 modelled as the base-class getters 0x006469F0/0x00647DB0, which live combat \
                 dispatched to zero times in 56,789 captured calls.",
        plan: Plan::Damage {
            seeds: &[
                0x2545_F491_4F6C_DD1D,
                0x9E37_79B9_7F4A_7C15,
                0x0000_0000_0000_0001,
                0xDEAD_BEEF_CAFE_BABE,
                0x1234_5678_9ABC_DEF0,
                0xFFFF_FFFF_FFFF_FFFF,
                0x0F0F_0F0F_F0F0_F0F0,
                0xA5A5_5A5A_C3C3_3C3C,
            ],
            // 8 x 1,000,000 reproduces the ledger's 7,986,695 counted trials (the balance
            // between the two is the excluded #DE and collision cases, both reported).
            trials_per_seed: 1_000_000,
            distribution: "numerics from a mixture (50% ±100, 25% ±10k, 12.5% full i32, \
                           12.5% boundary constants); balance_pct uniform i16; angles biased \
                           onto the eleven classifier boundaries; masks biased onto the \
                           fourteen bit patterns the code tests; type ids drawn to reach the \
                           three id-keyed guards; all 26 stubbed predicates fair coins",
        },
    },
    Case {
        id: "rng_next_float",
        va: 0x00A3_9CF0,
        abi: "__thiscall(this = &u32 state), f32 result in xmm0",
        model: "oracle::models::rng::next_float — NO don-sim implementation exists",
        subsystem: "rng",
        ledger: "§2.1 Random::get() / Random::get(lo, hi) — the engine RNG",
        derivation: "docs/derivation/rng.md §2; ledger §2.1. PDB: ?get@Random@@QAEMXZ",
        reachability: "reads and writes only *(u32*)this — a 4-byte scratch is a complete \
                       environment",
        caveat: "Ledger §2.1 is explicit that there is NO implementation of this in the \
                 repo — the model exists only in the oracle. So this case tests a \
                 transcription and nothing the simulation will ever run. Half-open [lo, hi), \
                 and lo == hi returns lo WITHOUT advancing the state, which is a determinism \
                 hazard for a whole stream rather than one draw.",
        plan: Plan::RngNextFloat {
            edge_seeds: &[0, 1, 0xffff_ffff, 0x8000_0000, 0x7fff_ffff, 0x3c6e_f35f],
            random_seeds: 64,
            total_steps: 999_950,
            distribution: "six edge seeds (0 is what every statically-constructed Random \
                           starts at) plus 64 random seeds, each walked forward; compares the \
                           f32 bit pattern and the resulting state at every step",
        },
    },
    Case {
        id: "rng_in_range",
        va: 0x00A3_9D70,
        abi: "__thiscall(this = &u32 state) + 2 stdcall dwords, ret 8 (SEH prologue)",
        model: "oracle::models::rng::in_range — NO don-sim implementation exists",
        subsystem: "rng",
        ledger: "§2.1 Random::get() / Random::get(lo, hi) — the engine RNG",
        derivation: "docs/derivation/rng.md §2",
        reachability: "same 4-byte state; needs a %fs base because the prologue writes fs:[0]",
        caveat: "Bounds kept inside ±0xFFFF so the retail warning branch at 0x00A39DB7 — \
                 which calls into unconstructed globals — is never taken. See \
                 rng_in_range_wide for the other side. The SEH frame is installed but never \
                 unwound: no exception is raised in any trial.",
        plan: Plan::RngInRange {
            edges: &[
                (0, 0, 0),
                (0, 0, 1),
                (0, 1, 0),
                (0, 0, 0xffff),
                (0, -0xffff, 0xffff),
                (0, -5, 5),
                (0, 5, -5),
                (0xffff_ffff, 0, 100),
                (0x3c6e_f35f, -1, 1),
                (12345, 0xffff, 0xffff),
                (12345, 0xffff, 0),
                (99, -32768, 32767),
            ],
            random: 1_000_000,
            wide: false,
            distribution: "state uniform over u32; lo,hi uniform over ±0xFFFF; compares the \
                           return value and the resulting state",
        },
    },
    Case {
        id: "rng_in_range_wide",
        va: 0x00A3_9D70,
        abi: "__thiscall(this = &u32 state) + 2 stdcall dwords, ret 8 (SEH prologue)",
        model: "oracle::models::rng::in_range — NO don-sim implementation exists",
        subsystem: "rng",
        ledger: "§2.1 Random::get() / Random::get(lo, hi) — the engine RNG",
        derivation: "docs/derivation/rng.md §2",
        reachability: "as above, plus the once-per-session warning flag at 0x00EE13A8 forced \
                       to 1 so the arithmetic path runs without touching live globals",
        caveat: "The warning flag is *forced*, so this case establishes what the arithmetic \
                 does past 0xFFFF, not what a real session does the first time it happens.",
        plan: Plan::RngInRange {
            edges: &[
                (1, 0, 1_000_000),
                (1, 0, 100_000),
                (1, -1_000_000, 1_000_000),
            ],
            random: 500_000,
            wide: true,
            distribution: "state uniform over u32; lo,hi uniform over ±2^29",
        },
    },
    Case {
        id: "vector_dist",
        va: 0x0046_CFF0,
        abi: "__fastcall(ecx, edx) -> eax (105 bytes)",
        model: "oracle::models::vector_dist — NO implementation in this repo (ledger §2.2)",
        subsystem: "pathfinding / geometry",
        ledger: "§2.2 vector_dist(a, b) — the engine's integer distance kernel",
        derivation: "docs/derivation/pathfinding.md; PDB ?vector_dist@@YAHHH@Z",
        reachability: "ISLAND, called from 105 distinct functions engine-wide",
        caveat: "Ledger §7.4 recorded this harness as living ONLY at hbox:~/lane-pathfinding, \
                 so the result would not survive that box being rebuilt. The model is now \
                 in-tree — but there is still no don-sim implementation, so this tests a \
                 transcription and not anything the simulation runs.",
        plan: Plan::Fastcall2 {
            model: models::vector_dist,
            edges: &[
                (0, 0),
                (0, 1),
                (1, 0),
                (1, 1),
                (-1, -1),
                (1, -1),
                (-1, 1),
                (59999, 59999),
                (60000, 59999),
                (59999, 60000),
                (60000, 60000),
                (0xEA5F, 0xEA5F),
                (0xEA60, 1),
                (1, 0xEA60),
                (i32::MAX, i32::MAX),
                (i32::MAX, 0),
                (0, i32::MAX),
                (i32::MIN, 0),
                (0, i32::MIN),
                (i32::MIN, i32::MIN),
                (i32::MIN, i32::MAX),
                (i32::MAX, i32::MIN),
                (i32::MIN, 1),
                (1, i32::MIN),
                (-60000, -60000),
                (192, 0),
            ],
            dists: &[
                Phase2 {
                    dist: Dist2::Full { count: 2_000_000 },
                    description: "uniform 32-bit pairs — stresses the overflow guard and the \
                                  wrapping abs",
                },
                Phase2 {
                    dist: Dist2::Centered {
                        half: 65_536,
                        count: 1_000_000,
                    },
                    description: "realistic map deltas: RoN world coords step 192 per A* cell \
                                  and a big map is a few hundred cells across, so |d| ≲ 40k",
                },
                Phase2 {
                    dist: Dist2::Straddle {
                        center: 60_000,
                        span: 31,
                        count: 1_000_000,
                    },
                    description: "straddling the 0xEA60 guard exactly, both signs",
                },
            ],
        },
    },
    Case {
        id: "adler32",
        va: 0x00A4_6830,
        abi: "__fastcall(ecx = sum, edx = buf) + one stack dword len; CALLER cleans (`ret`)",
        model: "don_sim::checksum::adler32 — the shipped primitive, not a copy",
        subsystem: "determinism / lockstep checksum",
        ledger: "§2.3 adler32(adler, buf, len) — the lockstep checksum primitive",
        derivation: "docs/derivation/checksum.md; PDB ?adler32@@YAKKPBEK@Z",
        reachability: "leaf; reads only the buffer it is handed",
        caveat: "Ledger §7.4 recorded this harness as living ONLY at \
                 hbox:~/don-oracle-checksum. Default case count here is 100,000 rather than \
                 the historical 500,000 so the suite stays runnable; `--scale 5` reproduces \
                 the original count. REPOINTED 2026-08-08 by the world-channel lane: the \
                 model used to be `oracle::models::adler32`, a copy transcribed into the \
                 oracle, so the case proved the copy. It is now the single implementation \
                 every checksum walker in the workspace calls, which is what makes this \
                 case evidence about shipped code. Note the binary holds a second, \
                 structurally identical `_adler32` at 0x005089d0; the checksum path calls \
                 THIS one (`CheckSum::walk_function` 0x00936ff0 is `call 0xa46830`).",
        plan: Plan::Adler32 {
            model: don_sim::checksum::adler32,
            // The two structural boundaries in the routine: the 16-byte unrolled block and
            // NMAX = 5552. Uniform lengths alone would straddle neither reliably.
            boundary_lens: &[0, 1, 2, 15, 16, 17, 31, 5551, 5552, 5553, 11104, 11105],
            bufcap: 24576,
            random_cases: 100_000,
            distribution: "12 boundary lengths, then uniform lengths in [0, 24576] with \
                           uniform random bytes and a uniform random 32-bit initial value; \
                           plus the buf == NULL case, which the disassembly says returns 1",
        },
    },
    Case {
        id: "rules_as_scaled",
        va: 0x00A1_D110,
        abi: "__thiscall(this = String) + one stack dword, ret 4. PDB: String::fraction(int)",
        model: "don_rules::value::as_scaled",
        subsystem: "rules",
        ledger: "§1.7 as_scaled / as_int / wtoi — the rule-value tokenizer",
        derivation: "docs/derivation/economy.md §2.2; ledger §1.7",
        reachability: "callable once its two CRT imports are replaced: IAT slots 0x00AC54AC \
                       (_wtoi) and 0x00AC5434 (wcschr) are patched to our own leaves",
        caveat: "Conditional on the two substituted CRT leaves. The claim covers the slash \
                 search, the 32-bit multiply, the truncating divide and the zero-denominator \
                 early-out — NOT _wtoi itself. Overflowing digit strings have never been \
                 compared against MSVC's CRT. The engine parses UTF-16 and don_rules takes \
                 &str; they agree on ASCII, which is all the shipped corpus contains. \
                 wcschr searches the WHOLE string, so a '/' in trailing prose would open a \
                 denominator in a scaled field; no shipped value does, but a mod could.",
        plan: Plan::AsScaled {
            scales: &[192, 256, 100],
            edges: &[
                "",
                " ",
                "/",
                "0/0",
                "1/0",
                "0/16",
                "/16",
                "5/",
                "-3/4",
                "3/-4",
                "  7  /  2 ",
                "1/16 tile",
                "1/192 tile",
                "6/5 base rate",
                "2/3",
                "2/1",
                "12/10",
                "80/100",
                "+5/+2",
                "2147483647/1",
                "1/2147483647",
                "-2147483648/1",
                "abc",
                "abc/2",
                "1/2/3",
                "10 resources",
                "450 frames",
                "50%",
                "0",
                "-1",
                "99999999/7",
            ],
            corpus_file: "data/rules.xml",
            generated: 200_000,
            distribution: "every value=\"…\" and entryN=\"…\" string in the shipped rules.xml \
                           at all three scales, plus 31 hand-chosen edges at all three scales, \
                           plus generated num[/den][prose] with num ∈ [-2000,2000], \
                           den ∈ [-200,200], eight prose tails; INT_MIN/-1 excluded as a \
                           retail #DE",
        },
    },
];
/// Tier-B claims in `docs/provenance-ledger.md` that this suite **cannot** re-run.
///
/// They are listed here, and copied into the JSON, so the ledger generator can mark those
/// rows as not-re-run instead of leaving a reader to assume a green suite covered them.
/// An unregistered claim that nobody records is indistinguishable from a passing one.
pub struct Gap {
    pub claim: &'static str,
    pub why: &'static str,
}

pub static KNOWN_GAPS: &[Gap] = &[
    Gap {
        claim: "Full seeded Map::make terrain and start-position equivalence",
        why: "Map::make is a 3,021-byte virtual orchestration routine that immediately \
              needs the selected one of 21 map-style objects, GameInfo, Rules/Constants, \
              RString leaves, engine arrays and allocators. The registry executes and \
              proves its seed prefix, common post-continent diagonal repair, start-city \
              representation/writer/radius, fairness scorer, and complete Region-start \
              selector. The terrain repair still receives a fabricated WData land \
              plane, and the selector also receives a fabricated Region coordinate \
              list; the registry does \
              not substitute approximate continent or Region construction. \
              docs/mechanics/map-terrain.md §7.1 records the executable dependency plan \
              for extending this boundary.",
    },
    Gap {
        claim: "§1.4 entrench_dir_level — the entrenchment direction classifier",
        why: "Inlined at 0x00644E0E–0x00644E2D inside ObjectData::get_damage; there is no \
              standalone function to call. It runs only indirectly, inside damage_pipeline \
              (642,609 of 7,986,695 trials), and its boundaries are pinned by mutation rather \
              than by a direct differential.",
    },
    Gap {
        claim: "§1.6 get_attack / get_armor — upgrade branches, and live dispatch",
        why: "The base paths run as real retail code inside damage_pipeline. The upgrade \
              branches need LeaderData::has_tribe_bonus(0x16) to return true, and the \
              fabricated world sets game[+0x20] |= 4 to force it false — never executed. \
              Worse (ledger §4.4): over 56,789 live damage calls the vtable dispatched to \
              these two base-class leaves ZERO times, so even the tested path is not the one \
              live combat takes.",
    },
    Gap {
        claim: "§1.8 Rules — the typed rules block (828/834 constants matched live memory)",
        why: "Validated against a running process on the Parallels VM, not against the oracle. \
              Re-running it needs the live game, so it is outside this harness by construction.",
    },
    Gap {
        claim: "§1.10 Field-offset table, 1,223 constants",
        why: "Mechanically extracted from loader bind sites and downgraded from B to \
              structural (ledger §4.10). There is no callable function to differentially test.",
    },
    Gap {
        claim: "§1.9 Economy — resource tick, commerce caps, rate ramp, attrition timing",
        why: "Ledger tier is C: transcribed from capstone output and never executed against \
              retail. No oracle harness exists for any of it. Listed here so the absence stays \
              visible rather than being read off a green suite.",
    },
    Gap {
        claim: "damage_pipeline mutation evidence (31 of 32 step mutations caught)",
        why: "Produced by editing the port and re-running (hbox:~/don-oracle/mutate.py). It is \
              a property of a deliberately modified tree and cannot be reproduced by running \
              the unmodified suite. A mutation run is the right way to check this harness \
              still bites; it is not something the harness can assert about itself.",
    },
    Gap {
        claim: "§3 Structural results — checksum field set, replay format, live type tables, \
                pathfinding structure, live damage capture",
        why: "Measured, but not behavioural claims about a Rust function, so there is nothing \
              for a differential case to compare. They are not weakened by this suite's \
              silence and must not be strengthened by its green.",
    },
];
