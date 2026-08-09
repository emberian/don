//! The `NetSys` implementation: 65 vtable thunks over `don_net::Session`.
//!
//! Only about twenty of the sixty-five slots need real behaviour — the ones the
//! game calls every frame (`check_pulse`, `process_system_messages`, `get`,
//! `send`, `send_all`, `is_host`, `get_num_players`) plus the session lifecycle
//! (`init`, `host`, `join`, `close`, `disconnect`). The rest exist so the table
//! is the right length and nothing dispatches into empty memory; each returns a
//! benign value.

use crate::abi::*;
use core::ffi::c_void;
use don_net::session::{Role, Session};
use don_net::transport::{Dest, TcpTransport, Transport};
use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Largest packet we will copy into the game's receive buffer.
///
/// The biggest legitimate message is `NetMsg_SyncDirInfo` at 529 bytes; a
/// command package is at most `8 + CommandPackage::data[512]` = 520. The
/// game's buffer is a member of `NetDaemon` whose exact extent we have not
/// measured, so this cap is deliberately just above the largest real message.
/// Anything bigger is dropped rather than memcpy'd into a buffer we cannot
/// prove is large enough — a remote peer must not be able to smash the game's
/// heap through us.
const MAX_DELIVER: usize = 1024;

struct State {
    session: Session<TcpTransport>,
    /// Game-layer packets waiting for `NetSys::get`.
    inbox: VecDeque<(i32, Vec<u8>)>,
    players: Vec<Box<NetPlayerObj>>,
    started: Instant,
    time_out_ms: u32,
    allow_timeout: i32,
    playing: i32,
    joining: i32,
    accept_host_messages: i32,
    num_allowed_players: i32,
    host_port: u32,
    local_port: u32,
    matchmaking_id: i32,
    error_callback: Option<unsafe extern "C" fn(i32)>,
}

#[repr(C)]
struct NetSysObj {
    base: NetSysBase,
    /// Boxed so the C-visible prefix stays exactly `NetSysBase`.
    state: *mut State,
}

static mut CONNECTED: bool = true;

pub fn connected() -> bool {
    unsafe { CONNECTED }
}

pub fn set_connected(v: bool) {
    unsafe { CONNECTED = v }
}

fn env(k: &str) -> Option<String> {
    std::env::var(k).ok()
}

fn build_transport(id: i32) -> std::io::Result<(TcpTransport, Role)> {
    match env("DON_NET_ROLE").as_deref() {
        Some("join") => {
            let addr = env("DON_NET_ADDR").unwrap_or_else(|| "127.0.0.1:31337".into());
            Ok((TcpTransport::join(id, addr.as_str())?, Role::Client))
        }
        _ => {
            let bind = env("DON_NET_BIND").unwrap_or_else(|| "0.0.0.0:31337".into());
            Ok((TcpTransport::host(id, bind.as_str())?, Role::Host))
        }
    }
}

pub fn create() -> *mut NetSysBase {
    let id = env("DON_NET_ID")
        .and_then(|v| v.parse::<i32>().ok())
        .unwrap_or_else(|| std::process::id() as i32);
    let name = env("DON_NET_NAME").unwrap_or_else(|| "donnet".into());

    let Ok((transport, role)) = build_transport(id) else {
        return core::ptr::null_mut();
    };
    let state = Box::new(State {
        session: Session::new(transport, role, name),
        inbox: VecDeque::new(),
        players: Vec::new(),
        started: Instant::now(),
        time_out_ms: 30_000,
        allow_timeout: 1,
        playing: 0,
        joining: 0,
        accept_host_messages: 1,
        num_allowed_players: 8,
        host_port: 31337,
        local_port: 31337,
        matchmaking_id: 0,
        error_callback: None,
    });
    let obj = Box::new(NetSysObj {
        base: NetSysBase {
            vftable: &NETSYS_VTABLE,
            num_players: 1,
            players: [core::ptr::null_mut(); 8],
            local_player: core::ptr::null_mut(),
            host_player: core::ptr::null_mut(),
            local_player_connection: LOCAL_PLAYER_CONNECTED,
            local_player_disconnect_pct: 0.0,
            net_sessions: [0u8; 28],
            log: core::ptr::null_mut(),
        },
        state: Box::into_raw(state),
    });
    // Deliberately leaked: the game never frees the object (there is no
    // `delete_netsys_object_ptr` export and the loader keeps the pointer in the
    // `netsys` global at 0x00E335C8 for the process lifetime).
    &mut Box::leak(obj).base
}

/// Run `f` against the session behind a `NetSys*`, with a panic guard — a Rust
/// panic unwinding into MSVC C++ frames is undefined, and the crate builds with
/// `panic = "abort"` so this is belt and braces for the null case only.
pub unsafe fn with<R>(this: *mut NetSysBase, f: impl FnOnce(&mut Session<TcpTransport>) -> R) -> Option<R> {
    let obj = this as *mut NetSysObj;
    if obj.is_null() || (*obj).state.is_null() {
        return None;
    }
    Some(f(&mut (*(*obj).state).session))
}

unsafe fn st<'a>(this: *mut NetSysBase) -> Option<&'a mut State> {
    let obj = this as *mut NetSysObj;
    if obj.is_null() || (*obj).state.is_null() {
        return None;
    }
    Some(&mut *(*obj).state)
}

unsafe fn now_ms(s: &State) -> u64 {
    s.started.elapsed().as_millis() as u64
}

/// Refresh the `NetPlayer*` array the game can read directly, and drain new
/// game-layer packets into our own inbox.
unsafe fn pump(this: *mut NetSysBase) {
    let Some(s) = st(this) else { return };
    let t = now_ms(s);
    let _ = s.session.poll(t, Duration::from_millis(0));
    for e in s.session.drain_events() {
        if let don_net::session::Event::Game { from, msg } = e {
            if msg.bytes.len() <= MAX_DELIVER {
                s.inbox.push_back((from, msg.bytes));
            }
        }
    }
    // Rebuild the player objects. Cheap: at most eight, and only on change.
    let roster: Vec<(i32, bool, bool, String)> = s
        .session
        .players()
        .iter()
        .map(|p| (p.unique_id, p.is_host, p.is_local, p.name.clone()))
        .collect();
    if roster.len() != s.players.len()
        || roster.iter().zip(s.players.iter()).any(|(r, p)| r.0 != p.unique_id)
    {
        s.players.clear();
        for (i, (uid, is_host, is_local, name)) in roster.iter().enumerate() {
            let mut po = Box::new(NetPlayerObj {
                vftable: &NETPLAYER_VTABLE,
                unique_id: *uid,
                flags: if *is_host { SNLPLAYER_HOST } else { 0 }
                    | if *is_local { SNLPLAYER_LOCAL } else { 0 },
                sync_counter: 0,
                dsync_frame: 0,
                last_pulse_ms: 0,
                ping_ms: 0,
                player_index: i as i32,
                game_version: 0,
                id_utf16: [0u16; 40],
                name_utf8: [0u8; 64],
            });
            for (j, u) in uid.to_string().encode_utf16().take(39).enumerate() {
                po.id_utf16[j] = u;
            }
            let nb = name.as_bytes();
            let n = nb.len().min(63);
            po.name_utf8[..n].copy_from_slice(&nb[..n]);
            s.players.push(po);
        }
    }
    let obj = this as *mut NetSysObj;
    (*obj).base.num_players = s.players.len() as i32;
    (*obj).base.players = [core::ptr::null_mut(); 8];
    for (i, p) in s.players.iter_mut().enumerate().take(8) {
        (*obj).base.players[i] = p.as_mut() as *mut NetPlayerObj;
    }
    (*obj).base.local_player = s
        .players
        .iter_mut()
        .find(|p| p.flags & SNLPLAYER_LOCAL != 0)
        .map(|p| p.as_mut() as *mut NetPlayerObj)
        .unwrap_or(core::ptr::null_mut());
    (*obj).base.host_player = s
        .players
        .iter_mut()
        .find(|p| p.flags & SNLPLAYER_HOST != 0)
        .map(|p| p.as_mut() as *mut NetPlayerObj)
        .unwrap_or(core::ptr::null_mut());
}

// ---------------------------------------------------------------------------
// NetSys vtable thunks
// ---------------------------------------------------------------------------

macro_rules! nop {
    ($name:ident ( $($a:ty),* )) => {
        unsafe extern "thiscall" fn $name(_this: *mut NetSysBase $(, _: $a)*) {}
    };
    ($name:ident ( $($a:ty),* ) -> $r:ty = $v:expr) => {
        unsafe extern "thiscall" fn $name(_this: *mut NetSysBase $(, _: $a)*) -> $r { $v }
    };
}

unsafe extern "thiscall" fn ns_dtor(_this: *mut NetSysBase, _flags: u32) -> *mut c_void {
    // The game never deletes us; see `create`.
    core::ptr::null_mut()
}

unsafe extern "thiscall" fn ns_init(
    _this: *mut NetSysBase,
    _messenger: *mut c_void,
    _guid: *const c_void,
    _a: i32,
    _b: i32,
    _c: u32,
    _d: u32,
) -> i32 {
    LIBERR_OK
}

unsafe extern "thiscall" fn ns_close(_this: *mut NetSysBase) {}

nop!(ns_get_memory_manager() -> *mut c_void = core::ptr::null_mut());

unsafe extern "thiscall" fn ns_is_host(this: *mut NetSysBase) -> bool {
    st(this).map(|s| s.session.role == Role::Host).unwrap_or(false)
}

nop!(ns_enable_join(i32));

unsafe extern "thiscall" fn ns_set_playing(this: *mut NetSysBase, v: i32) {
    if let Some(s) = st(this) {
        s.playing = v;
    }
}

unsafe extern "thiscall" fn ns_is_playing(this: *mut NetSysBase) -> i32 {
    st(this).map(|s| s.playing).unwrap_or(0)
}

unsafe extern "thiscall" fn ns_is_joining_in_process(this: *mut NetSysBase) -> i32 {
    st(this).map(|s| s.joining).unwrap_or(0)
}

unsafe extern "thiscall" fn ns_is_session_full(this: *mut NetSysBase) -> i32 {
    st(this)
        .map(|s| i32::from(s.session.players().len() as i32 >= s.num_allowed_players))
        .unwrap_or(0)
}

nop!(ns_get_url_string(*mut c_void) -> *mut c_void = core::ptr::null_mut());

unsafe extern "thiscall" fn ns_accept_host_messages(this: *mut NetSysBase, v: i32) {
    if let Some(s) = st(this) {
        s.accept_host_messages = v;
    }
}

unsafe extern "thiscall" fn ns_set_number_players(this: *mut NetSysBase, v: i32) {
    if let Some(s) = st(this) {
        s.num_allowed_players = v;
    }
}

nop!(ns_set_number_observers(i32));

unsafe extern "thiscall" fn ns_send_dsync(this: *mut NetSysBase, frame: i32) {
    let _ = with(this, |s| s.send_dsync(frame));
}

unsafe extern "thiscall" fn ns_check_pulse(this: *mut NetSysBase) {
    pump(this);
}

unsafe extern "thiscall" fn ns_set_time_out(this: *mut NetSysBase, v: u32) {
    if let Some(s) = st(this) {
        s.time_out_ms = v;
        s.session.set_timeout_ms(v as u64);
    }
}

unsafe extern "thiscall" fn ns_get_time_out(this: *mut NetSysBase) -> u32 {
    st(this).map(|s| s.time_out_ms).unwrap_or(0)
}

unsafe extern "thiscall" fn ns_set_allow_timeout(this: *mut NetSysBase, v: i32) {
    if let Some(s) = st(this) {
        s.allow_timeout = v;
    }
}

unsafe extern "thiscall" fn ns_get_allow_timeout(this: *mut NetSysBase) -> i32 {
    st(this).map(|s| s.allow_timeout).unwrap_or(1)
}

unsafe extern "thiscall" fn ns_get_num_allowed_players(this: *mut NetSysBase) -> i32 {
    st(this).map(|s| s.num_allowed_players).unwrap_or(8)
}

unsafe extern "thiscall" fn ns_send(
    this: *mut NetSysBase,
    packet: *const u8,
    size: i32,
    to: *const NetPlayerObj,
    _flags: i32,
) -> bool {
    if packet.is_null() || size <= 0 || size as usize > MAX_DELIVER {
        return false;
    }
    let bytes = core::slice::from_raw_parts(packet, size as usize);
    let dest = if to.is_null() { Dest::All } else { Dest::One((*to).unique_id) };
    st(this)
        .map(|s| s.session.transport.send(dest, bytes).is_ok())
        .unwrap_or(false)
}

unsafe extern "thiscall" fn ns_send_all(
    this: *mut NetSysBase,
    packet: *const u8,
    size: i32,
    _flags: i32,
) -> bool {
    if packet.is_null() || size <= 0 || size as usize > MAX_DELIVER {
        return false;
    }
    let bytes = core::slice::from_raw_parts(packet, size as usize);
    st(this)
        .map(|s| s.session.transport.send(Dest::All, bytes).is_ok())
        .unwrap_or(false)
}

unsafe extern "thiscall" fn ns_get(
    this: *mut NetSysBase,
    packet: *mut u8,
    from: *mut *const NetPlayerObj,
    size: *mut u32,
) -> bool {
    pump(this);
    let Some(s) = st(this) else { return false };
    let Some((sender, bytes)) = s.inbox.pop_front() else { return false };
    if packet.is_null() || bytes.len() > MAX_DELIVER {
        return false;
    }
    core::ptr::copy_nonoverlapping(bytes.as_ptr(), packet, bytes.len());
    if !size.is_null() {
        *size = bytes.len() as u32;
    }
    if !from.is_null() {
        *from = s
            .players
            .iter_mut()
            .find(|p| p.unique_id == sender)
            .map(|p| p.as_mut() as *const NetPlayerObj)
            .unwrap_or(core::ptr::null());
    }
    true
}

nop!(ns_poll_services(*mut c_void));

unsafe extern "thiscall" fn ns_host(
    _this: *mut NetSysBase,
    _a: *const c_void,
    _b: *const c_void,
    _c: *const c_void,
    _d: *const c_void,
    _e: *const c_void,
) -> i32 {
    LIBERR_OK
}

unsafe extern "thiscall" fn ns_join(
    this: *mut NetSysBase,
    _s: *const c_void,
    _a: *const c_void,
    _b: *const c_void,
    _c: *const c_void,
    _d: i32,
) -> i32 {
    if let Some(s) = st(this) {
        s.joining = 1;
    }
    LIBERR_OK
}

unsafe extern "thiscall" fn ns_join_ip(
    this: *mut NetSysBase,
    _a: *const c_void,
    _b: *const c_void,
    _c: *const c_void,
    _d: *const c_void,
    _e: i32,
    _f: i32,
) -> i32 {
    if let Some(s) = st(this) {
        s.joining = 1;
    }
    LIBERR_OK
}

unsafe extern "thiscall" fn ns_cancel_joining(this: *mut NetSysBase) {
    if let Some(s) = st(this) {
        s.joining = 0;
    }
}

nop!(ns_cancel_join());
nop!(ns_cancel_join_skybox());
nop!(ns_disconnect(bool));
nop!(ns_poll_sessions(*const c_void) -> i32 = LIBERR_OK);
nop!(ns_stop_poll_sessions());
nop!(ns_clear_net_sessions(u32));
nop!(ns_poll_players(*const c_void, *mut c_void) -> i32 = 0);

unsafe extern "thiscall" fn ns_find_player_from_id(
    _this: *mut NetSysBase,
    _id: *const c_void,
) -> *const NetPlayerObj {
    core::ptr::null()
}

unsafe extern "thiscall" fn ns_validate_player(
    this: *mut NetSysBase,
    p: *const NetPlayerObj,
) -> i32 {
    if p.is_null() {
        return 0;
    }
    st(this)
        .map(|s| i32::from(s.players.iter().any(|q| q.unique_id == (*p).unique_id)))
        .unwrap_or(0)
}

nop!(ns_update_recently_played_with_list());

unsafe extern "thiscall" fn ns_process_system_messages(this: *mut NetSysBase) {
    pump(this);
}

nop!(ns_send_drop_player(*const NetPlayerObj));
nop!(ns_drop_player(*const NetPlayerObj));
nop!(ns_cancel_drop_player(*const NetPlayerObj));
nop!(ns_set_log(*mut c_void));
nop!(ns_log_set_frame(i32));
nop!(ns_log_connection(*const c_void));
nop!(ns_log_connection2(*const c_void));

/// `__cdecl`, variadic — see the note on the vtable field. Ignoring the
/// variadic tail is safe precisely because the caller cleans the stack.
unsafe extern "C" fn ns_log_connection_fmt(_this: *mut NetSysBase, _fmt: *const u16) {}

nop!(ns_get_ip_addresses() -> *mut c_void = core::ptr::null_mut());

unsafe extern "thiscall" fn ns_get_host_port(this: *mut NetSysBase) -> u32 {
    st(this).map(|s| s.host_port).unwrap_or(0)
}
unsafe extern "thiscall" fn ns_set_host_port(this: *mut NetSysBase, v: u32) {
    if let Some(s) = st(this) {
        s.host_port = v;
    }
}
unsafe extern "thiscall" fn ns_get_local_port(this: *mut NetSysBase) -> u32 {
    st(this).map(|s| s.local_port).unwrap_or(0)
}
unsafe extern "thiscall" fn ns_set_local_port(this: *mut NetSysBase, v: u32) {
    if let Some(s) = st(this) {
        s.local_port = v;
    }
}

nop!(ns_set_ip_override(*const c_void));

unsafe extern "thiscall" fn ns_set_matchmaking_id(this: *mut NetSysBase, v: i32) {
    if let Some(s) = st(this) {
        s.matchmaking_id = v;
    }
}

unsafe extern "thiscall" fn ns_get_num_players(this: *mut NetSysBase) -> i32 {
    pump(this);
    st(this).map(|s| s.players.len() as i32).unwrap_or(0)
}

/// Slot 56 (`vt+0xE0`). The loader calls this immediately after the factory
/// returns, so it must exist and must not fault. **[measured, `0x00538490`]**
unsafe extern "thiscall" fn ns_error_set_callback(
    this: *mut NetSysBase,
    cb: Option<unsafe extern "C" fn(i32)>,
) {
    if let Some(s) = st(this) {
        s.error_callback = cb;
    }
}

nop!(ns_get_group_data() -> *mut c_void = core::ptr::null_mut());
nop!(ns_get_service_data() -> *mut c_void = core::ptr::null_mut());
nop!(ns_delete_group_data(*mut c_void));
nop!(ns_delete_service_data(*mut c_void));
nop!(ns_log_state());
nop!(ns_notify_waiting_on_player(i32, u32));

// Slot 63 (`vt+0xFC`). Also called by the loader immediately after the
// factory returns, so it must exist and must not fault. **[measured]**
nop!(ns_set_profiler(*mut c_void));
nop!(ns_cleanup_system());

pub static NETSYS_VTABLE: NetSysVtable = NetSysVtable {
    destructor: ns_dtor,
    init: ns_init,
    close: ns_close,
    get_memory_manager: ns_get_memory_manager,
    is_host: ns_is_host,
    enable_join: ns_enable_join,
    set_playing: ns_set_playing,
    is_playing: ns_is_playing,
    is_joining_in_process: ns_is_joining_in_process,
    is_session_full: ns_is_session_full,
    get_url_string: ns_get_url_string,
    accept_host_messages: ns_accept_host_messages,
    set_number_players: ns_set_number_players,
    set_number_observers: ns_set_number_observers,
    send_dsync: ns_send_dsync,
    check_pulse: ns_check_pulse,
    set_time_out: ns_set_time_out,
    get_time_out: ns_get_time_out,
    set_allow_timeout: ns_set_allow_timeout,
    get_allow_timeout: ns_get_allow_timeout,
    get_num_allowed_players: ns_get_num_allowed_players,
    send: ns_send,
    send_all: ns_send_all,
    get: ns_get,
    poll_services: ns_poll_services,
    host: ns_host,
    join: ns_join,
    join_ip: ns_join_ip,
    cancel_joining: ns_cancel_joining,
    cancel_join: ns_cancel_join,
    cancel_join_skybox: ns_cancel_join_skybox,
    disconnect: ns_disconnect,
    poll_sessions: ns_poll_sessions,
    stop_poll_sessions: ns_stop_poll_sessions,
    clear_net_sessions: ns_clear_net_sessions,
    poll_players: ns_poll_players,
    find_player_from_id: ns_find_player_from_id,
    validate_player: ns_validate_player,
    update_recently_played_with_list: ns_update_recently_played_with_list,
    process_system_messages: ns_process_system_messages,
    send_drop_player: ns_send_drop_player,
    drop_player: ns_drop_player,
    cancel_drop_player: ns_cancel_drop_player,
    set_log: ns_set_log,
    log_set_frame: ns_log_set_frame,
    log_connection: ns_log_connection,
    log_connection2: ns_log_connection2,
    log_connection_fmt: ns_log_connection_fmt,
    get_ip_addresses: ns_get_ip_addresses,
    get_host_port: ns_get_host_port,
    set_host_port: ns_set_host_port,
    get_local_port: ns_get_local_port,
    set_local_port: ns_set_local_port,
    set_ip_override: ns_set_ip_override,
    set_matchmaking_id: ns_set_matchmaking_id,
    get_num_players: ns_get_num_players,
    error_set_callback: ns_error_set_callback,
    get_group_data: ns_get_group_data,
    get_service_data: ns_get_service_data,
    delete_group_data: ns_delete_group_data,
    delete_service_data: ns_delete_service_data,
    log_state: ns_log_state,
    notify_waiting_on_player: ns_notify_waiting_on_player,
    set_profiler: ns_set_profiler,
    cleanup_system: ns_cleanup_system,
};

// ---------------------------------------------------------------------------
// NetPlayer vtable
// ---------------------------------------------------------------------------

unsafe extern "thiscall" fn np_dtor(_t: *mut NetPlayerObj, _f: u32) -> *mut c_void {
    core::ptr::null_mut()
}
unsafe extern "thiscall" fn np_is_local(t: *mut NetPlayerObj) -> i32 {
    i32::from((*t).flags & SNLPLAYER_LOCAL != 0)
}
unsafe extern "thiscall" fn np_is_host(t: *mut NetPlayerObj) -> i32 {
    i32::from((*t).flags & SNLPLAYER_HOST != 0)
}
unsafe extern "thiscall" fn np_is_pending(t: *mut NetPlayerObj) -> i32 {
    i32::from((*t).flags & SNLPLAYER_PENDING != 0)
}
unsafe extern "thiscall" fn np_is_observer(t: *mut NetPlayerObj) -> i32 {
    i32::from((*t).flags & SNLPLAYER_OBSERVER != 0)
}
unsafe extern "thiscall" fn np_get_internal_name(t: *mut NetPlayerObj) -> *const c_void {
    (*t).name_utf8.as_ptr() as *const c_void
}
unsafe extern "thiscall" fn np_get_id(_t: *mut NetPlayerObj, out: *mut c_void) -> *mut c_void {
    // Returns a std::wstring by value via the hidden return slot. We cannot
    // construct one without MSVC's allocator, so hand back the caller's own
    // buffer untouched; every caller in the game copies from it and a
    // zero-initialised wstring reads as empty.
    out
}
unsafe extern "thiscall" fn np_get_platform_id(_t: *mut NetPlayerObj, out: *mut c_void) -> *mut c_void {
    out
}
unsafe extern "thiscall" fn np_get_platform(_t: *mut NetPlayerObj, out: *mut c_void) -> *mut c_void {
    out
}
unsafe extern "thiscall" fn np_get_player_index(t: *mut NetPlayerObj) -> i32 {
    (*t).player_index
}
unsafe extern "thiscall" fn np_get_game_version(t: *mut NetPlayerObj) -> u32 {
    (*t).game_version
}
unsafe extern "thiscall" fn np_get_ping_time(t: *mut NetPlayerObj) -> u32 {
    (*t).ping_ms
}
unsafe extern "thiscall" fn np_get_time_since_last_pulse(t: *mut NetPlayerObj) -> u32 {
    (*t).last_pulse_ms
}
unsafe extern "thiscall" fn np_get_send_queue_info(
    _t: *mut NetPlayerObj,
    a: *mut u32,
    b: *mut u32,
) {
    if !a.is_null() {
        *a = 0;
    }
    if !b.is_null() {
        *b = 0;
    }
}
unsafe extern "thiscall" fn np_reset_sync_counter(t: *mut NetPlayerObj) {
    (*t).sync_counter = 0;
}
unsafe extern "thiscall" fn np_inc_sync_counter(t: *mut NetPlayerObj) {
    (*t).sync_counter = (*t).sync_counter.wrapping_add(1);
}
unsafe extern "thiscall" fn np_get_sync_counter(t: *mut NetPlayerObj) -> u32 {
    (*t).sync_counter
}
unsafe extern "thiscall" fn np_get_name(t: *mut NetPlayerObj) -> *const c_void {
    (*t).name_utf8.as_ptr() as *const c_void
}
unsafe extern "thiscall" fn np_get_description(t: *mut NetPlayerObj) -> *const c_void {
    (*t).name_utf8.as_ptr() as *const c_void
}
unsafe extern "thiscall" fn np_set_name(_t: *mut NetPlayerObj, _n: *const c_void) {}
unsafe extern "thiscall" fn np_get_dsync_frame(t: *mut NetPlayerObj) -> i32 {
    (*t).dsync_frame
}

pub static NETPLAYER_VTABLE: NetPlayerVtable = NetPlayerVtable {
    destructor: np_dtor,
    is_local: np_is_local,
    is_host: np_is_host,
    is_pending: np_is_pending,
    is_observer: np_is_observer,
    get_internal_name: np_get_internal_name,
    get_id: np_get_id,
    get_platform_id: np_get_platform_id,
    get_platform: np_get_platform,
    get_player_index: np_get_player_index,
    get_game_version: np_get_game_version,
    get_ping_time: np_get_ping_time,
    get_time_since_last_pulse: np_get_time_since_last_pulse,
    get_send_queue_info: np_get_send_queue_info,
    reset_sync_counter: np_reset_sync_counter,
    inc_sync_counter: np_inc_sync_counter,
    get_sync_counter: np_get_sync_counter,
    get_name: np_get_name,
    get_description: np_get_description,
    set_name: np_set_name,
    get_dsync_frame: np_get_dsync_frame,
};
