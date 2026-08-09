//! Load-only executable acceptance for the replacement CrossplayNetLib.dll.
//!
//! This is deliberately a separate PE32 process. Static export inspection
//! cannot prove that Windows can load the DLL, that the Rust runtime starts,
//! or that an x86 `__thiscall` crosses the vtable correctly. This host performs
//! exactly those operations while `DON_NET_LOAD_ONLY=1` makes every network or
//! session entry point fail closed.

#![allow(clippy::missing_safety_doc)]

use core::ffi::c_void;
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};

const NETSYS_VTABLE_SLOTS: usize = 65;
const LIBERR_NOT_AVAILABLE: i32 = 26;

const FACTORY: &[u8] = b"get_netsys_object_ptr\0";
const IS_CONNECTED: &[u8] = b"?is_connected_to_network@CrossplayNetLib@@YA_NXZ\0";
const SET_CONNECTED: &[u8] = b"?set_network_connection_state@CrossplayNetLib@@YAX_N@Z\0";
const SEND_READY: &[u8] = b"?send_ready_flag@CrossplayNetLibSys@@QAEX_N@Z\0";
const RESET_READY: &[u8] = b"?reset_ready_flags@CrossplayNetLibSys@@QAEXXZ\0";
const IS_HOST: &[u8] =
    b"?IsHost@CrossplayNetLibSys@@QAE_NABVLobbyMemberDTO@DTO@Lobby@Crossplay@@@Z\0";
const ON_PLAYER_JOINED: &[u8] = b"?OnPlayerJoined@CrossplayNetLibSys@@QAEXABVLobbyMemberDTO@DTO@Lobby@Crossplay@@ABV?$basic_string@_WU?$char_traits@_W@std@@V?$allocator@_W@2@@std@@@Z\0";
const SHIM_MARKER: &[u8] = b"shim_is_connected_to_network\0";
const SHIM_MATERIALIZE: &[u8] = b"shim_materialize_load_only_peer\0";

const SHIPPED_EXPORTS: &[&[u8]] = &[
    b"?IsHost@CrossplayNetLibSys@@QAE_NABVLobbyMemberDTO@DTO@Lobby@Crossplay@@@Z\0",
    b"?OnHostUpdated@CrossplayNetLibSys@@QAEXABV?$basic_string@_WU?$char_traits@_W@std@@V?$allocator@_W@2@@std@@@Z\0",
    b"?OnPlayerJoined@CrossplayNetLibSys@@QAEXABVLobbyMemberDTO@DTO@Lobby@Crossplay@@ABV?$basic_string@_WU?$char_traits@_W@std@@V?$allocator@_W@2@@std@@@Z\0",
    b"?OnPlayerLeft@CrossplayNetLibSys@@QAEXABVLobbyMemberDTO@DTO@Lobby@Crossplay@@@Z\0",
    b"?OnPlayerLeft@CrossplayNetLibSys@@QAEXPBVCrossplayNetLibPlayer@@@Z\0",
    b"?is_connected_to_network@CrossplayNetLib@@YA_NXZ\0",
    b"?reset_ready_flags@CrossplayNetLibSys@@QAEXXZ\0",
    b"?send_ready_flag@CrossplayNetLibSys@@QAEX_N@Z\0",
    b"?set_network_connection_state@CrossplayNetLib@@YAX_N@Z\0",
    b"?set_p2p_callbacks@CrossplayNetLibSys@@QAEXV?$function@$$A6AXPAVICrossplayPlayer@P2P@Crossplay@@@Z@std@@0V?$function@$$A6AXV?$basic_string@_WU?$char_traits@_W@std@@V?$allocator@_W@2@@std@@0@Z@3@@Z\0",
    FACTORY,
];

#[repr(C)]
struct NetSysPrefix {
    vftable: *const *const c_void,
    num_players: i32,
    players: [*mut NetPlayerPrefix; 8],
    local_player: *mut NetPlayerPrefix,
    host_player: *mut NetPlayerPrefix,
    base_tail: [u8; 40],
    flags: i32,
    net_messenger: *mut c_void,
    reserved_to_crossplay: [u8; 108],
    m_crossplay: *mut c_void,
}

#[repr(C)]
struct NetPlayerPrefix {
    vftable: *const *const c_void,
    drop_requests: [u8; 28],
    flags: i32,
    crossplay_player: *mut c_void,
    ready: bool,
    ready_padding: [u8; 3],
    id: MsvcWstring,
}

#[repr(C)]
struct FakeMessenger {
    vftable: *const *const c_void,
    callbacks: u32,
    added: u32,
    flags_seen_on_add: i32,
    crossplay_non_null_on_add: bool,
    id_seen_on_add: MsvcWstring,
    session_data: u16,
}

#[repr(C)]
struct FakeService {
    vftable: *const *const c_void,
    timeout_ms: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct MsvcGameString {
    bytes: [u8; 20],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct MsvcWstring {
    sso: [u16; 8],
    len: u32,
    capacity: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct MsvcArrayNetPlayers {
    bytes: [u8; 28],
}

const _: () = assert!(std::mem::size_of::<MsvcGameString>() == 20);
const _: () = assert!(std::mem::size_of::<MsvcWstring>() == 24);
const _: () = assert!(std::mem::size_of::<MsvcArrayNetPlayers>() == 28);
const _: () = assert!(std::mem::offset_of!(NetSysPrefix, flags) == 0x58);
const _: () = assert!(std::mem::offset_of!(NetSysPrefix, net_messenger) == 0x5c);
const _: () = assert!(std::mem::offset_of!(NetSysPrefix, m_crossplay) == 0xcc);
const _: () = assert!(std::mem::offset_of!(NetPlayerPrefix, flags) == 0x20);
const _: () = assert!(std::mem::offset_of!(NetPlayerPrefix, crossplay_player) == 0x24);

#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryW(path: *const u16) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
    fn FreeLibrary(module: *mut c_void) -> i32;
    fn GetLastError() -> u32;
}

struct Module(*mut c_void);

impl Drop for Module {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                FreeLibrary(self.0);
            }
        }
    }
}

unsafe extern "thiscall" fn msg_dtor(this: *mut FakeMessenger, _flags: u32) -> *mut c_void {
    this.cast()
}
unsafe extern "thiscall" fn msg_player(_this: *mut FakeMessenger, _player: *const NetPlayerPrefix) {
}
unsafe extern "thiscall" fn msg_added(this: *mut FakeMessenger, player: *const NetPlayerPrefix) {
    (*this).callbacks += 1;
    (*this).added += 1;
    if !player.is_null() {
        (*this).flags_seen_on_add = (*player).flags;
        (*this).crossplay_non_null_on_add = !(*player).crossplay_player.is_null();
        (*this).id_seen_on_add = (*player).id;
    }
}
unsafe extern "thiscall" fn msg_void(_this: *mut FakeMessenger) {}
unsafe extern "thiscall" fn msg_join(_this: *mut FakeMessenger, _result: i32, _version: u32) {}
unsafe extern "thiscall" fn msg_allow(_this: *mut FakeMessenger, _data: *const c_void) -> i32 {
    1
}
unsafe extern "thiscall" fn msg_session(this: *mut FakeMessenger) -> *const c_void {
    (&raw const (*this).session_data).cast()
}
unsafe extern "thiscall" fn service_set_timeout(this: *mut FakeService, timeout_ms: i32) {
    (*this).timeout_ms = timeout_ms;
}

fn main() {
    match run() {
        Ok(report) => println!("{report}"),
        Err(error) => fail(&error),
    }
}

fn run() -> Result<String, String> {
    let mut args = env::args_os().skip(1);
    let dll = args.next().map(PathBuf::from).unwrap_or(default_dll()?);
    let trace = args.next().map(PathBuf::from).unwrap_or_else(default_trace);
    if args.next().is_some() {
        return Err("usage: netsys-load-smoke.exe [CrossplayNetLib.dll] [trace.log]".into());
    }
    if !dll.is_file() {
        return Err(format!("DLL does not exist: {}", dll.display()));
    }
    if trace.exists() {
        return Err(format!(
            "refusing to append to existing trace: {}",
            trace.display()
        ));
    }

    env::set_var("DON_NET_LOAD_ONLY", "1");
    env::set_var("DON_NET_NAME", "Ai");
    env::set_var("DON_NET_TRACE", &trace);
    env::set_var("DON_NET_CONNECTIVITY_OVERRIDE", "1");

    let wide = wide_path(&dll);
    let raw_module = unsafe { LoadLibraryW(wide.as_ptr()) };
    if raw_module.is_null() {
        return Err(format!(
            "LoadLibraryW failed for {}: win32={}",
            dll.display(),
            unsafe { GetLastError() }
        ));
    }
    let module = Module(raw_module);

    // Refuse the shipped DLL (or any unrelated DLL with the same factory)
    // before calling into it with null StringTable arguments. This internal
    // alias is intentionally present only in our replacement.
    resolve(&module, SHIM_MARKER)?;

    for name in SHIPPED_EXPORTS {
        resolve(&module, name)?;
    }

    let mut stack_pointer_checks = 0usize;
    let factory: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut NetSysPrefix =
        unsafe { std::mem::transmute(resolve(&module, FACTORY)?) };
    let object = checked_call("factory", &mut stack_pointer_checks, || unsafe {
        factory(core::ptr::null_mut(), core::ptr::null_mut())
    })?;
    if object.is_null() {
        return Err("factory returned null in load-only mode".into());
    }
    if unsafe { (*object).vftable.is_null() } {
        return Err("factory returned an object with a null vtable".into());
    }
    if unsafe { (*object).num_players } != 0 {
        return Err(format!(
            "NetSys prefix num_players: expected shipped constructor state 0, got {}",
            unsafe { (*object).num_players }
        ));
    }

    let vtable = unsafe { (*object).vftable };
    for slot in 0..NETSYS_VTABLE_SLOTS {
        if unsafe { (*vtable.add(slot)).is_null() } {
            return Err(format!("NetSys vtable slot {slot} is null"));
        }
    }
    if unsafe { (*object).flags } != 0x40 {
        return Err(format!(
            "NetSys concrete flags: expected shipped constructor state 0x40, got 0x{:x}",
            unsafe { (*object).flags }
        ));
    }

    // These are the two virtual calls made immediately by NetSys::load_dll at
    // retail VA 0x00538490 after the factory returns.
    let error_set_callback: unsafe extern "thiscall" fn(
        *mut NetSysPrefix,
        Option<unsafe extern "C" fn(i32)>,
    ) = unsafe { vtable_fn(vtable, 56) };
    let set_profiler: unsafe extern "thiscall" fn(*mut NetSysPrefix, *mut c_void) =
        unsafe { vtable_fn(vtable, 63) };
    checked_call(
        "NetSys[56] error_set_callback",
        &mut stack_pointer_checks,
        || unsafe { error_set_callback(object, None) },
    )?;
    checked_call(
        "NetSys[63] set_profiler",
        &mut stack_pointer_checks,
        || unsafe { set_profiler(object, core::ptr::null_mut()) },
    )?;

    // Close the next exact retail boundary: load_steam_net_lib calls slot1,
    // then reads the retained service at object+0xCC and dispatches service
    // slot47/+0xBC SetP2PTimeoutDuration(20). The fakes are inert and local.
    let messenger_vtable = [
        msg_dtor as *const c_void,
        msg_player as *const c_void,
        msg_added as *const c_void,
        msg_player as *const c_void,
        msg_void as *const c_void,
        msg_player as *const c_void,
        msg_join as *const c_void,
        msg_player as *const c_void,
        msg_player as *const c_void,
        msg_player as *const c_void,
        msg_allow as *const c_void,
        msg_session as *const c_void,
    ];
    let mut messenger = FakeMessenger {
        vftable: messenger_vtable.as_ptr(),
        callbacks: 0,
        added: 0,
        flags_seen_on_add: 0,
        crossplay_non_null_on_add: false,
        id_seen_on_add: MsvcWstring {
            sso: [0; 8],
            len: 0,
            capacity: 7,
        },
        session_data: 0x5a31,
    };
    let mut service_vtable = [core::ptr::null::<c_void>(); 58];
    service_vtable[47] = service_set_timeout as *const c_void;
    let mut service = FakeService {
        vftable: service_vtable.as_ptr(),
        timeout_ms: -1,
    };
    let init: unsafe extern "thiscall" fn(
        *mut NetSysPrefix,
        *mut c_void,
        *const c_void,
        i32,
        i32,
        u32,
        u32,
    ) -> i32 = unsafe { vtable_fn(vtable, 1) };
    let init_result = checked_call("NetSys[1] init", &mut stack_pointer_checks, || unsafe {
        init(
            object,
            (&mut messenger as *mut FakeMessenger).cast(),
            (&service as *const FakeService).cast(),
            0x1234,
            0,
            0,
            0,
        )
    })?;
    if init_result != 0
        || unsafe { (*object).net_messenger } != (&mut messenger as *mut FakeMessenger).cast()
        || unsafe { (*object).m_crossplay } != (&mut service as *mut FakeService).cast()
    {
        return Err(format!(
            "NetSys[1] did not retain exact +0x5C/+0xCC pointers: result={init_result}"
        ));
    }
    if messenger.callbacks != 0 {
        return Err("NetSys[1] invoked a NetMessenger callback during init".into());
    }
    let retained_service = unsafe { (*object).m_crossplay.cast::<FakeService>() };
    let retained_vtable = unsafe { (*retained_service).vftable };
    let set_timeout: unsafe extern "thiscall" fn(*mut FakeService, i32) =
        unsafe { vtable_fn(retained_vtable, 47) };
    checked_call(
        "ICrossPlayService[47] SetP2PTimeoutDuration",
        &mut stack_pointer_checks,
        || unsafe { set_timeout(retained_service, 20) },
    )?;
    if service.timeout_ms != 20 {
        return Err("retail post-init service slot47 did not receive timeout 20".into());
    }

    // Prove the diagnostic cannot silently become a network session.
    let host: unsafe extern "thiscall" fn(
        *mut NetSysPrefix,
        *const c_void,
        *const c_void,
        *const c_void,
        *const c_void,
        *const c_void,
    ) -> i32 = unsafe { vtable_fn(vtable, 25) };
    let join: unsafe extern "thiscall" fn(
        *mut NetSysPrefix,
        *const c_void,
        *const c_void,
        *const c_void,
        *const c_void,
        i32,
    ) -> i32 = unsafe { vtable_fn(vtable, 26) };
    let send_all: unsafe extern "thiscall" fn(*mut NetSysPrefix, *const u8, i32, i32) -> bool =
        unsafe { vtable_fn(vtable, 22) };
    let get: unsafe extern "thiscall" fn(
        *mut NetSysPrefix,
        *mut u8,
        *mut *const c_void,
        *mut u32,
    ) -> bool = unsafe { vtable_fn(vtable, 23) };

    let host_result = checked_call("NetSys[25] host", &mut stack_pointer_checks, || unsafe {
        host(
            object,
            core::ptr::null(),
            core::ptr::null(),
            core::ptr::null(),
            core::ptr::null(),
            core::ptr::null(),
        )
    })?;
    let join_result = checked_call("NetSys[26] join", &mut stack_pointer_checks, || unsafe {
        join(
            object,
            core::ptr::null(),
            core::ptr::null(),
            core::ptr::null(),
            core::ptr::null(),
            0,
        )
    })?;
    let send_result = checked_call(
        "NetSys[22] send_all",
        &mut stack_pointer_checks,
        || unsafe { send_all(object, core::ptr::null(), 0, 0) },
    )?;
    let get_result = checked_call("NetSys[23] get", &mut stack_pointer_checks, || unsafe {
        get(
            object,
            core::ptr::null_mut(),
            core::ptr::null_mut(),
            core::ptr::null_mut(),
        )
    })?;
    if host_result != LIBERR_NOT_AVAILABLE || join_result != LIBERR_NOT_AVAILABLE {
        return Err(format!(
            "load-only lifecycle gate failed: host={host_result} join={join_result}"
        ));
    }
    if send_result || get_result {
        return Err(format!(
            "load-only traffic gate failed: send={send_result} get={get_result}"
        ));
    }

    // Exercise every NetSys slot whose PDB signature disagreed with the
    // earlier prototype. The ESP guard makes callee-cleanup drift observable
    // in this PE32 process instead of waiting for retail to corrupt its stack.
    let get_url_string: unsafe extern "thiscall" fn(*mut NetSysPrefix) -> *const MsvcGameString =
        unsafe { vtable_fn(vtable, 10) };
    let poll_services: unsafe extern "thiscall" fn(*mut NetSysPrefix, *mut c_void) -> i32 =
        unsafe { vtable_fn(vtable, 24) };
    let disconnect: unsafe extern "thiscall" fn(*mut NetSysPrefix, bool) -> i32 =
        unsafe { vtable_fn(vtable, 31) };
    let poll_players: unsafe extern "thiscall" fn(
        *mut NetSysPrefix,
        *const c_void,
        MsvcArrayNetPlayers,
    ) -> i32 = unsafe { vtable_fn(vtable, 35) };
    let find_player_from_id: unsafe extern "thiscall" fn(
        *mut NetSysPrefix,
        MsvcWstring,
    ) -> *const NetPlayerPrefix = unsafe { vtable_fn(vtable, 36) };
    let log_connection2: unsafe extern "thiscall" fn(*mut NetSysPrefix, *const c_void, i32) =
        unsafe { vtable_fn(vtable, 46) };
    let get_group_data: unsafe extern "thiscall" fn(*mut NetSysPrefix, *mut c_void) -> *mut c_void =
        unsafe { vtable_fn(vtable, 57) };
    let get_service_data: unsafe extern "thiscall" fn(
        *mut NetSysPrefix,
        *mut c_void,
    ) -> *mut c_void = unsafe { vtable_fn(vtable, 58) };

    let url = checked_call(
        "NetSys[10] get_url_string",
        &mut stack_pointer_checks,
        || unsafe { get_url_string(object) },
    )?;
    if url.is_null() || unsafe { (*url).bytes[11] } != 1 {
        return Err("NetSys[10] get_url_string returned a non-canonical empty String".into());
    }
    let poll_services_result = checked_call(
        "NetSys[24] poll_services",
        &mut stack_pointer_checks,
        || unsafe { poll_services(object, core::ptr::null_mut()) },
    )?;
    let disconnect_result = checked_call(
        "NetSys[31] disconnect",
        &mut stack_pointer_checks,
        || unsafe { disconnect(object, false) },
    )?;
    let poll_players_result = checked_call(
        "NetSys[35] poll_players",
        &mut stack_pointer_checks,
        || unsafe {
            poll_players(
                object,
                core::ptr::null(),
                MsvcArrayNetPlayers { bytes: [0; 28] },
            )
        },
    )?;
    let found = checked_call(
        "NetSys[36] find_player_from_id",
        &mut stack_pointer_checks,
        || unsafe {
            find_player_from_id(
                object,
                MsvcWstring {
                    sso: [0; 8],
                    len: 0,
                    capacity: 7,
                },
            )
        },
    )?;
    checked_call(
        "NetSys[46] log_connection(String,int)",
        &mut stack_pointer_checks,
        || unsafe { log_connection2(object, core::ptr::null(), 0) },
    )?;
    let group = checked_call(
        "NetSys[57] get_group_data",
        &mut stack_pointer_checks,
        || unsafe { get_group_data(object, core::ptr::null_mut()) },
    )?;
    let service = checked_call(
        "NetSys[58] get_service_data",
        &mut stack_pointer_checks,
        || unsafe { get_service_data(object, core::ptr::null_mut()) },
    )?;
    if poll_services_result != LIBERR_NOT_AVAILABLE
        || disconnect_result != LIBERR_NOT_AVAILABLE
        || poll_players_result != LIBERR_NOT_AVAILABLE
    {
        return Err(format!(
            "load-only corrected-slot gates failed: poll_services={poll_services_result} disconnect={disconnect_result} poll_players={poll_players_result}"
        ));
    }
    if !found.is_null() || !group.is_null() || !service.is_null() {
        return Err("load-only corrected pointer-return slot returned non-null".into());
    }

    let is_connected: unsafe extern "C" fn() -> bool =
        unsafe { std::mem::transmute(resolve(&module, IS_CONNECTED)?) };
    let set_connected: unsafe extern "C" fn(bool) =
        unsafe { std::mem::transmute(resolve(&module, SET_CONNECTED)?) };
    let connected_before = checked_call(
        "export is_connected_to_network before setter",
        &mut stack_pointer_checks,
        || unsafe { is_connected() },
    )?;
    checked_call(
        "export set_network_connection_state",
        &mut stack_pointer_checks,
        || unsafe { set_connected(false) },
    )?;
    let connected_after = checked_call(
        "export is_connected_to_network after setter",
        &mut stack_pointer_checks,
        || unsafe { is_connected() },
    )?;
    env::set_var("DON_NET_CONNECTIVITY_OVERRIDE", "0");
    let connected_forced_offline = checked_call(
        "export is_connected_to_network forced offline",
        &mut stack_pointer_checks,
        || unsafe { is_connected() },
    )?;
    if !connected_before || !connected_after || connected_forced_offline {
        return Err(format!(
            "connectivity export remained coupled to session/setter state: before={connected_before} after={connected_after} forced_offline={connected_forced_offline}"
        ));
    }

    let send_ready: unsafe extern "thiscall" fn(*mut NetSysPrefix, bool) =
        unsafe { std::mem::transmute(resolve(&module, SEND_READY)?) };
    let reset_ready: unsafe extern "thiscall" fn(*mut NetSysPrefix) =
        unsafe { std::mem::transmute(resolve(&module, RESET_READY)?) };
    let is_host: unsafe extern "thiscall" fn(*mut NetSysPrefix, *const c_void) -> bool =
        unsafe { std::mem::transmute(resolve(&module, IS_HOST)?) };
    checked_call(
        "export send_ready_flag",
        &mut stack_pointer_checks,
        || unsafe { send_ready(object, true) },
    )?;
    checked_call(
        "export reset_ready_flags",
        &mut stack_pointer_checks,
        || unsafe { reset_ready(object) },
    )?;
    let host_role = checked_call("export IsHost", &mut stack_pointer_checks, || unsafe {
        is_host(object, core::ptr::null())
    })?;
    if !host_role {
        return Err("load-only host object did not report its role".into());
    }

    // Friend Game calls ns_host first and invokes the direct OnPlayerJoined
    // export only afterwards. The shim-only load-only gate reproduces that
    // exact split without traffic: pointers are coherent at host return, but
    // the pending local callback must wait for the authenticated retail id.
    let materialize: unsafe extern "C" fn(*mut NetSysPrefix) -> bool =
        unsafe { std::mem::transmute(resolve(&module, SHIM_MATERIALIZE)?) };
    let materialized = checked_call(
        "shim_materialize_load_only_peer",
        &mut stack_pointer_checks,
        || unsafe { materialize(object) },
    )?;
    if !materialized || messenger.added != 0 {
        return Err(format!(
            "pre-OnPlayerJoined materialisation/callback split failed: returned={materialized} callbacks={} added={}",
            messenger.callbacks, messenger.added
        ));
    }
    if unsafe {
        u16::from_le_bytes([
            (*object).reserved_to_crossplay[0x50],
            (*object).reserved_to_crossplay[0x51],
        ])
    } != messenger.session_data
    {
        return Err("host lifecycle did not retain NetMessenger session data at +0xB0".into());
    }
    let player = unsafe { (*object).players[0] };
    if player.is_null()
        || unsafe { (*object).local_player != player }
        || unsafe { (*object).host_player != player }
        || unsafe { (*player).flags & 2 == 0 }
    {
        return Err(
            "pre-OnPlayerJoined host did not expose one coherent pending local player".into(),
        );
    }

    let retail_id = MsvcWstring {
        sso: [
            b'r' as u16,
            b'e' as u16,
            b't' as u16,
            b'a' as u16,
            b'i' as u16,
            b'l' as u16,
            b'1' as u16,
            0,
        ],
        len: 7,
        capacity: 7,
    };
    let on_player_joined: unsafe extern "thiscall" fn(
        *mut NetSysPrefix,
        *const MsvcWstring,
        *const MsvcWstring,
    ) = unsafe { std::mem::transmute(resolve(&module, ON_PLAYER_JOINED)?) };
    checked_call(
        "export OnPlayerJoined(retail local identity)",
        &mut stack_pointer_checks,
        || unsafe { on_player_joined(object, &retail_id, &retail_id) },
    )?;
    if messenger.added != 1
        || messenger.flags_seen_on_add & 2 == 0
        || !messenger.crossplay_non_null_on_add
        || messenger.id_seen_on_add.len != 7
        || messenger.id_seen_on_add.sso[..7] != retail_id.sso[..7]
        || unsafe { (*player).flags & 2 != 0 }
    {
        return Err(format!(
            "OnPlayerJoined did not callback with final id while pending then clear: added={} flags_seen=0x{:x} pending_after={} id_len={}",
            messenger.added,
            messenger.flags_seen_on_add,
            unsafe { (*player).flags & 2 != 0 },
            messenger.id_seen_on_add.len
        ));
    }
    checked_call(
        "export OnPlayerJoined(idempotent repeat)",
        &mut stack_pointer_checks,
        || unsafe { on_player_joined(object, &retail_id, &retail_id) },
    )?;
    if messenger.added != 1 {
        return Err("repeated OnPlayerJoined emitted a duplicate player-added callback".into());
    }

    // Then call every non-destructor NetPlayer slot. This protects the hidden
    // String/wstring return buffers and each callee-cleanup width.
    let get_num_players: unsafe extern "thiscall" fn(*mut NetSysPrefix) -> i32 =
        unsafe { vtable_fn(vtable, 55) };
    let num_players = checked_call(
        "NetSys[55] get_num_players",
        &mut stack_pointer_checks,
        || unsafe { get_num_players(object) },
    )?;
    if num_players != 1 || unsafe { (*object).num_players } != 1 {
        return Err(format!(
            "local materialisation expected one player, got return={num_players} prefix={}",
            unsafe { (*object).num_players }
        ));
    }
    if player.is_null()
        || unsafe { (*object).local_player != player }
        || unsafe { (*object).host_player != player }
    {
        return Err("local materialisation did not atomically populate player/local/host".into());
    }
    let player_vtable = unsafe { (*player).vftable };
    if player_vtable.is_null() {
        return Err("materialised NetPlayer has a null vtable".into());
    }
    for slot in 0..21 {
        if unsafe { (*player_vtable.add(slot)).is_null() } {
            return Err(format!("NetPlayer vtable slot {slot} is null"));
        }
    }

    let player_predicate = |slot| unsafe {
        vtable_fn::<unsafe extern "thiscall" fn(*mut NetPlayerPrefix) -> i32>(player_vtable, slot)
    };
    let is_local_player = checked_call(
        "NetPlayer[1] is_local",
        &mut stack_pointer_checks,
        || unsafe { player_predicate(1)(player) },
    )?;
    let is_host_player = checked_call(
        "NetPlayer[2] is_host",
        &mut stack_pointer_checks,
        || unsafe { player_predicate(2)(player) },
    )?;
    let is_pending = checked_call(
        "NetPlayer[3] is_pending",
        &mut stack_pointer_checks,
        || unsafe { player_predicate(3)(player) },
    )?;
    let is_observer = checked_call(
        "NetPlayer[4] is_observer",
        &mut stack_pointer_checks,
        || unsafe { player_predicate(4)(player) },
    )?;
    if (is_local_player, is_host_player, is_pending, is_observer) != (1, 1, 0, 0) {
        return Err(format!(
            "unexpected local NetPlayer flags: local={is_local_player} host={is_host_player} pending={is_pending} observer={is_observer}"
        ));
    }

    type GameStringGet = unsafe extern "thiscall" fn(
        *mut NetPlayerPrefix,
        *mut MsvcGameString,
    ) -> *mut MsvcGameString;
    type GameStringSet = unsafe extern "thiscall" fn(*mut NetPlayerPrefix, *const MsvcGameString);
    let mut name = MsvcGameString { bytes: [0xff; 20] };
    let get_name: GameStringGet = unsafe { vtable_fn(player_vtable, 5) };
    let name_result = checked_call(
        "NetPlayer[5] get_internal_name",
        &mut stack_pointer_checks,
        || unsafe { get_name(player, &mut name) },
    )?;
    let name_pointer = u32::from_le_bytes(name.bytes[..4].try_into().unwrap());
    let name_len = u16::from_le_bytes(name.bytes[4..6].try_into().unwrap());
    if name_result != &mut name
        || name_pointer == 0
        || name_len != 2
        || name.bytes[10] != 1
        || name.bytes[11] != 1
    {
        return Err("NetPlayer[5] returned an invalid hidden String buffer".into());
    }
    let set_name: GameStringSet = unsafe { vtable_fn(player_vtable, 6) };
    checked_call(
        "NetPlayer[6] set_name",
        &mut stack_pointer_checks,
        || unsafe { set_name(player, &name) },
    )?;
    let mut description = MsvcGameString { bytes: [0xff; 20] };
    let get_description: GameStringGet = unsafe { vtable_fn(player_vtable, 7) };
    let description_result = checked_call(
        "NetPlayer[7] get_internal_description",
        &mut stack_pointer_checks,
        || unsafe { get_description(player, &mut description) },
    )?;
    let mut canonical_empty = [0u8; 20];
    canonical_empty[11] = 1;
    if description_result != &mut description || description.bytes != canonical_empty {
        return Err("NetPlayer[7] returned an invalid hidden String buffer".into());
    }
    let set_description: GameStringSet = unsafe { vtable_fn(player_vtable, 8) };
    checked_call(
        "NetPlayer[8] set_description",
        &mut stack_pointer_checks,
        || unsafe { set_description(player, &description) },
    )?;

    type WstringGet =
        unsafe extern "thiscall" fn(*mut NetPlayerPrefix, *mut MsvcWstring) -> *mut MsvcWstring;
    let mut returned_wstrings = Vec::new();
    for (slot, label) in [(9, "get_id"), (10, "get_platform_id"), (11, "get_platform")] {
        let getter: WstringGet = unsafe { vtable_fn(player_vtable, slot) };
        let mut value = MsvcWstring {
            sso: [0xffff; 8],
            len: u32::MAX,
            capacity: u32::MAX,
        };
        let returned = checked_call(
            &format!("NetPlayer[{slot}] {label}"),
            &mut stack_pointer_checks,
            || unsafe { getter(player, &mut value) },
        )?;
        if returned != &mut value || value.capacity != 7 || value.len > 7 {
            return Err(format!(
                "NetPlayer[{slot}] returned an invalid hidden wstring buffer"
            ));
        }
        returned_wstrings.push(value);
    }
    let platform = &returned_wstrings[2];
    if platform.len != 3 || platform.sso[..3] != [b'd' as u16, b'o' as u16, b'n' as u16] {
        return Err("NetPlayer[11] platform was not the expected SSO string 'don'".into());
    }
    let found_local = checked_call(
        "NetSys[36] find_player_from_id(local)",
        &mut stack_pointer_checks,
        || unsafe { find_player_from_id(object, returned_wstrings[0]) },
    )?;
    if found_local != player {
        return Err("NetSys[36] did not resolve the stable local NetPlayer id".into());
    }

    let get_i32 = |slot| unsafe {
        vtable_fn::<unsafe extern "thiscall" fn(*mut NetPlayerPrefix) -> i32>(player_vtable, slot)
    };
    let get_u32 = |slot| unsafe {
        vtable_fn::<unsafe extern "thiscall" fn(*mut NetPlayerPrefix) -> u32>(player_vtable, slot)
    };
    let player_index = checked_call(
        "NetPlayer[12] get_player_index",
        &mut stack_pointer_checks,
        || unsafe { get_i32(12)(player) },
    )?;
    let game_version = checked_call(
        "NetPlayer[13] get_game_version",
        &mut stack_pointer_checks,
        || unsafe { get_u32(13)(player) },
    )?;
    let ping = checked_call(
        "NetPlayer[14] get_ping_time",
        &mut stack_pointer_checks,
        || unsafe { get_u32(14)(player) },
    )?;
    let pulse = checked_call(
        "NetPlayer[15] get_time_since_last_pulse",
        &mut stack_pointer_checks,
        || unsafe { get_u32(15)(player) },
    )?;
    if (player_index, game_version, ping, pulse) != (0, 0x1234, 0, 0) {
        return Err(format!(
            "unexpected local NetPlayer values: index={player_index} version={game_version} ping={ping} pulse={pulse}"
        ));
    }

    let queue_info: unsafe extern "thiscall" fn(*mut NetPlayerPrefix, *mut u32, *mut u32) =
        unsafe { vtable_fn(player_vtable, 16) };
    let mut queued_bytes = u32::MAX;
    let mut queued_packets = u32::MAX;
    checked_call(
        "NetPlayer[16] get_send_queue_info",
        &mut stack_pointer_checks,
        || unsafe { queue_info(player, &mut queued_bytes, &mut queued_packets) },
    )?;
    if queued_bytes != 0 || queued_packets != 0 {
        return Err("NetPlayer[16] did not initialise both queue outputs".into());
    }
    type PlayerVoid = unsafe extern "thiscall" fn(*mut NetPlayerPrefix);
    let reset_sync: PlayerVoid = unsafe { vtable_fn(player_vtable, 17) };
    let inc_sync: PlayerVoid = unsafe { vtable_fn(player_vtable, 18) };
    let get_sync: unsafe extern "thiscall" fn(*mut NetPlayerPrefix) -> i32 =
        unsafe { vtable_fn(player_vtable, 19) };
    let log_state: PlayerVoid = unsafe { vtable_fn(player_vtable, 20) };
    checked_call(
        "NetPlayer[17] reset_sync_counter",
        &mut stack_pointer_checks,
        || unsafe { reset_sync(player) },
    )?;
    checked_call(
        "NetPlayer[18] inc_sync_counter",
        &mut stack_pointer_checks,
        || unsafe { inc_sync(player) },
    )?;
    let sync = checked_call(
        "NetPlayer[19] get_sync_counter",
        &mut stack_pointer_checks,
        || unsafe { get_sync(player) },
    )?;
    if sync != 1 {
        return Err(format!("NetPlayer sync counter expected 1, got {sync}"));
    }
    checked_call(
        "NetPlayer[20] log_state",
        &mut stack_pointer_checks,
        || unsafe { log_state(player) },
    )?;

    let trace_text =
        fs::read_to_string(&trace).map_err(|e| format!("read trace {}: {e}", trace.display()))?;
    for required in [
        "factory=ready abi=netsys-v65 role=Host load_only=true local_addr=127.0.0.1:",
        "call=vtable.ns_error_set_callback",
        "call=vtable.ns_set_profiler",
        "call=vtable.ns_init",
        "init=stored messenger=true crossplay_service=true object_size=0x3d4",
        "call=vtable.ns_host",
        "call=vtable.ns_join",
        "call=vtable.ns_send_all",
        "call=vtable.ns_get",
        "call=vtable.ns_get_url_string",
        "call=vtable.ns_poll_services",
        "call=vtable.ns_disconnect",
        "call=vtable.ns_poll_players",
        "call=vtable.ns_find_player_from_id",
        "call=vtable.ns_log_connection2",
        "call=vtable.ns_get_group_data",
        "call=vtable.ns_get_service_data",
        "call=vtable.ns_get_num_players",
        "callback=NetMessenger.on_player_added player_non_null=true",
        "call=export.send_ready_flag",
        "call=export.reset_ready_flags",
        "call=netplayer.get_internal_name",
        "call=netplayer.set_name",
        "call=netplayer.get_internal_description",
        "call=netplayer.set_description",
        "call=netplayer.get_id",
        "call=netplayer.get_platform_id",
        "call=netplayer.get_platform",
        "call=netplayer.get_send_queue_info",
        "call=netplayer.get_sync_counter",
        "call=netplayer.log_state",
    ] {
        if !trace_text.contains(required) {
            return Err(format!("trace is missing required evidence: {required}"));
        }
    }
    if trace_text.contains("local_addr=0.0.0.0:") {
        return Err("load-only trace exposed a non-loopback listener".into());
    }

    drop(module);
    Ok(format!(
        "{{\"schema\":\"don.netsys-load-smoke.v3\",\"status\":\"pass\",\"pe\":\"PE32-i386\",\"shipped_exports_resolved\":11,\"factory_non_null\":true,\"vtable_slots_non_null\":65,\"retail_loader_slots_called\":[1,56,63],\"retail_post_init_service_slot\":47,\"retained_offsets\":[92,204],\"netsys_corrected_slots_called\":[10,24,31,35,36,46,57,58],\"netplayer_slots_called\":20,\"netmessenger_add_order\":\"pending-then-clear\",\"stack_pointer_checks\":{stack_pointer_checks},\"connectivity\":{{\"production_source\":\"InternetGetConnectedState\",\"load_only_override_online_before_noop_setter\":{connected_before},\"load_only_override_online_after_noop_setter\":{connected_after},\"load_only_override_offline\":{connected_forced_offline}}},\"load_only\":{{\"host_result\":26,\"join_result\":26,\"send\":false,\"get\":false,\"listener\":\"127.0.0.1:ephemeral\"}},\"peer_name\":\"Ai\",\"credential_material\":\"none\",\"retail_process_modified\":false}}"
    ))
}

fn default_dll() -> Result<PathBuf, String> {
    let exe = env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    let parent = exe
        .parent()
        .ok_or_else(|| "smoke executable has no parent directory".to_string())?;
    Ok(parent.join("CrossplayNetLib.dll"))
}

fn default_trace() -> PathBuf {
    env::temp_dir().join(format!("don-netsys-load-smoke-{}.log", std::process::id()))
}

fn wide_path(path: &Path) -> Vec<u16> {
    OsStr::new(path).encode_wide().chain(Some(0)).collect()
}

fn resolve(module: &Module, name: &[u8]) -> Result<*mut c_void, String> {
    let address = unsafe { GetProcAddress(module.0, name.as_ptr()) };
    if address.is_null() {
        let printable = String::from_utf8_lossy(&name[..name.len().saturating_sub(1)]);
        return Err(format!(
            "GetProcAddress failed for {printable}: win32={}",
            unsafe { GetLastError() }
        ));
    }
    Ok(address)
}

unsafe fn vtable_fn<F>(vtable: *const *const c_void, slot: usize) -> F
where
    F: Copy,
{
    assert!(std::mem::size_of::<F>() == std::mem::size_of::<*const c_void>());
    std::mem::transmute_copy(&*vtable.add(slot))
}

#[inline(always)]
fn read_esp() -> usize {
    let value: usize;
    unsafe {
        std::arch::asm!(
            "mov {0:e}, esp",
            out(reg) value,
            options(nomem, nostack, preserves_flags)
        );
    }
    value
}

fn checked_call<T>(label: &str, checks: &mut usize, call: impl FnOnce() -> T) -> Result<T, String> {
    let before = read_esp();
    let result = call();
    let after = read_esp();
    if before != after {
        return Err(format!(
            "stack cleanup mismatch after {label}: before=0x{before:08x} after=0x{after:08x}"
        ));
    }
    *checks += 1;
    Ok(result)
}

fn fail(message: &str) -> ! {
    let escaped = message.replace('\\', "\\\\").replace('"', "\\\"");
    eprintln!(
        "{{\"schema\":\"don.netsys-load-smoke.v1\",\"status\":\"fail\",\"error\":\"{}\",\"credential_material\":\"none\",\"retail_process_modified\":false}}",
        escaped
    );
    std::process::exit(1)
}
