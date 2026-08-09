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
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

extern "C" {
    fn malloc(size: usize) -> *mut c_void;
    fn free(block: *mut c_void);
}

#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleW(name: *const u16) -> *mut c_void;
}

#[link(name = "wininet")]
extern "system" {
    fn InternetGetConnectedState(flags: *mut u32, reserved: u32) -> i32;
}

/// Exact extent of `NetDaemon::data`, the destination passed to `NetSys::get`.
///
/// `rise.pdb` type `0x9C28` gives `NetDaemon::data` type `0x8912` at offset
/// `+8`; type `0x8912` is `unsigned char[2048]`. `NetDaemon::process` at
/// `0x00950F30` passes `this+8` directly to vtable `+0x5c`. The shipped
/// `NetFifo::get` at `0x10013550` copies the queued allocation's recorded size
/// without a capacity argument. Packets above this exact destination extent
/// are therefore refused before any copy.
const NETDAEMON_RECEIVE_EXTENT: usize = 2048;

static CONNECTED: AtomicBool = AtomicBool::new(false);

struct Trace {
    file: Option<File>,
    seen: BTreeSet<&'static str>,
    sequence: u64,
}

static TRACE: OnceLock<Mutex<Trace>> = OnceLock::new();

fn env_truthy(k: &str) -> bool {
    matches!(
        env(k).as_deref(),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

pub fn load_only_mode() -> bool {
    env_truthy("DON_NET_LOAD_ONLY")
}

fn trace_state() -> &'static Mutex<Trace> {
    TRACE.get_or_init(|| {
        let requested = env("DON_NET_TRACE")
            .map(std::path::PathBuf::from)
            .or_else(|| {
                load_only_mode().then(|| {
                    std::env::temp_dir().join(format!("don-netsys-shim-{}.log", std::process::id()))
                })
            });
        let file =
            requested.and_then(|path| OpenOptions::new().create(true).append(true).open(path).ok());
        Mutex::new(Trace {
            file,
            seen: BTreeSet::new(),
            sequence: 0,
        })
    })
}

fn write_trace(trace: &mut Trace, args: fmt::Arguments<'_>) {
    trace.sequence = trace.sequence.wrapping_add(1);
    let sequence = trace.sequence;
    let Some(file) = trace.file.as_mut() else {
        return;
    };
    let _ = writeln!(file, "seq={} pid={} {}", sequence, std::process::id(), args);
    // The load-only log is crash evidence. Do not leave its last calls in a
    // userspace buffer if retail faults immediately after an ABI boundary.
    let _ = file.flush();
}

pub fn trace_once(event: &'static str) {
    let Ok(mut trace) = trace_state().lock() else {
        return;
    };
    if trace.seen.insert(event) {
        write_trace(&mut trace, format_args!("call={event}"));
    }
}

pub fn trace_detail(args: fmt::Arguments<'_>) {
    let Ok(mut trace) = trace_state().lock() else {
        return;
    };
    write_trace(&mut trace, args);
}

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
    load_only: bool,
    game_version: u32,
    active: bool,
    local_member_id: Vec<u16>,
    setup_bridge: bool,
    bridged_slots: BTreeMap<i32, i32>,
}

#[repr(C)]
struct NetSysObj {
    base: NetSysBase,
    // Reproduce the direct-access prefix of shipped CrossplayNetLibSys. Retail
    // reads m_crossplay at +0xCC immediately after init returns success.
    flags: i32,
    net_messenger: *mut NetMessenger,
    reserved_to_crossplay: [u8; 108],
    m_crossplay: *mut c_void,
    reserved_shipped_tail: [u8; 768],
    /// Boxed so the C-visible prefix stays exactly `NetSysBase`.
    state: *mut State,
}

const _: () = {
    assert!(core::mem::offset_of!(NetSysObj, flags) == 0x58);
    assert!(core::mem::offset_of!(NetSysObj, net_messenger) == 0x5c);
    assert!(core::mem::offset_of!(NetSysObj, m_crossplay) == 0xcc);
    assert!(core::mem::offset_of!(NetSysObj, state) == 0x3d0);
};

/// Match shipped `CrossplayNetLib::is_connected_to_network` at VA
/// `0x10018550`: call `InternetGetConnectedState(&flags, 0)` and return whether
/// its BOOL result is nonzero. This is pre-session OS availability, not the
/// later NetSys roster/session state tracked by `CONNECTED`.
pub fn network_available() -> bool {
    // The disposable PE32 smoke cannot rely on Wine's WinINet implementation
    // completing. An explicit load-only-only override gives it a bounded
    // regression for the pre-session gate without changing retail semantics.
    if load_only_mode() {
        match env("DON_NET_CONNECTIVITY_OVERRIDE").as_deref() {
            Some("0") | Some("false") => return false,
            Some("1") | Some("true") => return true,
            _ => {}
        }
    }
    let mut flags = 0u32;
    unsafe { InternetGetConnectedState(&mut flags, 0) != 0 }
}

pub fn set_connected(v: bool) {
    // Shipped set_network_connection_state is an empty function. Connection
    // truth is owned by successful lifecycle transitions below, not by this
    // compatibility export.
    let _ = v;
}

pub unsafe fn is_load_only(this: *mut NetSysBase) -> bool {
    st(this).is_some_and(|s| s.load_only)
}

fn env(k: &str) -> Option<String> {
    std::env::var(k).ok()
}

fn build_transport(id: i32) -> std::io::Result<(TcpTransport, Role)> {
    if load_only_mode() {
        // A loopback ephemeral listener keeps the ordinary `Session` object
        // valid while making the diagnostic incapable of accepting a remote
        // connection or colliding with the configured gameplay port.
        return Ok((TcpTransport::host(id, "127.0.0.1:0")?, Role::Host));
    }
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
    trace_once("factory.get_netsys_object_ptr");
    let id = env("DON_NET_ID")
        .and_then(|v| v.parse::<i32>().ok())
        .unwrap_or_else(|| std::process::id() as i32);
    let name = env("DON_NET_NAME").unwrap_or_else(|| "donnet".into());

    let Ok((transport, role)) = build_transport(id) else {
        trace_detail(format_args!("factory=refused reason=transport"));
        return core::ptr::null_mut();
    };
    let local_addr = transport
        .local_addr()
        .map(|addr| addr.to_string())
        .unwrap_or_else(|_| "unavailable".into());
    let load_only = load_only_mode();
    // Merely loading the replacement is not a connection. The lifecycle
    // entry point makes this true after `init` retained retail's callbacks.
    CONNECTED.store(false, Ordering::Relaxed);
    trace_detail(format_args!(
        "factory=ready abi=netsys-v65 role={role:?} load_only={load_only} local_addr={local_addr}"
    ));
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
        load_only,
        game_version: 0,
        active: false,
        local_member_id: Vec::new(),
        setup_bridge: env_truthy("DON_NET_SETUP_BRIDGE"),
        bridged_slots: BTreeMap::new(),
    });
    let obj = Box::new(NetSysObj {
        base: NetSysBase {
            vftable: &NETSYS_VTABLE,
            // Match the shipped CrossplayNetLibSys constructor: membership is
            // empty until the first session/lobby materialisation. Advertising
            // one player while players[0] is null lets retail dereference a
            // contradiction before `pump` can populate the prefix.
            num_players: 0,
            players: [core::ptr::null_mut(); 8],
            local_player: core::ptr::null_mut(),
            host_player: core::ptr::null_mut(),
            local_player_connection: LOCAL_PLAYER_CONNECTED,
            local_player_disconnect_pct: 0.0,
            net_sessions: [0u8; 28],
            log: core::ptr::null_mut(),
        },
        // Shipped CrossplayNetLibSys constructor state at concrete +0x58.
        flags: 0x40,
        net_messenger: core::ptr::null_mut(),
        reserved_to_crossplay: [0; 108],
        m_crossplay: core::ptr::null_mut(),
        reserved_shipped_tail: [0; 768],
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
pub unsafe fn with<R>(
    this: *mut NetSysBase,
    f: impl FnOnce(&mut Session<TcpTransport>) -> R,
) -> Option<R> {
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

/// Encode every `i32` bit pattern bijectively into at most seven lower-case
/// base-36 code units (`u32::MAX == "1z141z3"`). This is our transport's
/// identity spelling, not a claim about PlayFab ids; its purpose is to keep
/// every returned MSVC `wstring` in the measured seven-code-unit SSO domain.
fn encode_id_sso(id: i32) -> ([u16; 8], u32) {
    let mut value = id as u32;
    let mut reversed = [0u16; 7];
    let mut count = 0usize;
    loop {
        let digit = (value % 36) as u8;
        reversed[count] = if digit < 10 {
            u16::from(b'0' + digit)
        } else {
            u16::from(b'a' + digit - 10)
        };
        count += 1;
        value /= 36;
        if value == 0 {
            break;
        }
    }
    let mut out = [0u16; 8];
    for i in 0..count {
        out[i] = reversed[count - 1 - i];
    }
    (out, count as u32)
}

const fn empty_game_string() -> MsvcGameString {
    let mut bytes = [0u8; 20];
    bytes[11] = 1;
    MsvcGameString { bytes }
}

fn sso_wstring(code_units: [u16; 8], len: u32) -> MsvcWstring {
    MsvcWstring {
        sso: code_units,
        len,
        capacity: 7,
    }
}

unsafe fn wstring_units(value: &MsvcWstring) -> Option<&[u16]> {
    let len = value.len as usize;
    if len > 128 || value.capacity < value.len {
        return None;
    }
    if value.capacity < 8 {
        return Some(&value.sso[..len]);
    }
    let ptr = core::ptr::read_unaligned(value.sso.as_ptr().cast::<*const u16>());
    if ptr.is_null() {
        return None;
    }
    Some(core::slice::from_raw_parts(ptr, len))
}

unsafe fn destroy_wstring(value: &mut MsvcWstring) {
    if value.capacity >= 8 {
        let ptr = core::ptr::read_unaligned(value.sso.as_ptr().cast::<*mut c_void>());
        if !ptr.is_null() {
            free(ptr);
        }
    }
    *value = sso_wstring([0; 8], 0);
}

unsafe fn owned_wstring(units: &[u16]) -> Option<MsvcWstring> {
    if units.len() <= 7 {
        let mut sso = [0u16; 8];
        sso[..units.len()].copy_from_slice(units);
        return Some(sso_wstring(sso, units.len() as u32));
    }
    let bytes = (units.len() + 1).checked_mul(core::mem::size_of::<u16>())?;
    let ptr = malloc(bytes).cast::<u16>();
    if ptr.is_null() {
        return None;
    }
    core::ptr::copy_nonoverlapping(units.as_ptr(), ptr, units.len());
    *ptr.add(units.len()) = 0;
    let mut storage = [0u16; 8];
    core::ptr::write_unaligned(storage.as_mut_ptr().cast::<*mut u16>(), ptr);
    Some(MsvcWstring {
        sso: storage,
        len: units.len() as u32,
        capacity: units.len() as u32,
    })
}

unsafe fn set_player_id(object: &mut NetPlayerObj, units: &[u16]) -> bool {
    if units.len() > object.id_wide.len() - 1 {
        return false;
    }
    let Some(id) = owned_wstring(units) else {
        return false;
    };
    let Some(platform_id) = owned_wstring(units) else {
        let mut id = id;
        destroy_wstring(&mut id);
        return false;
    };
    destroy_wstring(&mut object.id);
    destroy_wstring(&mut object.platform_id);
    object.id = id;
    object.platform_id = platform_id;
    object.id_wide = [0; 129];
    object.id_wide[..units.len()].copy_from_slice(units);
    object.id_len = units.len() as u16;
    true
}

impl Drop for NetPlayerObj {
    fn drop(&mut self) {
        unsafe {
            destroy_wstring(&mut self.id);
            destroy_wstring(&mut self.platform);
            destroy_wstring(&mut self.platform_id);
        }
    }
}

fn build_player(player: &don_net::session::Player) -> Box<NetPlayerObj> {
    let (id_sso, id_len) = encode_id_sso(player.unique_id);
    let mut id_wide = [0u16; 129];
    id_wide[..id_len as usize].copy_from_slice(&id_sso[..id_len as usize]);
    let mut platform = [0u16; 8];
    platform[..3].copy_from_slice(&[b'd' as u16, b'o' as u16, b'n' as u16]);
    let mut name_wide = [0u16; 65];
    let mut name_len = 0usize;
    for unit in player.name.encode_utf16().take(64) {
        name_wide[name_len] = unit;
        name_len += 1;
    }
    let mut object = Box::new(NetPlayerObj {
        vftable: &NETPLAYER_VTABLE,
        drop_requests: [0; 28],
        // Shipped process_playerlist calls on_player_added while this pending
        // bit is still set, then clears it immediately afterwards.
        flags: SNLPLAYER_PENDING,
        crossplay_player: core::ptr::null_mut(),
        ready: player.ready,
        ready_padding: [0; 3],
        id: sso_wstring(id_sso, id_len),
        platform: sso_wstring(platform, 3),
        platform_id: sso_wstring(id_sso, id_len),
        name: empty_game_string(),
        description: empty_game_string(),
        game_version: 0,
        last_pulse_ms: player.last_pulse_ms as u32,
        sync_counter: 0,
        dsync_frame: 0,
        unique_id: player.unique_id,
        player_index: player.slot as i32,
        ping_ms: 0,
        id_wide,
        id_len: id_len as u16,
        name_wide,
        name_len: name_len as u16,
    });
    // SetupWin::check_all_connected (retail 0x005BCC23) tests the concrete
    // CrossplayNetLibPlayer field at +0x24 for non-null. Our transport has no
    // ICrossplayPlayer, so the stable allocation itself is the durable opaque
    // membership handle. The game only compares this field in the recovered
    // setup gate; shim code never dispatches through it.
    object.crossplay_player = object.as_mut() as *mut NetPlayerObj as *mut c_void;
    object
}

fn update_player(object: &mut NetPlayerObj, player: &don_net::session::Player) {
    object.flags = (object.flags & SNLPLAYER_PENDING)
        | if player.is_host { SNLPLAYER_HOST } else { 0 }
        | if player.is_local { SNLPLAYER_LOCAL } else { 0 };
    object.ready = player.ready;
    object.last_pulse_ms = player.last_pulse_ms as u32;
    object.player_index = player.slot as i32;
}

fn game_string_from_wide(ptr: *const u16, len: u16) -> MsvcGameString {
    let mut value = empty_game_string();
    value.bytes[..4].copy_from_slice(&(ptr as u32).to_le_bytes());
    value.bytes[4..6].copy_from_slice(&len.to_le_bytes());
    value.bytes[8..10].copy_from_slice(&len.to_le_bytes());
    value.bytes[10] = 1; // const-literal form; no allocation ownership
    value
}

unsafe fn messenger(this: *mut NetSysBase) -> Option<*mut NetMessenger> {
    let object = this as *mut NetSysObj;
    if object.is_null() || (*object).net_messenger.is_null() {
        None
    } else {
        Some((*object).net_messenger)
    }
}

unsafe fn notify_player_added(this: *mut NetSysBase, player: *const NetPlayerObj) {
    let Some(messenger) = messenger(this) else {
        return;
    };
    let vtable = (*messenger).vftable;
    if !vtable.is_null() {
        trace_detail(format_args!(
            "callback=NetMessenger.on_player_added player_non_null={}",
            !player.is_null()
        ));
        ((*vtable).on_player_added)(messenger, player);
    }
}

unsafe fn notify_player_deleted(this: *mut NetSysBase, player: *const NetPlayerObj) {
    let Some(messenger) = messenger(this) else {
        return;
    };
    let vtable = (*messenger).vftable;
    if !vtable.is_null() {
        trace_detail(format_args!(
            "callback=NetMessenger.on_player_deleted player_non_null={}",
            !player.is_null()
        ));
        ((*vtable).on_player_deleted)(messenger, player);
    }
}

const CONNECTION_DATA_RVA: usize = 0x00aa_e448;
const SETUPWIN_GLOBAL_RVA: usize = 0x008a_b3a0;
const CONNECTION_ADD_PLAYER_RVA: usize = 0x0054_f560;
const CONNECTION_GET_PLAYER_INDEX_RVA: usize = 0x0054_d180;
const CONNECTION_SEND_PLAYER_RVA: usize = 0x0054_ec40;

unsafe fn bridge_context(this: *mut NetSysBase) -> Option<(*mut u8, *mut c_void)> {
    let state = st(this)?;
    if state.load_only || !state.setup_bridge || state.session.role != Role::Host {
        return None;
    }
    let base = GetModuleHandleW(core::ptr::null()).cast::<u8>();
    if base.is_null() {
        return None;
    }
    let setup = *base.add(SETUPWIN_GLOBAL_RVA).cast::<*mut c_void>();
    if setup.is_null() {
        trace_detail(format_args!("setup_bridge=deferred reason=setupwin-null"));
        return None;
    }
    Some((base, base.add(CONNECTION_DATA_RVA).cast()))
}

unsafe fn bridge_add_player(this: *mut NetSysBase, player: *const NetPlayerObj) -> Option<i32> {
    if player.is_null() || (*player).flags & SNLPLAYER_LOCAL != 0 {
        return None;
    }
    let (base, connection_data) = bridge_context(this)?;
    let units = &(&(*player).id_wide)[..(*player).id_len as usize];
    let add_id = owned_wstring(units)?;
    let find_id = owned_wstring(units)?;
    let add_player: unsafe extern "thiscall" fn(*mut c_void, MsvcWstring) =
        core::mem::transmute(base.add(CONNECTION_ADD_PLAYER_RVA));
    let get_player_index: unsafe extern "thiscall" fn(*mut c_void, MsvcWstring) -> i32 =
        core::mem::transmute(base.add(CONNECTION_GET_PLAYER_INDEX_RVA));
    add_player(connection_data, add_id);
    let slot = get_player_index(connection_data, find_id);
    if !(0..8).contains(&slot) {
        trace_detail(format_args!(
            "setup_bridge=refused action=add_player reason=slot-not-found"
        ));
        return None;
    }
    trace_detail(format_args!(
        "setup_bridge=ok action=add_player slot={slot} credential_material=none"
    ));
    Some(slot)
}

unsafe fn bridge_ready(this: *mut NetSysBase, unique_id: i32, ready: bool) -> bool {
    let slot = {
        let Some(state) = st(this) else { return false };
        let Some(&slot) = state.bridged_slots.get(&unique_id) else {
            return false;
        };
        slot
    };
    let Some((base, connection_data)) = bridge_context(this) else {
        return false;
    };
    // PlayerConnectionData is exactly 59 bytes; ready is byte +58.
    *connection_data.cast::<u8>().add(slot as usize * 59 + 58) = u8::from(ready);
    let send_player: unsafe extern "thiscall" fn(*mut c_void, i32, bool) =
        core::mem::transmute(base.add(CONNECTION_SEND_PLAYER_RVA));
    send_player(connection_data, slot, true);
    trace_detail(format_args!(
        "setup_bridge=ok action=ready slot={slot} ready={ready} credential_material=none"
    ));
    true
}

/// Refresh the `NetPlayer*` array the game can read directly, and drain new
/// game-layer packets into our own inbox.
unsafe fn pump(this: *mut NetSysBase) {
    let (removed, added_ptrs, next_local, next_host, ready_changes) = {
        let Some(s) = st(this) else { return };
        if !s.active {
            return;
        }
        let t = now_ms(s);
        let _ = s.session.poll(t, Duration::from_millis(0));
        let mut ready_changes = Vec::new();
        for e in s.session.drain_events() {
            match e {
                don_net::session::Event::Game { from, msg } => {
                    if msg.bytes.len() <= NETDAEMON_RECEIVE_EXTENT {
                        s.inbox.push_back((from, msg.bytes));
                    }
                }
                don_net::session::Event::ReadyChanged { unique_id, ready } => {
                    ready_changes.push((unique_id, ready));
                }
                _ => {}
            }
        }
        // Keep each NetPlayer allocation stable. Shipped OnPlayerLeft first
        // removes the pointer from NetSys::players, then invokes
        // NetMessenger::on_player_deleted while the object remains alive
        // (0x10018B4B..0x10018BAD). Rebuilding every box would violate both parts.
        let roster = s.session.players().to_vec();
        let mut old = core::mem::take(&mut s.players);
        let mut next = Vec::with_capacity(roster.len());
        let mut added = Vec::new();
        for player in &roster {
            let mut object = if let Some(index) = old
                .iter()
                .position(|candidate| candidate.unique_id == player.unique_id)
            {
                old.remove(index)
            } else {
                added.push(player.unique_id);
                build_player(player)
            };
            update_player(&mut object, player);
            if player.is_local
                && !s.local_member_id.is_empty()
                && object.id_wide[..object.id_len as usize] != s.local_member_id
            {
                let _ = set_player_id(&mut object, &s.local_member_id);
            }
            object.game_version = s.game_version;
            next.push(object);
        }
        s.players = next;
        let obj = this as *mut NetSysObj;
        (*obj).base.num_players = s.players.len() as i32;
        (*obj).base.players = [core::ptr::null_mut(); 8];
        for (i, p) in s.players.iter_mut().enumerate().take(8) {
            (*obj).base.players[i] = p.as_mut() as *mut NetPlayerObj;
        }
        let next_local = s
            .players
            .iter_mut()
            .find(|p| p.flags & SNLPLAYER_LOCAL != 0)
            .map(|p| p.as_mut() as *mut NetPlayerObj)
            .unwrap_or(core::ptr::null_mut());
        let next_host = s
            .players
            .iter_mut()
            .find(|p| p.flags & SNLPLAYER_HOST != 0)
            .map(|p| p.as_mut() as *mut NetPlayerObj)
            .unwrap_or(core::ptr::null_mut());
        let local_removed = old
            .iter()
            .any(|player| core::ptr::eq(player.as_ref(), (*obj).base.local_player));
        let host_removed = old
            .iter()
            .any(|player| core::ptr::eq(player.as_ref(), (*obj).base.host_player));
        // Additions must see coherent direct pointers. For removal, shipped
        // leaves the old host/local pointer intact until the delete callback
        // returns, even though players[]/num_players were already compacted.
        if !local_removed {
            (*obj).base.local_player = next_local;
        }
        if !host_removed {
            (*obj).base.host_player = next_host;
        }
        let added_ptrs = added
            .into_iter()
            .filter_map(|id| {
                s.players
                    .iter()
                    .find(|player| player.unique_id == id)
                    .map(|player| player.as_ref() as *const NetPlayerObj)
            })
            .collect::<Vec<_>>();
        (old, added_ptrs, next_local, next_host, ready_changes)
    };

    // The prefix mutation and Rust borrows are complete before retail sees a
    // callback. Removed boxes stay alive through on_player_deleted.
    for player in &removed {
        notify_player_deleted(this, player.as_ref());
    }
    let object = this as *mut NetSysObj;
    (*object).base.local_player = next_local;
    (*object).base.host_player = next_host;
    for player in added_ptrs {
        let bridged_slot = bridge_add_player(this, player);
        if let Some(slot) = bridged_slot {
            if let Some(state) = st(this) {
                state.bridged_slots.insert((*player).unique_id, slot);
            }
        } else {
            notify_player_added(this, player);
        }
        // Exact shipped ordering: process_playerlist clears pending only after
        // NetMessenger::on_player_added returns (0x1001AADE..0x1001AAF4).
        (*(player as *mut NetPlayerObj)).flags &= !SNLPLAYER_PENDING;
    }
    for (unique_id, ready) in ready_changes {
        let _ = bridge_ready(this, unique_id, ready);
    }
}

/// Activate only the loopback/ephemeral diagnostic session so the external
/// PE32 smoke can dispatch every NetPlayer slot. This never enables traffic
/// and refuses normal retail mode.
pub unsafe fn materialize_load_only_peer(this: *mut NetSysBase) -> bool {
    let Some(state) = st(this) else { return false };
    if !state.load_only {
        return false;
    }
    state.active = true;
    pump(this);
    true
}

/// Capture the authenticated retail host's own lobby-member id without ever
/// logging it. `LobbyMemberDTO` inherits `UserDTO`; its first field is the
/// measured 24-byte MSVC `wstring _userId`. The value becomes only a local
/// alias, so the owned TCP peer still needs no account or ticket.
pub unsafe fn capture_local_member_id(this: *mut NetSysBase, member: *const c_void) -> bool {
    if member.is_null() {
        return false;
    }
    let Some(units) = wstring_units(&*member.cast::<MsvcWstring>()) else {
        return false;
    };
    if units.is_empty() {
        return false;
    }
    let copy = units.to_vec();
    let Some(state) = st(this) else { return false };
    state.local_member_id = copy;
    if let Some(player) = state
        .players
        .iter_mut()
        .find(|player| player.flags & SNLPLAYER_LOCAL != 0)
    {
        let ok = set_player_id(player, &state.local_member_id);
        trace_detail(format_args!(
            "local_member_id=captured code_units={} player_updated={ok}",
            state.local_member_id.len()
        ));
        ok
    } else {
        trace_detail(format_args!(
            "local_member_id=captured code_units={} player_updated=false",
            state.local_member_id.len()
        ));
        true
    }
}

// ---------------------------------------------------------------------------
// NetSys vtable thunks
// ---------------------------------------------------------------------------

macro_rules! nop {
    ($name:ident ( $($a:ty),* )) => {
        unsafe extern "thiscall" fn $name(_this: *mut NetSysBase $(, _: $a)*) {
            trace_once(concat!("vtable.", stringify!($name)));
        }
    };
    ($name:ident ( $($a:ty),* ) -> $r:ty = $v:expr) => {
        unsafe extern "thiscall" fn $name(_this: *mut NetSysBase $(, _: $a)*) -> $r {
            trace_once(concat!("vtable.", stringify!($name)));
            $v
        }
    };
}

unsafe extern "thiscall" fn ns_dtor(_this: *mut NetSysBase, _flags: u32) -> *mut c_void {
    trace_once("vtable.ns_dtor");
    // The game never deletes us; see `create`.
    core::ptr::null_mut()
}

unsafe extern "thiscall" fn ns_init(
    this: *mut NetSysBase,
    messenger: *mut c_void,
    crossplay_service: *const c_void,
    game_version: i32,
    _b: i32,
    _c: u32,
    _d: u32,
) -> i32 {
    trace_once("vtable.ns_init");
    let object = this as *mut NetSysObj;
    if object.is_null() {
        return LIBERR_NOT_AVAILABLE;
    }
    (*object).net_messenger = messenger.cast();
    (*object).m_crossplay = crossplay_service.cast_mut();
    if let Some(state) = st(this) {
        // Shipped init stores its first integer argument at the concrete
        // CrossplayNetLibSys game-version field (+0x270).
        state.game_version = game_version as u32;
    }
    trace_detail(format_args!(
        "init=stored messenger={} crossplay_service={} object_size=0x3d4",
        !messenger.is_null(),
        !crossplay_service.is_null()
    ));
    // Returning success authorises retail's immediate `[netsys+0xCC]` service
    // vcall. Only do so when both retained pointers are concrete; load-only is
    // still safe because every session/traffic entry point remains refused.
    if messenger.is_null() || crossplay_service.is_null() {
        return LIBERR_NOT_AVAILABLE;
    }
    LIBERR_OK
}

unsafe extern "thiscall" fn ns_close(this: *mut NetSysBase) {
    trace_once("vtable.ns_close");
    if let Some(state) = st(this) {
        state.active = false;
        state.joining = 0;
    }
    let object = this as *mut NetSysObj;
    if !object.is_null() {
        (*object).flags = 0;
    }
    CONNECTED.store(false, Ordering::Relaxed);
}

nop!(ns_get_memory_manager() -> *mut c_void = core::ptr::null_mut());

unsafe extern "thiscall" fn ns_is_host(this: *mut NetSysBase) -> bool {
    trace_once("vtable.ns_is_host");
    st(this)
        .map(|s| s.session.role == Role::Host)
        .unwrap_or(false)
}

nop!(ns_enable_join(i32));

unsafe extern "thiscall" fn ns_set_playing(this: *mut NetSysBase, v: i32) {
    trace_once("vtable.ns_set_playing");
    if let Some(s) = st(this) {
        s.playing = v;
    }
}

unsafe extern "thiscall" fn ns_is_playing(this: *mut NetSysBase) -> i32 {
    trace_once("vtable.ns_is_playing");
    st(this).map(|s| s.playing).unwrap_or(0)
}

unsafe extern "thiscall" fn ns_is_joining_in_process(this: *mut NetSysBase) -> i32 {
    trace_once("vtable.ns_is_joining_in_process");
    st(this).map(|s| s.joining).unwrap_or(0)
}

unsafe extern "thiscall" fn ns_is_session_full(this: *mut NetSysBase) -> i32 {
    trace_once("vtable.ns_is_session_full");
    st(this)
        .map(|s| i32::from(s.session.players().len() as i32 >= s.num_allowed_players))
        .unwrap_or(0)
}

static EMPTY_GAME_STRING: MsvcGameString = empty_game_string();

unsafe extern "thiscall" fn ns_get_url_string(_this: *mut NetSysBase) -> *const MsvcGameString {
    trace_once("vtable.ns_get_url_string");
    &EMPTY_GAME_STRING
}

unsafe extern "thiscall" fn ns_accept_host_messages(this: *mut NetSysBase, v: i32) {
    trace_once("vtable.ns_accept_host_messages");
    if let Some(s) = st(this) {
        s.accept_host_messages = v;
    }
}

unsafe extern "thiscall" fn ns_set_number_players(this: *mut NetSysBase, v: i32) {
    trace_once("vtable.ns_set_number_players");
    if let Some(s) = st(this) {
        s.num_allowed_players = v;
    }
}

nop!(ns_set_number_observers(i32));

unsafe extern "thiscall" fn ns_send_dsync(this: *mut NetSysBase, frame: i32) {
    trace_once("vtable.ns_send_dsync");
    if st(this).is_some_and(|s| s.load_only) {
        return;
    }
    let _ = with(this, |s| s.send_dsync(frame));
}

unsafe extern "thiscall" fn ns_check_pulse(this: *mut NetSysBase) {
    trace_once("vtable.ns_check_pulse");
    pump(this);
}

unsafe extern "thiscall" fn ns_set_time_out(this: *mut NetSysBase, v: u32) {
    trace_once("vtable.ns_set_time_out");
    if let Some(s) = st(this) {
        s.time_out_ms = v;
        s.session.set_timeout_ms(v as u64);
    }
}

unsafe extern "thiscall" fn ns_get_time_out(this: *mut NetSysBase) -> u32 {
    trace_once("vtable.ns_get_time_out");
    st(this).map(|s| s.time_out_ms).unwrap_or(0)
}

unsafe extern "thiscall" fn ns_set_allow_timeout(this: *mut NetSysBase, v: i32) {
    trace_once("vtable.ns_set_allow_timeout");
    if let Some(s) = st(this) {
        s.allow_timeout = v;
    }
}

unsafe extern "thiscall" fn ns_get_allow_timeout(this: *mut NetSysBase) -> i32 {
    trace_once("vtable.ns_get_allow_timeout");
    st(this).map(|s| s.allow_timeout).unwrap_or(1)
}

unsafe extern "thiscall" fn ns_get_num_allowed_players(this: *mut NetSysBase) -> i32 {
    trace_once("vtable.ns_get_num_allowed_players");
    st(this).map(|s| s.num_allowed_players).unwrap_or(8)
}

unsafe extern "thiscall" fn ns_send(
    this: *mut NetSysBase,
    packet: *const u8,
    size: i32,
    to: *const NetPlayerObj,
    _flags: i32,
) -> bool {
    trace_once("vtable.ns_send");
    if packet.is_null()
        || size <= 0
        || size as usize > NETDAEMON_RECEIVE_EXTENT
        || st(this).is_some_and(|s| s.load_only)
    {
        return false;
    }
    let bytes = core::slice::from_raw_parts(packet, size as usize);
    let dest = if to.is_null() {
        Dest::All
    } else {
        Dest::One((*to).unique_id)
    };
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
    trace_once("vtable.ns_send_all");
    if packet.is_null()
        || size <= 0
        || size as usize > NETDAEMON_RECEIVE_EXTENT
        || st(this).is_some_and(|s| s.load_only)
    {
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
    trace_once("vtable.ns_get");
    if st(this).is_some_and(|s| s.load_only) {
        return false;
    }
    pump(this);
    let Some(s) = st(this) else { return false };
    let Some((sender, bytes)) = s.inbox.pop_front() else {
        return false;
    };
    if packet.is_null() || bytes.len() > NETDAEMON_RECEIVE_EXTENT {
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

unsafe extern "thiscall" fn ns_poll_services(this: *mut NetSysBase, _services: *mut c_void) -> i32 {
    trace_once("vtable.ns_poll_services");
    if st(this).is_some_and(|s| s.load_only) {
        LIBERR_NOT_AVAILABLE
    } else {
        LIBERR_OK
    }
}

unsafe extern "thiscall" fn ns_host(
    this: *mut NetSysBase,
    _a: *const c_void,
    _b: *const c_void,
    _c: *const c_void,
    _d: *const c_void,
    _e: *const c_void,
) -> i32 {
    trace_once("vtable.ns_host");
    let object = this as *mut NetSysObj;
    let ready =
        !object.is_null() && !(*object).net_messenger.is_null() && !(*object).m_crossplay.is_null();
    let Some(state) = st(this) else {
        return LIBERR_NOT_AVAILABLE;
    };
    if state.load_only || state.session.role != Role::Host || !ready {
        return LIBERR_NOT_AVAILABLE;
    }
    state.active = true;
    state.joining = 0;
    (*object).flags |= 0x11;
    CONNECTED.store(true, Ordering::Relaxed);
    pump(this);
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
    trace_once("vtable.ns_join");
    let object = this as *mut NetSysObj;
    let ready =
        !object.is_null() && !(*object).net_messenger.is_null() && !(*object).m_crossplay.is_null();
    {
        let Some(state) = st(this) else {
            return LIBERR_NOT_AVAILABLE;
        };
        if state.load_only || state.session.role != Role::Client || !ready {
            return LIBERR_NOT_AVAILABLE;
        }
        state.active = true;
        state.joining = 1;
    }
    (*object).flags |= 0x90;
    CONNECTED.store(true, Ordering::Relaxed);
    pump(this);
    // There is no lobby DTO round-trip in the owned TCP route. The local
    // player is materialised synchronously by pump, which is the equivalent
    // point at which shipped OnPlayerJoined clears bit 0x80.
    if let Some(state) = st(this) {
        state.joining = 0;
    }
    (*object).flags &= !0x80;
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
    trace_once("vtable.ns_join_ip");
    // Shipped join_ip is a dead `return 26`; the replacement deliberately
    // activates it as the zero-credential owned-TCP seam, with the same state
    // transition as `join` and no interpretation of lobby/auth arguments.
    ns_join(
        this,
        core::ptr::null(),
        core::ptr::null(),
        core::ptr::null(),
        core::ptr::null(),
        0,
    )
}

unsafe extern "thiscall" fn ns_cancel_joining(this: *mut NetSysBase) {
    trace_once("vtable.ns_cancel_joining");
    if let Some(s) = st(this) {
        s.joining = 0;
    }
}

nop!(ns_cancel_join());
nop!(ns_cancel_join_skybox());
unsafe extern "thiscall" fn ns_disconnect(this: *mut NetSysBase, _forced: bool) -> i32 {
    trace_once("vtable.ns_disconnect");
    if st(this).is_some_and(|s| s.load_only) {
        LIBERR_NOT_AVAILABLE
    } else {
        ns_close(this);
        LIBERR_OK
    }
}
nop!(ns_poll_sessions(*const c_void) -> i32 = LIBERR_OK);
nop!(ns_stop_poll_sessions());
nop!(ns_clear_net_sessions(u32));
unsafe extern "thiscall" fn ns_poll_players(
    this: *mut NetSysBase,
    _session: *const c_void,
    players: MsvcArrayNetPlayers,
) -> i32 {
    trace_once("vtable.ns_poll_players");
    // The shipped slot destroys the by-value 28-byte Array before `ret 0x20`.
    let backing = core::ptr::read_unaligned(players.bytes.as_ptr().add(0x10).cast::<*mut c_void>());
    if !backing.is_null() {
        free(backing);
    }
    if st(this).is_some_and(|s| s.load_only) {
        LIBERR_NOT_AVAILABLE
    } else {
        LIBERR_OK
    }
}

unsafe extern "thiscall" fn ns_find_player_from_id(
    this: *mut NetSysBase,
    mut id: MsvcWstring,
) -> *const NetPlayerObj {
    trace_once("vtable.ns_find_player_from_id");
    let result = wstring_units(&id).and_then(|wanted| {
        st(this).and_then(|state| {
            state
                .players
                .iter()
                .find(|player| player.id_wide[..player.id_len as usize] == *wanted)
                .map(|player| player.as_ref() as *const NetPlayerObj)
        })
    });
    // Shipped destroys the by-value wstring and returns with `ret 0x18`.
    destroy_wstring(&mut id);
    result.unwrap_or(core::ptr::null())
}

unsafe extern "thiscall" fn ns_validate_player(
    this: *mut NetSysBase,
    p: *const NetPlayerObj,
) -> i32 {
    trace_once("vtable.ns_validate_player");
    if p.is_null() {
        return 0;
    }
    st(this)
        .map(|s| i32::from(s.players.iter().any(|q| q.unique_id == (*p).unique_id)))
        .unwrap_or(0)
}

nop!(ns_update_recently_played_with_list());

unsafe extern "thiscall" fn ns_process_system_messages(this: *mut NetSysBase) {
    trace_once("vtable.ns_process_system_messages");
    pump(this);
}

nop!(ns_send_drop_player(*const NetPlayerObj));
nop!(ns_drop_player(*const NetPlayerObj));
nop!(ns_cancel_drop_player(*const NetPlayerObj));
nop!(ns_set_log(*mut c_void));
nop!(ns_log_set_frame(i32));
nop!(ns_log_connection(*const c_void));
nop!(ns_log_connection2(*const c_void, i32));

/// `__cdecl`, variadic — see the note on the vtable field. Ignoring the
/// variadic tail is safe precisely because the caller cleans the stack.
unsafe extern "C" fn ns_log_connection_fmt(_this: *mut NetSysBase, _fmt: *const u16) {
    trace_once("vtable.ns_log_connection_fmt");
}

nop!(ns_get_ip_addresses() -> *mut c_void = core::ptr::null_mut());

unsafe extern "thiscall" fn ns_get_host_port(this: *mut NetSysBase) -> u32 {
    trace_once("vtable.ns_get_host_port");
    st(this).map(|s| s.host_port).unwrap_or(0)
}
unsafe extern "thiscall" fn ns_set_host_port(this: *mut NetSysBase, v: u32) {
    trace_once("vtable.ns_set_host_port");
    if let Some(s) = st(this) {
        s.host_port = v;
    }
}
unsafe extern "thiscall" fn ns_get_local_port(this: *mut NetSysBase) -> u32 {
    trace_once("vtable.ns_get_local_port");
    st(this).map(|s| s.local_port).unwrap_or(0)
}
unsafe extern "thiscall" fn ns_set_local_port(this: *mut NetSysBase, v: u32) {
    trace_once("vtable.ns_set_local_port");
    if let Some(s) = st(this) {
        s.local_port = v;
    }
}

nop!(ns_set_ip_override(*const c_void));

unsafe extern "thiscall" fn ns_set_matchmaking_id(this: *mut NetSysBase, v: i32) {
    trace_once("vtable.ns_set_matchmaking_id");
    if let Some(s) = st(this) {
        s.matchmaking_id = v;
    }
}

unsafe extern "thiscall" fn ns_get_num_players(this: *mut NetSysBase) -> i32 {
    trace_once("vtable.ns_get_num_players");
    pump(this);
    st(this).map(|s| s.players.len() as i32).unwrap_or(0)
}

/// Slot 56 (`vt+0xE0`). The loader calls this immediately after the factory
/// returns, so it must exist and must not fault. **[measured, `0x00538490`]**
unsafe extern "thiscall" fn ns_error_set_callback(
    this: *mut NetSysBase,
    cb: Option<unsafe extern "C" fn(i32)>,
) {
    trace_once("vtable.ns_error_set_callback");
    if let Some(s) = st(this) {
        s.error_callback = cb;
    }
}

nop!(ns_get_group_data(*mut c_void) -> *mut c_void = core::ptr::null_mut());
nop!(ns_get_service_data(*mut c_void) -> *mut c_void = core::ptr::null_mut());
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
    trace_once("netplayer.destructor");
    core::ptr::null_mut()
}
unsafe extern "thiscall" fn np_is_local(t: *mut NetPlayerObj) -> i32 {
    trace_once("netplayer.is_local");
    i32::from((*t).flags & SNLPLAYER_LOCAL != 0)
}
unsafe extern "thiscall" fn np_is_host(t: *mut NetPlayerObj) -> i32 {
    trace_once("netplayer.is_host");
    i32::from((*t).flags & SNLPLAYER_HOST != 0)
}
unsafe extern "thiscall" fn np_is_pending(t: *mut NetPlayerObj) -> i32 {
    trace_once("netplayer.is_pending");
    i32::from((*t).flags & SNLPLAYER_PENDING != 0)
}
unsafe extern "thiscall" fn np_is_observer(t: *mut NetPlayerObj) -> i32 {
    trace_once("netplayer.is_observer");
    i32::from((*t).flags & SNLPLAYER_OBSERVER != 0)
}
unsafe fn write_empty_game_string(out: *mut MsvcGameString) -> *mut MsvcGameString {
    if out.is_null() {
        return core::ptr::null_mut();
    }
    *out = empty_game_string();
    out
}

unsafe extern "thiscall" fn np_get_internal_name(
    t: *mut NetPlayerObj,
    out: *mut MsvcGameString,
) -> *mut MsvcGameString {
    trace_once("netplayer.get_internal_name");
    if t.is_null() || out.is_null() {
        return core::ptr::null_mut();
    }
    *out = game_string_from_wide((*t).name_wide.as_ptr(), (*t).name_len);
    out
}

unsafe extern "thiscall" fn np_set_name(_t: *mut NetPlayerObj, _name: *const MsvcGameString) {
    trace_once("netplayer.set_name");
}

unsafe extern "thiscall" fn np_get_internal_description(
    _t: *mut NetPlayerObj,
    out: *mut MsvcGameString,
) -> *mut MsvcGameString {
    trace_once("netplayer.get_internal_description");
    write_empty_game_string(out)
}

unsafe extern "thiscall" fn np_set_description(
    _t: *mut NetPlayerObj,
    _description: *const MsvcGameString,
) {
    trace_once("netplayer.set_description");
}
unsafe fn write_sso_wstring(
    out: *mut MsvcWstring,
    code_units: &[u16; 8],
    len: u32,
) -> *mut MsvcWstring {
    if out.is_null() || len > 7 {
        return core::ptr::null_mut();
    }
    (*out).sso = [0; 8];
    (&mut (*out).sso)[..len as usize].copy_from_slice(&code_units[..len as usize]);
    (*out).len = len;
    (*out).capacity = 7;
    out
}

unsafe extern "thiscall" fn np_get_id(
    t: *mut NetPlayerObj,
    out: *mut MsvcWstring,
) -> *mut MsvcWstring {
    trace_once("netplayer.get_id");
    if t.is_null() {
        return core::ptr::null_mut();
    }
    let units = &(&(*t).id_wide)[..(*t).id_len as usize];
    let Some(value) = owned_wstring(units) else {
        return core::ptr::null_mut();
    };
    *out = value;
    out
}

unsafe extern "thiscall" fn np_get_platform_id(
    t: *mut NetPlayerObj,
    out: *mut MsvcWstring,
) -> *mut MsvcWstring {
    trace_once("netplayer.get_platform_id");
    if t.is_null() {
        return core::ptr::null_mut();
    }
    let units = &(&(*t).id_wide)[..(*t).id_len as usize];
    let Some(value) = owned_wstring(units) else {
        return core::ptr::null_mut();
    };
    *out = value;
    out
}

unsafe extern "thiscall" fn np_get_platform(
    t: *mut NetPlayerObj,
    out: *mut MsvcWstring,
) -> *mut MsvcWstring {
    trace_once("netplayer.get_platform");
    if t.is_null() {
        return core::ptr::null_mut();
    }
    write_sso_wstring(out, &(*t).platform.sso, (*t).platform.len)
}
unsafe extern "thiscall" fn np_get_player_index(t: *mut NetPlayerObj) -> i32 {
    trace_once("netplayer.get_player_index");
    (*t).player_index
}
unsafe extern "thiscall" fn np_get_game_version(t: *mut NetPlayerObj) -> u32 {
    trace_once("netplayer.get_game_version");
    (*t).game_version
}
unsafe extern "thiscall" fn np_get_ping_time(t: *mut NetPlayerObj) -> u32 {
    trace_once("netplayer.get_ping_time");
    (*t).ping_ms
}
unsafe extern "thiscall" fn np_get_time_since_last_pulse(t: *mut NetPlayerObj) -> u32 {
    trace_once("netplayer.get_time_since_last_pulse");
    (*t).last_pulse_ms
}
unsafe extern "thiscall" fn np_get_send_queue_info(
    _t: *mut NetPlayerObj,
    a: *mut u32,
    b: *mut u32,
) {
    trace_once("netplayer.get_send_queue_info");
    if !a.is_null() {
        *a = 0;
    }
    if !b.is_null() {
        *b = 0;
    }
}
unsafe extern "thiscall" fn np_reset_sync_counter(t: *mut NetPlayerObj) {
    trace_once("netplayer.reset_sync_counter");
    (*t).sync_counter = 0;
}
unsafe extern "thiscall" fn np_inc_sync_counter(t: *mut NetPlayerObj) {
    trace_once("netplayer.inc_sync_counter");
    (*t).sync_counter = (*t).sync_counter.wrapping_add(1);
}
unsafe extern "thiscall" fn np_get_sync_counter(t: *mut NetPlayerObj) -> i32 {
    trace_once("netplayer.get_sync_counter");
    (*t).sync_counter as i32
}
unsafe extern "thiscall" fn np_log_state(_t: *mut NetPlayerObj) {
    trace_once("netplayer.log_state");
}

pub static NETPLAYER_VTABLE: NetPlayerVtable = NetPlayerVtable {
    destructor: np_dtor,
    is_local: np_is_local,
    is_host: np_is_host,
    is_pending: np_is_pending,
    is_observer: np_is_observer,
    get_internal_name: np_get_internal_name,
    set_name: np_set_name,
    get_internal_description: np_get_internal_description,
    set_description: np_set_description,
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
    log_state: np_log_state,
};

#[cfg(test)]
mod tests {
    use super::*;

    fn text(buf: &[u16]) -> String {
        String::from_utf16(buf).unwrap()
    }

    #[test]
    fn every_i32_identity_fits_the_measured_wstring_sso_domain() {
        for (id, expected) in [
            (0, "0"),
            (1, "1"),
            (-1, "1z141z3"),
            (i32::MIN, "zik0zk"),
            (i32::MAX, "zik0zj"),
        ] {
            let (encoded, len) = encode_id_sso(id);
            assert!(len <= 7);
            assert_eq!(text(&encoded[..len as usize]), expected);
            assert_eq!(encoded[len as usize], 0);
        }
    }

    #[test]
    fn returned_wstring_is_a_complete_non_owning_msvc_sso_object() {
        let (encoded, len) = encode_id_sso(-1);
        let mut out = MsvcWstring {
            sso: [0xffff; 8],
            len: u32::MAX,
            capacity: u32::MAX,
        };
        let result = unsafe { write_sso_wstring(&mut out, &encoded, len) };
        assert!(core::ptr::eq(result, &out));
        assert_eq!(out.len, 7);
        assert_eq!(out.capacity, 7);
        assert_eq!(text(&out.sso[..out.len as usize]), "1z141z3");
        assert_eq!(out.sso[7], 0);
    }

    #[test]
    fn receive_copy_ceiling_is_the_pdb_array_extent() {
        assert_eq!(NETDAEMON_RECEIVE_EXTENT, 2048);
    }
}
