//! `CrossplayProxy.dll`'s binary interface — **generated, do not edit by hand**.
//!
//! Regenerate with:
//!
//! ```sh
//! python3 crates/don-crossplay/gen/gen_abi.py
//! uv run --with capstone python3 crates/don-crossplay/gen/gen_abi.py --check-ret
//! ```
//!
//! Everything here is layout, not behaviour, and every number was read out of a
//! shipped private PDB and cross-checked against the shipped DLL. Nothing in
//! this crate implements a method.
//!
//! # The one thing that will bite you: there are TWO `ICrossPlayService`s
//!
//! `CrossplayProxy.pdb` declares both:
//!
//! * `CrossplayProxy::ICrossPlayService` — 58 slots, byte offsets
//!   `0x000..=0x0e4`. **This is the shipped vtable.**
//!   `CrossplayProxy::CrossPlayService` (the object `Service()` returns) derives
//!   from it at offset 0, and `rise.pdb` and `CrossplayNetLib.pdb` both declare
//!   their `Crossplay::ICrossPlayService` with a slot-for-slot **identical**
//!   layout — same names, same signatures, same offsets, zero differences.
//! * `Crossplay::ICrossPlayService` — 96 slots, offsets
//!   `0x000..=0x17c`, from a *newer vendor header* this build does
//!   not use. Its slot 0 is a destructor, slot 1 is `SetNew`, and `Init` is at
//!   slot 3. Every single slot disagrees with the shipped table. (`pdb-extract`
//!   reports 97 virtual *methods* for it because slot 0 carries both
//!   `~ICrossPlayService` and `__vecDelDtor`; the table is 96 pointers.)
//!
//! Using the 96-slot record as "the ICrossPlayService vtable" would
//! produce an ABI that is wrong in every slot while compiling perfectly. It is
//! emitted below as [`VendorICrossPlayServiceVtable`] purely so the trap is
//! recorded in checked code; retail never calls it.
//!
//! # Calling convention
//!
//! Every virtual is an MSVC x86 C++ member function: `__thiscall` — `this` in
//! `ECX`, remaining arguments pushed right-to-left, **callee** cleans the stack.
//! Rust spells that `extern "thiscall"`, which only exists on x86, so the
//! vtables are emitted with `extern "thiscall"` on `target_arch = "x86"` and
//! with `extern "C"` elsewhere. The slot-order assertions are written in units
//! of `size_of::<*const ()>()` and therefore hold on both; the exact byte-offset
//! assertions are additionally checked under `target_arch = "x86"`.
//!
//! A class returned by value becomes a hidden first stack argument (the caller's
//! return slot), which the callee also returns in `EAX`; that is the same shape
//! `crates/netsys-shim/src/abi.rs` uses for `NetPlayer::get_id`.
//!
//! # Provenance
//!
//! * `ron-bin/sbl/CrossplayProxy.pdb` GUID `{d36f6bf3-9aa0-4388-895e-f3e8f776e191}`,
//!   SHA-256 `b2c1cb8055ea2e9bfc6e7c0bd2f884ae3c322fd3220dc681ba87c6242bcd5c76`
//! * `ron-bin/dll/CrossplayProxy.dll` SHA-256 `ec7a6c18f03c6d7463fc3d36b7f663ec23c1477225d46b202b29756ef7554abc`,
//!   PE32/i386, image base `0x10000000`, 4 exports
//! * `CrossplayProxy::CrossPlayService::\`vftable'` at RVA `0x9adc4`
//! * cross-checked against `ron-bin/sbl/rise.pdb` and
//!   `ron-bin/sbl/CrossplayNetLib.pdb`
//!
//! Checks the generator ran, and their results, on the run that produced
//! this file:
//!
//! * cross-PDB vtable identity: CrossplayProxy::ICrossPlayService (58 slots) vs rise.pdb Crossplay::ICrossPlayService (58) vs CrossplayNetLib.pdb Crossplay::ICrossPlayService (58): IDENTICAL
//! * shipped .rdata vtable at RVA 0x9adc4: 58/58 interface slots resolve to CrossplayProxy::CrossPlayService::<declared name>; slot 58 is the deleting destructor (yes); slot 59 is outside .text (yes) so the emitted table is 236 bytes
//! * DTO layout agreement: 15/15 identical wherever declared; 3 declared in only one PDB: JoinLobbyResultDTO(rise), MatchFoundDTO(CrossplayProxy), MatchMakingEnqueuedDTO(CrossplayProxy)
//! * stack-cleanup check: SKIPPED (capstone not importable)
//! * slots emitted as opaque because a by-value parameter type is only forward-declared in this PDB: VendorICrossPlayService::SetNew (+4), VendorICrossPlayService::SetDelete (+8), VendorICrossPlayService::P2PGetAllConnectedPlayers (+308)

#![allow(non_snake_case)]
#![allow(non_camel_case_types)]

/// Pointer width of the target. The shipped ABI is x86, where this is 4.
pub const PTR_SIZE: usize = core::mem::size_of::<*const core::ffi::c_void>();


/// MSVC x86 `std::basic_string<wchar_t>` — 24 bytes. Same object
/// `crates/netsys-shim/src/abi.rs` calls `MsvcWstring`. **[measured]**
#[repr(C, align(4))]
#[derive(Clone, Copy)]
pub struct MsvcWstring {
    pub raw: [u8; 24],
}
const _: () = {
    assert!(core::mem::size_of::<MsvcWstring>() == 24);
    // Load-bearing, not decoration: `ICrossplayLogger::Log` takes one of these
    // **by value**, and on `i686-pc-windows-msvc` rustc passes an aggregate
    // aligned above 4 as a pointer instead of on the stack. See `MsvcFunction`.
    assert!(core::mem::align_of::<MsvcWstring>() == 4);
};

/// MSVC x86 `std::basic_string<char>` — 24 bytes. **[measured]**
#[repr(C, align(4))]
#[derive(Clone, Copy)]
pub struct MsvcString {
    pub raw: [u8; 24],
}
const _: () = {
    assert!(core::mem::size_of::<MsvcString>() == 24);
    assert!(core::mem::align_of::<MsvcString>() == 4);
};

/// MSVC x86 `std::function<...>` — 40 bytes for every one of the 191
/// specialisations defined in `CrossplayProxy.pdb`. The target pointer lives at
/// `+0x24`; `CrossPlayService::SetServiceErrorCallback` (`0x100145c0`) reads
/// exactly that offset off its argument. **[measured]**
///
/// `target` is a **32-bit** guest pointer, spelled `u32` so the layout is the
/// x86 one on every host this crate is checked on. Never dereference it here.
///
/// # The alignment is part of the calling convention
///
/// `align(4)` is not decoration. On `i686-pc-windows-msvc` rustc passes an
/// aggregate whose alignment exceeds 4 **indirectly, as a pointer**, and one
/// with alignment 4 **on the stack by value**. MSVC does the same, which is why
/// `ICrossplayLogger::Register(LogLevel, std::function)` pops 44 bytes and not
/// 8. Declaring this type over-aligned silently changes every by-value slot on
/// this interface into a different ABI, compiles clean, passes every layout
/// assertion, and destroys the caller's stack on the first call.
///
/// Measured both ways: `crossplay-load-smoke` faulted on a null read at
/// `+0x24` with the emitted `Register` ending in `ret 8`, and reaches the
/// callback with `ret 44` after this line was corrected. **[measured]**
#[repr(C, align(4))]
#[derive(Clone, Copy)]
pub struct MsvcFunction {
    pub storage: [u8; 36],
    pub target: u32,
}
const _: () = {
    assert!(core::mem::size_of::<MsvcFunction>() == 40);
    assert!(core::mem::offset_of!(MsvcFunction, target) == 0x24);
    // The one that decides whether 24 by-value callback arguments are pushed or
    // passed as a pointer. Read the note above before touching it.
    assert!(core::mem::align_of::<MsvcFunction>() == 4);
};

/// MSVC x86 `std::vector<T>` — 12 bytes: three 32-bit guest pointers
/// (`first`, `last`, `end_of_storage`). **[measured]**
#[repr(C, align(4))]
#[derive(Clone, Copy)]
pub struct MsvcVector {
    pub first: u32,
    pub last: u32,
    pub end: u32,
}
const _: () = {
    assert!(core::mem::size_of::<MsvcVector>() == 12);
    assert!(core::mem::align_of::<MsvcVector>() == 4);
};

/// An opaque interface object: one vtable pointer, 4 bytes.
/// `Crossplay::P2P::ICrossplayPlayer` and `Crossplay::ICrossPlayService` are
/// both `size=4`. **[measured]**
#[repr(C)]
pub struct ICrossPlayService {
    pub vftable: *const ICrossPlayServiceVtable,
}
const _: () = assert!(core::mem::size_of::<ICrossPlayService>() == PTR_SIZE);

/// `Crossplay::P2P::ICrossplayPlayer`, the per-peer handle. **[measured]**
#[repr(C)]
pub struct ICrossplayPlayer {
    pub vftable: *const ICrossplayPlayerVtable,
}
const _: () = assert!(core::mem::size_of::<ICrossplayPlayer>() == PTR_SIZE);

/// `Crossplay::Logging::ICrossplayLogger`. **[measured]**
#[repr(C)]
pub struct ICrossplayLogger {
    pub vftable: *const ICrossplayLoggerVtable,
}
const _: () = assert!(core::mem::size_of::<ICrossplayLogger>() == PTR_SIZE);

/// `CrossplayProxy::INetworkClient`, the Party-facing side of the proxy.
/// **[measured]**
#[repr(C)]
pub struct INetworkClient {
    pub vftable: *const INetworkClientVtable,
}
const _: () = assert!(core::mem::size_of::<INetworkClient>() == PTR_SIZE);

// ---------------------------------------------------------------------------
// Enumerations. All are 4-byte `int` in every PDB that declares them.
// ---------------------------------------------------------------------------

/// `Crossplay::Lobby::Visibility`. **[measured]**
pub type Visibility = i32;
pub const VISIBILITY_PUBLIC: Visibility = 0;
pub const VISIBILITY_PRIVATE: Visibility = 1;

/// `Crossplay::Lobby::DTO::ELobbyProximity`. **[measured]**
pub type LobbyProximity = i32;
pub const LOBBY_PROXIMITY_DEFAULT: LobbyProximity = 0;
pub const LOBBY_PROXIMITY_CLOSE: LobbyProximity = 1;
pub const LOBBY_PROXIMITY_NEAR: LobbyProximity = 2;
pub const LOBBY_PROXIMITY_FAR: LobbyProximity = 3;
pub const LOBBY_PROXIMITY_WORLDWIDE: LobbyProximity = 4;

/// `Crossplay::User::SessionStatus`. **[measured]**
pub type SessionStatus = i32;
pub const SESSION_STATUS_STOPPED: SessionStatus = 0;
pub const SESSION_STATUS_STARTING: SessionStatus = 1;
pub const SESSION_STATUS_STARTED: SessionStatus = 2;
pub const SESSION_STATUS_STOPPING: SessionStatus = 3;

/// `Crossplay::CrossplayStatus`. **[measured]**
pub type CrossplayStatus = i32;
pub const CROSSPLAY_STATUS_ENABLED: CrossplayStatus = 0;
pub const CROSSPLAY_STATUS_TURNONLY: CrossplayStatus = 1;
pub const CROSSPLAY_STATUS_DISABLED: CrossplayStatus = 2;

/// `Crossplay::Logging::LogLevel`. **[measured]**
pub type LogLevel = i32;
pub const LOG_LEVEL_NONE: LogLevel = 0;
pub const LOG_LEVEL_DEBUG: LogLevel = 1;
pub const LOG_LEVEL_INFO: LogLevel = 2;
pub const LOG_LEVEL_WARN: LogLevel = 4;
pub const LOG_LEVEL_ERROR: LogLevel = 8;
pub const LOG_LEVEL_CRITICAL: LogLevel = 16;

// ---------------------------------------------------------------------------
// DTOs. Offsets and sizes are the PDB's; every one is asserted below it.
// ---------------------------------------------------------------------------

/// Opaque 8-byte MSVC aggregate: `std::map<std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >,int,std::l...`. **[measured]**
#[repr(C, align(4))]
#[derive(Clone, Copy)]
pub struct MsvcMap8 {
    pub raw: [u8; 8],
}
const _: () = assert!(core::mem::size_of::<MsvcMap8>() == 8);

/// Opaque 32-byte MSVC aggregate: `std::unordered_map<std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >,...`. **[measured]**
#[repr(C, align(4))]
#[derive(Clone, Copy)]
pub struct MsvcUnorderedMap32 {
    pub raw: [u8; 32],
}
const _: () = assert!(core::mem::size_of::<MsvcUnorderedMap32>() == 32);

/// `Crossplay::User::DTO::UserDTO` — 96 bytes. **[measured]**
#[repr(C)]
#[derive(Clone, Copy)]
pub struct UserDTO {
    /// +0 `_userId` : `std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >`
    pub user_id: MsvcWstring,
    /// +24 `_userName` : `std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >`
    pub user_name: MsvcWstring,
    /// +48 `_platform` : `std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >`
    pub platform: MsvcWstring,
    /// +72 `_platformAccountId` : `std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >`
    pub platform_account_id: MsvcWstring,
}
const _: () = {
    assert!(core::mem::size_of::<UserDTO>() == 96);
    assert!(core::mem::offset_of!(UserDTO, user_id) == 0);
    assert!(core::mem::offset_of!(UserDTO, user_name) == 24);
    assert!(core::mem::offset_of!(UserDTO, platform) == 48);
    assert!(core::mem::offset_of!(UserDTO, platform_account_id) == 72);
};

/// `Crossplay::Lobby::DTO::LobbyMemberDTO` — 96 bytes. **[measured]**
#[repr(C)]
#[derive(Clone, Copy)]
pub struct LobbyMemberDTO {
    /// base class `Crossplay::User::DTO::UserDTO` at +0.
    pub base: UserDTO,
}
const _: () = {
    assert!(core::mem::size_of::<LobbyMemberDTO>() == 96);
    assert!(core::mem::offset_of!(LobbyMemberDTO, base) == 0);
};

/// `Crossplay::Lobby::DTO::TurnServerDTO` — 72 bytes. **[measured]**
#[repr(C)]
#[derive(Clone, Copy)]
pub struct TurnServerDTO {
    /// +0 `_address` : `std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >`
    pub address: MsvcWstring,
    /// +24 `_userName` : `std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >`
    pub user_name: MsvcWstring,
    /// +48 `_password` : `std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >`
    pub password: MsvcWstring,
}
const _: () = {
    assert!(core::mem::size_of::<TurnServerDTO>() == 72);
    assert!(core::mem::offset_of!(TurnServerDTO, address) == 0);
    assert!(core::mem::offset_of!(TurnServerDTO, user_name) == 24);
    assert!(core::mem::offset_of!(TurnServerDTO, password) == 48);
};

/// `Crossplay::Lobby::DTO::LobbyDTO` — 208 bytes. **[measured]**
#[repr(C)]
#[derive(Clone, Copy)]
pub struct LobbyDTO {
    /// +0 `_id` : `std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >`
    pub id: MsvcWstring,
    /// +24 `_ownerUserId` : `std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >`
    pub owner_user_id: MsvcWstring,
    /// +48 `_sessionReference` : `std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >`
    pub session_reference: MsvcWstring,
    /// +72 `_maxMembers` : `int`
    pub max_members: i32,
    /// +76 `_availableSlots` : `int`
    pub available_slots: i32,
    /// +80 `_botCount` : `int`
    pub bot_count: i32,
    /// +84 `_attributeVersion` : `int`
    pub attribute_version: i32,
    /// +88 `_visibitily` : `Crossplay::Lobby::Visibility`
    pub visibitily: Visibility,
    /// +92 `_members` : `std::vector<Crossplay::Lobby::DTO::LobbyMemberDTO,std::allocator<Crossplay::Lobby::DTO::Lobby...`
    pub members: MsvcVector,
    /// +104 `_attributes` : `std::unordered_map<std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t...`
    pub attributes: MsvcUnorderedMap32,
    /// +136 `_turnServer` : `Crossplay::Lobby::DTO::TurnServerDTO`
    pub turn_server: TurnServerDTO,
}
const _: () = {
    assert!(core::mem::size_of::<LobbyDTO>() == 208);
    assert!(core::mem::offset_of!(LobbyDTO, id) == 0);
    assert!(core::mem::offset_of!(LobbyDTO, owner_user_id) == 24);
    assert!(core::mem::offset_of!(LobbyDTO, session_reference) == 48);
    assert!(core::mem::offset_of!(LobbyDTO, max_members) == 72);
    assert!(core::mem::offset_of!(LobbyDTO, available_slots) == 76);
    assert!(core::mem::offset_of!(LobbyDTO, bot_count) == 80);
    assert!(core::mem::offset_of!(LobbyDTO, attribute_version) == 84);
    assert!(core::mem::offset_of!(LobbyDTO, visibitily) == 88);
    assert!(core::mem::offset_of!(LobbyDTO, members) == 92);
    assert!(core::mem::offset_of!(LobbyDTO, attributes) == 104);
    assert!(core::mem::offset_of!(LobbyDTO, turn_server) == 136);
};

/// `Crossplay::Lobby::DTO::JoinLobbyDTO` — 312 bytes. **[measured]**
#[repr(C)]
#[derive(Clone, Copy)]
pub struct JoinLobbyDTO {
    /// +0 `joinedUser` : `Crossplay::Lobby::DTO::LobbyMemberDTO`
    pub joined_user: LobbyMemberDTO,
    /// +96 `lobbyDTO` : `Crossplay::Lobby::DTO::LobbyDTO`
    pub lobby_d_t_o: LobbyDTO,
    /// +304 `timestamp` : `long long`
    pub timestamp: i64,
}
const _: () = {
    assert!(core::mem::size_of::<JoinLobbyDTO>() == 312);
    assert!(core::mem::offset_of!(JoinLobbyDTO, joined_user) == 0);
    assert!(core::mem::offset_of!(JoinLobbyDTO, lobby_d_t_o) == 96);
    assert!(core::mem::offset_of!(JoinLobbyDTO, timestamp) == 304);
};

/// `Crossplay::Lobby::DTO::LeaveLobbyDTO` — 312 bytes. **[measured]**
#[repr(C)]
#[derive(Clone, Copy)]
pub struct LeaveLobbyDTO {
    /// +0 `leftUser` : `Crossplay::Lobby::DTO::LobbyMemberDTO`
    pub left_user: LobbyMemberDTO,
    /// +96 `lobbyDTO` : `Crossplay::Lobby::DTO::LobbyDTO`
    pub lobby_d_t_o: LobbyDTO,
    /// +304 `timestamp` : `long long`
    pub timestamp: i64,
}
const _: () = {
    assert!(core::mem::size_of::<LeaveLobbyDTO>() == 312);
    assert!(core::mem::offset_of!(LeaveLobbyDTO, left_user) == 0);
    assert!(core::mem::offset_of!(LeaveLobbyDTO, lobby_d_t_o) == 96);
    assert!(core::mem::offset_of!(LeaveLobbyDTO, timestamp) == 304);
};

/// `Crossplay::Lobby::DTO::UpdateLobbyDTO` — 320 bytes. **[measured]**
#[repr(C)]
#[derive(Clone, Copy)]
pub struct UpdateLobbyDTO {
    /// +0 `success` : `bool`
    pub success: bool,
    /// MSVC alignment padding, +1..+4.
    pub _pad0: [u8; 3],
    /// +4 `updatingUser` : `Crossplay::Lobby::DTO::LobbyMemberDTO`
    pub updating_user: LobbyMemberDTO,
    /// +100 `lobbyDTO` : `Crossplay::Lobby::DTO::LobbyDTO`
    pub lobby_d_t_o: LobbyDTO,
    /// MSVC alignment padding, +308..+312.
    pub _pad1: [u8; 4],
    /// +312 `timestamp` : `long long`
    pub timestamp: i64,
}
const _: () = {
    assert!(core::mem::size_of::<UpdateLobbyDTO>() == 320);
    assert!(core::mem::offset_of!(UpdateLobbyDTO, success) == 0);
    assert!(core::mem::offset_of!(UpdateLobbyDTO, updating_user) == 4);
    assert!(core::mem::offset_of!(UpdateLobbyDTO, lobby_d_t_o) == 100);
    assert!(core::mem::offset_of!(UpdateLobbyDTO, timestamp) == 312);
};

/// `Crossplay::Lobby::DTO::UpdateLobbyResultDTO` — 224 bytes. **[measured]**
#[repr(C)]
#[derive(Clone, Copy)]
pub struct UpdateLobbyResultDTO {
    /// +0 `success` : `bool`
    pub success: bool,
    /// MSVC alignment padding, +1..+4.
    pub _pad0: [u8; 3],
    /// +4 `lobbyDTO` : `Crossplay::Lobby::DTO::LobbyDTO`
    pub lobby_d_t_o: LobbyDTO,
    /// MSVC alignment padding, +212..+216.
    pub _pad1: [u8; 4],
    /// +216 `timestamp` : `long long`
    pub timestamp: i64,
}
const _: () = {
    assert!(core::mem::size_of::<UpdateLobbyResultDTO>() == 224);
    assert!(core::mem::offset_of!(UpdateLobbyResultDTO, success) == 0);
    assert!(core::mem::offset_of!(UpdateLobbyResultDTO, lobby_d_t_o) == 4);
    assert!(core::mem::offset_of!(UpdateLobbyResultDTO, timestamp) == 216);
};

// Declared only in `rise.pdb`; absent from `CrossplayProxy.pdb`.
/// `Crossplay::Lobby::DTO::JoinLobbyResultDTO` — 216 bytes. **[measured]**
#[repr(C)]
#[derive(Clone, Copy)]
pub struct JoinLobbyResultDTO {
    /// +0 `success` : `bool`
    pub success: bool,
    /// MSVC alignment padding, +1..+4.
    pub _pad0: [u8; 3],
    /// +4 `lobby` : `Crossplay::Lobby::DTO::LobbyDTO`
    pub lobby: LobbyDTO,
    /// +212 `errorCode` : `int`
    pub error_code: i32,
}
const _: () = {
    assert!(core::mem::size_of::<JoinLobbyResultDTO>() == 216);
    assert!(core::mem::offset_of!(JoinLobbyResultDTO, success) == 0);
    assert!(core::mem::offset_of!(JoinLobbyResultDTO, lobby) == 4);
    assert!(core::mem::offset_of!(JoinLobbyResultDTO, error_code) == 212);
};

/// `Crossplay::Lobby::DTO::LobbyChatMessageDTO` — 152 bytes. **[measured]**
#[repr(C)]
#[derive(Clone, Copy)]
pub struct LobbyChatMessageDTO {
    /// +0 `_lobbyId` : `std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >`
    pub lobby_id: MsvcWstring,
    /// +24 `_chatMessage` : `std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >`
    pub chat_message: MsvcWstring,
    /// +48 `_chatUser` : `Crossplay::Lobby::DTO::LobbyMemberDTO`
    pub chat_user: LobbyMemberDTO,
    /// +144 `_timestamp` : `long long`
    pub timestamp: i64,
}
const _: () = {
    assert!(core::mem::size_of::<LobbyChatMessageDTO>() == 152);
    assert!(core::mem::offset_of!(LobbyChatMessageDTO, lobby_id) == 0);
    assert!(core::mem::offset_of!(LobbyChatMessageDTO, chat_message) == 24);
    assert!(core::mem::offset_of!(LobbyChatMessageDTO, chat_user) == 48);
    assert!(core::mem::offset_of!(LobbyChatMessageDTO, timestamp) == 144);
};

/// `Crossplay::Lobby::DTO::LobbySearchCriteriaDTO` — 12 bytes. **[measured]**
#[repr(C)]
#[derive(Clone, Copy)]
pub struct LobbySearchCriteriaDTO {
    /// +0 `_maxResults` : `int`
    pub max_results: i32,
    /// +4 `_minAvailableSlots` : `int`
    pub min_available_slots: i32,
    /// +8 `_proximity` : `Crossplay::Lobby::DTO::ELobbyProximity`
    pub proximity: LobbyProximity,
}
const _: () = {
    assert!(core::mem::size_of::<LobbySearchCriteriaDTO>() == 12);
    assert!(core::mem::offset_of!(LobbySearchCriteriaDTO, max_results) == 0);
    assert!(core::mem::offset_of!(LobbySearchCriteriaDTO, min_available_slots) == 4);
    assert!(core::mem::offset_of!(LobbySearchCriteriaDTO, proximity) == 8);
};

/// `Crossplay::Lobby::DTO::LobbySearchResultDTO` — 12 bytes. **[measured]**
#[repr(C)]
#[derive(Clone, Copy)]
pub struct LobbySearchResultDTO {
    /// +0 `lobbies` : `std::vector<Crossplay::Lobby::DTO::LobbyDTO,std::allocator<Crossplay::Lobby::DTO::LobbyDTO> >`
    pub lobbies: MsvcVector,
}
const _: () = {
    assert!(core::mem::size_of::<LobbySearchResultDTO>() == 12);
    assert!(core::mem::offset_of!(LobbySearchResultDTO, lobbies) == 0);
};

/// `Crossplay::Leaderboard::DTO::Entry` — 56 bytes. **[measured]**
#[repr(C)]
#[derive(Clone, Copy)]
pub struct LeaderboardEntry {
    /// +0 `displayName` : `std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >`
    pub display_name: MsvcWstring,
    /// +24 `playFabId` : `std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >`
    pub play_fab_id: MsvcWstring,
    /// +48 `position` : `long`
    pub position: i32,
    /// +52 `statisticValue` : `long`
    pub statistic_value: i32,
}
const _: () = {
    assert!(core::mem::size_of::<LeaderboardEntry>() == 56);
    assert!(core::mem::offset_of!(LeaderboardEntry, display_name) == 0);
    assert!(core::mem::offset_of!(LeaderboardEntry, play_fab_id) == 24);
    assert!(core::mem::offset_of!(LeaderboardEntry, position) == 48);
    assert!(core::mem::offset_of!(LeaderboardEntry, statistic_value) == 52);
};

/// `Crossplay::MatchMaking::DTO::MatchFoundDTO` — 72 bytes. **[measured]**
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MatchFoundDTO {
    /// +0 `_ticketId` : `std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >`
    pub ticket_id: MsvcWstring,
    /// +24 `_lobbyId` : `std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >`
    pub lobby_id: MsvcWstring,
    /// +48 `_userIds` : `std::vector<std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >,std...`
    pub user_ids: MsvcVector,
    /// MSVC alignment padding, +60..+64.
    pub _pad0: [u8; 4],
    /// +64 `_matchingTimeInSeconds` : `long long`
    pub matching_time_in_seconds: i64,
}
const _: () = {
    assert!(core::mem::size_of::<MatchFoundDTO>() == 72);
    assert!(core::mem::offset_of!(MatchFoundDTO, ticket_id) == 0);
    assert!(core::mem::offset_of!(MatchFoundDTO, lobby_id) == 24);
    assert!(core::mem::offset_of!(MatchFoundDTO, user_ids) == 48);
    assert!(core::mem::offset_of!(MatchFoundDTO, matching_time_in_seconds) == 64);
};

/// `Crossplay::MatchMaking::DTO::MatchMakingEnqueuedDTO` — 24 bytes. **[measured]**
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MatchMakingEnqueuedDTO {
    /// +0 `_ticketId` : `std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >`
    pub ticket_id: MsvcWstring,
}
const _: () = {
    assert!(core::mem::size_of::<MatchMakingEnqueuedDTO>() == 24);
    assert!(core::mem::offset_of!(MatchMakingEnqueuedDTO, ticket_id) == 0);
};

// ---------------------------------------------------------------------------
// Vtables. `$abi` is `"thiscall"` on x86 and `"C"` elsewhere; the slot
// assertions are written in pointer units and hold on both.
// ---------------------------------------------------------------------------

macro_rules! crossplay_vtables {
    ($abi:literal) => {
        /// `CrossplayProxy::ICrossPlayService` — the shipped 58-slot vtable.
        ///
        /// Slot-for-slot identical to `Crossplay::ICrossPlayService` as declared by
        /// `rise.pdb` and `CrossplayNetLib.pdb`, and every slot resolves to the
        /// matching `CrossplayProxy::CrossPlayService::<name>` in the shipped DLL's `.rdata` table.
        /// **[measured]**
        #[repr(C)]
        pub struct ICrossPlayServiceVtable {
            /// slot 0, vtable byte offset `0x000` —
            /// `void Init()`
            /// callee pops 0 stack bytes.
            pub Init: unsafe extern $abi fn(*mut ICrossPlayService),
            /// slot 1, vtable byte offset `0x004` —
            /// `void SetServiceUrl(const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&)`
            /// callee pops 4 stack bytes.
            pub SetServiceUrl: unsafe extern $abi fn(*mut ICrossPlayService, *const MsvcWstring),
            /// slot 2, vtable byte offset `0x008` —
            /// `void SetServiceErrorCallback(const std::function<void __cdecl(std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >)>&)`
            /// callee pops 4 stack bytes.
            pub SetServiceErrorCallback: unsafe extern $abi fn(*mut ICrossPlayService, *const MsvcFunction),
            /// slot 3, vtable byte offset `0x00c` —
            /// `void SetReliability(bool)`
            /// callee pops 4 stack bytes.
            pub SetReliability: unsafe extern $abi fn(*mut ICrossPlayService, bool),
            /// slot 4, vtable byte offset `0x010` —
            /// `void StartSession(const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&, const std::function<void __cdecl(std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > const &)>&, const std::function<void __cdecl(std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >)>&)`
            /// callee pops 12 stack bytes.
            pub StartSession: unsafe extern $abi fn(*mut ICrossPlayService, *const MsvcWstring, *const MsvcFunction, *const MsvcFunction),
            /// slot 5, vtable byte offset `0x014` —
            /// `void StopSession()`
            /// callee pops 0 stack bytes.
            pub StopSession: unsafe extern $abi fn(*mut ICrossPlayService),
            /// slot 6, vtable byte offset `0x018` —
            /// `void SetRenewTokenCallback(const std::function<void __cdecl(std::function<void __cdecl(std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > const &)>)>&)`
            /// callee pops 4 stack bytes.
            pub SetRenewTokenCallback: unsafe extern $abi fn(*mut ICrossPlayService, *const core::ffi::c_void),
            /// slot 7, vtable byte offset `0x01c` —
            /// `Crossplay::User::SessionStatus GetSessionStatus() const`
            /// callee pops 0 stack bytes.
            pub GetSessionStatus: unsafe extern $abi fn(*mut ICrossPlayService) -> SessionStatus,
            /// slot 8, vtable byte offset `0x020` —
            /// `Crossplay::CrossplayStatus GetCrossplayStatus() const`
            /// callee pops 0 stack bytes.
            pub GetCrossplayStatus: unsafe extern $abi fn(*mut ICrossPlayService) -> CrossplayStatus,
            /// slot 9, vtable byte offset `0x024` —
            /// `void BlockUser(const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&)`
            /// callee pops 4 stack bytes.
            pub BlockUser: unsafe extern $abi fn(*mut ICrossPlayService, *const MsvcWstring),
            /// slot 10, vtable byte offset `0x028` —
            /// `void UnblockUser(const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&)`
            /// callee pops 4 stack bytes.
            pub UnblockUser: unsafe extern $abi fn(*mut ICrossPlayService, *const MsvcWstring),
            /// slot 11, vtable byte offset `0x02c` —
            /// `bool IsUserBlocked(const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&) const`
            /// callee pops 4 stack bytes.
            pub IsUserBlocked: unsafe extern $abi fn(*mut ICrossPlayService, *const MsvcWstring) -> bool,
            /// slot 12, vtable byte offset `0x030` —
            /// `void GetLobby(const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&, std::function<void __cdecl(Crossplay::Lobby::DTO::LobbyDTO const &)>, std::function<void __cdecl(std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >)>)`
            /// callee pops 84 stack bytes.
            pub GetLobby: unsafe extern $abi fn(*mut ICrossPlayService, *const MsvcWstring, MsvcFunction, MsvcFunction),
            /// slot 13, vtable byte offset `0x034` —
            /// `void FindLobbies(const Crossplay::Lobby::DTO::LobbySearchCriteriaDTO&, std::function<void __cdecl(Crossplay::Lobby::DTO::LobbySearchResultDTO const &)>, std::function<void __cdecl(std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >)>)`
            /// callee pops 84 stack bytes.
            pub FindLobbies: unsafe extern $abi fn(*mut ICrossPlayService, *const LobbySearchCriteriaDTO, MsvcFunction, MsvcFunction),
            /// slot 14, vtable byte offset `0x038` —
            /// `void CreateLobby(int, Crossplay::Lobby::Visibility, const std::unordered_map<std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >,std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >,std::hash<std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > >,std::equal_to<std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > >,std::allocator<std::pair<std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > const ,std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > > > >&, std::function<void __cdecl(Crossplay::Lobby::DTO::LobbyDTO const &)>, std::function<void __cdecl(std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >)>)`
            /// callee pops 92 stack bytes.
            pub CreateLobby: unsafe extern $abi fn(*mut ICrossPlayService, i32, Visibility, *const MsvcUnorderedMap32, MsvcFunction, MsvcFunction),
            /// slot 15, vtable byte offset `0x03c` —
            /// `void JoinLobby(const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&, const std::function<void __cdecl(Crossplay::Lobby::DTO::LobbyDTO const &)>&, const std::function<void __cdecl(int,std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > const &)>&)`
            /// callee pops 12 stack bytes.
            pub JoinLobby: unsafe extern $abi fn(*mut ICrossPlayService, *const MsvcWstring, *const MsvcFunction, *const MsvcFunction),
            /// slot 16, vtable byte offset `0x040` —
            /// `void SetJoinLobbyCallback(const std::function<void __cdecl(Crossplay::Lobby::DTO::JoinLobbyDTO const &,bool)>&)`
            /// callee pops 4 stack bytes.
            pub SetJoinLobbyCallback: unsafe extern $abi fn(*mut ICrossPlayService, *const MsvcFunction),
            /// slot 17, vtable byte offset `0x044` —
            /// `void LeaveLobby(const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&, const std::function<void __cdecl(void)>&, const std::function<void __cdecl(int)>&)`
            /// callee pops 12 stack bytes.
            pub LeaveLobby: unsafe extern $abi fn(*mut ICrossPlayService, *const MsvcWstring, *const MsvcFunction, *const MsvcFunction),
            /// slot 18, vtable byte offset `0x048` —
            /// `void SetLeaveLobbyCallback(const std::function<void __cdecl(Crossplay::Lobby::DTO::LeaveLobbyDTO const &,bool)>&)`
            /// callee pops 4 stack bytes.
            pub SetLeaveLobbyCallback: unsafe extern $abi fn(*mut ICrossPlayService, *const MsvcFunction),
            /// slot 19, vtable byte offset `0x04c` —
            /// `void UpdateLobby(const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&, int, const std::unordered_map<std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >,std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >,std::hash<std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > >,std::equal_to<std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > >,std::allocator<std::pair<std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > const ,std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > > > >&, int, int, const std::function<void __cdecl(Crossplay::Lobby::DTO::UpdateLobbyResultDTO const &)>&, std::function<void __cdecl(std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >)>)`
            /// callee pops 64 stack bytes.
            pub UpdateLobby: unsafe extern $abi fn(*mut ICrossPlayService, *const MsvcWstring, i32, *const MsvcUnorderedMap32, i32, i32, *const MsvcFunction, MsvcFunction),
            /// slot 20, vtable byte offset `0x050` —
            /// `void SetUpdateLobbyCallback(const std::function<void __cdecl(Crossplay::Lobby::DTO::UpdateLobbyDTO const &,bool)>&)`
            /// callee pops 4 stack bytes.
            pub SetUpdateLobbyCallback: unsafe extern $abi fn(*mut ICrossPlayService, *const MsvcFunction),
            /// slot 21, vtable byte offset `0x054` —
            /// `void LobbyCancelPendingRequests()`
            /// callee pops 0 stack bytes.
            pub LobbyCancelPendingRequests: unsafe extern $abi fn(*mut ICrossPlayService),
            /// slot 22, vtable byte offset `0x058` —
            /// `void StartGame(const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&, bool, std::function<void __cdecl(std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > const &)>, std::function<void __cdecl(std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > const &)>)`
            /// callee pops 88 stack bytes.
            pub StartGame: unsafe extern $abi fn(*mut ICrossPlayService, *const MsvcWstring, bool, MsvcFunction, MsvcFunction),
            /// slot 23, vtable byte offset `0x05c` —
            /// `void CancelGameStart(const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&, std::function<void __cdecl(std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > const &)>, std::function<void __cdecl(std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > const &)>)`
            /// callee pops 84 stack bytes.
            pub CancelGameStart: unsafe extern $abi fn(*mut ICrossPlayService, *const MsvcWstring, MsvcFunction, MsvcFunction),
            /// slot 24, vtable byte offset `0x060` —
            /// `void SendLobbyChat(const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&, const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&)`
            /// callee pops 8 stack bytes.
            pub SendLobbyChat: unsafe extern $abi fn(*mut ICrossPlayService, *const MsvcWstring, *const MsvcWstring),
            /// slot 25, vtable byte offset `0x064` —
            /// `void SetLobbyMessageReceivedCallback(const std::function<void __cdecl(Crossplay::Lobby::DTO::LobbyChatMessageDTO const &,bool)>&)`
            /// callee pops 4 stack bytes.
            pub SetLobbyMessageReceivedCallback: unsafe extern $abi fn(*mut ICrossPlayService, *const MsvcFunction),
            /// slot 26, vtable byte offset `0x068` —
            /// `void SetCrossplayEnabled(const std::function<void __cdecl(bool)>&)`
            /// callee pops 4 stack bytes.
            pub SetCrossplayEnabled: unsafe extern $abi fn(*mut ICrossPlayService, *const MsvcFunction),
            /// slot 27, vtable byte offset `0x06c` —
            /// `void SetStats(const std::map<std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >,int,std::less<std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > >,std::allocator<std::pair<std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > const ,int> > >&, std::function<void __cdecl(bool)>, std::function<void __cdecl(std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >)>)`
            /// callee pops 84 stack bytes.
            pub SetStats: unsafe extern $abi fn(*mut ICrossPlayService, *const MsvcMap8, MsvcFunction, MsvcFunction),
            /// slot 28, vtable byte offset `0x070` —
            /// `void GetStats(const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&, std::function<void __cdecl(std::map<std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >,int,std::less<std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > >,std::allocator<std::pair<std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > const ,int> > > const &)>, std::function<void __cdecl(std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >)>)`
            /// callee pops 84 stack bytes.
            pub GetStats: unsafe extern $abi fn(*mut ICrossPlayService, *const MsvcWstring, MsvcFunction, MsvcFunction),
            /// slot 29, vtable byte offset `0x074` —
            /// `void GetGlobalLeaderboards(const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&, const int, std::function<void __cdecl(std::vector<Crossplay::Leaderboard::DTO::Entry,std::allocator<Crossplay::Leaderboard::DTO::Entry> > const &)>, std::function<void __cdecl(std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >)>)`
            /// callee pops 88 stack bytes.
            pub GetGlobalLeaderboards: unsafe extern $abi fn(*mut ICrossPlayService, *const MsvcWstring, i32, MsvcFunction, MsvcFunction),
            /// slot 30, vtable byte offset `0x078` —
            /// `void GetLeaderboardsAroundPlayer(const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&, const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&, const int, std::function<void __cdecl(std::vector<Crossplay::Leaderboard::DTO::Entry,std::allocator<Crossplay::Leaderboard::DTO::Entry> > const &)>, std::function<void __cdecl(std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >)>)`
            /// callee pops 92 stack bytes.
            pub GetLeaderboardsAroundPlayer: unsafe extern $abi fn(*mut ICrossPlayService, *const MsvcWstring, *const MsvcWstring, i32, MsvcFunction, MsvcFunction),
            /// slot 31, vtable byte offset `0x07c` —
            /// `void CreateInvitation(const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&, const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&, std::function<void __cdecl(bool)>, std::function<void __cdecl(std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >)>)`
            /// callee pops 88 stack bytes.
            pub CreateInvitation: unsafe extern $abi fn(*mut ICrossPlayService, *const MsvcWstring, *const MsvcWstring, MsvcFunction, MsvcFunction),
            /// slot 32, vtable byte offset `0x080` —
            /// `void AcceptInvitation(const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&, std::function<void __cdecl(std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > const &)>, std::function<void __cdecl(std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >)>)`
            /// callee pops 84 stack bytes.
            pub AcceptInvitation: unsafe extern $abi fn(*mut ICrossPlayService, *const MsvcWstring, MsvcFunction, MsvcFunction),
            /// slot 33, vtable byte offset `0x084` —
            /// `std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > GetInvitationId() const`
            /// callee pops 4 stack bytes.
            pub GetInvitationId: unsafe extern $abi fn(*mut ICrossPlayService, *mut MsvcWstring) -> *mut MsvcWstring,
            /// slot 34, vtable byte offset `0x088` —
            /// `void JoinChat(const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&, std::function<void __cdecl(Crossplay::Lobby::DTO::LobbyDTO const &)>, std::function<void __cdecl(std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > const &)>)`
            /// callee pops 84 stack bytes.
            pub JoinChat: unsafe extern $abi fn(*mut ICrossPlayService, *const MsvcWstring, MsvcFunction, MsvcFunction),
            /// slot 35, vtable byte offset `0x08c` —
            /// `void SetChatJoinCallback(const std::function<void __cdecl(std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > const &,Crossplay::Lobby::DTO::JoinLobbyDTO const &,bool)>&)`
            /// callee pops 4 stack bytes.
            pub SetChatJoinCallback: unsafe extern $abi fn(*mut ICrossPlayService, *const MsvcFunction),
            /// slot 36, vtable byte offset `0x090` —
            /// `void LeaveChat(const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&)`
            /// callee pops 4 stack bytes.
            pub LeaveChat: unsafe extern $abi fn(*mut ICrossPlayService, *const MsvcWstring),
            /// slot 37, vtable byte offset `0x094` —
            /// `void SetChatLeaveCallback(const std::function<void __cdecl(std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > const &,Crossplay::Lobby::DTO::LeaveLobbyDTO const &,bool)>&)`
            /// callee pops 4 stack bytes.
            pub SetChatLeaveCallback: unsafe extern $abi fn(*mut ICrossPlayService, *const MsvcFunction),
            /// slot 38, vtable byte offset `0x098` —
            /// `void SendChatMessage(const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&, const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&)`
            /// callee pops 8 stack bytes.
            pub SendChatMessage: unsafe extern $abi fn(*mut ICrossPlayService, *const MsvcWstring, *const MsvcWstring),
            /// slot 39, vtable byte offset `0x09c` —
            /// `void SetChatMessageReceivedCallback(const std::function<void __cdecl(std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > const &,Crossplay::Lobby::DTO::LobbyChatMessageDTO const &,bool)>&)`
            /// callee pops 4 stack bytes.
            pub SetChatMessageReceivedCallback: unsafe extern $abi fn(*mut ICrossPlayService, *const MsvcFunction),
            /// slot 40, vtable byte offset `0x0a0` —
            /// `void SetP2PConnectionOpenedCallback(const std::function<void __cdecl(Crossplay::P2P::ICrossplayPlayer *)>&)`
            /// callee pops 4 stack bytes.
            pub SetP2PConnectionOpenedCallback: unsafe extern $abi fn(*mut ICrossPlayService, *const MsvcFunction),
            /// slot 41, vtable byte offset `0x0a4` —
            /// `void SetP2PConnectionClosedCallback(const std::function<void __cdecl(Crossplay::P2P::ICrossplayPlayer *)>&)`
            /// callee pops 4 stack bytes.
            pub SetP2PConnectionClosedCallback: unsafe extern $abi fn(*mut ICrossPlayService, *const MsvcFunction),
            /// slot 42, vtable byte offset `0x0a8` —
            /// `void SetP2PDataChannelOpenedCallback(const std::function<void __cdecl(Crossplay::P2P::ICrossplayPlayer *)>&)`
            /// callee pops 4 stack bytes.
            pub SetP2PDataChannelOpenedCallback: unsafe extern $abi fn(*mut ICrossPlayService, *const MsvcFunction),
            /// slot 43, vtable byte offset `0x0ac` —
            /// `void SetP2PDataChannelClosedCallback(const std::function<void __cdecl(Crossplay::P2P::ICrossplayPlayer *)>&)`
            /// callee pops 4 stack bytes.
            pub SetP2PDataChannelClosedCallback: unsafe extern $abi fn(*mut ICrossPlayService, *const MsvcFunction),
            /// slot 44, vtable byte offset `0x0b0` —
            /// `void SetReceivedP2PTextCallback(const std::function<void __cdecl(Crossplay::P2P::ICrossplayPlayer *,std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > const &)>&)`
            /// callee pops 4 stack bytes.
            pub SetReceivedP2PTextCallback: unsafe extern $abi fn(*mut ICrossPlayService, *const MsvcFunction),
            /// slot 45, vtable byte offset `0x0b4` —
            /// `void SetReceivedP2PDataCallback(const std::function<void __cdecl(Crossplay::P2P::ICrossplayPlayer *,unsigned char const *,unsigned int)>&)`
            /// callee pops 4 stack bytes.
            pub SetReceivedP2PDataCallback: unsafe extern $abi fn(*mut ICrossPlayService, *const MsvcFunction),
            /// slot 46, vtable byte offset `0x0b8` —
            /// `void SetP2PConnectionFailedCallback(const std::function<void __cdecl(std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >,std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >)>&)`
            /// callee pops 4 stack bytes.
            pub SetP2PConnectionFailedCallback: unsafe extern $abi fn(*mut ICrossPlayService, *const core::ffi::c_void),
            /// slot 47, vtable byte offset `0x0bc` —
            /// `void SetP2PTimeoutDuration(int)`
            /// callee pops 4 stack bytes.
            pub SetP2PTimeoutDuration: unsafe extern $abi fn(*mut ICrossPlayService, i32),
            /// slot 48, vtable byte offset `0x0c0` —
            /// `void P2PStartConnection(const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&, const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&)`
            /// callee pops 8 stack bytes.
            pub P2PStartConnection: unsafe extern $abi fn(*mut ICrossPlayService, *const MsvcWstring, *const MsvcWstring),
            /// slot 49, vtable byte offset `0x0c4` —
            /// `bool P2PSendToAll(unsigned char*, unsigned int)`
            /// callee pops 8 stack bytes.
            pub P2PSendToAll: unsafe extern $abi fn(*mut ICrossPlayService, *mut u8, u32) -> bool,
            /// slot 50, vtable byte offset `0x0c8` —
            /// `bool P2PSend(Crossplay::P2P::ICrossplayPlayer*, unsigned char*, unsigned int)`
            /// callee pops 12 stack bytes.
            pub P2PSend: unsafe extern $abi fn(*mut ICrossPlayService, *mut ICrossplayPlayer, *mut u8, u32) -> bool,
            /// slot 51, vtable byte offset `0x0cc` —
            /// `void P2PCloseAll()`
            /// callee pops 0 stack bytes.
            pub P2PCloseAll: unsafe extern $abi fn(*mut ICrossPlayService),
            /// slot 52, vtable byte offset `0x0d0` —
            /// `void P2PClose(const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&)`
            /// callee pops 4 stack bytes.
            pub P2PClose: unsafe extern $abi fn(*mut ICrossPlayService, *const MsvcWstring),
            /// slot 53, vtable byte offset `0x0d4` —
            /// `bool IsConnectedToHub()`
            /// callee pops 0 stack bytes.
            pub IsConnectedToHub: unsafe extern $abi fn(*mut ICrossPlayService) -> bool,
            /// slot 54, vtable byte offset `0x0d8` —
            /// `void CreateLocalPlayerLoopback()`
            /// callee pops 0 stack bytes.
            pub CreateLocalPlayerLoopback: unsafe extern $abi fn(*mut ICrossPlayService),
            /// slot 55, vtable byte offset `0x0dc` —
            /// `void SetUsername(const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&)`
            /// callee pops 4 stack bytes.
            pub SetUsername: unsafe extern $abi fn(*mut ICrossPlayService, *const MsvcWstring),
            /// slot 56, vtable byte offset `0x0e0` —
            /// `std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >& GetPlayerGuid()`
            /// callee pops 0 stack bytes.
            pub GetPlayerGuid: unsafe extern $abi fn(*mut ICrossPlayService) -> *mut MsvcWstring,
            /// slot 57, vtable byte offset `0x0e4` —
            /// `void Tick()`
            /// callee pops 0 stack bytes.
            pub Tick: unsafe extern $abi fn(*mut ICrossPlayService),
        }
        /// `Crossplay::Logging::ICrossplayLogger` — 4 slots. Export ordinal 1 returns one.
        /// **[measured]**
        #[repr(C)]
        pub struct ICrossplayLoggerVtable {
            /// slot 0, vtable byte offset `0x000` —
            /// `void* __vecDelDtor(unsigned int)`
            /// callee pops 4 stack bytes.
            pub vector_deleting_destructor: unsafe extern $abi fn(*mut ICrossplayLogger, u32) -> *mut core::ffi::c_void,
            /// slot 1, vtable byte offset `0x004` —
            /// `void Register(Crossplay::Logging::LogLevel, std::function<void __cdecl(enum Crossplay::Logging::LogLevel,std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >,char const *,int)>)`
            /// callee pops 44 stack bytes.
            pub Register: unsafe extern $abi fn(*mut ICrossplayLogger, LogLevel, MsvcFunction),
            /// slot 2, vtable byte offset `0x008` —
            /// `void UnregisterAll()`
            /// callee pops 0 stack bytes.
            pub UnregisterAll: unsafe extern $abi fn(*mut ICrossplayLogger),
            /// slot 3, vtable byte offset `0x00c` —
            /// `void Log(Crossplay::Logging::LogLevel, std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >, const char*, int)`
            /// callee pops 36 stack bytes.
            pub Log: unsafe extern $abi fn(*mut ICrossplayLogger, LogLevel, MsvcWstring, *const core::ffi::c_char, i32),
        }
        /// `Crossplay::P2P::ICrossplayPlayer` — 14 slots, the per-peer handle.
        /// **[measured]**
        #[repr(C)]
        pub struct ICrossplayPlayerVtable {
            /// slot 0, vtable byte offset `0x000` —
            /// `void* __vecDelDtor(unsigned int)`
            /// callee pops 4 stack bytes.
            pub vector_deleting_destructor: unsafe extern $abi fn(*mut ICrossplayPlayer, u32) -> *mut core::ffi::c_void,
            /// slot 1, vtable byte offset `0x004` —
            /// `bool Send(unsigned char*, int) const`
            /// callee pops 8 stack bytes.
            pub Send__s1: unsafe extern $abi fn(*mut ICrossplayPlayer, *mut u8, i32) -> bool,
            /// slot 2, vtable byte offset `0x008` —
            /// `bool Send(const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&) const`
            /// callee pops 4 stack bytes.
            pub Send__s2: unsafe extern $abi fn(*mut ICrossplayPlayer, *const MsvcWstring) -> bool,
            /// slot 3, vtable byte offset `0x00c` —
            /// `void MuteAudio()`
            /// callee pops 0 stack bytes.
            pub MuteAudio: unsafe extern $abi fn(*mut ICrossplayPlayer),
            /// slot 4, vtable byte offset `0x010` —
            /// `void UnmuteAudio()`
            /// callee pops 0 stack bytes.
            pub UnmuteAudio: unsafe extern $abi fn(*mut ICrossplayPlayer),
            /// slot 5, vtable byte offset `0x014` —
            /// `void CloseConnection()`
            /// callee pops 0 stack bytes.
            pub CloseConnection: unsafe extern $abi fn(*mut ICrossplayPlayer),
            /// slot 6, vtable byte offset `0x018` —
            /// `bool IsDataChannelOpen()`
            /// callee pops 0 stack bytes.
            pub IsDataChannelOpen: unsafe extern $abi fn(*mut ICrossplayPlayer) -> bool,
            /// slot 7, vtable byte offset `0x01c` —
            /// `void SetReceivedTextCallback(const std::function<void __cdecl(std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >)>&)`
            /// callee pops 4 stack bytes.
            pub SetReceivedTextCallback: unsafe extern $abi fn(*mut ICrossplayPlayer, *const MsvcFunction),
            /// slot 8, vtable byte offset `0x020` —
            /// `void SetReceivedDataCallback(const std::function<void __cdecl(unsigned char const *,int)>&)`
            /// callee pops 4 stack bytes.
            pub SetReceivedDataCallback: unsafe extern $abi fn(*mut ICrossplayPlayer, *const core::ffi::c_void),
            /// slot 9, vtable byte offset `0x024` —
            /// `void SetDataChannelOpenedCallback(const std::function<void __cdecl(void)>&)`
            /// callee pops 4 stack bytes.
            pub SetDataChannelOpenedCallback: unsafe extern $abi fn(*mut ICrossplayPlayer, *const MsvcFunction),
            /// slot 10, vtable byte offset `0x028` —
            /// `void SetDataChannelClosedCallback(const std::function<void __cdecl(void)>&)`
            /// callee pops 4 stack bytes.
            pub SetDataChannelClosedCallback: unsafe extern $abi fn(*mut ICrossplayPlayer, *const MsvcFunction),
            /// slot 11, vtable byte offset `0x02c` —
            /// `void SetConnectionOpenedCallback(const std::function<void __cdecl(void)>&)`
            /// callee pops 4 stack bytes.
            pub SetConnectionOpenedCallback: unsafe extern $abi fn(*mut ICrossplayPlayer, *const MsvcFunction),
            /// slot 12, vtable byte offset `0x030` —
            /// `void SetConnectionClosedCallback(const std::function<void __cdecl(void)>&)`
            /// callee pops 4 stack bytes.
            pub SetConnectionClosedCallback: unsafe extern $abi fn(*mut ICrossplayPlayer, *const MsvcFunction),
            /// slot 13, vtable byte offset `0x034` —
            /// `const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >& GetId() const`
            /// callee pops 0 stack bytes.
            pub GetId: unsafe extern $abi fn(*mut ICrossplayPlayer) -> *const MsvcWstring,
        }
        /// `CrossplayProxy::INetworkClient` — 15 slots.
        /// The Party-facing side; `CrossplayProxy::NetworkFSM` implements it.
        /// **[measured]**
        #[repr(C)]
        pub struct INetworkClientVtable {
            /// slot 0, vtable byte offset `0x000` —
            /// `const std::basic_string<char,std::char_traits<char>,std::allocator<char> >& GetNetworkID() const`
            /// callee pops 0 stack bytes.
            pub GetNetworkID: unsafe extern $abi fn(*mut INetworkClient) -> *const MsvcString,
            /// slot 1, vtable byte offset `0x004` —
            /// `const std::basic_string<char,std::char_traits<char>,std::allocator<char> >& GetNetworkDescriptor() const`
            /// callee pops 0 stack bytes.
            pub GetNetworkDescriptor: unsafe extern $abi fn(*mut INetworkClient) -> *const MsvcString,
            /// slot 2, vtable byte offset `0x008` —
            /// `void CreateNetwork(unsigned int, const std::function<void __cdecl(bool,std::basic_string<char,std::char_traits<char>,std::allocator<char> > const &,std::basic_string<char,std::char_traits<char>,std::allocator<char> > const &)>&)`
            /// callee pops 8 stack bytes.
            pub CreateNetwork: unsafe extern $abi fn(*mut INetworkClient, u32, *const MsvcFunction),
            /// slot 3, vtable byte offset `0x00c` —
            /// `void JoinNetwork(const std::basic_string<char,std::char_traits<char>,std::allocator<char> >&, const std::function<void __cdecl(bool,std::basic_string<char,std::char_traits<char>,std::allocator<char> > const &)>&)`
            /// callee pops 8 stack bytes.
            pub JoinNetwork: unsafe extern $abi fn(*mut INetworkClient, *const MsvcString, *const MsvcFunction),
            /// slot 4, vtable byte offset `0x010` —
            /// `void LeaveNetwork(const std::function<void __cdecl(bool,std::basic_string<char,std::char_traits<char>,std::allocator<char> > const &)>&)`
            /// callee pops 4 stack bytes.
            pub LeaveNetwork: unsafe extern $abi fn(*mut INetworkClient, *const MsvcFunction),
            /// slot 5, vtable byte offset `0x014` —
            /// `bool SendP2PData(const std::basic_string<char,std::char_traits<char>,std::allocator<char> >&, unsigned int, const void*)`
            /// callee pops 12 stack bytes.
            pub SendP2PData: unsafe extern $abi fn(*mut INetworkClient, *const MsvcString, u32, *const core::ffi::c_void) -> bool,
            /// slot 6, vtable byte offset `0x018` —
            /// `bool SendP2PDataToAll(unsigned int, const void*)`
            /// callee pops 8 stack bytes.
            pub SendP2PDataToAll: unsafe extern $abi fn(*mut INetworkClient, u32, *const core::ffi::c_void) -> bool,
            /// slot 7, vtable byte offset `0x01c` —
            /// `void SendP2PTextData(const std::basic_string<char,std::char_traits<char>,std::allocator<char> >&, unsigned int, const char*)`
            /// callee pops 12 stack bytes.
            pub SendP2PTextData: unsafe extern $abi fn(*mut INetworkClient, *const MsvcString, u32, *const core::ffi::c_char),
            /// slot 8, vtable byte offset `0x020` —
            /// `void SendP2PTextDataToAll(unsigned int, const char*)`
            /// callee pops 8 stack bytes.
            pub SendP2PTextDataToAll: unsafe extern $abi fn(*mut INetworkClient, u32, *const core::ffi::c_char),
            /// slot 9, vtable byte offset `0x024` —
            /// `void SetPlayerJoinedCallback(const std::function<void __cdecl(std::basic_string<char,std::char_traits<char>,std::allocator<char> > const &,std::basic_string<char,std::char_traits<char>,std::allocator<char> > const &)>&)`
            /// callee pops 4 stack bytes.
            pub SetPlayerJoinedCallback: unsafe extern $abi fn(*mut INetworkClient, *const MsvcFunction),
            /// slot 10, vtable byte offset `0x028` —
            /// `void SetPlayerLeftCallback(const std::function<void __cdecl(std::basic_string<char,std::char_traits<char>,std::allocator<char> > const &,std::basic_string<char,std::char_traits<char>,std::allocator<char> > const &,enum Party::PartyDestroyedReason)>&)`
            /// callee pops 4 stack bytes.
            pub SetPlayerLeftCallback: unsafe extern $abi fn(*mut INetworkClient, *const MsvcFunction),
            /// slot 11, vtable byte offset `0x02c` —
            /// `void SetP2PDataReceivedCallback(const std::function<void __cdecl(std::basic_string<char,std::char_traits<char>,std::allocator<char> > const &,std::basic_string<char,std::char_traits<char>,std::allocator<char> > const &,unsigned int,void const *)>&)`
            /// callee pops 4 stack bytes.
            pub SetP2PDataReceivedCallback: unsafe extern $abi fn(*mut INetworkClient, *const MsvcFunction),
            /// slot 12, vtable byte offset `0x030` —
            /// `void SetP2PTextDataReceivedCallback(const std::function<void __cdecl(std::basic_string<char,std::char_traits<char>,std::allocator<char> > const &,std::basic_string<char,std::char_traits<char>,std::allocator<char> > const &,unsigned int,char const *)>&)`
            /// callee pops 4 stack bytes.
            pub SetP2PTextDataReceivedCallback: unsafe extern $abi fn(*mut INetworkClient, *const MsvcFunction),
            /// slot 13, vtable byte offset `0x034` —
            /// `void SetFailedConnectionToNetworkCallback(const std::function<void __cdecl(std::basic_string<char,std::char_traits<char>,std::allocator<char> > const &,std::basic_string<char,std::char_traits<char>,std::allocator<char> > const &)>&)`
            /// callee pops 4 stack bytes.
            pub SetFailedConnectionToNetworkCallback: unsafe extern $abi fn(*mut INetworkClient, *const MsvcFunction),
            /// slot 14, vtable byte offset `0x038` —
            /// `void ResetCallbacks()`
            /// callee pops 0 stack bytes.
            pub ResetCallbacks: unsafe extern $abi fn(*mut INetworkClient),
        }
        /// `Crossplay::ICrossPlayService` as declared by `CrossplayProxy.pdb` — 96 slots.
        ///
        /// **THIS IS NOT THE SHIPPED VTABLE AND RETAIL NEVER CALLS IT.** It comes
        /// from a newer vendor header that this build does not use. It is emitted
        /// only so the discrepancy is recorded in checked code rather than prose:
        /// the extra `P2PEnableTcp` / `SetP2PAllowedPorts` / matchmaking-ticket
        /// APIs show what the vendor SDK grew after RoN:EE shipped.
        #[repr(C)]
        pub struct VendorICrossPlayServiceVtable {
            /// slot 0, vtable byte offset `0x000` —
            /// `void* __vecDelDtor(unsigned int)`
            /// callee pops 4 stack bytes.
            pub vector_deleting_destructor: unsafe extern $abi fn(*mut core::ffi::c_void, u32) -> *mut core::ffi::c_void,
            /// slot 1, vtable byte offset `0x004` —
            /// `void SetNew(std::function<void * __cdecl(unsigned int)>)`
            /// **NOT DERIVED** (unresolved type `std::function<void * __cdecl(unsigned int)>`); do not call through this field.
            pub SetNew: *const core::ffi::c_void,
            /// slot 2, vtable byte offset `0x008` —
            /// `void SetDelete(std::function<void __cdecl(void *)>)`
            /// **NOT DERIVED** (unresolved type `std::function<void __cdecl(void *)>`); do not call through this field.
            pub SetDelete: *const core::ffi::c_void,
            /// slot 3, vtable byte offset `0x00c` —
            /// `void Init()`
            /// callee pops 0 stack bytes.
            pub Init: unsafe extern $abi fn(*mut core::ffi::c_void),
            /// slot 4, vtable byte offset `0x010` —
            /// `void Shutdown()`
            /// callee pops 0 stack bytes.
            pub Shutdown: unsafe extern $abi fn(*mut core::ffi::c_void),
            /// slot 5, vtable byte offset `0x014` —
            /// `void SetServiceUrl(const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&)`
            /// callee pops 4 stack bytes.
            pub SetServiceUrl: unsafe extern $abi fn(*mut core::ffi::c_void, *const MsvcWstring),
            /// slot 6, vtable byte offset `0x018` —
            /// `void SetServiceErrorCallback(const std::function<void __cdecl(std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >)>&)`
            /// callee pops 4 stack bytes.
            pub SetServiceErrorCallback: unsafe extern $abi fn(*mut core::ffi::c_void, *const MsvcFunction),
            /// slot 7, vtable byte offset `0x01c` —
            /// `void SetReliability(bool)`
            /// callee pops 4 stack bytes.
            pub SetReliability: unsafe extern $abi fn(*mut core::ffi::c_void, bool),
            /// slot 8, vtable byte offset `0x020` —
            /// `void SetP2PPacketOrderedDelivery(bool)`
            /// callee pops 4 stack bytes.
            pub SetP2PPacketOrderedDelivery: unsafe extern $abi fn(*mut core::ffi::c_void, bool),
            /// slot 9, vtable byte offset `0x024` —
            /// `void SetP2PMaxPacketLifetimeMs(int)`
            /// callee pops 4 stack bytes.
            pub SetP2PMaxPacketLifetimeMs: unsafe extern $abi fn(*mut core::ffi::c_void, i32),
            /// slot 10, vtable byte offset `0x028` —
            /// `void SetP2PMaxPacketRetryAttempts(int)`
            /// callee pops 4 stack bytes.
            pub SetP2PMaxPacketRetryAttempts: unsafe extern $abi fn(*mut core::ffi::c_void, i32),
            /// slot 11, vtable byte offset `0x02c` —
            /// `void SetP2PAllowedPorts(unsigned short, unsigned short)`
            /// callee pops 8 stack bytes.
            pub SetP2PAllowedPorts: unsafe extern $abi fn(*mut core::ffi::c_void, u16, u16),
            /// slot 12, vtable byte offset `0x030` —
            /// `void P2PDisallowPorts(unsigned short, unsigned short)`
            /// callee pops 8 stack bytes.
            pub P2PDisallowPorts: unsafe extern $abi fn(*mut core::ffi::c_void, u16, u16),
            /// slot 13, vtable byte offset `0x034` —
            /// `void StartSession(const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&, const std::function<void __cdecl(std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > const &)>&, const std::function<void __cdecl(std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >)>&)`
            /// callee pops 12 stack bytes.
            pub StartSession: unsafe extern $abi fn(*mut core::ffi::c_void, *const MsvcWstring, *const MsvcFunction, *const MsvcFunction),
            /// slot 14, vtable byte offset `0x038` —
            /// `void StopSession()`
            /// callee pops 0 stack bytes.
            pub StopSession: unsafe extern $abi fn(*mut core::ffi::c_void),
            /// slot 15, vtable byte offset `0x03c` —
            /// `void SetRenewTokenCallback(const std::function<void __cdecl(std::function<void __cdecl(std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > const &)>)>&)`
            /// callee pops 4 stack bytes.
            pub SetRenewTokenCallback: unsafe extern $abi fn(*mut core::ffi::c_void, *const core::ffi::c_void),
            /// slot 16, vtable byte offset `0x040` —
            /// `Crossplay::User::SessionStatus GetSessionStatus() const`
            /// callee pops 0 stack bytes.
            pub GetSessionStatus: unsafe extern $abi fn(*mut core::ffi::c_void) -> SessionStatus,
            /// slot 17, vtable byte offset `0x044` —
            /// `Crossplay::CrossplayStatus GetCrossplayStatus() const`
            /// callee pops 0 stack bytes.
            pub GetCrossplayStatus: unsafe extern $abi fn(*mut core::ffi::c_void) -> CrossplayStatus,
            /// slot 18, vtable byte offset `0x048` —
            /// `bool IsLoggedOn()`
            /// callee pops 0 stack bytes.
            pub IsLoggedOn: unsafe extern $abi fn(*mut core::ffi::c_void) -> bool,
            /// slot 19, vtable byte offset `0x04c` —
            /// `bool IsLoggedOff()`
            /// callee pops 0 stack bytes.
            pub IsLoggedOff: unsafe extern $abi fn(*mut core::ffi::c_void) -> bool,
            /// slot 20, vtable byte offset `0x050` —
            /// `bool IsLoggingOnInProcess()`
            /// callee pops 0 stack bytes.
            pub IsLoggingOnInProcess: unsafe extern $abi fn(*mut core::ffi::c_void) -> bool,
            /// slot 21, vtable byte offset `0x054` —
            /// `void BlockUser(const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&)`
            /// callee pops 4 stack bytes.
            pub BlockUser: unsafe extern $abi fn(*mut core::ffi::c_void, *const MsvcWstring),
            /// slot 22, vtable byte offset `0x058` —
            /// `void BlockXuids(const std::list<std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >,std::allocator<std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > > >&)`
            /// callee pops 4 stack bytes.
            pub BlockXuids: unsafe extern $abi fn(*mut core::ffi::c_void, *const core::ffi::c_void),
            /// slot 23, vtable byte offset `0x05c` —
            /// `void UnblockUser(const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&)`
            /// callee pops 4 stack bytes.
            pub UnblockUser: unsafe extern $abi fn(*mut core::ffi::c_void, *const MsvcWstring),
            /// slot 24, vtable byte offset `0x060` —
            /// `const std::list<std::unique_ptr<Crossplay::User::DTO::UserDTO,std::default_delete<Crossplay::User::DTO::UserDTO> >,std::allocator<std::unique_ptr<Crossplay::User::DTO::UserDTO,std::default_delete<Crossplay::User::DTO::UserDTO> > > >* GetBlockedUsers() const`
            /// callee pops 0 stack bytes.
            pub GetBlockedUsers: unsafe extern $abi fn(*mut core::ffi::c_void) -> *const core::ffi::c_void,
            /// slot 25, vtable byte offset `0x064` —
            /// `bool IsUserBlocked(const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&) const`
            /// callee pops 4 stack bytes.
            pub IsUserBlocked: unsafe extern $abi fn(*mut core::ffi::c_void, *const MsvcWstring) -> bool,
            /// slot 26, vtable byte offset `0x068` —
            /// `void GetLobby(const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&, std::function<void __cdecl(Crossplay::Lobby::DTO::LobbyDTO const &)>, std::function<void __cdecl(std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >)>)`
            /// callee pops 84 stack bytes.
            pub GetLobby: unsafe extern $abi fn(*mut core::ffi::c_void, *const MsvcWstring, MsvcFunction, MsvcFunction),
            /// slot 27, vtable byte offset `0x06c` —
            /// `void FindLobbies(const Crossplay::Lobby::DTO::LobbySearchCriteriaDTO&, std::function<void __cdecl(Crossplay::Lobby::DTO::LobbySearchResultDTO const &)>, std::function<void __cdecl(std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >)>)`
            /// callee pops 84 stack bytes.
            pub FindLobbies: unsafe extern $abi fn(*mut core::ffi::c_void, *const LobbySearchCriteriaDTO, MsvcFunction, MsvcFunction),
            /// slot 28, vtable byte offset `0x070` —
            /// `void CreateLobby(int, Crossplay::Lobby::Visibility, const std::unordered_map<std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >,std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >,std::hash<std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > >,std::equal_to<std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > >,std::allocator<std::pair<std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > const ,std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > > > >&, std::function<void __cdecl(Crossplay::Lobby::DTO::LobbyDTO const &)>, std::function<void __cdecl(std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >)>)`
            /// callee pops 92 stack bytes.
            pub CreateLobby__s28: unsafe extern $abi fn(*mut core::ffi::c_void, i32, Visibility, *const MsvcUnorderedMap32, MsvcFunction, MsvcFunction),
            /// slot 29, vtable byte offset `0x074` —
            /// `void CreateLobby(int, const std::unordered_map<std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >,std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >,std::hash<std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > >,std::equal_to<std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > >,std::allocator<std::pair<std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > const ,std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > > > >&, std::function<void __cdecl(Crossplay::Lobby::DTO::LobbyDTO const &)>, std::function<void __cdecl(std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >)>)`
            /// callee pops 88 stack bytes.
            pub CreateLobby__s29: unsafe extern $abi fn(*mut core::ffi::c_void, i32, *const MsvcUnorderedMap32, MsvcFunction, MsvcFunction),
            /// slot 30, vtable byte offset `0x078` —
            /// `void JoinLobby(const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&, const std::function<void __cdecl(Crossplay::Lobby::DTO::LobbyDTO const &)>&, const std::function<void __cdecl(int,std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > const &)>&)`
            /// callee pops 12 stack bytes.
            pub JoinLobby: unsafe extern $abi fn(*mut core::ffi::c_void, *const MsvcWstring, *const MsvcFunction, *const MsvcFunction),
            /// slot 31, vtable byte offset `0x07c` —
            /// `void SetJoinLobbyCallback(const std::function<void __cdecl(Crossplay::Lobby::DTO::JoinLobbyDTO const &,bool)>&)`
            /// callee pops 4 stack bytes.
            pub SetJoinLobbyCallback: unsafe extern $abi fn(*mut core::ffi::c_void, *const MsvcFunction),
            /// slot 32, vtable byte offset `0x080` —
            /// `void LeaveLobby(const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&, const std::function<void __cdecl(void)>&, const std::function<void __cdecl(int)>&)`
            /// callee pops 12 stack bytes.
            pub LeaveLobby__s32: unsafe extern $abi fn(*mut core::ffi::c_void, *const MsvcWstring, *const MsvcFunction, *const MsvcFunction),
            /// slot 33, vtable byte offset `0x084` —
            /// `void LeaveLobby(const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&)`
            /// callee pops 4 stack bytes.
            pub LeaveLobby__s33: unsafe extern $abi fn(*mut core::ffi::c_void, *const MsvcWstring),
            /// slot 34, vtable byte offset `0x088` —
            /// `void SetLeaveLobbyCallback(const std::function<void __cdecl(Crossplay::Lobby::DTO::LeaveLobbyDTO const &,bool)>&)`
            /// callee pops 4 stack bytes.
            pub SetLeaveLobbyCallback: unsafe extern $abi fn(*mut core::ffi::c_void, *const MsvcFunction),
            /// slot 35, vtable byte offset `0x08c` —
            /// `void SetLobbyAttribute(const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&, int, const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&, const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&)`
            /// callee pops 16 stack bytes.
            pub SetLobbyAttribute: unsafe extern $abi fn(*mut core::ffi::c_void, *const MsvcWstring, i32, *const MsvcWstring, *const MsvcWstring),
            /// slot 36, vtable byte offset `0x090` —
            /// `void SetLobbyAttributes(const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&, int, const std::unordered_map<std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >,std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >,std::hash<std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > >,std::equal_to<std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > >,std::allocator<std::pair<std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > const ,std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > > > >&)`
            /// callee pops 12 stack bytes.
            pub SetLobbyAttributes: unsafe extern $abi fn(*mut core::ffi::c_void, *const MsvcWstring, i32, *const MsvcUnorderedMap32),
            /// slot 37, vtable byte offset `0x094` —
            /// `void SetLobbyMaxMemberCount(const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&, int)`
            /// callee pops 8 stack bytes.
            pub SetLobbyMaxMemberCount: unsafe extern $abi fn(*mut core::ffi::c_void, *const MsvcWstring, i32),
            /// slot 38, vtable byte offset `0x098` —
            /// `void UpdateLobby(const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&, int, const std::unordered_map<std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >,std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >,std::hash<std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > >,std::equal_to<std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > >,std::allocator<std::pair<std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > const ,std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > > > >&, int, int, const std::function<void __cdecl(Crossplay::Lobby::DTO::UpdateLobbyResultDTO const &)>&, std::function<void __cdecl(std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >)>)`
            /// callee pops 64 stack bytes.
            pub UpdateLobby: unsafe extern $abi fn(*mut core::ffi::c_void, *const MsvcWstring, i32, *const MsvcUnorderedMap32, i32, i32, *const MsvcFunction, MsvcFunction),
            /// slot 39, vtable byte offset `0x09c` —
            /// `void SetUpdateLobbyCallback(const std::function<void __cdecl(Crossplay::Lobby::DTO::UpdateLobbyDTO const &,bool)>&)`
            /// callee pops 4 stack bytes.
            pub SetUpdateLobbyCallback: unsafe extern $abi fn(*mut core::ffi::c_void, *const MsvcFunction),
            /// slot 40, vtable byte offset `0x0a0` —
            /// `void LobbyCancelPendingRequests()`
            /// callee pops 0 stack bytes.
            pub LobbyCancelPendingRequests: unsafe extern $abi fn(*mut core::ffi::c_void),
            /// slot 41, vtable byte offset `0x0a4` —
            /// `void StartGame(const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&, bool, std::function<void __cdecl(std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > const &)>, std::function<void __cdecl(std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > const &)>)`
            /// callee pops 88 stack bytes.
            pub StartGame: unsafe extern $abi fn(*mut core::ffi::c_void, *const MsvcWstring, bool, MsvcFunction, MsvcFunction),
            /// slot 42, vtable byte offset `0x0a8` —
            /// `void CancelGameStart(const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&, std::function<void __cdecl(std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > const &)>, std::function<void __cdecl(std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > const &)>)`
            /// callee pops 84 stack bytes.
            pub CancelGameStart: unsafe extern $abi fn(*mut core::ffi::c_void, *const MsvcWstring, MsvcFunction, MsvcFunction),
            /// slot 43, vtable byte offset `0x0ac` —
            /// `void SendLobbyChat(const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&, const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&)`
            /// callee pops 8 stack bytes.
            pub SendLobbyChat: unsafe extern $abi fn(*mut core::ffi::c_void, *const MsvcWstring, *const MsvcWstring),
            /// slot 44, vtable byte offset `0x0b0` —
            /// `void SetLobbyMessageReceivedCallback(const std::function<void __cdecl(Crossplay::Lobby::DTO::LobbyChatMessageDTO const &,bool)>&)`
            /// callee pops 4 stack bytes.
            pub SetLobbyMessageReceivedCallback: unsafe extern $abi fn(*mut core::ffi::c_void, *const MsvcFunction),
            /// slot 45, vtable byte offset `0x0b4` —
            /// `void SetCrossplayEnabled(const std::function<void __cdecl(bool)>&)`
            /// callee pops 4 stack bytes.
            pub SetCrossplayEnabled: unsafe extern $abi fn(*mut core::ffi::c_void, *const MsvcFunction),
            /// slot 46, vtable byte offset `0x0b8` —
            /// `void SetCrossplayStatusUpdated(const std::function<void __cdecl(enum Crossplay::CrossplayStatus)>&)`
            /// callee pops 4 stack bytes.
            pub SetCrossplayStatusUpdated: unsafe extern $abi fn(*mut core::ffi::c_void, *const core::ffi::c_void),
            /// slot 47, vtable byte offset `0x0bc` —
            /// `void SetStats(const std::map<std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >,int,std::less<std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > >,std::allocator<std::pair<std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > const ,int> > >&, std::function<void __cdecl(bool)>, std::function<void __cdecl(std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >)>)`
            /// callee pops 84 stack bytes.
            pub SetStats: unsafe extern $abi fn(*mut core::ffi::c_void, *const MsvcMap8, MsvcFunction, MsvcFunction),
            /// slot 48, vtable byte offset `0x0c0` —
            /// `void GetStats(const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&, std::function<void __cdecl(std::map<std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >,int,std::less<std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > >,std::allocator<std::pair<std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > const ,int> > > const &)>, std::function<void __cdecl(std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >)>)`
            /// callee pops 84 stack bytes.
            pub GetStats: unsafe extern $abi fn(*mut core::ffi::c_void, *const MsvcWstring, MsvcFunction, MsvcFunction),
            /// slot 49, vtable byte offset `0x0c4` —
            /// `void GetGlobalLeaderboards(const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&, const int, std::function<void __cdecl(std::vector<Crossplay::Leaderboard::DTO::Entry,std::allocator<Crossplay::Leaderboard::DTO::Entry> > const &)>, std::function<void __cdecl(std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >)>)`
            /// callee pops 88 stack bytes.
            pub GetGlobalLeaderboards: unsafe extern $abi fn(*mut core::ffi::c_void, *const MsvcWstring, i32, MsvcFunction, MsvcFunction),
            /// slot 50, vtable byte offset `0x0c8` —
            /// `void GetLeaderboardsAroundPlayer(const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&, const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&, const int, std::function<void __cdecl(std::vector<Crossplay::Leaderboard::DTO::Entry,std::allocator<Crossplay::Leaderboard::DTO::Entry> > const &)>, std::function<void __cdecl(std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >)>)`
            /// callee pops 92 stack bytes.
            pub GetLeaderboardsAroundPlayer: unsafe extern $abi fn(*mut core::ffi::c_void, *const MsvcWstring, *const MsvcWstring, i32, MsvcFunction, MsvcFunction),
            /// slot 51, vtable byte offset `0x0cc` —
            /// `void CreateInvitation(const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&, const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&, std::function<void __cdecl(bool)>, std::function<void __cdecl(std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >)>)`
            /// callee pops 88 stack bytes.
            pub CreateInvitation: unsafe extern $abi fn(*mut core::ffi::c_void, *const MsvcWstring, *const MsvcWstring, MsvcFunction, MsvcFunction),
            /// slot 52, vtable byte offset `0x0d0` —
            /// `void AcceptInvitation(const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&, std::function<void __cdecl(std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > const &)>, std::function<void __cdecl(std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >)>)`
            /// callee pops 84 stack bytes.
            pub AcceptInvitation: unsafe extern $abi fn(*mut core::ffi::c_void, *const MsvcWstring, MsvcFunction, MsvcFunction),
            /// slot 53, vtable byte offset `0x0d4` —
            /// `std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > GetInvitationId() const`
            /// callee pops 4 stack bytes.
            pub GetInvitationId: unsafe extern $abi fn(*mut core::ffi::c_void, *mut MsvcWstring) -> *mut MsvcWstring,
            /// slot 54, vtable byte offset `0x0d8` —
            /// `void JoinChat(const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&, std::function<void __cdecl(Crossplay::Lobby::DTO::LobbyDTO const &)>, std::function<void __cdecl(std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > const &)>)`
            /// callee pops 84 stack bytes.
            pub JoinChat__s54: unsafe extern $abi fn(*mut core::ffi::c_void, *const MsvcWstring, MsvcFunction, MsvcFunction),
            /// slot 55, vtable byte offset `0x0dc` —
            /// `void JoinChat(const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&)`
            /// callee pops 4 stack bytes.
            pub JoinChat__s55: unsafe extern $abi fn(*mut core::ffi::c_void, *const MsvcWstring),
            /// slot 56, vtable byte offset `0x0e0` —
            /// `void SetChatJoinCallback(const std::function<void __cdecl(std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > const &,Crossplay::Lobby::DTO::JoinLobbyDTO const &,bool)>&)`
            /// callee pops 4 stack bytes.
            pub SetChatJoinCallback: unsafe extern $abi fn(*mut core::ffi::c_void, *const MsvcFunction),
            /// slot 57, vtable byte offset `0x0e4` —
            /// `void LeaveChat(const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&)`
            /// callee pops 4 stack bytes.
            pub LeaveChat: unsafe extern $abi fn(*mut core::ffi::c_void, *const MsvcWstring),
            /// slot 58, vtable byte offset `0x0e8` —
            /// `void SetChatLeaveCallback(const std::function<void __cdecl(std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > const &,Crossplay::Lobby::DTO::LeaveLobbyDTO const &,bool)>&)`
            /// callee pops 4 stack bytes.
            pub SetChatLeaveCallback: unsafe extern $abi fn(*mut core::ffi::c_void, *const MsvcFunction),
            /// slot 59, vtable byte offset `0x0ec` —
            /// `void SendChatMessage(const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&, const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&)`
            /// callee pops 8 stack bytes.
            pub SendChatMessage: unsafe extern $abi fn(*mut core::ffi::c_void, *const MsvcWstring, *const MsvcWstring),
            /// slot 60, vtable byte offset `0x0f0` —
            /// `void SetChatMessageReceivedCallback(const std::function<void __cdecl(std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > const &,Crossplay::Lobby::DTO::LobbyChatMessageDTO const &,bool)>&)`
            /// callee pops 4 stack bytes.
            pub SetChatMessageReceivedCallback: unsafe extern $abi fn(*mut core::ffi::c_void, *const MsvcFunction),
            /// slot 61, vtable byte offset `0x0f4` —
            /// `void SetP2PConnectionOpenedCallback(const std::function<void __cdecl(Crossplay::P2P::ICrossplayPlayer *)>&)`
            /// callee pops 4 stack bytes.
            pub SetP2PConnectionOpenedCallback: unsafe extern $abi fn(*mut core::ffi::c_void, *const MsvcFunction),
            /// slot 62, vtable byte offset `0x0f8` —
            /// `void SetP2PConnectionClosedCallback(const std::function<void __cdecl(Crossplay::P2P::ICrossplayPlayer *)>&)`
            /// callee pops 4 stack bytes.
            pub SetP2PConnectionClosedCallback: unsafe extern $abi fn(*mut core::ffi::c_void, *const MsvcFunction),
            /// slot 63, vtable byte offset `0x0fc` —
            /// `void SetP2PDataChannelOpenedCallback(const std::function<void __cdecl(Crossplay::P2P::ICrossplayPlayer *)>&)`
            /// callee pops 4 stack bytes.
            pub SetP2PDataChannelOpenedCallback: unsafe extern $abi fn(*mut core::ffi::c_void, *const MsvcFunction),
            /// slot 64, vtable byte offset `0x100` —
            /// `void SetP2PDataChannelClosedCallback(const std::function<void __cdecl(Crossplay::P2P::ICrossplayPlayer *)>&)`
            /// callee pops 4 stack bytes.
            pub SetP2PDataChannelClosedCallback: unsafe extern $abi fn(*mut core::ffi::c_void, *const MsvcFunction),
            /// slot 65, vtable byte offset `0x104` —
            /// `void SetReceivedP2PTextCallback(const std::function<void __cdecl(Crossplay::P2P::ICrossplayPlayer *,std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > const &)>&)`
            /// callee pops 4 stack bytes.
            pub SetReceivedP2PTextCallback: unsafe extern $abi fn(*mut core::ffi::c_void, *const MsvcFunction),
            /// slot 66, vtable byte offset `0x108` —
            /// `void SetReceivedP2PDataCallback(const std::function<void __cdecl(Crossplay::P2P::ICrossplayPlayer *,unsigned char const *,unsigned int)>&)`
            /// callee pops 4 stack bytes.
            pub SetReceivedP2PDataCallback: unsafe extern $abi fn(*mut core::ffi::c_void, *const MsvcFunction),
            /// slot 67, vtable byte offset `0x10c` —
            /// `void SetP2PConnectionFailedCallback(const std::function<void __cdecl(std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >,std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >)>&)`
            /// callee pops 4 stack bytes.
            pub SetP2PConnectionFailedCallback: unsafe extern $abi fn(*mut core::ffi::c_void, *const core::ffi::c_void),
            /// slot 68, vtable byte offset `0x110` —
            /// `void SetP2PTimeoutDuration(int)`
            /// callee pops 4 stack bytes.
            pub SetP2PTimeoutDuration: unsafe extern $abi fn(*mut core::ffi::c_void, i32),
            /// slot 69, vtable byte offset `0x114` —
            /// `void P2PStartConnection(const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&, const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&)`
            /// callee pops 8 stack bytes.
            pub P2PStartConnection: unsafe extern $abi fn(*mut core::ffi::c_void, *const MsvcWstring, *const MsvcWstring),
            /// slot 70, vtable byte offset `0x118` —
            /// `bool P2PSendToAll(unsigned char*, unsigned int)`
            /// callee pops 8 stack bytes.
            pub P2PSendToAll__s70: unsafe extern $abi fn(*mut core::ffi::c_void, *mut u8, u32) -> bool,
            /// slot 71, vtable byte offset `0x11c` —
            /// `bool P2PSendToAll(const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&)`
            /// callee pops 4 stack bytes.
            pub P2PSendToAll__s71: unsafe extern $abi fn(*mut core::ffi::c_void, *const MsvcWstring) -> bool,
            /// slot 72, vtable byte offset `0x120` —
            /// `bool P2PSend(Crossplay::P2P::ICrossplayPlayer*, unsigned char*, unsigned int)`
            /// callee pops 12 stack bytes.
            pub P2PSend__s72: unsafe extern $abi fn(*mut core::ffi::c_void, *mut ICrossplayPlayer, *mut u8, u32) -> bool,
            /// slot 73, vtable byte offset `0x124` —
            /// `bool P2PSend(Crossplay::P2P::ICrossplayPlayer*, const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&)`
            /// callee pops 8 stack bytes.
            pub P2PSend__s73: unsafe extern $abi fn(*mut core::ffi::c_void, *mut ICrossplayPlayer, *const MsvcWstring) -> bool,
            /// slot 74, vtable byte offset `0x128` —
            /// `void P2PCloseAll()`
            /// callee pops 0 stack bytes.
            pub P2PCloseAll: unsafe extern $abi fn(*mut core::ffi::c_void),
            /// slot 75, vtable byte offset `0x12c` —
            /// `void P2PClose(const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&)`
            /// callee pops 4 stack bytes.
            pub P2PClose__s75: unsafe extern $abi fn(*mut core::ffi::c_void, *const MsvcWstring),
            /// slot 76, vtable byte offset `0x130` —
            /// `void P2PClose(Crossplay::P2P::ICrossplayPlayer*)`
            /// callee pops 4 stack bytes.
            pub P2PClose__s76: unsafe extern $abi fn(*mut core::ffi::c_void, *mut ICrossplayPlayer),
            /// slot 77, vtable byte offset `0x134` —
            /// `std::vector<Crossplay::P2P::ICrossplayPlayer *,std::allocator<Crossplay::P2P::ICrossplayPlayer *> > P2PGetAllConnectedPlayers()`
            /// **NOT DERIVED** (unresolved type `std::vector<Crossplay::P2P::ICrossplayPlayer *,std::allocator<Crossplay::P2P::ICrossplayPlayer *> >`); do not call through this field.
            pub P2PGetAllConnectedPlayers: *const core::ffi::c_void,
            /// slot 78, vtable byte offset `0x138` —
            /// `void GetServiceAvailability(const std::function<void __cdecl(void)>&, std::function<void __cdecl(int)>)`
            /// callee pops 44 stack bytes.
            pub GetServiceAvailability: unsafe extern $abi fn(*mut core::ffi::c_void, *const MsvcFunction, MsvcFunction),
            /// slot 79, vtable byte offset `0x13c` —
            /// `bool IsConnectedToHub()`
            /// callee pops 0 stack bytes.
            pub IsConnectedToHub: unsafe extern $abi fn(*mut core::ffi::c_void) -> bool,
            /// slot 80, vtable byte offset `0x140` —
            /// `void ConnectToHub(const std::function<void __cdecl(bool)>&, std::function<void __cdecl(std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >)>)`
            /// callee pops 44 stack bytes.
            pub ConnectToHub: unsafe extern $abi fn(*mut core::ffi::c_void, *const MsvcFunction, MsvcFunction),
            /// slot 81, vtable byte offset `0x144` —
            /// `void DisconnectFromHub(const std::function<void __cdecl(void)>&)`
            /// callee pops 4 stack bytes.
            pub DisconnectFromHub: unsafe extern $abi fn(*mut core::ffi::c_void, *const MsvcFunction),
            /// slot 82, vtable byte offset `0x148` —
            /// `void CreateLocalPlayerLoopback()`
            /// callee pops 0 stack bytes.
            pub CreateLocalPlayerLoopback: unsafe extern $abi fn(*mut core::ffi::c_void),
            /// slot 83, vtable byte offset `0x14c` —
            /// `void EnableNetworkLogging(Crossplay::Logging::LogLevel)`
            /// callee pops 4 stack bytes.
            pub EnableNetworkLogging: unsafe extern $abi fn(*mut core::ffi::c_void, LogLevel),
            /// slot 84, vtable byte offset `0x150` —
            /// `void EnableInsaneNetworkLogging(std::basic_string<char,std::char_traits<char>,std::allocator<char> >)`
            /// callee pops 24 stack bytes.
            pub EnableInsaneNetworkLogging: unsafe extern $abi fn(*mut core::ffi::c_void, MsvcString),
            /// slot 85, vtable byte offset `0x154` —
            /// `void P2PEnableTcp(bool)`
            /// callee pops 4 stack bytes.
            pub P2PEnableTcp: unsafe extern $abi fn(*mut core::ffi::c_void, bool),
            /// slot 86, vtable byte offset `0x158` —
            /// `void SetUsername(const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&)`
            /// callee pops 4 stack bytes.
            pub SetUsername: unsafe extern $abi fn(*mut core::ffi::c_void, *const MsvcWstring),
            /// slot 87, vtable byte offset `0x15c` —
            /// `std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >& GetPlayerGuid()`
            /// callee pops 0 stack bytes.
            pub GetPlayerGuid: unsafe extern $abi fn(*mut core::ffi::c_void) -> *mut MsvcWstring,
            /// slot 88, vtable byte offset `0x160` —
            /// `void CreateTicket(const std::unordered_map<std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >,std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >,std::hash<std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > >,std::equal_to<std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > >,std::allocator<std::pair<std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > const ,std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > > > >&, const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&)`
            /// callee pops 8 stack bytes.
            pub CreateTicket: unsafe extern $abi fn(*mut core::ffi::c_void, *const MsvcUnorderedMap32, *const MsvcWstring),
            /// slot 89, vtable byte offset `0x164` —
            /// `void CreateTicketWithPlayerCount(const std::unordered_map<std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >,std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >,std::hash<std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > >,std::equal_to<std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > >,std::allocator<std::pair<std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > const ,std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > > > >&, int, const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&)`
            /// callee pops 12 stack bytes.
            pub CreateTicketWithPlayerCount: unsafe extern $abi fn(*mut core::ffi::c_void, *const MsvcUnorderedMap32, i32, *const MsvcWstring),
            /// slot 90, vtable byte offset `0x168` —
            /// `void CreateLobbyTicket(const std::unordered_map<std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >,std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >,std::hash<std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > >,std::equal_to<std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > >,std::allocator<std::pair<std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > const ,std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > > > >&, const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&, const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&)`
            /// callee pops 12 stack bytes.
            pub CreateLobbyTicket: unsafe extern $abi fn(*mut core::ffi::c_void, *const MsvcUnorderedMap32, *const MsvcWstring, *const MsvcWstring),
            /// slot 91, vtable byte offset `0x16c` —
            /// `void CreateLobbyAndSlotsTicket(const std::unordered_map<std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >,std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >,std::hash<std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > >,std::equal_to<std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > >,std::allocator<std::pair<std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > const ,std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> > > > >&, const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&, int, const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&)`
            /// callee pops 16 stack bytes.
            pub CreateLobbyAndSlotsTicket: unsafe extern $abi fn(*mut core::ffi::c_void, *const MsvcUnorderedMap32, *const MsvcWstring, i32, *const MsvcWstring),
            /// slot 92, vtable byte offset `0x170` —
            /// `void SetMatchFoundCallback(const std::function<void __cdecl(Crossplay::MatchMaking::DTO::MatchFoundDTO const &,bool)>&)`
            /// callee pops 4 stack bytes.
            pub SetMatchFoundCallback: unsafe extern $abi fn(*mut core::ffi::c_void, *const core::ffi::c_void),
            /// slot 93, vtable byte offset `0x174` —
            /// `void SetMatchMakingTicketEnqueuedCallback(const std::function<void __cdecl(Crossplay::MatchMaking::DTO::MatchMakingEnqueuedDTO const &,bool)>&)`
            /// callee pops 4 stack bytes.
            pub SetMatchMakingTicketEnqueuedCallback: unsafe extern $abi fn(*mut core::ffi::c_void, *const core::ffi::c_void),
            /// slot 94, vtable byte offset `0x178` —
            /// `void CancelTicket(const std::basic_string<wchar_t,std::char_traits<wchar_t>,std::allocator<wchar_t> >&)`
            /// callee pops 4 stack bytes.
            pub CancelTicket: unsafe extern $abi fn(*mut core::ffi::c_void, *const MsvcWstring),
            /// slot 95, vtable byte offset `0x17c` —
            /// `void Tick()`
            /// callee pops 0 stack bytes.
            pub Tick: unsafe extern $abi fn(*mut core::ffi::c_void),
        }
    };
}

#[cfg(target_arch = "x86")]
crossplay_vtables!("thiscall");
#[cfg(not(target_arch = "x86"))]
crossplay_vtables!("C");

/// Vtable slot count for [`ICrossPlayServiceVtable`]. **[measured]**
pub const ICROSSPLAYSERVICE_SLOTS: usize = 58;
/// x86 vtable size in bytes: `ICROSSPLAYSERVICE_SLOTS * 4`. **[measured]**
pub const ICROSSPLAYSERVICE_VTABLE_BYTES_X86: usize = 232;

const _: () = {
    assert!(core::mem::size_of::<ICrossPlayServiceVtable>() == ICROSSPLAYSERVICE_SLOTS * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossPlayServiceVtable, Init) == 0 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossPlayServiceVtable, SetServiceUrl) == 1 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossPlayServiceVtable, SetServiceErrorCallback) == 2 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossPlayServiceVtable, SetReliability) == 3 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossPlayServiceVtable, StartSession) == 4 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossPlayServiceVtable, StopSession) == 5 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossPlayServiceVtable, SetRenewTokenCallback) == 6 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossPlayServiceVtable, GetSessionStatus) == 7 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossPlayServiceVtable, GetCrossplayStatus) == 8 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossPlayServiceVtable, BlockUser) == 9 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossPlayServiceVtable, UnblockUser) == 10 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossPlayServiceVtable, IsUserBlocked) == 11 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossPlayServiceVtable, GetLobby) == 12 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossPlayServiceVtable, FindLobbies) == 13 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossPlayServiceVtable, CreateLobby) == 14 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossPlayServiceVtable, JoinLobby) == 15 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossPlayServiceVtable, SetJoinLobbyCallback) == 16 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossPlayServiceVtable, LeaveLobby) == 17 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossPlayServiceVtable, SetLeaveLobbyCallback) == 18 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossPlayServiceVtable, UpdateLobby) == 19 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossPlayServiceVtable, SetUpdateLobbyCallback) == 20 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossPlayServiceVtable, LobbyCancelPendingRequests) == 21 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossPlayServiceVtable, StartGame) == 22 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossPlayServiceVtable, CancelGameStart) == 23 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossPlayServiceVtable, SendLobbyChat) == 24 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossPlayServiceVtable, SetLobbyMessageReceivedCallback) == 25 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossPlayServiceVtable, SetCrossplayEnabled) == 26 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossPlayServiceVtable, SetStats) == 27 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossPlayServiceVtable, GetStats) == 28 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossPlayServiceVtable, GetGlobalLeaderboards) == 29 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossPlayServiceVtable, GetLeaderboardsAroundPlayer) == 30 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossPlayServiceVtable, CreateInvitation) == 31 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossPlayServiceVtable, AcceptInvitation) == 32 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossPlayServiceVtable, GetInvitationId) == 33 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossPlayServiceVtable, JoinChat) == 34 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossPlayServiceVtable, SetChatJoinCallback) == 35 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossPlayServiceVtable, LeaveChat) == 36 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossPlayServiceVtable, SetChatLeaveCallback) == 37 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossPlayServiceVtable, SendChatMessage) == 38 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossPlayServiceVtable, SetChatMessageReceivedCallback) == 39 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossPlayServiceVtable, SetP2PConnectionOpenedCallback) == 40 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossPlayServiceVtable, SetP2PConnectionClosedCallback) == 41 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossPlayServiceVtable, SetP2PDataChannelOpenedCallback) == 42 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossPlayServiceVtable, SetP2PDataChannelClosedCallback) == 43 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossPlayServiceVtable, SetReceivedP2PTextCallback) == 44 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossPlayServiceVtable, SetReceivedP2PDataCallback) == 45 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossPlayServiceVtable, SetP2PConnectionFailedCallback) == 46 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossPlayServiceVtable, SetP2PTimeoutDuration) == 47 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossPlayServiceVtable, P2PStartConnection) == 48 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossPlayServiceVtable, P2PSendToAll) == 49 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossPlayServiceVtable, P2PSend) == 50 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossPlayServiceVtable, P2PCloseAll) == 51 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossPlayServiceVtable, P2PClose) == 52 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossPlayServiceVtable, IsConnectedToHub) == 53 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossPlayServiceVtable, CreateLocalPlayerLoopback) == 54 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossPlayServiceVtable, SetUsername) == 55 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossPlayServiceVtable, GetPlayerGuid) == 56 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossPlayServiceVtable, Tick) == 57 * PTR_SIZE);
};
#[cfg(target_arch = "x86")]
const _: () = assert!(core::mem::size_of::<ICrossPlayServiceVtable>() == ICROSSPLAYSERVICE_VTABLE_BYTES_X86);

/// Vtable slot count for [`ICrossplayLoggerVtable`]. **[measured]**
pub const ICROSSPLAYLOGGER_SLOTS: usize = 4;
/// x86 vtable size in bytes: `ICROSSPLAYLOGGER_SLOTS * 4`. **[measured]**
pub const ICROSSPLAYLOGGER_VTABLE_BYTES_X86: usize = 16;

const _: () = {
    assert!(core::mem::size_of::<ICrossplayLoggerVtable>() == ICROSSPLAYLOGGER_SLOTS * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossplayLoggerVtable, vector_deleting_destructor) == 0 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossplayLoggerVtable, Register) == 1 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossplayLoggerVtable, UnregisterAll) == 2 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossplayLoggerVtable, Log) == 3 * PTR_SIZE);
};
#[cfg(target_arch = "x86")]
const _: () = assert!(core::mem::size_of::<ICrossplayLoggerVtable>() == ICROSSPLAYLOGGER_VTABLE_BYTES_X86);

/// Vtable slot count for [`ICrossplayPlayerVtable`]. **[measured]**
pub const ICROSSPLAYPLAYER_SLOTS: usize = 14;
/// x86 vtable size in bytes: `ICROSSPLAYPLAYER_SLOTS * 4`. **[measured]**
pub const ICROSSPLAYPLAYER_VTABLE_BYTES_X86: usize = 56;

const _: () = {
    assert!(core::mem::size_of::<ICrossplayPlayerVtable>() == ICROSSPLAYPLAYER_SLOTS * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossplayPlayerVtable, vector_deleting_destructor) == 0 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossplayPlayerVtable, Send__s1) == 1 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossplayPlayerVtable, Send__s2) == 2 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossplayPlayerVtable, MuteAudio) == 3 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossplayPlayerVtable, UnmuteAudio) == 4 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossplayPlayerVtable, CloseConnection) == 5 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossplayPlayerVtable, IsDataChannelOpen) == 6 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossplayPlayerVtable, SetReceivedTextCallback) == 7 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossplayPlayerVtable, SetReceivedDataCallback) == 8 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossplayPlayerVtable, SetDataChannelOpenedCallback) == 9 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossplayPlayerVtable, SetDataChannelClosedCallback) == 10 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossplayPlayerVtable, SetConnectionOpenedCallback) == 11 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossplayPlayerVtable, SetConnectionClosedCallback) == 12 * PTR_SIZE);
    assert!(core::mem::offset_of!(ICrossplayPlayerVtable, GetId) == 13 * PTR_SIZE);
};
#[cfg(target_arch = "x86")]
const _: () = assert!(core::mem::size_of::<ICrossplayPlayerVtable>() == ICROSSPLAYPLAYER_VTABLE_BYTES_X86);

/// Vtable slot count for [`INetworkClientVtable`]. **[measured]**
pub const INETWORKCLIENT_SLOTS: usize = 15;
/// x86 vtable size in bytes: `INETWORKCLIENT_SLOTS * 4`. **[measured]**
pub const INETWORKCLIENT_VTABLE_BYTES_X86: usize = 60;

const _: () = {
    assert!(core::mem::size_of::<INetworkClientVtable>() == INETWORKCLIENT_SLOTS * PTR_SIZE);
    assert!(core::mem::offset_of!(INetworkClientVtable, GetNetworkID) == 0 * PTR_SIZE);
    assert!(core::mem::offset_of!(INetworkClientVtable, GetNetworkDescriptor) == 1 * PTR_SIZE);
    assert!(core::mem::offset_of!(INetworkClientVtable, CreateNetwork) == 2 * PTR_SIZE);
    assert!(core::mem::offset_of!(INetworkClientVtable, JoinNetwork) == 3 * PTR_SIZE);
    assert!(core::mem::offset_of!(INetworkClientVtable, LeaveNetwork) == 4 * PTR_SIZE);
    assert!(core::mem::offset_of!(INetworkClientVtable, SendP2PData) == 5 * PTR_SIZE);
    assert!(core::mem::offset_of!(INetworkClientVtable, SendP2PDataToAll) == 6 * PTR_SIZE);
    assert!(core::mem::offset_of!(INetworkClientVtable, SendP2PTextData) == 7 * PTR_SIZE);
    assert!(core::mem::offset_of!(INetworkClientVtable, SendP2PTextDataToAll) == 8 * PTR_SIZE);
    assert!(core::mem::offset_of!(INetworkClientVtable, SetPlayerJoinedCallback) == 9 * PTR_SIZE);
    assert!(core::mem::offset_of!(INetworkClientVtable, SetPlayerLeftCallback) == 10 * PTR_SIZE);
    assert!(core::mem::offset_of!(INetworkClientVtable, SetP2PDataReceivedCallback) == 11 * PTR_SIZE);
    assert!(core::mem::offset_of!(INetworkClientVtable, SetP2PTextDataReceivedCallback) == 12 * PTR_SIZE);
    assert!(core::mem::offset_of!(INetworkClientVtable, SetFailedConnectionToNetworkCallback) == 13 * PTR_SIZE);
    assert!(core::mem::offset_of!(INetworkClientVtable, ResetCallbacks) == 14 * PTR_SIZE);
};
#[cfg(target_arch = "x86")]
const _: () = assert!(core::mem::size_of::<INetworkClientVtable>() == INETWORKCLIENT_VTABLE_BYTES_X86);

/// Not the shipped interface — see the module docs.
/// Vtable slot count for [`VendorICrossPlayServiceVtable`]. **[measured]**
pub const VENDOR_ICROSSPLAYSERVICE_SLOTS: usize = 96;
/// x86 vtable size in bytes: `VENDOR_ICROSSPLAYSERVICE_SLOTS * 4`. **[measured]**
pub const VENDOR_ICROSSPLAYSERVICE_VTABLE_BYTES_X86: usize = 384;

const _: () = {
    assert!(core::mem::size_of::<VendorICrossPlayServiceVtable>() == VENDOR_ICROSSPLAYSERVICE_SLOTS * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, vector_deleting_destructor) == 0 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, SetNew) == 1 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, SetDelete) == 2 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, Init) == 3 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, Shutdown) == 4 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, SetServiceUrl) == 5 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, SetServiceErrorCallback) == 6 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, SetReliability) == 7 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, SetP2PPacketOrderedDelivery) == 8 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, SetP2PMaxPacketLifetimeMs) == 9 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, SetP2PMaxPacketRetryAttempts) == 10 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, SetP2PAllowedPorts) == 11 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, P2PDisallowPorts) == 12 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, StartSession) == 13 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, StopSession) == 14 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, SetRenewTokenCallback) == 15 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, GetSessionStatus) == 16 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, GetCrossplayStatus) == 17 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, IsLoggedOn) == 18 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, IsLoggedOff) == 19 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, IsLoggingOnInProcess) == 20 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, BlockUser) == 21 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, BlockXuids) == 22 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, UnblockUser) == 23 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, GetBlockedUsers) == 24 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, IsUserBlocked) == 25 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, GetLobby) == 26 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, FindLobbies) == 27 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, CreateLobby__s28) == 28 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, CreateLobby__s29) == 29 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, JoinLobby) == 30 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, SetJoinLobbyCallback) == 31 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, LeaveLobby__s32) == 32 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, LeaveLobby__s33) == 33 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, SetLeaveLobbyCallback) == 34 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, SetLobbyAttribute) == 35 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, SetLobbyAttributes) == 36 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, SetLobbyMaxMemberCount) == 37 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, UpdateLobby) == 38 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, SetUpdateLobbyCallback) == 39 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, LobbyCancelPendingRequests) == 40 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, StartGame) == 41 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, CancelGameStart) == 42 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, SendLobbyChat) == 43 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, SetLobbyMessageReceivedCallback) == 44 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, SetCrossplayEnabled) == 45 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, SetCrossplayStatusUpdated) == 46 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, SetStats) == 47 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, GetStats) == 48 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, GetGlobalLeaderboards) == 49 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, GetLeaderboardsAroundPlayer) == 50 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, CreateInvitation) == 51 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, AcceptInvitation) == 52 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, GetInvitationId) == 53 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, JoinChat__s54) == 54 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, JoinChat__s55) == 55 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, SetChatJoinCallback) == 56 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, LeaveChat) == 57 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, SetChatLeaveCallback) == 58 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, SendChatMessage) == 59 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, SetChatMessageReceivedCallback) == 60 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, SetP2PConnectionOpenedCallback) == 61 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, SetP2PConnectionClosedCallback) == 62 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, SetP2PDataChannelOpenedCallback) == 63 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, SetP2PDataChannelClosedCallback) == 64 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, SetReceivedP2PTextCallback) == 65 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, SetReceivedP2PDataCallback) == 66 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, SetP2PConnectionFailedCallback) == 67 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, SetP2PTimeoutDuration) == 68 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, P2PStartConnection) == 69 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, P2PSendToAll__s70) == 70 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, P2PSendToAll__s71) == 71 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, P2PSend__s72) == 72 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, P2PSend__s73) == 73 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, P2PCloseAll) == 74 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, P2PClose__s75) == 75 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, P2PClose__s76) == 76 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, P2PGetAllConnectedPlayers) == 77 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, GetServiceAvailability) == 78 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, IsConnectedToHub) == 79 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, ConnectToHub) == 80 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, DisconnectFromHub) == 81 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, CreateLocalPlayerLoopback) == 82 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, EnableNetworkLogging) == 83 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, EnableInsaneNetworkLogging) == 84 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, P2PEnableTcp) == 85 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, SetUsername) == 86 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, GetPlayerGuid) == 87 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, CreateTicket) == 88 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, CreateTicketWithPlayerCount) == 89 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, CreateLobbyTicket) == 90 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, CreateLobbyAndSlotsTicket) == 91 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, SetMatchFoundCallback) == 92 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, SetMatchMakingTicketEnqueuedCallback) == 93 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, CancelTicket) == 94 * PTR_SIZE);
    assert!(core::mem::offset_of!(VendorICrossPlayServiceVtable, Tick) == 95 * PTR_SIZE);
};
#[cfg(target_arch = "x86")]
const _: () = assert!(core::mem::size_of::<VendorICrossPlayServiceVtable>() == VENDOR_ICROSSPLAYSERVICE_VTABLE_BYTES_X86);
