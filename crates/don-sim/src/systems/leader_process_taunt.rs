//! `Leader::process_taunt` `0x006B8CC0` — step 8's last unported child.
//!
//! ```text
//! Game::do_frame                          0x00591EF0
//!  +- [8] Leaders::process_all            0x006ED2A0   <- systems/leaders.rs
//!       +- the taunt-table scan           0x006ED3CE
//!            +- Leader::process_taunt     0x006B8CC0   <- THIS FILE (2,340 B)
//!                 +- LeaderData::type_avail        0x006E33A0  (host answer)
//!                 +- Game::teams_locked            0x00594880  ported
//!                 +- LeaderData::is_neutral        0x006EBAE0  (host answer)
//!                 +- Leader::action_clear_all      0x006D15E0  ported
//!                 |    +- IFaceDiploNeg::check_click_stamp 0x008035E0  product
//!                 |    +- Leader::clear_agree      0x006D1AF0  ported
//!                 |    |    +- LeaderData::bucket_add 0x0043ED10 ported
//!                 |    |    +- Diplomacy::clear_dows  0x0047E000 ported
//!                 |    +- LeaderData::any_proposals 0x006D5AD0  product gate
//!                 |    +- Diplomacy::clear_all      0x0047E030  ported
//!                 +- Leader::action_offer          0x006D1780  ported
//!                 +- Leader::action_respond        0x006D03C0  UNPORTED (3,988 B)
//!                 +- Leader::chat_to_local         0x006EC520  product
//!                 +- Taunts::id_to_index/play      0x00980DC0/0x00980CF0  product
//!                 +- SoundGlobal::play             0x0097F770  product
//!                 +- Random::get on internal_random 0x00A39D70 product
//! ```
//!
//! **Tier C.** Structure, constants, branch order, receivers and rounding read from
//! `ron-bin/riseofnations.exe` (sha256 `30478a44…625079`) with radare2, cross-read against
//! `re/decomp-all/006b8cc0.c`, `ron-bin/sbl/rise.pdb` (`re/symtab.json`,
//! `schema/pdb-types.json`, `schema/symbols.json`) and `schema/rise-symbols.tsv`. **Nothing
//! here has been executed against retail and no oracle case was added.** The tests that
//! drive `Sim::do_frame` are integration tests of this port, not evidence about the game.
//!
//! # "The AI-chat body" was wrong twice over
//!
//! `crates/don-sim/src/tick.rs`'s gap note calls this "the AI-chat body". It is not, and the
//! sibling correction that called it "a resource transfer" is also not exactly right. What
//! the 2,340 bytes actually do, in three disjoint groups:
//!
//! * **`TAUNT_FOOD`..`TAUNT_OIL` (1..5) stage a diplomatic tribute.** Above a stockpile of
//!   `0x96`, retail calls `Leader::action_clear_all` / `Leader::action_offer` /
//!   `Leader::action_respond`. `action_offer` does **not** move resources: it writes
//!   `LeaderData::dip[who].offers[res] += amount/3` on the asked leader and the exact
//!   negation into `leaders[who].dip[me].offers[res]` — a two-sided ledger. The stockpile
//!   only moves inside `Leader::action_respond` `0x006D03C0`, which is 3,988 bytes and is
//!   this port's one named boundary ([`TauntUnresolved::ActionRespond`]).
//! * **`TAUNT_BUILD_WONDER`..`TAUNT_HELP` (7..16) rewrite the AI build-priority scalars**
//!   `LeaderData::wonder_mod`/`ground_mod`/`air_mod`/`sea_mod`/`infra_mod`/`defense_mod`
//!   (`+0x794`..`+0x7A8`) and `Personality::raid` (`+0x6DEC`), then clamp five of them.
//!   That is plain per-leader simulation state, written on **every** dispatch regardless of
//!   who is watching. No paraphrase of this function as presentation survives contact with
//!   `0x006B94A4`.
//! * **`TAUNT_NEED` (6) is presentation only** — it returns before any store unless
//!   `who == Console::who`.
//!
//! # Five things a paraphrase loses
//!
//! Each is `[measured]`; the decompilation in `re/decomp-all/006b8cc0.c` gets the first four
//! wrong or drops them.
//!
//! 1. **The two `LeaderData::type_avail` calls have different receivers.** `0x006B8D89`
//!    passes `ECX = leaders[this->who]`; `0x006B8DA7` passes `ECX = leaders[who]`. Ghidra
//!    prints `FUN_006e33a0(iVar7,1)` twice. So a tribute needs the resource enabled for
//!    **both** leaders, which is a different predicate from calling one query twice.
//! 2. **`Leader::action_clear_all`'s second `clear_agree` runs on the other leader.**
//!    `0x006D1610` loads `ECX = who * 0x6EEC + 0x00E3A390`. Same for `action_offer`'s at
//!    `0x006D1811`. Both refund out of `leaders[who]`'s tributes, not `this`'s.
//! 3. **The stockpile is re-read after `action_clear_all`.** `0x006B8E44` reads it for the
//!    threshold and `0x006B8EEA` reads it *again* for the `/3`. `action_clear_all` reaches
//!    `LeaderData::bucket_add` `0x0043ED10`, which writes the stockpile, so the two reads
//!    can differ and the offered amount is the second one.
//! 4. **The `/3` is a signed truncating divide** (`0x55555556` `imul`, `shr 31`, `add`), and
//!    the threshold is a **signed** `cmp eax, 0x96 ; jge` — a negative stockpile takes the
//!    "not enough" branch rather than wrapping.
//! 5. **The presentation die is `internal_random` `0x00EB697C`, not `game_random`
//!    `0x00C06184`.** `0x006B929E` loads `ECX = 0xEB697C`. That matters because the draw is
//!    gated on `who == Console::who`: had it been the simulation stream, every taunt aimed
//!    at the local player would desync a lockstep match. It is not, so it cannot.
//!
//! # What this file deliberately does not write
//!
//! * **`Leader::action_respond` `0x006D03C0`** (3,988 B). It is the actual stockpile write
//!   (`0x006D0...`: `enc->bucket[r] = (plain - taken) ^ 0x8221`), plus a per-resource
//!   affordability pass over every ally and a second `LeaderData::type_avail` sweep. It gets
//!   its own lane; reaching it here records [`TauntUnresolved::ActionRespond`] and leaves
//!   every byte of leader state alone.
//! * **`LeaderData::type_avail` `0x006E33A0`** (1,091 B) and **`LeaderData::is_neutral`
//!   `0x006EBAE0`** (which needs the `GameInfo::player[8]` table at `Game + 0x44`). Both are
//!   typed host answers on [`TauntEnv`], absent by default, and an absent answer refuses the
//!   dispatch instead of guessing one.
//! * **The initial values of the six `*_mod` scalars and `Personality::raid`.** They are
//!   `Leader::init`'s, not this function's. A default [`TauntLeaderState`] leaves them zero,
//!   so the first `TAUNT_BUILD_*` clamps them to their floors — which is exactly what retail
//!   does from a zeroed leader, and is not a claim about a real match.
//! * **The `loc_str_array_orig` text.** Ordinals are recorded; `0x00C8CD00` is
//!   `loc_str_array_orig + 0x10` (a `StringTable` element pointer, 20-byte `String`
//!   records), so byte offset `/ 20` is the ordinal. This is **not** the
//!   `internal_strings.xml` array at `[[0x00C06378] + 0x10]`; do not decode it against that.

#![allow(clippy::needless_range_loop)]

use crate::systems::economy::{NUM_RESOURCES, RES_KNOWLEDGE};
use crate::systems::leaders::{Leader, Leaders, NUM_LEADER_SLOTS};

// ===========================================================================================
// Retail addresses, offsets and constants
// ===========================================================================================

/// Byte offsets inside `LeaderData`, each from the instruction that touches it and each
/// named by `schema/pdb-types.json`'s `LeaderData` field list.
pub mod offsets {
    /// `0x006B8DDB` — `gift_stamp[8]`. The frame of the last granted tribute, per target.
    pub const GIFT_STAMP: usize = 0x194;
    /// `0x006B8D46` — `last_taunt[8]`, an `enum TauntRequest`.
    pub const LAST_TAUNT: usize = 0x354;
    /// `0x006B8D54` — `taunt_frame[8]`. Distinct from `incoming_taunt_frame` at `+0x3D4`,
    /// which is the dispatcher's own table.
    pub const LAST_TAUNT_FRAME: usize = 0x374;
    /// `0x006D1B10` — `tributes[6]`, the escrow `Leader::clear_agree` refunds out of.
    pub const TRIBUTES: usize = 0x498;
    /// `0x006B8F2E` — `wonder_mod`.
    pub const WONDER_MOD: usize = 0x794;
    /// `0x006B8F44` — `ground_mod`.
    pub const GROUND_MOD: usize = 0x798;
    /// `0x006B8F68` — `air_mod`.
    pub const AIR_MOD: usize = 0x79C;
    /// `0x006B8F53` — `sea_mod`.
    pub const SEA_MOD: usize = 0x7A0;
    /// `0x006B8F7D` — `infra_mod`.
    pub const INFRA_MOD: usize = 0x7A4;
    /// `0x006B90DD` — `defense_mod`.
    pub const DEFENSE_MOD: usize = 0x7A8;
    /// `0x006D1AF9` — `dip[8]`, `struct Diplomacy` at stride `0x5C`.
    pub const DIP: usize = 0x692C;
    /// `0x006B907F` — `pers` is at `+0x6DD4` and `raid` is `Personality +0x18`.
    pub const PERSONALITY_RAID: usize = 0x6DEC;
}

/// `enum TauntRequest`, verbatim from the PDB type stream. The dispatch is
/// `dec ecx ; cmp ecx, 0xF ; ja default ; jmp [ecx*4 + 0x006B9560]` at `0x006B8D5B`, so
/// `TAUNT_NONE` and anything above `TAUNT_HELP` take the default arm.
pub mod taunt {
    pub const NONE: i32 = 0;
    pub const FOOD: i32 = 1;
    pub const TIMBER: i32 = 2;
    pub const WEALTH: i32 = 3;
    pub const METAL: i32 = 4;
    pub const OIL: i32 = 5;
    pub const NEED: i32 = 6;
    pub const BUILD_WONDER: i32 = 7;
    pub const BUILD_GROUND: i32 = 8;
    pub const BUILD_SEA: i32 = 9;
    pub const BUILD_AIR: i32 = 10;
    pub const BUILD_INFRA: i32 = 11;
    pub const RUSH: i32 = 12;
    pub const BOOM: i32 = 13;
    pub const ATTACK: i32 = 14;
    pub const DEFEND: i32 = 15;
    pub const HELP: i32 = 16;
    pub const COUNT: i32 = 17;
}

/// The resource each of `TAUNT_FOOD..TAUNT_OIL` asks for, from the five jump-table arms at
/// `0x006B8D6B`/`0x006B8D6F`/`0x006B8D76`/`0x006B8D7D`/`0x006B8D84`. Note the skip: the
/// wire codes are contiguous, the resource slots are not — `TAUNT_METAL` is slot **4**,
/// because slot 3 is knowledge and nothing can ask for it.
pub const TAUNT_RESOURCE: [usize; 5] = [0, 1, 2, 4, 5];

/// `0x006B8E52` — `cmp eax, 0x96 ; jge`. Signed, so this is `stockpile >= 150`.
pub const TRIBUTE_THRESHOLD: i32 = 0x96;

/// `0x006B8DF3` — `cmp eax, 0x1194 ; jge`. 4,500 frames, five game minutes at 15 fps.
pub const GIFT_COOLDOWN_FRAMES: i32 = 0x1194;

/// `0x006ED2F1` — the `diplo` value that means allied, shared with the step-8 hostile scan.
pub const DIPLO_ALLIED: i32 = 2;

/// `LeaderData::leader_flags & 4`, `0x006B8CEF`. A **human** leader never processes a taunt.
pub const FLAG_HUMAN: u32 = 0x0000_0004;
/// `LeaderData::leader_flags & 2`, `0x006B8CF8`. Same bit `Leaders::process_all` gates on.
pub const FLAG_ACTIVE: u32 = 0x0000_0002;

/// `0x006B9209` / `0x006B946E` — `player_profile + 0x34 & 0x1000`, the taunt-audio option.
pub const PROFILE_TAUNT_AUDIO: u32 = 0x1000;

/// `0x006B929A` — `Random::get(0, 0xB)` on `internal_random`, exclusive of `0xB` as far as
/// the following `cmp eax, 0xA ; ja` can observe.
pub const FLAVOUR_ROLL_RANGE: (i32, i32) = (0, 0xB);

/// `loc_str_array_orig` ordinals, as byte offset from `[0x00C8CD00]` divided by 20.
pub mod loc {
    /// `+0xC774`, `0x006B8E04` — "I gave you some recently".
    pub const TRIBUTE_TOO_SOON: u32 = 0xC774 / 20;
    /// `+0xC788`, `0x006B8E63` — "I do not have enough %1".
    pub const TRIBUTE_NOT_ENOUGH: u32 = 0xC788 / 20;
    /// `+0xB284`, `0x006B91DF` — the `TAUNT_NEED` line.
    pub const NEED_RESOURCE: u32 = 0xB284 / 20;
    /// `+0xB504`, `0x006D163E` — `action_clear_all`'s "proposals withdrawn" line.
    pub const PROPOSALS_CLEARED: u32 = 0xB504 / 20;
    /// `+0xC79C` .. `+0xC864` in steps of 20, `0x006B92C2` .. `0x006B9443`.
    pub const FLAVOUR_FIRST: u32 = 0xC79C / 20;
}

/// `Taunts` ids the two presentation paths play. `TAUNT_NEED` maps the chosen resource
/// through the jump table at `0x006B95A0`; the flavour roll uses `0x50 + roll` with the
/// `roll == 8` arm playing nothing.
pub mod taunt_sound {
    /// `0x006B9197`..`0x006B91B8`, indexed by resource slot. Slot 3 is unreachable — the
    /// scan that picks the slot skips knowledge — and its arm is the shared `xor edi, edi`.
    pub const NEED_BY_RESOURCE: [i32; super::NUM_RESOURCES] = [4, 5, 7, 0, 6, 8];
    /// `0x006B92CD` — the flavour ids run `0x50..=0x59` with a hole where `roll == 8`
    /// substitutes the leader's name instead of playing anything.
    pub const FLAVOUR_FIRST: i32 = 0x50;
}

// ===========================================================================================
// Retail structs this body writes
// ===========================================================================================

/// `struct Diplomacy`, 92 bytes, `LeaderData +0x692C + who * 0x5C`. Layout verbatim from the
/// PDB; `Diplomacy::attacks` is carried because the struct is one save/checksum object, not
/// because this function reads it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Diplomacy {
    /// `+0x00`. `Leader::clear_agree` only refunds when this is exactly `1`.
    pub agree: i32,
    /// `+0x04`.
    pub any_offer: i32,
    /// `+0x08`. `Diplomacy::clear_all` sets this to `-1`, not to zero.
    pub treaty: i32,
    /// `+0x0C`, 6 dwords.
    pub offers: [i32; NUM_RESOURCES],
    /// `+0x24`, 6 dwords.
    pub dows: [i32; NUM_RESOURCES],
    /// `+0x3C`, 8 dwords.
    pub attacks: [i32; NUM_LEADER_SLOTS],
}

impl Diplomacy {
    /// `Diplomacy::clear_dows` `0x0047E000` — six stores, nothing else.
    pub fn clear_dows(&mut self) {
        self.dows = [0; NUM_RESOURCES];
    }

    /// `Diplomacy::clear_all` `0x0047E030`. `treaty = -1` is the first store and is the one
    /// thing a `= Default::default()` would get wrong.
    pub fn clear_all(&mut self) {
        self.agree = 0;
        self.any_offer = 0;
        self.treaty = -1;
        self.offers = [0; NUM_RESOURCES];
        self.dows = [0; NUM_RESOURCES];
        self.attacks = [0; NUM_LEADER_SLOTS];
    }

    /// `LeaderData::any_proposals` `0x006D5AD0`, as `action_clear_all`'s product gate reads
    /// it. Not re-derived from the disassembly of that function: this is the local
    /// projection the caller needs, and the presentation receipt records the request so a
    /// host that owns the real query can answer it.
    pub fn has_any_proposal(&self) -> bool {
        self.agree != 0
            || self.any_offer != 0
            || self.offers.iter().any(|v| *v != 0)
            || self.dows.iter().any(|v| *v != 0)
    }
}

/// `LeaderData +0x794 .. +0x7A8` — the six AI build-priority scalars, in address order.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct BuildMods {
    /// `+0x794`. Written only by `TAUNT_BUILD_WONDER`, and never clamped.
    pub wonder: i32,
    /// `+0x798`.
    pub ground: i32,
    /// `+0x79C`.
    pub air: i32,
    /// `+0x7A0`.
    pub sea: i32,
    /// `+0x7A4`.
    pub infra: i32,
    /// `+0x7A8`.
    pub defense: i32,
}

/// The slice of `LeaderData` that `Leader::process_taunt` and its ported callees touch.
///
/// One aggregate rather than seven loose fields on [`Leader`], following this crate's
/// `unit_stats` / `build_stats` / `event_frame` idiom.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct TauntLeaderState {
    /// `+0x194 gift_stamp[8]`.
    pub gift_stamp: [i32; NUM_LEADER_SLOTS],
    /// `+0x354 last_taunt[8]`, an `enum TauntRequest`.
    pub last_taunt: [i32; NUM_LEADER_SLOTS],
    /// `+0x374 taunt_frame[8]`.
    pub last_taunt_frame: [i32; NUM_LEADER_SLOTS],
    /// `+0x498 tributes[6]`.
    pub tributes: [i32; NUM_RESOURCES],
    /// `+0x794..+0x7A8`.
    pub mods: BuildMods,
    /// `+0x6DEC`, i.e. `pers.raid`.
    pub personality_raid: i32,
    /// `+0x692C dip[8]`.
    pub dip: [Diplomacy; NUM_LEADER_SLOTS],
}

// ===========================================================================================
// Host answers
// ===========================================================================================

/// The external queries `Leader::process_taunt` makes, none of which live inside step 8.
///
/// Every one defaults to "not supplied", and a dispatch that needs an unsupplied answer is
/// refused and counted rather than guessed. `flavour_rolls` is the only mutable field; it is
/// drained one entry per reached flavour selection, matching retail's one draw per reached
/// dispatch.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct TauntEnv {
    /// `Console + 0x298`. `None` means the host has not supplied a local display player, so
    /// nothing that compares against it can be evaluated.
    pub local_who: Option<i32>,
    /// `GameInfo::team_style`, `Game + 0x24`, the byte `Game::teams_locked` `0x00594880`
    /// reads. `None` refuses any dispatch that reaches the cooldown predicate.
    pub team_style: Option<i8>,
    /// `LeaderData::is_neutral` `0x006EBAE0`, per **array position**. Its real body is
    /// `team_style == 7 && (players[..].flags & 1) && players[..].team == 8` over
    /// `GameInfo::player[8]` at `Game + 0x44`, stride `0x8C`; that table has no owner in
    /// step 8, so the answer is supplied rather than recomputed.
    pub is_neutral: [Option<bool>; NUM_LEADER_SLOTS],
    /// `LeaderData::type_avail(res, 1)` `0x006E33A0`, per array position per resource.
    /// Stored as the raw `int` because the two call sites test it differently: the tribute
    /// arms want `== 4`, `TAUNT_NEED` wants `!= 0`.
    pub type_avail: [[Option<i32>; NUM_RESOURCES]; NUM_LEADER_SLOTS],
    /// `player_profile + 0x34 & 0x1000`.
    pub taunt_audio: bool,
    /// Host-supplied `internal_random` `0x00EB697C` draws for `Random::get(0, 0xB)` at
    /// `0x006B92A3`, consumed front to back. **This is not `game_random`**, so supplying or
    /// withholding them cannot move simulation state; an empty queue records
    /// [`TauntUnresolved::FlavourRoll`] and emits no line.
    pub flavour_rolls: std::collections::VecDeque<i32>,
}

// ===========================================================================================
// Outcomes
// ===========================================================================================

/// Why a dispatch produced no further work. Every variant is a retail `return` this port
/// reached, not an approximation.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TauntStop {
    /// `0x006B8CF2` — `leader_flags & 4`.
    LeaderIsHuman,
    /// `0x006B8CFB` — `!(leader_flags & 2)`.
    LeaderNotActive,
    /// `0x006B8D16` — `leaders[me].diplo[who] != 2`.
    NotAllied,
    /// `0x006B8D2E` — `leaders[who].diplo[me] != 2`.
    NotMutuallyAllied,
    /// `0x006B8D36` — `this->who == who`.
    Self_,
    /// `0x006B8D5E` — the jump table's `ja`, i.e. `kind` outside `1..=16`.
    UnknownTauntCode,
    /// `0x006B8DA1` / `0x006B8DBE` — a `type_avail` answer that is not `4`.
    ResourceNotAvailable,
    /// `0x006B8DF8` — inside `GIFT_COOLDOWN_FRAMES` of the last granted tribute.
    GiftCooldown,
    /// `0x006B8E57` — stockpile below [`TRIBUTE_THRESHOLD`].
    NotEnoughStockpile,
    /// `0x006B9185` — `TAUNT_NEED` found no candidate resource.
    NoNeededResource,
    /// `0x006B91C7` / `0x006B91CF` — `TAUNT_NEED`'s local-player and sound-id gates.
    NeedIsRemote,
}

/// A named retail call or input this port could not execute. This is the honest
/// `Gap::LeaderProcessTaunt` charge; presentation shortfalls are counted separately.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TauntUnresolved {
    /// `Leader::process_taunt` indexes `leaders[]` by `LeaderData::who` and by its second
    /// argument. Retail reads out of bounds when either is outside `0..8`; this port
    /// refuses instead.
    SlotOutOfRange,
    /// `LeaderData::type_avail` `0x006E33A0` — no host answer for this leader/resource.
    TypeAvail,
    /// `Game::teams_locked` `0x00594880` — `GameInfo::team_style` not supplied.
    TeamsLocked,
    /// `LeaderData::is_neutral` `0x006EBAE0` — no host answer.
    IsNeutral,
    /// **`Leader::action_respond` `0x006D03C0`.** The 3,988-byte body that actually moves
    /// the stockpile. Reaching this leaves every byte of leader state alone.
    ActionRespond,
    /// `Random::get(0, 0xB)` on `internal_random` `0x00EB697C` — presentation only; this
    /// variant is never counted into [`TauntPassCounts::unresolved_calls`].
    FlavourRoll,
    /// `Console + 0x298` not supplied, so a presentation gate could not be evaluated.
    /// Presentation only, counted separately.
    LocalWho,
}

impl TauntUnresolved {
    /// Whether this shortfall withheld **simulation** work. `FlavourRoll` and `LocalWho`
    /// only ever gate `msg`, `chat_to_local` and `Taunts::play`.
    pub fn is_simulation(self) -> bool {
        !matches!(
            self,
            TauntUnresolved::FlavourRoll | TauntUnresolved::LocalWho
        )
    }
}

/// One reached presentation call, in retail order. The headless core owns delivery, not
/// rendering: each receipt keeps the call's retail VA and its arguments so a product adapter
/// can perform it and a test can assert the sequence.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TauntProductCall {
    /// `IFaceDiploNeg::check_click_stamp` `0x008035E0`.
    DiploClickStamp { from: i32, to: i32 },
    /// `String::operator=(msg, loc_str_array_orig[ordinal])` `0x00A1EEB0`.
    SetMessage { ordinal: u32 },
    /// `msg = msg.parse(types[res]->name)` — `String::parse` `0x00A1CD60` at `0x006B8E86`.
    SubstituteResourceName { resource: usize },
    /// `msg = msg.parse(leaders[slot].get_name())` — `LeaderData::get_name` `0x006DAA70`.
    SubstituteLeaderName { slot: usize },
    /// `msg += taunts.list[Taunts::id_to_index(id)]->text` — `0x00980DC0` + `0x00A1D440`.
    AppendTauntText { taunt_id: i32 },
    /// `Leader::chat_to_local(msg, who, 2, flush)` `0x006EC520`.
    ChatToLocal { who: i32, kind: i32, flush: i32 },
    /// `Taunts::play(id)` `0x00980CF0`.
    PlayTaunt { taunt_id: i32 },
    /// `SoundGlobal::play(0x40)` `0x0097F770` — `action_offer`'s rejection blip.
    PlayUiSound { category: i32 },
    /// The `internal_random` draw request itself, emitted whether or not a roll was
    /// available, so the sequence is complete either way.
    FlavourRoll { lo: i32, hi: i32 },
}

/// What one `Leader::process_taunt` call did.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TauntCall {
    /// Array position of the leader whose table fired.
    pub slot: usize,
    /// First argument, `LeaderData::incoming_taunt[k]` (`+0x394`).
    pub kind: i32,
    /// Second argument, `LeaderData::incoming_taunt_who[k]` (`+0x3B4`).
    pub who: i32,
    /// Where retail returned, when it returned early.
    pub stop: Option<TauntStop>,
    /// Named calls and inputs this port could not execute, in the order reached.
    pub unresolved: Vec<TauntUnresolved>,
    /// Reached presentation calls, in retail order.
    pub product: Vec<TauntProductCall>,
    /// `Leader::action_offer` `0x006D1780` actually applied a ledger entry.
    pub offer_applied: Option<TauntOffer>,
    /// The five clamps at `0x006B94A4` ran, i.e. the dispatch took a `TAUNT_BUILD_*`,
    /// `TAUNT_RUSH`, `TAUNT_BOOM`, `TAUNT_ATTACK`, `TAUNT_DEFEND`, `TAUNT_HELP` or unknown
    /// arm rather than returning early.
    pub mods_clamped: bool,
}

impl TauntCall {
    /// Whether every retail call on this dispatch's **simulation** path executed.
    pub fn is_simulation_resolved(&self) -> bool {
        !self.unresolved.iter().any(|u| u.is_simulation())
    }
}

/// One applied `Leader::action_offer` `0x006D1780`, both sides.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TauntOffer {
    /// The offering leader's array position.
    pub from: usize,
    /// The receiving leader's array position.
    pub to: usize,
    pub resource: usize,
    pub amount: i32,
}

/// Per-frame taunt totals, for a scheduler that wants to charge a gap honestly.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct TauntPassCounts {
    /// `Leader::process_taunt` calls the table scan made.
    pub dispatched: u32,
    /// Of those, dispatches whose whole simulation path executed.
    pub resolved: u32,
    /// Named retail simulation calls or inputs that did not execute. **This is the number
    /// `Gap::LeaderProcessTaunt` should be charged**, not the dispatch count.
    pub unresolved_calls: u32,
    /// Presentation shortfalls (`internal_random`, `Console::who`), never simulation.
    pub presentation_unresolved: u32,
}

impl TauntPassCounts {
    fn record(&mut self, call: &TauntCall) {
        self.dispatched += 1;
        let sim_short = call.unresolved.iter().filter(|u| u.is_simulation()).count() as u32;
        self.unresolved_calls += sim_short;
        self.presentation_unresolved += call.unresolved.len() as u32 - sim_short;
        if sim_short == 0 {
            self.resolved += 1;
        }
    }
}

// ===========================================================================================
// Small ported bodies
// ===========================================================================================

/// `Game::teams_locked` `0x00594880` — `team_style` is none of `0`, `8`, `11`.
pub fn teams_locked(team_style: i8) -> bool {
    !matches!(team_style, 0 | 8 | 11)
}

/// The `sar` after a sign-bias `add`, i.e. C's truncating divide by a power of two.
/// `0x006B8F59`: `cdq ; and edx, mask ; add eax, edx ; sar eax, shift`.
fn sar_round_toward_zero(v: i32, shift: u32) -> i32 {
    let mask = (1i32 << shift) - 1;
    v.wrapping_add((v >> 31) & mask) >> shift
}

/// `LeaderData::bucket_add` `0x0043ED10`. The stockpile is stored `^ 0x8221`; retail
/// decodes, adds, and re-encodes, so this is a plain add on the decoded value.
fn bucket_add(leader: &mut Leader, resource: usize, amount: i32) {
    leader.econ.stockpile[resource] = leader.econ.stockpile[resource].wrapping_add(amount);
}

/// `Leader::clear_agree` `0x006D1AF0`.
///
/// Note the receiver split retail performs and Ghidra loses: `tributes` is read off `this`
/// (`EDX`), while `bucket_add`'s receiver is `leaders[this->who]` (`0x006D1B2D`). This port
/// keeps them separate for the same reason `leaders.rs` keeps `slot` and loop index
/// separate — they are only equal when the array is canonical.
fn clear_agree(ls: &mut Leaders, this_index: usize, who: usize) -> Option<()> {
    if ls.leaders[this_index].taunt.dip[who].agree == 1 {
        let owner = usize::try_from(ls.leaders[this_index].slot).ok()?;
        if owner >= NUM_LEADER_SLOTS {
            return None;
        }
        for res in 0..NUM_RESOURCES {
            // 0x006D1B18 — dows first, on `!= 0`, so a negative entry also refunds.
            let dow = ls.leaders[this_index].taunt.dip[who].dows[res];
            if dow != 0 {
                let held = ls.leaders[this_index].taunt.tributes[res];
                let take = if dow < held { dow } else { held };
                ls.leaders[this_index].taunt.tributes[res] = held.wrapping_sub(take);
                bucket_add(&mut ls.leaders[owner], res, take);
            }
            // 0x006D1B41 — offers second, on `> 0`.
            let offer = ls.leaders[this_index].taunt.dip[who].offers[res];
            if offer > 0 {
                let held = ls.leaders[this_index].taunt.tributes[res];
                let take = if offer < held { offer } else { held };
                ls.leaders[this_index].taunt.tributes[res] = held.wrapping_sub(take);
                bucket_add(&mut ls.leaders[owner], res, take);
            }
        }
        // 0x006D1B7D.
        ls.leaders[this_index].taunt.dip[who].clear_dows();
    }
    // 0x006D1B84 / 0x006D1B87 — unconditional, outside the `agree == 1` arm.
    ls.leaders[this_index].taunt.dip[who].agree = 0;
    ls.leaders[this_index].taunt.dip[who].any_offer = 0;
    Some(())
}

/// `Leader::action_clear_all` `0x006D15E0`.
///
/// `this_index` is the caller's array position; `who` is the counterparty's. Both
/// `clear_agree` calls and both `Diplomacy::clear_all` calls are two-sided, and the second
/// of each pair runs on `leaders[who]`.
fn action_clear_all(
    ls: &mut Leaders,
    env: &TauntEnv,
    this_index: usize,
    who: usize,
    call: &mut TauntCall,
) -> Option<()> {
    let me = usize::try_from(ls.leaders[this_index].slot).ok()?;
    if me >= NUM_LEADER_SLOTS {
        return None;
    }
    // 0x006D15FA.
    call.product.push(TauntProductCall::DiploClickStamp {
        from: me as i32,
        to: who as i32,
    });
    // 0x006D1602 — this->clear_agree(who).
    clear_agree(ls, this_index, who)?;
    // 0x006D1616 — leaders[who].clear_agree(this->who).
    clear_agree(ls, who, me)?;
    // 0x006D161B — presentation, local player only.
    match env.local_who {
        Some(local) if local == who as i32 => {
            if ls.leaders[this_index].taunt.dip[who].has_any_proposal() {
                call.product.push(TauntProductCall::SetMessage {
                    ordinal: loc::PROPOSALS_CLEARED,
                });
                call.product.push(TauntProductCall::ChatToLocal {
                    who: who as i32,
                    kind: 2,
                    flush: 1,
                });
            }
        }
        Some(_) => {}
        None => call.unresolved.push(TauntUnresolved::LocalWho),
    }
    // 0x006D166A / 0x006D167B — both records, both directions.
    ls.leaders[me].taunt.dip[who].clear_all();
    ls.leaders[who].taunt.dip[me].clear_all();
    Some(())
}

/// `Leader::action_offer` `0x006D1780`.
///
/// Three things the shape hides. The affordability test is skipped entirely when
/// `amount <= 0` (`0x006D17A9` jumps straight into the apply arm). The `any_offer = 1`
/// store at `0x006D17F8` is **dead** — `clear_agree` two instructions later zeroes the same
/// dword unconditionally — and is reproduced here rather than dropped, because a reader who
/// sees `any_offer == 0` after an offer should find the reason in the port. And the
/// reciprocal entry is written on `leaders[who]`, negated.
fn action_offer(
    ls: &mut Leaders,
    env: &TauntEnv,
    this_index: usize,
    who: usize,
    resource: usize,
    amount: i32,
    call: &mut TauntCall,
) -> Option<()> {
    let me = usize::try_from(ls.leaders[this_index].slot).ok()?;
    if me >= NUM_LEADER_SLOTS {
        return None;
    }
    call.product.push(TauntProductCall::DiploClickStamp {
        from: me as i32,
        to: who as i32,
    });

    if amount > 0 {
        // 0x006D17C1 — the already-pledged amount, off `leaders[this->who]`.
        let pledged = ls.leaders[me].taunt.dip[who].offers[resource];
        let stock = ls.leaders[me].econ.stockpile[resource];
        if pledged.wrapping_add(amount) > stock {
            // 0x006D17E1 — refused; a blip if it was the local player's own click.
            match env.local_who {
                Some(local) if local == me as i32 => {
                    call.product
                        .push(TauntProductCall::PlayUiSound { category: 0x40 });
                }
                Some(_) => {}
                None => call.unresolved.push(TauntUnresolved::LocalWho),
            }
            return Some(());
        }
    }

    // 0x006D17F8 — dead store, see the doc comment.
    ls.leaders[this_index].taunt.dip[who].any_offer = 1;
    // 0x006D1803 / 0x006D1817.
    clear_agree(ls, this_index, who)?;
    clear_agree(ls, who, me)?;
    // 0x006D1830 / 0x006D183F.
    ls.leaders[me].taunt.dip[who].offers[resource] =
        ls.leaders[me].taunt.dip[who].offers[resource].wrapping_add(amount);
    ls.leaders[who].taunt.dip[me].offers[resource] =
        ls.leaders[who].taunt.dip[me].offers[resource].wrapping_sub(amount);
    call.offer_applied = Some(TauntOffer {
        from: me,
        to: who,
        resource,
        amount,
    });
    Some(())
}

// ===========================================================================================
// Leader::process_taunt 0x006B8CC0
// ===========================================================================================

/// **`Leader::process_taunt` `0x006B8CC0`**, whole.
///
/// `this_index` is the array position of the leader whose `incoming_taunt` table fired;
/// `kind` is `LeaderData::incoming_taunt[k]` and `who` is `LeaderData::incoming_taunt_who[k]`
/// — that argument order is `0x006ED3ED`/`0x006ED3F1`, where the *second* push is the first
/// parameter.
///
/// `frame` is the pre-increment `Game + 0x550`, the same value the dispatcher matched the
/// table against.
pub fn process_taunt(
    ls: &mut Leaders,
    env: &mut TauntEnv,
    frame: i32,
    this_index: usize,
    kind: i32,
    who: i32,
) -> TauntCall {
    let mut call = TauntCall {
        slot: this_index,
        kind,
        who,
        stop: None,
        unresolved: Vec::new(),
        product: Vec::new(),
        offer_applied: None,
        mods_clamped: false,
    };

    // 0x006B8CE0 — every guard below indexes the array by `this->who`, not by this loop
    // position. They coincide in a canonical array and this port does not assume it.
    let Ok(me) = usize::try_from(ls.leaders[this_index].slot) else {
        call.unresolved.push(TauntUnresolved::SlotOutOfRange);
        return call;
    };
    if me >= NUM_LEADER_SLOTS {
        call.unresolved.push(TauntUnresolved::SlotOutOfRange);
        return call;
    }

    // 0x006B8CEF / 0x006B8CFB.
    if ls.leaders[me].flags & FLAG_HUMAN != 0 {
        call.stop = Some(TauntStop::LeaderIsHuman);
        return call;
    }
    if ls.leaders[me].flags & FLAG_ACTIVE == 0 {
        call.stop = Some(TauntStop::LeaderNotActive);
        return call;
    }

    // 0x006B8D01 — `leaders[this->who].who`, read back out of the array.
    let me_who = ls.leaders[me].slot;
    let Ok(who_idx) = usize::try_from(who) else {
        call.unresolved.push(TauntUnresolved::SlotOutOfRange);
        return call;
    };
    if who_idx >= NUM_LEADER_SLOTS {
        call.unresolved.push(TauntUnresolved::SlotOutOfRange);
        return call;
    }
    if who != me_who {
        // 0x006B8D0E — allied on the asked leader's own record …
        if ls.leaders[me].diplo[who_idx] != DIPLO_ALLIED {
            call.stop = Some(TauntStop::NotAllied);
            return call;
        }
        // 0x006B8D29 — … and on the asker's, indexed by `leaders[me].who`.
        let Ok(me_who_idx) = usize::try_from(me_who) else {
            call.unresolved.push(TauntUnresolved::SlotOutOfRange);
            return call;
        };
        if me_who_idx >= NUM_LEADER_SLOTS {
            call.unresolved.push(TauntUnresolved::SlotOutOfRange);
            return call;
        }
        if ls.leaders[who_idx].diplo[me_who_idx] != DIPLO_ALLIED {
            call.stop = Some(TauntStop::NotMutuallyAllied);
            return call;
        }
    }
    // 0x006B8D34 — compares `this->who` against the argument, after the alliance test.
    if me as i32 == who {
        call.stop = Some(TauntStop::Self_);
        return call;
    }

    // 0x006B8D46 / 0x006B8D54 — the two arrays the dispatcher never touches, written on
    // `this` (not on `leaders[this->who]`) and indexed by the argument.
    ls.leaders[this_index].taunt.last_taunt[who_idx] = kind;
    ls.leaders[this_index].taunt.last_taunt_frame[who_idx] = frame;

    match kind {
        taunt::FOOD..=taunt::OIL => {
            let resource = TAUNT_RESOURCE[(kind - taunt::FOOD) as usize];
            tribute_arm(ls, env, frame, this_index, me, who_idx, resource, &mut call);
        }
        taunt::NEED => need_arm(ls, env, me, who_idx, &mut call),
        _ => build_mod_arm(ls, env, this_index, me, who_idx, kind, &mut call),
    }
    call
}

/// `TAUNT_FOOD`..`TAUNT_OIL`, `0x006B8D89`..`0x006B8F2B`.
#[allow(clippy::too_many_arguments)]
fn tribute_arm(
    ls: &mut Leaders,
    env: &TauntEnv,
    frame: i32,
    this_index: usize,
    me: usize,
    who: usize,
    resource: usize,
    call: &mut TauntCall,
) {
    // 0x006B8D89 on `leaders[this->who]`, 0x006B8DA7 on `leaders[who]` — different
    // receivers, same query.
    for holder in [me, who] {
        match env.type_avail[holder][resource] {
            None => {
                call.unresolved.push(TauntUnresolved::TypeAvail);
                return;
            }
            Some(4) => {}
            Some(_) => {
                call.stop = Some(TauntStop::ResourceNotAvailable);
                return;
            }
        }
    }

    // 0x006B8DC4 — the result is kept and reused at 0x006B8EB1.
    let Some(team_style) = env.team_style else {
        call.unresolved.push(TauntUnresolved::TeamsLocked);
        return;
    };
    let locked = teams_locked(team_style);
    // 0x006B8DCE / 0x006B8DD9 — `is_neutral` is only asked when teams are locked.
    let cooldown_applies = if locked {
        match env.is_neutral[this_index] {
            None => {
                call.unresolved.push(TauntUnresolved::IsNeutral);
                return;
            }
            Some(n) => n,
        }
    } else {
        true
    };

    if cooldown_applies {
        // 0x006B8DDB..0x006B8DF8.
        let stamp = ls.leaders[this_index].taunt.gift_stamp[who];
        if stamp != 0 && frame.wrapping_sub(stamp) < GIFT_COOLDOWN_FRAMES {
            call.stop = Some(TauntStop::GiftCooldown);
            call.product.push(TauntProductCall::SetMessage {
                ordinal: loc::TRIBUTE_TOO_SOON,
            });
            chat_to_local(env, who as i32, 1, call);
            return;
        }
    }

    // 0x006B8E44 — the threshold read, off `leaders[this->who]`'s encrypted block.
    let stock = ls.leaders[me].econ.stockpile[resource];
    if stock < TRIBUTE_THRESHOLD {
        // 0x006B8E59 — "not enough", with the resource's own name substituted in.
        call.stop = Some(TauntStop::NotEnoughStockpile);
        call.product.push(TauntProductCall::SetMessage {
            ordinal: loc::TRIBUTE_NOT_ENOUGH,
        });
        call.product
            .push(TauntProductCall::SubstituteResourceName { resource });
        chat_to_local(env, who as i32, 1, call);
        return;
    }

    // 0x006B8EB1 — the same predicate as the cooldown, re-evaluated rather than cached.
    if cooldown_applies {
        ls.leaders[this_index].taunt.gift_stamp[who] = frame;
    }

    // 0x006B8ED7.
    if action_clear_all(ls, env, this_index, who, call).is_none() {
        call.unresolved.push(TauntUnresolved::SlotOutOfRange);
        return;
    }
    // 0x006B8EEA — the stockpile is read a **second** time. `action_clear_all` reaches
    // `bucket_add`, so this can differ from the value that passed the threshold.
    let payable = ls.leaders[me].econ.stockpile[resource] / 3;
    if action_offer(ls, env, this_index, who, resource, payable, call).is_none() {
        call.unresolved.push(TauntUnresolved::SlotOutOfRange);
        return;
    }
    // 0x006B8F16 — `Leader::action_respond(who, 1)`. Unported; see the module header.
    call.unresolved.push(TauntUnresolved::ActionRespond);
}

/// `TAUNT_NEED`, `0x006B9109`..`0x006B9261`. Nothing here writes leader state, which is why
/// it takes `&Leaders`: both econ reads and both `type_avail` receivers are
/// `leaders[this->who]` and `leaders[who]`, never the dispatching slot.
fn need_arm(ls: &Leaders, env: &TauntEnv, me: usize, who: usize, call: &mut TauntCall) {
    // 0x006B9109 — the running minimum starts at 9,999 and ties keep the later slot,
    // because the compare is `jg skip` on `amount > best`.
    let mut best_amount = 9999i32;
    let mut best_res: i32 = -1;
    for res in 0..NUM_RESOURCES {
        // 0x006B9130 / 0x006B9148 — `!= 0` here, not `== 4`.
        let mine = env.type_avail[me][res];
        let theirs = env.type_avail[who][res];
        if mine.is_none() || theirs.is_none() {
            call.unresolved.push(TauntUnresolved::TypeAvail);
            return;
        }
        if mine == Some(0) || theirs == Some(0) {
            continue;
        }
        // 0x006B9151 — knowledge is excluded by index, not by availability.
        if res == RES_KNOWLEDGE {
            continue;
        }
        let amount = ls.leaders[me].econ.stockpile[res];
        if amount <= best_amount {
            best_res = res as i32;
            best_amount = amount;
        }
    }
    if best_res < 0 {
        call.stop = Some(TauntStop::NoNeededResource);
        return;
    }
    let sound = taunt_sound::NEED_BY_RESOURCE[best_res as usize];

    // 0x006B91BC — everything past here is local-player presentation.
    match env.local_who {
        None => {
            call.unresolved.push(TauntUnresolved::LocalWho);
            return;
        }
        Some(local) if local != who as i32 => {
            call.stop = Some(TauntStop::NeedIsRemote);
            return;
        }
        Some(_) => {}
    }
    if sound == 0 {
        call.stop = Some(TauntStop::NeedIsRemote);
        return;
    }
    call.product.push(TauntProductCall::SetMessage {
        ordinal: loc::NEED_RESOURCE,
    });
    call.product
        .push(TauntProductCall::AppendTauntText { taunt_id: sound });
    // 0x006B920C — the audio option decides whether the line is flushed or the sound is.
    if env.taunt_audio {
        call.product.push(TauntProductCall::ChatToLocal {
            who: who as i32,
            kind: 2,
            flush: 0,
        });
        call.product
            .push(TauntProductCall::PlayTaunt { taunt_id: sound });
    } else {
        call.product.push(TauntProductCall::ChatToLocal {
            who: who as i32,
            kind: 2,
            flush: 1,
        });
    }
}

/// `TAUNT_BUILD_WONDER`..`TAUNT_HELP` and the default arm, `0x006B8F2E`..`0x006B954A`.
///
/// Every arm here writes leader state, and the five clamps at `0x006B94A4` run for **all**
/// of them including the unknown-code default — which is why an unrecognised taunt code
/// still normalises the scalars.
fn build_mod_arm(
    ls: &mut Leaders,
    env: &mut TauntEnv,
    this_index: usize,
    me: usize,
    who: usize,
    kind: i32,
    call: &mut TauntCall,
) {
    let mods = &mut ls.leaders[this_index].taunt.mods;
    let mut marked = true;
    match kind {
        // 0x006B8F2E.
        taunt::BUILD_WONDER => mods.wonder = 1,
        // 0x006B8F44.
        taunt::BUILD_GROUND => {
            mods.ground = mods.ground.wrapping_shl(5);
            mods.sea = sar_round_toward_zero(mods.sea, 4);
            mods.air = sar_round_toward_zero(mods.air, 4);
            mods.infra = sar_round_toward_zero(mods.infra, 2);
        }
        // 0x006B8F9E.
        taunt::BUILD_SEA => {
            mods.ground = sar_round_toward_zero(mods.ground, 4);
            mods.sea = mods.sea.wrapping_shl(5);
            mods.air = sar_round_toward_zero(mods.air, 4);
            mods.infra = sar_round_toward_zero(mods.infra, 2);
        }
        // 0x006B8FBE.
        taunt::BUILD_AIR => {
            mods.ground = sar_round_toward_zero(mods.ground, 4);
            mods.sea = sar_round_toward_zero(mods.sea, 4);
            mods.air = mods.air.wrapping_shl(5);
            mods.infra = sar_round_toward_zero(mods.infra, 2);
        }
        // 0x006B8FF3.
        taunt::BUILD_INFRA => {
            mods.ground = sar_round_toward_zero(mods.ground, 2);
            mods.sea = sar_round_toward_zero(mods.sea, 2);
            mods.air = sar_round_toward_zero(mods.air, 2);
            mods.infra = mods.infra.wrapping_shl(2);
        }
        // 0x006B904D.
        taunt::RUSH => {
            mods.ground = 0x200;
            mods.sea = 0x200;
            mods.air = 0x200;
            mods.infra = 0x80;
            mods.defense = 0x80;
            ls.leaders[this_index].taunt.personality_raid = 1;
        }
        // 0x006B9095.
        taunt::BOOM => {
            mods.ground = 4;
            mods.sea = 4;
            mods.air = 4;
            mods.infra = 0x200;
            mods.defense = 0x80;
            ls.leaders[this_index].taunt.personality_raid = -2;
        }
        // 0x006B90DD / 0x006B90F3.
        taunt::ATTACK => mods.defense = 0x40,
        taunt::DEFEND => mods.defense = 0x400,
        // 0x006B8F38 — marks, writes nothing.
        taunt::HELP => {}
        // 0x006B9264 — the jump table's `ja` arm. `[ebp+0xc] = edi` with `edi == 0` is what
        // leaves it unmarked, so the presentation block is skipped and only the clamps run.
        _ => {
            marked = false;
            call.stop = Some(TauntStop::UnknownTauntCode);
        }
    }

    // 0x006B9267..0x006B94A4 — presentation. `msg = EMPTY_STRING` first, so `TAUNT_HELP`
    // reaches the length gate at 0x006B9460 with an empty message and emits nothing.
    let mut message_set = false;
    let mut sound = 0i32;
    if marked {
        let local = match env.local_who {
            None => {
                call.unresolved.push(TauntUnresolved::LocalWho);
                None
            }
            Some(l) => Some(l),
        };
        if local == Some(who as i32) {
            // 0x006B9290 — `TAUNT_HELP` skips the draw entirely.
            if kind != taunt::HELP {
                call.product.push(TauntProductCall::FlavourRoll {
                    lo: FLAVOUR_ROLL_RANGE.0,
                    hi: FLAVOUR_ROLL_RANGE.1,
                });
                match env.flavour_rolls.pop_front() {
                    None => call.unresolved.push(TauntUnresolved::FlavourRoll),
                    Some(roll) if (0..=10).contains(&roll) => {
                        call.product.push(TauntProductCall::SetMessage {
                            ordinal: loc::FLAVOUR_FIRST + roll as u32,
                        });
                        message_set = true;
                        if roll == 8 {
                            // 0x006B93B0 — the name arm plays nothing.
                            call.product
                                .push(TauntProductCall::SubstituteLeaderName { slot: me });
                        } else {
                            sound = taunt_sound::FLAVOUR_FIRST + roll;
                        }
                    }
                    // 0x006B92A8 — `cmp eax, 0xA ; ja` falls past the whole switch.
                    Some(_) => {}
                }
            }
            // 0x006B9453..0x006B94A4.
            if message_set {
                if sound != 0 && env.taunt_audio {
                    call.product.push(TauntProductCall::ChatToLocal {
                        who: who as i32,
                        kind: 2,
                        flush: 0,
                    });
                    call.product
                        .push(TauntProductCall::PlayTaunt { taunt_id: sound });
                } else {
                    call.product.push(TauntProductCall::ChatToLocal {
                        who: who as i32,
                        kind: 2,
                        flush: 1,
                    });
                }
            }
        }
    }

    // 0x006B94A4 — always, for every arm that reaches here.
    let mods = &mut ls.leaders[this_index].taunt.mods;
    mods.ground = mods.ground.clamp(1, 0x8000);
    mods.sea = mods.sea.clamp(1, 0x8000);
    mods.air = mods.air.clamp(1, 0x8000);
    mods.infra = mods.infra.clamp(0x80, 0x8000);
    mods.defense = mods.defense.clamp(0x10, 0x1000);
    call.mods_clamped = true;
}

/// `Leader::chat_to_local(msg, who, 2, flush)` `0x006EC520`. The call is unconditional in
/// retail; the function itself compares `who` against `Console + 0x298` and returns.
fn chat_to_local(env: &TauntEnv, who: i32, flush: i32, call: &mut TauntCall) {
    if env.local_who.is_none() {
        call.unresolved.push(TauntUnresolved::LocalWho);
    }
    call.product.push(TauntProductCall::ChatToLocal {
        who,
        kind: 2,
        flush,
    });
}

/// The dispatcher's taunt scan body, `0x006B8D5B`-side. Kept here so `leaders.rs` holds one
/// call rather than a copy of the argument decode.
pub fn dispatch_table_entry(
    ls: &mut Leaders,
    env: &mut TauntEnv,
    counts: &mut TauntPassCounts,
    frame: i32,
    this_index: usize,
    entry: usize,
) -> TauntCall {
    // 0x006ED3E2 / 0x006ED3ED / 0x006ED3F1 — both arguments are re-read per entry.
    let kind = ls.leaders[this_index].taunt_kind[entry];
    let who = ls.leaders[this_index].taunt_arg[entry];
    let call = process_taunt(ls, env, frame, this_index, kind, who);
    counts.record(&call);
    call
}

// ===========================================================================================
// Tests
// ===========================================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::systems::leaders::flag;

    fn allied_pair() -> (Leaders, TauntEnv) {
        let mut ls = Leaders::new();
        for i in 0..NUM_LEADER_SLOTS {
            ls.leaders[i].slot = i as i32;
            ls.leaders[i].flags = flag::IN_GAME | flag::PROCESS;
        }
        ls.leaders[0].diplo[1] = DIPLO_ALLIED;
        ls.leaders[1].diplo[0] = DIPLO_ALLIED;
        let mut env = TauntEnv {
            local_who: Some(-1),
            team_style: Some(0),
            taunt_audio: false,
            ..TauntEnv::default()
        };
        for i in 0..NUM_LEADER_SLOTS {
            env.is_neutral[i] = Some(false);
            for r in 0..NUM_RESOURCES {
                env.type_avail[i][r] = Some(4);
            }
        }
        (ls, env)
    }

    #[test]
    fn teams_locked_excludes_exactly_three_styles() {
        for style in -8i8..=16 {
            assert_eq!(
                teams_locked(style),
                !matches!(style, 0 | 8 | 11),
                "team_style {style}"
            );
        }
    }

    #[test]
    fn divide_by_sixteen_truncates_toward_zero() {
        assert_eq!(sar_round_toward_zero(31, 4), 1);
        assert_eq!(sar_round_toward_zero(-31, 4), -1);
        assert_eq!(sar_round_toward_zero(-1, 4), 0);
        assert_eq!(sar_round_toward_zero(-3, 2), 0);
        assert_eq!(sar_round_toward_zero(-4, 2), -1);
    }

    #[test]
    fn a_human_leader_never_processes_a_taunt() {
        let (mut ls, mut env) = allied_pair();
        ls.leaders[0].flags |= FLAG_HUMAN;
        let call = process_taunt(&mut ls, &mut env, 100, 0, taunt::FOOD, 1);
        assert_eq!(call.stop, Some(TauntStop::LeaderIsHuman));
        assert_eq!(ls.leaders[0].taunt.last_taunt[1], 0);
    }

    #[test]
    fn the_two_arrays_are_written_before_the_switch() {
        // An unknown code still stamps both, because the stores are at 0x006B8D46 and
        // 0x006B8D54 — ahead of the jump table.
        let (mut ls, mut env) = allied_pair();
        let call = process_taunt(&mut ls, &mut env, 77, 0, 999, 1);
        assert_eq!(call.stop, Some(TauntStop::UnknownTauntCode));
        assert_eq!(ls.leaders[0].taunt.last_taunt[1], 999);
        assert_eq!(ls.leaders[0].taunt.last_taunt_frame[1], 77);
        // …and the clamps still ran.
        assert!(call.mods_clamped);
        assert_eq!(ls.leaders[0].taunt.mods.ground, 1);
        assert_eq!(ls.leaders[0].taunt.mods.infra, 0x80);
        assert_eq!(ls.leaders[0].taunt.mods.defense, 0x10);
    }

    #[test]
    fn a_tribute_needs_the_resource_available_on_both_leaders() {
        let (mut ls, mut env) = allied_pair();
        ls.leaders[0].econ.stockpile[0] = 900;
        // The asked leader has food; the asker does not. Ghidra's rendering of the two
        // calls as one repeated query cannot express this case.
        env.type_avail[1][0] = Some(0);
        let call = process_taunt(&mut ls, &mut env, 100, 0, taunt::FOOD, 1);
        assert_eq!(call.stop, Some(TauntStop::ResourceNotAvailable));
        assert!(call.offer_applied.is_none());
    }

    #[test]
    fn the_threshold_is_signed_and_the_offer_is_a_third() {
        let (mut ls, mut env) = allied_pair();
        ls.leaders[0].econ.stockpile[0] = 0x95;
        let low = process_taunt(&mut ls, &mut env, 100, 0, taunt::FOOD, 1);
        assert_eq!(low.stop, Some(TauntStop::NotEnoughStockpile));

        ls.leaders[0].econ.stockpile[0] = 0x96;
        let ok = process_taunt(&mut ls, &mut env, 100, 0, taunt::FOOD, 1);
        assert_eq!(ok.stop, None);
        assert_eq!(
            ok.offer_applied,
            Some(TauntOffer {
                from: 0,
                to: 1,
                resource: 0,
                amount: 0x96 / 3,
            })
        );
        assert_eq!(ls.leaders[0].taunt.dip[1].offers[0], 50);
        assert_eq!(ls.leaders[1].taunt.dip[0].offers[0], -50);
        // The one thing this port refuses to make up.
        assert_eq!(ok.unresolved, vec![TauntUnresolved::ActionRespond]);

        // A negative stockpile takes the "not enough" branch rather than wrapping.
        ls.leaders[0].econ.stockpile[0] = -1;
        ls.leaders[0].taunt.gift_stamp[1] = 0;
        let negative = process_taunt(&mut ls, &mut env, 100, 0, taunt::FOOD, 1);
        assert_eq!(negative.stop, Some(TauntStop::NotEnoughStockpile));
    }

    #[test]
    fn the_offered_amount_is_read_after_action_clear_all_refunds() {
        let (mut ls, mut env) = allied_pair();
        ls.leaders[0].econ.stockpile[0] = 300;
        // Arm the refund: `agree == 1` plus escrowed tributes plus a pledged offer.
        ls.leaders[0].taunt.dip[1].agree = 1;
        ls.leaders[0].taunt.dip[1].offers[0] = 60;
        ls.leaders[0].taunt.tributes[0] = 60;
        let call = process_taunt(&mut ls, &mut env, 100, 0, taunt::FOOD, 1);
        // 300 passed the threshold; `clear_agree` then refunded 60, and the `/3` used 360.
        assert_eq!(ls.leaders[0].econ.stockpile[0], 360);
        assert_eq!(call.offer_applied.unwrap().amount, 120);
        assert_eq!(ls.leaders[0].taunt.tributes[0], 0);
    }

    #[test]
    fn the_gift_cooldown_is_skipped_when_teams_are_locked_and_nobody_is_neutral() {
        let (mut ls, mut env) = allied_pair();
        ls.leaders[0].econ.stockpile[0] = 900;
        ls.leaders[0].taunt.gift_stamp[1] = 10;

        // team_style 0 -> not locked -> the cooldown applies.
        env.team_style = Some(0);
        let blocked = process_taunt(&mut ls, &mut env, 100, 0, taunt::FOOD, 1);
        assert_eq!(blocked.stop, Some(TauntStop::GiftCooldown));

        // team_style 1 -> locked, and slot 0 is not neutral -> no cooldown at all.
        env.team_style = Some(1);
        let allowed = process_taunt(&mut ls, &mut env, 100, 0, taunt::FOOD, 1);
        assert_eq!(allowed.stop, None);
        // …and because the cooldown did not apply, the stamp was not refreshed.
        assert_eq!(ls.leaders[0].taunt.gift_stamp[1], 10);
    }

    #[test]
    fn a_missing_host_answer_refuses_the_dispatch_instead_of_guessing() {
        let (mut ls, mut env) = allied_pair();
        ls.leaders[0].econ.stockpile[0] = 900;
        env.team_style = None;
        let call = process_taunt(&mut ls, &mut env, 100, 0, taunt::FOOD, 1);
        assert_eq!(call.unresolved, vec![TauntUnresolved::TeamsLocked]);
        assert!(call.offer_applied.is_none());
        assert_eq!(ls.leaders[0].taunt.dip[1].offers[0], 0);
    }

    #[test]
    fn build_taunts_rewrite_and_clamp_the_ai_scalars() {
        let (mut ls, mut env) = allied_pair();
        ls.leaders[0].taunt.mods = BuildMods {
            wonder: 0,
            ground: 0x100,
            air: 0x100,
            sea: 0x100,
            infra: 0x100,
            defense: 0x100,
        };
        let call = process_taunt(&mut ls, &mut env, 100, 0, taunt::BUILD_GROUND, 1);
        assert_eq!(call.stop, None);
        let m = ls.leaders[0].taunt.mods;
        assert_eq!(m.ground, 0x2000);
        assert_eq!(m.sea, 0x10);
        assert_eq!(m.air, 0x10);
        // 0x100 >> 2 = 0x40, clamped up to the 0x80 floor.
        assert_eq!(m.infra, 0x80);
        assert_eq!(m.defense, 0x100);
    }

    #[test]
    fn rush_and_boom_write_personality_raid() {
        let (mut ls, mut env) = allied_pair();
        process_taunt(&mut ls, &mut env, 100, 0, taunt::RUSH, 1);
        assert_eq!(ls.leaders[0].taunt.personality_raid, 1);
        assert_eq!(ls.leaders[0].taunt.mods.infra, 0x80);
        process_taunt(&mut ls, &mut env, 100, 0, taunt::BOOM, 1);
        assert_eq!(ls.leaders[0].taunt.personality_raid, -2);
        assert_eq!(ls.leaders[0].taunt.mods.ground, 4);
        assert_eq!(ls.leaders[0].taunt.mods.infra, 0x200);
    }

    #[test]
    fn the_flavour_roll_is_presentation_and_never_charges_the_simulation() {
        let (mut ls, mut env) = allied_pair();
        env.local_who = Some(1);
        let call = process_taunt(&mut ls, &mut env, 100, 0, taunt::ATTACK, 1);
        assert_eq!(call.unresolved, vec![TauntUnresolved::FlavourRoll]);
        assert!(call.is_simulation_resolved());
        // The scalar write and the clamp happened anyway.
        assert_eq!(ls.leaders[0].taunt.mods.defense, 0x40);

        env.flavour_rolls.push_back(8);
        let named = process_taunt(&mut ls, &mut env, 100, 0, taunt::ATTACK, 1);
        assert!(named
            .product
            .contains(&TauntProductCall::SubstituteLeaderName { slot: 0 }));
        assert!(!named
            .product
            .iter()
            .any(|p| matches!(p, TauntProductCall::PlayTaunt { .. })));
    }

    #[test]
    fn taunt_help_marks_but_emits_nothing() {
        let (mut ls, mut env) = allied_pair();
        env.local_who = Some(1);
        env.flavour_rolls.push_back(0);
        let call = process_taunt(&mut ls, &mut env, 100, 0, taunt::HELP, 1);
        assert_eq!(call.stop, None);
        assert!(call.product.is_empty());
        // The roll was not consumed: 0x006B9294 skips the draw for TAUNT_HELP.
        assert_eq!(env.flavour_rolls.len(), 1);
        assert!(call.mods_clamped);
    }

    #[test]
    fn need_picks_the_lowest_non_knowledge_resource_and_ties_go_to_the_later_slot() {
        let (mut ls, mut env) = allied_pair();
        env.local_who = Some(1);
        ls.leaders[0].econ.stockpile = [50, 50, 700, 0, 700, 700];
        let call = process_taunt(&mut ls, &mut env, 100, 0, taunt::NEED, 1);
        // Knowledge is 0 and would win on value, but slot 3 is skipped by index.
        assert!(call.product.contains(&TauntProductCall::AppendTauntText {
            taunt_id: taunt_sound::NEED_BY_RESOURCE[1],
        }));
    }

    #[test]
    fn need_writes_no_leader_state() {
        let (mut ls, mut env) = allied_pair();
        env.local_who = Some(1);
        ls.leaders[0].econ.stockpile = [50, 700, 700, 0, 700, 700];
        let before = ls.leaders[0].taunt;
        let call = process_taunt(&mut ls, &mut env, 100, 0, taunt::NEED, 1);
        let mut after = ls.leaders[0].taunt;
        // Only the two pre-switch stamps may differ.
        after.last_taunt = before.last_taunt;
        after.last_taunt_frame = before.last_taunt_frame;
        assert_eq!(before, after);
        assert!(!call.mods_clamped);
    }

    #[test]
    fn an_out_of_range_target_is_refused_rather_than_read_out_of_bounds() {
        let (mut ls, mut env) = allied_pair();
        let call = process_taunt(&mut ls, &mut env, 100, 0, taunt::BUILD_INFRA, 22);
        assert_eq!(call.unresolved, vec![TauntUnresolved::SlotOutOfRange]);
        assert!(!call.mods_clamped);
    }

    #[test]
    fn action_offer_refuses_more_than_the_stockpile_backs() {
        let (mut ls, mut env) = allied_pair();
        env.local_who = Some(0);
        let mut call = TauntCall {
            slot: 0,
            kind: taunt::FOOD,
            who: 1,
            stop: None,
            unresolved: Vec::new(),
            product: Vec::new(),
            offer_applied: None,
            mods_clamped: false,
        };
        ls.leaders[0].econ.stockpile[0] = 10;
        action_offer(&mut ls, &env, 0, 1, 0, 11, &mut call);
        assert!(call.offer_applied.is_none());
        assert!(call
            .product
            .contains(&TauntProductCall::PlayUiSound { category: 0x40 }));

        // A non-positive amount skips the affordability test entirely (0x006D17A9).
        let mut ok = call.clone();
        ok.product.clear();
        ok.offer_applied = None;
        action_offer(&mut ls, &env, 0, 1, 0, -5, &mut ok);
        assert_eq!(ok.offer_applied.unwrap().amount, -5);
        assert_eq!(ls.leaders[0].taunt.dip[1].offers[0], -5);
        assert_eq!(ls.leaders[1].taunt.dip[0].offers[0], 5);
    }

    #[test]
    fn clear_all_sets_treaty_to_minus_one_not_zero() {
        let mut d = Diplomacy {
            agree: 1,
            any_offer: 1,
            treaty: 3,
            offers: [7; NUM_RESOURCES],
            dows: [7; NUM_RESOURCES],
            attacks: [7; NUM_LEADER_SLOTS],
        };
        d.clear_all();
        assert_eq!(d.treaty, -1);
        assert_eq!(
            d,
            Diplomacy {
                treaty: -1,
                ..Diplomacy::default()
            }
        );
    }
}
