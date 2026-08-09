//! The binary interface `riseofnations.exe` expects from `CrossplayNetLib.dll`.
//!
//! Everything in this file is layout, not behaviour. Getting it wrong is a
//! silent crash inside the game's own code, so every number carries where it
//! came from.
//!
//! # The factory — measured, and it corrects the prior lane
//!
//! `docs/tracks/netcode-symbols.md` §5 Option A listed the factory's first
//! argument as an unknown "`MemMgr` vtable" and called it the blocking unknown
//! for this whole approach. **It is not a `MemMgr` and there is no vtable
//! contract.** Two independent reads settle it:
//!
//! * Caller side. The loader is `0x00538490`. Its call is
//!   `get_netsys_object_ptr(&<0x00C8CCF0>, *(void**)0x00C06378)`. The PDB names
//!   those two: `0x00C8CCF0` is `loc_str_array_orig`, a 24-byte **`StringTable`**
//!   static in `basic/stringtable.obj`, and `0x00C06378` is `int_str_array`, a
//!   **`StringTable*`**. Both arguments are string tables — the localised one
//!   and the internal one.
//! * Callee side. Disassembling the shipped `CrossplayNetLib.dll` at
//!   `RVA 0x14B00` shows the whole function:
//!   ```text
//!   mov  eax, [ebp+8]          ; arg1 only
//!   push 0x3d4                 ; 4 + sizeof(CrossplayNetLibSys)=0x3d0
//!   mov  [0x1011a048], eax     ; stash arg1 in a DLL global
//!   call [0x1006f20c]          ; operator new
//!   ...
//!   ret                        ; no stack cleanup  => __cdecl
//!   ```
//!   `[ebp+0xC]` is **never read**. The second argument is ignored by the
//!   shipped implementation, so a replacement may ignore both.
//!
//! The mangled name in `CrossplayNetLib.map` is `_get_netsys_object_ptr`, and
//! the PDB signature demangles to
//! `class NetSys * __cdecl get_netsys_object_ptr(class StringTable *, class StringTable *)`.
//! **[measured]**
//!
//! Immediately after the factory returns, the loader calls two virtuals — these
//! must not fault or the game dies before the main menu:
//! `vt[0xE0]` = slot 56 `error_set_callback(void(*)(int))` and
//! `vt[0xFC]` = slot 63 `set_profiler(SysProfile*)`. **[measured, `0x00538490`]**
//!
//! # Calling convention
//!
//! Every virtual is an MSVC x86 C++ member function: `__thiscall` — `this` in
//! `ECX`, remaining arguments pushed right-to-left, **callee** cleans the stack.
//! Rust spells that `extern "thiscall"`, which is supported on
//! `i686-pc-windows-msvc`. The nine directly-imported `CrossplayNetLibSys`
//! methods are `QAE` in their mangled names, which is exactly `__thiscall`
//! public non-const; the two free functions are `YA` = `__cdecl`.

#![allow(non_snake_case)]

use core::ffi::c_void;

/// `NetSys::eLocalPlayerDisconnect`. **[measured]**
pub const LOCAL_PLAYER_CONNECTED: i32 = 0;
pub const LOCAL_PLAYER_DISCONNECTING: i32 = 1;
pub const LOCAL_PLAYER_DISCONNECTED: i32 = 2;

/// `LIBERR_OK`. The `Liberr` enum has 41 values; only success matters to us.
pub const LIBERR_OK: i32 = 0;

/// Number of virtual slots in `NetSys`. Byte offsets run 0..=0x100, so the
/// table is `65 * 4 = 260` bytes. **[measured, `CrossplayNetLib.pdb` type
/// `0x4572`]**
pub const NETSYS_VTABLE_SLOTS: usize = 65;

/// Byte offset of a slot index.
pub const fn slot_offset(i: usize) -> usize {
    i * 4
}

/// `NetSys`'s own data members, 88 bytes, reproduced exactly.
///
/// The game holds a `NetSys*` and calls through the vtable, but the base class
/// is not opaque to it — `num_players` and the `players` array are ordinary
/// public members of a class the game compiled against. Reproducing the layout
/// costs nothing and removes a whole class of failure.
///
/// ```text
/// + 0 __vftable                            + 4 int num_players
/// + 8 NetPlayer* players[8]                +40 NetPlayer* local_player
/// +44 NetPlayer* host_player               +48 eLocalPlayerDisconnect local_player_connection
/// +52 float local_player_disconnect_pct    +56 Array<NetSession*> net_sessions
/// +84 Log* log                             = 88
/// ```
/// **[measured]**
#[repr(C)]
pub struct NetSysBase {
    pub vftable: *const NetSysVtable,
    pub num_players: i32,
    pub players: [*mut NetPlayerObj; 8],
    pub local_player: *mut NetPlayerObj,
    pub host_player: *mut NetPlayerObj,
    pub local_player_connection: i32,
    pub local_player_disconnect_pct: f32,
    /// `Array<NetSession*>` is 28 bytes; we never hand it to the game, but the
    /// bytes must be there so `log` lands at +84.
    pub net_sessions: [u8; 28],
    pub log: *mut c_void,
}

const _: () = assert!(core::mem::size_of::<NetSysBase>() == 88);

/// The 65-slot `NetSys` vtable, in order. Byte offsets are in the comments and
/// come from `CrossplayNetLib.pdb`. **[measured]**
#[repr(C)]
pub struct NetSysVtable {
    /* 0x000 */ pub destructor: unsafe extern "thiscall" fn(*mut NetSysBase, u32) -> *mut c_void,
    /* 0x004 */
    pub init: unsafe extern "thiscall" fn(
        *mut NetSysBase,
        *mut c_void,
        *const c_void,
        i32,
        i32,
        u32,
        u32,
    ) -> i32,
    /* 0x008 */ pub close: unsafe extern "thiscall" fn(*mut NetSysBase),
    /* 0x00c */ pub get_memory_manager: unsafe extern "thiscall" fn(*mut NetSysBase) -> *mut c_void,
    /* 0x010 */ pub is_host: unsafe extern "thiscall" fn(*mut NetSysBase) -> bool,
    /* 0x014 */ pub enable_join: unsafe extern "thiscall" fn(*mut NetSysBase, i32),
    /* 0x018 */ pub set_playing: unsafe extern "thiscall" fn(*mut NetSysBase, i32),
    /* 0x01c */ pub is_playing: unsafe extern "thiscall" fn(*mut NetSysBase) -> i32,
    /* 0x020 */ pub is_joining_in_process: unsafe extern "thiscall" fn(*mut NetSysBase) -> i32,
    /* 0x024 */ pub is_session_full: unsafe extern "thiscall" fn(*mut NetSysBase) -> i32,
    /* 0x028 */
    pub get_url_string: unsafe extern "thiscall" fn(*mut NetSysBase, *mut c_void) -> *mut c_void,
    /* 0x02c */ pub accept_host_messages: unsafe extern "thiscall" fn(*mut NetSysBase, i32),
    /* 0x030 */ pub set_number_players: unsafe extern "thiscall" fn(*mut NetSysBase, i32),
    /* 0x034 */ pub set_number_observers: unsafe extern "thiscall" fn(*mut NetSysBase, i32),
    /* 0x038 */ pub send_dsync: unsafe extern "thiscall" fn(*mut NetSysBase, i32),
    /* 0x03c */ pub check_pulse: unsafe extern "thiscall" fn(*mut NetSysBase),
    /* 0x040 */ pub set_time_out: unsafe extern "thiscall" fn(*mut NetSysBase, u32),
    /* 0x044 */ pub get_time_out: unsafe extern "thiscall" fn(*mut NetSysBase) -> u32,
    /* 0x048 */ pub set_allow_timeout: unsafe extern "thiscall" fn(*mut NetSysBase, i32),
    /* 0x04c */ pub get_allow_timeout: unsafe extern "thiscall" fn(*mut NetSysBase) -> i32,
    /* 0x050 */ pub get_num_allowed_players: unsafe extern "thiscall" fn(*mut NetSysBase) -> i32,
    /* 0x054 */
    pub send: unsafe extern "thiscall" fn(
        *mut NetSysBase,
        *const u8,
        i32,
        *const NetPlayerObj,
        i32,
    ) -> bool,
    /* 0x058 */
    pub send_all: unsafe extern "thiscall" fn(*mut NetSysBase, *const u8, i32, i32) -> bool,
    /* 0x05c */
    pub get: unsafe extern "thiscall" fn(
        *mut NetSysBase,
        *mut u8,
        *mut *const NetPlayerObj,
        *mut u32,
    ) -> bool,
    /* 0x060 */ pub poll_services: unsafe extern "thiscall" fn(*mut NetSysBase, *mut c_void),
    /* 0x064 */
    pub host: unsafe extern "thiscall" fn(
        *mut NetSysBase,
        *const c_void,
        *const c_void,
        *const c_void,
        *const c_void,
        *const c_void,
    ) -> i32,
    /* 0x068 */
    pub join: unsafe extern "thiscall" fn(
        *mut NetSysBase,
        *const c_void,
        *const c_void,
        *const c_void,
        *const c_void,
        i32,
    ) -> i32,
    /* 0x06c */
    pub join_ip: unsafe extern "thiscall" fn(
        *mut NetSysBase,
        *const c_void,
        *const c_void,
        *const c_void,
        *const c_void,
        i32,
        i32,
    ) -> i32,
    /* 0x070 */ pub cancel_joining: unsafe extern "thiscall" fn(*mut NetSysBase),
    /* 0x074 */ pub cancel_join: unsafe extern "thiscall" fn(*mut NetSysBase),
    /* 0x078 */ pub cancel_join_skybox: unsafe extern "thiscall" fn(*mut NetSysBase),
    /* 0x07c */ pub disconnect: unsafe extern "thiscall" fn(*mut NetSysBase, bool),
    /* 0x080 */ pub poll_sessions: unsafe extern "thiscall" fn(*mut NetSysBase, *const c_void) -> i32,
    /* 0x084 */ pub stop_poll_sessions: unsafe extern "thiscall" fn(*mut NetSysBase),
    /* 0x088 */ pub clear_net_sessions: unsafe extern "thiscall" fn(*mut NetSysBase, u32),
    /* 0x08c */
    pub poll_players: unsafe extern "thiscall" fn(*mut NetSysBase, *const c_void, *mut c_void) -> i32,
    /* 0x090 */
    pub find_player_from_id:
        unsafe extern "thiscall" fn(*mut NetSysBase, *const c_void) -> *const NetPlayerObj,
    /* 0x094 */
    pub validate_player: unsafe extern "thiscall" fn(*mut NetSysBase, *const NetPlayerObj) -> i32,
    /* 0x098 */ pub update_recently_played_with_list: unsafe extern "thiscall" fn(*mut NetSysBase),
    /* 0x09c */ pub process_system_messages: unsafe extern "thiscall" fn(*mut NetSysBase),
    /* 0x0a0 */ pub send_drop_player: unsafe extern "thiscall" fn(*mut NetSysBase, *const NetPlayerObj),
    /* 0x0a4 */ pub drop_player: unsafe extern "thiscall" fn(*mut NetSysBase, *const NetPlayerObj),
    /* 0x0a8 */
    pub cancel_drop_player: unsafe extern "thiscall" fn(*mut NetSysBase, *const NetPlayerObj),
    /* 0x0ac */ pub set_log: unsafe extern "thiscall" fn(*mut NetSysBase, *mut c_void),
    /* 0x0b0 */ pub log_set_frame: unsafe extern "thiscall" fn(*mut NetSysBase, i32),
    /* 0x0b4 */ pub log_connection: unsafe extern "thiscall" fn(*mut NetSysBase, *const c_void),
    /* 0x0b8 */ pub log_connection2: unsafe extern "thiscall" fn(*mut NetSysBase, *const c_void),
    /// Variadic. MSVC compiles a member function with an ellipsis as `__cdecl`
    /// with `this` pushed as the first stack argument, **not** `__thiscall`, so
    /// this slot is `extern "C"` and the caller cleans the stack — which is why
    /// declaring only the two fixed parameters and ignoring the rest is safe.
    /* 0x0bc */
    pub log_connection_fmt: unsafe extern "C" fn(*mut NetSysBase, *const u16),
    /* 0x0c0 */ pub get_ip_addresses: unsafe extern "thiscall" fn(*mut NetSysBase) -> *mut c_void,
    /* 0x0c4 */ pub get_host_port: unsafe extern "thiscall" fn(*mut NetSysBase) -> u32,
    /* 0x0c8 */ pub set_host_port: unsafe extern "thiscall" fn(*mut NetSysBase, u32),
    /* 0x0cc */ pub get_local_port: unsafe extern "thiscall" fn(*mut NetSysBase) -> u32,
    /* 0x0d0 */ pub set_local_port: unsafe extern "thiscall" fn(*mut NetSysBase, u32),
    /* 0x0d4 */ pub set_ip_override: unsafe extern "thiscall" fn(*mut NetSysBase, *const c_void),
    /* 0x0d8 */ pub set_matchmaking_id: unsafe extern "thiscall" fn(*mut NetSysBase, i32),
    /* 0x0dc */ pub get_num_players: unsafe extern "thiscall" fn(*mut NetSysBase) -> i32,
    /* 0x0e0 */
    pub error_set_callback:
        unsafe extern "thiscall" fn(*mut NetSysBase, Option<unsafe extern "C" fn(i32)>),
    /* 0x0e4 */ pub get_group_data: unsafe extern "thiscall" fn(*mut NetSysBase) -> *mut c_void,
    /* 0x0e8 */ pub get_service_data: unsafe extern "thiscall" fn(*mut NetSysBase) -> *mut c_void,
    /* 0x0ec */ pub delete_group_data: unsafe extern "thiscall" fn(*mut NetSysBase, *mut c_void),
    /* 0x0f0 */ pub delete_service_data: unsafe extern "thiscall" fn(*mut NetSysBase, *mut c_void),
    /* 0x0f4 */ pub log_state: unsafe extern "thiscall" fn(*mut NetSysBase),
    /* 0x0f8 */
    pub notify_waiting_on_player: unsafe extern "thiscall" fn(*mut NetSysBase, i32, u32),
    /* 0x0fc */ pub set_profiler: unsafe extern "thiscall" fn(*mut NetSysBase, *mut c_void),
    /* 0x100 */ pub cleanup_system: unsafe extern "thiscall" fn(*mut NetSysBase),
}

const _: () =
    assert!(core::mem::size_of::<NetSysVtable>() == NETSYS_VTABLE_SLOTS * 4);
const _: () = assert!(core::mem::size_of::<NetSysVtable>() == 0x104);

/// `NetPlayer`, 21 virtuals. **[measured]**
///
/// The game calls these on the pointers our `get`/`players[]` hand back, so
/// every slot must be real. Byte offsets 0x00..=0x50.
#[repr(C)]
pub struct NetPlayerVtable {
    /* 0x00 */ pub destructor: unsafe extern "thiscall" fn(*mut NetPlayerObj, u32) -> *mut c_void,
    /* 0x04 */ pub is_local: unsafe extern "thiscall" fn(*mut NetPlayerObj) -> i32,
    /* 0x08 */ pub is_host: unsafe extern "thiscall" fn(*mut NetPlayerObj) -> i32,
    /* 0x0c */ pub is_pending: unsafe extern "thiscall" fn(*mut NetPlayerObj) -> i32,
    /* 0x10 */ pub is_observer: unsafe extern "thiscall" fn(*mut NetPlayerObj) -> i32,
    /* 0x14 */
    pub get_internal_name: unsafe extern "thiscall" fn(*mut NetPlayerObj) -> *const c_void,
    /* 0x18 */
    pub get_id: unsafe extern "thiscall" fn(*mut NetPlayerObj, *mut c_void) -> *mut c_void,
    /* 0x1c */
    pub get_platform_id: unsafe extern "thiscall" fn(*mut NetPlayerObj, *mut c_void) -> *mut c_void,
    /* 0x20 */
    pub get_platform: unsafe extern "thiscall" fn(*mut NetPlayerObj, *mut c_void) -> *mut c_void,
    /* 0x24 */ pub get_player_index: unsafe extern "thiscall" fn(*mut NetPlayerObj) -> i32,
    /* 0x28 */ pub get_game_version: unsafe extern "thiscall" fn(*mut NetPlayerObj) -> u32,
    /* 0x2c */ pub get_ping_time: unsafe extern "thiscall" fn(*mut NetPlayerObj) -> u32,
    /* 0x30 */
    pub get_time_since_last_pulse: unsafe extern "thiscall" fn(*mut NetPlayerObj) -> u32,
    /* 0x34 */
    pub get_send_queue_info: unsafe extern "thiscall" fn(*mut NetPlayerObj, *mut u32, *mut u32),
    /* 0x38 */ pub reset_sync_counter: unsafe extern "thiscall" fn(*mut NetPlayerObj),
    /* 0x3c */ pub inc_sync_counter: unsafe extern "thiscall" fn(*mut NetPlayerObj),
    /* 0x40 */ pub get_sync_counter: unsafe extern "thiscall" fn(*mut NetPlayerObj) -> u32,
    /* 0x44 */ pub get_name: unsafe extern "thiscall" fn(*mut NetPlayerObj) -> *const c_void,
    /* 0x48 */ pub get_description: unsafe extern "thiscall" fn(*mut NetPlayerObj) -> *const c_void,
    /* 0x4c */ pub set_name: unsafe extern "thiscall" fn(*mut NetPlayerObj, *const c_void),
    /* 0x50 */ pub get_dsync_frame: unsafe extern "thiscall" fn(*mut NetPlayerObj) -> i32,
}

const _: () = assert!(core::mem::size_of::<NetPlayerVtable>() == 21 * 4);

/// Our `NetPlayer` implementation object. The first word must be the vtable.
#[repr(C)]
pub struct NetPlayerObj {
    pub vftable: *const NetPlayerVtable,
    /// `CrossplayNetLibPlayer::unique_id` equivalent.
    pub unique_id: i32,
    pub flags: i32,
    pub sync_counter: u32,
    pub dsync_frame: i32,
    pub last_pulse_ms: u32,
    pub ping_ms: u32,
    pub player_index: i32,
    pub game_version: u32,
    /// UTF-16, NUL-terminated, so `get_id` can hand back a stable pointer.
    pub id_utf16: [u16; 40],
    /// Narrow name for `get_internal_name`.
    pub name_utf8: [u8; 64],
}

/// `CrossplayNetLibPlayer::Flags`. **[measured]**
pub const SNLPLAYER_HOST: i32 = 1;
pub const SNLPLAYER_PENDING: i32 = 2;
pub const SNLPLAYER_LOCAL: i32 = 4;
pub const SNLPLAYER_OBSERVER: i32 = 8;
