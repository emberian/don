//! `CrossplayNetLib.dll` — a drop-in replacement.
//!
//! `riseofnations.exe` does not import its network stack; it `LoadLibrary`s
//! `CrossplayNetLib.dll` and `GetProcAddress`es `get_netsys_object_ptr`
//! (`0x00538490`). Everything else it needs is reached either through the
//! returned object's 65-slot vtable or through nine directly-imported
//! `CrossplayNetLibSys` methods plus two free functions. Provide those twelve
//! symbols and the right vtable and the retail binary talks to us instead of to
//! PlayFab — with no patching of the game at all.
//!
//! # What this replaces it with
//!
//! A TCP transport plus the `don_net::session::Session` state machine, which is
//! the same code proven to hand turns between two OS processes (see
//! `crates/don-net/tests/tcp_session.rs` and the `donnet-peer` binary). The
//! packet formats on that transport are the shipped ones — `IPT_ADDPLAYER`,
//! `IPT_PLAYERLIST`, `IPT_READYFLAG`, `NETMSG_COMMANDPACKAGEDATA` — so a
//! `NetSys::send_all` from the real game is relayed verbatim.
//!
//! Configuration comes from the environment, because there is no UI to hang it
//! off and no config file the game would read on our behalf:
//!
//! | variable | meaning | default |
//! |---|---|---|
//! | `DON_NET_ROLE` | `host` or `join` | `host` |
//! | `DON_NET_BIND` | host listen address | `0.0.0.0:31337` |
//! | `DON_NET_ADDR` | address to dial when joining | `127.0.0.1:31337` |
//! | `DON_NET_ID` | our unique id, an i32 | pid-derived |
//! | `DON_NET_NAME` | our player name | `donnet` |
//!
//! # Honest status
//!
//! Built and layout-checked; **not yet loaded by the retail game**. The claim
//! this file supports is "the ABI is right and the transport works", proven by
//! the compile-time layout assertions in [`abi`], the export-table check in
//! `tools/check-exports.py`, and don-net's own tests. Whether the game gets
//! through its menu with this DLL in place is untested and needs the VM.
//! Do not upgrade that sentence without running it.

#![allow(non_snake_case)]
#![allow(clippy::missing_safety_doc)]

pub mod abi;
mod netsys;

use core::ffi::c_void;

pub use abi::*;

// ---------------------------------------------------------------------------
// The factory. `__cdecl`, two `StringTable*`, both ignorable — see abi.rs for
// the disassembly that proves the shipped DLL ignores the second one too.
// ---------------------------------------------------------------------------

/// `class NetSys * __cdecl get_netsys_object_ptr(class StringTable *, class StringTable *)`
///
/// Exported undecorated, exactly as the shipped DLL does (ordinal 11, and the
/// linker map spells it `_get_netsys_object_ptr`, i.e. plain `extern "C"`).
#[no_mangle]
pub extern "C" fn get_netsys_object_ptr(
    _localised_strings: *mut c_void,
    _internal_strings: *mut c_void,
) -> *mut NetSysBase {
    netsys::trace_once("export.get_netsys_object_ptr");
    netsys::create()
}

// ---------------------------------------------------------------------------
// The nine `CrossplayNetLibSys` methods and two free functions the exe imports
// directly. Mangled names are copied verbatim from the shipped DLL's export
// table (`pefile` dump of ron-bin/dll/CrossplayNetLib.dll) — a single character
// wrong here is an unresolved import and the game will not start.
// ---------------------------------------------------------------------------

/// `bool CrossplayNetLib::is_connected_to_network()` — `__cdecl`.
///
/// Shipped VA `0x10018550` calls WinINet's `InternetGetConnectedState` and
/// returns its nonzero result. Retail uses this before `NetSys::host`, so it
/// must not be coupled to our later roster/session-connected bit.
#[no_mangle]
pub extern "C" fn shim_is_connected_to_network() -> bool {
    netsys::trace_once("export.is_connected_to_network");
    netsys::network_available()
}

/// Shim-only diagnostic marker/action. The PE32 smoke resolves this before it
/// asks the load-only object to materialise its local NetPlayer; shipped DLLs
/// cannot satisfy that gate and are never called with synthetic arguments.
#[no_mangle]
pub unsafe extern "C" fn shim_materialize_load_only_peer(this: *mut NetSysBase) -> bool {
    netsys::materialize_load_only_peer(this)
}

/// Shim-only PE32 acceptance for the SetupWin `ConnectionData` bridge. The
/// supplied callbacks are inert smoke functions and this refuses retail mode.
#[no_mangle]
pub unsafe extern "C" fn shim_test_setup_bridge_sequence(
    this: *mut NetSysBase,
    connection_data: *mut c_void,
    add_player: *const c_void,
    get_player_index: *const c_void,
    send_player: *const c_void,
) -> bool {
    netsys::test_setup_bridge_sequence(
        this,
        connection_data,
        add_player,
        get_player_index,
        send_player,
    )
}

/// Shim-only load-only acceptance for MSVC `std::function` clone/move/delete
/// ownership. Pointer arguments keep inline targets at stable addresses so the
/// PE32 smoke can exercise replacement without relying on Rust's by-value move
/// to emulate a C++ copy constructor.
#[no_mangle]
pub unsafe extern "C" fn shim_test_p2p_callback_ownership(
    this: *mut NetSysBase,
    opened: *const MsvcFunction40,
    closed: *const MsvcFunction40,
    failed: *const MsvcFunction40,
) -> bool {
    if !netsys::is_load_only(this) || opened.is_null() || closed.is_null() || failed.is_null() {
        return false;
    }
    netsys::retain_p2p_callbacks(this, &*opened, &*closed, &*failed)
}

/// `void CrossplayNetLib::set_network_connection_state(bool)` — `__cdecl`.
#[no_mangle]
pub extern "C" fn shim_set_network_connection_state(state: bool) {
    netsys::trace_once("export.set_network_connection_state");
    netsys::set_connected(state);
}

/// `void CrossplayNetLibSys::send_ready_flag(bool)` — `__thiscall`.
///
/// The lobby-side entry point to `IPT_READYFLAG`. This one is load-bearing:
/// the game calls it to say the local player has readied, and our session
/// broadcasts the 2-byte packet.
#[no_mangle]
pub unsafe extern "thiscall" fn shim_send_ready_flag(this: *mut NetSysBase, ready: bool) {
    netsys::trace_once("export.send_ready_flag");
    if netsys::is_load_only(this) {
        return;
    }
    netsys::with(this, |s| {
        let _ = s.send_ready_flag(ready);
    });
}

/// `void CrossplayNetLibSys::reset_ready_flags()` — `__thiscall`.
#[no_mangle]
pub unsafe extern "thiscall" fn shim_reset_ready_flags(this: *mut NetSysBase) {
    netsys::trace_once("export.reset_ready_flags");
    if netsys::is_load_only(this) {
        return;
    }
    netsys::with(this, |s| {
        let _ = s.reset_ready_flags();
    });
}

/// `bool CrossplayNetLibSys::IsHost(const Crossplay::Lobby::DTO::LobbyMemberDTO&)`
///
/// We never see a real `LobbyMemberDTO` — there is no PlayFab lobby behind this
/// DLL — so the answer comes from our own role.
#[no_mangle]
pub unsafe extern "thiscall" fn shim_IsHost(this: *mut NetSysBase, _member: *const c_void) -> bool {
    netsys::trace_once("export.IsHost");
    netsys::role_is_host(this)
}

/// `void CrossplayNetLibSys::OnPlayerJoined(const LobbyMemberDTO&, const std::wstring&)`
#[no_mangle]
pub unsafe extern "thiscall" fn shim_OnPlayerJoined(
    this: *mut NetSysBase,
    member: *const c_void,
    _name: *const c_void,
) {
    netsys::trace_once("export.OnPlayerJoined");
    // The only authenticated DTO in the zero-credential route is retail's own
    // host. Retain its id solely so NetPlayer::get_id matches slot 0; remote
    // membership remains transport-driven and carries no credential material.
    let _ = netsys::capture_local_member_id(this, member);
}

/// `void CrossplayNetLibSys::OnPlayerLeft(const LobbyMemberDTO&)`
#[no_mangle]
pub unsafe extern "thiscall" fn shim_OnPlayerLeft_member(
    _this: *mut NetSysBase,
    _member: *const c_void,
) {
    netsys::trace_once("export.OnPlayerLeft.member");
}

/// `void CrossplayNetLibSys::OnPlayerLeft(const CrossplayNetLibPlayer*)` — the
/// overload taking our own player type.
#[no_mangle]
pub unsafe extern "thiscall" fn shim_OnPlayerLeft_player(
    _this: *mut NetSysBase,
    _player: *const c_void,
) {
    netsys::trace_once("export.OnPlayerLeft.player");
}

/// `void CrossplayNetLibSys::OnHostUpdated(const std::wstring&)`
#[no_mangle]
pub unsafe extern "thiscall" fn shim_OnHostUpdated(_this: *mut NetSysBase, _id: *const c_void) {
    netsys::trace_once("export.OnHostUpdated");
}

/// `void CrossplayNetLibSys::set_p2p_callbacks(function<...>, function<...>, function<...>)`
///
/// Three 40-byte `std::function`s by value. The shipped callee at
/// `0x10017420` clone-assigns them into the concrete object at
/// `+0x358/+0x380/+0x3A8`, then destroys each by-value input. Preserve the
/// same independent ownership even though the owned transport has no
/// Crossplay data-channel service event source to invoke them yet.
#[no_mangle]
pub unsafe extern "thiscall" fn shim_set_p2p_callbacks(
    this: *mut NetSysBase,
    mut opened: MsvcFunction40,
    mut closed: MsvcFunction40,
    mut failed: MsvcFunction40,
) {
    netsys::trace_once("export.set_p2p_callbacks");
    let _ = netsys::retain_p2p_callbacks(this, &opened, &closed, &failed);
    netsys::destroy_msvc_function(&mut opened);
    netsys::destroy_msvc_function(&mut closed);
    netsys::destroy_msvc_function(&mut failed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vtable_is_sixty_five_slots_of_four_bytes() {
        assert_eq!(core::mem::size_of::<NetSysVtable>(), 65 * 4);
        assert_eq!(core::mem::size_of::<NetSysVtable>(), 0x104);
    }

    #[test]
    fn netsys_base_matches_the_pdb_layout() {
        assert_eq!(core::mem::size_of::<NetSysBase>(), 88);
    }

    #[test]
    fn msvc_boundary_objects_match_the_pdb_and_callee_layouts() {
        assert_eq!(core::mem::size_of::<MsvcWstring>(), 24);
        assert_eq!(core::mem::offset_of!(MsvcWstring, len), 0x10);
        assert_eq!(core::mem::offset_of!(MsvcWstring, capacity), 0x14);
        assert_eq!(core::mem::size_of::<MsvcFunction40>(), 40);
        assert_eq!(core::mem::offset_of!(MsvcFunction40, target), 0x24);
    }
}
