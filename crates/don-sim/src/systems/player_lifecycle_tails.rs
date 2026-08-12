// SPDX-License-Identifier: GPL-3.0-or-later
//! The player-lifecycle tails that command rows 70, 71 and 80 stop at today.
//!
//! Rows 70/71/80 decode and plan their exact handler prefixes in
//! [`super::adjacent`]/[`super::late`] and
//! [`super`](super), but every one of them ended at an unrecovered `Player::*` /
//! `DropControl::*` body.  This module recovers those bodies:
//!
//! | retail | VA | size |
//! |---|---|---:|
//! | `Player::leave_game(int)` | `0x006EE010` | 256 |
//! | `Player::resign(int)` | `0x006EDCB0` | 459 |
//! | `Player::quit(int)` | `0x006EDC00` | 174 |
//! | `Player::drop(void)` | `0x006EDE80` | 397 |
//! | `DropControl::process_drop(int, int)` | `0x00959500` | 933 |
//!
//! **Tier C.**  Every branch, constant and store below is `[measured]` by capstone
//! disassembly of `ron-bin/riseofnations.exe` (sha256 `30478a44…625079`), named from
//! `ron-bin/sbl/rise.pdb` and its type stream.  Nothing here has been executed against
//! retail, and a passing test in this crate is a transcription pin, not a differential run.
//!
//! Two callees are deliberately *not* flattened into these planners and surface as typed
//! [`LifecycleCall`] work the host must execute, or as a [`LifecycleBoundary`]:
//!
//! * `Leader::defeat` `0x006ECB00` — already owned by
//!   [`super::super::victory_score::Leaders::defeat`], including the terminal queue/Unit
//!   cleanup and `Game::check_victory`.  The command bridge's [`Fleet`](crate::command::Fleet)
//!   host owns no `Leaders`, so an applied lifecycle receipt still requires a host that does.
//! * `Leader::action_declare` `0x006DAB50` — the open tail of command row 38.  Only
//!   `DropControl::process_drop` states 1 and 2 reach it.
//! * `LeaderData::find_capital` `0x006EB930` — reached only by the capital-elimination arm
//!   of `Player::leave_game`.
//!
//! The layouts are from the PDB type stream: `Player` (stride `0x8C`) `flags:u16 +0x30`,
//! `tribe +0x32`, `who +0x33`, `team:i8 +0x34`, `play +0x36`; `GameInfo` at `Game+0x0C`, so
//! `GameInfo::team_style` is `Game+0x24`, `GameInfo::elimination` is `Game+0x37` and
//! `GameInfo::player[8]` is `Game+0x44`; `Game::playing` `+0x558`; `Game::semaphore`
//! `BitMask<256>` `+0x814`, whose `flags` dword is `Game+0x81C` and whose 32 bit-bytes start
//! at `Game+0x820`; `LeaderData` (stride `0x6EEC`) `leader_flags +0x00`, `who +0x08`,
//! `multi_diff +0x50`, `diplos[8] +0x74`, `lost_capital_timer +0x418`; `Console::who +0x298`
//! and `Console::play +0x2A0`.

pub const PLAYER_SLOTS: usize = 8;
pub const LEADER_SLOTS: usize = 8;
/// `Player` record stride, `imul … 0x8C` at `0x006EDD1B`, `0x0095957D` and `0x00943972`.
pub const PLAYER_STRIDE: usize = 0x8C;
/// `LeaderData` record stride, `imul … 0x6EEC` at `0x006EE08F` and `0x0095966D`.
pub const LEADER_STRIDE: usize = 0x6EEC;
/// `Game::semaphore` bit-byte count (`BitMask<256>::ptr`).
pub const SEMAPHORE_BYTES: usize = 32;

// ---------------------------------------------------------------------------
// `Player::flags` bits.  The PDB names the field but not its bits; each constant below
// names only the measured test/store site.
// ---------------------------------------------------------------------------

/// Tested by both co-tenant scans (`0x006EE05C`, `0x009595A7`).  Same bit as
/// [`super::super::setup_diplomacy::PLAYER_PRESENT`].
pub const PLAYER_PRESENT: u16 = 0x0001;
/// Additionally required by the `Player::leave_game` scan at `0x006EE058`.  The otherwise
/// identical `DropControl::process_drop` scan does **not** test it.
pub const PLAYER_LEAVE_SCAN_REQUIRED: u16 = 0x0004;
/// Set by `Player::leave_game` at `0x006EE01D` (`or word ptr [esi+0x30], 0x10`).
pub const PLAYER_LEFT: u16 = 0x0010;
/// Set by `Player::resign` at `0x006EDCD7` (`or word ptr [esi+0x30], 0x40`).
pub const PLAYER_RESIGNED: u16 = 0x0040;
/// The `DropControl::process_drop` state-3 entry gate at `0x00959583`.
pub const PLAYER_DROP_GATE: u16 = 0x0080;
/// `test eax, 0x100` at `0x006EE060` / `0x009595AB`; a set bit rejects the candidate.
pub const PLAYER_SCAN_EXCLUDE_HIGH: u16 = 0x0100;
/// `test al, 0xD0` at `0x006EE067` / `0x009595B2`; any set bit rejects the candidate.
pub const PLAYER_SCAN_EXCLUDE_MASK: u16 = 0x00D0;
/// `and word ptr [players[play].flags], 0xFFEB` at `0x00959668` — state 3 clears exactly
/// [`PLAYER_LEAVE_SCAN_REQUIRED`] and [`PLAYER_LEFT`].
pub const PLAYER_DROP_RETAIN_MASK: u16 = 0xFFEB;
/// `Player::team` value written to every present player by `process_drop` states 1 and 2
/// (`mov byte ptr [.. + 0x34], 8`).  Same sentinel as
/// [`super::super::setup_diplomacy::TEAM_AUTO`].
pub const PLAYER_TEAM_AUTO: i8 = 8;

// ---------------------------------------------------------------------------
// `LeaderData::leader_flags` bits and other leader state.
// ---------------------------------------------------------------------------

/// `test byte ptr [leader], 1` at `0x00959741` and `0x00959753`, and the low bit of the
/// `and eax, 3` at `0x006EE09B`.  Same bit as
/// [`super::super::victory_score::leader_flag::VALID`].
pub const LEADER_VALID: i32 = 0x01;
/// `and eax, 3; cmp al, 3` at `0x006EE09B` — both low bits are required before the
/// capital-elimination arm of `Player::leave_game` is even considered.
pub const LEADER_VALID_ACTIVE: i32 = 0x03;
/// [`super::super::victory_score::leader_flag::HUMAN`].  `process_drop` state 3 clears it
/// (`and dword ptr [leader], 0xFFFFFFFB`, `0x00959673`) and writes
/// [`DROPPED_LEADER_MULTI_DIFF`] — the dropped player's leader becomes AI-driven.
pub const LEADER_HUMAN: i32 = 0x04;
/// `mov dword ptr [leader + 0x50], 3` at `0x0095967A`: the dropped player's leader is left
/// at `LeaderData::multi_diff == 3`.
pub const DROPPED_LEADER_MULTI_DIFF: i32 = 3;

// ---------------------------------------------------------------------------
// `Game::semaphore` bytes touched here.  Bit `n` lives at byte `n / 8`, mask `1 << (n % 8)`.
// ---------------------------------------------------------------------------

/// `Game+0x820 & 0x04` — semaphore bit 2, `victory_score::game_sem::NET_OR_RECORDING`.
/// Gates the whole co-tenant scan in `Player::leave_game` (`0x006EE022`, re-tested at
/// `0x006EE047`).
pub const SEM_NET_OR_RECORDING: u32 = 2;
/// `Game+0x820 & 0x10` — semaphore bit 4.  `CommandPackage::process_ungraceful_player_drop`
/// requires it before entering `DropControl` (`0x00943EFA`); `Player::quit` and the row-71
/// handler both branch on it.
pub const SEM_DROP_CONTROL: u32 = 4;
/// `Game+0x820 & 0x40` — semaphore bit 6, `victory_score::game_sem::GAME_OVER`.  Suppresses
/// the `IFaceMainBase::do_notice` envelope.
pub const SEM_GAME_OVER: u32 = 6;
/// `Game+0x821 & 0x80` — semaphore bit 15.  Set when the local player resigns
/// (`0x006EDE59`), saved/restored across `Player::quit` (`0x006EDC0F`).
pub const SEM_LOCAL_LEFT: u32 = 15;
/// `Game+0x822 & 0x04` — semaphore bit 18, set by the row-71 handler prefix at `0x00943A95`.
pub const SEM_QUIT_PREFIX: u32 = 18;

// ---------------------------------------------------------------------------
// `Leader::defeat` arguments and `SoundGlobalCat` values.
// ---------------------------------------------------------------------------

/// `push 1` at `0x006EE0C4` / `0x006EE0EF`: `DefeatTypeIndex::DEFEAT_CAPITAL`.
pub const DEFEAT_TYPE_CAPITAL: i32 = 1;
/// `push 6` at `0x006EDE3A`: `Player::resign` leaves with `DefeatTypeIndex` 6
/// (`victory_score::DefeatType::Resign`).
pub const LEAVE_REASON_RESIGN: i32 = 6;
/// `push 7` at `0x006EDFF5` in `Player::drop`: `DefeatTypeIndex` 7
/// (`victory_score::DefeatType::Disconnect`).
pub const LEAVE_REASON_DROP: i32 = 7;

/// `push 0x80` at `0x006EDCF4` — the local player resigned.
pub const SOUND_LOCAL_RESIGN: i32 = 0x80;
/// `push 0x153` at `0x006EDCEA` and `0x006EDEAF` — the local player quit or dropped.
pub const SOUND_LOCAL_QUIT: i32 = 0x153;
/// `push 7` at `0x006EDE33` — a friendly (own or mutually allied) leader left.
pub const SOUND_FRIENDLY_LEFT: i32 = 7;
/// `push 0x22` at `0x006EDE2F` — any other leader left.
pub const SOUND_OTHER_LEFT: i32 = 0x22;
/// `DiploButtonCats::DIPLO_ALLY`, compared by the mutual test at `0x006EDE14`/`0x006EDE28`.
pub const DIPLO_ALLY: i32 = 2;

// ---------------------------------------------------------------------------
// String-table records referenced by the presentation envelopes.  Both arrays hold 20-byte
// `String` records, so a `.text` byte offset decodes as `20 * index`.
// ---------------------------------------------------------------------------

/// Record index into `[[0x00C06378] + 0x10]` (the internal-string array): `0x1A3EC / 20`.
pub const INTERNAL_STRING_NOTICE: u32 = 5375;
/// Record index into the same array: `0x1A400 / 20`, the `GameLog::create_report` title.
pub const INTERNAL_STRING_QUIT_REPORT: u32 = 5376;
/// Record index into the same array: `0xCD28 / 20`, the `DropControl` log line.
pub const INTERNAL_STRING_DROP_LOG: u32 = 2626;

/// Record index into the distinct `[0x00C8CD00]` string array: `0xF104 / 20`, the
/// "player resigned" message consumed by `Player::resign`.
pub const TEXT_RESIGNED: u32 = 3085;
/// `0xF0F0 / 20`, the "player dropped" message consumed by `Player::drop`.
pub const TEXT_DROPPED: u32 = 3084;
/// `0x56F4 / 20`, the drop-vote message emitted by `DropControl::process_drop` state 3.
pub const TEXT_DROP_VOTE: u32 = 1113;

// ---------------------------------------------------------------------------
// Image
// ---------------------------------------------------------------------------

/// One `Player` record, exactly the fields these bodies read or write.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PlayerRow {
    /// `Player+0x30`.
    pub flags: u16,
    /// `Player+0x33`.
    pub who: u8,
    /// `Player+0x34`, signed.
    pub team: i8,
    /// `Player+0x36`.  Retail indexes `GameInfo::player[]` with it *and* scans that array by
    /// slot, so the planner refuses an image where the two disagree.
    pub play: u8,
}

/// One `LeaderData` record, exactly the fields these bodies read or write.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LeaderRow {
    /// `LeaderData+0x00`.
    pub leader_flags: i32,
    /// `LeaderData+0x08`.
    pub who: i32,
    /// `LeaderData+0x50`.
    pub multi_diff: i32,
    /// `LeaderData+0x418`.
    pub lost_capital_timer: i32,
    /// `LeaderData+0x74`, the directional declarations.
    pub diplos: [i32; LEADER_SLOTS],
}

impl Default for LeaderRow {
    fn default() -> Self {
        Self {
            leader_flags: 0,
            who: 0,
            multi_diff: 0,
            lost_capital_timer: 0,
            diplos: [0; LEADER_SLOTS],
        }
    }
}

/// The complete host image these five bodies read.  Every field is required; there is no
/// "unknown" arm, because a missing fact here would silently change a branch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LifecycleImage {
    pub players: [PlayerRow; PLAYER_SLOTS],
    pub leaders: [LeaderRow; LEADER_SLOTS],
    /// `GameInfo::team_style`, `Game+0x24`.
    pub team_style: u8,
    /// `GameInfo::elimination`, `Game+0x37`.
    pub elimination: u8,
    /// `Game::playing`, `Game+0x558`.
    pub playing: i32,
    /// `Game::semaphore.ptr`, `Game+0x820`.
    pub semaphore: [u8; SEMAPHORE_BYTES],
    /// `Game::semaphore.flags`, `Game+0x81C`.
    pub semaphore_flags: i32,
    /// `Console::play`, `Console+0x2A0`.
    pub console_play: i32,
    /// `Console::who`, `Console+0x298`.
    pub console_who: i32,
    /// `DropControl+0x94` — the "vote window already opened" latch.
    pub drop_window_open: bool,
}

impl Default for LifecycleImage {
    fn default() -> Self {
        Self {
            players: [PlayerRow::default(); PLAYER_SLOTS],
            leaders: [LeaderRow::default(); LEADER_SLOTS],
            team_style: 0,
            elimination: 0,
            playing: 1,
            semaphore: [0; SEMAPHORE_BYTES],
            semaphore_flags: 0,
            console_play: -1,
            console_who: -1,
            drop_window_open: false,
        }
    }
}

impl LifecycleImage {
    #[inline]
    pub fn sem(&self, bit: u32) -> bool {
        self.semaphore[(bit / 8) as usize] & (1u8 << (bit % 8)) != 0
    }

    /// Host-side helper for the row-71 handler prefix, which writes the same semaphore the
    /// lifecycle body then reads.
    #[inline]
    pub fn set_semaphore_bit(&mut self, bit: u32, on: bool) {
        let byte = (bit / 8) as usize;
        let mask = 1u8 << (bit % 8);
        if on {
            self.semaphore[byte] |= mask;
        } else {
            self.semaphore[byte] &= !mask;
        }
    }
}

// ---------------------------------------------------------------------------
// Effects
// ---------------------------------------------------------------------------

/// Ordered presentation receipts.  These do not mutate walked simulation state, and they
/// never authorize skipping a state boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LifecyclePresentation {
    /// `Player::name_with_platform_symbol` `0x006EE1D0` for one player row.
    PlayerName { play: u8 },
    /// `String::parse` + `MessageWin::add_message` `0x007E9FB0`.
    Message { text_record: u32 },
    /// `IFaceMainBase::do_notice` `0x008107D0`.
    Notice { internal_string_record: u32 },
    /// `SoundGlobal::play(category)` `0x0097F770`.  Sound selection may draw from the sound
    /// RNG when a renderer delivers it; that stream is not simulation authority.
    Sound { category: i32 },
    /// `GameLog::create_report` `0x00931DC0`.
    Report { internal_string_record: u32 },
    /// `DropControl::clear` `0x0095A100`, the `0xCD28` log line and `MPDropWin::init`
    /// `0x0095A640` — retail's drop-vote screen.
    DropVoteWindow { internal_string_record: u32 },
}

/// Authoritative simulation calls the planner refuses to flatten.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LifecycleCall {
    /// `Leader::defeat(defeat_type, arg, instant)` `0x006ECB00` on `leaders[who]`.
    LeaderDefeat {
        who: u8,
        defeat_type: i32,
        arg: i32,
        instant: i32,
    },
    /// `Leader::action_declare(whom, treaty, no_payment, over)` `0x006DAB50` on
    /// `leaders[who]`.  Command row 38's open tail.
    LeaderActionDeclare {
        who: u8,
        whom: i32,
        treaty: i32,
        no_payment: i32,
        over: i32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LifecycleEffect {
    SetPlayerFlags { play: u8, flags: u16 },
    SetPlayerTeam { play: u8, team: i8 },
    SetLeaderFlags { who: u8, leader_flags: i32 },
    SetLeaderMultiDiff { who: u8, multi_diff: i32 },
    SetTeamStyle(u8),
    SetPlaying(i32),
    SetSemaphoreBit { bit: u32, on: bool },
    SetSemaphoreFlags(i32),
    SetDropWindowOpen(bool),
    Presentation(LifecyclePresentation),
    Call(LifecycleCall),
}

/// A branch that is exact up to a callee this lane did not recover.  A boundary authorizes
/// no mutation: [`LifecycleReceipt::validates`] refuses to validate it as applied.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LifecycleBoundary {
    /// `Player::leave_game`'s `GameInfo::elimination == 1` arm calls
    /// `LeaderData::find_capital(&a, &b, -1, -1)` `0x006EB930` and passes its second out
    /// parameter as `Leader::defeat`'s `arg`.
    FindCapitalForDefeat { who: u8 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LifecycleError {
    /// `play` is outside `GameInfo::player[8]`.
    PlayOutOfRange { play: i32 },
    /// The image breaks the retail setup invariant `players[i].play == i`.
    PlaySlotMismatch { slot: usize, play: u8 },
    /// `Player::who` selects a leader slot outside `leaders[8]`.
    WhoOutOfRange { play: u8, who: u8 },
    /// The image breaks the retail setup invariant `leaders[i].who == i`, which every
    /// diplomacy-facing body in this family relies on.
    LeaderSlotMismatch { slot: usize, who: i32 },
    /// `Console::who` is a leader index the mutual-ally test would read out of range.
    ConsoleWhoOutOfRange { console_who: i32 },
}

/// One planned transaction: the after-image plus its instruction-ordered effect list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LifecyclePlan {
    pub image: LifecycleImage,
    pub effects: Vec<LifecycleEffect>,
    /// `Some` when the plan reached an unrecovered callee.  Effects recorded before it are
    /// observations, not authorized mutations.
    pub boundary: Option<LifecycleBoundary>,
}

impl LifecyclePlan {
    fn start(image: &LifecycleImage) -> Self {
        Self {
            image: image.clone(),
            effects: Vec::new(),
            boundary: None,
        }
    }

    fn present(&mut self, p: LifecyclePresentation) {
        self.effects.push(LifecycleEffect::Presentation(p));
    }

    fn call(&mut self, c: LifecycleCall) {
        self.effects.push(LifecycleEffect::Call(c));
    }

    fn set_player_flags(&mut self, play: u8, flags: u16) {
        self.image.players[play as usize].flags = flags;
        self.effects
            .push(LifecycleEffect::SetPlayerFlags { play, flags });
    }

    fn set_sem(&mut self, bit: u32, on: bool) {
        self.image.set_semaphore_bit(bit, on);
        self.effects
            .push(LifecycleEffect::SetSemaphoreBit { bit, on });
    }

    fn set_sem_flags(&mut self, value: i32) {
        self.image.semaphore_flags = value;
        self.effects.push(LifecycleEffect::SetSemaphoreFlags(value));
    }
}

// ---------------------------------------------------------------------------
// Shared validation
// ---------------------------------------------------------------------------

fn validate(image: &LifecycleImage, play: i32) -> Result<u8, LifecycleError> {
    if !(0..PLAYER_SLOTS as i32).contains(&play) {
        return Err(LifecycleError::PlayOutOfRange { play });
    }
    for (slot, row) in image.players.iter().enumerate() {
        if row.play as usize != slot {
            return Err(LifecycleError::PlaySlotMismatch {
                slot,
                play: row.play,
            });
        }
    }
    for (slot, row) in image.leaders.iter().enumerate() {
        if row.who != slot as i32 {
            return Err(LifecycleError::LeaderSlotMismatch { slot, who: row.who });
        }
    }
    let play = play as u8;
    let who = image.players[play as usize].who;
    if who as usize >= LEADER_SLOTS {
        return Err(LifecycleError::WhoOutOfRange { play, who });
    }
    Ok(play)
}

// ---------------------------------------------------------------------------
// `Player::leave_game(int)` `0x006EE010`
// ---------------------------------------------------------------------------

/// The co-tenant scan at `0x006EE050..0x006EE089`.  Returns the first slot that still holds
/// this player's leader, which makes `leave_game` return with **no** defeat.
///
/// Instruction order is load-bearing and is preserved: `flags & 4`, `flags & 1`,
/// `flags & 0x100`, `flags & 0xD0`, then `slot != play`, then the byte compare of
/// `players[slot].who` against `players[play].who`.
pub fn leave_game_cotenant(image: &LifecycleImage, play: u8) -> Option<usize> {
    let who = image.players[play as usize].who;
    for (slot, row) in image.players.iter().enumerate() {
        let f = row.flags;
        if f & PLAYER_LEAVE_SCAN_REQUIRED == 0 {
            continue;
        }
        if f & PLAYER_PRESENT == 0 {
            continue;
        }
        if f & PLAYER_SCAN_EXCLUDE_HIGH != 0 {
            continue;
        }
        if f & PLAYER_SCAN_EXCLUDE_MASK != 0 {
            continue;
        }
        if slot == play as usize {
            continue;
        }
        if row.who == who {
            return Some(slot);
        }
    }
    None
}

/// `Player::leave_game(int reason)` `0x006EE010`, appended to `plan`.
///
/// 1. `players[play].flags |= 0x10`.
/// 2. only under semaphore bit 2, run the co-tenant scan; a hit returns with no defeat.
/// 3. `leaders[who]`: when `(leader_flags & 3) == 3` **and** `lost_capital_timer != 0`,
///    the leave reason is discarded and retail defeats as `DEFEAT_CAPITAL`; under
///    `GameInfo::elimination == 1` the `arg` comes from `LeaderData::find_capital`, else it
///    is `LeaderData::who`.
/// 4. otherwise `Leader::defeat(reason, -1, 0)`.
fn append_leave_game(plan: &mut LifecyclePlan, play: u8, reason: i32) {
    let flags = plan.image.players[play as usize].flags | PLAYER_LEFT;
    plan.set_player_flags(play, flags);

    if plan.image.sem(SEM_NET_OR_RECORDING) && leave_game_cotenant(&plan.image, play).is_some() {
        return;
    }

    let who = plan.image.players[play as usize].who;
    let leader = plan.image.leaders[who as usize];
    let capital_arm = leader.leader_flags & LEADER_VALID_ACTIVE == LEADER_VALID_ACTIVE
        && leader.lost_capital_timer != 0;

    if !capital_arm {
        plan.call(LifecycleCall::LeaderDefeat {
            who,
            defeat_type: reason,
            arg: -1,
            instant: 0,
        });
        return;
    }

    if plan.image.elimination == 1 {
        plan.boundary = Some(LifecycleBoundary::FindCapitalForDefeat { who });
        return;
    }

    plan.call(LifecycleCall::LeaderDefeat {
        who,
        defeat_type: DEFEAT_TYPE_CAPITAL,
        arg: leader.who,
        instant: 0,
    });
}

// ---------------------------------------------------------------------------
// The shared remote-departure envelope
// ---------------------------------------------------------------------------

/// The sound selection at `0x006EDDF3..0x006EDE33`, shared verbatim by `Player::resign` and
/// `Player::drop`.
///
/// `SOUND_FRIENDLY_LEFT` when `Console::who` *is* the departing leader's `LeaderData::who`,
/// or when the two are **mutually** allied.  The forward test reads
/// `leaders[who].diplos[console_who]`; the reverse reads `leaders[console_who].diplos[who]`
/// through `GameAccessConst::leadersc` `0x00C061E0`, indexed by `LeaderData::who` rather
/// than by the slot.
pub fn departure_sound(image: &LifecycleImage, who: u8) -> Result<i32, LifecycleError> {
    let leader_who = image.leaders[who as usize].who;
    let console_who = image.console_who;
    if console_who == leader_who {
        return Ok(SOUND_FRIENDLY_LEFT);
    }
    if !(0..LEADER_SLOTS as i32).contains(&console_who) {
        return Err(LifecycleError::ConsoleWhoOutOfRange { console_who });
    }
    let forward = image.leaders[who as usize].diplos[console_who as usize];
    if forward != DIPLO_ALLY {
        return Ok(SOUND_OTHER_LEFT);
    }
    let reverse = image.leaders[console_who as usize].diplos[leader_who as usize];
    Ok(if reverse == DIPLO_ALLY {
        SOUND_FRIENDLY_LEFT
    } else {
        SOUND_OTHER_LEFT
    })
}

/// `0x006EDCFE..0x006EDE33` in `Player::resign` and `0x006EDEB9..0x006EDFEE` in
/// `Player::drop`: build the departure name, post the
/// message, optionally raise the interface notice, and select the category.  Retail suppresses
/// the notice under semaphore bit 6.
fn append_remote_departure(
    plan: &mut LifecyclePlan,
    play: u8,
    text_record: u32,
) -> Result<i32, LifecycleError> {
    plan.present(LifecyclePresentation::PlayerName { play });
    plan.present(LifecyclePresentation::Message { text_record });
    if !plan.image.sem(SEM_GAME_OVER) {
        plan.present(LifecyclePresentation::Notice {
            internal_string_record: INTERNAL_STRING_NOTICE,
        });
    }
    departure_sound(&plan.image, plan.image.players[play as usize].who)
}

// ---------------------------------------------------------------------------
// `Player::resign(int)` `0x006EDCB0`
// ---------------------------------------------------------------------------

/// `Player::resign(int from_quit)` `0x006EDCB0`.
///
/// The local arm skips the whole message/notice/diplomacy block and selects its category
/// purely from `from_quit`.  Both arms then play the sound, run `leave_game(6)` and — only
/// for the local player — set semaphore bit 15 and zero the semaphore flags dword.
pub fn plan_resign(
    image: &LifecycleImage,
    play: i32,
    from_quit: i32,
) -> Result<LifecyclePlan, LifecycleError> {
    let play = validate(image, play)?;
    let mut plan = LifecyclePlan::start(image);
    append_resign(&mut plan, play, from_quit)?;
    Ok(plan)
}

fn append_resign(plan: &mut LifecyclePlan, play: u8, from_quit: i32) -> Result<(), LifecycleError> {
    let flags = plan.image.players[play as usize].flags | PLAYER_RESIGNED;
    plan.set_player_flags(play, flags);

    let local = i32::from(play) == plan.image.console_play;
    let category = if local {
        if from_quit == 0 {
            SOUND_LOCAL_RESIGN
        } else {
            SOUND_LOCAL_QUIT
        }
    } else {
        append_remote_departure(plan, play, TEXT_RESIGNED)?
    };
    plan.present(LifecyclePresentation::Sound { category });

    append_leave_game(plan, play, LEAVE_REASON_RESIGN);
    if plan.boundary.is_some() {
        return Ok(());
    }

    if local {
        plan.set_sem(SEM_LOCAL_LEFT, true);
        plan.set_sem_flags(0);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// `Player::drop(void)` `0x006EDE80`
// ---------------------------------------------------------------------------

/// `Player::drop()` `0x006EDE80`.  Same envelope as `Player::resign`'s remote arm, but it
/// never sets `PLAYER_RESIGNED`, never touches the semaphore, and leaves with reason 7.
pub fn plan_drop(image: &LifecycleImage, play: i32) -> Result<LifecyclePlan, LifecycleError> {
    let play = validate(image, play)?;
    let mut plan = LifecyclePlan::start(image);
    append_drop(&mut plan, play)?;
    Ok(plan)
}

fn append_drop(plan: &mut LifecyclePlan, play: u8) -> Result<(), LifecycleError> {
    let category = if i32::from(play) == plan.image.console_play {
        SOUND_LOCAL_QUIT
    } else {
        append_remote_departure(plan, play, TEXT_DROPPED)?
    };
    plan.present(LifecyclePresentation::Sound { category });
    append_leave_game(plan, play, LEAVE_REASON_DROP);
    Ok(())
}

// ---------------------------------------------------------------------------
// `Player::quit(int)` `0x006EDC00`
// ---------------------------------------------------------------------------

/// `Player::quit(int force_stop)` `0x006EDC00`.
///
/// Retail samples semaphore bit 15 **before** `Player::resign(1)` runs — the resign body may
/// set it — and then restores or clears it. `Game::playing` is cleared unless the drop-control
/// semaphore bit is set and `force_stop` is zero.  A non-local player quitting while
/// semaphore bit 2 is set returns before either.
pub fn plan_quit(
    image: &LifecycleImage,
    play: i32,
    force_stop: i32,
) -> Result<LifecyclePlan, LifecycleError> {
    let play = validate(image, play)?;
    let mut plan = LifecyclePlan::start(image);
    append_quit(&mut plan, play, force_stop)?;
    Ok(plan)
}

fn append_quit(plan: &mut LifecyclePlan, play: u8, force_stop: i32) -> Result<(), LifecycleError> {
    let saved_local_left = plan.image.sem(SEM_LOCAL_LEFT);

    append_resign(plan, play, 1)?;
    if plan.boundary.is_some() {
        return Ok(());
    }

    if saved_local_left {
        plan.set_sem(SEM_LOCAL_LEFT, true);
        plan.set_sem_flags(0);
    } else {
        plan.set_sem(SEM_LOCAL_LEFT, false);
        if plan.image.semaphore_flags == 0 {
            plan.set_sem_flags(2);
        }
    }

    let local = i32::from(play) == plan.image.console_play;
    if !local && plan.image.sem(SEM_NET_OR_RECORDING) {
        return Ok(());
    }

    if !plan.image.sem(SEM_DROP_CONTROL) || force_stop != 0 {
        plan.image.playing = 0;
        plan.effects.push(LifecycleEffect::SetPlaying(0));
    }
    plan.present(LifecyclePresentation::Report {
        internal_string_record: INTERNAL_STRING_QUIT_REPORT,
    });
    Ok(())
}

// ---------------------------------------------------------------------------
// `DropControl::process_drop(int, int)` `0x00959500`
// ---------------------------------------------------------------------------

/// `DropControl::process_drop`'s state-3 co-tenant scan at `0x00959598..0x009595C8`.
///
/// Identical to [`leave_game_cotenant`] **except** that it does not test
/// [`PLAYER_LEAVE_SCAN_REQUIRED`] and it excludes the subject slot first.  That difference is
/// real: a player row that has already left (bit 4 cleared by an earlier state-3 drop) still
/// blocks a second player's leader from being handed to the AI.
pub fn process_drop_cotenant(image: &LifecycleImage, play: u8) -> Option<usize> {
    let who = image.players[play as usize].who;
    for (slot, row) in image.players.iter().enumerate() {
        if slot == play as usize {
            continue;
        }
        let f = row.flags;
        if f & PLAYER_PRESENT == 0 {
            continue;
        }
        if f & PLAYER_SCAN_EXCLUDE_HIGH != 0 {
            continue;
        }
        if f & PLAYER_SCAN_EXCLUDE_MASK != 0 {
            continue;
        }
        if row.who == who {
            return Some(slot);
        }
    }
    None
}

/// The all-pairs war fan-out at `0x00959731..0x0095978B` (state 1) and
/// `0x0095981A..0x0095987A` (state 2), which are byte-identical loops.
///
/// For every ordered pair of [`LEADER_VALID`] leaders `k < m`, retail calls
/// `leaders[k].action_declare(m, 0, 1, 1)`.  `no_payment = 1` skips both
/// `LeaderData::afford_dow` `0x006D5CE0` and `Leader::pay_dow` `0x006D2B10`
/// (`0x006DABC7` and `0x006DACEC`); `over = 1` skips the no-rush stamp gate.  The call is
/// still not flattened here: its `Leader::set_diplo` tail, team fan-out and local envelope
/// are command row 38's open work.
fn append_drop_war_fanout(plan: &mut LifecyclePlan) {
    for k in 0..LEADER_SLOTS {
        if plan.image.leaders[k].leader_flags & LEADER_VALID == 0 {
            continue;
        }
        if k + 1 >= LEADER_SLOTS {
            continue;
        }
        for m in (k + 1)..LEADER_SLOTS {
            if plan.image.leaders[m].leader_flags & LEADER_VALID == 0 {
                continue;
            }
            plan.call(LifecycleCall::LeaderActionDeclare {
                who: k as u8,
                whom: m as i32,
                treaty: 0,
                no_payment: 1,
                over: 1,
            });
        }
    }
}

/// States 1 and 2 share a body except for the `GameInfo::team_style` they write.
fn append_drop_dissolve_teams(plan: &mut LifecyclePlan, team_style: u8) {
    plan.image.team_style = team_style;
    plan.effects.push(LifecycleEffect::SetTeamStyle(team_style));
    for slot in 0..PLAYER_SLOTS {
        if plan.image.players[slot].flags & PLAYER_PRESENT == 0 {
            continue;
        }
        plan.image.players[slot].team = PLAYER_TEAM_AUTO;
        plan.effects.push(LifecycleEffect::SetPlayerTeam {
            play: slot as u8,
            team: PLAYER_TEAM_AUTO,
        });
    }
    append_drop_war_fanout(plan);
}

/// `DropControl::process_drop(int play, int state)` `0x00959500`.
///
/// The prologue opens retail's drop-vote window exactly once per `DropControl` instance.
/// State 3 hands the dropped player's leader to the AI and returns without `Player::drop`;
/// states 1 and 2 dissolve every team, declare war between all remaining leader pairs and
/// then drop the player; any other state drops the player directly.
pub fn plan_process_drop(
    image: &LifecycleImage,
    play: i32,
    state: i32,
) -> Result<LifecyclePlan, LifecycleError> {
    let play = validate(image, play)?;
    let mut plan = LifecyclePlan::start(image);

    if !plan.image.drop_window_open {
        plan.present(LifecyclePresentation::DropVoteWindow {
            internal_string_record: INTERNAL_STRING_DROP_LOG,
        });
        plan.image.drop_window_open = true;
        plan.effects.push(LifecycleEffect::SetDropWindowOpen(true));
    }

    if state == 3 {
        if plan.image.players[play as usize].flags & PLAYER_DROP_GATE != 0 {
            return Ok(plan);
        }
        if process_drop_cotenant(&plan.image, play).is_some() {
            return Ok(plan);
        }
        plan.present(LifecyclePresentation::PlayerName { play });
        plan.present(LifecyclePresentation::Message {
            text_record: TEXT_DROP_VOTE,
        });
        let flags = plan.image.players[play as usize].flags & PLAYER_DROP_RETAIN_MASK;
        plan.set_player_flags(play, flags);

        let who = plan.image.players[play as usize].who;
        let leader_flags = plan.image.leaders[who as usize].leader_flags & !LEADER_HUMAN;
        plan.image.leaders[who as usize].leader_flags = leader_flags;
        plan.effects
            .push(LifecycleEffect::SetLeaderFlags { who, leader_flags });
        plan.image.leaders[who as usize].multi_diff = DROPPED_LEADER_MULTI_DIFF;
        plan.effects.push(LifecycleEffect::SetLeaderMultiDiff {
            who,
            multi_diff: DROPPED_LEADER_MULTI_DIFF,
        });
        return Ok(plan);
    }

    match state {
        1 => append_drop_dissolve_teams(&mut plan, 0),
        2 => append_drop_dissolve_teams(&mut plan, 1),
        _ => {}
    }
    append_drop(&mut plan, play)?;
    Ok(plan)
}

// ---------------------------------------------------------------------------
// Receipt
// ---------------------------------------------------------------------------

/// The five recovered entry points, as one request type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LifecycleRequest {
    /// `Player::resign(from_quit)`.
    Resign { play: i32, from_quit: i32 },
    /// `Player::quit(force_stop)`.
    Quit { play: i32, force_stop: i32 },
    /// `Player::drop()`.
    Drop { play: i32 },
    /// `DropControl::process_drop(play, state)`.
    ProcessDrop { play: i32, state: i32 },
}

pub fn plan_lifecycle(
    image: &LifecycleImage,
    request: LifecycleRequest,
) -> Result<LifecyclePlan, LifecycleError> {
    match request {
        LifecycleRequest::Resign { play, from_quit } => plan_resign(image, play, from_quit),
        LifecycleRequest::Quit { play, force_stop } => plan_quit(image, play, force_stop),
        LifecycleRequest::Drop { play } => plan_drop(image, play),
        LifecycleRequest::ProcessDrop { play, state } => plan_process_drop(image, play, state),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LifecycleStatus {
    Applied,
    Unavailable,
}

/// Atomic receipt.  An `Applied` receipt is valid only when the host's supplied before-image
/// replans to exactly the observed plan, the plan reached no boundary, and every
/// [`LifecycleCall`] it contains was acknowledged in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LifecycleReceipt {
    pub request: LifecycleRequest,
    pub status: LifecycleStatus,
    pub before: Option<LifecycleImage>,
    pub plan: Option<LifecyclePlan>,
    /// The `LifecycleCall`s the host executed, in the order it executed them.
    pub executed_calls: Vec<LifecycleCall>,
}

impl LifecycleReceipt {
    pub fn unavailable(request: LifecycleRequest) -> Self {
        Self {
            request,
            status: LifecycleStatus::Unavailable,
            before: None,
            plan: None,
            executed_calls: Vec::new(),
        }
    }

    /// Every [`LifecycleCall`] in the plan, in instruction order.
    pub fn required_calls(plan: &LifecyclePlan) -> Vec<LifecycleCall> {
        plan.effects
            .iter()
            .filter_map(|e| match e {
                LifecycleEffect::Call(c) => Some(*c),
                _ => None,
            })
            .collect()
    }

    pub fn validates(&self, expected: LifecycleRequest) -> bool {
        if self.request != expected {
            return false;
        }
        match self.status {
            LifecycleStatus::Unavailable => {
                self.before.is_none() && self.plan.is_none() && self.executed_calls.is_empty()
            }
            LifecycleStatus::Applied => {
                let (Some(before), Some(observed)) = (self.before.as_ref(), self.plan.as_ref())
                else {
                    return false;
                };
                if observed.boundary.is_some() {
                    return false;
                }
                match plan_lifecycle(before, expected) {
                    Ok(plan) => {
                        plan == *observed && self.executed_calls == Self::required_calls(observed)
                    }
                    Err(_) => false,
                }
            }
        }
    }
}
