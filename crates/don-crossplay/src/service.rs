// SPDX-License-Identifier: GPL-3.0-or-later
//! A DoN-owned object that presents the shipped 58-slot
//! `CrossplayProxy::ICrossPlayService` vtable.
//!
//! [`crate::abi`] says how each slot is called. [`crate::local`] says what the
//! service does. This module is the join: one `#[repr(C)]` object whose first
//! word is a vtable pointer, 58 entry points with the measured signatures, and
//! the marshalling in [`crate::msvc`] between them.
//!
//! # Status — read this before quoting anything from here
//!
//! **Nothing in this file has been executed by `riseofnations.exe`.** No image
//! is installed into a game directory and no live process is touched. What the
//! host and PE32 tests establish is that calling every slot through a real
//! function-pointer table drives the backend correctly and produces DTOs whose
//! bytes match the measured layouts; what `crates/don-crossplay/dll`'s loader
//! adds is that the Windows loader accepts the image, the `__thiscall` boundary
//! is crossed with the stack balanced, and the `std::function` ownership
//! primitives in [`crate::func`] really run. Whether the shipped game is
//! satisfied by any of it is untested and needs the load-only run described in
//! `docs/tracks/crossplay-abi.md` §5.
//!
//! # The object layout is ours
//!
//! Retail's `CrossPlayService` is 2520 bytes and only one of its fields is
//! reachable from outside the DLL — the session status at `+0x930`, and only
//! through `GetSessionStatus` and `IsConnectedToHub`, both vtable slots. So no
//! external code indexes this object, and reproducing a layout that was never
//! transcribed would be decoration. What *is* load-bearing is that the vtable
//! pointer sits at offset 0, which `#[repr(C)]` and a test both pin.
//!
//! # Which slots do something
//!
//! Every slot falls in exactly one row and the counts sum to 58.
//!
//! | behaviour | n | slots |
//! |---|---|---|
//! | real | 28 | 0 `Init`, 2 `SetServiceErrorCallback`, 4 `StartSession`, 5 `StopSession`, 7 `GetSessionStatus`, 8 `GetCrossplayStatus`, 12 `GetLobby`, 13 `FindLobbies`, 14 `CreateLobby`, 15 `JoinLobby`, 17 `LeaveLobby`, 19 `UpdateLobby`, 21 `LobbyCancelPendingRequests`, 22 `StartGame`, 23 `CancelGameStart`, 33 `GetInvitationId`, 40–43, 45 `SetReceivedP2PDataCallback`, 49 `P2PSendToAll`, 50 `P2PSend`, 51 `P2PCloseAll`, 53 `IsConnectedToHub`, 55 `SetUsername`, 56 `GetPlayerGuid`, 57 `Tick` |
//! | retained callback, never invoked | 11 | 6, 16, 18, 20, 25, 26, 35, 37, 39, 44, 46 — see [`crate::local::Backend::tick`] for why the lobby trio is here |
//! | retained scalar | 2 | 3 `SetReliability`, 47 `SetP2PTimeoutDuration` |
//! | reproduced inert | 7 | 1, 9, 10, 11, 48, 52, 54 — the shipped bodies are no-ops or a constant `false` **[measured]** |
//! | fail closed | 10 | 24, 27–32, 34, 36, 38 — the caller's error callback runs with [`crate::local::error::NOT_AVAILABLE`] |
//!
//! Slot 44 `SetReceivedP2PTextCallback` is retained and its delivery path is
//! written, but nothing in [`crate::local::Directory`] produces a text payload:
//! retail's text channel is not modelled and DoN's turn traffic is binary.

// Every thunk is `fn<M: GuestMem>` so all 58 have one shape, including the ones
// whose shipped body does nothing and therefore never mention `M`; and
// `LocalPlayer::release` takes `Box<Self>` on purpose, to consume the allocation
// the service handed out rather than leave a dangling peer object behind.
#![allow(clippy::extra_unused_type_parameters, clippy::boxed_local)]

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::ffi::c_void;

use crate::abi::{
    CrossplayStatus, ICrossPlayService, ICrossPlayServiceVtable, ICrossplayPlayer,
    ICrossplayPlayerVtable, LobbySearchCriteriaDTO, MsvcFunction, MsvcMap8, MsvcUnorderedMap32,
    MsvcWstring, SessionStatus, Visibility, CROSSPLAY_STATUS_ENABLED,
};
use crate::func::{self, OwnedFunction};
use crate::local::{
    AsyncDirectory, Attributes, Backend, Directory, Emission, Notice, Outcome, ReqId,
};
use crate::msvc::{self, Gp, GuestMem, Scratch};

/// How many dispatch records the service keeps. Bounded, so a long session
/// cannot grow without limit; the journal exists to make the eventual load-only
/// run self-describing, in the same spirit as `netsys-shim`'s trace.
pub const JOURNAL_CAPACITY: usize = 512;

/// Largest `P2PSendToAll`/`P2PSend` payload this service copies out of guest
/// memory. A retail lockstep turn packet is orders of magnitude below this; the
/// bound exists so a corrupt length is a refusal, not an allocation.
/// **[DoN policy]**
pub const MAX_PACKET_BYTES: u32 = 1 << 20;

/// One recorded outbound dispatch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dispatch {
    /// The interface method whose callback was reached.
    pub slot: &'static str,
    /// `true` when a non-empty `std::function` was actually invoked, `false`
    /// when no callback was installed and the built DTO was discarded.
    pub invoked: bool,
    /// Guest address of the argument that was built, or 0.
    pub argument: Gp,
}

/// The `std::function` pair a pending request must answer on.
///
/// Both arrive **by value** and are therefore this object's to own and to
/// destroy — see [`crate::func`] for the two measurements that establish it —
/// so they are held as [`OwnedFunction`], whose address survives being inserted
/// into `pending` and removed again.
struct Pending {
    slot: &'static str,
    ok: OwnedFunction,
    err: OwnedFunction,
}

// How a `std::function` the game installed reaches this object.
//
// Retail hands these over by const reference for every `Set*Callback` setter
// and **by value** for `CreateLobby`, `GetLobby`, `FindLobbies`, `StartGame`,
// `CancelGameStart`, `UpdateLobby`'s eighth argument and the stats/leaderboard
// family — a distinction measured by the `ret imm16` check in `gen/gen_abi.py`
// (54 slots agree, 0 disagree).
//
// Both cases go through `crate::func`, which runs the measured `_Copy` and
// `_Delete_this` rather than copying the 40 bytes: a bitwise copy aliases
// storage the caller still owns, and for an inline target — the common
// captureless- and small-lambda case, where `target == &self` — it leaves a
// pointer into a stack frame that is gone by the time `Tick` answers the
// request. That is a use-after-free, not a leak, which is why this is not
// optional in a DLL the game calls.
//
// On any target that is not x86 there is no guest code to run, so `crate::func`
// degrades to the bitwise copy the host tests have always exercised.

/// A `std::function` holding nothing, for tests that drive a slot without
/// installing a callback.
#[cfg(test)]
const fn empty_function() -> MsvcFunction {
    func::empty()
}

fn gp_of<T>(p: *const T) -> Gp {
    p as *const u8 as usize as Gp
}

/// Copy a **by-reference** `std::function` argument, or produce an empty one.
///
/// The caller keeps owning `f`, so this is `_Copy` and never a take. A null
/// pointer is a caller that installed nothing.
///
/// # Safety
///
/// `f`, when non-null, must point at a live 40-byte `std::function`.
unsafe fn retained_or_empty(f: *const MsvcFunction) -> OwnedFunction {
    if f.is_null() {
        return OwnedFunction::empty();
    }
    // SAFETY: the caller's guarantee.
    unsafe { OwnedFunction::retain(&*f) }
}

// ---------------------------------------------------------------------------
// Retained-callback slots.
// ---------------------------------------------------------------------------

/// Index into [`LocalCrossPlayService::callbacks`]. One per interface slot that
/// installs a `std::function` on the service.
pub mod cb {
    pub const SERVICE_ERROR: usize = 0;
    pub const RENEW_TOKEN: usize = 1;
    pub const JOIN_LOBBY_BROADCAST: usize = 2;
    pub const LEAVE_LOBBY_BROADCAST: usize = 3;
    pub const UPDATE_LOBBY_BROADCAST: usize = 4;
    pub const LOBBY_MESSAGE: usize = 5;
    pub const CROSSPLAY_ENABLED: usize = 6;
    pub const CHAT_JOIN: usize = 7;
    pub const CHAT_LEAVE: usize = 8;
    pub const CHAT_MESSAGE: usize = 9;
    pub const P2P_CONNECTION_OPENED: usize = 10;
    pub const P2P_CONNECTION_CLOSED: usize = 11;
    pub const P2P_DATA_CHANNEL_OPENED: usize = 12;
    pub const P2P_DATA_CHANNEL_CLOSED: usize = 13;
    pub const P2P_TEXT: usize = 14;
    pub const P2P_DATA: usize = 15;
    pub const P2P_CONNECTION_FAILED: usize = 16;
    pub const COUNT: usize = 17;

    /// Human names, index-aligned with the constants above.
    pub const NAMES: [&str; COUNT] = [
        "SetServiceErrorCallback",
        "SetRenewTokenCallback",
        "SetJoinLobbyCallback",
        "SetLeaveLobbyCallback",
        "SetUpdateLobbyCallback",
        "SetLobbyMessageReceivedCallback",
        "SetCrossplayEnabled",
        "SetChatJoinCallback",
        "SetChatLeaveCallback",
        "SetChatMessageReceivedCallback",
        "SetP2PConnectionOpenedCallback",
        "SetP2PConnectionClosedCallback",
        "SetP2PDataChannelOpenedCallback",
        "SetP2PDataChannelClosedCallback",
        "SetReceivedP2PTextCallback",
        "SetReceivedP2PDataCallback",
        "SetP2PConnectionFailedCallback",
    ];
}

// ---------------------------------------------------------------------------
// Invoking a retained std::function.
// ---------------------------------------------------------------------------

/// `std::_Func_base`'s vtable slot for `_Do_call`. See [`crate::func`] for the
/// measurement of all six slots. **[measured]**
pub const FUNC_DO_CALL_SLOT: usize = func::FUNC_DO_CALL_SLOT;

/// The `std::function` payload pointer, at `+0x24` of the 40-byte object.
/// **[measured — `SetServiceErrorCallback` at `0x100145c0` reads exactly it]**
pub const FUNC_TARGET_OFFSET: usize = 0x24;
const _: () = assert!(core::mem::offset_of!(MsvcFunction, target) == FUNC_TARGET_OFFSET);

/// Call a retained `std::function` with up to three already-marshalled 4-byte
/// arguments, pushed right to left as `__thiscall` requires.
///
/// Returns `false` when nothing is installed. On any target that is not x86 this
/// is a no-op returning `false`, because a `u32` is not a callable address
/// there — the host tests exercise everything up to and including the DTO bytes
/// and stop at this line.
///
/// # Safety
///
/// The caller must have marshalled arguments matching the installed function's
/// real signature. Nothing here can check that; `abi.rs` records each slot's
/// callback signature and every call site below cites it.
unsafe fn invoke_target(target: u32, args: &[u32]) -> bool {
    if target == 0 || args.len() > 3 {
        return false;
    }
    #[cfg(target_arch = "x86")]
    {
        let this = target as usize as *mut c_void;
        // SAFETY: a non-zero `target` points at a `_Func_impl` whose first word
        // is its vtable, per the measured layout above.
        let vtable: *const *const c_void = unsafe { *this.cast::<*const *const c_void>() };
        if vtable.is_null() {
            return false;
        }
        let entry: *const c_void = unsafe { *vtable.add(FUNC_DO_CALL_SLOT) };
        unsafe {
            match args.len() {
                0 => {
                    let f: unsafe extern "thiscall" fn(*mut c_void) = core::mem::transmute(entry);
                    f(this);
                }
                1 => {
                    let f: unsafe extern "thiscall" fn(*mut c_void, u32) =
                        core::mem::transmute(entry);
                    f(this, args[0]);
                }
                2 => {
                    let f: unsafe extern "thiscall" fn(*mut c_void, u32, u32) =
                        core::mem::transmute(entry);
                    f(this, args[0], args[1]);
                }
                _ => {
                    let f: unsafe extern "thiscall" fn(*mut c_void, u32, u32, u32) =
                        core::mem::transmute(entry);
                    f(this, args[0], args[1], args[2]);
                }
            }
        }
        true
    }
    #[cfg(not(target_arch = "x86"))]
    {
        let _ = args;
        false
    }
}

// ---------------------------------------------------------------------------
// The per-peer ICrossplayPlayer object.
// ---------------------------------------------------------------------------

/// One remote peer, as `Crossplay::P2P::ICrossplayPlayer`.
///
/// `SetReceivedP2PDataCallback`'s signature is
/// `void(ICrossplayPlayer*, const unsigned char*, unsigned int)`, so every
/// inbound packet arrives attached to one of these, and `CrossplayNetLibSys`
/// keys its peers on the pointer. The object is owned by the service and lives
/// exactly as long as the peer is open.
#[repr(C)]
pub struct LocalPlayer {
    /// **Must stay first.** The only thing retail knows about this object.
    #[allow(dead_code)]
    vftable: *const ICrossplayPlayerVtable,
    /// Guest address of a persistent `wstring` holding the peer id, which
    /// `GetId` returns by reference.
    id_at: Gp,
    id_blocks: Vec<(Gp, u32)>,
    user_id: String,
    open: bool,
}

impl LocalPlayer {
    fn new<M: GuestMem>(mem: &mut M, user_id: &str) -> Option<Box<Self>> {
        let mut scratch = Scratch::new(mem);
        let id_at = scratch.wstring_ref(user_id)?;
        Some(Box::new(Self {
            vftable: &PLAYER_VTABLE,
            id_at,
            id_blocks: scratch.into_blocks(),
            user_id: user_id.to_string(),
            open: true,
        }))
    }

    fn release<M: GuestMem>(mut self: Box<Self>, mem: &mut M) {
        msvc::free_blocks(mem, core::mem::take(&mut self.id_blocks));
    }

    pub fn user_id(&self) -> &str {
        &self.user_id
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// The pointer to hand to a game callback.
    pub fn as_ptr(&self) -> *mut ICrossplayPlayer {
        self as *const Self as *mut ICrossplayPlayer
    }

    /// # Safety
    ///
    /// `this` must be a pointer this crate produced from a [`LocalPlayer`].
    unsafe fn from_ptr<'a>(this: *mut ICrossplayPlayer) -> Option<&'a mut Self> {
        if this.is_null() {
            return None;
        }
        // SAFETY: the only `ICrossplayPlayer*` this crate hands out is a
        // `LocalPlayer`, whose vtable pointer is at offset 0 by `repr(C)`.
        Some(unsafe { &mut *(this as *mut Self) })
    }
}

// The player vtable and its thunks. Written once and expanded under the two
// calling conventions, because `extern "thiscall"` exists only on x86 while the
// slot-order assertions in `abi.rs` are meaningful on both.
macro_rules! player_impl {
    ($abi:literal) => {
        use super::*;

        unsafe extern $abi fn dtor(_this: *mut ICrossplayPlayer, _flags: u32) -> *mut c_void {
            // The service owns every peer object; retail never deletes one.
            core::ptr::null_mut()
        }
        unsafe extern $abi fn send_bytes(
            _this: *mut ICrossplayPlayer,
            _data: *mut u8,
            _len: i32,
        ) -> bool {
            // Refused: a packet has to go through the service so the directory
            // routes it, and this object does not carry the service pointer.
            // `P2PSend(player, data, len)` is the path that works.
            false
        }
        unsafe extern $abi fn send_text(
            _this: *mut ICrossplayPlayer,
            _text: *const MsvcWstring,
        ) -> bool {
            false
        }
        unsafe extern $abi fn mute(_this: *mut ICrossplayPlayer) {}
        unsafe extern $abi fn unmute(_this: *mut ICrossplayPlayer) {}
        unsafe extern $abi fn close(this: *mut ICrossplayPlayer) {
            if let Some(p) = unsafe { LocalPlayer::from_ptr(this) } {
                p.open = false;
            }
        }
        unsafe extern $abi fn is_open(this: *mut ICrossplayPlayer) -> bool {
            match unsafe { LocalPlayer::from_ptr(this) } {
                Some(p) => p.open,
                None => false,
            }
        }
        unsafe extern $abi fn set_fn(_this: *mut ICrossplayPlayer, _f: *const MsvcFunction) {}
        unsafe extern $abi fn set_fn_opaque(_this: *mut ICrossplayPlayer, _f: *const c_void) {}
        unsafe extern $abi fn get_id(this: *mut ICrossplayPlayer) -> *const MsvcWstring {
            match unsafe { LocalPlayer::from_ptr(this) } {
                Some(p) => p.id_at as usize as *const MsvcWstring,
                None => core::ptr::null(),
            }
        }

        pub(super) static VTABLE: ICrossplayPlayerVtable = ICrossplayPlayerVtable {
            vector_deleting_destructor: dtor,
            Send__s1: send_bytes,
            Send__s2: send_text,
            MuteAudio: mute,
            UnmuteAudio: unmute,
            CloseConnection: close,
            IsDataChannelOpen: is_open,
            SetReceivedTextCallback: set_fn,
            SetReceivedDataCallback: set_fn_opaque,
            SetDataChannelOpenedCallback: set_fn,
            SetDataChannelClosedCallback: set_fn,
            SetConnectionOpenedCallback: set_fn,
            SetConnectionClosedCallback: set_fn,
            GetId: get_id,
        };
    };
}

#[cfg(target_arch = "x86")]
mod player_thunks {
    player_impl!("thiscall");
}
#[cfg(not(target_arch = "x86"))]
mod player_thunks {
    player_impl!("C");
}
use player_thunks::VTABLE as PLAYER_VTABLE;

// ---------------------------------------------------------------------------
// The shared directory handle.
// ---------------------------------------------------------------------------

/// How a service instance reaches the lobby directory.
///
/// Deliberately reference-counting-free: this crate is `alloc`-only and the
/// shipped service is pumped from one thread (`Tick` from
/// `NetDaemon::process_all` **[measured]**). A game process holds one service
/// that owns its directory outright; the tests put two services on one directory
/// the test owns, which is what proves the lobby path.
pub enum DirectoryRef {
    /// This service owns the directory.
    Owned(Box<Directory>),
    /// The directory belongs to the caller.
    ///
    /// # Safety invariant
    ///
    /// The pointee must outlive this service and must not be mutated through
    /// another path while a slot call is in progress.
    Borrowed(*mut Directory),
    /// A DoN-owned asynchronous directory transport. It is pumped only from
    /// slot 57 and does not alter the published 58-slot ABI.
    Remote(Box<dyn AsyncDirectory>),
}

// ---------------------------------------------------------------------------
// The service object.
// ---------------------------------------------------------------------------

/// A DoN-owned `ICrossPlayService`.
///
/// `M` is the address space marshalled DTOs live in: `msvc::RawMem` on the
/// i686 target, [`msvc::ArenaMem`] in the host tests. Both run the same layout
/// code, and the vtable is monomorphised per `M`.
#[repr(C)]
pub struct LocalCrossPlayService<M: GuestMem + 'static> {
    /// **Must stay first.** The only thing retail knows about this object.
    #[allow(dead_code)]
    vftable: *const ICrossPlayServiceVtable,
    mem: M,
    backend: Backend,
    directory: DirectoryRef,
    pending: BTreeMap<ReqId, Pending>,
    peers: BTreeMap<String, Box<LocalPlayer>>,
    journal: Vec<Dispatch>,
    callbacks: [OwnedFunction; cb::COUNT],
    /// Persistent storage behind `GetPlayerGuid`'s returned `wstring&`.
    guid_at: Gp,
    guid_blocks: Vec<(Gp, u32)>,
    guid_text: String,
}

impl<M: GuestMem + 'static> LocalCrossPlayService<M> {
    /// The vtable for this instantiation. A `const` of reference type, so the
    /// table is promoted to a `'static` allocation per monomorphisation exactly
    /// as a C++ vtable is one object per class.
    const VTABLE: &'static ICrossPlayServiceVtable = &ICrossPlayServiceVtable {
        Init: thunks::init::<M>,
        SetServiceUrl: thunks::noop_wstring::<M>,
        SetServiceErrorCallback: thunks::retain_service_error::<M>,
        SetReliability: thunks::set_reliability::<M>,
        StartSession: thunks::start_session::<M>,
        StopSession: thunks::stop_session::<M>,
        SetRenewTokenCallback: thunks::retain_renew_token::<M>,
        GetSessionStatus: thunks::get_session_status::<M>,
        GetCrossplayStatus: thunks::get_crossplay_status::<M>,
        BlockUser: thunks::noop_wstring::<M>,
        UnblockUser: thunks::noop_wstring::<M>,
        IsUserBlocked: thunks::constant_false::<M>,
        GetLobby: thunks::get_lobby::<M>,
        FindLobbies: thunks::find_lobbies::<M>,
        CreateLobby: thunks::create_lobby::<M>,
        JoinLobby: thunks::join_lobby::<M>,
        SetJoinLobbyCallback: thunks::retain_join_broadcast::<M>,
        LeaveLobby: thunks::leave_lobby::<M>,
        SetLeaveLobbyCallback: thunks::retain_leave_broadcast::<M>,
        UpdateLobby: thunks::update_lobby::<M>,
        SetUpdateLobbyCallback: thunks::retain_update_broadcast::<M>,
        LobbyCancelPendingRequests: thunks::lobby_cancel_pending::<M>,
        StartGame: thunks::start_game::<M>,
        CancelGameStart: thunks::cancel_game_start::<M>,
        SendLobbyChat: thunks::send_lobby_chat::<M>,
        SetLobbyMessageReceivedCallback: thunks::retain_lobby_message::<M>,
        SetCrossplayEnabled: thunks::retain_crossplay_enabled::<M>,
        SetStats: thunks::set_stats::<M>,
        GetStats: thunks::get_stats::<M>,
        GetGlobalLeaderboards: thunks::get_global_leaderboards::<M>,
        GetLeaderboardsAroundPlayer: thunks::get_leaderboards_around_player::<M>,
        CreateInvitation: thunks::create_invitation::<M>,
        AcceptInvitation: thunks::accept_invitation::<M>,
        GetInvitationId: thunks::get_invitation_id::<M>,
        JoinChat: thunks::join_chat::<M>,
        SetChatJoinCallback: thunks::retain_chat_join::<M>,
        LeaveChat: thunks::leave_chat::<M>,
        SetChatLeaveCallback: thunks::retain_chat_leave::<M>,
        SendChatMessage: thunks::send_chat_message::<M>,
        SetChatMessageReceivedCallback: thunks::retain_chat_message::<M>,
        SetP2PConnectionOpenedCallback: thunks::retain_p2p_connection_opened::<M>,
        SetP2PConnectionClosedCallback: thunks::retain_p2p_connection_closed::<M>,
        SetP2PDataChannelOpenedCallback: thunks::retain_p2p_channel_opened::<M>,
        SetP2PDataChannelClosedCallback: thunks::retain_p2p_channel_closed::<M>,
        SetReceivedP2PTextCallback: thunks::retain_p2p_text::<M>,
        SetReceivedP2PDataCallback: thunks::retain_p2p_data::<M>,
        SetP2PConnectionFailedCallback: thunks::retain_p2p_failed::<M>,
        SetP2PTimeoutDuration: thunks::set_p2p_timeout::<M>,
        P2PStartConnection: thunks::noop_two_wstrings::<M>,
        P2PSendToAll: thunks::p2p_send_to_all::<M>,
        P2PSend: thunks::p2p_send::<M>,
        P2PCloseAll: thunks::p2p_close_all::<M>,
        P2PClose: thunks::noop_wstring::<M>,
        IsConnectedToHub: thunks::is_connected_to_hub::<M>,
        CreateLocalPlayerLoopback: thunks::noop_void::<M>,
        SetUsername: thunks::set_username::<M>,
        GetPlayerGuid: thunks::get_player_guid::<M>,
        Tick: thunks::tick::<M>,
    };

    /// The vtable this object publishes.
    ///
    /// `#[inline(never)]`, and the **only** place `Self::VTABLE` is named.
    /// `VTABLE` is an associated `const`, so every separate `&`-of-it is its own
    /// promoted allocation; with two use sites the object published one table
    /// and this accessor returned another with identical contents at a
    /// different address. Harmless to retail, which never compares them, and a
    /// lie in every diagnostic that does. Measured on `i686-pc-windows-msvc`,
    /// where the host build happened to merge them and the target build did not.
    #[inline(never)]
    pub fn vtable() -> &'static ICrossPlayServiceVtable {
        Self::VTABLE
    }

    /// A service instance with its own private lobby directory.
    pub fn new(mem: M, user_id: &str, user_name: &str) -> Box<Self> {
        Self::with_directory(mem, user_id, user_name, DirectoryRef::Owned(Box::default()))
    }

    /// A service instance sharing a directory the caller owns.
    ///
    /// # Safety
    ///
    /// `directory` must outlive the returned service and must not be mutated
    /// through another path while a slot call is in progress.
    pub unsafe fn joined(
        mem: M,
        user_id: &str,
        user_name: &str,
        directory: *mut Directory,
    ) -> Box<Self> {
        Self::with_directory(mem, user_id, user_name, DirectoryRef::Borrowed(directory))
    }

    /// A service instance using a non-blocking DoN-owned directory transport.
    /// Slot calls still queue requests; the transport is submitted and polled
    /// only by `Tick`, preserving the callback boundary of the local service.
    pub fn remote(
        mem: M,
        user_id: &str,
        user_name: &str,
        directory: Box<dyn AsyncDirectory>,
    ) -> Box<Self> {
        Self::with_directory(mem, user_id, user_name, DirectoryRef::Remote(directory))
    }

    fn with_directory(mem: M, user_id: &str, user_name: &str, directory: DirectoryRef) -> Box<Self> {
        Box::new(Self {
            vftable: Self::vtable(),
            mem,
            backend: Backend::new(user_id, user_name),
            directory,
            pending: BTreeMap::new(),
            peers: BTreeMap::new(),
            journal: Vec::new(),
            callbacks: core::array::from_fn(|_| OwnedFunction::empty()),
            guid_at: 0,
            guid_blocks: Vec::new(),
            guid_text: String::new(),
        })
    }

    /// The pointer `Crossplay::Service()` would return.
    pub fn as_service(&self) -> *mut ICrossPlayService {
        self as *const Self as *mut ICrossPlayService
    }

    pub fn backend(&self) -> &Backend {
        &self.backend
    }

    pub fn backend_mut(&mut self) -> &mut Backend {
        &mut self.backend
    }

    pub fn journal(&self) -> &[Dispatch] {
        &self.journal
    }

    pub fn peer(&self, user_id: &str) -> Option<&LocalPlayer> {
        self.peers.get(user_id).map(|b| &**b)
    }

    /// The address space this service marshals in.
    ///
    /// Anything building arguments to pass *into* a slot has to use the same
    /// one. On the i686 target that is the process heap and the distinction is
    /// invisible; on the host it is the arena, and a caller that allocated
    /// elsewhere would hand over an address this service cannot read.
    pub fn memory_mut(&mut self) -> &mut M {
        &mut self.mem
    }

    /// Whether the game has installed the callback at `index` (see [`cb`]).
    pub fn callback_installed(&self, index: usize) -> bool {
        self.callbacks
            .get(index)
            .map(OwnedFunction::installed)
            .unwrap_or(false)
    }

    /// # Safety
    ///
    /// `this` must be a pointer this crate produced from a
    /// `Box<LocalCrossPlayService<M>>` of the same `M`.
    unsafe fn of<'a>(this: *mut ICrossPlayService) -> Option<&'a mut Self> {
        if this.is_null() {
            return None;
        }
        // SAFETY: the vtable pointer is at offset 0 by `repr(C)`, so the two
        // pointers have the same address; the caller guarantees provenance.
        Some(unsafe { &mut *(this as *mut Self) })
    }

    fn record(&mut self, slot: &'static str, invoked: bool, argument: Gp) {
        if self.journal.len() >= JOURNAL_CAPACITY {
            self.journal.remove(0);
        }
        self.journal.push(Dispatch {
            slot,
            invoked,
            argument,
        });
    }

    /// Park a request's two answer callbacks until `Tick` resolves it.
    ///
    /// Each thunk builds its own [`OwnedFunction`], because whether a callback
    /// is taken ([`OwnedFunction::adopt`], by value) or copied
    /// ([`OwnedFunction::retain`], by reference) is a per-slot fact from the
    /// PDB signature, not something this function can infer.
    fn remember(&mut self, req: ReqId, slot: &'static str, ok: OwnedFunction, err: OwnedFunction) {
        self.pending.insert(req, Pending { slot, ok, err });
    }

    /// Take an independent copy of a setter's by-reference callback, releasing
    /// whatever occupied that slot before.
    fn retain(&mut self, index: usize, f: *const MsvcFunction) {
        if f.is_null() {
            return;
        }
        // SAFETY: retail passes a live 40-byte `std::function` it keeps owning,
        // so the discipline is `_Copy`, never a take.
        self.callbacks[index] = unsafe { OwnedFunction::retain(&*f) };
    }

    /// Refresh the persistent `wstring` behind `GetPlayerGuid`.
    fn sync_guid(&mut self) {
        let want = self.backend.player_guid().to_string();
        if want == self.guid_text && self.guid_at != 0 {
            return;
        }
        let old = core::mem::take(&mut self.guid_blocks);
        msvc::free_blocks(&mut self.mem, old);
        let mut scratch = Scratch::new(&mut self.mem);
        match scratch.wstring_ref(&want) {
            Some(at) => {
                let blocks = scratch.into_blocks();
                self.guid_at = at;
                self.guid_blocks = blocks;
                self.guid_text = want;
            }
            None => {
                scratch.release();
                self.guid_at = 0;
                self.guid_text = String::new();
            }
        }
    }

    /// Drain one `Tick` worth of backend work and run the callbacks it implies.
    fn pump(&mut self) {
        let emissions = match &mut self.directory {
            DirectoryRef::Owned(directory) => self.backend.tick(directory),
            DirectoryRef::Borrowed(pointer) => {
                // SAFETY: the invariant documented on `DirectoryRef::Borrowed`.
                let directory = unsafe { &mut **pointer };
                self.backend.tick(directory)
            }
            DirectoryRef::Remote(directory) => self.backend.tick_remote(&mut **directory),
        };
        self.sync_guid();
        for emission in emissions {
            match emission {
                Emission::Completion(req, outcome) => self.complete(req, outcome),
                Emission::Notice(notice) => self.notify(notice),
            }
        }
        while let Some(message) = self.backend.take_service_error() {
            let slot = self.callbacks[cb::SERVICE_ERROR].target();
            let mut scratch = Scratch::new(&mut self.mem);
            let arg = scratch.wstring_ref(&message).unwrap_or(0);
            // SAFETY: `void(wstring)` is a by-value parameter, so `_Do_call`
            // takes an rvalue reference — one pointer. **[measured: the
            // `_Do_call` for that specialisation is a `ret 4` body]**
            let invoked = arg != 0 && unsafe { invoke_target(slot, &[arg]) };
            scratch.release();
            self.record(cb::NAMES[cb::SERVICE_ERROR], invoked, arg);
        }
    }

    fn complete(&mut self, req: ReqId, outcome: Outcome) {
        let Some(pending) = self.pending.remove(&req) else {
            return;
        };
        let ok = pending.ok.target();
        let err = pending.err.target();
        let mut scratch = Scratch::new(&mut self.mem);
        let mut argument = 0;
        let mut args: Vec<u32> = Vec::new();
        let target = match &outcome {
            Outcome::SessionStarted(text) | Outcome::SessionReference(text) => {
                argument = scratch.wstring_ref(text).unwrap_or(0);
                args.push(argument);
                ok
            }
            Outcome::Lobby(lobby) => {
                argument = scratch.lobby_ref(lobby).unwrap_or(0);
                args.push(argument);
                ok
            }
            Outcome::Lobbies(lobbies) => {
                argument = scratch.search_result_ref(lobbies).unwrap_or(0);
                args.push(argument);
                ok
            }
            Outcome::Updated(update) => {
                // `UpdateLobbyResultDTO`: bool at +0, LobbyDTO at +4,
                // i64 at +216, 224 bytes total. **[measured — abi.rs]**
                let mut raw = [0u8; 224];
                raw[0] = update.success as u8;
                match scratch.lobby(&update.lobby) {
                    Some(dto) => raw[4..212].copy_from_slice(&dto),
                    None => {
                        scratch.release();
                        return;
                    }
                }
                raw[216..224].copy_from_slice(&update.timestamp.to_le_bytes());
                argument = scratch.place(&raw).unwrap_or(0);
                args.push(argument);
                ok
            }
            Outcome::Done => ok,
            Outcome::Failed { code, message } => {
                argument = scratch.wstring_ref(message).unwrap_or(0);
                // `JoinLobby`'s error callback is `void(int, const wstring&)`,
                // `LeaveLobby`'s is `void(int)`, and every other one on this
                // interface is `void(wstring)`. **[measured — abi.rs slot
                // signatures]**
                match pending.slot {
                    "JoinLobby" => {
                        args.push(*code as u32);
                        args.push(argument);
                    }
                    "LeaveLobby" => args.push(*code as u32),
                    _ => args.push(argument),
                }
                err
            }
        };
        // SAFETY: the argument list follows the callback signature `abi.rs`
        // records for `pending.slot`, as annotated above.
        let invoked = unsafe { invoke_target(target, &args) };
        scratch.release();
        self.record(pending.slot, invoked, argument);
    }

    fn notify(&mut self, notice: Notice) {
        match notice {
            Notice::PeerOpened(user_id) => {
                let Some(player) = LocalPlayer::new(&mut self.mem, &user_id) else {
                    self.backend
                        .report_service_error("could not marshal a peer id");
                    return;
                };
                let ptr = gp_of(player.as_ptr());
                self.peers.insert(user_id, player);
                // `CrossplayNetLibSys::init` installs all seven P2P callbacks in
                // one function, and the netlib treats a connection and its data
                // channel as separate events, so both fire in that order.
                // **[measured that both are installed; DoN policy that they
                // fire back to back, since a local directory has no separate
                // negotiation phase]**
                let opened = self.callbacks[cb::P2P_CONNECTION_OPENED].target();
                let channel = self.callbacks[cb::P2P_DATA_CHANNEL_OPENED].target();
                // SAFETY: `void(ICrossplayPlayer*)` — one pointer argument.
                let a = unsafe { invoke_target(opened, &[ptr]) };
                self.record(cb::NAMES[cb::P2P_CONNECTION_OPENED], a, ptr);
                let b = unsafe { invoke_target(channel, &[ptr]) };
                self.record(cb::NAMES[cb::P2P_DATA_CHANNEL_OPENED], b, ptr);
            }
            Notice::PeerClosed(user_id) => {
                let Some(player) = self.peers.remove(&user_id) else {
                    return;
                };
                let ptr = gp_of(player.as_ptr());
                let channel = self.callbacks[cb::P2P_DATA_CHANNEL_CLOSED].target();
                let closed = self.callbacks[cb::P2P_CONNECTION_CLOSED].target();
                // SAFETY: `void(ICrossplayPlayer*)`.
                let a = unsafe { invoke_target(channel, &[ptr]) };
                self.record(cb::NAMES[cb::P2P_DATA_CHANNEL_CLOSED], a, ptr);
                let b = unsafe { invoke_target(closed, &[ptr]) };
                self.record(cb::NAMES[cb::P2P_CONNECTION_CLOSED], b, ptr);
                player.release(&mut self.mem);
            }
            Notice::Data { from, bytes } => {
                let Some(ptr) = self.peers.get(&from).map(|p| gp_of(p.as_ptr())) else {
                    return;
                };
                let slot = self.callbacks[cb::P2P_DATA].target();
                let mut scratch = Scratch::new(&mut self.mem);
                let at = scratch.place(&bytes).unwrap_or(0);
                // SAFETY: `void(ICrossplayPlayer*, const unsigned char*,
                // unsigned int)` — three 4-byte arguments.
                let invoked =
                    at != 0 && unsafe { invoke_target(slot, &[ptr, at, bytes.len() as u32]) };
                scratch.release();
                self.record(cb::NAMES[cb::P2P_DATA], invoked, at);
            }
            Notice::Text { from, text } => {
                let Some(ptr) = self.peers.get(&from).map(|p| gp_of(p.as_ptr())) else {
                    return;
                };
                let slot = self.callbacks[cb::P2P_TEXT].target();
                let mut scratch = Scratch::new(&mut self.mem);
                let at = scratch.wstring_ref(&text).unwrap_or(0);
                // SAFETY: `void(ICrossplayPlayer*, const wstring&)`.
                let invoked = at != 0 && unsafe { invoke_target(slot, &[ptr, at]) };
                scratch.release();
                self.record(cb::NAMES[cb::P2P_TEXT], invoked, at);
            }
        }
    }

    fn read_wstring_arg(&self, at: *const MsvcWstring) -> Option<String> {
        if at.is_null() {
            return None;
        }
        msvc::read_wstring(&self.mem, gp_of(at))
    }

    fn read_attributes_arg(&self, at: *const MsvcUnorderedMap32) -> Option<Attributes> {
        if at.is_null() {
            return None;
        }
        msvc::read_attributes(&self.mem, gp_of(at))
    }

    fn read_packet(&self, data: *mut u8, len: u32) -> Option<Vec<u8>> {
        if data.is_null() || len == 0 || len > MAX_PACKET_BYTES {
            return None;
        }
        let mut bytes = alloc::vec![0u8; len as usize];
        self.mem.read(gp_of(data.cast_const()), &mut bytes).then_some(bytes)
    }

    /// Queue a refusal for a slot this backend does not implement, answering on
    /// the caller's own error callback. The `LIBERR_NOT_AVAILABLE` discipline
    /// `crates/netsys-shim` uses, transposed to a callback interface.
    fn refuse(&mut self, slot: &'static str, err: OwnedFunction) {
        let req = self.backend.fail_closed(slot);
        self.remember(req, slot, OwnedFunction::empty(), err);
    }
}

impl<M: GuestMem + 'static> Drop for LocalCrossPlayService<M> {
    fn drop(&mut self) {
        let peers: Vec<Box<LocalPlayer>> = core::mem::take(&mut self.peers).into_values().collect();
        for peer in peers {
            peer.release(&mut self.mem);
        }
        let blocks = core::mem::take(&mut self.guid_blocks);
        msvc::free_blocks(&mut self.mem, blocks);
    }
}

// ---------------------------------------------------------------------------
// The 58 thunks, expanded once per calling convention.
// ---------------------------------------------------------------------------

macro_rules! service_impl {
    ($abi:literal) => {
        use super::*;

        // --- lifecycle ---------------------------------------------------

        pub(super) unsafe extern $abi fn init<M: GuestMem + 'static>(this: *mut ICrossPlayService) {
            if let Some(s) = unsafe { LocalCrossPlayService::<M>::of(this) } {
                s.backend.init();
            }
        }

        pub(super) unsafe extern $abi fn start_session<M: GuestMem + 'static>(
            this: *mut ICrossPlayService,
            token: *const MsvcWstring,
            on_ok: *const MsvcFunction,
            on_err: *const MsvcFunction,
        ) {
            let Some(s) = (unsafe { LocalCrossPlayService::<M>::of(this) }) else { return };
            // The shipped `StartSession` takes an entity token. DoN has no
            // account system, so a non-empty string is taken as this peer's
            // identity and the configured id is used otherwise.
            // **[DoN policy]**
            let id = match s.read_wstring_arg(token) {
                Some(t) if !t.is_empty() => t,
                _ => s.backend.user_id().to_string(),
            };
            let req = s.backend.start_session(&id);
            // SAFETY: both are `const&` parameters retail keeps owning.
            let (ok, err) = unsafe { (retained_or_empty(on_ok), retained_or_empty(on_err)) };
            s.remember(req, "StartSession", ok, err);
        }

        pub(super) unsafe extern $abi fn stop_session<M: GuestMem + 'static>(
            this: *mut ICrossPlayService,
        ) {
            let Some(s) = (unsafe { LocalCrossPlayService::<M>::of(this) }) else { return };
            let closed = match &mut s.directory {
                DirectoryRef::Owned(directory) => s.backend.stop_session(directory),
                DirectoryRef::Borrowed(pointer) => {
                    // SAFETY: the invariant documented on `DirectoryRef::Borrowed`.
                    let directory = unsafe { &mut **pointer };
                    s.backend.stop_session(directory)
                }
                DirectoryRef::Remote(directory) => {
                    s.backend.stop_session_remote(&mut **directory)
                }
            };
            for user_id in closed {
                s.notify(Notice::PeerClosed(user_id));
            }
            s.pending.clear();
            s.sync_guid();
        }

        pub(super) unsafe extern $abi fn get_session_status<M: GuestMem + 'static>(
            this: *mut ICrossPlayService,
        ) -> SessionStatus {
            match unsafe { LocalCrossPlayService::<M>::of(this) } {
                Some(s) => s.backend.session_status(),
                None => crate::abi::SESSION_STATUS_STOPPED,
            }
        }

        pub(super) unsafe extern $abi fn is_connected_to_hub<M: GuestMem + 'static>(
            this: *mut ICrossPlayService,
        ) -> bool {
            match unsafe { LocalCrossPlayService::<M>::of(this) } {
                Some(s) => s.backend.is_connected_to_hub(),
                None => false,
            }
        }

        pub(super) unsafe extern $abi fn tick<M: GuestMem + 'static>(this: *mut ICrossPlayService) {
            if let Some(s) = unsafe { LocalCrossPlayService::<M>::of(this) } {
                s.pump();
            }
        }

        // --- slots whose shipped body does nothing ------------------------

        /// One of the eight slots ICF-folded onto `ret 4` at RVA `0x145b0`:
        /// `SetServiceUrl`, `BlockUser`, `UnblockUser`, `P2PClose`.
        /// **[measured]**
        pub(super) unsafe extern $abi fn noop_wstring<M: GuestMem + 'static>(
            _this: *mut ICrossPlayService,
            _arg: *const MsvcWstring,
        ) {
        }

        /// `IsUserBlocked` is `xor al, al; ret 4` at RVA `0x14f70` — a constant
        /// `false`, not a lookup. **[measured]**
        pub(super) unsafe extern $abi fn constant_false<M: GuestMem + 'static>(
            _this: *mut ICrossPlayService,
            _arg: *const MsvcWstring,
        ) -> bool {
            false
        }

        /// `P2PStartConnection` is `ret 8` at RVA `0x1dc60`. **[measured]**
        pub(super) unsafe extern $abi fn noop_two_wstrings<M: GuestMem + 'static>(
            _this: *mut ICrossPlayService,
            _a: *const MsvcWstring,
            _b: *const MsvcWstring,
        ) {
        }

        /// `CreateLocalPlayerLoopback` is `ret 0` at RVA `0x1e350`.
        /// **[measured]**
        pub(super) unsafe extern $abi fn noop_void<M: GuestMem + 'static>(
            _this: *mut ICrossPlayService,
        ) {
        }

        /// `SetReliability` is folded onto the same `ret 4`, so retaining the
        /// flag is already more than the shipped build does. **[measured]**
        pub(super) unsafe extern $abi fn set_reliability<M: GuestMem + 'static>(
            this: *mut ICrossPlayService,
            reliable: bool,
        ) {
            if let Some(s) = unsafe { LocalCrossPlayService::<M>::of(this) } {
                s.backend.set_reliability(reliable);
            }
        }

        // --- retained callbacks -------------------------------------------

        pub(super) unsafe extern $abi fn retain_service_error<M: GuestMem + 'static>(
            this: *mut ICrossPlayService,
            f: *const MsvcFunction,
        ) {
            if let Some(s) = unsafe { LocalCrossPlayService::<M>::of(this) } {
                s.retain(cb::SERVICE_ERROR, f);
            }
        }
        pub(super) unsafe extern $abi fn retain_join_broadcast<M: GuestMem + 'static>(
            this: *mut ICrossPlayService,
            f: *const MsvcFunction,
        ) {
            if let Some(s) = unsafe { LocalCrossPlayService::<M>::of(this) } {
                s.retain(cb::JOIN_LOBBY_BROADCAST, f);
            }
        }
        pub(super) unsafe extern $abi fn retain_leave_broadcast<M: GuestMem + 'static>(
            this: *mut ICrossPlayService,
            f: *const MsvcFunction,
        ) {
            if let Some(s) = unsafe { LocalCrossPlayService::<M>::of(this) } {
                s.retain(cb::LEAVE_LOBBY_BROADCAST, f);
            }
        }
        pub(super) unsafe extern $abi fn retain_update_broadcast<M: GuestMem + 'static>(
            this: *mut ICrossPlayService,
            f: *const MsvcFunction,
        ) {
            if let Some(s) = unsafe { LocalCrossPlayService::<M>::of(this) } {
                s.retain(cb::UPDATE_LOBBY_BROADCAST, f);
            }
        }
        pub(super) unsafe extern $abi fn retain_lobby_message<M: GuestMem + 'static>(
            this: *mut ICrossPlayService,
            f: *const MsvcFunction,
        ) {
            if let Some(s) = unsafe { LocalCrossPlayService::<M>::of(this) } {
                s.retain(cb::LOBBY_MESSAGE, f);
            }
        }
        pub(super) unsafe extern $abi fn retain_crossplay_enabled<M: GuestMem + 'static>(
            this: *mut ICrossPlayService,
            f: *const MsvcFunction,
        ) {
            if let Some(s) = unsafe { LocalCrossPlayService::<M>::of(this) } {
                s.retain(cb::CROSSPLAY_ENABLED, f);
            }
        }
        pub(super) unsafe extern $abi fn retain_chat_join<M: GuestMem + 'static>(
            this: *mut ICrossPlayService,
            f: *const MsvcFunction,
        ) {
            if let Some(s) = unsafe { LocalCrossPlayService::<M>::of(this) } {
                s.retain(cb::CHAT_JOIN, f);
            }
        }
        pub(super) unsafe extern $abi fn retain_chat_leave<M: GuestMem + 'static>(
            this: *mut ICrossPlayService,
            f: *const MsvcFunction,
        ) {
            if let Some(s) = unsafe { LocalCrossPlayService::<M>::of(this) } {
                s.retain(cb::CHAT_LEAVE, f);
            }
        }
        pub(super) unsafe extern $abi fn retain_chat_message<M: GuestMem + 'static>(
            this: *mut ICrossPlayService,
            f: *const MsvcFunction,
        ) {
            if let Some(s) = unsafe { LocalCrossPlayService::<M>::of(this) } {
                s.retain(cb::CHAT_MESSAGE, f);
            }
        }
        pub(super) unsafe extern $abi fn retain_p2p_connection_opened<M: GuestMem + 'static>(
            this: *mut ICrossPlayService,
            f: *const MsvcFunction,
        ) {
            if let Some(s) = unsafe { LocalCrossPlayService::<M>::of(this) } {
                s.retain(cb::P2P_CONNECTION_OPENED, f);
            }
        }
        pub(super) unsafe extern $abi fn retain_p2p_connection_closed<M: GuestMem + 'static>(
            this: *mut ICrossPlayService,
            f: *const MsvcFunction,
        ) {
            if let Some(s) = unsafe { LocalCrossPlayService::<M>::of(this) } {
                s.retain(cb::P2P_CONNECTION_CLOSED, f);
            }
        }
        pub(super) unsafe extern $abi fn retain_p2p_channel_opened<M: GuestMem + 'static>(
            this: *mut ICrossPlayService,
            f: *const MsvcFunction,
        ) {
            if let Some(s) = unsafe { LocalCrossPlayService::<M>::of(this) } {
                s.retain(cb::P2P_DATA_CHANNEL_OPENED, f);
            }
        }
        pub(super) unsafe extern $abi fn retain_p2p_channel_closed<M: GuestMem + 'static>(
            this: *mut ICrossPlayService,
            f: *const MsvcFunction,
        ) {
            if let Some(s) = unsafe { LocalCrossPlayService::<M>::of(this) } {
                s.retain(cb::P2P_DATA_CHANNEL_CLOSED, f);
            }
        }
        pub(super) unsafe extern $abi fn retain_p2p_text<M: GuestMem + 'static>(
            this: *mut ICrossPlayService,
            f: *const MsvcFunction,
        ) {
            if let Some(s) = unsafe { LocalCrossPlayService::<M>::of(this) } {
                s.retain(cb::P2P_TEXT, f);
            }
        }
        pub(super) unsafe extern $abi fn retain_p2p_data<M: GuestMem + 'static>(
            this: *mut ICrossPlayService,
            f: *const MsvcFunction,
        ) {
            if let Some(s) = unsafe { LocalCrossPlayService::<M>::of(this) } {
                s.retain(cb::P2P_DATA, f);
            }
        }

        /// `SetRenewTokenCallback` is one of the eight folded no-ops, and its
        /// `std::function` specialisation is only forward-declared in this PDB,
        /// so `abi.rs` types the parameter as an opaque pointer. Retained as a
        /// raw 40-byte copy anyway, for a later lane. **[measured inert]**
        pub(super) unsafe extern $abi fn retain_renew_token<M: GuestMem + 'static>(
            this: *mut ICrossPlayService,
            f: *const c_void,
        ) {
            if let Some(s) = unsafe { LocalCrossPlayService::<M>::of(this) } {
                s.retain(cb::RENEW_TOKEN, f.cast::<MsvcFunction>());
            }
        }

        /// `SetP2PConnectionFailedCallback` is a folded no-op in the shipped
        /// build **and** is installed by `CrossplayNetLibSys::init`.
        /// **[measured both]**
        pub(super) unsafe extern $abi fn retain_p2p_failed<M: GuestMem + 'static>(
            this: *mut ICrossPlayService,
            f: *const c_void,
        ) {
            if let Some(s) = unsafe { LocalCrossPlayService::<M>::of(this) } {
                s.retain(cb::P2P_CONNECTION_FAILED, f.cast::<MsvcFunction>());
            }
        }

        // --- lobby CRUD ----------------------------------------------------

        pub(super) unsafe extern $abi fn get_lobby<M: GuestMem + 'static>(
            this: *mut ICrossPlayService,
            id: *const MsvcWstring,
            mut on_ok: MsvcFunction,
            mut on_err: MsvcFunction,
        ) {
            let Some(s) = (unsafe { LocalCrossPlayService::<M>::of(this) }) else { return };
            let req = match s.read_wstring_arg(id) {
                Some(id) => s.backend.get_lobby(&id),
                None => s.backend.fail_closed("GetLobby: unreadable lobby id"),
            };
            // SAFETY: both arrived by value and are this callee's to destroy.
            let (ok, err) = unsafe {
                (
                    OwnedFunction::adopt(&mut on_ok),
                    OwnedFunction::adopt(&mut on_err),
                )
            };
            s.remember(req, "GetLobby", ok, err);
        }

        pub(super) unsafe extern $abi fn find_lobbies<M: GuestMem + 'static>(
            this: *mut ICrossPlayService,
            criteria: *const LobbySearchCriteriaDTO,
            mut on_ok: MsvcFunction,
            mut on_err: MsvcFunction,
        ) {
            let Some(s) = (unsafe { LocalCrossPlayService::<M>::of(this) }) else { return };
            let req = if criteria.is_null() {
                s.backend.fail_closed("FindLobbies: null criteria")
            } else {
                // SAFETY: three plain `int`s, layout asserted in `abi.rs`.
                let c = unsafe { *criteria };
                // `_proximity` is accepted and ignored: DoN has no geographic
                // notion of a peer. **[DoN policy]**
                s.backend.find_lobbies(c.max_results, c.min_available_slots)
            };
            // SAFETY: both arrived by value and are this callee's to destroy.
            let (ok, err) = unsafe {
                (
                    OwnedFunction::adopt(&mut on_ok),
                    OwnedFunction::adopt(&mut on_err),
                )
            };
            s.remember(req, "FindLobbies", ok, err);
        }

        pub(super) unsafe extern $abi fn create_lobby<M: GuestMem + 'static>(
            this: *mut ICrossPlayService,
            max_members: i32,
            visibility: Visibility,
            attributes: *const MsvcUnorderedMap32,
            mut on_ok: MsvcFunction,
            mut on_err: MsvcFunction,
        ) {
            let Some(s) = (unsafe { LocalCrossPlayService::<M>::of(this) }) else { return };
            let req = match s.read_attributes_arg(attributes) {
                Some(attrs) => s.backend.create_lobby(max_members, visibility, attrs),
                None => s.backend.fail_closed("CreateLobby: unreadable attribute map"),
            };
            // SAFETY: both arrived by value and are this callee's to destroy.
            let (ok, err) = unsafe {
                (
                    OwnedFunction::adopt(&mut on_ok),
                    OwnedFunction::adopt(&mut on_err),
                )
            };
            s.remember(req, "CreateLobby", ok, err);
        }

        pub(super) unsafe extern $abi fn join_lobby<M: GuestMem + 'static>(
            this: *mut ICrossPlayService,
            id: *const MsvcWstring,
            on_ok: *const MsvcFunction,
            on_err: *const MsvcFunction,
        ) {
            let Some(s) = (unsafe { LocalCrossPlayService::<M>::of(this) }) else { return };
            let req = match s.read_wstring_arg(id) {
                Some(id) => s.backend.join_lobby(&id),
                None => s.backend.fail_closed("JoinLobby: unreadable lobby id"),
            };
            // SAFETY: both are `const&` parameters retail keeps owning.
            let (ok, err) = unsafe { (retained_or_empty(on_ok), retained_or_empty(on_err)) };
            s.remember(req, "JoinLobby", ok, err);
        }

        pub(super) unsafe extern $abi fn leave_lobby<M: GuestMem + 'static>(
            this: *mut ICrossPlayService,
            id: *const MsvcWstring,
            on_ok: *const MsvcFunction,
            on_err: *const MsvcFunction,
        ) {
            let Some(s) = (unsafe { LocalCrossPlayService::<M>::of(this) }) else { return };
            let req = match s.read_wstring_arg(id) {
                Some(id) => s.backend.leave_lobby(&id),
                None => s.backend.fail_closed("LeaveLobby: unreadable lobby id"),
            };
            // SAFETY: both are `const&` parameters retail keeps owning.
            let (ok, err) = unsafe { (retained_or_empty(on_ok), retained_or_empty(on_err)) };
            s.remember(req, "LeaveLobby", ok, err);
        }

        #[allow(clippy::too_many_arguments)]
        pub(super) unsafe extern $abi fn update_lobby<M: GuestMem + 'static>(
            this: *mut ICrossPlayService,
            id: *const MsvcWstring,
            max_members: i32,
            attributes: *const MsvcUnorderedMap32,
            bot_count: i32,
            _unnamed: i32,
            on_ok: *const MsvcFunction,
            mut on_err: MsvcFunction,
        ) {
            let Some(s) = (unsafe { LocalCrossPlayService::<M>::of(this) }) else { return };
            // `UpdateLobby(const wstring&, int, const map&, int, int, onOk,
            // onErr)`. The PDB names none of the three `int`s. DoN reads the
            // first as the max member count and the second as the bot count, by
            // analogy with `CreateLobby(int maxMembers, ...)` and
            // `LobbyDTO::_botCount`, and **ignores** the third because nothing
            // identifies it. **[inferred — this pairing is not measured.]**
            let req = match (s.read_wstring_arg(id), s.read_attributes_arg(attributes)) {
                (Some(id), Some(attrs)) => {
                    s.backend.update_lobby(&id, max_members, bot_count, attrs)
                }
                _ => s.backend.fail_closed("UpdateLobby: unreadable arguments"),
            };
            // `onOk` is `const&` and `onErr` is by value — the one slot on
            // this interface that mixes the two. **[measured — abi.rs]**
            // SAFETY: each matches the parameter kind the PDB records.
            let (ok, err) = unsafe {
                (
                    retained_or_empty(on_ok),
                    OwnedFunction::adopt(&mut on_err),
                )
            };
            s.remember(req, "UpdateLobby", ok, err);
        }

        pub(super) unsafe extern $abi fn lobby_cancel_pending<M: GuestMem + 'static>(
            this: *mut ICrossPlayService,
        ) {
            let Some(s) = (unsafe { LocalCrossPlayService::<M>::of(this) }) else { return };
            // Drop the retained callbacks without answering them: that is what
            // cancelling a request the caller no longer wants means. The queued
            // backend work still resolves and is discarded by `complete`'s
            // missing-entry path.
            s.pending.clear();
        }

        // --- match start ----------------------------------------------------

        pub(super) unsafe extern $abi fn start_game<M: GuestMem + 'static>(
            this: *mut ICrossPlayService,
            id: *const MsvcWstring,
            _unnamed: bool,
            mut on_ok: MsvcFunction,
            mut on_err: MsvcFunction,
        ) {
            let Some(s) = (unsafe { LocalCrossPlayService::<M>::of(this) }) else { return };
            // The `bool` between the lobby id and the callbacks is unnamed in
            // the PDB signature and nothing was measured about it, so it is
            // accepted and not acted on. **[not derived]**
            let req = match s.read_wstring_arg(id) {
                // DoN publishes the lobby id as the session reference: the
                // shipped ok callback is `void(const wstring&)` and a joined
                // peer needs *some* stable handle to recognise the start.
                // **[DoN policy]**
                Some(id) => s.backend.start_game(&id, &id.clone()),
                None => s.backend.fail_closed("StartGame: unreadable lobby id"),
            };
            // SAFETY: both arrived by value and are this callee's to destroy.
            let (ok, err) = unsafe {
                (
                    OwnedFunction::adopt(&mut on_ok),
                    OwnedFunction::adopt(&mut on_err),
                )
            };
            s.remember(req, "StartGame", ok, err);
        }

        pub(super) unsafe extern $abi fn cancel_game_start<M: GuestMem + 'static>(
            this: *mut ICrossPlayService,
            id: *const MsvcWstring,
            mut on_ok: MsvcFunction,
            mut on_err: MsvcFunction,
        ) {
            let Some(s) = (unsafe { LocalCrossPlayService::<M>::of(this) }) else { return };
            let req = match s.read_wstring_arg(id) {
                Some(id) => s.backend.cancel_game_start(&id),
                None => s.backend.fail_closed("CancelGameStart: unreadable lobby id"),
            };
            // SAFETY: both arrived by value and are this callee's to destroy.
            let (ok, err) = unsafe {
                (
                    OwnedFunction::adopt(&mut on_ok),
                    OwnedFunction::adopt(&mut on_err),
                )
            };
            s.remember(req, "CancelGameStart", ok, err);
        }

        // --- P2P -------------------------------------------------------------

        pub(super) unsafe extern $abi fn set_p2p_timeout<M: GuestMem + 'static>(
            this: *mut ICrossPlayService,
            ms: i32,
        ) {
            if let Some(s) = unsafe { LocalCrossPlayService::<M>::of(this) } {
                s.backend.set_p2p_timeout(ms);
            }
        }

        pub(super) unsafe extern $abi fn p2p_send_to_all<M: GuestMem + 'static>(
            this: *mut ICrossPlayService,
            data: *mut u8,
            len: u32,
        ) -> bool {
            let Some(s) = (unsafe { LocalCrossPlayService::<M>::of(this) }) else { return false };
            let Some(bytes) = s.read_packet(data, len) else { return false };
            match &mut s.directory {
                DirectoryRef::Owned(directory) => {
                    s.backend.p2p_send_to_all(directory, &bytes)
                }
                DirectoryRef::Borrowed(pointer) => {
                    // SAFETY: the invariant documented on `DirectoryRef::Borrowed`.
                    let directory = unsafe { &mut **pointer };
                    s.backend.p2p_send_to_all(directory, &bytes)
                }
                DirectoryRef::Remote(directory) => {
                    s.backend.p2p_send_to_all_remote(&mut **directory, &bytes)
                }
            }
        }

        pub(super) unsafe extern $abi fn p2p_send<M: GuestMem + 'static>(
            this: *mut ICrossPlayService,
            player: *mut ICrossplayPlayer,
            data: *mut u8,
            len: u32,
        ) -> bool {
            let Some(s) = (unsafe { LocalCrossPlayService::<M>::of(this) }) else { return false };
            let peer = match unsafe { LocalPlayer::from_ptr(player) } {
                Some(p) => p.user_id().to_string(),
                None => return false,
            };
            let Some(bytes) = s.read_packet(data, len) else { return false };
            match &mut s.directory {
                DirectoryRef::Owned(directory) => {
                    s.backend.p2p_send(directory, &peer, &bytes)
                }
                DirectoryRef::Borrowed(pointer) => {
                    // SAFETY: the invariant documented on `DirectoryRef::Borrowed`.
                    let directory = unsafe { &mut **pointer };
                    s.backend.p2p_send(directory, &peer, &bytes)
                }
                DirectoryRef::Remote(directory) => {
                    s.backend.p2p_send_remote(&mut **directory, &peer, &bytes)
                }
            }
        }

        pub(super) unsafe extern $abi fn p2p_close_all<M: GuestMem + 'static>(
            this: *mut ICrossPlayService,
        ) {
            let Some(s) = (unsafe { LocalCrossPlayService::<M>::of(this) }) else { return };
            let closed = match &mut s.directory {
                DirectoryRef::Owned(directory) => s.backend.p2p_close_all(directory),
                DirectoryRef::Borrowed(pointer) => {
                    // SAFETY: the invariant documented on `DirectoryRef::Borrowed`.
                    let directory = unsafe { &mut **pointer };
                    s.backend.p2p_close_all(directory)
                }
                DirectoryRef::Remote(_) => s.backend.p2p_close_all_remote(),
            };
            for user_id in closed {
                s.notify(Notice::PeerClosed(user_id));
            }
        }

        // --- identity ---------------------------------------------------------

        pub(super) unsafe extern $abi fn get_crossplay_status<M: GuestMem + 'static>(
            _this: *mut ICrossPlayService,
        ) -> CrossplayStatus {
            // DoN's directory carries no platform distinction at all, so
            // crossplay is unconditionally enabled. **[DoN policy]**
            CROSSPLAY_STATUS_ENABLED
        }

        pub(super) unsafe extern $abi fn set_username<M: GuestMem + 'static>(
            this: *mut ICrossPlayService,
            name: *const MsvcWstring,
        ) {
            let Some(s) = (unsafe { LocalCrossPlayService::<M>::of(this) }) else { return };
            if let Some(name) = s.read_wstring_arg(name) {
                s.backend.set_username(&name);
            }
        }

        pub(super) unsafe extern $abi fn get_player_guid<M: GuestMem + 'static>(
            this: *mut ICrossPlayService,
        ) -> *mut MsvcWstring {
            let Some(s) = (unsafe { LocalCrossPlayService::<M>::of(this) }) else {
                return core::ptr::null_mut();
            };
            s.sync_guid();
            s.guid_at as usize as *mut MsvcWstring
        }

        /// `GetInvitationId` returns a `wstring` **by value**, so its first
        /// stack argument is the caller's return slot, which it also returns in
        /// `EAX`. DoN has no invitation system, so it writes an empty string —
        /// a valid 24-byte object, never uninitialised memory.
        pub(super) unsafe extern $abi fn get_invitation_id<M: GuestMem + 'static>(
            _this: *mut ICrossPlayService,
            out: *mut MsvcWstring,
        ) -> *mut MsvcWstring {
            if !out.is_null() {
                let mut raw = [0u8; 24];
                // _Mysize 0, _Myres 7: the empty small-string form.
                raw[20..24].copy_from_slice(&7u32.to_le_bytes());
                // SAFETY: the caller supplied a 24-byte return slot.
                unsafe { core::ptr::write(out.cast::<[u8; 24]>(), raw) };
            }
            out
        }

        // --- fail closed -------------------------------------------------------

        pub(super) unsafe extern $abi fn send_lobby_chat<M: GuestMem + 'static>(
            this: *mut ICrossPlayService,
            _lobby: *const MsvcWstring,
            _text: *const MsvcWstring,
        ) {
            if let Some(s) = unsafe { LocalCrossPlayService::<M>::of(this) } {
                s.refuse("SendLobbyChat", OwnedFunction::empty());
            }
        }

        pub(super) unsafe extern $abi fn join_chat<M: GuestMem + 'static>(
            this: *mut ICrossPlayService,
            _id: *const MsvcWstring,
            mut _on_ok: MsvcFunction,
            mut on_err: MsvcFunction,
        ) {
            // SAFETY: both arrived by value, so both are this callee's to
            // destroy even though the call is refused.
            let (_ok, err) = unsafe {
                (
                    OwnedFunction::adopt(&mut _on_ok),
                    OwnedFunction::adopt(&mut on_err),
                )
            };
            if let Some(s) = unsafe { LocalCrossPlayService::<M>::of(this) } {
                s.refuse("JoinChat", err);
            }
        }

        pub(super) unsafe extern $abi fn leave_chat<M: GuestMem + 'static>(
            this: *mut ICrossPlayService,
            _id: *const MsvcWstring,
        ) {
            if let Some(s) = unsafe { LocalCrossPlayService::<M>::of(this) } {
                s.refuse("LeaveChat", OwnedFunction::empty());
            }
        }

        pub(super) unsafe extern $abi fn send_chat_message<M: GuestMem + 'static>(
            this: *mut ICrossPlayService,
            _id: *const MsvcWstring,
            _text: *const MsvcWstring,
        ) {
            if let Some(s) = unsafe { LocalCrossPlayService::<M>::of(this) } {
                s.refuse("SendChatMessage", OwnedFunction::empty());
            }
        }

        pub(super) unsafe extern $abi fn set_stats<M: GuestMem + 'static>(
            this: *mut ICrossPlayService,
            _stats: *const MsvcMap8,
            mut _on_ok: MsvcFunction,
            mut on_err: MsvcFunction,
        ) {
            // SAFETY: both arrived by value, so both are this callee's to
            // destroy even though the call is refused.
            let (_ok, err) = unsafe {
                (
                    OwnedFunction::adopt(&mut _on_ok),
                    OwnedFunction::adopt(&mut on_err),
                )
            };
            if let Some(s) = unsafe { LocalCrossPlayService::<M>::of(this) } {
                s.refuse("SetStats", err);
            }
        }

        pub(super) unsafe extern $abi fn get_stats<M: GuestMem + 'static>(
            this: *mut ICrossPlayService,
            _id: *const MsvcWstring,
            mut _on_ok: MsvcFunction,
            mut on_err: MsvcFunction,
        ) {
            // SAFETY: both arrived by value, so both are this callee's to
            // destroy even though the call is refused.
            let (_ok, err) = unsafe {
                (
                    OwnedFunction::adopt(&mut _on_ok),
                    OwnedFunction::adopt(&mut on_err),
                )
            };
            if let Some(s) = unsafe { LocalCrossPlayService::<M>::of(this) } {
                s.refuse("GetStats", err);
            }
        }

        pub(super) unsafe extern $abi fn get_global_leaderboards<M: GuestMem + 'static>(
            this: *mut ICrossPlayService,
            _name: *const MsvcWstring,
            _count: i32,
            mut _on_ok: MsvcFunction,
            mut on_err: MsvcFunction,
        ) {
            // SAFETY: both arrived by value, so both are this callee's to
            // destroy even though the call is refused.
            let (_ok, err) = unsafe {
                (
                    OwnedFunction::adopt(&mut _on_ok),
                    OwnedFunction::adopt(&mut on_err),
                )
            };
            if let Some(s) = unsafe { LocalCrossPlayService::<M>::of(this) } {
                s.refuse("GetGlobalLeaderboards", err);
            }
        }

        pub(super) unsafe extern $abi fn get_leaderboards_around_player<M: GuestMem + 'static>(
            this: *mut ICrossPlayService,
            _name: *const MsvcWstring,
            _player: *const MsvcWstring,
            _count: i32,
            mut _on_ok: MsvcFunction,
            mut on_err: MsvcFunction,
        ) {
            // SAFETY: both arrived by value, so both are this callee's to
            // destroy even though the call is refused.
            let (_ok, err) = unsafe {
                (
                    OwnedFunction::adopt(&mut _on_ok),
                    OwnedFunction::adopt(&mut on_err),
                )
            };
            if let Some(s) = unsafe { LocalCrossPlayService::<M>::of(this) } {
                s.refuse("GetLeaderboardsAroundPlayer", err);
            }
        }

        pub(super) unsafe extern $abi fn create_invitation<M: GuestMem + 'static>(
            this: *mut ICrossPlayService,
            _a: *const MsvcWstring,
            _b: *const MsvcWstring,
            mut _on_ok: MsvcFunction,
            mut on_err: MsvcFunction,
        ) {
            // SAFETY: both arrived by value, so both are this callee's to
            // destroy even though the call is refused.
            let (_ok, err) = unsafe {
                (
                    OwnedFunction::adopt(&mut _on_ok),
                    OwnedFunction::adopt(&mut on_err),
                )
            };
            if let Some(s) = unsafe { LocalCrossPlayService::<M>::of(this) } {
                s.refuse("CreateInvitation", err);
            }
        }

        pub(super) unsafe extern $abi fn accept_invitation<M: GuestMem + 'static>(
            this: *mut ICrossPlayService,
            _id: *const MsvcWstring,
            mut _on_ok: MsvcFunction,
            mut on_err: MsvcFunction,
        ) {
            // SAFETY: both arrived by value, so both are this callee's to
            // destroy even though the call is refused.
            let (_ok, err) = unsafe {
                (
                    OwnedFunction::adopt(&mut _on_ok),
                    OwnedFunction::adopt(&mut on_err),
                )
            };
            if let Some(s) = unsafe { LocalCrossPlayService::<M>::of(this) } {
                s.refuse("AcceptInvitation", err);
            }
        }
    };
}

#[cfg(target_arch = "x86")]
mod thunks {
    service_impl!("thiscall");
}
#[cfg(not(target_arch = "x86"))]
mod thunks {
    service_impl!("C");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abi::{
        LobbyProximity, SESSION_STATUS_STARTED, SESSION_STATUS_STARTING, SESSION_STATUS_STOPPED,
        VISIBILITY_PUBLIC,
    };
    use crate::local::{error, Lobby};
    use crate::msvc::{read_attributes, read_u32, read_wstring, ArenaMem};

    type Svc = LocalCrossPlayService<ArenaMem>;

    fn vt() -> &'static ICrossPlayServiceVtable {
        Svc::vtable()
    }

    /// Place a `wstring` in the service's own arena and hand back the pointer a
    /// caller would push. Deliberately leaks the arena blocks: a test process is
    /// the whole lifetime, and `ArenaMem` reports leaks separately.
    fn wstr(svc: &mut Svc, text: &str) -> *const MsvcWstring {
        let mut scratch = Scratch::new(svc.memory_mut());
        let at = scratch.wstring_ref(text).expect("wstring");
        let _ = scratch.into_blocks();
        at as usize as *const MsvcWstring
    }

    fn attrs(svc: &mut Svc, pairs: &[(&str, &str)]) -> *const MsvcUnorderedMap32 {
        let mut map = Attributes::new();
        for (k, v) in pairs {
            map.insert((*k).to_string(), (*v).to_string());
        }
        let mut scratch = Scratch::new(svc.memory_mut());
        let built = scratch.attributes(&map).expect("map");
        let at = scratch.place(&built).expect("place");
        let _ = scratch.into_blocks();
        at as usize as *const MsvcUnorderedMap32
    }

    fn packet(svc: &mut Svc, bytes: &[u8]) -> *mut u8 {
        let mut scratch = Scratch::new(svc.memory_mut());
        let at = scratch.place(bytes).expect("packet");
        let _ = scratch.into_blocks();
        at as usize as *mut u8
    }

    /// The last journal entry for `slot`, if any.
    fn last(svc: &Svc, slot: &str) -> Option<Dispatch> {
        svc.journal().iter().rev().find(|d| d.slot == slot).cloned()
    }

    /// Read back the `LobbyDTO` a completion built.
    fn lobby_at(svc: &Svc, at: Gp) -> Lobby {
        Lobby {
            id: read_wstring(&svc.mem, at).unwrap(),
            owner_user_id: read_wstring(&svc.mem, at + 24).unwrap(),
            session_reference: read_wstring(&svc.mem, at + 48).unwrap(),
            max_members: read_u32(&svc.mem, at + 72).unwrap() as i32,
            bot_count: read_u32(&svc.mem, at + 80).unwrap() as i32,
            attribute_version: read_u32(&svc.mem, at + 84).unwrap() as i32,
            visibility: read_u32(&svc.mem, at + 88).unwrap() as i32,
            members: Vec::new(),
            attributes: read_attributes(&svc.mem, at + 104).unwrap(),
            game_started: false,
        }
    }

    fn start(svc: &mut Svc, id: &str) {
        unsafe { (vt().Init)(svc.as_service()) };
        let token = wstr(svc, id);
        unsafe {
            (vt().StartSession)(
                svc.as_service(),
                token,
                &empty_function(),
                &empty_function(),
            )
        };
        assert_eq!(
            unsafe { (vt().GetSessionStatus)(svc.as_service()) },
            SESSION_STATUS_STARTING
        );
        unsafe { (vt().Tick)(svc.as_service()) };
        assert_eq!(
            unsafe { (vt().GetSessionStatus)(svc.as_service()) },
            SESSION_STATUS_STARTED
        );
        assert!(unsafe { (vt().IsConnectedToHub)(svc.as_service()) });
    }

    #[test]
    fn the_vtable_pointer_is_the_object_address() {
        let svc = Svc::new(ArenaMem::new(), "host", "Host");
        assert_eq!(core::mem::offset_of!(Svc, vftable), 0);
        assert_eq!(svc.as_service() as usize, &*svc as *const Svc as usize);
        // ...and the published table is the one the object carries.
        assert_eq!(svc.vftable, Svc::vtable() as *const _);
        // 58 slots of pointer width, as `abi.rs` asserts independently.
        assert_eq!(
            core::mem::size_of::<ICrossPlayServiceVtable>(),
            58 * core::mem::size_of::<*const ()>()
        );
    }

    #[test]
    fn the_measured_inert_slots_are_reproduced_inert() {
        let mut svc = Svc::new(ArenaMem::new(), "host", "Host");
        start(&mut svc, "host");
        let arg = wstr(&mut svc, "anything");
        let before = svc.journal().len();
        unsafe {
            // The eight ICF-folded `ret 4` bodies at RVA 0x145b0 that take a
            // wstring, plus the two bare returns.
            (vt().SetServiceUrl)(svc.as_service(), arg);
            (vt().BlockUser)(svc.as_service(), arg);
            (vt().UnblockUser)(svc.as_service(), arg);
            (vt().P2PClose)(svc.as_service(), arg);
            (vt().P2PStartConnection)(svc.as_service(), arg, arg);
            (vt().CreateLocalPlayerLoopback)(svc.as_service());
            // IsUserBlocked is `xor al, al; ret 4` — a constant false.
            assert!(!(vt().IsUserBlocked)(svc.as_service(), arg));
        }
        // None of them may produce work, a callback or a state change.
        assert_eq!(svc.journal().len(), before);
        unsafe { (vt().Tick)(svc.as_service()) };
        assert_eq!(svc.journal().len(), before);
        assert_eq!(
            unsafe { (vt().GetSessionStatus)(svc.as_service()) },
            SESSION_STATUS_STARTED
        );
    }

    #[test]
    fn get_player_guid_returns_a_stable_readable_wstring() {
        let mut svc = Svc::new(ArenaMem::new(), "host", "Host");
        // Before the session starts the guid is empty, not garbage.
        unsafe { (vt().Init)(svc.as_service()) };
        let at = unsafe { (vt().GetPlayerGuid)(svc.as_service()) };
        assert_eq!(read_wstring(&svc.mem, gp_of(at)).as_deref(), Some(""));
        start(&mut svc, "host-with-a-long-id");
        let a = unsafe { (vt().GetPlayerGuid)(svc.as_service()) };
        let b = unsafe { (vt().GetPlayerGuid)(svc.as_service()) };
        assert_eq!(a, b, "the returned reference must be stable");
        assert_eq!(
            read_wstring(&svc.mem, gp_of(a)).as_deref(),
            Some("host-with-a-long-id")
        );
    }

    #[test]
    fn get_invitation_id_writes_a_valid_empty_string_into_the_return_slot() {
        let svc = Svc::new(ArenaMem::new(), "host", "Host");
        let mut out = MsvcWstring { raw: [0xAB; 24] };
        let returned = unsafe { (vt().GetInvitationId)(svc.as_service(), &mut out) };
        assert_eq!(returned, &mut out as *mut MsvcWstring);
        assert_eq!(u32::from_le_bytes(out.raw[16..20].try_into().unwrap()), 0);
        assert_eq!(u32::from_le_bytes(out.raw[20..24].try_into().unwrap()), 7);
        assert_eq!(&out.raw[0..16], &[0u8; 16]);
    }

    #[test]
    fn a_two_peer_lobby_runs_end_to_end_through_the_vtable() {
        let mut directory = Directory::new();
        let directory_ptr: *mut Directory = &mut directory;
        let mut host = unsafe { Svc::joined(ArenaMem::new(), "host", "Host", directory_ptr) };
        let mut peer = unsafe { Svc::joined(ArenaMem::new(), "peer", "Peer", directory_ptr) };
        start(&mut host, "host");
        start(&mut peer, "peer");

        // CrossplayNetLibSys::init installs all seven P2P callbacks in one
        // function; install them here the same way (empty functions, since a
        // host test has no guest code to call).
        for svc in [&mut host, &mut peer] {
            let f = empty_function();
            unsafe {
                (vt().SetP2PConnectionOpenedCallback)(svc.as_service(), &f);
                (vt().SetP2PConnectionClosedCallback)(svc.as_service(), &f);
                (vt().SetP2PDataChannelOpenedCallback)(svc.as_service(), &f);
                (vt().SetP2PDataChannelClosedCallback)(svc.as_service(), &f);
                (vt().SetReceivedP2PTextCallback)(svc.as_service(), &f);
                (vt().SetReceivedP2PDataCallback)(svc.as_service(), &f);
                (vt().SetP2PConnectionFailedCallback)(
                    svc.as_service(),
                    (&f as *const MsvcFunction).cast(),
                );
            }
        }

        // --- CreateLobby, with the real attribute schema on the wire --------
        let map = attrs(
            &mut host,
            &[("game_seed", "3735928559"), ("name", "don end-to-end")],
        );
        unsafe {
            (vt().CreateLobby)(
                host.as_service(),
                4,
                VISIBILITY_PUBLIC,
                map,
                empty_function(),
                empty_function(),
            )
        };
        unsafe { (vt().Tick)(host.as_service()) };
        let created = last(&host, "CreateLobby").expect("CreateLobby completed");
        assert_ne!(created.argument, 0, "a LobbyDTO must have been built");
        let lobby = lobby_at(&host, created.argument);
        assert_eq!(lobby.owner_user_id, "host");
        assert_eq!(lobby.max_members, 4);
        assert_eq!(
            lobby.attributes.get("game_seed").map(String::as_str),
            Some("3735928559"),
            "the attribute map retail passed must survive into the DTO"
        );

        // --- FindLobbies from the other peer --------------------------------
        let criteria = LobbySearchCriteriaDTO {
            max_results: 10,
            min_available_slots: 1,
            proximity: 0 as LobbyProximity,
        };
        unsafe {
            (vt().FindLobbies)(
                peer.as_service(),
                &criteria,
                empty_function(),
                empty_function(),
            )
        };
        unsafe { (vt().Tick)(peer.as_service()) };
        let found = last(&peer, "FindLobbies").expect("FindLobbies completed");
        let first = read_u32(&peer.mem, found.argument).unwrap();
        let last_ptr = read_u32(&peer.mem, found.argument + 4).unwrap();
        assert_eq!(last_ptr - first, 208, "exactly one LobbyDTO in the result");
        assert_eq!(read_wstring(&peer.mem, first).as_deref(), Some(lobby.id.as_str()));

        // --- JoinLobby ------------------------------------------------------
        let id = wstr(&mut peer, &lobby.id);
        unsafe {
            (vt().JoinLobby)(
                peer.as_service(),
                id,
                &empty_function(),
                &empty_function(),
            )
        };
        unsafe { (vt().Tick)(peer.as_service()) };
        let joined = last(&peer, "JoinLobby").expect("JoinLobby completed");
        let joined_lobby = lobby_at(&peer, joined.argument);
        assert_eq!(
            joined_lobby.attributes.get("game_seed").map(String::as_str),
            Some("3735928559"),
            "the joining peer must receive the simulation seed"
        );
        let members_first = read_u32(&peer.mem, joined.argument + 92).unwrap();
        let members_last = read_u32(&peer.mem, joined.argument + 96).unwrap();
        assert_eq!(members_last - members_first, 2 * 96, "two LobbyMemberDTOs");
        assert_eq!(
            read_wstring(&peer.mem, members_first).as_deref(),
            Some("host")
        );
        assert_eq!(
            read_wstring(&peer.mem, members_first + 96).as_deref(),
            Some("peer")
        );

        // --- the P2P connection callbacks fire on both sides ----------------
        unsafe { (vt().Tick)(host.as_service()) };
        unsafe { (vt().Tick)(peer.as_service()) };
        assert!(last(&host, "SetP2PConnectionOpenedCallback").is_some());
        assert!(last(&host, "SetP2PDataChannelOpenedCallback").is_some());
        assert!(host.peer("peer").is_some());
        assert!(peer.peer("host").is_some());
        assert!(host.peer("peer").unwrap().is_open());

        // --- P2PSendToAll delivers to the other peer's data callback --------
        let bytes = b"NETMSG_COMMANDPACKAGEDATA";
        let data = packet(&mut host, bytes);
        assert!(unsafe { (vt().P2PSendToAll)(host.as_service(), data, bytes.len() as u32) });
        unsafe { (vt().Tick)(peer.as_service()) };
        let received = last(&peer, "SetReceivedP2PDataCallback").expect("data delivered");
        let mut got = alloc::vec![0u8; bytes.len()];
        assert!(peer.mem.read(received.argument, &mut got));
        assert_eq!(&got, bytes);

        // --- P2PSend to the specific ICrossplayPlayer we were handed --------
        let player = peer.peer("host").unwrap().as_ptr();
        let unicast = packet(&mut peer, b"pong");
        assert!(unsafe { (vt().P2PSend)(peer.as_service(), player, unicast, 4) });
        unsafe { (vt().Tick)(host.as_service()) };
        let back = last(&host, "SetReceivedP2PDataCallback").expect("unicast delivered");
        let mut got = alloc::vec![0u8; 4];
        assert!(host.mem.read(back.argument, &mut got));
        assert_eq!(&got, b"pong");

        // --- GetId on the peer object is a readable wstring -----------------
        let player_vtable = unsafe { &*(*(player as *const LocalPlayer)).vftable };
        let id_ref = unsafe { (player_vtable.GetId)(player) };
        assert_eq!(read_wstring(&peer.mem, gp_of(id_ref)).as_deref(), Some("host"));

        // --- StartGame publishes the reference ------------------------------
        let id = wstr(&mut host, &lobby.id);
        unsafe {
            (vt().StartGame)(
                host.as_service(),
                id,
                true,
                empty_function(),
                empty_function(),
            )
        };
        unsafe { (vt().Tick)(host.as_service()) };
        let started = last(&host, "StartGame").expect("StartGame completed");
        assert_eq!(
            read_wstring(&host.mem, started.argument).as_deref(),
            Some(lobby.id.as_str())
        );

        // --- LeaveLobby closes the peer ------------------------------------
        let id = wstr(&mut peer, &lobby.id);
        unsafe {
            (vt().LeaveLobby)(
                peer.as_service(),
                id,
                &empty_function(),
                &empty_function(),
            )
        };
        unsafe { (vt().Tick)(peer.as_service()) };
        assert!(last(&peer, "LeaveLobby").is_some());
        unsafe { (vt().Tick)(host.as_service()) };
        unsafe { (vt().Tick)(host.as_service()) };
        assert!(last(&host, "SetP2PConnectionClosedCallback").is_some());
        assert!(host.peer("peer").is_none());

        drop(host);
        drop(peer);
    }

    #[test]
    fn update_lobby_marshals_an_update_result_dto() {
        let mut svc = Svc::new(ArenaMem::new(), "host", "Host");
        start(&mut svc, "host");
        let map = attrs(&mut svc, &[("game_seed", "1")]);
        unsafe {
            (vt().CreateLobby)(
                svc.as_service(),
                4,
                VISIBILITY_PUBLIC,
                map,
                empty_function(),
                empty_function(),
            )
        };
        unsafe { (vt().Tick)(svc.as_service()) };
        let created = last(&svc, "CreateLobby").unwrap();
        let id_text = read_wstring(&svc.mem, created.argument).unwrap();

        let id = wstr(&mut svc, &id_text);
        let delta = attrs(&mut svc, &[("steam_ready_0", "1")]);
        unsafe {
            (vt().UpdateLobby)(
                svc.as_service(),
                id,
                0,
                delta,
                -1,
                0,
                &empty_function(),
                empty_function(),
            )
        };
        unsafe { (vt().Tick)(svc.as_service()) };
        let updated = last(&svc, "UpdateLobby").expect("UpdateLobby completed");
        let at = updated.argument;
        // UpdateLobbyResultDTO: bool +0, LobbyDTO +4, i64 +216.
        let mut success = [0u8; 1];
        assert!(svc.mem.read(at, &mut success));
        assert_eq!(success[0], 1);
        let attributes = read_attributes(&svc.mem, at + 4 + 104).unwrap();
        assert_eq!(attributes.get("steam_ready_0").map(String::as_str), Some("1"));
        assert_eq!(attributes.get("game_seed").map(String::as_str), Some("1"));
        // attribute_version went 1 -> 2, and DoN publishes it as the timestamp.
        assert_eq!(read_u32(&svc.mem, at + 4 + 84), Some(2));
        let mut ts = [0u8; 8];
        assert!(svc.mem.read(at + 216, &mut ts));
        assert_eq!(i64::from_le_bytes(ts), 2);
    }

    #[test]
    fn the_refused_slots_answer_with_not_available_and_never_succeed() {
        let mut svc = Svc::new(ArenaMem::new(), "host", "Host");
        start(&mut svc, "host");
        let w = wstr(&mut svc, "x");
        let map8 = MsvcMap8 { raw: [0; 8] };
        let f = empty_function();
        unsafe {
            (vt().SendLobbyChat)(svc.as_service(), w, w);
            (vt().JoinChat)(svc.as_service(), w, f, f);
            (vt().LeaveChat)(svc.as_service(), w);
            (vt().SendChatMessage)(svc.as_service(), w, w);
            (vt().SetStats)(svc.as_service(), &map8, f, f);
            (vt().GetStats)(svc.as_service(), w, f, f);
            (vt().GetGlobalLeaderboards)(svc.as_service(), w, 10, f, f);
            (vt().GetLeaderboardsAroundPlayer)(svc.as_service(), w, w, 10, f, f);
            (vt().CreateInvitation)(svc.as_service(), w, w, f, f);
            (vt().AcceptInvitation)(svc.as_service(), w, f, f);
        }
        let refused = [
            "SendLobbyChat",
            "JoinChat",
            "LeaveChat",
            "SendChatMessage",
            "SetStats",
            "GetStats",
            "GetGlobalLeaderboards",
            "GetLeaderboardsAroundPlayer",
            "CreateInvitation",
            "AcceptInvitation",
        ];
        for _ in 0..refused.len() + 2 {
            unsafe { (vt().Tick)(svc.as_service()) };
        }
        for slot in refused {
            let d = last(&svc, slot).unwrap_or_else(|| panic!("{slot} never answered"));
            assert!(
                !d.invoked,
                "{slot} must not have invoked anything (no callback installed here)"
            );
            let message = read_wstring(&svc.mem, d.argument).unwrap_or_default();
            assert!(
                message.contains(slot),
                "{slot}'s refusal must name itself, got {message:?}"
            );
            assert!(message.contains("not available"), "{message:?}");
        }
        // Nothing refused may leave a lobby behind.
        assert!(svc.backend().joined_lobby().is_none());
    }

    #[test]
    fn unreadable_arguments_fail_closed_rather_than_being_guessed() {
        let mut svc = Svc::new(ArenaMem::new(), "host", "Host");
        start(&mut svc, "host");
        unsafe {
            (vt().GetLobby)(
                svc.as_service(),
                core::ptr::null(),
                empty_function(),
                empty_function(),
            );
            (vt().CreateLobby)(
                svc.as_service(),
                4,
                VISIBILITY_PUBLIC,
                core::ptr::null(),
                empty_function(),
                empty_function(),
            );
            (vt().FindLobbies)(
                svc.as_service(),
                core::ptr::null(),
                empty_function(),
                empty_function(),
            );
        }
        for _ in 0..5 {
            unsafe { (vt().Tick)(svc.as_service()) };
        }
        for slot in ["GetLobby", "CreateLobby", "FindLobbies"] {
            let d = last(&svc, slot).unwrap_or_else(|| panic!("{slot} never answered"));
            let message = read_wstring(&svc.mem, d.argument).unwrap_or_default();
            assert!(message.contains("not available"), "{slot}: {message:?}");
        }
        // A null packet pointer is refused, not read.
        assert!(!unsafe { (vt().P2PSendToAll)(svc.as_service(), core::ptr::null_mut(), 8) });
        let data = packet(&mut svc, b"x");
        assert!(!unsafe { (vt().P2PSendToAll)(svc.as_service(), data, MAX_PACKET_BYTES + 1) });
    }

    #[test]
    fn callbacks_are_retained_on_the_slot_they_were_installed_on() {
        let svc = Svc::new(ArenaMem::new(), "host", "Host");
        // A `std::function` is "installed" iff `_Copy` produced a target. The
        // probe is a real six-slot `_Func_base` with an inline target, because
        // retention now runs guest code and a fabricated pointer would fault on
        // the target where that ABI is real.
        let mut f = empty_function();
        crate::func::probe::install(&mut f);
        unsafe {
            (vt().SetServiceErrorCallback)(svc.as_service(), &f);
            (vt().SetJoinLobbyCallback)(svc.as_service(), &f);
            (vt().SetLeaveLobbyCallback)(svc.as_service(), &f);
            (vt().SetUpdateLobbyCallback)(svc.as_service(), &f);
            (vt().SetLobbyMessageReceivedCallback)(svc.as_service(), &f);
            (vt().SetCrossplayEnabled)(svc.as_service(), &f);
            (vt().SetChatJoinCallback)(svc.as_service(), &f);
            (vt().SetChatLeaveCallback)(svc.as_service(), &f);
            (vt().SetChatMessageReceivedCallback)(svc.as_service(), &f);
            (vt().SetRenewTokenCallback)(svc.as_service(), (&f as *const MsvcFunction).cast());
            (vt().SetP2PConnectionOpenedCallback)(svc.as_service(), &f);
            (vt().SetP2PConnectionClosedCallback)(svc.as_service(), &f);
            (vt().SetP2PDataChannelOpenedCallback)(svc.as_service(), &f);
            (vt().SetP2PDataChannelClosedCallback)(svc.as_service(), &f);
            (vt().SetReceivedP2PTextCallback)(svc.as_service(), &f);
            (vt().SetReceivedP2PDataCallback)(svc.as_service(), &f);
            (vt().SetP2PConnectionFailedCallback)(
                svc.as_service(),
                (&f as *const MsvcFunction).cast(),
            );
        }
        for i in 0..cb::COUNT {
            assert!(svc.callback_installed(i), "{} not retained", cb::NAMES[i]);
        }
        // A null pointer must not clear or crash.
        unsafe { (vt().SetServiceErrorCallback)(svc.as_service(), core::ptr::null()) };
        assert!(svc.callback_installed(cb::SERVICE_ERROR));
    }

    #[test]
    fn the_three_lobby_broadcast_callbacks_are_retained_and_never_invoked() {
        // Their `bool` parameter is unobserved in both images, so invoking them
        // would mean inventing it. This test is the guard on that decision.
        let mut directory = Directory::new();
        let directory_ptr: *mut Directory = &mut directory;
        let mut host = unsafe { Svc::joined(ArenaMem::new(), "host", "Host", directory_ptr) };
        let mut peer = unsafe { Svc::joined(ArenaMem::new(), "peer", "Peer", directory_ptr) };
        start(&mut host, "host");
        start(&mut peer, "peer");
        let mut f = empty_function();
        crate::func::probe::install(&mut f);
        let calls_before = crate::func::probe::DO_CALLS.load(core::sync::atomic::Ordering::Relaxed);
        unsafe {
            (vt().SetJoinLobbyCallback)(host.as_service(), &f);
            (vt().SetLeaveLobbyCallback)(host.as_service(), &f);
            (vt().SetUpdateLobbyCallback)(host.as_service(), &f);
        }
        let map = attrs(&mut host, &[]);
        unsafe {
            (vt().CreateLobby)(
                host.as_service(),
                4,
                VISIBILITY_PUBLIC,
                map,
                empty_function(),
                empty_function(),
            )
        };
        unsafe { (vt().Tick)(host.as_service()) };
        let id_text = read_wstring(&host.mem, last(&host, "CreateLobby").unwrap().argument).unwrap();
        let id = wstr(&mut peer, &id_text);
        unsafe { (vt().JoinLobby)(peer.as_service(), id, &empty_function(), &empty_function()) };
        for _ in 0..4 {
            unsafe { (vt().Tick)(peer.as_service()) };
            unsafe { (vt().Tick)(host.as_service()) };
        }
        for slot in [
            "SetJoinLobbyCallback",
            "SetLeaveLobbyCallback",
            "SetUpdateLobbyCallback",
        ] {
            assert!(
                last(&host, slot).is_none(),
                "{slot} was invoked; its bool parameter is not derived"
            );
        }
        // The journal is this crate's own record; the probe counts from the
        // other side of the call, so a dispatch that skipped the journal would
        // still be caught.
        assert_eq!(
            crate::func::probe::DO_CALLS.load(core::sync::atomic::Ordering::Relaxed),
            calls_before,
            "a retained-but-never-invoked callback reached _Do_call"
        );
        drop(host);
        drop(peer);
    }

    #[test]
    fn stop_session_tears_down_and_is_idempotent() {
        let mut directory = Directory::new();
        let directory_ptr: *mut Directory = &mut directory;
        let mut host = unsafe { Svc::joined(ArenaMem::new(), "host", "Host", directory_ptr) };
        let mut peer = unsafe { Svc::joined(ArenaMem::new(), "peer", "Peer", directory_ptr) };
        start(&mut host, "host");
        start(&mut peer, "peer");
        let map = attrs(&mut host, &[]);
        unsafe {
            (vt().CreateLobby)(
                host.as_service(),
                4,
                VISIBILITY_PUBLIC,
                map,
                empty_function(),
                empty_function(),
            )
        };
        unsafe { (vt().Tick)(host.as_service()) };
        let id_text = read_wstring(&host.mem, last(&host, "CreateLobby").unwrap().argument).unwrap();
        let id = wstr(&mut peer, &id_text);
        unsafe { (vt().JoinLobby)(peer.as_service(), id, &empty_function(), &empty_function()) };
        unsafe { (vt().Tick)(peer.as_service()) };
        unsafe { (vt().Tick)(host.as_service()) };
        assert!(host.peer("peer").is_some());

        unsafe { (vt().StopSession)(host.as_service()) };
        assert_eq!(
            unsafe { (vt().GetSessionStatus)(host.as_service()) },
            SESSION_STATUS_STOPPED
        );
        assert!(!unsafe { (vt().IsConnectedToHub)(host.as_service()) });
        assert!(host.peer("peer").is_none(), "peers must be released");
        assert_eq!(
            read_wstring(&host.mem, gp_of(unsafe { (vt().GetPlayerGuid)(host.as_service()) }))
                .as_deref(),
            Some("")
        );
        // Idempotent: the shipped body returns immediately from Stopped.
        unsafe { (vt().StopSession)(host.as_service()) };
        assert_eq!(
            unsafe { (vt().GetSessionStatus)(host.as_service()) },
            SESSION_STATUS_STOPPED
        );
        drop(host);
        drop(peer);
    }

    #[test]
    fn lobby_cancel_pending_requests_drops_the_answer_without_crashing() {
        let mut svc = Svc::new(ArenaMem::new(), "host", "Host");
        start(&mut svc, "host");
        let map = attrs(&mut svc, &[]);
        unsafe {
            (vt().CreateLobby)(
                svc.as_service(),
                4,
                VISIBILITY_PUBLIC,
                map,
                empty_function(),
                empty_function(),
            )
        };
        unsafe { (vt().LobbyCancelPendingRequests)(svc.as_service()) };
        unsafe { (vt().Tick)(svc.as_service()) };
        assert!(last(&svc, "CreateLobby").is_none());
    }

    #[test]
    fn set_username_and_reliability_reach_the_backend() {
        let mut svc = Svc::new(ArenaMem::new(), "host", "Host");
        start(&mut svc, "host");
        let name = wstr(&mut svc, "Renamed Player");
        unsafe { (vt().SetUsername)(svc.as_service(), name) };
        assert_eq!(svc.backend().username(), "Renamed Player");
        unsafe { (vt().SetReliability)(svc.as_service(), false) };
        assert!(!svc.backend().reliable());
        unsafe { (vt().SetP2PTimeoutDuration)(svc.as_service(), 5000) };
        assert_eq!(svc.backend().p2p_timeout(), 5000);
        assert_eq!(
            unsafe { (vt().GetCrossplayStatus)(svc.as_service()) },
            crate::abi::CROSSPLAY_STATUS_ENABLED
        );
    }

    #[test]
    fn a_null_this_pointer_is_survivable_on_every_slot() {
        // Retail should never do this, but a slot that dereferences null turns a
        // bug elsewhere into a crash inside our DLL, which is the worst place
        // for it to surface.
        let null = core::ptr::null_mut();
        let f = empty_function();
        let map8 = MsvcMap8 { raw: [0; 8] };
        unsafe {
            (vt().Init)(null);
            (vt().SetServiceUrl)(null, core::ptr::null());
            (vt().SetServiceErrorCallback)(null, &f);
            (vt().SetReliability)(null, true);
            (vt().StartSession)(null, core::ptr::null(), &f, &f);
            (vt().StopSession)(null);
            (vt().SetRenewTokenCallback)(null, (&f as *const MsvcFunction).cast());
            assert_eq!((vt().GetSessionStatus)(null), SESSION_STATUS_STOPPED);
            (vt().GetCrossplayStatus)(null);
            (vt().BlockUser)(null, core::ptr::null());
            (vt().UnblockUser)(null, core::ptr::null());
            assert!(!(vt().IsUserBlocked)(null, core::ptr::null()));
            (vt().GetLobby)(null, core::ptr::null(), f, f);
            (vt().FindLobbies)(null, core::ptr::null(), f, f);
            (vt().CreateLobby)(null, 1, VISIBILITY_PUBLIC, core::ptr::null(), f, f);
            (vt().JoinLobby)(null, core::ptr::null(), &f, &f);
            (vt().SetJoinLobbyCallback)(null, &f);
            (vt().LeaveLobby)(null, core::ptr::null(), &f, &f);
            (vt().SetLeaveLobbyCallback)(null, &f);
            (vt().UpdateLobby)(null, core::ptr::null(), 0, core::ptr::null(), 0, 0, &f, f);
            (vt().SetUpdateLobbyCallback)(null, &f);
            (vt().LobbyCancelPendingRequests)(null);
            (vt().StartGame)(null, core::ptr::null(), false, f, f);
            (vt().CancelGameStart)(null, core::ptr::null(), f, f);
            (vt().SendLobbyChat)(null, core::ptr::null(), core::ptr::null());
            (vt().SetLobbyMessageReceivedCallback)(null, &f);
            (vt().SetCrossplayEnabled)(null, &f);
            (vt().SetStats)(null, &map8, f, f);
            (vt().GetStats)(null, core::ptr::null(), f, f);
            (vt().GetGlobalLeaderboards)(null, core::ptr::null(), 0, f, f);
            (vt().GetLeaderboardsAroundPlayer)(null, core::ptr::null(), core::ptr::null(), 0, f, f);
            (vt().CreateInvitation)(null, core::ptr::null(), core::ptr::null(), f, f);
            (vt().AcceptInvitation)(null, core::ptr::null(), f, f);
            assert!((vt().GetInvitationId)(null, core::ptr::null_mut()).is_null());
            (vt().JoinChat)(null, core::ptr::null(), f, f);
            (vt().SetChatJoinCallback)(null, &f);
            (vt().LeaveChat)(null, core::ptr::null());
            (vt().SetChatLeaveCallback)(null, &f);
            (vt().SendChatMessage)(null, core::ptr::null(), core::ptr::null());
            (vt().SetChatMessageReceivedCallback)(null, &f);
            (vt().SetP2PConnectionOpenedCallback)(null, &f);
            (vt().SetP2PConnectionClosedCallback)(null, &f);
            (vt().SetP2PDataChannelOpenedCallback)(null, &f);
            (vt().SetP2PDataChannelClosedCallback)(null, &f);
            (vt().SetReceivedP2PTextCallback)(null, &f);
            (vt().SetReceivedP2PDataCallback)(null, &f);
            (vt().SetP2PConnectionFailedCallback)(null, (&f as *const MsvcFunction).cast());
            (vt().SetP2PTimeoutDuration)(null, 0);
            (vt().P2PStartConnection)(null, core::ptr::null(), core::ptr::null());
            assert!(!(vt().P2PSendToAll)(null, core::ptr::null_mut(), 0));
            assert!(!(vt().P2PSend)(null, core::ptr::null_mut(), core::ptr::null_mut(), 0));
            (vt().P2PCloseAll)(null);
            (vt().P2PClose)(null, core::ptr::null());
            assert!(!(vt().IsConnectedToHub)(null));
            (vt().CreateLocalPlayerLoopback)(null);
            (vt().SetUsername)(null, core::ptr::null());
            assert!((vt().GetPlayerGuid)(null).is_null());
            (vt().Tick)(null);
        }
    }

    #[test]
    fn the_journal_is_bounded() {
        let mut svc = Svc::new(ArenaMem::new(), "host", "Host");
        let _ = &mut svc;
        start(&mut svc, "host");
        for _ in 0..(JOURNAL_CAPACITY + 50) {
            unsafe { (vt().LeaveChat)(svc.as_service(), core::ptr::null()) };
            unsafe { (vt().Tick)(svc.as_service()) };
        }
        assert_eq!(svc.journal().len(), JOURNAL_CAPACITY);
    }

    #[test]
    fn error_codes_reach_the_backend_unchanged() {
        // JoinLobby's error callback is `void(int, const wstring&)`; the code is
        // the first argument, so it must be the backend's, not a flattened one.
        let mut svc = Svc::new(ArenaMem::new(), "host", "Host");
        start(&mut svc, "host");
        let id = wstr(&mut svc, "nope");
        unsafe { (vt().JoinLobby)(svc.as_service(), id, &empty_function(), &empty_function()) };
        unsafe { (vt().Tick)(svc.as_service()) };
        let d = last(&svc, "JoinLobby").unwrap();
        let message = read_wstring(&svc.mem, d.argument).unwrap();
        assert_eq!(message, "no such lobby");
        assert_eq!(error::NOT_FOUND, -3);
    }

    #[cfg(feature = "std-rpc")]
    #[test]
    fn remote_service_completion_still_crosses_a_later_vtable_tick() {
        use crate::directory_rpc::DirectoryRpcClient;
        use std::thread;
        use std::time::{Duration, Instant};

        let directory = DirectoryRpcClient::listen("127.0.0.1:0").unwrap();
        let svc = Svc::remote(ArenaMem::new(), "host", "Host", Box::new(directory));
        let empty = empty_function();
        unsafe {
            (vt().Init)(svc.as_service());
            (vt().StartSession)(svc.as_service(), core::ptr::null(), &empty, &empty);
        }
        assert_eq!(svc.backend().session_status(), SESSION_STATUS_STARTING);

        // The first vtable Tick submits to the worker and cannot complete the
        // callback/journal entry inline on the game thread.
        unsafe { (vt().Tick)(svc.as_service()) };
        assert!(last(&svc, "StartSession").is_none());

        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline && last(&svc, "StartSession").is_none() {
            thread::sleep(Duration::from_millis(1));
            unsafe { (vt().Tick)(svc.as_service()) };
        }
        assert!(last(&svc, "StartSession").is_some());
        assert_eq!(svc.backend().session_status(), SESSION_STATUS_STARTED);
    }
}
