//! `Army` and `Armies` — step 13 of `Game::do_frame`, the layer above `Group`.
//!
//! Serves **no** `CheckSums::check_all` channel. That is a finding, not an omission, and
//! §"Checksum participation" below states the evidence.
//!
//! # What an Army is, as distinct from a Group
//!
//! A **`Group`** (`groups_guys.rs`) is a *selection*: up to 128 objects plus formation
//! state, 64 slots per player, revalidated one slot per player per frame by
//! `Groups::process`. Every player command addresses a `Group`.
//!
//! An **`Army`** is a *standing formation of groups*. `ArmyData::list` is `int[16]` and
//! holds **group ids**, `num_groups` counts them, and `Army::add_group` `0x006F8C00`
//! refuses at 16 with an `Error::report`. The link is two-way: `GroupData::army` `+0x08`
//! holds the owning army's *index*, and `Army::close` `0x006F8EA0` writes `-1` back into
//! it. There are exactly **16 armies per player, preallocated at `Armies::init`** — the
//! per-owner `PtrArray<Army>` is filled with 16 `malloc(0xA4)` objects and never grows, so
//! `Armies::find_city`, `num_armies`, `find_army`, `find_aggressive_army` and the merge
//! search inside `Army::process` all loop a hard `0..16` with no bounds read [measured,
//! `Armies::init` `0x006F3C10`].
//!
//! Three consequences worth stating plainly:
//!
//! * **An army never dies, it goes invalid.** `Army::close` sets `valid = 0`, `status = 0`,
//!   `human_frame = 0`, `num_groups = 0` and leaves the object in its slot. Slot reuse is
//!   `Armies::init_army` `0x006F36A0`, which takes the first invalid slot, or — if all 16
//!   are live — **the slot with the fewest `num_units`**, clobbering it.
//! * **An army owns no units directly.** `num_units` / `num_captains` / `num_standard` /
//!   `num_decoys` / `role` are *derived*, recomputed from the member groups by
//!   `Army::normalize` `0x006F9B50` every time the army is processed. `ArmyData::get_unit`
//!   `0x006F9DF0` flattens the groups into one index space; that is the only way army code
//!   reaches an object.
//! * **An army is per-player AI structure, not per-player UI structure.** The owner loop in
//!   `Armies::process_all` gates on `Leader::leader_flags`, and `Unit::add_to_army`
//!   `0x005F7740` is called only from the `Unit::think_*` family — the AI. A human player's
//!   units enter an army only through `Armies::update_city` / scenario paths, and a live
//!   army obeying a human order is exactly what the `human_frame` countdown encodes.
//!
//! Relationship to `Leader`: none structurally — `Army` holds no `Leader` pointer. It reads
//! `leaders[who]` as a global for its gates (`leader_flags` bits `0x01`/`0x0C`/`0x40`/`0x08`,
//! `leader_flags2` bits `0x0A`, `city_num` `+0x3F8`, `LeaderData::is_enemy` `0x006EBAA0`).
//! `Leader::plan_strategy` is one of only three callers of `Armies::init_army`, so the
//! Leader *creates* armies and then leaves them to run themselves.
//!
//! # Provenance
//!
//! Everything below is `[measured]` on this Mac against `ron-bin/riseofnations.exe`
//! (sha256 `30478a44…625079`) and `ron-bin/sbl/rise.pdb`, by capstone disassembly.
//! Layout comes from `schema/pdb-types.json` (`ArmyData`, `sizeof` 152; `Army`, 160).
//!
//! | symbol | VA | size | role |
//! |---|---|---:|---|
//! | `Armies::process_all` | `0x006F3B00` | 120 | **step 13 of `Game::do_frame`** |
//! | `Army::process` | `0x006F93D0` | 1138 | the per-army state machine |
//! | `Army::normalize` | `0x006F9B50` | 657 | recompute aggregates + sort groups |
//! | `Army::close` | `0x006F8EA0` | 118 | retire, unlink groups |
//! | `Army::init` | `0x006F9000` | 288 | (re)initialise a slot |
//! | `Army::add_group` | `0x006F8C00` | 466 | append, cap 16 |
//! | `Army::remove_group` | `0x006F8B50` | 166 | remove + compact + renormalize |
//! | `Army::add_unit` | `0x006F9F40` | 326 | absorb one object |
//! | `Army::member(int,int)` | `0x006F8DE0` | 183 | is object `o` in this army? |
//! | `Army::member(int)` | `0x006F9A50` | 64 | is group `g` in this army? |
//! | `Army::count` | `0x006F9120` | 94 | sum `GroupData::count` over groups |
//! | `Army::set_stance` | `0x006F8750` | 111 | fan a stance to every group |
//! | `Army::send_here` | `0x006F98A0` | 422 | retarget + fan `action_move_to` |
//! | `ArmyData::get_unit` | `0x006F9DF0` | 332 | flattened member index |
//! | `Army::center_of_gravity` | `0x006F8A20` | 303 | mean live member `WCoord` |
//! | `Army::is_moving` | `0x006F5470` | 607 | >1/3 of members in transit |
//! | `Army::is_engaged` | `0x006F56D0` | 466 | >1/4 of members attacking nearby |
//! | `Army::walk_data` | `0x006F9850` | 70 | save-game walk, 2 or 152 bytes |
//! | `Army::find_target` | `0x006F69B0` | 7571 | **the only RNG consumer in the class** |
//! | `Armies::init` | `0x006F3C10` | 280 | 8 x 16 preallocation |
//! | `Armies::init_army` | `0x006F36A0` | 88 | slot allocation |
//! | `Armies::init_navy` | `0x006F31B0` | 71 | `init_army` + `navy = 1` |
//! | `Armies::num_armies` | `0x006F3200` | 74 | count by status mask + region |
//! | `Armies::find_city` | `0x006F3160` | 66 | mustering army for a city |
//! | `Armies::find_aggressive_army` | `0x006F2E10` | 146 | first army outside home borders |
//! | `Armies::find_useful_army` | `0x006F2FE0` | 258 | nearest-per-captain army |
//! | `Armies::leader_defeated` | `0x006F2F90` | 69 | `Army::stop` for all |
//! | `Armies::diplo_change` | `0x006F30F0` | 105 | forced `process(1)` for all |
//! | `Armies::emergency` | `0x006F3250` | 139 | clear target + forced `process(1)` |
//! | `Armies::update_city` | `0x006F2D70` | 147 | retarget every army pointed at a city |
//! | `Armies::walk_data` | `0x006F3700` | 1013 | save-game walk of the 8 `PtrArray`s |
//!
//! # Checksum participation — the finding
//!
//! `CheckSums::check_armies` `0x00936CF0` **exists and has zero callers** [measured,
//! `tools/pdb/callers.py 936cf0` → 0; the function is a 15-byte thunk straight to
//! `Armies::walk_data`]. `Armies::walk_data` is reached from `WalkDataGame::walk_data`,
//! `SaveGame::verify_save`, `LoadGame::verify_load` and `GameLog::say_checksum`, but **not**
//! from `CheckSums::check_all` `0x00936560`, whose fifteen channels are enumerated in
//! `docs/mechanics/COVERAGE.md` §1.
//!
//! So army state **is** save-game state and **is not** desync-checksum state. That is a real
//! exception to `README-LLM.md`'s "sim-critical state ≡ save-game state" and it is recorded
//! in `docs/mechanics/armies.md`. It does not make armies unimportant to lockstep: every
//! effect an army has reaches the wire through `Group::action_*` → `UnitOrder`, and the
//! `groups`, `guys` and `units` channels do hash those. It means an army-only divergence is
//! *invisible until it moves a unit*, which is a worse debugging property, not a better one.
//!
//! # Fidelity
//!
//! **Tier C.** Instruction-level transcription with local tests; nothing here has been
//! executed against a retail oracle. The complete step-13 dispatcher and deterministic
//! `Army::process` prefix are executable through [`Armies::process_all`]. Reached AI bodies
//! remain explicit [`ArmyGaps`] rather than guessed. The machine-readable retail-oracle
//! boundary is [`RUNTIME_FIDELITY_READY`] / [`RUNTIME_FIDELITY_BLOCKERS`], and the evidence
//! ledger is `docs/mechanics/armies.md`.

use crate::rng::Random;
use crate::systems::groups_guys::{sinx, vector_dist, CheckSum, NUM_LEADERS};
use crate::trig::find_angle;

/// Whether this module may serve step 13 on a fidelity or product surface.
///
/// This is deliberately `false` even though the recovered control-flow tests pass. The
/// production library contains no army tick driver while any named retail body remains
/// absent.
pub const RUNTIME_FIDELITY_READY: bool = false;

/// The recovered step-13 dispatcher and deterministic state-machine prefix are safe to
/// execute with an [`ArmyWorld`] host. This is deliberately independent of retail-oracle
/// fidelity: unresolved reached bodies are returned in [`ArmyProcessTrace::gaps`].
pub const STEP13_DISPATCH_READY: bool = true;

/// Retail bodies reached by the recovered step-13 driver but not executed by it.
///
/// Keep this list literal and non-empty until each function has retail differential
/// evidence. A module declaration is discoverability, not a completeness claim.
pub const RUNTIME_FIDELITY_BLOCKERS: &[&str] = &[
    "Army::do_mustering 0x006F4260",
    "Army::do_defending 0x006F4070",
    "Army::do_marching 0x006F3DF0",
    "Army::do_forming 0x006F43C0",
    "Army::do_transporting 0x006F4690",
    "Army::engagement 0x006F5160",
    "Army::use_generals 0x006F4C30",
    "Army::use_spies 0x006F4AF0",
    "Army::use_scouts 0x006F49A0",
    "Army::find_muster_spot 0x006F5CC0",
    "Army::find_target body 0x006F69B0",
    "Army::stop 0x006F9180",
];

// ---------------------------------------------------------------------------
// Shapes and constants, all [measured]
// ---------------------------------------------------------------------------

/// `Armies::init` `0x006F3C10` pushes exactly `0x10` freshly allocated `Army` objects into
/// each owner list, and every search loop in the class runs `0..0x10` without reading
/// `PtrArray::length`.
pub const ARMIES_PER_PLAYER: usize = 16;

/// `ArmyData::list : int[16]`, and `Army::add_group` `0x006F8C00` reports an error at
/// `num_groups == 0x10` rather than growing.
pub const ARMY_MAX_GROUPS: usize = 16;

/// `sizeof(Army)` — `ArmyData` 152 + the virtual-base pointer and padding of `ArmyOut`.
pub const SIZEOF_ARMY: usize = 160;

/// `malloc` size per army in `Armies::init`: 160 plus the 4-byte `new[]` cookie.
pub const ARMY_ALLOC: usize = 0xA4;

/// `Army::walk_data` `0x006F9850` always walks `[this+0, this+2)` — the `valid` short.
pub const ARMY_WALK_HEAD: usize = 2;

/// …and, when `valid != 0`, additionally `[this+2, this+0x98)`.
pub const ARMY_WALK_TAIL_HI: usize = 0x98;

/// Bytes hashed for a live army: 2 + 150.
pub const ARMY_WALK_LEN: usize = ARMY_WALK_TAIL_HI;

/// `Army::init` `0x006F9000` offsets the initial `y` a fixed 768 south of the founding
/// city, one `WCoord` cell.
pub const ARMY_INIT_Y_OFFSET: i32 = 0x300;

/// `Army::send_here` `0x006F98A0` spreads successive groups by this many `Coord` along the
/// muster angle: `mov [ebp-8], 0x180` feeding `sin_table`.
pub const SEND_HERE_SPREAD: i32 = 0x180;

/// `Army::is_engaged` `0x006F56D0` rejects members farther than this from `(x, y)` and
/// farther than this from the centre of gravity.
pub const ENGAGED_RADIUS: i32 = 0xC00;

/// `Army::is_moving` `0x006F5470` rejects members farther than this from the centre of
/// gravity when deciding "still with the army".
pub const MOVING_RADIUS: i32 = 0xF00;

/// `Armies::find_useful_army` / `find_army` seed `ArmiesData::find_dist` `0x00CB4BAC` with
/// this before every scan.
pub const FIND_DIST_SEED: i32 = 0x5F5_E0FF; // 99,999,999

// --- Army::status bits, each named by the arm it gates in `Army::process` ---------------

/// `test byte [esi+4], 1` → `set_stance(1)` + `Army::do_mustering` `0x006F4260`.
pub const ST_MUSTERING: i32 = 0x01;
/// `test byte [esi+4], 2` → `set_stance(0)` + `Army::do_marching` `0x006F3DF0`.
pub const ST_MARCHING: i32 = 0x02;
/// `test byte [esi+4], 0x10` → `Army::do_forming` `0x006F43C0`; when clear, `engagement`.
pub const ST_FORMING: i32 = 0x10;
/// `test byte [esi+4], 0x20` → `set_stance(1)` + `Army::do_defending` `0x006F4070`.
pub const ST_DEFENDING: i32 = 0x20;
/// `test byte [esi+4], 0x40` → `Army::do_transporting` `0x006F4690`.
pub const ST_TRANSPORTING: i32 = 0x40;
/// Consumed and cleared by `Armies::process_all` before it calls `Army::process(1)`.
/// Set by `Armies::update_city` when a tracked city changes hands to owner 0.
pub const ST_HURRY: i32 = 0x80;

/// `Army::process`: `mov eax, ecx; and eax, 0x18; cmp eax, ecx; jne` — a status that is a
/// subset of `{FORMING, 0x08}` is a dead end and is reset to `MARCHING`.
pub const ST_DEAD_END_MASK: i32 = 0x18;

/// Cleared together by the re-target arm (`and eax, 0xFFFFFFED`) and set together
/// afterwards (`or eax, 0x12`).
pub const ST_RETARGET_MASK: i32 = ST_MARCHING | ST_FORMING; // 0x12

// --- Leader flag bits this class reads --------------------------------------------------

/// `Leader::leader_flags` `+0x00` bit 0. Cleared → the owner is skipped entirely.
pub const LF_ACTIVE: u32 = 0x01;
/// `Leader::leader_flags` `+0x00` bits 2-3. Exactly `0x04` → the owner is skipped.
pub const LF_KIND_MASK: u32 = 0x0C;
/// The one value of [`LF_KIND_MASK`] that disables army processing.
pub const LF_KIND_SKIP: u32 = 0x04;
/// `Leader::leader_flags` `+0x00` bit 3 — `Army::find_target` returns immediately.
pub const LF_NO_TARGETING: u32 = 0x08;
/// `Leader::leader_flags` `+0x00` bit 6 — `Army::process` returns before `normalize`.
pub const LF_ARMIES_OFF: u32 = 0x40;
/// `Leader::leader_flags2` `+0x04` bits 1 and 3. Either set → the owner is skipped.
pub const LF2_SKIP_MASK: u32 = 0x0A;

// --- CountIndex arms this class uses ----------------------------------------------------
//
// `GroupData::count(CountIndex, int, int)` `0x00711720` is a 32-entry jump table at
// `0x00712364` indexed straight by the enum. Only the four arms below are reached from
// `Army`; their predicates are transcribed from the arm bodies.

/// Arm 4 `0x00711B6C`: live map units that can move and are not simultaneously
/// `Unit+0x68 & 0x40001 == 0x40001`.
pub const COUNT_MOBILE: i32 = 4;
/// Arm 5 `0x00711BD3`: `SubObjectData::is_spellcaster` and not `Unit+0x68 & 1`.
pub const COUNT_SPELLCASTER: i32 = 5;
/// Arm 6 `0x00711C2B`: `Unit+0x68 & 1` set, virtual slot `+0xE8` is `UnitData::is_captain`,
/// and `Unit+0x8E` bit 15 set — the decoy predicate.
pub const COUNT_DECOY: i32 = 6;
/// Arm `0x13` `0x00711F38`: count members of a given type, excluding `Unit+0x68 & 1`.
pub const COUNT_TYPE: i32 = 0x13;

/// `Army::process`'s `use_generals` gate: `count(COUNT_TYPE, 0x36)`.
pub const TYPE_ARG_GENERAL: i32 = 0x36;
/// `Army::process`'s `use_spies` gate.
pub const TYPE_ARG_SPY: i32 = 0x3A;
/// `Army::process`'s `use_scouts` gate.
pub const TYPE_ARG_SCOUT: i32 = 0x45;
/// `Army::normalize`'s first `num_standard` subtraction, and `Army::find_target`'s prologue.
pub const TYPE_ARG_SIEGE: i32 = 0x3F;
/// `Army::normalize`'s second `num_standard` subtraction.
pub const TYPE_ARG_281: i32 = 0x119;
/// `Army::find_target` prologue strength term.
pub const TYPE_ARG_227: i32 = 0xE3;
/// `Army::find_target` prologue strength term.
pub const TYPE_ARG_132: i32 = 0x84;

// --- process_all phasing ----------------------------------------------------------------

/// `Army::process`'s light pass fires when `(frame - 30 + offset) % 128 == 0`.
pub const PHASE_LIGHT: i32 = 128;
/// The bias inside that test: `add eax, -0x1E`.
pub const PHASE_LIGHT_BIAS: i32 = -30;
/// The heavy pass fires when `(frame + offset) % 256 == 0`.
pub const PHASE_HEAVY: i32 = 256;

/// `lea edi,[army + who*2]; add edi,edi` — the per-army phase offset.
#[inline]
pub fn phase_offset(army: i32, who: i32) -> i32 {
    (army + who * 2) * 2
}

// ---------------------------------------------------------------------------
// The gap ledger
// ---------------------------------------------------------------------------

/// Named retail sub-calls this port reaches but does not execute, counted rather than
/// guessed — the same discipline `crate::tick` applies to the anti-air dud roll.
///
/// The retail-body fields are call sites inside outer control flow that *does* run here. A
/// non-zero count is a precise statement of which body did not happen this frame. The RNG
/// hazard is separate: because those bodies are missing, the exact draw count is unknown
/// and is never invented.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ArmyGaps {
    /// `Army::do_mustering` `0x006F4260`.
    pub do_mustering: u64,
    /// `Army::do_defending` `0x006F4070`.
    pub do_defending: u64,
    /// `Army::do_marching` `0x006F3DF0` — the caller of `find_target`.
    pub do_marching: u64,
    /// `Army::do_forming` `0x006F43C0`.
    pub do_forming: u64,
    /// `Army::do_transporting` `0x006F4690` — the other caller of `find_target`.
    pub do_transporting: u64,
    /// `Army::engagement` `0x006F5160`.
    pub engagement: u64,
    /// `Army::use_generals` `0x006F4C30`.
    pub use_generals: u64,
    /// `Army::use_spies` `0x006F4AF0`.
    pub use_spies: u64,
    /// `Army::use_scouts` `0x006F49A0`.
    pub use_scouts: u64,
    /// `Army::find_muster_spot` `0x006F5CC0` (3,309 B).
    pub find_muster_spot: u64,
    /// Dispatches into an unported body that can reach `Army::find_target` and therefore
    /// `game_random`. This is a count of unresolved stream hazards, **not** a guessed count
    /// of RNG draws. The exact draw count is data-dependent inside the absent body.
    pub game_random_stream_unresolved: u64,
}

impl ArmyGaps {
    /// Total counted skips, for a one-number coverage line.
    pub fn total(&self) -> u64 {
        self.do_mustering
            + self.do_defending
            + self.do_marching
            + self.do_forming
            + self.do_transporting
            + self.engagement
            + self.use_generals
            + self.use_spies
            + self.use_scouts
            + self.find_muster_spot
    }

    fn merge(&mut self, other: &ArmyGaps) {
        self.do_mustering += other.do_mustering;
        self.do_defending += other.do_defending;
        self.do_marching += other.do_marching;
        self.do_forming += other.do_forming;
        self.do_transporting += other.do_transporting;
        self.engagement += other.engagement;
        self.use_generals += other.use_generals;
        self.use_spies += other.use_spies;
        self.use_scouts += other.use_scouts;
        self.find_muster_spot += other.find_muster_spot;
        self.game_random_stream_unresolved += other.game_random_stream_unresolved;
    }
}

/// Per-call evidence from the exact `Armies::process_all` dispatcher and the recovered
/// deterministic prefix of every reached `Army::process`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ArmyProcessTrace {
    /// Owners that passed all three retail leader gates.
    pub owners_enabled: u32,
    /// Army slots examined under enabled owners, including invalid preallocated slots.
    pub slots_examined: u32,
    /// Valid armies dispatched to `Army::process`.
    pub armies_processed: u32,
    /// Valid armies whose 256-frame heavy prefix ran, including hurry-forced passes.
    pub heavy_passes: u32,
    /// Reached bodies which remain unresolved.
    pub gaps: ArmyGaps,
}

/// The three `Random::get` sites inside `Army::find_target` `0x006F69B0`, in address order.
///
/// All three draw from **`game_random` `0x00C06184`** — the main simulation stream, not a
/// private one — via `int Random::get(int,int)` `0x00A39D70` with `lo = 0`, `hi = 0xFFFF`.
/// They are the only RNG consumers anywhere in the `Army`/`Armies` cone [measured: a scan
/// of every `Army::*` / `Armies::*` procedure's direct call targets found `Random::get` in
/// `find_target` and nowhere else].
///
/// The post-processing is transcribed exactly, because drawing the right number of times
/// with the wrong arithmetic desyncs just as badly as not drawing:
///
/// | site | VA | expression |
/// |---|---|---|
/// | [`FindTargetDraw::AggressiveCoin`] | `0x006F6DBB` | `Random::get(0,0xFFFF) & 0x80000001` — a parity test, reached only when `Armies::find_aggressive_army` returns `< 0` |
/// | [`FindTargetDraw::TargetJitterA`] | `0x006F718A` | `900 + Random::get(0,0xFFFF) % 200` |
/// | [`FindTargetDraw::TargetJitterB`] | `0x006F801A` | `900 + Random::get(0,0xFFFF) % 200` |
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FindTargetDraw {
    /// `0x006F6DBB`.
    AggressiveCoin,
    /// `0x006F718A`.
    TargetJitterA,
    /// `0x006F801A`.
    TargetJitterB,
}

impl FindTargetDraw {
    /// The virtual address of the `call Random::get` for this draw.
    pub fn va(self) -> u32 {
        match self {
            FindTargetDraw::AggressiveCoin => 0x006F_6DBB,
            FindTargetDraw::TargetJitterA => 0x006F_718A,
            FindTargetDraw::TargetJitterB => 0x006F_801A,
        }
    }
}

/// `900 + Random::get(0, 0xFFFF) % 200`, the shared tail of the two jitter sites.
///
/// `cdq; mov ecx,0xC8; idiv ecx; lea ebx,[edx+0x384]` — signed remainder, so the fold is
/// C's `%` and not `rem_euclid`; the draw is non-negative so the two agree, and the
/// transcription keeps `%` to match the instruction.
#[inline]
pub fn find_target_jitter(rng: &mut Random) -> i32 {
    0x384 + rng.get(0, 0xFFFF) % 200
}

/// `Random::get(0, 0xFFFF) & 0x80000001`, the aggressive-army coin at `0x006F6DBB`.
///
/// The result is compared against zero by the shared `jne` at `0x006F6DCB`, so this is the
/// low-bit parity of the draw. Returns `true` when the branch is taken (odd draw).
#[inline]
pub fn find_target_coin(rng: &mut Random) -> bool {
    (rng.get(0, 0xFFFF) & 0x8000_0001u32 as i32) != 0
}

// ---------------------------------------------------------------------------
// The host interface
// ---------------------------------------------------------------------------

/// Everything `Army` reads or writes that does not live in `ArmyData`.
///
/// Retail reads these as globals (`leaders` `0x00E3A390`, `cities` `0x00C09960`, `units`
/// `0x00C0AEB0`, `objects` `0x00C0AB70`, `groups` `0x00E85F10`, `world` via
/// `GameAccess::world`). Group access is by **global group id** — the id stored in
/// `ArmyData::list` is a flat index into the 512-entry `Array<Group>`, since every use is
/// `imul ecx, gid, 0x9D4; add ecx, groups.list` — so nothing here hands out a `&GroupData`
/// and the borrow checker never gets in the way of a faithful transcription.
///
/// Implementors must supply *decoded* object coordinates: retail stores `Object::x_internal`
/// `+0x10` and `y_internal` `+0x14` XOR'd with `0x00063637` and every reader in this class
/// un-XORs before use.
pub trait ArmyWorld {
    // --- clocks and map ---------------------------------------------------------------
    /// `Game::frame` `+0x550`.
    fn frame(&self) -> i32;
    /// `World::width` `+0x00` in tiles; `Army::send_here` clamps `x` to `width*768 - 1`.
    fn world_width(&self) -> i32;
    /// `World::height` `+0x04` in tiles.
    fn world_height(&self) -> i32;
    /// `WorldData::wdata[..].region` — `Army::is_moving` compares it against `Army::reg`,
    /// and `Armies::find_aggressive_army` compares the *border owner* byte at `+0x0F`.
    fn tile_region(&self, wx: i32, wy: i32) -> i32;
    /// The `wdata` border-owner byte at `+0x0F` that `find_aggressive_army` reads.
    fn tile_owner(&self, wx: i32, wy: i32) -> i32;

    // --- leaders ----------------------------------------------------------------------
    /// `Leader::leader_flags` `+0x00`.
    fn leader_flags(&self, who: usize) -> u32;
    /// `Leader::leader_flags2` `+0x04`.
    fn leader_flags2(&self, who: usize) -> u32;
    /// `Leader::city_num` `+0x3F8`.
    fn leader_city_num(&self, who: usize) -> i32;
    /// `LeaderData::is_enemy` `0x006EBAA0`.
    fn leader_is_enemy(&self, who: usize, other: i32) -> bool;

    // --- cities -----------------------------------------------------------------------
    /// `City::city_flags` `+0x04`; bit 0 is the liveness the retire path tests.
    fn city_flags(&self, who: usize, city: i32) -> i32;
    /// `City::reg` `+0x0A`, sign-extended from a `short`.
    fn city_reg(&self, who: usize, city: i32) -> i32;
    /// `City::x` `+0x0C` and `City::y` `+0x10`. Plain `Coord`s — **not** XOR'd.
    fn city_pos(&self, who: usize, city: i32) -> (i32, i32);

    // --- objects and units ------------------------------------------------------------
    /// `Object::flags` `+0x08` bit 0 — alive.
    fn object_alive(&self, who: usize, o: i32) -> bool;
    /// Decoded `Object::x_internal`/`y_internal`, i.e. `raw ^ 0x00063637`.
    fn object_pos(&self, who: usize, o: i32) -> (i32, i32);
    /// `Object` virtual `+0x08` (`is_unit`) *and* virtual `+0xBC`, the pair every
    /// member-iterating routine tests before using a member's position.
    fn unit_on_map(&self, who: usize, o: i32) -> bool;
    /// `UnitData::get_action` `0x00608450` → virtual `+0x10`; `Army::is_engaged` wants
    /// `10`. Return a negative value for "no action".
    fn unit_action_type(&self, who: usize, o: i32) -> i32;
    /// `Army::is_moving`'s order probe. `None` = the unit has no current order;
    /// `Some(true)` = the current order's virtual `+0x14` is non-zero.
    fn unit_order_active(&self, who: usize, o: i32) -> Option<bool>;
    /// The sort key `Army::normalize`'s second phase uses: `unit->ptype` `+0x18`, then
    /// `TypeData::cat` at `+0x14` of that `UnitType` [measured, PDB layout].
    fn unit_type_category(&self, who: usize, o: i32) -> i32;

    // --- groups, by global id ---------------------------------------------------------
    /// `GroupData::id` `+0x04`. `Army::add_group` stores *this*, not the argument.
    fn group_id(&self, gid: i32) -> i32;
    /// `GroupData::army` `+0x08`.
    fn group_army(&self, gid: i32) -> i32;
    /// Write `GroupData::army` `+0x08`.
    fn set_group_army(&mut self, gid: i32, army: i32);
    /// `GroupData::num` `+0x0C`.
    fn group_num(&self, gid: i32) -> i32;
    /// `GroupData::role` `+0x34`.
    fn group_role(&self, gid: i32) -> i32;
    /// `GroupData::buildings` `+0x49`.
    fn group_buildings(&self, gid: i32) -> bool;
    /// `GroupData::who` `+0x4A`.
    fn group_who(&self, gid: i32) -> i32;
    /// `GroupData::list[k]` `+0x8CC`.
    fn group_member(&self, gid: i32, k: i32) -> i32;
    /// `Group::get_num_cap`, virtual slot `+0x08`.
    fn group_num_cap(&self, gid: i32) -> i32;
    /// `GroupData::count` `0x00711720`.
    fn group_count(&self, gid: i32, ci: i32, arg: i32) -> i32;
    /// `GroupData::find_leader` `0x0070CCB0` with `arg 0`.
    fn group_find_leader(&self, gid: i32) -> i32;
    /// `GroupData::get_stance_type` `0x0070D370`.
    fn group_stance_type(&self, gid: i32) -> i32;
    /// `Group::normalize` `0x00711540`.
    fn group_normalize(&mut self, gid: i32);
    /// `Group::action_halt` `0x0070D0C0` with `arg 0`.
    fn group_action_halt(&mut self, gid: i32);
    /// `Group::action_stance` `0x0070D440`.
    fn group_action_stance(&mut self, gid: i32, stance: i32);
    /// `Group::action_move_to` `0x0070FBA0`, called by `Army::send_here` as
    /// `action_move_to(x, y, 2, 1, angle, order, 1, -1, -1, 0)`.
    fn group_action_move_to(&mut self, gid: i32, x: i32, y: i32, angle: i32, order: i32);
    /// `Group::add` `0x00714350` into a scratch group, then `Groups::push_group`
    /// `0x0070F9E0`. Returns the new global group id, or a negative on failure.
    fn push_singleton_group(&mut self, who: usize, o: i32) -> i32;
    /// `Unit::set_group` `0x00605220`.
    fn unit_set_group(&mut self, who: usize, o: i32, gid: i32);
    /// `Group` virtual `+0x0C` — the in-place add `Army::add_unit` uses when the army
    /// already has a group 0.
    fn group_add_member(&mut self, gid: i32, o: i32, who: usize);
}

/// Live facts required by the outer step-13 dispatcher when no valid Army has yet been
/// created. `Leader::plan_strategy` is the normal creator; until that AI body runs, the
/// retail-preallocated 16 slots per enabled owner are all invalid and no deeper host query
/// is reached.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Step13DispatchInputs {
    pub frame: i32,
    pub world_width: i32,
    pub world_height: i32,
    pub leader_flags: [u32; NUM_LEADERS],
    pub leader_flags2: [u32; NUM_LEADERS],
}

/// Result of executing the ordinary preallocated-slot step-13 boundary.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Step13DispatchTrace {
    pub process: ArmyProcessTrace,
    /// Valid armies suppressed because their Group/Unit/City host was not attached. This
    /// is zero for a newly initialized game, whose 128 slots are all invalid.
    pub missing_live_host_armies: u32,
}

struct DispatcherOnlyWorld {
    inputs: Step13DispatchInputs,
}

impl ArmyWorld for DispatcherOnlyWorld {
    fn frame(&self) -> i32 {
        self.inputs.frame
    }
    fn world_width(&self) -> i32 {
        self.inputs.world_width
    }
    fn world_height(&self) -> i32 {
        self.inputs.world_height
    }
    fn leader_flags(&self, who: usize) -> u32 {
        self.inputs.leader_flags[who]
    }
    fn leader_flags2(&self, who: usize) -> u32 {
        self.inputs.leader_flags2[who]
    }

    fn tile_region(&self, _wx: i32, _wy: i32) -> i32 {
        unreachable!("invalid preallocated armies never query terrain")
    }
    fn tile_owner(&self, _wx: i32, _wy: i32) -> i32 {
        unreachable!("invalid preallocated armies never query terrain")
    }
    fn leader_city_num(&self, _who: usize) -> i32 {
        unreachable!("invalid preallocated armies never query cities")
    }
    fn leader_is_enemy(&self, _who: usize, _other: i32) -> bool {
        unreachable!("invalid preallocated armies never query diplomacy")
    }
    fn city_flags(&self, _who: usize, _city: i32) -> i32 {
        unreachable!("invalid preallocated armies never query cities")
    }
    fn city_reg(&self, _who: usize, _city: i32) -> i32 {
        unreachable!("invalid preallocated armies never query cities")
    }
    fn city_pos(&self, _who: usize, _city: i32) -> (i32, i32) {
        unreachable!("invalid preallocated armies never query cities")
    }
    fn object_alive(&self, _who: usize, _o: i32) -> bool {
        unreachable!("invalid preallocated armies never query objects")
    }
    fn object_pos(&self, _who: usize, _o: i32) -> (i32, i32) {
        unreachable!("invalid preallocated armies never query objects")
    }
    fn unit_on_map(&self, _who: usize, _o: i32) -> bool {
        unreachable!("invalid preallocated armies never query units")
    }
    fn unit_action_type(&self, _who: usize, _o: i32) -> i32 {
        unreachable!("invalid preallocated armies never query units")
    }
    fn unit_order_active(&self, _who: usize, _o: i32) -> Option<bool> {
        unreachable!("invalid preallocated armies never query orders")
    }
    fn unit_type_category(&self, _who: usize, _o: i32) -> i32 {
        unreachable!("invalid preallocated armies never query type data")
    }
    fn group_id(&self, _gid: i32) -> i32 {
        unreachable!("invalid preallocated armies never query groups")
    }
    fn group_army(&self, _gid: i32) -> i32 {
        unreachable!("invalid preallocated armies never query groups")
    }
    fn set_group_army(&mut self, _gid: i32, _army: i32) {
        unreachable!("invalid preallocated armies never mutate groups")
    }
    fn group_num(&self, _gid: i32) -> i32 {
        unreachable!("invalid preallocated armies never query groups")
    }
    fn group_role(&self, _gid: i32) -> i32 {
        unreachable!("invalid preallocated armies never query groups")
    }
    fn group_buildings(&self, _gid: i32) -> bool {
        unreachable!("invalid preallocated armies never query groups")
    }
    fn group_who(&self, _gid: i32) -> i32 {
        unreachable!("invalid preallocated armies never query groups")
    }
    fn group_member(&self, _gid: i32, _k: i32) -> i32 {
        unreachable!("invalid preallocated armies never query groups")
    }
    fn group_num_cap(&self, _gid: i32) -> i32 {
        unreachable!("invalid preallocated armies never query groups")
    }
    fn group_count(&self, _gid: i32, _ci: i32, _arg: i32) -> i32 {
        unreachable!("invalid preallocated armies never query groups")
    }
    fn group_find_leader(&self, _gid: i32) -> i32 {
        unreachable!("invalid preallocated armies never query groups")
    }
    fn group_stance_type(&self, _gid: i32) -> i32 {
        unreachable!("invalid preallocated armies never query groups")
    }
    fn group_normalize(&mut self, _gid: i32) {
        unreachable!("invalid preallocated armies never mutate groups")
    }
    fn group_action_halt(&mut self, _gid: i32) {
        unreachable!("invalid preallocated armies never issue group actions")
    }
    fn group_action_stance(&mut self, _gid: i32, _stance: i32) {
        unreachable!("invalid preallocated armies never issue group actions")
    }
    fn group_action_move_to(&mut self, _gid: i32, _x: i32, _y: i32, _angle: i32, _order: i32) {
        unreachable!("invalid preallocated armies never issue group actions")
    }
    fn push_singleton_group(&mut self, _who: usize, _o: i32) -> i32 {
        unreachable!("invalid preallocated armies never create groups")
    }
    fn unit_set_group(&mut self, _who: usize, _o: i32, _gid: i32) {
        unreachable!("invalid preallocated armies never mutate units")
    }
    fn group_add_member(&mut self, _gid: i32, _o: i32, _who: usize) {
        unreachable!("invalid preallocated armies never mutate groups")
    }
}

// ---------------------------------------------------------------------------
// ArmyData
// ---------------------------------------------------------------------------

/// One army. Field order and offsets are `ArmyData`'s, `sizeof` 152.
///
/// The declaration order below **is** the checksum/save image order, which is why the
/// struct is written flat rather than grouped by meaning.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArmyData {
    /// `+0x00` non-zero once `init` has run and until `close`. The walk's length prefix.
    pub valid: i16,
    /// `+0x02` this army's own index inside `armies.lists[who]`, 0..15.
    pub army: i16,
    /// `+0x04` the state-machine bitset; see the `ST_*` constants.
    pub status: i32,
    /// `+0x08` terrain region, seeded from the founding city's `City::reg`.
    pub reg: i32,
    /// `+0x0C` OR of every member group's `GroupData::role`.
    pub role: i32,
    /// `+0x10` derived: sum of member groups' `num`.
    pub num_units: i32,
    /// `+0x14` derived: sum of member groups' `get_num_cap()`.
    pub num_captains: i32,
    /// `+0x18` derived; see [`ArmyData::normalize`] for the five-term expression.
    pub num_standard: i32,
    /// `+0x1C` derived: sum of `count(COUNT_DECOY)`.
    pub num_decoys: i32,
    /// `+0x20` index into `cities.lists[who]`; doubles as the scan cursor in the retire path.
    pub city: i32,
    /// `+0x24` non-zero for a navy, set by `Armies::init_navy`.
    pub navy: i32,
    /// `+0x28` frames left obeying an explicit human order. Decremented at the very top of
    /// `Army::process`, before any gate.
    pub human_frame: i32,
    /// `+0x2C` written by `Army::init` to 0; not read anywhere in the processed path.
    pub hurry: i32,
    /// `+0x30` target object index, or -1.
    pub target_o: i32,
    /// `+0x34` owner of the target object, or -1.
    pub target_who: i32,
    /// `+0x38` rally point `Coord`.
    pub x: i32,
    /// `+0x3C` rally point `Coord`.
    pub y: i32,
    /// `+0x40` written to 0 by `init`; the formation heading used by `do_forming`.
    pub angle: i32,
    /// `+0x44` written to 0 by `init`.
    pub rally_dist: i32,
    /// `+0x48` muster point `WCoord`.
    pub muster_x: i32,
    /// `+0x4C` muster point `WCoord`.
    pub muster_y: i32,
    /// `+0x50` heading toward the rally point; `send_here` recomputes it with `find_angle`.
    pub muster_angle: i32,
    /// `+0x54` member **group ids**; `-1` is the empty slot `init` fills with.
    pub list: [i32; ARMY_MAX_GROUPS],
    /// `+0x94` owner slot 0..7.
    pub who: i16,
    /// `+0x96` live prefix length of [`ArmyData::list`].
    pub num_groups: i16,
}

impl Default for ArmyData {
    fn default() -> Self {
        ArmyData {
            valid: 0,
            army: 0,
            status: 0,
            reg: -1,
            role: 0,
            num_units: 0,
            num_captains: 0,
            num_standard: 0,
            num_decoys: 0,
            city: -1,
            navy: 0,
            human_frame: 0,
            hurry: 0,
            target_o: -1,
            target_who: -1,
            x: -1,
            y: -1,
            angle: 0,
            rally_dist: 0,
            muster_x: -1,
            muster_y: -1,
            muster_angle: 0,
            list: [-1; ARMY_MAX_GROUPS],
            who: 0,
            num_groups: 0,
        }
    }
}

impl ArmyData {
    // --- image / checksum ------------------------------------------------------------

    /// The 152-byte `ArmyData` image, little-endian, in declaration order.
    pub fn image(&self) -> [u8; 152] {
        let mut out = [0u8; 152];
        out[0..2].copy_from_slice(&self.valid.to_le_bytes());
        out[2..4].copy_from_slice(&self.army.to_le_bytes());
        let ints = [
            (4, self.status),
            (8, self.reg),
            (12, self.role),
            (16, self.num_units),
            (20, self.num_captains),
            (24, self.num_standard),
            (28, self.num_decoys),
            (32, self.city),
            (36, self.navy),
            (40, self.human_frame),
            (44, self.hurry),
            (48, self.target_o),
            (52, self.target_who),
            (56, self.x),
            (60, self.y),
            (64, self.angle),
            (68, self.rally_dist),
            (72, self.muster_x),
            (76, self.muster_y),
            (80, self.muster_angle),
        ];
        for (off, v) in ints {
            out[off..off + 4].copy_from_slice(&v.to_le_bytes());
        }
        for (i, g) in self.list.iter().enumerate() {
            let off = 84 + i * 4;
            out[off..off + 4].copy_from_slice(&g.to_le_bytes());
        }
        out[148..150].copy_from_slice(&self.who.to_le_bytes());
        out[150..152].copy_from_slice(&self.num_groups.to_le_bytes());
        out
    }

    /// `Army::walk_data` `0x006F9850`, exactly.
    ///
    /// ```text
    /// walk_name(int_str_array[0x10] + 0xA78)
    /// walk(this + 0x00, this + 0x02)          ; the `valid` short, always
    /// if (valid != 0)
    ///     walk(this + 0x02, this + 0x98)      ; army .. num_groups, 150 bytes
    /// ```
    ///
    /// So an invalid slot still contributes 2 bytes and a live one contributes 152. The
    /// second range is **`this+2`, not `this+4`** — `army` is inside the image, which is
    /// what makes slot identity part of the save.
    pub fn walk(&self, cs: &mut CheckSum) {
        let img = self.image();
        cs.walk(&img[0..ARMY_WALK_HEAD]);
        if self.valid != 0 {
            cs.walk(&img[ARMY_WALK_HEAD..ARMY_WALK_TAIL_HI]);
        }
    }

    /// Bytes this army contributes to a walk — 2 or 152.
    pub fn walked_len(&self) -> usize {
        if self.valid != 0 {
            ARMY_WALK_LEN
        } else {
            ARMY_WALK_HEAD
        }
    }

    // --- construction ----------------------------------------------------------------

    /// `Army::init` `0x006F9000` — `init(int army, int who, int city)`.
    ///
    /// The founding city seeds `reg` and the rally point, with the rally point pushed
    /// [`ARMY_INIT_Y_OFFSET`] south of the city centre. With `city < 0` the five
    /// city-derived fields all become `-1` rather than 0, which is why
    /// [`ArmyData::default`] uses `-1` for them.
    ///
    /// `status` starts at [`ST_MUSTERING`] and `valid` at 1.
    pub fn init<W: ArmyWorld + ?Sized>(&mut self, w: &W, army: i32, who: i32, city: i32) {
        self.army = army as i16;
        self.who = who as i16;
        self.city = city;
        if city >= 0 {
            let (cx, cy) = w.city_pos(who as usize, city);
            self.reg = w.city_reg(who as usize, city);
            self.x = cx;
            self.y = cy + ARMY_INIT_Y_OFFSET;
            self.muster_x = div3_shift8(self.x);
            self.muster_y = div3_shift8(self.y);
        } else {
            self.reg = -1;
            self.x = -1;
            self.y = -1;
            self.muster_x = -1;
            self.muster_y = -1;
        }
        self.num_groups = 0;
        self.num_units = 0;
        self.num_captains = 0;
        self.num_standard = 0;
        self.num_decoys = 0;
        self.human_frame = 0;
        self.role = 0;
        self.valid = 1;
        self.status = ST_MUSTERING;
        self.angle = 0;
        self.muster_angle = 0;
        self.rally_dist = 0;
        self.target_o = -1;
        self.target_who = -1;
        self.navy = 0;
        self.hurry = 0;
        self.list = [-1; ARMY_MAX_GROUPS];
    }

    /// `Army::close` `0x006F8EA0`.
    ///
    /// Unlinks every member group whose `GroupData::army` still names this army — the test
    /// is `group.army == this->army`, on the **index**, so a group that has since been
    /// re-homed is left alone — halts it, then blanks four fields. Note what it does *not*
    /// clear: `reg`, `city`, `navy`, the target, the rally point and `list` all survive,
    /// and `Armies::init_army` overwrites them on reuse.
    pub fn close<W: ArmyWorld + ?Sized>(&mut self, w: &mut W) {
        if self.valid == 0 {
            return;
        }
        for i in 0..self.num_groups as usize {
            let gid = self.list[i];
            if gid < 0 {
                continue;
            }
            if w.group_army(gid) == self.army as i32 {
                w.set_group_army(gid, -1);
                w.group_action_halt(gid);
            }
        }
        self.valid = 0;
        self.status = 0;
        self.human_frame = 0;
        self.num_groups = 0;
    }

    // --- membership ------------------------------------------------------------------

    /// `Army::member(int group_id)` `0x006F9A50` — is this group id in the live prefix?
    pub fn member_group(&self, gid: i32) -> bool {
        if gid < 0 {
            return false;
        }
        self.list[..self.num_groups.max(0) as usize].contains(&gid)
    }

    /// `Army::member(int o, int who)` `0x006F8DE0` — is object `o` a member?
    ///
    /// Retail matches the *group's* `who` against the argument, then requires the object to
    /// be alive, then scans that group's member list. The object-liveness test uses
    /// `objects.lists[group.who]`, i.e. the group's owner, not the army's.
    pub fn member<W: ArmyWorld + ?Sized>(&self, w: &W, o: i32, who: i32) -> bool {
        for i in 0..self.num_groups as usize {
            let gid = self.list[i];
            if gid < 0 {
                continue;
            }
            let gwho = w.group_who(gid);
            if gwho != who {
                continue;
            }
            if !w.object_alive(gwho as usize, o) {
                continue;
            }
            let n = w.group_num(gid);
            for k in 0..n {
                if w.group_member(gid, k) == o {
                    return true;
                }
            }
        }
        false
    }

    /// `Army::count` `0x006F9120` — sum `GroupData::count(ci, arg, 0)` over member groups.
    pub fn count<W: ArmyWorld + ?Sized>(&self, w: &W, ci: i32, arg: i32) -> i32 {
        let mut total = 0i32;
        for i in 0..self.num_groups as usize {
            let gid = self.list[i];
            if gid < 0 {
                continue;
            }
            total = total.wrapping_add(w.group_count(gid, ci, arg));
        }
        total
    }

    /// `ArmyData::get_unit` `0x006F9DF0` — flatten the member groups into one index space.
    ///
    /// Building groups (`GroupData::buildings != 0`) are **skipped entirely**, so
    /// `get_unit` indexes a space that can be smaller than `num_units` — retail reports an
    /// internal error when the index runs off the end and returns `-1`. The `-1` is
    /// reproduced; the error report is not.
    pub fn get_unit<W: ArmyWorld + ?Sized>(&self, w: &W, mut i: i32) -> i32 {
        if i >= self.num_units {
            return -1;
        }
        for k in 0..self.num_groups as usize {
            let gid = self.list[k];
            if gid < 0 {
                continue;
            }
            if w.group_buildings(gid) {
                continue;
            }
            let n = w.group_num(gid);
            if i < n {
                return w.group_member(gid, i);
            }
            i -= n;
        }
        -1
    }

    /// `Army::add_group` `0x006F8C00`.
    ///
    /// Returns `false` on either refusal path — the list being full, or the group belonging
    /// to another player. Retail raises an assertable `Error::report` on both and then
    /// falls through to the same no-op.
    ///
    /// Two details that matter: the value stored is **`GroupData::id`**, not the `gid`
    /// argument (they coincide for a persistent group and diverge for a scratch one), and
    /// `num_units`/`num_captains`/`role` are accumulated here rather than recomputed, so an
    /// `add_group` without a following `normalize` leaves them ahead of the truth.
    pub fn add_group<W: ArmyWorld + ?Sized>(&mut self, w: &mut W, gid: i32) -> bool {
        if self.num_groups as usize == ARMY_MAX_GROUPS {
            return false;
        }
        if w.group_who(gid) != self.who as i32 {
            return false;
        }
        let slot = self.num_groups as usize;
        self.list[slot] = w.group_id(gid);
        self.num_groups += 1;
        w.set_group_army(gid, self.army as i32);
        self.num_units += w.group_num(gid);
        self.num_captains += w.group_num_cap(gid);
        self.role |= w.group_role(gid);
        true
    }

    /// `Army::remove_group` `0x006F8B50`.
    ///
    /// Searches by `GroupData::id`, compacts the tail down one slot, sets the group's
    /// `army` to `-1` and re-runs [`ArmyData::normalize`]. A group whose `army` no longer
    /// names this army is ignored outright.
    pub fn remove_group<W: ArmyWorld + ?Sized>(&mut self, w: &mut W, gid: i32) {
        if gid < 0 {
            return;
        }
        if w.group_army(gid) != self.army as i32 {
            return;
        }
        if self.num_groups == 0 {
            return;
        }
        let id = w.group_id(gid);
        let n = self.num_groups as usize;
        let mut at = 0usize;
        while at < n {
            if self.list[at] == id {
                break;
            }
            at += 1;
        }
        if at == n {
            return;
        }
        self.num_groups -= 1;
        let m = self.num_groups as usize;
        let mut j = at;
        while j < m {
            self.list[j] = self.list[j + 1];
            j += 1;
        }
        w.set_group_army(gid, -1);
        self.normalize(w);
    }

    /// `Army::add_unit` `0x006F9F40`.
    ///
    /// Prefers to fold the object into **group 0**; only when the army has no groups, or
    /// slot 0 is a tombstone, does it build a scratch `Group`, push it and `add_group` the
    /// result. Either way it finishes with `Unit::set_group`.
    pub fn add_unit<W: ArmyWorld + ?Sized>(&mut self, w: &mut W, o: i32) {
        let who = self.who as i32;
        if !w.unit_on_map(who as usize, o) {
            return;
        }
        if self.member(w, o, who) {
            return;
        }
        let existing = if self.num_groups > 0 && self.list[0] >= 0 {
            Some(self.list[0])
        } else {
            None
        };
        let gid = match existing {
            Some(g) => {
                let before = w.group_num(g);
                w.group_add_member(g, o, who as usize);
                let after = w.group_num(g);
                self.num_units += after - before;
                self.num_captains += 1;
                g
            }
            None => {
                let g = w.push_singleton_group(who as usize, o);
                if g < 0 {
                    return;
                }
                self.add_group(w, g);
                g
            }
        };
        w.unit_set_group(who as usize, o, gid);
    }

    // --- derived state ---------------------------------------------------------------

    /// `Army::normalize` `0x006F9B50` — recompute the five aggregates, then order the
    /// group list.
    ///
    /// **Phase 1** walks the groups *backwards* from `num_groups-1`, zeroing `role`,
    /// `num_units`, `num_captains`, `num_standard` and `num_decoys` first. A group that
    /// `Group::normalize` empties, or that turns out to be a building group, is removed —
    /// and `remove_group` re-enters `normalize`, so retail **returns immediately** rather
    /// than continuing the walk. That early return is reproduced; without it the aggregates
    /// are computed twice for the surviving prefix.
    ///
    /// The `num_standard` recurrence is the load-bearing line and it is not a simple sum:
    ///
    /// ```text
    /// num_standard += get_num_cap()
    ///               - count(5, 0)        ; spellcasters
    ///               - count(0x13, 0x3F)  ; siege
    ///               - count(6, 0)        ; decoys
    ///               - count(0x13, 0x119)
    /// ```
    ///
    /// **Phase 2** is an insertion sort over the group list, descending by
    /// `TypeData::cat` (`unit->ptype->+0x14`) of each group's leader unit, where a building
    /// group's "leader" is `list[0]` and everyone else's is `GroupData::find_leader`.
    /// The PDB names this field `cat`; it is not a combat-strength rank.
    pub fn normalize<W: ArmyWorld + ?Sized>(&mut self, w: &mut W) {
        self.role = 0;
        self.num_units = 0;
        self.num_captains = 0;
        self.num_standard = 0;
        self.num_decoys = 0;

        let mut i = self.num_groups as i32 - 1;
        while i >= 0 {
            let gid = self.list[i as usize];
            if gid >= 0 {
                w.group_normalize(gid);
                if w.group_num(gid) == 0 || w.group_buildings(gid) {
                    self.remove_group(w, gid);
                    return;
                }
                self.num_units += w.group_num(gid);
                self.num_captains += w.group_num_cap(gid);
                self.num_decoys += w.group_count(gid, COUNT_DECOY, 0);
                let mut s = self.num_standard;
                s -= w.group_count(gid, COUNT_SPELLCASTER, 0);
                s -= w.group_count(gid, COUNT_TYPE, TYPE_ARG_SIEGE);
                s -= w.group_count(gid, COUNT_DECOY, 0);
                s -= w.group_count(gid, COUNT_TYPE, TYPE_ARG_281);
                self.num_standard = s + w.group_num_cap(gid);
                self.role |= w.group_role(gid);
            }
            i -= 1;
        }

        self.sort_groups(w);
    }

    /// Phase 2 of [`ArmyData::normalize`], split out so it can be tested alone.
    fn sort_groups<W: ArmyWorld + ?Sized>(&mut self, w: &W) {
        let who = self.who as usize;
        let leader_of = |w: &W, gid: i32| -> i32 {
            let n = w.group_num(gid);
            if n < 0 {
                -1
            } else if w.group_buildings(gid) {
                w.group_member(gid, 0)
            } else {
                w.group_find_leader(gid)
            }
        };
        let mut i = 1usize;
        while i < self.num_groups.max(0) as usize {
            let gi = self.list[i];
            if gi < 0 || w.group_num(gi) == 0 {
                i += 1;
                continue;
            }
            let key_i = leader_of(w, gi);
            let mut j = i as i32 - 1;
            while j >= 0 {
                let gj = self.list[j as usize];
                if gj >= 0 && w.group_num(gj) == 0 {
                    j -= 1;
                    continue;
                }
                let key_j = if gj < 0 { -1 } else { leader_of(w, gj) };
                // `cmp previous.cat, inserted.cat; jge` at 0x006F9D99..0x006F9D9C:
                // stop shifting when the preceding category is already greater or equal.
                if w.unit_type_category(who, key_j) >= w.unit_type_category(who, key_i) {
                    break;
                }
                self.list.swap(j as usize, j as usize + 1);
                j -= 1;
            }
            i += 1;
        }
    }

    /// `Army::center_of_gravity` `0x006F8A20` — the mean `WCoord` of live, on-map members.
    ///
    /// With no qualifying member it falls back to the rally point, converted the same way:
    /// `div_3_table[coord >> 8]`, i.e. `coord / 768`.
    pub fn center_of_gravity<W: ArmyWorld + ?Sized>(&self, w: &W) -> (i32, i32) {
        let who = self.who as usize;
        let mut sx = 0i32;
        let mut sy = 0i32;
        let mut n = 0i32;
        for i in 0..self.num_units {
            let o = self.get_unit(w, i);
            if o < 0 || !w.object_alive(who, o) || !w.unit_on_map(who, o) {
                continue;
            }
            let (ox, oy) = w.object_pos(who, o);
            sx += div3_shift8(ox);
            sy += div3_shift8(oy);
            n += 1;
        }
        if n != 0 {
            (sx / n, sy / n)
        } else {
            (div3_shift8(self.x), div3_shift8(self.y))
        }
    }

    /// `Army::is_engaged` `0x006F56D0` — true when **more than a quarter** of the members
    /// are attacking within [`ENGAGED_RADIUS`] of both the rally point and the centre of
    /// gravity.
    ///
    /// It calls `normalize` first, so it is not a pure query.
    pub fn is_engaged<W: ArmyWorld + ?Sized>(&mut self, w: &mut W) -> bool {
        self.normalize(w);
        let who = self.who as usize;
        let mut cog: Option<(i32, i32)> = None;
        let mut hits = 0i32;
        for i in 0..self.num_units {
            let o = self.get_unit(w, i);
            if o < 0 || !w.unit_on_map(who, o) {
                continue;
            }
            if w.unit_action_type(who, o) != 10 {
                continue;
            }
            let (ox, oy) = w.object_pos(who, o);
            if vector_dist((ox - self.x).abs(), (oy - self.y).abs()) >= ENGAGED_RADIUS {
                continue;
            }
            let (cx, cy) = *cog.get_or_insert_with(|| self.center_of_gravity(w));
            let dx = (cx * 3 * 256 - ox + 0x180).abs();
            let dy = (cy * 3 * 256 - oy + 0x180).abs();
            if vector_dist(dy, dx) <= ENGAGED_RADIUS {
                hits += 1;
            }
        }
        hits != 0 && hits > self.num_units / 4
    }

    /// `Army::is_moving` `0x006F5470` — true when **more than a third** of the members are
    /// in transit.
    ///
    /// A member counts as moving in one of two ways. With an active order it must be
    /// within [`MOVING_RADIUS`] of the centre of gravity; with an *inactive* order it counts
    /// only if the tile it stands on is in a **different region** from `Army::reg`. The
    /// asymmetry is retail's and is why an army half-way between two land masses reads as
    /// moving long after its units have stopped.
    pub fn is_moving<W: ArmyWorld + ?Sized>(&self, w: &W) -> bool {
        let who = self.who as usize;
        let mut cog: Option<(i32, i32)> = None;
        let mut hits = 0i32;
        for i in 0..self.num_units {
            let o = self.get_unit(w, i);
            if o < 0 || !w.unit_on_map(who, o) {
                continue;
            }
            let active = match w.unit_order_active(who, o) {
                None => continue,
                Some(a) => a,
            };
            let (ox, oy) = w.object_pos(who, o);
            if !active {
                let reg = w.tile_region(div3_shift8(ox), div3_shift8(oy));
                if reg != self.reg {
                    hits += 1;
                }
                continue;
            }
            let (cx, cy) = *cog.get_or_insert_with(|| self.center_of_gravity(w));
            let dx = (cx * 3 * 256 - ox + 0x180).abs();
            let dy = (cy * 3 * 256 - oy + 0x180).abs();
            if vector_dist(dy, dx) <= MOVING_RADIUS {
                hits += 1;
            }
        }
        hits > self.num_units / 3
    }

    // --- commands --------------------------------------------------------------------

    /// `Army::set_stance` `0x006F8750` — fan a stance to every non-empty member group whose
    /// `get_stance_type()` is 0.
    pub fn set_stance<W: ArmyWorld + ?Sized>(&self, w: &mut W, stance: i32) {
        for i in 0..self.num_groups as usize {
            let gid = self.list[i];
            if gid < 0 {
                continue;
            }
            if w.group_num(gid) == 0 {
                continue;
            }
            if w.group_stance_type(gid) != 0 {
                continue;
            }
            w.group_action_stance(gid, stance);
        }
    }

    /// `Army::send_here` `0x006F98A0` — move the rally point and push every group at it.
    ///
    /// Three transcription notes:
    ///
    /// * The map clamp is `x < 0 → 0` then `x >= width*768 → width*768 - 1`, in `Coord`.
    /// * The heading is recomputed **only when both** `x != this->x` *and* `y != this->y`
    ///   — `je` on each, falling to the same skip label. Moving along a single axis leaves
    ///   `muster_angle` stale. That is retail's, `cmp/je` at `0x006F98F3` and `0x006F98FA`.
    /// * Successive groups are offset along the heading by [`SEND_HERE_SPREAD`] using
    ///   `sinx`, `x` gaining and `y` losing, so a four-group army arrives as a line.
    pub fn send_here<W: ArmyWorld + ?Sized>(&mut self, w: &mut W, x: i32, y: i32, order: i32) {
        let mut px = x.max(0);
        let mut py = y.max(0);
        let mw = w.world_width() * 3 * 256;
        if px >= mw {
            px = mw - 1;
        }
        let mh = w.world_height() * 3 * 256;
        if py >= mh {
            py = mh - 1;
        }
        if px != self.x && py != self.y {
            self.muster_angle = find_angle(px - self.x, py - self.y);
        }
        self.x = px;
        self.y = py;
        self.muster_x = div3_shift8(self.x);
        self.muster_y = div3_shift8(self.y);

        let angle = self.muster_angle;
        let mut ex = self.x;
        let mut ey = self.y;
        for i in 0..self.num_groups as usize {
            let gid = self.list[i];
            if gid < 0 {
                continue;
            }
            if w.group_num(gid) == 0 {
                continue;
            }
            w.set_group_army(gid, self.army as i32);
            w.group_action_move_to(gid, ex, ey, angle, order);
            ex = ex.wrapping_add(sinx(
                angle.wrapping_sub(0x8000_0000u32 as i32),
                SEND_HERE_SPREAD,
            ));
            ey = ey.wrapping_sub(sinx(angle.wrapping_add(0x4000_0000), SEND_HERE_SPREAD));
        }
    }

    // --- Army::find_target ------------------------------------------------------------

    /// `Army::find_target` `0x006F69B0` — **prologue only**, and the three RNG sites.
    ///
    /// The full function is 7,571 bytes and reaches `Armies::find_aggressive_army`,
    /// `Armies::send_navy`, `LeaderData::find_capital` / `has_preq` / `type_avail` /
    /// `get_diff` / `is_tribute_period`, `CityData::get_level` / `num_wonders`,
    /// `Game::wonder_winning`, `ObjectsData::find_building`, `Groups::push_group`,
    /// `Group::action_attack` / `action_move_to` / `action_stance` and `Region::is_coast`.
    /// Porting the body is a lane of its own; `docs/mechanics/armies.md` §"find_target"
    /// records what is read so far so the next lane starts from evidence.
    ///
    /// What *is* transcribed here is the entry gate, because it is self-contained and it is
    /// what decides whether the RNG is reached at all:
    ///
    /// ```text
    /// siege = count(0x13, 0x3F)
    /// if (leaders[who].leader_flags & 0x08) return;        ; targeting disabled
    /// strong = false
    /// if (navy != 0) strong = true
    /// else {
    ///     n = count(4, 0)*2 + count(0x13, 0xE3) + count(0x13, 0x84)
    ///     if (n >= 4) {
    ///         if ((leader.data_encrypted->epoch[0] ^ 0x00063187) < 3) strong = true
    ///         else if (!LeaderData::type_avail(0x3F, 1))              strong = true
    ///         else if (siege != 0)                                    strong = true
    ///     }
    /// }
    /// ```
    ///
    /// The `^ 0x00063187` is not a typo for the object-coordinate key `0x00063637`.
    /// `LeaderDataEncrypt` obfuscates its fields with **per-field** keys — `epoch[0]` at
    /// `+0xE8` uses `0x00063187` and `ages` at `+0xDC` uses `0x00062766` at the third RNG
    /// site. Any live-memory reader of `Leader` must key per field.
    ///
    /// Returns the prologue's `strong` flag; the caller records the body as a gap.
    pub fn find_target_prologue<W: ArmyWorld + ?Sized>(
        &self,
        w: &W,
        epoch0: i32,
        siege_type_available: bool,
    ) -> Option<bool> {
        let who = self.who as usize;
        let siege = self.count(w, COUNT_TYPE, TYPE_ARG_SIEGE);
        if w.leader_flags(who) & LF_NO_TARGETING != 0 {
            return None;
        }
        if self.navy != 0 {
            return Some(true);
        }
        let n = self.count(w, COUNT_MOBILE, 0) * 2
            + self.count(w, COUNT_TYPE, TYPE_ARG_227)
            + self.count(w, COUNT_TYPE, TYPE_ARG_132);
        if n < 4 {
            return Some(false);
        }
        if epoch0 < 3 {
            return Some(true);
        }
        if !siege_type_available {
            return Some(true);
        }
        Some(siege != 0)
    }
}

/// `div_3_table[c >> 8]` `0x00CAE5FC` — the `Coord` → `WCoord` step, `floor(c / 768)`.
///
/// Retail uses `sar` (arithmetic) then a table lookup, so this is `c >> 8` then `/ 3`; the
/// table is indexed by a non-negative value at every site in this class because the shifted
/// coordinate is a map position.
#[inline]
pub fn div3_shift8(c: i32) -> i32 {
    (c >> 8) / 3
}

// ---------------------------------------------------------------------------
// Armies
// ---------------------------------------------------------------------------

/// `Armies`, the singleton at `0x00C09700`: eight `PtrArray<Army>` of sixteen each.
#[derive(Clone, Debug)]
pub struct Armies {
    /// `ArmiesData::lists : PtrArray<Army>[8]`.
    pub lists: Vec<Vec<ArmyData>>,
    /// `ArmiesData::find_dist` `0x00CB4BAC` — a **static**, not a member, shared by
    /// `find_army`, `find_local_army` and `find_useful_army` and reseeded at each entry.
    /// It is not walked, so it is neither save nor checksum state.
    pub find_dist: i32,
}

impl Default for Armies {
    fn default() -> Self {
        Armies::new()
    }
}

impl Armies {
    /// `Armies::init` `0x006F3C10` — 8 lists of 16 default-constructed armies.
    ///
    /// Retail `malloc`s [`ARMY_ALLOC`] per army and pushes the pointer, growing each
    /// `PtrArray` to `length == size == 16`. Nothing ever pushes a seventeenth.
    pub fn new() -> Armies {
        Armies {
            lists: (0..NUM_LEADERS)
                .map(|_| vec![ArmyData::default(); ARMIES_PER_PLAYER])
                .collect(),
            find_dist: FIND_DIST_SEED,
        }
    }

    /// Execute the exact outer dispatcher over retail's preallocated invalid slots.
    ///
    /// A valid `Army` reaches Group, Unit, City, diplomacy, and type-table state. The
    /// lightweight tick does not yet own a complete adapter for those stores, so such
    /// records are counted and the whole call fails closed before any Army mutation. A
    /// newly initialized game has no valid Army records and therefore executes the full
    /// dispatcher without needing that host.
    pub fn process_step13_dispatch(&mut self, inputs: Step13DispatchInputs) -> Step13DispatchTrace {
        let mut world = DispatcherOnlyWorld { inputs };
        let mut missing_live_host_armies = 0u32;
        for who in 0..NUM_LEADERS {
            if !owner_enabled(&world, who) {
                continue;
            }
            missing_live_host_armies = missing_live_host_armies.wrapping_add(
                self.lists[who]
                    .iter()
                    .filter(|army| army.valid != 0)
                    .count() as u32,
            );
        }
        if missing_live_host_armies != 0 {
            let mut process = ArmyProcessTrace::default();
            for who in 0..NUM_LEADERS {
                if owner_enabled(&world, who) {
                    process.owners_enabled = process.owners_enabled.wrapping_add(1);
                    process.slots_examined = process
                        .slots_examined
                        .wrapping_add(self.lists[who].len() as u32);
                }
            }
            return Step13DispatchTrace {
                process,
                missing_live_host_armies,
            };
        }
        Step13DispatchTrace {
            process: self.process_all(&mut world),
            missing_live_host_armies: 0,
        }
    }

    /// `Armies::init_army` `0x006F36A0`.
    ///
    /// First invalid slot wins; otherwise **the live slot with the fewest `num_units`**,
    /// with ties going to the highest index and a starting bound of 9,999 that no real army
    /// exceeds. The retail compare is `<=`, so every equal-sized later slot replaces the
    /// earlier candidate. Returns the slot.
    pub fn init_army<W: ArmyWorld + ?Sized>(&mut self, w: &W, who: usize, city: i32) -> i32 {
        let mut best = 0usize;
        let mut best_units = 0x270F;
        for i in 0..ARMIES_PER_PLAYER {
            if self.lists[who][i].valid == 0 {
                best = i;
                break;
            }
            let n = self.lists[who][i].num_units;
            if n <= best_units {
                best_units = n;
                best = i;
            }
        }
        self.lists[who][best].init(w, best as i32, who as i32, city);
        best as i32
    }

    /// `Armies::init_navy` `0x006F31B0` — `init_army`, then `navy = 1` and an explicit
    /// region rather than the founding city's.
    pub fn init_navy<W: ArmyWorld + ?Sized>(
        &mut self,
        w: &W,
        who: usize,
        city: i32,
        reg: i32,
    ) -> i32 {
        let slot = self.init_army(w, who, city);
        if slot < 0 {
            return slot;
        }
        self.lists[who][slot as usize].navy = 1;
        self.lists[who][slot as usize].reg = reg;
        slot
    }

    /// `Armies::num_armies` `0x006F3200` — count valid armies matching a status mask and,
    /// when `reg >= 0`, a region.
    pub fn num_armies(&self, who: usize, status_mask: i32, reg: i32) -> i32 {
        let mut n = 0;
        for a in &self.lists[who] {
            if a.valid == 0 {
                continue;
            }
            if a.status & status_mask == 0 {
                continue;
            }
            if reg >= 0 && reg != a.reg {
                continue;
            }
            n += 1;
        }
        n
    }

    /// `Armies::find_city` `0x006F3160` — the first **mustering** army assigned to a city.
    pub fn find_city(&self, who: usize, city: i32) -> i32 {
        for (i, a) in self.lists[who].iter().enumerate() {
            if a.valid != 0 && a.city == city && a.status & ST_MUSTERING != 0 {
                return i as i32;
            }
        }
        -1
    }

    /// `Armies::find_aggressive_army` `0x006F2E10` — the first army with at least two
    /// captains, not mustering, **standing outside its owner's own borders**.
    ///
    /// Called only from `Army::find_target`, twice. The border test reads the `wdata` byte
    /// at `+0x0F` under the army's rally point and compares it against `who`.
    pub fn find_aggressive_army<W: ArmyWorld + ?Sized>(&self, w: &W, who: usize) -> i32 {
        for (i, a) in self.lists[who].iter().enumerate() {
            if a.valid == 0 {
                continue;
            }
            if a.num_captains < 2 {
                continue;
            }
            if a.status & ST_MUSTERING != 0 {
                continue;
            }
            let owner = w.tile_owner(div3_shift8(a.x), div3_shift8(a.y));
            if owner != who as i32 {
                return i as i32;
            }
        }
        -1
    }

    /// `Armies::find_useful_army` `0x006F2FE0` — the army with the smallest
    /// **distance per captain** to a point, tripling the cost of a cross-region choice.
    ///
    /// The region of the query point comes from `WorldData::get_tregion`; the caller passes
    /// it in here. Note the `>> 6` in retail before the `div_3_table` lookup — the query
    /// arrives in `Coord` and is reduced to `TCoord`-ish before the region lookup, while the
    /// distance itself is computed in raw `Coord`.
    pub fn find_useful_army<W: ArmyWorld + ?Sized>(
        &mut self,
        w: &W,
        who: usize,
        x: i32,
        y: i32,
        query_region: i32,
    ) -> i32 {
        self.find_dist = FIND_DIST_SEED;
        let mut best = -1i32;
        let anywhere = w.leader_flags(who) & 0x700 != 0;
        for (i, a) in self.lists[who].iter().enumerate() {
            if a.valid == 0 || a.num_captains == 0 {
                continue;
            }
            if !(a.navy != 0 || anywhere) && query_region != a.reg {
                continue;
            }
            let mut d = vector_dist((x - a.x).abs(), (y - a.y).abs());
            if query_region != a.reg {
                d = d.wrapping_mul(3);
            }
            let score = d / a.num_captains;
            if score > self.find_dist {
                continue;
            }
            self.find_dist = score;
            best = i as i32;
        }
        best
    }

    /// `Armies::leader_defeated` `0x006F2F90` — `Army::stop` for every valid army.
    ///
    /// `Army::stop` `0x006F9180` (580 B) clears every member's orders through
    /// `Unit::close_orders` / `Unit::clear_partial_path` / `Unit::update_action` and honours
    /// `ScenarioData::ignore_orders`; it is not ported. This entry point therefore only
    /// counts, and says so.
    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn leader_defeated_research_gap_count(&mut self, who: usize, stopped: &mut u64) {
        for a in &self.lists[who] {
            if a.valid != 0 {
                *stopped += 1;
            }
        }
    }

    /// `Armies::diplo_change` `0x006F30F0` — same owner gate as `process_all`, then a
    /// **forced** `Army::process(1)` for every valid army.
    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn diplo_change_research_partial<W: ArmyWorld + ?Sized>(
        &mut self,
        w: &mut W,
        who: usize,
        gaps: &mut ArmyGaps,
    ) {
        if !owner_enabled(w, who) {
            return;
        }
        let mut i = 0usize;
        while i < self.lists[who].len() {
            if self.lists[who][i].valid != 0 {
                self.process_one_research_partial(w, who, i, true, gaps);
            }
            i += 1;
        }
    }

    /// `Armies::emergency` `0x006F3250` — clear the target and force a re-think.
    ///
    /// Called from `Object::do_damage`: taking a hit at home drops every army's current
    /// target and re-runs the state machine the same frame, which is why an AI reacts to a
    /// raid inside one tick rather than at its next 256-frame phase.
    #[cfg(test)]
    pub(crate) fn emergency_research_partial<W: ArmyWorld + ?Sized>(
        &mut self,
        w: &mut W,
        who: usize,
        gaps: &mut ArmyGaps,
    ) {
        if !owner_enabled(w, who) {
            return;
        }
        let mut i = 0usize;
        while i < self.lists[who].len() {
            if self.lists[who][i].valid != 0 {
                self.lists[who][i].target_o = -1;
                self.lists[who][i].target_who = -1;
                self.process_one_research_partial(w, who, i, true, gaps);
            }
            i += 1;
        }
    }

    /// `Armies::update_city` `0x006F2D70` — retarget every army of every player that was
    /// pointed at `(old_o, old_who)`.
    ///
    /// The owner gate here is **weaker** than `process_all`'s: it tests `leader_flags & 1`
    /// and `(leader_flags & 0xC) != 4` but **not** `leader_flags2 & 0xA`. Setting the new
    /// owner to 0 additionally raises [`ST_HURRY`], which `process_all` consumes on its next
    /// pass. `Cities::capture_city` `0x00733380` is its only caller.
    pub fn update_city<W: ArmyWorld + ?Sized>(
        &mut self,
        w: &W,
        old_o: i32,
        old_who: i32,
        new_o: i32,
        new_who: i32,
    ) {
        for who in 0..NUM_LEADERS {
            let f = w.leader_flags(who);
            if f & LF_ACTIVE == 0 {
                continue;
            }
            if f & LF_KIND_MASK == LF_KIND_SKIP {
                continue;
            }
            for a in self.lists[who].iter_mut() {
                if a.valid == 0 {
                    continue;
                }
                if a.target_o != old_o || a.target_who != old_who {
                    continue;
                }
                a.target_o = new_o;
                a.target_who = new_who;
                if new_who == 0 {
                    a.status |= ST_HURRY;
                }
            }
        }
    }

    // --- the tick step ----------------------------------------------------------------

    /// **`Armies::process_all` `0x006F3B00` — step 13 of `Game::do_frame`.**
    ///
    /// ```text
    /// for who in 0..8:
    ///     L = leaders[who]
    ///     if !(L.leader_flags & 1)              continue
    ///     if  (L.leader_flags & 0xC) == 4       continue
    ///     if  (L.leader_flags2 & 0xA)           continue
    ///     for i in 0 .. armies.lists[who].length:      ; re-read every iteration
    ///         a = armies.lists[who].list[i]
    ///         if (a.valid == 0) continue
    ///         if (a.status & 0x80) { a.status &= ~0x80; a.process(1) }
    ///         else                 {                    a.process(0) }
    /// ```
    ///
    /// Two properties a reimplementation must not lose. The list length is loaded from
    /// memory at the top **and** bottom of the inner loop, so an `Army::close` that shrank
    /// the list would be seen immediately — it never does, because `close` only clears
    /// `valid`. And the owner loop is a **fixed 0..7 in index order**, with none of the
    /// `(frame + i) % 10` rotation `Objects::process_all` applies: army order is stable
    /// across frames, unit order is not.
    pub fn process_all<W: ArmyWorld + ?Sized>(&mut self, w: &mut W) -> ArmyProcessTrace {
        let mut trace = ArmyProcessTrace::default();
        for who in 0..NUM_LEADERS {
            if !owner_enabled(w, who) {
                continue;
            }
            trace.owners_enabled += 1;
            let mut i = 0usize;
            while i < self.lists[who].len() {
                trace.slots_examined += 1;
                if self.lists[who][i].valid != 0 {
                    let hurry = self.lists[who][i].status & ST_HURRY != 0;
                    if hurry {
                        self.lists[who][i].status &= !ST_HURRY;
                    }
                    if self.process_one(w, who, i, hurry, &mut trace.gaps) {
                        trace.heavy_passes += 1;
                    }
                    trace.armies_processed += 1;
                }
                i += 1;
            }
        }
        trace
    }

    #[cfg(test)]
    pub(crate) fn process_all_research_partial<W: ArmyWorld + ?Sized>(
        &mut self,
        w: &mut W,
        gaps: &mut ArmyGaps,
    ) -> u32 {
        let trace = self.process_all(w);
        gaps.merge(&trace.gaps);
        trace.armies_processed
    }

    /// Research transcription of `Army::process` `0x006F93D0`'s outer state machine.
    ///
    /// Every dispatched retail body listed in [`RUNTIME_FIDELITY_BLOCKERS`] is counted and
    /// skipped. This method is therefore test-only and explicitly named
    /// `_research_partial`; it cannot be wired into a product or fidelity tick.
    ///
    /// It is a method on [`Armies`] rather than [`ArmyData`] because the merge branch scans
    /// this owner's other fifteen armies.
    ///
    /// ```text
    /// if (human_frame) human_frame--
    /// if (!hurry) {
    ///     off = (army + who*2) * 2
    ///     if ((frame - 30 + off) % 128 == 0) {          ; the light pass
    ///         if (leader.flags & 0x40) return
    ///         normalize()
    ///         if (num_captains == 0) return
    ///         if (num_standard && count(0x13,0x36)) use_generals()
    ///         if (count(0x13,0x3A))                 use_spies()
    ///         if (count(0x13,0x45))                 use_scouts()
    ///     }
    ///     if ((frame + off) % 256 != 0) return          ; the heavy pass gate
    /// }
    /// if (leader.flags & 0x40) return
    /// normalize()
    /// <retire>  <human order>  <merge>  <retarget>  <dispatch>
    /// ```
    ///
    /// So an un-hurried army thinks hard once every 256 frames — **17.2 s at 67 ms/tick** —
    /// spread across armies and owners by `off`, and does the general/spy/scout housekeeping
    /// twice as often on a separate 128-frame phase biased 30 frames earlier. `Random::get`
    /// for artificial lag is *not* what staggers this; `off` is deterministic.
    ///
    /// Returns `true` if the heavy pass ran.
    fn process_one<W: ArmyWorld + ?Sized>(
        &mut self,
        w: &mut W,
        who: usize,
        idx: usize,
        hurry: bool,
        gaps: &mut ArmyGaps,
    ) -> bool {
        {
            let a = &mut self.lists[who][idx];
            if a.human_frame != 0 {
                a.human_frame -= 1;
            }
        }

        if !hurry {
            let (army, aw) = {
                let a = &self.lists[who][idx];
                (a.army as i32, a.who as i32)
            };
            let off = phase_offset(army, aw);
            let frame = w.frame();
            if (frame + PHASE_LIGHT_BIAS + off) % PHASE_LIGHT == 0 {
                if w.leader_flags(who) & LF_ARMIES_OFF != 0 {
                    return false;
                }
                self.lists[who][idx].normalize(w);
                if self.lists[who][idx].num_captains == 0 {
                    return false;
                }
                let a = self.lists[who][idx].clone();
                if a.num_standard != 0 && a.count(w, COUNT_TYPE, TYPE_ARG_GENERAL) != 0 {
                    gaps.use_generals += 1;
                }
                if a.count(w, COUNT_TYPE, TYPE_ARG_SPY) != 0 {
                    gaps.use_spies += 1;
                }
                if a.count(w, COUNT_TYPE, TYPE_ARG_SCOUT) != 0 {
                    gaps.use_scouts += 1;
                }
            }
            if (frame + off) % PHASE_HEAVY != 0 {
                return false;
            }
        }

        if w.leader_flags(who) & LF_ARMIES_OFF != 0 {
            return false;
        }
        self.lists[who][idx].normalize(w);

        // --- retire: no standard units left --------------------------------------------
        {
            let a = &self.lists[who][idx];
            if a.num_standard <= 0 && a.valid != 0 && a.status & ST_MUSTERING == 0 {
                let mut cursor = 0i32;
                let n = w.leader_city_num(who);
                let mut send: Option<(i32, i32)> = None;
                if n > 0 {
                    loop {
                        if w.city_flags(who, cursor) & 1 != 0 {
                            send = Some(w.city_pos(who, cursor));
                            break;
                        }
                        cursor += 1;
                        if cursor >= n {
                            break;
                        }
                    }
                }
                let a = &mut self.lists[who][idx];
                a.city = cursor;
                if let Some((cx, cy)) = send {
                    let mut owned = a.clone();
                    owned.send_here(w, cx, cy, 1);
                    self.lists[who][idx] = owned;
                }
                self.lists[who][idx].close(w);
                return true;
            }
        }

        // --- an explicit human order outranks everything --------------------------------
        {
            let a = self.lists[who][idx].clone();
            if a.human_frame != 0 {
                let mut owned = a;
                let (x, y) = (owned.x, owned.y);
                owned.send_here(w, x, y, 2);
                self.lists[who][idx] = owned;
                return true;
            }
        }

        // --- merge into a healthier sibling ---------------------------------------------
        if self.try_merge(w, who, idx) {
            return true;
        }

        // --- retarget -------------------------------------------------------------------
        {
            let moving = {
                let a = self.lists[who][idx].clone();
                a.is_moving(w)
            };
            let engaged = if moving {
                true
            } else {
                let mut a = self.lists[who][idx].clone();
                let e = a.is_engaged(w);
                self.lists[who][idx] = a;
                e
            };
            if !moving && !engaged {
                let cleared = self.lists[who][idx].status & !ST_RETARGET_MASK;
                self.lists[who][idx].status = cleared;
                let (target_o, target_who, navy) = {
                    let a = &self.lists[who][idx];
                    (a.target_o, a.target_who, a.navy)
                };
                if target_o >= 0 && target_who >= 0 {
                    if w.leader_is_enemy(who, target_who) && navy == 0 {
                        let (ox, oy) = w.object_pos(target_who as usize, target_o);
                        let a = &mut self.lists[who][idx];
                        a.muster_x = (div3_shift8(ox) + a.muster_x) / 2;
                        a.muster_y = (div3_shift8(oy) + a.muster_y) / 2;
                        a.status = cleared | ST_RETARGET_MASK;
                    } else {
                        gaps.find_muster_spot += 1;
                        self.lists[who][idx].status |= ST_RETARGET_MASK;
                    }
                }
            }
        }

        // --- dispatch --------------------------------------------------------------------
        {
            let status = self.lists[who][idx].status;
            if status & ST_DEAD_END_MASK == status {
                self.lists[who][idx].status = ST_MARCHING;
            }
        }
        let status = self.lists[who][idx].status;
        if status & ST_MUSTERING != 0 {
            let a = self.lists[who][idx].clone();
            a.set_stance(w, 1);
            gaps.do_mustering += 1;
        }
        if status & ST_DEFENDING != 0 {
            let a = self.lists[who][idx].clone();
            a.set_stance(w, 1);
            gaps.do_defending += 1;
        }
        if status & ST_MARCHING != 0 {
            let a = self.lists[who][idx].clone();
            a.set_stance(w, 0);
            gaps.do_marching += 1;
            gaps.game_random_stream_unresolved += 1;
        }
        if status & ST_FORMING != 0 {
            gaps.do_forming += 1;
        } else {
            let mut a = self.lists[who][idx].clone();
            let e = a.is_engaged(w);
            self.lists[who][idx] = a;
            if e {
                gaps.engagement += 1;
            }
        }
        if status & ST_TRANSPORTING != 0 {
            gaps.do_transporting += 1;
            gaps.game_random_stream_unresolved += 1;
        }
        if self.lists[who][idx].navy != 0 {
            let a = self.lists[who][idx].clone();
            a.set_stance(w, 3);
        }
        true
    }

    #[cfg(test)]
    pub(crate) fn process_one_research_partial<W: ArmyWorld + ?Sized>(
        &mut self,
        w: &mut W,
        who: usize,
        idx: usize,
        hurry: bool,
        gaps: &mut ArmyGaps,
    ) -> bool {
        self.process_one(w, who, idx, hurry, gaps)
    }

    /// The merge branch of `Army::process`, `0x006F9588`..`0x006F9792`.
    ///
    /// A shrunken army — `num_standard < (num_captains - num_decoys)/2` — scans **all
    /// sixteen** of its owner's slots for a land army with the same `reg`, more than four
    /// standard units and a healthy ratio of its own, and pours every live member of every
    /// one of its groups into it via `add_unit`, then closes.
    ///
    /// The scan cannot pick the army itself: the candidate test is the exact negation of the
    /// entry test, so `this` is excluded by construction rather than by an index compare.
    ///
    /// The transfer walks groups **backwards** and members **backwards** inside each group,
    /// and `add_unit` appends to the destination's group 0, so the merged army's member
    /// order is the reverse of the source's. That ordering is visible in the `groups`
    /// checksum channel.
    fn try_merge<W: ArmyWorld + ?Sized>(&mut self, w: &mut W, who: usize, idx: usize) -> bool {
        let (num_standard, num_captains, num_decoys, reg) = {
            let a = &self.lists[who][idx];
            (a.num_standard, a.num_captains, a.num_decoys, a.reg)
        };
        if num_standard >= (num_captains - num_decoys) / 2 {
            return false;
        }
        let mut dest: Option<usize> = None;
        for j in 0..ARMIES_PER_PLAYER {
            let o = &self.lists[who][j];
            if o.valid == 0 || o.navy != 0 || o.reg != reg {
                continue;
            }
            if o.num_standard <= 4 {
                continue;
            }
            if o.num_standard >= (o.num_captains - o.num_decoys) / 2 {
                dest = Some(j);
                break;
            }
        }
        let dest = match dest {
            Some(d) => d,
            None => return false,
        };

        let src = self.lists[who][idx].clone();
        let mut g = src.num_groups as i32 - 1;
        while g >= 0 {
            let gid = src.list[g as usize];
            if gid >= 0 {
                let mut k = w.group_num(gid) - 1;
                while k >= 0 {
                    let o = w.group_member(gid, k);
                    if w.object_alive(who, o) {
                        let mut d = self.lists[who][dest].clone();
                        d.add_unit(w, o);
                        self.lists[who][dest] = d;
                    }
                    k -= 1;
                }
            }
            g -= 1;
        }
        let mut a = self.lists[who][idx].clone();
        a.close(w);
        self.lists[who][idx] = a;
        true
    }

    // --- walking ----------------------------------------------------------------------

    /// `Armies::walk_data` `0x006F3700` — the save-game image of all eight lists.
    ///
    /// Each `PtrArray<Army>` contributes its `length` (i32), its `size` (i32), its
    /// `increment` (i16 at `+0x0C`) and its `flags` byte **with bit 6 masked off**
    /// (`and byte [esi+0x14], 0xBF`), then every element's `Army::walk_data`. That the
    /// capacity and growth hint are hashed is the `Array<T>` property `CODEX.md` warns
    /// about; a `Vec` with its own growth policy diverges here on identical logical state.
    ///
    /// **This walk feeds save/load and `GameLog::say_checksum`, not `CheckSums::check_all`**
    /// — see the module header.
    pub fn walk(&self, cs: &mut CheckSum) {
        for list in &self.lists {
            let length = list.len() as i32;
            cs.walk(&length.to_le_bytes());
            cs.walk(&length.to_le_bytes()); // size == length after Armies::init
            cs.walk(&(-1i16).to_le_bytes()); // increment, as Armies::init writes it
            cs.walk(&[0u8]); // flags & 0xBF
            for a in list {
                a.walk(cs);
            }
        }
    }

    /// Bytes an [`Armies::walk`] would hash, useful as a cheap structural assertion.
    pub fn walked_len(&self) -> usize {
        self.lists
            .iter()
            .map(|l| 11 + l.iter().map(|a| a.walked_len()).sum::<usize>())
            .sum()
    }
}

/// The owner gate shared by `Armies::process_all`, `diplo_change` and `emergency`.
///
/// `update_city` uses a **weaker** form of this and is deliberately not routed here.
#[inline]
pub fn owner_enabled<W: ArmyWorld + ?Sized>(w: &W, who: usize) -> bool {
    let f = w.leader_flags(who);
    if f & LF_ACTIVE == 0 {
        return false;
    }
    if f & LF_KIND_MASK == LF_KIND_SKIP {
        return false;
    }
    if w.leader_flags2(who) & LF2_SKIP_MASK != 0 {
        return false;
    }
    true
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovered_army_driver_is_fail_closed_for_runtime_fidelity() {
        assert!(!RUNTIME_FIDELITY_READY);
        assert!(!RUNTIME_FIDELITY_BLOCKERS.is_empty());
        assert!(RUNTIME_FIDELITY_BLOCKERS
            .iter()
            .any(|s| s.contains("find_target body")));
    }

    /// A minimal in-memory world. It is a *test double*, not a port: every answer is
    /// whatever the test set, so a test that passes here says the `Army` code walked the
    /// control flow the disassembly walks, and nothing about `Group`, `Unit` or `Leader`.
    #[derive(Clone, Debug)]
    struct TestWorld {
        frame: i32,
        width: i32,
        height: i32,
        leader_flags: [u32; NUM_LEADERS],
        leader_flags2: [u32; NUM_LEADERS],
        city_num: [i32; NUM_LEADERS],
        enemies: Vec<(usize, i32)>,
        cities: Vec<(usize, i32, i32, i32, i32)>, // who, idx, flags, x, y
        groups: Vec<TestGroup>,
        objects: Vec<TestObj>,
        regions: i32,
        owner_tile: i32,
        halted: Vec<i32>,
        stances: Vec<(i32, i32)>,
        moves: Vec<(i32, i32, i32, i32, i32)>,
        next_gid: i32,
    }

    #[derive(Clone, Debug, Default)]
    struct TestGroup {
        id: i32,
        army: i32,
        num: i32,
        role: i32,
        buildings: bool,
        who: i32,
        list: Vec<i32>,
        num_cap: i32,
        counts: Vec<((i32, i32), i32)>,
        leader: i32,
        stance_type: i32,
    }

    #[derive(Clone, Debug, Default)]
    struct TestObj {
        who: usize,
        o: i32,
        alive: bool,
        on_map: bool,
        x: i32,
        y: i32,
        action: i32,
        order: Option<bool>,
        type_category: i32,
    }

    impl TestWorld {
        fn new() -> TestWorld {
            TestWorld {
                frame: 0,
                width: 100,
                height: 100,
                leader_flags: [LF_ACTIVE; NUM_LEADERS],
                leader_flags2: [0; NUM_LEADERS],
                city_num: [0; NUM_LEADERS],
                enemies: Vec::new(),
                cities: Vec::new(),
                groups: Vec::new(),
                objects: Vec::new(),
                regions: 1,
                owner_tile: 0,
                halted: Vec::new(),
                stances: Vec::new(),
                moves: Vec::new(),
                next_gid: 0,
            }
        }
        fn push_group(&mut self, who: i32, members: &[i32], num_cap: i32) -> i32 {
            let id = self.next_gid;
            self.next_gid += 1;
            self.groups.push(TestGroup {
                id,
                army: -1,
                num: members.len() as i32,
                role: 1 << (id.min(20)),
                buildings: false,
                who,
                list: members.to_vec(),
                num_cap,
                counts: Vec::new(),
                leader: members.first().copied().unwrap_or(-1),
                stance_type: 0,
            });
            for &m in members {
                self.objects.push(TestObj {
                    who: who as usize,
                    o: m,
                    alive: true,
                    on_map: true,
                    x: 1000 + m * 100,
                    y: 2000 + m * 100,
                    action: 0,
                    order: None,
                    type_category: 10 + m,
                });
            }
            id
        }
        fn g(&self, gid: i32) -> &TestGroup {
            &self.groups[gid as usize]
        }
        fn obj(&self, who: usize, o: i32) -> Option<&TestObj> {
            self.objects.iter().find(|t| t.who == who && t.o == o)
        }
    }

    impl ArmyWorld for TestWorld {
        fn frame(&self) -> i32 {
            self.frame
        }
        fn world_width(&self) -> i32 {
            self.width
        }
        fn world_height(&self) -> i32 {
            self.height
        }
        fn tile_region(&self, _wx: i32, _wy: i32) -> i32 {
            self.regions
        }
        fn tile_owner(&self, _wx: i32, _wy: i32) -> i32 {
            self.owner_tile
        }
        fn leader_flags(&self, who: usize) -> u32 {
            self.leader_flags[who]
        }
        fn leader_flags2(&self, who: usize) -> u32 {
            self.leader_flags2[who]
        }
        fn leader_city_num(&self, who: usize) -> i32 {
            self.city_num[who]
        }
        fn leader_is_enemy(&self, who: usize, other: i32) -> bool {
            self.enemies.contains(&(who, other))
        }
        fn city_flags(&self, who: usize, city: i32) -> i32 {
            self.cities
                .iter()
                .find(|c| c.0 == who && c.1 == city)
                .map(|c| c.2)
                .unwrap_or(0)
        }
        fn city_reg(&self, _who: usize, _city: i32) -> i32 {
            self.regions
        }
        fn city_pos(&self, who: usize, city: i32) -> (i32, i32) {
            self.cities
                .iter()
                .find(|c| c.0 == who && c.1 == city)
                .map(|c| (c.3, c.4))
                .unwrap_or((0, 0))
        }
        fn object_alive(&self, who: usize, o: i32) -> bool {
            self.obj(who, o).map(|t| t.alive).unwrap_or(false)
        }
        fn object_pos(&self, who: usize, o: i32) -> (i32, i32) {
            self.obj(who, o).map(|t| (t.x, t.y)).unwrap_or((0, 0))
        }
        fn unit_on_map(&self, who: usize, o: i32) -> bool {
            self.obj(who, o).map(|t| t.on_map).unwrap_or(false)
        }
        fn unit_action_type(&self, who: usize, o: i32) -> i32 {
            self.obj(who, o).map(|t| t.action).unwrap_or(-1)
        }
        fn unit_order_active(&self, who: usize, o: i32) -> Option<bool> {
            self.obj(who, o).and_then(|t| t.order)
        }
        fn unit_type_category(&self, who: usize, o: i32) -> i32 {
            self.obj(who, o).map(|t| t.type_category).unwrap_or(0)
        }
        fn group_id(&self, gid: i32) -> i32 {
            self.g(gid).id
        }
        fn group_army(&self, gid: i32) -> i32 {
            self.g(gid).army
        }
        fn set_group_army(&mut self, gid: i32, army: i32) {
            self.groups[gid as usize].army = army;
        }
        fn group_num(&self, gid: i32) -> i32 {
            self.g(gid).num
        }
        fn group_role(&self, gid: i32) -> i32 {
            self.g(gid).role
        }
        fn group_buildings(&self, gid: i32) -> bool {
            self.g(gid).buildings
        }
        fn group_who(&self, gid: i32) -> i32 {
            self.g(gid).who
        }
        fn group_member(&self, gid: i32, k: i32) -> i32 {
            self.g(gid).list.get(k as usize).copied().unwrap_or(-1)
        }
        fn group_num_cap(&self, gid: i32) -> i32 {
            self.g(gid).num_cap
        }
        fn group_count(&self, gid: i32, ci: i32, arg: i32) -> i32 {
            self.g(gid)
                .counts
                .iter()
                .find(|((c, a), _)| *c == ci && *a == arg)
                .map(|(_, v)| *v)
                .unwrap_or(0)
        }
        fn group_find_leader(&self, gid: i32) -> i32 {
            self.g(gid).leader
        }
        fn group_stance_type(&self, gid: i32) -> i32 {
            self.g(gid).stance_type
        }
        fn group_normalize(&mut self, _gid: i32) {}
        fn group_action_halt(&mut self, gid: i32) {
            self.halted.push(gid);
        }
        fn group_action_stance(&mut self, gid: i32, stance: i32) {
            self.stances.push((gid, stance));
        }
        fn group_action_move_to(&mut self, gid: i32, x: i32, y: i32, angle: i32, order: i32) {
            self.moves.push((gid, x, y, angle, order));
        }
        fn push_singleton_group(&mut self, who: usize, o: i32) -> i32 {
            self.push_group(who as i32, &[o], 1)
        }
        fn unit_set_group(&mut self, _who: usize, _o: i32, _gid: i32) {}
        fn group_add_member(&mut self, gid: i32, o: i32, _who: usize) {
            let g = &mut self.groups[gid as usize];
            if !g.list.contains(&o) {
                g.list.push(o);
                g.num += 1;
            }
        }
    }

    fn armies_with(w: &mut TestWorld, who: usize, groups: &[i32]) -> Armies {
        let mut ar = Armies::new();
        ar.lists[who][0].init(w, 0, who as i32, -1);
        for &g in groups {
            ar.lists[who][0].add_group(w, g);
        }
        ar
    }

    // --- layout / walk ----------------------------------------------------------------

    #[test]
    fn the_image_is_the_pdb_layout() {
        let mut a = ArmyData::default();
        a.valid = 1;
        a.army = 0x0203;
        a.status = 0x0A0B0C0D;
        a.who = 7;
        a.num_groups = 3;
        a.list[0] = 0x11223344;
        let img = a.image();
        assert_eq!(img.len(), 152);
        assert_eq!(&img[0..2], &1i16.to_le_bytes());
        assert_eq!(&img[2..4], &0x0203i16.to_le_bytes());
        assert_eq!(&img[4..8], &0x0A0B0C0Di32.to_le_bytes());
        // ArmyData::list is at +0x54 and who/num_groups at +0x94/+0x96.
        assert_eq!(&img[84..88], &0x11223344i32.to_le_bytes());
        assert_eq!(&img[148..150], &7i16.to_le_bytes());
        assert_eq!(&img[150..152], &3i16.to_le_bytes());
    }

    #[test]
    fn an_invalid_army_still_walks_two_bytes() {
        let dead = ArmyData::default();
        assert_eq!(dead.walked_len(), 2);
        let mut live = ArmyData::default();
        live.valid = 1;
        assert_eq!(live.walked_len(), 152);

        let mut cs = CheckSum::default();
        dead.walk(&mut cs);
        let only_head = cs.value;
        let mut cs2 = CheckSum::default();
        live.walk(&mut cs2);
        assert_ne!(only_head, cs2.value);
    }

    #[test]
    fn a_fresh_armies_walks_eight_lists_of_sixteen_dead_slots() {
        let ar = Armies::new();
        // 8 * (4 + 4 + 2 + 1 header bytes + 16 * 2) = 8 * 43
        assert_eq!(ar.walked_len(), 8 * (11 + 16 * 2));
    }

    // --- container ---------------------------------------------------------------------

    #[test]
    fn init_preallocates_sixteen_armies_per_player() {
        let ar = Armies::new();
        assert_eq!(ar.lists.len(), NUM_LEADERS);
        for l in &ar.lists {
            assert_eq!(l.len(), ARMIES_PER_PLAYER);
            assert!(l.iter().all(|a| a.valid == 0));
        }
    }

    #[test]
    fn init_army_takes_the_first_free_slot_then_the_emptiest() {
        let mut w = TestWorld::new();
        let mut ar = Armies::new();
        assert_eq!(ar.init_army(&w, 0, -1), 0);
        assert_eq!(ar.init_army(&w, 0, -1), 1);
        // fill the rest
        for _ in 2..ARMIES_PER_PLAYER {
            ar.init_army(&w, 0, -1);
        }
        assert!(ar.lists[0].iter().all(|a| a.valid != 0));
        // Now every slot is live; the fewest-units slot wins, ties to the *last* index
        // because the retail compare is `<=`.
        for (i, a) in ar.lists[0].iter_mut().enumerate() {
            a.num_units = 100 - i as i32;
        }
        ar.lists[0][5].num_units = 1;
        assert_eq!(ar.init_army(&mut w, 0, -1), 5);
    }

    #[test]
    fn init_seeds_the_rally_point_from_the_city_plus_768_south() {
        let mut w = TestWorld::new();
        w.cities.push((0, 0, 1, 5000, 6000));
        let mut a = ArmyData::default();
        a.init(&w, 3, 0, 0);
        assert_eq!(a.army, 3);
        assert_eq!(a.who, 0);
        assert_eq!(a.x, 5000);
        assert_eq!(a.y, 6000 + ARMY_INIT_Y_OFFSET);
        assert_eq!(a.muster_x, div3_shift8(5000));
        assert_eq!(a.muster_y, div3_shift8(6000 + ARMY_INIT_Y_OFFSET));
        assert_eq!(a.status, ST_MUSTERING);
        assert_eq!(a.valid, 1);
        assert_eq!(a.list, [-1; ARMY_MAX_GROUPS]);
    }

    #[test]
    fn init_with_no_city_writes_minus_one_not_zero() {
        let w = TestWorld::new();
        let mut a = ArmyData::default();
        a.init(&w, 0, 0, -1);
        assert_eq!(
            (a.reg, a.x, a.y, a.muster_x, a.muster_y),
            (-1, -1, -1, -1, -1)
        );
    }

    // --- membership --------------------------------------------------------------------

    #[test]
    fn add_group_caps_at_sixteen_and_rejects_a_foreign_group() {
        let mut w = TestWorld::new();
        let mut a = ArmyData::default();
        a.init(&w, 0, 0, -1);
        for _ in 0..ARMY_MAX_GROUPS {
            let g = w.push_group(0, &[1], 1);
            assert!(a.add_group(&mut w, g));
        }
        assert_eq!(a.num_groups as usize, ARMY_MAX_GROUPS);
        let g = w.push_group(0, &[1], 1);
        assert!(!a.add_group(&mut w, g));

        let mut b = ArmyData::default();
        b.init(&w, 1, 0, -1);
        let foreign = w.push_group(3, &[1], 1);
        assert!(!b.add_group(&mut w, foreign));
        assert_eq!(b.num_groups, 0);
    }

    #[test]
    fn add_group_links_both_directions_and_accumulates() {
        let mut w = TestWorld::new();
        let g0 = w.push_group(0, &[1, 2, 3], 2);
        let g1 = w.push_group(0, &[4, 5], 1);
        let mut a = ArmyData::default();
        a.init(&w, 4, 0, -1);
        a.add_group(&mut w, g0);
        a.add_group(&mut w, g1);
        assert_eq!(a.num_groups, 2);
        assert_eq!(a.num_units, 5);
        assert_eq!(a.num_captains, 3);
        assert_eq!(w.group_army(g0), 4);
        assert_eq!(w.group_army(g1), 4);
        assert!(a.member_group(w.group_id(g0)));
        assert!(a.member(&w, 5, 0));
        assert!(!a.member(&w, 99, 0));
    }

    #[test]
    fn close_unlinks_only_groups_that_still_name_this_army() {
        let mut w = TestWorld::new();
        let g0 = w.push_group(0, &[1], 1);
        let g1 = w.push_group(0, &[2], 1);
        let mut a = ArmyData::default();
        a.init(&w, 2, 0, -1);
        a.add_group(&mut w, g0);
        a.add_group(&mut w, g1);
        // g1 gets re-homed under another army before close
        w.set_group_army(g1, 9);
        a.close(&mut w);
        assert_eq!(w.group_army(g0), -1);
        assert_eq!(w.group_army(g1), 9);
        assert_eq!(w.halted, vec![g0]);
        assert_eq!(a.valid, 0);
        assert_eq!(a.status, 0);
        assert_eq!(a.num_groups, 0);
        // close deliberately leaves the list contents alone
        assert_eq!(a.list[0], w.group_id(g0));
    }

    #[test]
    fn remove_group_compacts_and_renormalizes() {
        let mut w = TestWorld::new();
        let g0 = w.push_group(0, &[1], 1);
        let g1 = w.push_group(0, &[2], 1);
        let g2 = w.push_group(0, &[3], 1);
        let mut a = ArmyData::default();
        a.init(&w, 0, 0, -1);
        a.add_group(&mut w, g0);
        a.add_group(&mut w, g1);
        a.add_group(&mut w, g2);
        a.remove_group(&mut w, g1);
        assert_eq!(a.num_groups, 2);
        // remove_group re-enters normalize, whose category sort places g2 (cat 13)
        // ahead of g0 (cat 11).
        assert_eq!(&a.list[..2], &[w.group_id(g2), w.group_id(g0)]);
        assert_eq!(w.group_army(g1), -1);
    }

    #[test]
    fn get_unit_flattens_groups_and_skips_building_groups() {
        let mut w = TestWorld::new();
        let g0 = w.push_group(0, &[10, 11], 2);
        let gb = w.push_group(0, &[12, 13], 2);
        w.groups[gb as usize].buildings = true;
        let g1 = w.push_group(0, &[14], 1);
        let mut a = ArmyData::default();
        a.init(&w, 0, 0, -1);
        a.add_group(&mut w, g0);
        a.add_group(&mut w, gb);
        a.add_group(&mut w, g1);
        assert_eq!(a.num_units, 5);
        assert_eq!(a.get_unit(&w, 0), 10);
        assert_eq!(a.get_unit(&w, 1), 11);
        assert_eq!(a.get_unit(&w, 2), 14);
        // index 3 and 4 exist in `num_units` but not in the flattened space
        assert_eq!(a.get_unit(&w, 3), -1);
    }

    // --- normalize ---------------------------------------------------------------------

    #[test]
    fn normalize_recomputes_the_five_aggregates() {
        let mut w = TestWorld::new();
        let g0 = w.push_group(0, &[1, 2, 3], 3);
        w.groups[g0 as usize].counts = vec![
            ((COUNT_DECOY, 0), 1),
            ((COUNT_SPELLCASTER, 0), 1),
            ((COUNT_TYPE, TYPE_ARG_SIEGE), 1),
        ];
        let mut ar = armies_with(&mut w, 0, &[g0]);
        ar.lists[0][0].num_units = 999;
        ar.lists[0][0].normalize(&mut w);
        let a = &ar.lists[0][0];
        assert_eq!(a.num_units, 3);
        assert_eq!(a.num_captains, 3);
        assert_eq!(a.num_decoys, 1);
        // 0 - 1 (caster) - 1 (siege) - 1 (decoy) - 0 + 3 (cap) = 0
        assert_eq!(a.num_standard, 0);
        assert_eq!(a.role, w.group_role(g0));
    }

    #[test]
    fn normalize_drops_an_emptied_group_and_returns_immediately() {
        let mut w = TestWorld::new();
        let g0 = w.push_group(0, &[1], 1);
        let g1 = w.push_group(0, &[2, 3], 2);
        let mut ar = armies_with(&mut w, 0, &[g0, g1]);
        // the backwards walk sees g1 first; empty it
        w.groups[g1 as usize].num = 0;
        ar.lists[0][0].normalize(&mut w);
        assert_eq!(ar.lists[0][0].num_groups, 1);
        assert_eq!(ar.lists[0][0].list[0], w.group_id(g0));
        // the early return means g0 was never accumulated on this pass by the outer walk;
        // remove_group's own normalize did it instead, so the counts are g0's alone.
        assert_eq!(ar.lists[0][0].num_units, 1);
    }

    #[test]
    fn normalize_sorts_groups_by_leader_type_category_descending() {
        let mut w = TestWorld::new();
        let low_category = w.push_group(0, &[1], 1);
        let high_category = w.push_group(0, &[2], 1);
        for o in w.objects.iter_mut() {
            o.type_category = if o.o == 2 { 500 } else { 5 };
        }
        let mut ar = armies_with(&mut w, 0, &[low_category, high_category]);
        ar.lists[0][0].normalize(&mut w);
        assert_eq!(ar.lists[0][0].list[0], w.group_id(high_category));
        assert_eq!(ar.lists[0][0].list[1], w.group_id(low_category));
    }

    // --- geometry ----------------------------------------------------------------------

    #[test]
    fn center_of_gravity_falls_back_to_the_rally_point() {
        let w = TestWorld::new();
        let mut a = ArmyData::default();
        a.init(&w, 0, 0, -1);
        a.x = 7680;
        a.y = 15360;
        assert_eq!(
            a.center_of_gravity(&w),
            (div3_shift8(7680), div3_shift8(15360))
        );
    }

    #[test]
    fn center_of_gravity_averages_live_members_in_wcoord() {
        let mut w = TestWorld::new();
        let g = w.push_group(0, &[0, 1], 2);
        // objects 0 and 1 sit at (1000,2000) and (1100,2100)
        let mut ar = armies_with(&mut w, 0, &[g]);
        ar.lists[0][0].normalize(&mut w);
        let (cx, cy) = ar.lists[0][0].center_of_gravity(&w);
        assert_eq!(cx, (div3_shift8(1000) + div3_shift8(1100)) / 2);
        assert_eq!(cy, (div3_shift8(2000) + div3_shift8(2100)) / 2);
    }

    #[test]
    fn is_engaged_needs_more_than_a_quarter_attacking() {
        let mut w = TestWorld::new();
        let g = w.push_group(0, &[0, 1, 2, 3], 4);
        for o in w.objects.iter_mut() {
            o.x = 1000;
            o.y = 2000;
        }
        let mut ar = armies_with(&mut w, 0, &[g]);
        ar.lists[0][0].x = 1000;
        ar.lists[0][0].y = 2000;
        // exactly one attacking out of four: 1 > 4/4 == 1 is false
        w.objects[0].action = 10;
        let mut a = ar.lists[0][0].clone();
        assert!(!a.is_engaged(&mut w));
        // two of four: 2 > 1 is true
        w.objects[1].action = 10;
        let mut a = ar.lists[0][0].clone();
        assert!(a.is_engaged(&mut w));
    }

    #[test]
    fn is_moving_counts_an_idle_unit_only_when_it_left_the_region() {
        let mut w = TestWorld::new();
        let g = w.push_group(0, &[0, 1, 2], 3);
        for o in w.objects.iter_mut() {
            o.order = Some(false);
        }
        let mut ar = armies_with(&mut w, 0, &[g]);
        ar.lists[0][0].normalize(&mut w);
        ar.lists[0][0].reg = 1;
        w.regions = 1;
        assert!(!ar.lists[0][0].is_moving(&w));
        w.regions = 2; // every member now reads as out-of-region
        assert!(ar.lists[0][0].is_moving(&w));
    }

    // --- send_here ---------------------------------------------------------------------

    #[test]
    fn send_here_clamps_to_the_map_and_spreads_the_groups() {
        let mut w = TestWorld::new();
        w.width = 10;
        w.height = 10;
        let g0 = w.push_group(0, &[1], 1);
        let g1 = w.push_group(0, &[2], 1);
        let mut ar = armies_with(&mut w, 0, &[g0, g1]);
        let mut a = ar.lists[0][0].clone();
        a.send_here(&mut w, -50, 999_999, 1);
        assert_eq!(a.x, 0);
        assert_eq!(a.y, 10 * 768 - 1);
        assert_eq!(a.muster_x, 0);
        assert_eq!(a.muster_y, div3_shift8(10 * 768 - 1));
        assert_eq!(w.moves.len(), 2);
        assert_eq!(w.moves[0].0, g0);
        assert_eq!(w.moves[1].0, g1);
        // the second group is offset from the first
        assert!(w.moves[0].1 != w.moves[1].1 || w.moves[0].2 != w.moves[1].2);
        ar.lists[0][0] = a;
    }

    #[test]
    fn send_here_leaves_the_angle_stale_on_a_single_axis_move() {
        let mut w = TestWorld::new();
        let mut a = ArmyData::default();
        a.init(&w, 0, 0, -1);
        a.x = 1000;
        a.y = 2000;
        a.muster_angle = 0x1234;
        // y unchanged -> the `je` at 0x006F98FA skips find_angle
        a.send_here(&mut w, 5000, 2000, 1);
        assert_eq!(a.muster_angle, 0x1234);
        // both change -> recomputed
        a.send_here(&mut w, 9000, 9000, 1);
        assert_ne!(a.muster_angle, 0x1234);
    }

    // --- process_all -------------------------------------------------------------------

    #[test]
    fn production_dispatch_trace_counts_enabled_owners_slots_and_heavy_passes() {
        let mut w = TestWorld::new();
        w.leader_flags = [0; NUM_LEADERS];
        w.leader_flags[0] = LF_ACTIVE;
        let mut ar = Armies::new();
        ar.lists[0][0].init(&w, 0, 0, -1);

        let trace = ar.process_all(&mut w);

        assert_eq!(trace.owners_enabled, 1);
        assert_eq!(trace.slots_examined, ARMIES_PER_PLAYER as u32);
        assert_eq!(trace.armies_processed, 1);
        assert_eq!(trace.heavy_passes, 1);
        assert_eq!(trace.gaps.do_mustering, 1);
    }

    #[test]
    fn step13_preallocated_dispatch_executes_without_deeper_host_facts() {
        let mut ar = Armies::new();
        let mut inputs = Step13DispatchInputs::default();
        inputs.leader_flags[0] = LF_ACTIVE;

        let trace = ar.process_step13_dispatch(inputs);

        assert_eq!(trace.process.owners_enabled, 1);
        assert_eq!(trace.process.slots_examined, ARMIES_PER_PLAYER as u32);
        assert_eq!(trace.process.armies_processed, 0);
        assert_eq!(trace.missing_live_host_armies, 0);
    }

    #[test]
    fn step13_live_army_without_complete_host_fails_closed_before_mutation() {
        let mut ar = Armies::new();
        ar.lists[0][0].valid = 1;
        ar.lists[0][0].status = ST_HURRY | ST_MUSTERING;
        let before = ar.lists[0][0].clone();
        let mut inputs = Step13DispatchInputs::default();
        inputs.leader_flags[0] = LF_ACTIVE;

        let trace = ar.process_step13_dispatch(inputs);

        assert_eq!(trace.process.slots_examined, ARMIES_PER_PLAYER as u32);
        assert_eq!(trace.process.armies_processed, 0);
        assert_eq!(trace.missing_live_host_armies, 1);
        assert_eq!(ar.lists[0][0], before);
    }

    #[test]
    fn process_all_skips_a_disabled_owner_three_ways() {
        let mut w = TestWorld::new();
        let mut ar = Armies::new();
        for who in 0..NUM_LEADERS {
            ar.lists[who][0].init(&w, 0, who as i32, -1);
        }
        let mut gaps = ArmyGaps::default();
        assert_eq!(ar.process_all_research_partial(&mut w, &mut gaps), 8);

        w.leader_flags[0] = 0; // LF_ACTIVE clear
        w.leader_flags[1] = LF_ACTIVE | LF_KIND_SKIP; // (flags & 0xC) == 4
        w.leader_flags2[2] = 0x02; // flags2 & 0xA
        let mut gaps = ArmyGaps::default();
        assert_eq!(ar.process_all_research_partial(&mut w, &mut gaps), 5);
    }

    #[test]
    fn process_all_consumes_the_hurry_bit_and_forces_the_heavy_pass() {
        let mut w = TestWorld::new();
        w.frame = 1; // no phase fires at frame 1 for army 0 / owner 0
        let g = w.push_group(0, &[1, 2, 3], 3);
        let mut ar = armies_with(&mut w, 0, &[g]);
        let mut gaps = ArmyGaps::default();
        // un-hurried at a non-phase frame: nothing dispatches
        ar.process_all_research_partial(&mut w, &mut gaps);
        assert_eq!(gaps.total(), 0);

        ar.lists[0][0].status |= ST_HURRY;
        let mut gaps = ArmyGaps::default();
        ar.process_all_research_partial(&mut w, &mut gaps);
        assert_eq!(ar.lists[0][0].status & ST_HURRY, 0);
        assert!(gaps.total() > 0);
    }

    #[test]
    fn the_heavy_pass_fires_every_256_frames_offset_by_army_and_owner() {
        // (frame + (army + who*2)*2) % 256 == 0
        assert_eq!(phase_offset(0, 0), 0);
        assert_eq!(phase_offset(1, 0), 2);
        assert_eq!(phase_offset(0, 3), 12);
        assert_eq!(phase_offset(15, 7), 58);
        let mut hits = 0;
        for f in 0..PHASE_HEAVY {
            if (f + phase_offset(3, 2)) % PHASE_HEAVY == 0 {
                hits += 1;
            }
        }
        assert_eq!(hits, 1);
    }

    #[test]
    fn an_army_with_no_standard_units_retires_to_a_city() {
        let mut w = TestWorld::new();
        w.city_num[0] = 2;
        w.cities.push((0, 0, 0, 100, 100)); // flags bit 0 clear -> skipped
        w.cities.push((0, 1, 1, 4000, 5000)); // this one is taken
        let g = w.push_group(0, &[1], 1);
        // One captain minus one spellcaster is zero standard units, which is the retail
        // retirement predicate after normalize recomputes the derived counters.
        w.groups[g as usize].counts = vec![((COUNT_SPELLCASTER, 0), 1)];
        let mut ar = armies_with(&mut w, 0, &[g]);
        ar.lists[0][0].status = ST_MARCHING;
        let mut gaps = ArmyGaps::default();
        ar.process_one_research_partial(&mut w, 0, 0, true, &mut gaps);
        assert_eq!(ar.lists[0][0].valid, 0);
        assert_eq!(ar.lists[0][0].city, 1);
        assert_eq!(w.moves.len(), 1);
        assert_eq!(w.moves[0].4, 1); // order 1
    }

    #[test]
    fn a_mustering_army_with_no_standard_units_does_not_retire() {
        let mut w = TestWorld::new();
        let g = w.push_group(0, &[1], 1);
        let mut ar = armies_with(&mut w, 0, &[g]);
        ar.lists[0][0].status = ST_MUSTERING;
        let mut gaps = ArmyGaps::default();
        ar.process_one_research_partial(&mut w, 0, 0, true, &mut gaps);
        assert_eq!(ar.lists[0][0].valid, 1);
        assert_eq!(gaps.do_mustering, 1);
    }

    #[test]
    fn a_human_order_short_circuits_the_state_machine() {
        let mut w = TestWorld::new();
        let g = w.push_group(0, &[1], 1);
        w.groups[g as usize].counts = vec![((COUNT_TYPE, TYPE_ARG_SIEGE), -5)];
        let mut ar = armies_with(&mut w, 0, &[g]);
        ar.lists[0][0].status = ST_MARCHING;
        ar.lists[0][0].human_frame = 5;
        let mut gaps = ArmyGaps::default();
        ar.process_one_research_partial(&mut w, 0, 0, true, &mut gaps);
        // decremented at the top, then used
        assert_eq!(ar.lists[0][0].human_frame, 4);
        assert_eq!(gaps.do_marching, 0);
        assert_eq!(w.moves.len(), 1);
        assert_eq!(w.moves[0].4, 2); // order 2
    }

    #[test]
    fn a_dead_end_status_resets_to_marching() {
        let mut w = TestWorld::new();
        let g = w.push_group(0, &[1], 1);
        w.groups[g as usize].counts = vec![((COUNT_TYPE, TYPE_ARG_SIEGE), -5)];
        let mut ar = armies_with(&mut w, 0, &[g]);
        // status 0x10 is a subset of 0x18 -> reset to MARCHING
        ar.lists[0][0].status = ST_FORMING;
        let mut gaps = ArmyGaps::default();
        ar.process_one_research_partial(&mut w, 0, 0, true, &mut gaps);
        assert_eq!(ar.lists[0][0].status, ST_MARCHING);
        assert_eq!(gaps.do_marching, 1);
        assert_eq!(gaps.do_forming, 0);
    }

    #[test]
    fn retarget_pulls_the_muster_point_halfway_to_an_enemy_object() {
        let mut w = TestWorld::new();
        w.enemies.push((0, 3));
        let g = w.push_group(0, &[1], 1);
        w.groups[g as usize].counts = vec![((COUNT_TYPE, TYPE_ARG_SIEGE), -5)];
        // the target object belongs to owner 3
        w.objects.push(TestObj {
            who: 3,
            o: 42,
            alive: true,
            on_map: true,
            x: 30_000,
            y: 60_000,
            ..Default::default()
        });
        let mut ar = armies_with(&mut w, 0, &[g]);
        ar.lists[0][0].status = 0;
        ar.lists[0][0].target_o = 42;
        ar.lists[0][0].target_who = 3;
        ar.lists[0][0].muster_x = 0;
        ar.lists[0][0].muster_y = 0;
        let mut gaps = ArmyGaps::default();
        ar.process_one_research_partial(&mut w, 0, 0, true, &mut gaps);
        assert_eq!(ar.lists[0][0].muster_x, (div3_shift8(30_000) + 0) / 2);
        assert_eq!(ar.lists[0][0].muster_y, (div3_shift8(60_000) + 0) / 2);
        assert_eq!(ar.lists[0][0].status & ST_RETARGET_MASK, ST_RETARGET_MASK);
        assert_eq!(gaps.find_muster_spot, 0);
    }

    #[test]
    fn retarget_falls_to_find_muster_spot_for_a_non_enemy_target() {
        let mut w = TestWorld::new();
        let g = w.push_group(0, &[1], 1);
        w.groups[g as usize].counts = vec![((COUNT_TYPE, TYPE_ARG_SIEGE), -5)];
        let mut ar = armies_with(&mut w, 0, &[g]);
        ar.lists[0][0].status = 0;
        ar.lists[0][0].target_o = 42;
        ar.lists[0][0].target_who = 3; // not registered as an enemy
        let mut gaps = ArmyGaps::default();
        ar.process_one_research_partial(&mut w, 0, 0, true, &mut gaps);
        assert_eq!(gaps.find_muster_spot, 1);
        assert_eq!(ar.lists[0][0].status & ST_RETARGET_MASK, ST_RETARGET_MASK);
    }

    #[test]
    fn a_shrunken_army_merges_into_a_healthy_sibling_and_closes() {
        let mut w = TestWorld::new();
        let gs = w.push_group(0, &[1, 2], 2);
        let gd = w.push_group(0, &[3, 4, 5, 6, 7, 8], 6);
        let mut ar = Armies::new();
        ar.lists[0][0].init(&w, 0, 0, -1);
        ar.lists[0][0].add_group(&mut w, gs);
        ar.lists[0][1].init(&w, 1, 0, -1);
        ar.lists[0][1].add_group(&mut w, gd);
        // both share reg == -1 from the city-less init
        ar.lists[0][0].num_standard = 0;
        ar.lists[0][0].num_captains = 10;
        ar.lists[0][0].num_decoys = 0;
        ar.lists[0][1].num_standard = 6;
        ar.lists[0][1].num_captains = 6;
        ar.lists[0][1].num_decoys = 0;
        assert!(ar.try_merge(&mut w, 0, 0));
        assert_eq!(ar.lists[0][0].valid, 0);
        // the source's members landed in the destination's group 0, in reverse order
        assert_eq!(w.g(gd).list, vec![3, 4, 5, 6, 7, 8, 2, 1]);
    }

    #[test]
    fn the_merge_scan_cannot_pick_the_army_itself() {
        let mut w = TestWorld::new();
        let g = w.push_group(0, &[1], 1);
        let mut ar = armies_with(&mut w, 0, &[g]);
        ar.lists[0][0].num_standard = 0;
        ar.lists[0][0].num_captains = 10;
        assert!(!ar.try_merge(&mut w, 0, 0));
        assert_eq!(ar.lists[0][0].valid, 1);
    }

    // --- container queries -------------------------------------------------------------

    #[test]
    fn num_armies_and_find_city_read_the_status_mask() {
        let mut w = TestWorld::new();
        let mut ar = Armies::new();
        w.cities.push((0, 4, 1, 0, 0));
        ar.lists[0][0].init(&w, 0, 0, 4);
        ar.lists[0][1].init(&w, 1, 0, 4);
        ar.lists[0][1].status = ST_MARCHING;
        assert_eq!(ar.num_armies(0, ST_MUSTERING, -1), 1);
        assert_eq!(ar.num_armies(0, ST_MUSTERING | ST_MARCHING, -1), 2);
        assert_eq!(ar.find_city(0, 4), 0);
        assert_eq!(ar.find_city(0, 9), -1);
    }

    #[test]
    fn find_aggressive_army_wants_two_captains_outside_our_borders() {
        let mut w = TestWorld::new();
        let mut ar = Armies::new();
        ar.lists[0][0].init(&w, 0, 0, -1);
        ar.lists[0][0].num_captains = 2;
        ar.lists[0][0].status = ST_MARCHING;
        w.owner_tile = 0; // standing at home
        assert_eq!(ar.find_aggressive_army(&w, 0), -1);
        w.owner_tile = 5; // standing on someone else's ground
        assert_eq!(ar.find_aggressive_army(&w, 0), 0);
        ar.lists[0][0].status |= ST_MUSTERING;
        assert_eq!(ar.find_aggressive_army(&w, 0), -1);
    }

    #[test]
    fn find_useful_army_scores_distance_per_captain_and_triples_cross_region() {
        let w = TestWorld::new();
        let mut ar = Armies::new();
        ar.lists[0][0].init(&w, 0, 0, -1);
        ar.lists[0][0].num_captains = 1;
        ar.lists[0][0].reg = 1;
        ar.lists[0][0].x = 1000;
        ar.lists[0][0].y = 0;
        ar.lists[0][1].init(&w, 1, 0, -1);
        ar.lists[0][1].num_captains = 4;
        ar.lists[0][1].reg = 1;
        ar.lists[0][1].x = 2000;
        ar.lists[0][1].y = 0;
        // the farther army wins on captains
        assert_eq!(ar.find_useful_army(&w, 0, 0, 0, 1), 1);
        // put army 1 in another region: its cost triples to 6000/4 = 1500 > 1000
        ar.lists[0][1].reg = 2;
        assert_eq!(ar.find_useful_army(&w, 0, 0, 0, 1), 0);
    }

    #[test]
    fn update_city_retargets_and_hurries_only_for_owner_zero() {
        let w = TestWorld::new();
        let mut ar = Armies::new();
        ar.lists[0][0].init(&w, 0, 0, -1);
        ar.lists[0][0].target_o = 7;
        ar.lists[0][0].target_who = 3;
        ar.update_city(&w, 7, 3, 8, 2);
        assert_eq!(ar.lists[0][0].target_o, 8);
        assert_eq!(ar.lists[0][0].target_who, 2);
        assert_eq!(ar.lists[0][0].status & ST_HURRY, 0);
        ar.update_city(&w, 8, 2, 9, 0);
        assert_eq!(ar.lists[0][0].status & ST_HURRY, ST_HURRY);
    }

    #[test]
    fn emergency_clears_the_target_and_forces_a_pass() {
        let mut w = TestWorld::new();
        let g = w.push_group(0, &[1], 1);
        w.groups[g as usize].counts = vec![((COUNT_TYPE, TYPE_ARG_SIEGE), -5)];
        let mut ar = armies_with(&mut w, 0, &[g]);
        ar.lists[0][0].target_o = 3;
        ar.lists[0][0].target_who = 4;
        ar.lists[0][0].status = ST_MARCHING;
        let mut gaps = ArmyGaps::default();
        ar.emergency_research_partial(&mut w, 0, &mut gaps);
        assert_eq!(ar.lists[0][0].target_o, -1);
        assert_eq!(ar.lists[0][0].target_who, -1);
        assert_eq!(gaps.do_marching, 1);
    }

    // --- find_target -------------------------------------------------------------------

    #[test]
    fn find_target_prologue_respects_the_leader_gate() {
        let mut w = TestWorld::new();
        let g = w.push_group(0, &[1], 1);
        let ar = armies_with(&mut w, 0, &[g]);
        w.leader_flags[0] |= LF_NO_TARGETING;
        assert_eq!(ar.lists[0][0].find_target_prologue(&w, 5, true), None);
    }

    #[test]
    fn find_target_prologue_marks_a_navy_strong_without_counting() {
        let mut w = TestWorld::new();
        let g = w.push_group(0, &[1], 1);
        let mut ar = armies_with(&mut w, 0, &[g]);
        ar.lists[0][0].navy = 1;
        assert_eq!(ar.lists[0][0].find_target_prologue(&w, 9, true), Some(true));
    }

    #[test]
    fn find_target_prologue_needs_four_weighted_units() {
        let mut w = TestWorld::new();
        let g = w.push_group(0, &[1], 1);
        w.groups[g as usize].counts = vec![((COUNT_MOBILE, 0), 1)];
        let ar = armies_with(&mut w, 0, &[g]);
        // 1*2 + 0 + 0 = 2 < 4
        assert_eq!(
            ar.lists[0][0].find_target_prologue(&w, 9, true),
            Some(false)
        );
    }

    #[test]
    fn find_target_prologue_epoch_gate_uses_its_own_xor_key() {
        let mut w = TestWorld::new();
        let g = w.push_group(0, &[1], 1);
        w.groups[g as usize].counts = vec![((COUNT_MOBILE, 0), 2)];
        let ar = armies_with(&mut w, 0, &[g]);
        // 2*2 == 4 -> pass the strength gate; epoch < 3 short-circuits to `strong`
        assert_eq!(ar.lists[0][0].find_target_prologue(&w, 2, true), Some(true));
        // epoch >= 3 with siege available and no siege units in the army
        assert_eq!(
            ar.lists[0][0].find_target_prologue(&w, 3, true),
            Some(false)
        );
        // epoch >= 3 but the siege type is not researched
        assert_eq!(
            ar.lists[0][0].find_target_prologue(&w, 3, false),
            Some(true)
        );
    }

    #[test]
    fn the_three_find_target_draws_are_transcribed() {
        // Not a fidelity claim about retail's stream position — only that the arithmetic
        // wrapped around each draw is the arithmetic at the three call sites.
        let mut rng = Random::new(0x5EED);
        let mut probe = rng;
        let raw = probe.get(0, 0xFFFF);
        assert_eq!(
            find_target_coin(&mut rng),
            (raw & 0x8000_0001u32 as i32) != 0
        );

        let mut rng = Random::new(0x5EED);
        let mut probe = rng;
        let raw = probe.get(0, 0xFFFF);
        assert_eq!(find_target_jitter(&mut rng), 0x384 + raw % 200);
        assert!((0x384..0x384 + 200).contains(&(0x384 + raw % 200)));

        assert_eq!(FindTargetDraw::AggressiveCoin.va(), 0x006F_6DBB);
        assert_eq!(FindTargetDraw::TargetJitterA.va(), 0x006F_718A);
        assert_eq!(FindTargetDraw::TargetJitterB.va(), 0x006F_801A);
    }

    #[test]
    fn every_marching_army_records_an_unresolved_rng_stream_hazard() {
        // `do_marching` can reach `find_target`, but the body and its data-dependent draw
        // count are absent. Record the hazard without inventing a draw count.
        let mut w = TestWorld::new();
        let g = w.push_group(0, &[1], 1);
        w.groups[g as usize].counts = vec![((COUNT_TYPE, TYPE_ARG_SIEGE), -5)];
        let mut ar = armies_with(&mut w, 0, &[g]);
        ar.lists[0][0].status = ST_MARCHING;
        let mut gaps = ArmyGaps::default();
        ar.process_one_research_partial(&mut w, 0, 0, true, &mut gaps);
        assert_eq!(gaps.do_marching, 1);
        assert_eq!(gaps.game_random_stream_unresolved, 1);
    }

    // --- determinism -------------------------------------------------------------------

    #[test]
    fn process_all_is_deterministic_over_many_frames() {
        fn run() -> (Vec<i32>, u64, u32) {
            let mut w = TestWorld::new();
            let mut ar = Armies::new();
            let mut gaps = ArmyGaps::default();
            let mut total = 0u32;
            for who in 0..4 {
                for k in 0..3 {
                    let g = w.push_group(who as i32, &[(who * 10 + k) as i32], 2);
                    w.groups[g as usize].counts = vec![((COUNT_TYPE, TYPE_ARG_SIEGE), -3)];
                    let slot = ar.init_army(&w, who, -1);
                    let mut a = ar.lists[who][slot as usize].clone();
                    a.add_group(&mut w, g);
                    a.status = ST_MARCHING;
                    ar.lists[who][slot as usize] = a;
                }
            }
            for f in 0..600 {
                w.frame = f;
                total += ar.process_all_research_partial(&mut w, &mut gaps);
            }
            let statuses = ar
                .lists
                .iter()
                .flat_map(|l| l.iter().map(|a| a.status))
                .collect();
            (statuses, gaps.total(), total)
        }
        assert_eq!(run(), run());
    }

    #[test]
    fn the_heavy_pass_rate_matches_the_phase_arithmetic() {
        let mut w = TestWorld::new();
        let g = w.push_group(0, &[1], 1);
        w.groups[g as usize].counts = vec![((COUNT_TYPE, TYPE_ARG_SIEGE), -5)];
        let mut ar = armies_with(&mut w, 0, &[g]);
        ar.lists[0][0].status = ST_MARCHING;
        let mut heavy = 0;
        let mut gaps = ArmyGaps::default();
        for f in 0..1024 {
            w.frame = f;
            let before = gaps.do_marching;
            ar.process_one_research_partial(&mut w, 0, 0, false, &mut gaps);
            if gaps.do_marching != before {
                heavy += 1;
            }
        }
        // 1024 frames / 256 = 4 heavy passes for army 0 of owner 0.
        assert_eq!(heavy, 4);
    }
}
