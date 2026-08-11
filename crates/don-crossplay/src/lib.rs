// SPDX-License-Identifier: GPL-3.0-or-later
//! `don-crossplay` — the checked interface `CrossplayProxy.dll` presents.
//!
//! `CrossplayProxy.dll` is Q-LOC's PlayFab bridge. It owns Lobby, Matchmaking
//! and Party P2P behind one façade, and `riseofnations.exe` reaches all of it
//! through **two** imported free functions:
//!
//! ```text
//! 0xac5038  ?Logger@Logging@Crossplay@@YAPAVICrossplayLogger@12@XZ   ordinal 1
//! 0xac503c  ?Service@Crossplay@@YAPAUICrossPlayService@1@XZ          ordinal 2
//! ```
//!
//! Everything else is virtual dispatch through the object `Service()` returns.
//! [`abi`] is that contract and nothing else; it is generated from the shipped
//! private PDBs by `gen/gen_abi.py` and carries compile-time assertions on every
//! vtable slot index, every DTO field offset, and the alignment of every
//! aggregate a slot takes by value — the last of which is part of the *calling
//! convention*, not the layout, and is the one thing here a layout assertion
//! cannot check for you.
//!
//! See [`abi`]'s module documentation for the two-`ICrossPlayService` hazard,
//! which is the single fact most likely to silently produce a wrong ABI here,
//! and `docs/tracks/crossplay-abi.md` for the evidence and the game-critical
//! assessment.
//!
//! # The local backend
//!
//! [`abi`] describes the interface. [`local`], [`msvc`] and [`service`]
//! *implement* it, DoN-side: a lobby directory with no PlayFab, no account
//! system, no WinHTTP and no Party network anywhere in the path.
//!
//! * [`local`] — the semantics. Sessions, lobbies, attributes, P2P routing.
//!   Pure data, no `unsafe`, and the honest home of every DoN policy decision.
//! * [`msvc`] — the object boundary. Reading a `wstring` or an
//!   `unordered_map<wstring, wstring>` retail handed us, and building the DTOs
//!   it reads back. Offsets measured from `CrossplayProxy.pdb` and confirmed
//!   against the shipped `_Find_last`.
//! * [`service`] — the 58-slot vtable, bound to the two above.
//!
//! * [`func`] — who owns an MSVC `std::function` that crossed the boundary, and
//!   the two measured vtable slots that transfer it.
//! * [`logger`] — the 4-slot `ICrossplayLogger` behind export ordinal 1, which
//!   retail dereferences with no null check.
//!
//! **None of it has been executed by `riseofnations.exe`.** It *has* been built
//! into a PE32 `CrossplayProxy.dll` (`../dll/`) and driven by a disposable PE32
//! loader; see `docs/tracks/crossplay-proxy-dll.md` and [`service`]'s module
//! documentation for exactly what that does and does not establish.
//!
//! # What this crate deliberately does not know
//!
//! * *Retail's behaviour.* A slot's signature says how to call it, not what the
//!   shipped implementation did. Where a shipped behaviour *was* measured — the
//!   session state machine, the inert slots, `IsConnectedToHub` — it is
//!   reproduced and cited; everything else in [`local`] is DoN's own policy and
//!   is marked as such.
//! * *Anything about PlayFab authentication*, the title id, or the Party
//!   session configuration. The local backend exists so none of it is needed.

// The whole crate is `no_std`: the backend needs an allocator and nothing else,
// so a future DLL that has to run before the CRT is fully up keeps its options.
// Under `cfg(test)` std comes back, because libtest needs it.
#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_op_in_unsafe_fn)]

extern crate alloc;
#[cfg(feature = "std-rpc")]
extern crate std;

pub mod abi;

#[cfg(feature = "std-rpc")]
pub mod directory_rpc;

#[cfg(feature = "local-match")]
pub mod match_bridge;

#[cfg(feature = "local")]
pub mod func;
#[cfg(feature = "local")]
pub mod local;
#[cfg(feature = "local")]
pub mod logger;
#[cfg(feature = "local")]
pub mod msvc;
#[cfg(feature = "local")]
pub mod service;

#[cfg(feature = "local")]
pub use local::{
    AsyncDirectory, AsyncDirectoryEvent, Attributes, Backend, Directory, DirectoryAnswer,
    DirectoryCall, DirectoryOperation, Emission, Lobby, Member, Notice, Outcome, ReqId,
};
#[cfg(feature = "local")]
pub use logger::LocalLogger;
#[cfg(feature = "local")]
pub use service::{LocalCrossPlayService, LocalPlayer};

pub use abi::{
    ICrossPlayService, ICrossPlayServiceVtable, ICrossplayLogger, ICrossplayLoggerVtable,
    ICrossplayPlayer, ICrossplayPlayerVtable, INetworkClient, INetworkClientVtable, LobbyDTO,
    LobbyMemberDTO, MsvcFunction, MsvcString, MsvcVector, MsvcWstring, UserDTO,
    ICROSSPLAYSERVICE_SLOTS, ICROSSPLAYSERVICE_VTABLE_BYTES_X86,
};

/// The two decorated names `riseofnations.exe` imports from
/// `CrossplayProxy.dll`, with the export ordinals the shipped DLL assigns them.
///
/// A replacement DLL must export both names at these ordinals. **[measured:
/// `ron-bin/dll/CrossplayProxy.dll` export directory]**
pub const SHIPPED_EXPORTS: [(u16, &str); 4] = [
    (1, "?Logger@Logging@Crossplay@@YAPAVICrossplayLogger@12@XZ"),
    (2, "?Service@Crossplay@@YAPAUICrossPlayService@1@XZ"),
    // Data exports, not functions: the two well-known symbols that ask a hybrid
    // laptop for the discrete GPU.
    (3, "AmdPowerXpressRequestHighPerformance"),
    (4, "NvOptimusEnablement"),
];

/// The exported *name* of ordinal 2 is not the name of the function behind it.
///
/// The PDB procedure at `RVA 0x130e0` is
/// `?Service@CrossplayProxy@@YAPAUICrossPlayService@1@XZ` —
/// `CrossplayProxy::Service()` returning `CrossplayProxy::ICrossPlayService*`,
/// from `Qloc\CrossplayProxy\src\crossplayproxy.cpp:190`. The export table
/// publishes that same address under the *game's* spelling,
/// `?Service@Crossplay@@YAPAUICrossPlayService@1@XZ`. The two decorated names
/// name the same code and the same 58-slot layout in different namespaces.
/// Ordinal 1 needs no such alias: its PDB name already is the exported one.
/// **[measured]**
pub const SERVICE_INTERNAL_NAME: &str = "?Service@CrossplayProxy@@YAPAUICrossPlayService@1@XZ";

/// Image base of the shipped `CrossplayProxy.dll`. **[measured]**
pub const IMAGE_BASE: u32 = 0x1000_0000;

/// RVA of `CrossplayProxy::CrossPlayService::\`vftable'`, the concrete table the
/// generator reads back to check every slot. **[measured]**
pub const IMPL_VFTABLE_RVA: u32 = 0x9_adc4;

/// RVA of `Crossplay::Logging::Logger` (export ordinal 1). **[measured]**
pub const LOGGER_RVA: u32 = 0x1_2180;

/// RVA of the function exported as `Crossplay::Service` (ordinal 2).
/// **[measured]**
pub const SERVICE_RVA: u32 = 0x1_30e0;

/// The single folded no-op body shared by eight `ICrossPlayService` slots.
///
/// `RVA 0x145b0` is one instruction, `ret 4`. MSVC identical-COMDAT-folding
/// collapsed eight one-argument interface methods onto it, so in *this* build
/// `SetServiceUrl`, `SetReliability`, `SetRenewTokenCallback`, `BlockUser`,
/// `UnblockUser`, `SetCrossplayEnabled`, `SetP2PConnectionFailedCallback` and
/// `P2PClose` do nothing at all. That is a fact about the shipped
/// implementation, not permission to omit the slots — they still occupy their
/// vtable entries and are still called. **[measured]**
pub const FOLDED_NOOP_RVA: u32 = 0x1_45b0;

/// The eight slots whose shipped implementation is [`FOLDED_NOOP_RVA`].
/// Values are vtable byte offsets. **[measured]**
///
/// In slot order: `SetServiceUrl`, `SetReliability`, `SetRenewTokenCallback`,
/// `BlockUser`, `UnblockUser`, `SetCrossplayEnabled`,
/// `SetP2PConnectionFailedCallback`, `P2PClose`.
pub const FOLDED_NOOP_SLOT_OFFSETS: [u32; 8] = [4, 12, 24, 36, 40, 104, 184, 208];

/// Two more slots whose shipped body is a bare return, at their own addresses
/// (their stack cleanup differs, so ICF could not fold them onto
/// [`FOLDED_NOOP_RVA`]): `P2PStartConnection` at `+192` is `ret 8` at
/// `RVA 0x1dc60`, and `CreateLocalPlayerLoopback` at `+216` is `ret 0` at
/// `RVA 0x1e350`. **[measured]**
pub const INERT_SLOT_OFFSETS: [u32; 2] = [192, 216];

/// `IsUserBlocked` (`+44`, `RVA 0x14f70`) is `xor al, al; ret 4` — a constant
/// `false`, not a lookup. **[measured]**
pub const CONSTANT_FALSE_SLOT_OFFSET: u32 = 44;

/// `CrossPlayService`'s session-status field, at object offset `+0x930`.
///
/// `GetSessionStatus` (`+28`) is `mov eax, [ecx+0x930]; ret`, and
/// `IsConnectedToHub` (`+212`) is `cmp [ecx+0x930], 2; sete al` — so
/// "connected to hub" is exactly `SessionStatus::Started`. Recorded because it
/// is the cheapest thing a replacement has to get right for the many retail
/// call sites that gate on `GetSessionStatus`. **[measured]**
pub const SESSION_STATUS_FIELD_OFFSET: u32 = 0x930;
