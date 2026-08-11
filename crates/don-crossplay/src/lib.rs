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
//! This crate is that contract and nothing else: **no method is implemented, no
//! DLL is built, nothing is replaced.** [`abi`] is generated from the shipped
//! private PDBs by `gen/gen_abi.py` and carries compile-time assertions on every
//! vtable slot index and every DTO field offset.
//!
//! See [`abi`]'s module documentation for the two-`ICrossPlayService` hazard,
//! which is the single fact most likely to silently produce a wrong ABI here,
//! and `docs/tracks/crossplay-abi.md` for the evidence and the game-critical
//! assessment.
//!
//! # What this crate deliberately does not know
//!
//! * *Behaviour.* A slot's signature says how to call it, not what it does.
//! * *Which slots retail actually calls.* No call-site trace has been taken.
//!   The assessment in `docs/tracks/crossplay-abi.md` is evidence-weighted, not
//!   measured dispatch.
//! * *Anything about PlayFab authentication*, the title id, or the Party
//!   session configuration.

#![no_std]
#![forbid(unsafe_op_in_unsafe_fn)]

pub mod abi;

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
