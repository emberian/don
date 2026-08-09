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
//! `i686-pc-windows-msvc`. The eight directly-imported `CrossplayNetLibSys`
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

/// `LIBERR_NOT_AVAILABLE`. The complete `Liberr` enum comes from
/// `CrossplayNetLib.pdb` type `0x13B0`; diagnostic/load-only mode returns this
/// from operations which would enter a network session.
pub const LIBERR_NOT_AVAILABLE: i32 = 26;

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

/// MSVC `ObjectArray<String>` embedded in `CrossplayNetLibSys` at concrete
/// offset `+0x1A0`. The vtable owns `increase_size`; the returned array is a
/// borrowed owner-lifetime object and its `String` elements have stride 0x14.
/// **[measured, CrossplayNetLibSys PDB + 0x10015B30]**
#[repr(C)]
pub struct MsvcObjectArrayString {
    pub vftable: *const c_void,
    pub length: i32,
    pub size: i32,
    pub increment: i16,
    pub padding_0e: i16,
    pub list: *mut c_void,
    pub flags: u8,
    pub padding_15: [u8; 3],
}

impl MsvcObjectArrayString {
    /// Field image before the pinned retail constructor installs its vtable.
    /// The load-only smoke never grows or destroys this process-lifetime object.
    pub const fn empty_unconstructed() -> Self {
        Self {
            vftable: core::ptr::null(),
            length: 0,
            size: 0,
            increment: -1,
            padding_0e: 0,
            list: core::ptr::null_mut(),
            flags: 0,
            padding_15: [0; 3],
        }
    }
}

const _: () = {
    assert!(core::mem::size_of::<MsvcObjectArrayString>() == 0x18);
    assert!(core::mem::offset_of!(MsvcObjectArrayString, length) == 0x04);
    assert!(core::mem::offset_of!(MsvcObjectArrayString, size) == 0x08);
    assert!(core::mem::offset_of!(MsvcObjectArrayString, increment) == 0x0c);
    assert!(core::mem::offset_of!(MsvcObjectArrayString, list) == 0x10);
    assert!(core::mem::offset_of!(MsvcObjectArrayString, flags) == 0x14);
};

/// The 65-slot `NetSys` vtable, in order. Byte offsets are in the comments and
/// come from `CrossplayNetLib.pdb`. **[measured]**
#[repr(C)]
pub struct NetSysVtable {
    /* 0x000 */
    pub destructor: unsafe extern "thiscall" fn(*mut NetSysBase, u32) -> *mut c_void,
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
    /* 0x00c */
    pub get_memory_manager: unsafe extern "thiscall" fn(*mut NetSysBase) -> *mut c_void,
    /* 0x010 */ pub is_host: unsafe extern "thiscall" fn(*mut NetSysBase) -> bool,
    /* 0x014 */ pub enable_join: unsafe extern "thiscall" fn(*mut NetSysBase, i32),
    /* 0x018 */ pub set_playing: unsafe extern "thiscall" fn(*mut NetSysBase, i32),
    /* 0x01c */ pub is_playing: unsafe extern "thiscall" fn(*mut NetSysBase) -> i32,
    /* 0x020 */
    pub is_joining_in_process: unsafe extern "thiscall" fn(*mut NetSysBase) -> i32,
    /* 0x024 */ pub is_session_full: unsafe extern "thiscall" fn(*mut NetSysBase) -> i32,
    /* 0x028 */
    pub get_url_string: unsafe extern "thiscall" fn(*mut NetSysBase) -> *const MsvcGameString,
    /* 0x02c */ pub accept_host_messages: unsafe extern "thiscall" fn(*mut NetSysBase, i32),
    /* 0x030 */ pub set_number_players: unsafe extern "thiscall" fn(*mut NetSysBase, i32),
    /* 0x034 */ pub set_number_observers: unsafe extern "thiscall" fn(*mut NetSysBase, i32),
    /* 0x038 */ pub send_dsync: unsafe extern "thiscall" fn(*mut NetSysBase, i32),
    /* 0x03c */ pub check_pulse: unsafe extern "thiscall" fn(*mut NetSysBase),
    /* 0x040 */ pub set_time_out: unsafe extern "thiscall" fn(*mut NetSysBase, u32),
    /* 0x044 */ pub get_time_out: unsafe extern "thiscall" fn(*mut NetSysBase) -> u32,
    /* 0x048 */ pub set_allow_timeout: unsafe extern "thiscall" fn(*mut NetSysBase, i32),
    /* 0x04c */ pub get_allow_timeout: unsafe extern "thiscall" fn(*mut NetSysBase) -> i32,
    /* 0x050 */
    pub get_num_allowed_players: unsafe extern "thiscall" fn(*mut NetSysBase) -> i32,
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
    /* 0x060 */
    pub poll_services: unsafe extern "thiscall" fn(*mut NetSysBase, *mut c_void) -> i32,
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
    /* 0x07c */ pub disconnect: unsafe extern "thiscall" fn(*mut NetSysBase, bool) -> i32,
    /* 0x080 */
    pub poll_sessions: unsafe extern "thiscall" fn(*mut NetSysBase, *const c_void) -> i32,
    /* 0x084 */ pub stop_poll_sessions: unsafe extern "thiscall" fn(*mut NetSysBase),
    /* 0x088 */ pub clear_net_sessions: unsafe extern "thiscall" fn(*mut NetSysBase, u32),
    /* 0x08c */
    pub poll_players:
        unsafe extern "thiscall" fn(*mut NetSysBase, *const c_void, MsvcArrayNetPlayers) -> i32,
    /* 0x090 */
    pub find_player_from_id:
        unsafe extern "thiscall" fn(*mut NetSysBase, MsvcWstring) -> *const NetPlayerObj,
    /* 0x094 */
    pub validate_player: unsafe extern "thiscall" fn(*mut NetSysBase, *const NetPlayerObj) -> i32,
    /* 0x098 */
    pub update_recently_played_with_list: unsafe extern "thiscall" fn(*mut NetSysBase),
    /* 0x09c */ pub process_system_messages: unsafe extern "thiscall" fn(*mut NetSysBase),
    /* 0x0a0 */
    pub send_drop_player: unsafe extern "thiscall" fn(*mut NetSysBase, *const NetPlayerObj),
    /* 0x0a4 */
    pub drop_player: unsafe extern "thiscall" fn(*mut NetSysBase, *const NetPlayerObj),
    /* 0x0a8 */
    pub cancel_drop_player: unsafe extern "thiscall" fn(*mut NetSysBase, *const NetPlayerObj),
    /* 0x0ac */ pub set_log: unsafe extern "thiscall" fn(*mut NetSysBase, *mut c_void),
    /* 0x0b0 */ pub log_set_frame: unsafe extern "thiscall" fn(*mut NetSysBase, i32),
    /* 0x0b4 */
    pub log_connection: unsafe extern "thiscall" fn(*mut NetSysBase, *const c_void),
    /* 0x0b8 */
    pub log_connection2: unsafe extern "thiscall" fn(*mut NetSysBase, *const c_void, i32),
    /// Variadic. MSVC compiles a member function with an ellipsis as `__cdecl`
    /// with `this` pushed as the first stack argument, **not** `__thiscall`, so
    /// this slot is `extern "C"` and the caller cleans the stack — which is why
    /// declaring only the two fixed parameters and ignoring the rest is safe.
    /* 0x0bc */
    pub log_connection_fmt: unsafe extern "C" fn(*mut NetSysBase, *const u16),
    /* 0x0c0 */
    pub get_ip_addresses:
        unsafe extern "thiscall" fn(*mut NetSysBase) -> *const MsvcObjectArrayString,
    /* 0x0c4 */ pub get_host_port: unsafe extern "thiscall" fn(*mut NetSysBase) -> u32,
    /* 0x0c8 */ pub set_host_port: unsafe extern "thiscall" fn(*mut NetSysBase, u32),
    /* 0x0cc */ pub get_local_port: unsafe extern "thiscall" fn(*mut NetSysBase) -> u32,
    /* 0x0d0 */ pub set_local_port: unsafe extern "thiscall" fn(*mut NetSysBase, u32),
    /* 0x0d4 */
    pub set_ip_override: unsafe extern "thiscall" fn(*mut NetSysBase, *const c_void),
    /* 0x0d8 */ pub set_matchmaking_id: unsafe extern "thiscall" fn(*mut NetSysBase, i32),
    /* 0x0dc */ pub get_num_players: unsafe extern "thiscall" fn(*mut NetSysBase) -> i32,
    /* 0x0e0 */
    pub error_set_callback:
        unsafe extern "thiscall" fn(*mut NetSysBase, Option<unsafe extern "C" fn(i32)>),
    /* 0x0e4 */
    pub get_group_data: unsafe extern "thiscall" fn(*mut NetSysBase, *mut c_void) -> *mut c_void,
    /* 0x0e8 */
    pub get_service_data: unsafe extern "thiscall" fn(*mut NetSysBase, *mut c_void) -> *mut c_void,
    /* 0x0ec */
    pub delete_group_data: unsafe extern "thiscall" fn(*mut NetSysBase, *mut c_void),
    /* 0x0f0 */
    pub delete_service_data: unsafe extern "thiscall" fn(*mut NetSysBase, *mut c_void),
    /* 0x0f4 */ pub log_state: unsafe extern "thiscall" fn(*mut NetSysBase),
    /* 0x0f8 */
    pub notify_waiting_on_player: unsafe extern "thiscall" fn(*mut NetSysBase, i32, u32),
    /* 0x0fc */ pub set_profiler: unsafe extern "thiscall" fn(*mut NetSysBase, *mut c_void),
    /* 0x100 */ pub cleanup_system: unsafe extern "thiscall" fn(*mut NetSysBase),
}

const _: () = assert!(core::mem::size_of::<NetSysVtable>() == NETSYS_VTABLE_SLOTS * 4);
const _: () = assert!(core::mem::size_of::<NetSysVtable>() == 0x104);

// Every field offset is the byte offset printed by `CrossplayNetLib.pdb` type
// `0x4572`. Checking only the total size would not catch an accidentally
// reordered pair with equal-sized function pointers.
const _: () = {
    assert!(core::mem::offset_of!(NetSysVtable, destructor) == 0x000);
    assert!(core::mem::offset_of!(NetSysVtable, init) == 0x004);
    assert!(core::mem::offset_of!(NetSysVtable, close) == 0x008);
    assert!(core::mem::offset_of!(NetSysVtable, get_memory_manager) == 0x00c);
    assert!(core::mem::offset_of!(NetSysVtable, is_host) == 0x010);
    assert!(core::mem::offset_of!(NetSysVtable, enable_join) == 0x014);
    assert!(core::mem::offset_of!(NetSysVtable, set_playing) == 0x018);
    assert!(core::mem::offset_of!(NetSysVtable, is_playing) == 0x01c);
    assert!(core::mem::offset_of!(NetSysVtable, is_joining_in_process) == 0x020);
    assert!(core::mem::offset_of!(NetSysVtable, is_session_full) == 0x024);
    assert!(core::mem::offset_of!(NetSysVtable, get_url_string) == 0x028);
    assert!(core::mem::offset_of!(NetSysVtable, accept_host_messages) == 0x02c);
    assert!(core::mem::offset_of!(NetSysVtable, set_number_players) == 0x030);
    assert!(core::mem::offset_of!(NetSysVtable, set_number_observers) == 0x034);
    assert!(core::mem::offset_of!(NetSysVtable, send_dsync) == 0x038);
    assert!(core::mem::offset_of!(NetSysVtable, check_pulse) == 0x03c);
    assert!(core::mem::offset_of!(NetSysVtable, set_time_out) == 0x040);
    assert!(core::mem::offset_of!(NetSysVtable, get_time_out) == 0x044);
    assert!(core::mem::offset_of!(NetSysVtable, set_allow_timeout) == 0x048);
    assert!(core::mem::offset_of!(NetSysVtable, get_allow_timeout) == 0x04c);
    assert!(core::mem::offset_of!(NetSysVtable, get_num_allowed_players) == 0x050);
    assert!(core::mem::offset_of!(NetSysVtable, send) == 0x054);
    assert!(core::mem::offset_of!(NetSysVtable, send_all) == 0x058);
    assert!(core::mem::offset_of!(NetSysVtable, get) == 0x05c);
    assert!(core::mem::offset_of!(NetSysVtable, poll_services) == 0x060);
    assert!(core::mem::offset_of!(NetSysVtable, host) == 0x064);
    assert!(core::mem::offset_of!(NetSysVtable, join) == 0x068);
    assert!(core::mem::offset_of!(NetSysVtable, join_ip) == 0x06c);
    assert!(core::mem::offset_of!(NetSysVtable, cancel_joining) == 0x070);
    assert!(core::mem::offset_of!(NetSysVtable, cancel_join) == 0x074);
    assert!(core::mem::offset_of!(NetSysVtable, cancel_join_skybox) == 0x078);
    assert!(core::mem::offset_of!(NetSysVtable, disconnect) == 0x07c);
    assert!(core::mem::offset_of!(NetSysVtable, poll_sessions) == 0x080);
    assert!(core::mem::offset_of!(NetSysVtable, stop_poll_sessions) == 0x084);
    assert!(core::mem::offset_of!(NetSysVtable, clear_net_sessions) == 0x088);
    assert!(core::mem::offset_of!(NetSysVtable, poll_players) == 0x08c);
    assert!(core::mem::offset_of!(NetSysVtable, find_player_from_id) == 0x090);
    assert!(core::mem::offset_of!(NetSysVtable, validate_player) == 0x094);
    assert!(core::mem::offset_of!(NetSysVtable, update_recently_played_with_list) == 0x098);
    assert!(core::mem::offset_of!(NetSysVtable, process_system_messages) == 0x09c);
    assert!(core::mem::offset_of!(NetSysVtable, send_drop_player) == 0x0a0);
    assert!(core::mem::offset_of!(NetSysVtable, drop_player) == 0x0a4);
    assert!(core::mem::offset_of!(NetSysVtable, cancel_drop_player) == 0x0a8);
    assert!(core::mem::offset_of!(NetSysVtable, set_log) == 0x0ac);
    assert!(core::mem::offset_of!(NetSysVtable, log_set_frame) == 0x0b0);
    assert!(core::mem::offset_of!(NetSysVtable, log_connection) == 0x0b4);
    assert!(core::mem::offset_of!(NetSysVtable, log_connection2) == 0x0b8);
    assert!(core::mem::offset_of!(NetSysVtable, log_connection_fmt) == 0x0bc);
    assert!(core::mem::offset_of!(NetSysVtable, get_ip_addresses) == 0x0c0);
    assert!(core::mem::offset_of!(NetSysVtable, get_host_port) == 0x0c4);
    assert!(core::mem::offset_of!(NetSysVtable, set_host_port) == 0x0c8);
    assert!(core::mem::offset_of!(NetSysVtable, get_local_port) == 0x0cc);
    assert!(core::mem::offset_of!(NetSysVtable, set_local_port) == 0x0d0);
    assert!(core::mem::offset_of!(NetSysVtable, set_ip_override) == 0x0d4);
    assert!(core::mem::offset_of!(NetSysVtable, set_matchmaking_id) == 0x0d8);
    assert!(core::mem::offset_of!(NetSysVtable, get_num_players) == 0x0dc);
    assert!(core::mem::offset_of!(NetSysVtable, error_set_callback) == 0x0e0);
    assert!(core::mem::offset_of!(NetSysVtable, get_group_data) == 0x0e4);
    assert!(core::mem::offset_of!(NetSysVtable, get_service_data) == 0x0e8);
    assert!(core::mem::offset_of!(NetSysVtable, delete_group_data) == 0x0ec);
    assert!(core::mem::offset_of!(NetSysVtable, delete_service_data) == 0x0f0);
    assert!(core::mem::offset_of!(NetSysVtable, log_state) == 0x0f4);
    assert!(core::mem::offset_of!(NetSysVtable, notify_waiting_on_player) == 0x0f8);
    assert!(core::mem::offset_of!(NetSysVtable, set_profiler) == 0x0fc);
    assert!(core::mem::offset_of!(NetSysVtable, cleanup_system) == 0x100);
};

/// The 24-byte MSVC x86 `std::wstring` object used by the shipped DLL.
///
/// `CrossplayNetLibPlayer::get_id` at shipped VA `0x10027220` constructs its
/// hidden return object by writing `size=0` at `+0x10`, `capacity=7` at
/// `+0x14`, and a UTF-16 NUL at `+0`, then assigning the source string. The
/// string helpers select heap storage only when `capacity >= 8` (for example
/// `0x10017612`). We intentionally emit only the measured seven-code-unit SSO
/// representation, so retail's destructor never attempts to free memory
/// allocated by another CRT.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MsvcWstring {
    pub sso: [u16; 8],
    pub len: u32,
    pub capacity: u32,
}

const _: () = assert!(core::mem::size_of::<MsvcWstring>() == 24);
const _: () = assert!(core::mem::offset_of!(MsvcWstring, len) == 0x10);
const _: () = assert!(core::mem::offset_of!(MsvcWstring, capacity) == 0x14);

/// Big Huge Games' own `String`, returned by value from two `NetPlayer`
/// virtuals and by reference from `NetSys::get_url_string`.
///
/// The class is 20 bytes in `CrossplayNetLib.pdb`. Its canonical empty image
/// has `module_id=1` at byte 11; it owns no allocation.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MsvcGameString {
    pub bytes: [u8; 20],
}

const _: () = assert!(core::mem::size_of::<MsvcGameString>() == 20);

/// `Array<NetPlayer*>`, passed **by value** to `NetSys::poll_players`.
///
/// PDB size is 28 bytes. Keeping the aggregate by value is load-bearing on
/// x86: the callee must emit `ret 0x20` for the preceding `NetSession*` plus
/// these 28 inline bytes.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MsvcArrayNetPlayers {
    pub bytes: [u8; 28],
}

const _: () = assert!(core::mem::size_of::<MsvcArrayNetPlayers>() == 28);

/// `NetMessenger`'s complete 12-slot retail callback surface. **[measured,
/// `rise.pdb` class `NetMessenger`, vtable `0x00B247E8`]**
#[repr(C)]
pub struct NetMessengerVtable {
    /* 0x00 */
    pub destructor: unsafe extern "thiscall" fn(*mut NetMessenger, u32) -> *mut c_void,
    /* 0x04 */
    pub on_send_failed: unsafe extern "thiscall" fn(*mut NetMessenger, *const NetPlayerObj),
    /* 0x08 */
    pub on_player_added: unsafe extern "thiscall" fn(*mut NetMessenger, *const NetPlayerObj),
    /* 0x0c */
    pub on_name_changed: unsafe extern "thiscall" fn(*mut NetMessenger, *const NetPlayerObj),
    /* 0x10 */ pub on_session_lost: unsafe extern "thiscall" fn(*mut NetMessenger),
    /* 0x14 */
    pub on_host_migrate: unsafe extern "thiscall" fn(*mut NetMessenger, *const NetPlayerObj),
    /* 0x18 */ pub on_join: unsafe extern "thiscall" fn(*mut NetMessenger, i32, u32),
    /* 0x1c */
    pub on_player_deleted: unsafe extern "thiscall" fn(*mut NetMessenger, *const NetPlayerObj),
    /* 0x20 */
    pub on_player_timed_out: unsafe extern "thiscall" fn(*mut NetMessenger, *const NetPlayerObj),
    /* 0x24 */
    pub on_player_pulse: unsafe extern "thiscall" fn(*mut NetMessenger, *const NetPlayerObj),
    /* 0x28 */
    pub allow_connection: unsafe extern "thiscall" fn(*mut NetMessenger, *const c_void) -> i32,
    /* 0x2c */
    pub get_session_data: unsafe extern "thiscall" fn(*mut NetMessenger) -> *const c_void,
}

#[repr(C)]
pub struct NetMessenger {
    pub vftable: *const NetMessengerVtable,
}

const _: () = assert!(core::mem::size_of::<NetMessengerVtable>() == 12 * 4);
const _: () = assert!(core::mem::size_of::<NetMessenger>() == 4);

/// The by-value MSVC x86 `std::function` representation used by
/// `CrossplayNetLibSys::set_p2p_callbacks`.
///
/// All three specialisations are 40 bytes in `CrossplayNetLib.pdb`. The
/// shipped callee receives them inline at `[ebp+8]`, `[ebp+0x30]`, and
/// `[ebp+0x58]`, reads the target pointer at object `+0x24`, invokes its
/// vtable `+0x10` destructor with `target != object_base`, and returns with
/// `ret 0x78` (`0x10017420..0x100175a8`).
#[repr(C)]
pub struct MsvcFunction40 {
    pub storage: [u8; 36],
    pub target: *mut c_void,
}

const _: () = assert!(core::mem::size_of::<MsvcFunction40>() == 40);
const _: () = assert!(core::mem::offset_of!(MsvcFunction40, target) == 0x24);

/// `NetPlayer`, 21 virtuals. **[measured]**
///
/// The game calls these on the pointers our `get`/`players[]` hand back, so
/// every slot must be real. Byte offsets 0x00..=0x50.
#[repr(C)]
pub struct NetPlayerVtable {
    /* 0x00 */
    pub destructor: unsafe extern "thiscall" fn(*mut NetPlayerObj, u32) -> *mut c_void,
    /* 0x04 */ pub is_local: unsafe extern "thiscall" fn(*mut NetPlayerObj) -> i32,
    /* 0x08 */ pub is_host: unsafe extern "thiscall" fn(*mut NetPlayerObj) -> i32,
    /* 0x0c */ pub is_pending: unsafe extern "thiscall" fn(*mut NetPlayerObj) -> i32,
    /* 0x10 */ pub is_observer: unsafe extern "thiscall" fn(*mut NetPlayerObj) -> i32,
    /* 0x14 */
    pub get_internal_name:
        unsafe extern "thiscall" fn(*mut NetPlayerObj, *mut MsvcGameString) -> *mut MsvcGameString,
    /* 0x18 */
    pub set_name: unsafe extern "thiscall" fn(*mut NetPlayerObj, *const MsvcGameString),
    /* 0x1c */
    pub get_internal_description:
        unsafe extern "thiscall" fn(*mut NetPlayerObj, *mut MsvcGameString) -> *mut MsvcGameString,
    /* 0x20 */
    pub set_description: unsafe extern "thiscall" fn(*mut NetPlayerObj, *const MsvcGameString),
    /* 0x24 */
    pub get_id:
        unsafe extern "thiscall" fn(*mut NetPlayerObj, *mut MsvcWstring) -> *mut MsvcWstring,
    /* 0x28 */
    pub get_platform_id:
        unsafe extern "thiscall" fn(*mut NetPlayerObj, *mut MsvcWstring) -> *mut MsvcWstring,
    /* 0x2c */
    pub get_platform:
        unsafe extern "thiscall" fn(*mut NetPlayerObj, *mut MsvcWstring) -> *mut MsvcWstring,
    /* 0x30 */ pub get_player_index: unsafe extern "thiscall" fn(*mut NetPlayerObj) -> i32,
    /* 0x34 */ pub get_game_version: unsafe extern "thiscall" fn(*mut NetPlayerObj) -> u32,
    /* 0x38 */ pub get_ping_time: unsafe extern "thiscall" fn(*mut NetPlayerObj) -> u32,
    /* 0x3c */
    pub get_time_since_last_pulse: unsafe extern "thiscall" fn(*mut NetPlayerObj) -> u32,
    /* 0x40 */
    pub get_send_queue_info: unsafe extern "thiscall" fn(*mut NetPlayerObj, *mut u32, *mut u32),
    /* 0x44 */ pub reset_sync_counter: unsafe extern "thiscall" fn(*mut NetPlayerObj),
    /* 0x48 */ pub inc_sync_counter: unsafe extern "thiscall" fn(*mut NetPlayerObj),
    /* 0x4c */ pub get_sync_counter: unsafe extern "thiscall" fn(*mut NetPlayerObj) -> i32,
    /* 0x50 */ pub log_state: unsafe extern "thiscall" fn(*mut NetPlayerObj),
}

const _: () = assert!(core::mem::size_of::<NetPlayerVtable>() == 21 * 4);
const _: () = {
    assert!(core::mem::offset_of!(NetPlayerVtable, destructor) == 0x00);
    assert!(core::mem::offset_of!(NetPlayerVtable, is_local) == 0x04);
    assert!(core::mem::offset_of!(NetPlayerVtable, is_host) == 0x08);
    assert!(core::mem::offset_of!(NetPlayerVtable, is_pending) == 0x0c);
    assert!(core::mem::offset_of!(NetPlayerVtable, is_observer) == 0x10);
    assert!(core::mem::offset_of!(NetPlayerVtable, get_internal_name) == 0x14);
    assert!(core::mem::offset_of!(NetPlayerVtable, set_name) == 0x18);
    assert!(core::mem::offset_of!(NetPlayerVtable, get_internal_description) == 0x1c);
    assert!(core::mem::offset_of!(NetPlayerVtable, set_description) == 0x20);
    assert!(core::mem::offset_of!(NetPlayerVtable, get_id) == 0x24);
    assert!(core::mem::offset_of!(NetPlayerVtable, get_platform_id) == 0x28);
    assert!(core::mem::offset_of!(NetPlayerVtable, get_platform) == 0x2c);
    assert!(core::mem::offset_of!(NetPlayerVtable, get_player_index) == 0x30);
    assert!(core::mem::offset_of!(NetPlayerVtable, get_game_version) == 0x34);
    assert!(core::mem::offset_of!(NetPlayerVtable, get_ping_time) == 0x38);
    assert!(core::mem::offset_of!(NetPlayerVtable, get_time_since_last_pulse) == 0x3c);
    assert!(core::mem::offset_of!(NetPlayerVtable, get_send_queue_info) == 0x40);
    assert!(core::mem::offset_of!(NetPlayerVtable, reset_sync_counter) == 0x44);
    assert!(core::mem::offset_of!(NetPlayerVtable, inc_sync_counter) == 0x48);
    assert!(core::mem::offset_of!(NetPlayerVtable, get_sync_counter) == 0x4c);
    assert!(core::mem::offset_of!(NetPlayerVtable, log_state) == 0x50);
};

/// Our `NetPlayer` implementation object. The first 172 bytes reproduce the
/// shipped `CrossplayNetLibPlayer`; private metadata follows that prefix.
#[repr(C)]
pub struct NetPlayerObj {
    pub vftable: *const NetPlayerVtable,
    pub drop_requests: [u8; 28],
    pub flags: i32,
    pub crossplay_player: *mut c_void,
    pub ready: bool,
    pub ready_padding: [u8; 3],
    pub id: MsvcWstring,
    pub platform: MsvcWstring,
    pub platform_id: MsvcWstring,
    pub name: MsvcGameString,
    pub description: MsvcGameString,
    pub game_version: u32,
    pub last_pulse_ms: u32,
    pub sync_counter: u32,
    pub dsync_frame: i32,
    // Shim-private tail; retail's measured object ends at byte 172.
    pub unique_id: i32,
    pub player_index: i32,
    pub ping_ms: u32,
    pub id_wide: [u16; 129],
    pub id_len: u16,
    pub name_wide: [u16; 65],
    pub name_len: u16,
}

const _: () = {
    assert!(core::mem::offset_of!(NetPlayerObj, flags) == 32);
    assert!(core::mem::offset_of!(NetPlayerObj, crossplay_player) == 36);
    assert!(core::mem::offset_of!(NetPlayerObj, ready) == 40);
    assert!(core::mem::offset_of!(NetPlayerObj, id) == 44);
    assert!(core::mem::offset_of!(NetPlayerObj, platform) == 68);
    assert!(core::mem::offset_of!(NetPlayerObj, platform_id) == 92);
    assert!(core::mem::offset_of!(NetPlayerObj, name) == 116);
    assert!(core::mem::offset_of!(NetPlayerObj, description) == 136);
    assert!(core::mem::offset_of!(NetPlayerObj, game_version) == 156);
    assert!(core::mem::offset_of!(NetPlayerObj, last_pulse_ms) == 160);
    assert!(core::mem::offset_of!(NetPlayerObj, sync_counter) == 164);
    assert!(core::mem::offset_of!(NetPlayerObj, dsync_frame) == 168);
    assert!(core::mem::offset_of!(NetPlayerObj, unique_id) == 172);
};

/// `CrossplayNetLibPlayer::Flags`. **[measured]**
pub const SNLPLAYER_HOST: i32 = 1;
pub const SNLPLAYER_PENDING: i32 = 2;
pub const SNLPLAYER_LOCAL: i32 = 4;
pub const SNLPLAYER_OBSERVER: i32 = 8;
