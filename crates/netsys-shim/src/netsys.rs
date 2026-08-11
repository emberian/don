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
use don_net::extension::GameKeySource;
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
    fn GetModuleFileNameW(module: *mut c_void, filename: *mut u16, size: u32) -> u32;
}

#[link(name = "wininet")]
extern "system" {
    fn InternetGetConnectedState(flags: *mut u32, reserved: u32) -> i32;
}

/// 32-bit `MEMORY_BASIC_INFORMATION`. No `PartitionId` member: that field is
/// `#if defined(_WIN64)` only, and this DLL is PE32/i386 by construction.
#[repr(C)]
#[derive(Clone, Copy)]
struct MemoryBasicInformation {
    base_address: *mut c_void,
    allocation_base: *mut c_void,
    allocation_protect: u32,
    region_size: usize,
    state: u32,
    protect: u32,
    memory_type: u32,
}

const _: () = assert!(core::mem::size_of::<MemoryBasicInformation>() == 28);

#[link(name = "kernel32")]
extern "system" {
    fn VirtualQuery(
        address: *const c_void,
        buffer: *mut MemoryBasicInformation,
        length: usize,
    ) -> usize;
}

const MEM_COMMIT: u32 = 0x0000_1000;
const PAGE_GUARD: u32 = 0x0000_0100;
const PAGE_READABLE: u32 = 0x02 /* READONLY */
    | 0x04 /* READWRITE */
    | 0x08 /* WRITECOPY */
    | 0x20 /* EXECUTE_READ */
    | 0x40 /* EXECUTE_READWRITE */
    | 0x80 /* EXECUTE_WRITECOPY */;

/// Is `[address, address + len)` committed and readable *right now*?
///
/// Every retail address this shim dereferences outside its own image goes
/// through here first. `Game` is a heap object whose lifetime we do not own: it
/// does not exist at menu time, and a read through a stale pointer inside the
/// game's own process is a crash we would have caused. `VirtualQuery` is the
/// bounded, non-destructive way to ask.
unsafe fn readable(address: usize, len: usize) -> bool {
    if address == 0 || len == 0 {
        return false;
    }
    let Some(end) = address.checked_add(len) else {
        return false;
    };
    let mut info = MemoryBasicInformation {
        base_address: core::ptr::null_mut(),
        allocation_base: core::ptr::null_mut(),
        allocation_protect: 0,
        region_size: 0,
        state: 0,
        protect: 0,
        memory_type: 0,
    };
    let written = VirtualQuery(
        address as *const c_void,
        &mut info,
        core::mem::size_of::<MemoryBasicInformation>(),
    );
    if written != core::mem::size_of::<MemoryBasicInformation>() {
        return false;
    }
    let region_base = info.base_address as usize;
    let Some(region_end) = region_base.checked_add(info.region_size) else {
        return false;
    };
    info.state == MEM_COMMIT
        && info.protect & PAGE_GUARD == 0
        && info.protect & PAGE_READABLE != 0
        // One VirtualQuery describes one region; a range spilling past it may
        // continue into an uncommitted page, so refuse instead of walking.
        && region_base <= address
        && end <= region_end
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
    /// A transport bind failure must not turn the factory result into NULL:
    /// retail dispatches two loader virtuals even after its null diagnostic.
    /// An absent session is therefore a callable, fail-closed NetSys object.
    session: Option<Session<TcpTransport>>,
    role: Role,
    retail_objects_constructed: bool,
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
    init_count: u32,
    active: bool,
    /// Friend Game calls NetSys::host before its direct OnPlayerJoined export.
    /// Keep the local player pending until that later DTO supplies the retail
    /// identity; the callback must never expose our temporary transport id.
    defer_local_add_until_identity: bool,
    local_member_id: Vec<u16>,
    setup_bridge: bool,
    bridged_slots: BTreeMap<i32, i32>,
    /// Loaded image base, set only when this process is the pinned retail build
    /// *and* the `CommandPackage::send` key instructions are byte-identical to
    /// the ones the two `Game` offsets were read from. `None` disables the live
    /// key read entirely.
    game_key_base: Option<usize>,
    /// Fallback key for a host that is not the pinned retail executable, so
    /// there is no live `Game` to read (`DON_NET_GAME_KEY`). Never overrides a
    /// live read: an invented key would be exactly the guess this path exists to
    /// replace.
    configured_game_key: Option<u32>,
    /// Which match key each peer has already been told, so a key crosses the
    /// wire once per peer per match and a late joiner still gets one.
    game_key_announced: BTreeMap<i32, u32>,
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
    // Retail's Create/Join lobby callbacks directly copy-assign into the
    // concrete CrossplayNetLibSys::m_lobby at +0xD0; this is not reached
    // through our vtable or exports.  It must therefore be a live MSVC
    // LobbyDTO rather than zero padding.  The exact rise.pdb layout is 0xD0
    // bytes, followed by the shipped ip_addresses field at +0x1A0.
    m_lobby: [u8; 0xd0],
    ip_addresses: MsvcObjectArrayString,
    // Keep the scalar portion of the concrete retail object visible at its
    // exact offsets. Nontrivial FIFO/Log/Array/std::function members in the
    // remaining image are still bypassed by every shim thunk.
    ip_override: MsvcGameString,
    concrete_host_port: u32,
    concrete_local_port: u32,
    lobby_launched: u8,
    reserved_to_timeout: [u8; 0x97],
    concrete_timeout: u32,
    concrete_game_version: u32,
    concrete_matchmaking_id: i32,
    reserved_to_num_allowed: [u8; 0x44],
    concrete_num_allowed: i32,
    reserved_to_callbacks: [u8; 0x98],
    data_channel_opened_callback: MsvcFunction40,
    data_channel_closed_callback: MsvcFunction40,
    connection_failed_callback: MsvcFunction40,
    /// Boxed so the C-visible prefix stays exactly `NetSysBase`.
    state: *mut State,
}

const _: () = {
    assert!(core::mem::offset_of!(NetSysObj, flags) == 0x58);
    assert!(core::mem::offset_of!(NetSysObj, net_messenger) == 0x5c);
    assert!(core::mem::offset_of!(NetSysObj, m_crossplay) == 0xcc);
    assert!(core::mem::offset_of!(NetSysObj, m_lobby) == 0xd0);
    assert!(core::mem::offset_of!(NetSysObj, ip_addresses) == 0x1a0);
    assert!(core::mem::offset_of!(NetSysObj, ip_override) == 0x1b8);
    assert!(core::mem::offset_of!(NetSysObj, concrete_host_port) == 0x1cc);
    assert!(core::mem::offset_of!(NetSysObj, concrete_local_port) == 0x1d0);
    assert!(core::mem::offset_of!(NetSysObj, concrete_timeout) == 0x26c);
    assert!(core::mem::offset_of!(NetSysObj, concrete_game_version) == 0x270);
    assert!(core::mem::offset_of!(NetSysObj, concrete_matchmaking_id) == 0x274);
    assert!(core::mem::offset_of!(NetSysObj, concrete_num_allowed) == 0x2bc);
    assert!(core::mem::offset_of!(NetSysObj, data_channel_opened_callback) == 0x358);
    assert!(core::mem::offset_of!(NetSysObj, data_channel_closed_callback) == 0x380);
    assert!(core::mem::offset_of!(NetSysObj, connection_failed_callback) == 0x3a8);
    assert!(core::mem::offset_of!(NetSysObj, state) == 0x3d0);
};

// `Crossplay::Lobby::DTO::LobbyDTO::LobbyDTO` in the pinned retail executable.
// Calling retail's constructor keeps its std::wstring/unordered_map/json
// allocator representation exact.  The object is process-lifetime just like
// shipped CrossplayNetLibSys, so the matching destructor is never needed.
const LOBBY_DTO_CTOR_RVA: usize = 0x0004_b1f0;
type LobbyDtoCtor = unsafe extern "thiscall" fn(*mut c_void) -> *mut c_void;

// `ObjectArray<String>::ObjectArray` in the pinned retail executable. It
// installs the executable's vtable and exact empty `{0,0,-1,null,0}` state.
const OBJECT_ARRAY_STRING_CTOR_RVA: usize = 0x0003_9e80;
type ObjectArrayStringCtor =
    unsafe extern "thiscall" fn(*mut MsvcObjectArrayString) -> *mut MsvcObjectArrayString;

// `public: static class Game &GameAccess::game`, preferred VA 0x00C061EC (PDB
// public symbol; `ron-bin/sbl/rise.pdb`). MSVC stores a reference as a pointer,
// so the dword at this address is the live `Game*`.
const GAME_ACCESS_GAME_RVA: usize = 0x0080_61ec;
// `Game::info` at offset 12, type `GameInfo`; `GameInfo::seed` at offset 4.
// Both [measured] from the shipped PDB type records (`schema/pdb-types.json`).
const GAME_INFO_SEED_OFFSET: usize = 12 + 4;
// The exact extent this shim dereferences through the `Game*`: everything up to
// and including `info.seed`. Nothing else in the 3184-byte object is touched.
const GAME_SEED_READ_EXTENT: usize = GAME_INFO_SEED_OFFSET + 4;

// The instruction stream that proves the two offsets above are the ones retail
// itself uses, at `CommandPackage::send` `0x0094c1e0` + 0x132:
//
//   a1 ec 61 c0 00   mov  eax, dword ptr [0xc061ec]   ; GameAccess::game
//   83 c1 12         add  ecx, 0x12
//   0f bf fa         movsx edi, dx
//   8b 40 10         mov  eax, dword ptr [eax + 0x10] ; Game::info.seed
//   c1 e8 08         shr  eax, 8
//   0f b7 d8         movzx ebx, ax                    ; XOR key
//
// Pinning it makes the key path share the executable-identity discipline the
// two constructor RVAs already have: if this is not the code at that address,
// the offsets are not this build's and nothing is read. Byte 1 is the absolute
// operand the loader relocates.
const COMMAND_PACKAGE_KEY_RVA: usize = 0x0054_c312;
const COMMAND_PACKAGE_KEY_PREFIX: &[u8] = &[
    0xa1, 0xec, 0x61, 0xc0, 0x00, 0x83, 0xc1, 0x12, 0x0f, 0xbf, 0xfa, 0x8b, 0x40, 0x10, 0xc1, 0xe8,
    0x08, 0x0f, 0xb7, 0xd8,
];
const COMMAND_PACKAGE_KEY_RELOCS: &[usize] = &[1];

const RETAIL_PE_TIMESTAMP: u32 = 0x6674_863f;
const RETAIL_IMAGE_BASE: u32 = 0x0040_0000;
const RETAIL_IMAGE_SIZE: usize = 0x00bb_4000;
// Prefixes are recorded at the PREFERRED base. A loaded image is rebased, and any dword
// the relocation table covers is rewritten by the loader, so those offsets must be
// compared after subtracting the delta rather than byte-for-byte. Getting this wrong is
// silent: the gate simply reports a non-retail executable and every hard-coded RVA —
// including the two constructors below — is skipped.
//
// `LOBBY_DTO_CTOR_PREFIX+5` is `push imm32` (`0x00a5fe2a`), which is relocated. Measured
// live at module base 0x00190000: the operand read 0x007efe2a, exactly the pinned value
// plus the -0x270000 delta. See docs/assembly/netsys-retail-identity-relocation.md.
const LOBBY_DTO_CTOR_PREFIX: &[u8] = &[
    0x55, 0x8b, 0xec, 0x6a, 0xff, 0x68, 0x2a, 0xfe, 0xa5, 0x00, 0x64, 0xa1, 0x00, 0x00, 0x00, 0x00,
];
const LOBBY_DTO_CTOR_RELOCS: &[usize] = &[6];
// No absolute operand: every byte is position independent.
const OBJECT_ARRAY_STRING_CTOR_PREFIX: &[u8] = &[
    0x56, 0x8b, 0xf1, 0x83, 0xc8, 0xff, 0x66, 0x89, 0x46, 0x0c, 0xc7, 0x46, 0x04, 0x00, 0x00, 0x00,
];
const OBJECT_ARRAY_STRING_CTOR_RELOCS: &[usize] = &[];

/// Compare a loaded code prefix against its preferred-base recording.
///
/// Bytes outside `relocs` must match exactly. Each entry in `relocs` names the offset of a
/// loader-rewritten dword; it matches when `loaded == pinned.wrapping_add(delta)`. Rebasing
/// therefore stays invisible while a *different* callee at the same RVA still fails, which
/// masking the operand out would not catch.
fn prefix_matches_relocated(loaded: &[u8], pinned: &[u8], relocs: &[usize], delta: u32) -> bool {
    if loaded.len() != pinned.len() {
        return false;
    }
    for (offset, (actual, expected)) in loaded.iter().zip(pinned).enumerate() {
        if relocs
            .iter()
            .any(|start| offset >= *start && offset < start + 4)
        {
            continue;
        }
        if actual != expected {
            return false;
        }
    }
    relocs.iter().all(|start| {
        match (read_u32(loaded, *start), read_u32(pinned, *start)) {
            (Some(actual), Some(expected)) => actual == expected.wrapping_add(delta),
            _ => false,
        }
    })
}

fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

/// Validate the immutable PE identity fields before any hard-coded executable
/// RVA is invoked. The filename alone is not a build identity.
///
/// `loaded_base` is the address the image is actually mapped at, or `RETAIL_IMAGE_BASE` when
/// checking an on-disk copy.
///
/// The loader rewrites `OptionalHeader.ImageBase` in the mapped image to the address it chose:
/// measured live at `0x00190000` on an executable whose file says `0x00400000`. So that field
/// **stops carrying identity once the image is rebased** — there is no way to recover the
/// preferred base from a rebased header, and demanding the pinned value simply fails. Treat it
/// as a mapping sanity check there, and let identity rest on the fields the loader does not
/// touch (machine, timestamp, magic, size-of-image) plus the two code prefixes.
fn pinned_retail_pe_headers(headers: &[u8], loaded_base: u32) -> bool {
    if read_u16(headers, 0) != Some(0x5a4d) {
        return false;
    }
    let Some(pe) = read_u32(headers, 0x3c).map(|value| value as usize) else {
        return false;
    };
    let image_base = read_u32(headers, pe + 24 + 28);
    pe <= 0x800
        && headers.get(pe..pe + 4) == Some(b"PE\0\0")
        && read_u16(headers, pe + 4) == Some(0x014c)
        && read_u32(headers, pe + 8) == Some(RETAIL_PE_TIMESTAMP)
        && read_u16(headers, pe + 24) == Some(0x010b)
        && (image_base == Some(RETAIL_IMAGE_BASE) || image_base == Some(loaded_base))
        && read_u32(headers, pe + 24 + 56) == Some(RETAIL_IMAGE_SIZE as u32)
}

fn is_retail_executable_path(path: &[u16]) -> bool {
    let leaf = path
        .iter()
        .rposition(|unit| *unit == b'/' as u16 || *unit == b'\\' as u16)
        .map_or(path, |separator| &path[separator + 1..]);
    let expected = b"riseofnations.exe";
    leaf.len() == expected.len()
        && leaf.iter().zip(expected).all(|(actual, expected)| {
            *actual <= 0x7f && (*actual as u8).eq_ignore_ascii_case(expected)
        })
}

unsafe fn retail_executable_base() -> Option<*mut u8> {
    let base = GetModuleHandleW(core::ptr::null()).cast::<u8>();
    if base.is_null() {
        return None;
    }
    let mut path = [0u16; 1024];
    let length = GetModuleFileNameW(base.cast(), path.as_mut_ptr(), path.len() as u32);
    if length == 0
        || length as usize >= path.len()
        || !is_retail_executable_path(&path[..length as usize])
    {
        return None;
    }
    let headers = core::slice::from_raw_parts(base, 0x1000);
    let loaded_base = base as usize as u32;
    if !pinned_retail_pe_headers(headers, loaded_base) {
        return None;
    }
    // ASLR is enabled on the supported executable, so compare the code prefixes against the
    // actual load delta rather than the preferred base they were recorded at.
    let delta = loaded_base.wrapping_sub(RETAIL_IMAGE_BASE);
    if !prefix_matches_relocated(
        core::slice::from_raw_parts(base.add(LOBBY_DTO_CTOR_RVA), LOBBY_DTO_CTOR_PREFIX.len()),
        LOBBY_DTO_CTOR_PREFIX,
        LOBBY_DTO_CTOR_RELOCS,
        delta,
    ) || !prefix_matches_relocated(
        core::slice::from_raw_parts(
            base.add(OBJECT_ARRAY_STRING_CTOR_RVA),
            OBJECT_ARRAY_STRING_CTOR_PREFIX.len(),
        ),
        OBJECT_ARRAY_STRING_CTOR_PREFIX,
        OBJECT_ARRAY_STRING_CTOR_RELOCS,
        delta,
    ) {
        return None;
    }
    Some(base)
}

/// The two-step pointer chase, without any pointers: `slot` is the dword stored
/// at `GameAccess::game`, `read_dword` returns the dword at an address when it
/// is committed and readable.
///
/// Separated out so the refusals below are unit-testable on a machine that has
/// no `Game` at all. Everything that can be decided without dereferencing retail
/// memory is decided here.
fn game_key_from_slot(slot: u32, read_dword: impl Fn(usize) -> Option<u32>) -> Option<u32> {
    // Retail's `Game` is allocated by the CRT; a null slot means no game exists
    // yet, and a misaligned one is not an object this build ever produced.
    if slot == 0 || slot % 4 != 0 {
        return None;
    }
    let game = slot as usize;
    read_dword(game.checked_add(GAME_INFO_SEED_OFFSET)?)
}

/// Decide, once, whether this process is a build whose `Game` layout the two
/// pinned offsets describe.
///
/// The image identity cannot change at runtime, so this is settled at load and
/// the per-package path below only reads. Requiring the instruction stream at
/// `CommandPackage::send + 0x132` to match — after relocation adjustment — means
/// the offsets are never applied to a build that does not contain the code they
/// were read from.
unsafe fn game_key_read_base(retail_exe: Option<*mut u8>) -> Option<*mut u8> {
    let base = retail_exe?;
    let delta = (base as usize as u32).wrapping_sub(RETAIL_IMAGE_BASE);
    if !prefix_matches_relocated(
        core::slice::from_raw_parts(
            base.add(COMMAND_PACKAGE_KEY_RVA),
            COMMAND_PACKAGE_KEY_PREFIX.len(),
        ),
        COMMAND_PACKAGE_KEY_PREFIX,
        COMMAND_PACKAGE_KEY_RELOCS,
        delta,
    ) {
        trace_detail(format_args!(
            "game_key=unavailable reason=command-package-send-prefix-mismatch rva=0x{COMMAND_PACKAGE_KEY_RVA:x}"
        ));
        return None;
    }
    Some(base)
}

/// Read the multiplayer command-package key out of the live retail process.
///
/// This is `Game::info.seed`, the same word `CommandPackage::send` `0x0094c1e0`
/// reads six instructions before it calls our send slot. Returning `None` is
/// always allowed and always means "do not claim to know the key": no `Game`
/// yet, or memory that `VirtualQuery` will not vouch for.
///
/// The only caller invokes this from inside a `NetSys` send slot, i.e. while
/// retail is *itself* partway through `CommandPackage::send` having already
/// dereferenced this exact pointer at this exact offset on this exact thread. A
/// pointer that were wild would have faulted in retail's own code first; the
/// `VirtualQuery` gate below is for the paths where no game is running at all.
///
/// **Tier C.** The offsets are the shipped PDB's and the instruction stream at
/// the pinned RVA is compared byte-for-byte against this build's, but that a
/// decoded live turn confirms the value is a separate claim this function does
/// not make.
unsafe fn read_live_game_seed(base: *mut u8) -> Option<u32> {
    // The global lives inside the mapped image, so it is readable by
    // construction; the `Game*` it holds does not.
    let slot = *(base.add(GAME_ACCESS_GAME_RVA).cast::<u32>());
    game_key_from_slot(slot, |address| {
        readable(slot as usize, GAME_SEED_READ_EXTENT).then(|| *(address as *const u32))
    })
}

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

pub unsafe fn role_is_host(this: *mut NetSysBase) -> bool {
    st(this).is_some_and(|s| s.role == Role::Host)
}

fn env(k: &str) -> Option<String> {
    std::env::var(k).ok()
}

/// Decimal or `0x`-prefixed hexadecimal, matching `tools/owned-peer`'s
/// `--game-key`.
fn parse_u32(raw: &str) -> Option<u32> {
    let raw = raw.trim();
    match raw.strip_prefix("0x").or_else(|| raw.strip_prefix("0X")) {
        Some(digits) => u32::from_str_radix(digits, 16).ok(),
        None => raw.parse::<u32>().ok(),
    }
}

fn build_transport(id: i32) -> std::io::Result<(TcpTransport, Role)> {
    if load_only_mode() {
        if env_truthy("DON_NET_LOAD_ONLY_FORCE_TRANSPORT_FAILURE") {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AddrInUse,
                "forced load-only transport failure",
            ));
        }
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
    let load_only = load_only_mode();
    let retail_exe = unsafe { retail_executable_base() };
    let configured_role = if load_only || env("DON_NET_ROLE").as_deref() != Some("join") {
        Role::Host
    } else {
        Role::Client
    };
    let built = if load_only || retail_exe.is_some() {
        build_transport(id).ok()
    } else {
        None
    };
    let (session, role, local_addr) = match built {
        Some((transport, role)) => {
            let local_addr = transport
                .local_addr()
                .map(|addr| addr.to_string())
                .unwrap_or_else(|_| "unavailable".into());
            (Some(Session::new(transport, role, name)), role, local_addr)
        }
        None => {
            let reason = if retail_exe.is_none() && !load_only {
                "retail-executable-identity"
            } else {
                "transport"
            };
            trace_detail(format_args!("factory=inert reason={reason}"));
            (None, configured_role, "unavailable".into())
        }
    };
    // Merely loading the replacement is not a connection. The lifecycle
    // entry point makes this true after `init` retained retail's callbacks.
    CONNECTED.store(false, Ordering::Relaxed);
    if session.is_some() {
        trace_detail(format_args!(
            "factory=ready abi=netsys-v65 role={role:?} load_only={load_only} local_addr={local_addr} transport_ready=true retail_identity={}",
            retail_exe.is_some()
        ));
    } else {
        trace_detail(format_args!(
            "factory=callable-inert abi=netsys-v65 role={role:?} load_only={load_only} transport_ready=false retail_identity={}",
            retail_exe.is_some()
        ));
    }
    let state = Box::new(State {
        session,
        role,
        retail_objects_constructed: retail_exe.is_some(),
        inbox: VecDeque::new(),
        players: Vec::new(),
        started: Instant::now(),
        time_out_ms: 20_000,
        allow_timeout: 1,
        playing: 0,
        joining: 0,
        accept_host_messages: 1,
        num_allowed_players: 8,
        host_port: 0x88ab,
        local_port: 0x88ab,
        matchmaking_id: 0,
        error_callback: None,
        load_only,
        game_version: 0,
        init_count: 0,
        active: false,
        defer_local_add_until_identity: false,
        local_member_id: Vec::new(),
        setup_bridge: env_truthy("DON_NET_SETUP_BRIDGE"),
        bridged_slots: BTreeMap::new(),
        game_key_base: unsafe { game_key_read_base(retail_exe) }.map(|base| base as usize),
        configured_game_key: env("DON_NET_GAME_KEY").as_deref().and_then(parse_u32),
        game_key_announced: BTreeMap::new(),
    });
    let mut obj = Box::new(NetSysObj {
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
        m_lobby: [0; 0xd0],
        ip_addresses: MsvcObjectArrayString::empty_unconstructed(),
        ip_override: empty_game_string(),
        concrete_host_port: 0x88ab,
        concrete_local_port: 0x88ab,
        lobby_launched: 0,
        reserved_to_timeout: [0; 0x97],
        concrete_timeout: 20_000,
        concrete_game_version: 0,
        concrete_matchmaking_id: 0,
        reserved_to_num_allowed: [0; 0x44],
        concrete_num_allowed: 8,
        reserved_to_callbacks: [0; 0x98],
        data_channel_opened_callback: MsvcFunction40::empty(),
        data_channel_closed_callback: MsvcFunction40::empty(),
        connection_failed_callback: MsvcFunction40::empty(),
        state: Box::into_raw(state),
    });
    if let Some(exe) = retail_exe {
        let ctor: LobbyDtoCtor = unsafe { core::mem::transmute(exe.add(LOBBY_DTO_CTOR_RVA)) };
        let ip_ctor: ObjectArrayStringCtor =
            unsafe { core::mem::transmute(exe.add(OBJECT_ARRAY_STRING_CTOR_RVA)) };
        unsafe {
            ctor(obj.m_lobby.as_mut_ptr().cast());
            ip_ctor(&mut obj.ip_addresses);
        }
        trace_detail(format_args!(
            "factory=lobby-dto-constructed offset=0xd0 size=0xd0 ctor_rva=0x4b1f0 ip_array_offset=0x1a0 ip_array_ctor_rva=0x39e80"
        ));
    } else if !load_only {
        trace_detail(format_args!(
            "factory=inert reason=retail-executable-identity loader_virtuals=callable"
        ));
    } else {
        trace_detail(format_args!(
            "factory=lobby-dto-skipped reason=non-retail-load-only-executable"
        ));
    }
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
    (*(*obj).state).session.as_mut().map(f)
}

type MsvcFunctionCopy = unsafe extern "thiscall" fn(*mut c_void, *mut c_void) -> *mut c_void;
type MsvcFunctionMove = unsafe extern "thiscall" fn(*mut c_void, *mut c_void) -> *mut c_void;
type MsvcFunctionDelete = unsafe extern "thiscall" fn(*mut c_void, bool);

unsafe fn msvc_function_vtable(function: &MsvcFunction40) -> Option<*const *const c_void> {
    if function.target.is_null() {
        return None;
    }
    let vtable = *function.target.cast::<*const *const c_void>();
    (!vtable.is_null()).then_some(vtable)
}

pub unsafe fn destroy_msvc_function(function: &mut MsvcFunction40) {
    let Some(vtable) = msvc_function_vtable(function) else {
        *function = MsvcFunction40::empty();
        return;
    };
    let target = function.target;
    let object_base = function as *mut MsvcFunction40 as *mut c_void;
    let destructor: MsvcFunctionDelete = core::mem::transmute(*vtable.add(4));
    destructor(target, target != object_base);
    *function = MsvcFunction40::empty();
}

unsafe fn clone_msvc_function_into_empty(
    destination: &mut MsvcFunction40,
    source: &MsvcFunction40,
) -> bool {
    debug_assert!(destination.target.is_null());
    let Some(vtable) = msvc_function_vtable(source) else {
        return true;
    };
    let copy: MsvcFunctionCopy = core::mem::transmute(*vtable);
    let destination_base = destination as *mut MsvcFunction40 as *mut c_void;
    let target = copy(source.target, destination_base);
    destination.target = target;
    !target.is_null()
}

/// Clone-assign an x86 MSVC `std::function` without retaining the caller's
/// by-value object. Small targets must be reconstructed in the destination's
/// inline buffer; large targets remain independently heap-owned.
unsafe fn replace_msvc_function(destination: &mut MsvcFunction40, source: &MsvcFunction40) -> bool {
    let mut replacement = MsvcFunction40::empty();
    if !clone_msvc_function_into_empty(&mut replacement, source) {
        return false;
    }
    destroy_msvc_function(destination);
    if replacement.target.is_null() {
        return true;
    }
    let replacement_base = &mut replacement as *mut MsvcFunction40 as *mut c_void;
    if replacement.target == replacement_base {
        let Some(vtable) = msvc_function_vtable(&replacement) else {
            return false;
        };
        let move_target: MsvcFunctionMove = core::mem::transmute(*vtable.add(1));
        let destination_base = destination as *mut MsvcFunction40 as *mut c_void;
        destination.target = move_target(replacement.target, destination_base);
        destroy_msvc_function(&mut replacement);
        !destination.target.is_null()
    } else {
        core::ptr::copy_nonoverlapping(
            &replacement as *const MsvcFunction40,
            destination as *mut MsvcFunction40,
            1,
        );
        true
    }
}

pub unsafe fn retain_p2p_callbacks(
    this: *mut NetSysBase,
    opened: &MsvcFunction40,
    closed: &MsvcFunction40,
    failed: &MsvcFunction40,
) -> bool {
    let object = this.cast::<NetSysObj>();
    if object.is_null() {
        return false;
    }
    let opened_ok = replace_msvc_function(&mut (*object).data_channel_opened_callback, opened);
    let closed_ok = replace_msvc_function(&mut (*object).data_channel_closed_callback, closed);
    let failed_ok = replace_msvc_function(&mut (*object).connection_failed_callback, failed);
    trace_detail(format_args!(
        "p2p_callbacks=retained opened={opened_ok} closed={closed_ok} failed={failed_ok}"
    ));
    opened_ok && closed_ok && failed_ok
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

/// Shipped `host` asks NetMessenger slot 11 for session data and copies its
/// first two bytes into the concrete CrossplayNetLibSys field at +0xB0 before
/// returning. Preserve that direct-access field for retail's later setup path.
unsafe fn retain_host_session_data(this: *mut NetSysBase) {
    let Some(messenger) = messenger(this) else {
        return;
    };
    let vtable = (*messenger).vftable;
    if vtable.is_null() {
        return;
    }
    let data = ((*vtable).get_session_data)(messenger);
    if data.is_null() {
        return;
    }
    let object = this as *mut NetSysObj;
    // reserved_to_crossplay begins at +0x60; +0xB0 is byte index 0x50.
    core::ptr::copy_nonoverlapping(
        data.cast::<u8>(),
        (*object).reserved_to_crossplay.as_mut_ptr().add(0x50),
        2,
    );
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
    if state.load_only || !state.setup_bridge || state.role != Role::Host || state.session.is_none()
    {
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

type BridgeAddPlayer = unsafe extern "thiscall" fn(*mut c_void, MsvcWstring);
type BridgeGetPlayerIndex = unsafe extern "thiscall" fn(*mut c_void, MsvcWstring) -> i32;
type BridgeSendPlayer = unsafe extern "thiscall" fn(*mut c_void, i32, bool);

#[derive(Clone, Copy)]
struct BridgeCalls {
    connection_data: *mut c_void,
    add_player: BridgeAddPlayer,
    get_player_index: BridgeGetPlayerIndex,
    send_player: BridgeSendPlayer,
}

unsafe fn production_bridge_calls(this: *mut NetSysBase) -> Option<BridgeCalls> {
    let (base, connection_data) = bridge_context(this)?;
    Some(BridgeCalls {
        connection_data,
        add_player: core::mem::transmute(base.add(CONNECTION_ADD_PLAYER_RVA)),
        get_player_index: core::mem::transmute(base.add(CONNECTION_GET_PLAYER_INDEX_RVA)),
        send_player: core::mem::transmute(base.add(CONNECTION_SEND_PLAYER_RVA)),
    })
}

unsafe fn bridge_add_with(calls: BridgeCalls, player: *const NetPlayerObj) -> Option<i32> {
    let units = &(&(*player).id_wide)[..(*player).id_len as usize];
    let add_id = owned_wstring(units)?;
    let find_id = owned_wstring(units)?;
    (calls.add_player)(calls.connection_data, add_id);
    let slot = (calls.get_player_index)(calls.connection_data, find_id);
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

enum BridgeAddResult {
    Disabled,
    Deferred,
    Added(i32),
}

unsafe fn bridge_add_player(this: *mut NetSysBase, player: *const NetPlayerObj) -> BridgeAddResult {
    if player.is_null() || (*player).flags & SNLPLAYER_LOCAL != 0 {
        return BridgeAddResult::Disabled;
    }
    let enabled = st(this).is_some_and(|state| {
        !state.load_only
            && state.setup_bridge
            && state.role == Role::Host
            && state.session.is_some()
    });
    if !enabled {
        return BridgeAddResult::Disabled;
    }
    let Some(calls) = production_bridge_calls(this) else {
        return BridgeAddResult::Deferred;
    };
    bridge_add_with(calls, player)
        .map(BridgeAddResult::Added)
        .unwrap_or(BridgeAddResult::Deferred)
}

unsafe fn bridge_ready_with(calls: BridgeCalls, slot: i32, ready: bool) {
    // PlayerConnectionData is exactly 59 bytes; ready is byte +58.
    *calls
        .connection_data
        .cast::<u8>()
        .add(slot as usize * 59 + 58) = u8::from(ready);
    (calls.send_player)(calls.connection_data, slot, true);
}

unsafe fn bridge_ready(this: *mut NetSysBase, unique_id: i32, ready: bool) -> bool {
    let slot = {
        let Some(state) = st(this) else { return false };
        let Some(&slot) = state.bridged_slots.get(&unique_id) else {
            return false;
        };
        slot
    };
    let Some(calls) = production_bridge_calls(this) else {
        return false;
    };
    bridge_ready_with(calls, slot, ready);
    trace_detail(format_args!(
        "setup_bridge=ok action=ready slot={slot} ready={ready} credential_material=none"
    ));
    true
}

/// Refresh the `NetPlayer*` array the game can read directly, and drain new
/// game-layer packets into our own inbox.
unsafe fn pump(this: *mut NetSysBase) {
    let (removed, pending_ptrs, next_local, next_host, ready_changes) = {
        let Some(s) = st(this) else { return };
        if !s.active {
            return;
        }
        let t = now_ms(s);
        let Some(session) = s.session.as_mut() else {
            return;
        };
        let _ = session.poll(t, Duration::from_millis(0));
        let mut ready_changes = Vec::new();
        for e in session.drain_events() {
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
        let roster = session.players().to_vec();
        let mut old = core::mem::take(&mut s.players);
        let mut next = Vec::with_capacity(roster.len());
        for player in &roster {
            let mut object = if let Some(index) = old
                .iter()
                .position(|candidate| candidate.unique_id == player.unique_id)
            {
                old.remove(index)
            } else {
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
        // A later reconnect may reuse the same owned transport id. Remove its
        // old SetupWin slot mapping before the deletion callback, otherwise a
        // same-poll READYFLAG can target stale ConnectionData.
        for player in &old {
            s.bridged_slots.remove(&player.unique_id);
        }
        // Additions must see coherent direct pointers. For removal, shipped
        // leaves the old host/local pointer intact until the delete callback
        // returns, even though players[]/num_players were already compacted.
        if !local_removed {
            (*obj).base.local_player = next_local;
        }
        if !host_removed {
            (*obj).base.host_player = next_host;
        }
        // A setup bridge can legitimately be unavailable on the first pump:
        // Friend Game constructs SetupWin after host returns. Preserve PENDING
        // and retry on later pumps instead of losing the only add transition.
        let pending_ptrs = s
            .players
            .iter()
            .filter(|player| player.flags & SNLPLAYER_PENDING != 0)
            .map(|player| player.as_ref() as *const NetPlayerObj)
            .collect::<Vec<_>>();
        (old, pending_ptrs, next_local, next_host, ready_changes)
    };

    // The prefix mutation and Rust borrows are complete before retail sees a
    // callback. Removed boxes stay alive through on_player_deleted.
    for player in &removed {
        notify_player_deleted(this, player.as_ref());
    }
    let object = this as *mut NetSysObj;
    (*object).base.local_player = next_local;
    (*object).base.host_player = next_host;
    let mut readiness_replayed = BTreeSet::new();
    for player in pending_ptrs {
        let defer_local = (*player).flags & SNLPLAYER_LOCAL != 0
            && st(this).is_some_and(|state| state.defer_local_add_until_identity);
        if defer_local {
            trace_detail(format_args!(
                "callback=NetMessenger.on_player_added deferred=retail-identity"
            ));
            continue;
        }
        match bridge_add_player(this, player) {
            BridgeAddResult::Deferred => continue,
            BridgeAddResult::Disabled => notify_player_added(this, player),
            BridgeAddResult::Added(slot) => {
                if let Some(state) = st(this) {
                    state.bridged_slots.insert((*player).unique_id, slot);
                }
                // ConnectionData::add_player synchronously reaches SetupWin's
                // player-added callback. Match process_playerlist by clearing
                // PENDING immediately after that call returns.
                (*(player as *mut NetPlayerObj)).flags &= !SNLPLAYER_PENDING;
                // A remote READYFLAG may arrive in the same poll as roster
                // materialisation. Replay current truth once the slot exists.
                if (*player).ready {
                    let _ = bridge_ready(this, (*player).unique_id, true);
                    readiness_replayed.insert((*player).unique_id);
                }
                continue;
            }
        }
        // Exact shipped ordering: process_playerlist clears pending only after
        // NetMessenger::on_player_added returns (0x1001AADE..0x1001AAF4).
        (*(player as *mut NetPlayerObj)).flags &= !SNLPLAYER_PENDING;
    }
    for (unique_id, ready) in ready_changes {
        if !readiness_replayed.contains(&unique_id) {
            let _ = bridge_ready(this, unique_id, ready);
        }
    }
}

/// Enter the common Friend Game host lifecycle. Retail and the native smoke
/// share this exact implementation so the acceptance is mutation-sensitive
/// to the local pointer/pending/callback ordering used in `ns_host`.
unsafe fn activate_host_lifecycle(this: *mut NetSysBase, connected: bool) -> bool {
    let Some(state) = st(this) else { return false };
    if state.role != Role::Host || state.session.is_none() {
        return false;
    }
    state.active = true;
    state.defer_local_add_until_identity = true;
    state.joining = 0;
    let object = this as *mut NetSysObj;
    (*object).flags |= 0x11;
    retain_host_session_data(this);
    if connected {
        CONNECTED.store(true, Ordering::Relaxed);
    }
    pump(this);
    true
}

/// Activate only the loopback/ephemeral diagnostic session so the external
/// PE32 smoke can dispatch every NetPlayer slot. This never enables traffic
/// and refuses normal retail mode.
pub unsafe fn materialize_load_only_peer(this: *mut NetSysBase) -> bool {
    if !st(this).is_some_and(|state| state.load_only) {
        return false;
    }
    activate_host_lifecycle(this, false)
}

/// Drive the production bridge call/cleanup/readiness sequence against inert
/// callbacks supplied by the PE32 smoke. This is load-only-only and never
/// resolves retail RVAs, so it cannot mutate a game process.
pub unsafe fn test_setup_bridge_sequence(
    this: *mut NetSysBase,
    connection_data: *mut c_void,
    add_player: *const c_void,
    get_player_index: *const c_void,
    send_player: *const c_void,
) -> bool {
    if !st(this).is_some_and(|state| state.load_only)
        || connection_data.is_null()
        || add_player.is_null()
        || get_player_index.is_null()
        || send_player.is_null()
    {
        return false;
    }
    let calls = BridgeCalls {
        connection_data,
        add_player: core::mem::transmute(add_player),
        get_player_index: core::mem::transmute(get_player_index),
        send_player: core::mem::transmute(send_player),
    };
    let remote = don_net::session::Player {
        unique_id: 2,
        name: "Ai".into(),
        is_host: false,
        is_local: false,
        ready: true,
        last_pulse_ms: 0,
        slot: 1,
    };
    let mut object = build_player(&remote);
    let Some(slot) = bridge_add_with(calls, object.as_ref()) else {
        return false;
    };
    if slot != 1 || object.flags & SNLPLAYER_PENDING == 0 {
        return false;
    }
    object.flags &= !SNLPLAYER_PENDING;
    bridge_ready_with(calls, slot, true);
    object.flags & SNLPLAYER_PENDING == 0
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
    let (player, ok, code_units) = {
        let Some(state) = st(this) else { return false };
        state.local_member_id = copy;
        let code_units = state.local_member_id.len();
        let Some(player) = state
            .players
            .iter_mut()
            .find(|player| player.flags & SNLPLAYER_LOCAL != 0)
        else {
            trace_detail(format_args!(
                "local_member_id=captured code_units={code_units} player_updated=false"
            ));
            return true;
        };
        let ok = set_player_id(player, &state.local_member_id);
        let ptr = player.as_mut() as *mut NetPlayerObj;
        state.defer_local_add_until_identity = false;
        (ptr, ok, code_units)
    };
    if ok {
        // Exact process_playerlist lifecycle: callback observes PENDING and the
        // final authenticated ID; only its return clears the pending bit.
        if (*player).flags & SNLPLAYER_PENDING != 0 {
            notify_player_added(this, player);
            (*player).flags &= !SNLPLAYER_PENDING;
        }
        trace_detail(format_args!(
            "local_member_id=captured code_units={} player_updated={ok}",
            code_units
        ));
        ok
    } else {
        trace_detail(format_args!(
            "local_member_id=captured code_units={code_units} player_updated=false"
        ));
        false
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

unsafe fn reset_attempt_state(this: *mut NetSysBase) {
    let object = this as *mut NetSysObj;
    if object.is_null() {
        return;
    }
    if let Some(state) = st(this) {
        state.active = false;
        state.joining = 0;
        state.playing = 0;
        state.accept_host_messages = 1;
        state.inbox.clear();
        state.players.clear();
        state.bridged_slots.clear();
        // A key belongs to one match. `init` opens a new attempt epoch, so the
        // next match must re-announce rather than inherit.
        state.game_key_announced.clear();
        state.local_member_id.clear();
        state.defer_local_add_until_identity = false;
        state.started = Instant::now();
        state.time_out_ms = 20_000;
        state.allow_timeout = 1;
        state.num_allowed_players = 8;
        state.host_port = 0x88ab;
        state.local_port = 0x88ab;
        state.game_version = 0;
        state.matchmaking_id = 0;
        let role = state.role;
        if let Some(session) = state.session.as_mut() {
            session.reset_for_reuse(role);
            session.set_timeout_ms(20_000);
        }
    }
    (*object).base.num_players = 0;
    (*object).base.players = [core::ptr::null_mut(); 8];
    (*object).base.local_player = core::ptr::null_mut();
    (*object).base.host_player = core::ptr::null_mut();
    (*object).base.local_player_connection = LOCAL_PLAYER_CONNECTED;
    (*object).base.local_player_disconnect_pct = 0.0;
    (*object).net_messenger = core::ptr::null_mut();
    (*object).m_crossplay = core::ptr::null_mut();
    (*object).flags = 0x40;
    (*object).concrete_host_port = 0x88ab;
    (*object).concrete_local_port = 0x88ab;
    (*object).concrete_timeout = 20_000;
    (*object).concrete_game_version = 0;
    (*object).concrete_matchmaking_id = 0;
    (*object).concrete_num_allowed = 8;
    CONNECTED.store(false, Ordering::Relaxed);
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
    // Shipped init starts with close. Make re-entry a new attempt epoch rather
    // than leaving a hidden don-net roster behind the cleared retail prefix.
    reset_attempt_state(this);
    (*object).net_messenger = messenger.cast();
    (*object).m_crossplay = crossplay_service.cast_mut();
    let mut init_count = 0;
    if let Some(state) = st(this) {
        // Shipped init stores its first integer argument at the concrete
        // CrossplayNetLibSys game-version field (+0x270).
        state.game_version = game_version as u32;
        state.init_count = state.init_count.saturating_add(1);
        init_count = state.init_count;
    }
    (*object).concrete_game_version = game_version as u32;
    if init_count == 1 {
        trace_detail(format_args!(
            "init=stored messenger={} crossplay_service={} object_size=0x3d4",
            !messenger.is_null(),
            !crossplay_service.is_null()
        ));
    } else {
        trace_detail(format_args!(
            "init=reinitialized count={init_count} messenger={} crossplay_service={}",
            !messenger.is_null(),
            !crossplay_service.is_null()
        ));
    }
    // Returning success authorises retail's immediate `[netsys+0xCC]` service
    // vcall. Only do so when both retained pointers are concrete; load-only is
    // still safe because every session/traffic entry point remains refused.
    let operational = st(this).is_some_and(|state| {
        state.session.is_some() && (state.load_only || state.retail_objects_constructed)
    });
    if messenger.is_null() || crossplay_service.is_null() || !operational {
        return LIBERR_NOT_AVAILABLE;
    }
    LIBERR_OK
}

unsafe extern "thiscall" fn ns_close(this: *mut NetSysBase) {
    trace_once("vtable.ns_close");
    reset_attempt_state(this);
}

nop!(ns_get_memory_manager() -> *mut c_void = core::ptr::null_mut());

unsafe extern "thiscall" fn ns_is_host(this: *mut NetSysBase) -> bool {
    trace_once("vtable.ns_is_host");
    let object = this as *mut NetSysObj;
    !object.is_null() && (*object).flags & 1 != 0
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
    st(this)
        .map(|s| if s.playing != 0 { 0x20 } else { 0 })
        .unwrap_or(0)
}

unsafe extern "thiscall" fn ns_is_joining_in_process(this: *mut NetSysBase) -> i32 {
    trace_once("vtable.ns_is_joining_in_process");
    st(this).map(|s| i32::from(s.joining != 0)).unwrap_or(0)
}

unsafe extern "thiscall" fn ns_is_session_full(this: *mut NetSysBase) -> i32 {
    trace_once("vtable.ns_is_session_full");
    let object = this as *mut NetSysObj;
    st(this)
        .map(|s| i32::from((*object).base.num_players == s.num_allowed_players))
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
    let object = this.cast::<NetSysObj>();
    if !object.is_null() {
        (*object).concrete_num_allowed = v;
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
        if let Some(session) = s.session.as_mut() {
            session.set_timeout_ms(v as u64);
        }
    }
    let object = this.cast::<NetSysObj>();
    if !object.is_null() {
        (*object).concrete_timeout = v;
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
    st(this)
        .map(|s| if s.allow_timeout != 0 { 0x40 } else { 0 })
        .unwrap_or(0x40)
}

unsafe extern "thiscall" fn ns_get_num_allowed_players(this: *mut NetSysBase) -> i32 {
    trace_once("vtable.ns_get_num_allowed_players");
    st(this).map(|s| s.num_allowed_players).unwrap_or(8)
}

/// Hand every peer this match's key just before the package that needs it.
///
/// Called from the two `NetSys` slots `CommandPackage::send` `0x0094c1e0`
/// dispatches to (`+0x54` send, `+0x58` send_all). That is the only instant at
/// which `Game::info.seed` is known to be the value retail itself just used:
/// the read is six instructions upstream, on this thread, in this call.
///
/// The announcement goes out **before** the package it explains, and the
/// transport preserves order per peer, so a joining peer never has to decode a
/// package it has not yet been given the key for. A key crosses the wire once
/// per peer per match.
///
/// This is a DoN transport extension. Retail never sends, parses, or sees one —
/// the receiving `Session` consumes it exactly like the shipped internal range.
unsafe fn announce_game_key_for_package(this: *mut NetSysBase, packet: &[u8]) {
    if !matches!(
        don_net::msg::NetMsg::decode(packet).map(|framed| framed.msg),
        Ok(don_net::msg::NetMsg::CommandPackage { .. })
    ) {
        return;
    }
    let Some(state) = st(this) else { return };
    if state.load_only {
        return;
    }
    let peers = match state.session.as_ref() {
        Some(session) => session.transport.peers(),
        None => return,
    };
    if peers.is_empty() {
        return;
    }
    // Read every time rather than once: retail can start a second match on the
    // same session, and a stale key would make every package of it undecodable.
    // The image identity behind `game_key_base` is settled at load, so this is
    // one `VirtualQuery` and two loads.
    let live = state
        .game_key_base
        .and_then(|base| read_live_game_seed(base as *mut u8));
    let (seed, source) = match live {
        Some(seed) => (seed, GameKeySource::RetailGameInfoSeed),
        // Never a substitute for a live read; only for a host that is not the
        // pinned retail executable and therefore has no `Game` at all.
        None => match state.configured_game_key {
            Some(seed) => (seed, GameKeySource::Configured),
            None => return,
        },
    };
    for peer in peers {
        if state.game_key_announced.get(&peer) == Some(&seed) {
            continue;
        }
        let sent = state
            .session
            .as_mut()
            .is_some_and(|session| session.announce_game_key_to(peer, seed, source).is_ok());
        if sent {
            state.game_key_announced.insert(peer, seed);
            trace_detail(format_args!(
                "game_key=announced peer={peer} source={} seed=0x{seed:08x} xor_key=0x{:04x}",
                source.as_str(),
                (seed >> 8) as u16,
            ));
        }
    }
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
        || to.is_null()
        || size <= 0
        || size as usize > NETDAEMON_RECEIVE_EXTENT
        || st(this).is_some_and(|s| s.load_only)
    {
        return false;
    }
    let bytes = core::slice::from_raw_parts(packet, size as usize);
    announce_game_key_for_package(this, bytes);
    let dest = Dest::One((*to).unique_id);
    st(this)
        .and_then(|s| s.session.as_mut())
        .map(|session| session.transport.send(dest, bytes).is_ok())
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
    announce_game_key_for_package(this, bytes);
    st(this)
        .and_then(|s| s.session.as_mut())
        .map(|session| session.transport.send(Dest::All, bytes).is_ok())
        .unwrap_or(false)
}

unsafe extern "thiscall" fn ns_get(
    this: *mut NetSysBase,
    packet: *mut u8,
    from: *mut *const NetPlayerObj,
    size: *mut u32,
) -> bool {
    trace_once("vtable.ns_get");
    if packet.is_null() || from.is_null() || size.is_null() || st(this).is_some_and(|s| s.load_only)
    {
        return false;
    }
    pump(this);
    let Some(s) = st(this) else { return false };
    while let Some((sender, bytes)) = s.inbox.pop_front() {
        if bytes.len() > NETDAEMON_RECEIVE_EXTENT {
            continue;
        }
        let Some(sender_ptr) = s
            .players
            .iter_mut()
            .find(|player| player.unique_id == sender)
            .map(|player| player.as_mut() as *const NetPlayerObj)
        else {
            trace_detail(format_args!(
                "receive=dropped reason=sender-not-live unique_id={sender}"
            ));
            continue;
        };
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), packet, bytes.len());
        *size = bytes.len() as u32;
        *from = sender_ptr;
        return true;
    }
    false
}

unsafe extern "thiscall" fn ns_poll_services(
    _this: *mut NetSysBase,
    _services: *mut c_void,
) -> i32 {
    trace_once("vtable.ns_poll_services");
    LIBERR_NOT_AVAILABLE
}

unsafe extern "thiscall" fn ns_host(
    this: *mut NetSysBase,
    a: *const c_void,
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
    // Shipped host reads the first BHG String's length at +8 and returns
    // failure before touching session state when it is empty.
    let first_arg_nonempty = !a.is_null() && *a.cast::<u8>().add(8).cast::<u16>() != 0;
    if state.load_only
        || state.role != Role::Host
        || state.session.is_none()
        || !ready
        || !first_arg_nonempty
    {
        return LIBERR_NOT_AVAILABLE;
    }
    if activate_host_lifecycle(this, true) {
        LIBERR_OK
    } else {
        LIBERR_NOT_AVAILABLE
    }
}

unsafe extern "thiscall" fn ns_join(
    this: *mut NetSysBase,
    session_arg: *const c_void,
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
        if state.load_only
            || state.role != Role::Client
            || state.session.is_none()
            || !ready
            || session_arg.is_null()
        {
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
        core::ptr::NonNull::<u8>::dangling().as_ptr().cast(),
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
// Shipped CrossplayNetLib has no list-backed implementation for these polling
// APIs and returns LIBERR_NOT_AVAILABLE. Returning success with untouched
// output storage lets retail consume an object we never populated.
nop!(ns_poll_sessions(*const c_void) -> i32 = LIBERR_NOT_AVAILABLE);
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
    let _ = this;
    LIBERR_NOT_AVAILABLE
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
        .map(|s| {
            i32::from(
                s.players
                    .iter()
                    .any(|candidate| core::ptr::eq(candidate.as_ref(), p)),
            )
        })
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

unsafe extern "thiscall" fn ns_get_ip_addresses(
    this: *mut NetSysBase,
) -> *const MsvcObjectArrayString {
    trace_once("vtable.ns_get_ip_addresses");
    let object = this.cast::<NetSysObj>();
    if object.is_null() {
        core::ptr::null()
    } else {
        core::ptr::addr_of!((*object).ip_addresses)
    }
}

unsafe extern "thiscall" fn ns_get_host_port(this: *mut NetSysBase) -> u32 {
    trace_once("vtable.ns_get_host_port");
    st(this).map(|s| s.host_port).unwrap_or(0)
}
unsafe extern "thiscall" fn ns_set_host_port(this: *mut NetSysBase, v: u32) {
    trace_once("vtable.ns_set_host_port");
    if let Some(s) = st(this) {
        s.host_port = v;
    }
    let object = this.cast::<NetSysObj>();
    if !object.is_null() {
        (*object).concrete_host_port = v;
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
    let object = this.cast::<NetSysObj>();
    if !object.is_null() {
        (*object).concrete_local_port = v;
    }
}

nop!(ns_set_ip_override(*const c_void));

unsafe extern "thiscall" fn ns_set_matchmaking_id(this: *mut NetSysBase, v: i32) {
    trace_once("vtable.ns_set_matchmaking_id");
    if let Some(s) = st(this) {
        s.matchmaking_id = v;
    }
    let object = this.cast::<NetSysObj>();
    if !object.is_null() {
        (*object).concrete_matchmaking_id = v;
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

    #[test]
    fn lobby_constructor_is_gated_by_the_exact_retail_executable_leaf() {
        let retail: Vec<u16> = r"C:\Program Files (x86)\Steam\riseofnations.exe"
            .encode_utf16()
            .collect();
        let upper: Vec<u16> = r"C:\Games\RISEOFNATIONS.EXE".encode_utf16().collect();
        let smoke: Vec<u16> = r"C:\tmp\netsys-load-smoke.exe".encode_utf16().collect();
        assert!(is_retail_executable_path(&retail));
        assert!(is_retail_executable_path(&upper));
        assert!(!is_retail_executable_path(&smoke));
    }

    /// Captured from the supported executable running under ASLR on 2026-08-10: module base
    /// `0x00190000`, so the `push imm32` operand read `0x007efe2a` where the preferred-base
    /// recording holds `0x00a5fe2a`. Comparing these byte-for-byte is what made the live gate
    /// report a non-retail executable and skip both constructors.
    const LIVE_LOBBY_DTO_CTOR_PREFIX_AT_190000: &[u8] = &[
        0x55, 0x8b, 0xec, 0x6a, 0xff, 0x68, 0x2a, 0xfe, 0x7e, 0x00, 0x64, 0xa1, 0x00, 0x00, 0x00,
        0x00,
    ];
    const LIVE_MODULE_BASE: u32 = 0x0019_0000;

    #[test]
    fn a_rebased_lobby_constructor_prefix_still_identifies_retail() {
        let delta = LIVE_MODULE_BASE.wrapping_sub(RETAIL_IMAGE_BASE);

        // The defect this replaces: the measured live bytes are not the pinned bytes.
        assert_ne!(LIVE_LOBBY_DTO_CTOR_PREFIX_AT_190000, LOBBY_DTO_CTOR_PREFIX);

        assert!(prefix_matches_relocated(
            LIVE_LOBBY_DTO_CTOR_PREFIX_AT_190000,
            LOBBY_DTO_CTOR_PREFIX,
            LOBBY_DTO_CTOR_RELOCS,
            delta,
        ));

        // Still exact at the preferred base, where the delta is zero.
        assert!(prefix_matches_relocated(
            LOBBY_DTO_CTOR_PREFIX,
            LOBBY_DTO_CTOR_PREFIX,
            LOBBY_DTO_CTOR_RELOCS,
            0,
        ));
    }

    #[test]
    fn relocation_tolerance_does_not_blind_the_gate() {
        let delta = LIVE_MODULE_BASE.wrapping_sub(RETAIL_IMAGE_BASE);

        // A different callee at the same RVA must still fail, even in the relocated dword.
        // Masking the operand out instead of adjusting it would accept this.
        let mut wrong_target = LIVE_LOBBY_DTO_CTOR_PREFIX_AT_190000.to_vec();
        wrong_target[6] ^= 0x01;
        assert!(!prefix_matches_relocated(
            &wrong_target,
            LOBBY_DTO_CTOR_PREFIX,
            LOBBY_DTO_CTOR_RELOCS,
            delta,
        ));

        // A non-relocated opcode byte must still fail.
        let mut wrong_opcode = LIVE_LOBBY_DTO_CTOR_PREFIX_AT_190000.to_vec();
        wrong_opcode[0] = 0x90;
        assert!(!prefix_matches_relocated(
            &wrong_opcode,
            LOBBY_DTO_CTOR_PREFIX,
            LOBBY_DTO_CTOR_RELOCS,
            delta,
        ));

        // The wrong delta must fail: rebasing is tolerated, arbitrary operands are not.
        assert!(!prefix_matches_relocated(
            LIVE_LOBBY_DTO_CTOR_PREFIX_AT_190000,
            LOBBY_DTO_CTOR_PREFIX,
            LOBBY_DTO_CTOR_RELOCS,
            delta.wrapping_add(0x1000),
        ));

        // The relocation-free prefix keeps byte-exact semantics under any delta.
        let mut wrong_free = OBJECT_ARRAY_STRING_CTOR_PREFIX.to_vec();
        wrong_free[15] ^= 0x01;
        assert!(!prefix_matches_relocated(
            &wrong_free,
            OBJECT_ARRAY_STRING_CTOR_PREFIX,
            OBJECT_ARRAY_STRING_CTOR_RELOCS,
            delta,
        ));
        assert!(prefix_matches_relocated(
            OBJECT_ARRAY_STRING_CTOR_PREFIX,
            OBJECT_ARRAY_STRING_CTOR_PREFIX,
            OBJECT_ARRAY_STRING_CTOR_RELOCS,
            delta,
        ));
    }

    #[test]
    fn constructor_rvas_are_gated_by_the_pinned_pe_identity() {
        let mut headers = vec![0u8; 0x400];
        headers[0..2].copy_from_slice(&0x5a4du16.to_le_bytes());
        headers[0x3c..0x40].copy_from_slice(&0x138u32.to_le_bytes());
        headers[0x138..0x13c].copy_from_slice(b"PE\0\0");
        headers[0x13c..0x13e].copy_from_slice(&0x014cu16.to_le_bytes());
        headers[0x140..0x144].copy_from_slice(&RETAIL_PE_TIMESTAMP.to_le_bytes());
        let optional = 0x138 + 24;
        headers[optional..optional + 2].copy_from_slice(&0x010bu16.to_le_bytes());
        headers[optional + 28..optional + 32].copy_from_slice(&RETAIL_IMAGE_BASE.to_le_bytes());
        headers[optional + 56..optional + 60]
            .copy_from_slice(&(RETAIL_IMAGE_SIZE as u32).to_le_bytes());
        assert!(pinned_retail_pe_headers(&headers, RETAIL_IMAGE_BASE));

        for corrupt in [0usize, 0x13c, 0x140, optional, optional + 28, optional + 56] {
            let mut wrong = headers.clone();
            wrong[corrupt] ^= 0xff;
            assert!(
                !pinned_retail_pe_headers(&wrong, RETAIL_IMAGE_BASE),
                "accepted corruption at {corrupt:#x}"
            );
        }
        assert!(!pinned_retail_pe_headers(&headers[..0x150], RETAIL_IMAGE_BASE));

        // A mapped image carries the base the loader chose, measured live at 0x00190000 on
        // this executable. Demanding the preferred value there is what made the gate refuse
        // the real game.
        let mut rebased = headers.clone();
        rebased[optional + 28..optional + 32]
            .copy_from_slice(&LIVE_MODULE_BASE.to_le_bytes());
        assert!(!pinned_retail_pe_headers(&rebased, RETAIL_IMAGE_BASE));
        assert!(pinned_retail_pe_headers(&rebased, LIVE_MODULE_BASE));

        // It must still reject a header claiming some third base, so the relaxation is
        // "the loader rewrote it", not "any value goes".
        let mut elsewhere = headers.clone();
        elsewhere[optional + 28..optional + 32].copy_from_slice(&0x0666_0000u32.to_le_bytes());
        assert!(!pinned_retail_pe_headers(&elsewhere, LIVE_MODULE_BASE));
    }

    #[test]
    fn concrete_scalars_and_callback_storage_match_crossplaynetsys() {
        assert_eq!(core::mem::offset_of!(NetSysObj, concrete_host_port), 0x1cc);
        assert_eq!(core::mem::offset_of!(NetSysObj, concrete_local_port), 0x1d0);
        assert_eq!(core::mem::offset_of!(NetSysObj, concrete_timeout), 0x26c);
        assert_eq!(
            core::mem::offset_of!(NetSysObj, concrete_num_allowed),
            0x2bc
        );
        assert_eq!(
            core::mem::offset_of!(NetSysObj, data_channel_opened_callback),
            0x358
        );
        assert_eq!(
            core::mem::offset_of!(NetSysObj, data_channel_closed_callback),
            0x380
        );
        assert_eq!(
            core::mem::offset_of!(NetSysObj, connection_failed_callback),
            0x3a8
        );
        assert_eq!(core::mem::offset_of!(NetSysObj, state), 0x3d0);
    }

    #[test]
    fn the_pinned_key_instructions_are_the_source_of_the_two_key_offsets() {
        // `a1 <imm32>` is `mov eax, moffs32`: the operand is the address of
        // `GameAccess::game`, and it must be the same global this file names by
        // RVA rather than a second, independently guessed constant.
        assert_eq!(COMMAND_PACKAGE_KEY_PREFIX[0], 0xa1);
        assert_eq!(
            read_u32(COMMAND_PACKAGE_KEY_PREFIX, 1),
            Some(RETAIL_IMAGE_BASE + GAME_ACCESS_GAME_RVA as u32)
        );
        assert_eq!(COMMAND_PACKAGE_KEY_RELOCS, &[1]);
        // `8b 40 10` is `mov eax, [eax + 0x10]`: the displacement is
        // `Game::info` (+12) plus `GameInfo::seed` (+4).
        assert_eq!(&COMMAND_PACKAGE_KEY_PREFIX[11..13], &[0x8b, 0x40]);
        assert_eq!(
            usize::from(COMMAND_PACKAGE_KEY_PREFIX[13]),
            GAME_INFO_SEED_OFFSET
        );
        assert_eq!(GAME_INFO_SEED_OFFSET, 0x10);
        // `c1 e8 08` / `0f b7 d8` is `shr eax, 8` / `movzx ebx, ax`, i.e. the
        // `(seed >> 8) as u16` that `Obfuscation::xor_key` implements.
        assert_eq!(&COMMAND_PACKAGE_KEY_PREFIX[14..], &[0xc1, 0xe8, 0x08, 0x0f, 0xb7, 0xd8]);
        assert_eq!(GAME_SEED_READ_EXTENT, 0x14);
        // Every pinned RVA lies inside the pinned image.
        assert!(COMMAND_PACKAGE_KEY_RVA + COMMAND_PACKAGE_KEY_PREFIX.len() < RETAIL_IMAGE_SIZE);
        assert!(GAME_ACCESS_GAME_RVA + 4 < RETAIL_IMAGE_SIZE);
    }

    #[test]
    fn the_key_prefix_survives_relocation_and_still_rejects_a_different_callee() {
        let delta = 0xfd90_0000u32; // the measured live -0x270000
        let mut relocated = COMMAND_PACKAGE_KEY_PREFIX.to_vec();
        let operand = read_u32(COMMAND_PACKAGE_KEY_PREFIX, 1).unwrap();
        relocated[1..5].copy_from_slice(&operand.wrapping_add(delta).to_le_bytes());
        assert!(prefix_matches_relocated(
            &relocated,
            COMMAND_PACKAGE_KEY_PREFIX,
            COMMAND_PACKAGE_KEY_RELOCS,
            delta
        ));
        // Unrelocated bytes at the same address are not this build.
        assert!(!prefix_matches_relocated(
            COMMAND_PACKAGE_KEY_PREFIX,
            COMMAND_PACKAGE_KEY_PREFIX,
            COMMAND_PACKAGE_KEY_RELOCS,
            delta
        ));
        // A different displacement is a different field: the whole point of
        // adjusting the relocated dword rather than masking it out.
        let mut different_offset = relocated.clone();
        different_offset[13] = 0x14;
        assert!(!prefix_matches_relocated(
            &different_offset,
            COMMAND_PACKAGE_KEY_PREFIX,
            COMMAND_PACKAGE_KEY_RELOCS,
            delta
        ));
        let mut wrong_global = relocated.clone();
        wrong_global[1..5].copy_from_slice(&operand.wrapping_add(delta).wrapping_add(4).to_le_bytes());
        assert!(!prefix_matches_relocated(
            &wrong_global,
            COMMAND_PACKAGE_KEY_PREFIX,
            COMMAND_PACKAGE_KEY_RELOCS,
            delta
        ));
    }

    #[test]
    fn the_game_pointer_chase_refuses_everything_it_cannot_justify() {
        // No `Game` yet: the menu case, and the reason this is not read once at
        // load time.
        assert_eq!(game_key_from_slot(0, |_| Some(0xdead_beef)), None);
        // Retail objects are 4-aligned; an unaligned slot is not one.
        assert_eq!(game_key_from_slot(0x0040_0002, |_| Some(1)), None);
        // Unreadable memory is refused, not dereferenced.
        assert_eq!(game_key_from_slot(0x0040_0000, |_| None), None);
        // The read lands exactly at `Game + 0x10` and the whole 32-bit word is
        // returned; the `>> 8` belongs to `Obfuscation::xor_key`.
        let game = 0x0abc_1000u32;
        assert_eq!(
            game_key_from_slot(game, |address| {
                assert_eq!(address, game as usize + 0x10);
                Some(0x1234_5678)
            }),
            Some(0x1234_5678)
        );
        // A seed of zero is a real key, not a failure to read one.
        assert_eq!(game_key_from_slot(game, |_| Some(0)), Some(0));
    }

    #[test]
    fn the_configured_key_override_parses_both_spellings_and_nothing_else() {
        assert_eq!(parse_u32("0x005ac33d"), Some(0x005a_c33d));
        assert_eq!(parse_u32("0X005AC33D"), Some(0x005a_c33d));
        assert_eq!(parse_u32("5942589"), Some(5_942_589));
        assert_eq!(parse_u32(""), None);
        assert_eq!(parse_u32("0x"), None);
        assert_eq!(parse_u32("nope"), None);
        assert_eq!(parse_u32("0x1_0000_0000"), None);
    }

    #[test]
    fn our_own_image_is_readable_and_a_null_or_absurd_range_is_not() {
        let anchor = &RETAIL_PE_TIMESTAMP as *const u32 as usize;
        assert!(unsafe { readable(anchor, 4) });
        assert!(!unsafe { readable(0, 4) });
        assert!(!unsafe { readable(anchor, 0) });
        assert!(!unsafe { readable(usize::MAX, 4) });
        assert!(!unsafe { readable(usize::MAX - 1, 4) });
    }
}
