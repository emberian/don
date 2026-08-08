//! A fabricated world just large enough to run `FUN_00644130` (the damage pipeline).
//!
//! # Why this file exists
//!
//! `FUN_00644130` is not an ISLAND. It is `__thiscall` with six stack dwords and it
//! reaches, before it returns, into: two `Object`s and their two `UnitType`s, four
//! vtables, the `RULES` singleton, the game object, the per-player array, the map and its
//! tile records, two parallel object-pointer tables, a city table, and a global aux
//! object reached through `[0x00E85DDC]`. Handing it a buffer of random bytes segfaults
//! immediately.
//!
//! So we build the world instead. Every pointer the function follows lands in an arena we
//! own; every virtual call lands on a stub we generated whose return value we set per
//! trial. That turns ~30 object-graph predicates into *inputs*, which is exactly the shape
//! `don_sim::DamagePredicates` has — and it is what makes the arithmetic chain
//! differentially testable at volume.
//!
//! # What this does NOT establish
//!
//! The stubs mean we are testing the **arithmetic**, not the engine's resolution of the
//! predicates. A stub that returns 1 tells you nothing about when the real
//! `Object::vtbl[0x18]` returns 1. The harness also cannot reach four branches at all;
//! they are listed in `docs/derivation/damage-port.md` and are marked UNVERIFIED in
//! `don_sim`. Do not read whole-function coverage into a green run here.
//!
//! # Layout notes that were paid for in faults
//!
//! * The two object tables `[0x00C0AB84]` and `[0x00C0AEC0]` are *both* indexed
//!   `base[player*28] -> ptr_array; ptr_array[index]` and both must resolve to the same
//!   object; retail may or may not keep them equal, and we assume it does.
//! * `cmp edx, 0xB42174` at `0x00644453` and `cmp eax, 0x653790` at `0x006444AC` are
//!   **relocated** immediates. The fabricated objects must carry the *relocated*
//!   addresses, not the preferred-base ones.
//! * Setting `game[+0x20] |= 4` makes `FUN_006E1370` return false unconditionally
//!   (`0x006E1370` tests it first). That is what stops `get_attack`/`get_armor` from
//!   wandering into their upgrade paths, and it is also why steps 11 and 27 are
//!   unreachable here.

use std::ffi::c_void;

pub const ARENA_LEN: usize = 1 << 21;
const PAGE: usize = 4096;

// ---- arena offsets -------------------------------------------------------------------
pub const O_ATK_OBJ: usize = 0x0_0000;
pub const O_DEF_OBJ: usize = 0x0_0200;
pub const O_ATK_TYPE: usize = 0x0_0400;
pub const O_DEF_TYPE: usize = 0x0_0800;
pub const O_BUILD: usize = 0x0_0C00;
pub const O_MOUNT_TYPE: usize = 0x0_0D00;
pub const O_CARRIER: usize = 0x0_0E00;
pub const O_VT_ATK: usize = 0x0_1000;
pub const O_VT_DEF: usize = 0x0_1400;
pub const O_VT_ATKT: usize = 0x0_1800;
pub const O_VT_DEFT: usize = 0x0_1C00;
pub const O_VT_CARR: usize = 0x0_2000;
pub const O_VT_AUX: usize = 0x0_2400;
pub const O_VT_MOUNTLESS: usize = 0x0_2800;
pub const O_RULES: usize = 0x0_3000;
pub const O_GAME: usize = 0x0_5000;
pub const O_MAP: usize = 0x0_6000;
pub const O_TILES: usize = 0x0_6200;
pub const O_COORD: usize = 0x0_6400;
pub const O_PTRARR0: usize = 0x0_6800;
pub const O_PTRARR1: usize = 0x0_6900;
pub const O_C061D4: usize = 0x0_6A00;
pub const O_CITYARR: usize = 0x0_6C00;
pub const O_CITY: usize = 0x0_6D00;
pub const O_AUXROOT: usize = 0x0_7000;
pub const O_AUXOBJ: usize = 0x0_8000;
pub const O_PRED: usize = 0x0_8200; // 96 control dwords
pub const O_TECH_ATK: usize = 0x0_8400; // 256 dwords
pub const O_TECH_DEF: usize = 0x0_8800;
pub const O_OUTKIND: usize = 0x0_8C00;
pub const O_PLAYERS: usize = 0x1_0000; // 2 * 0x6EEC
pub const O_PLAYERAUX: usize = 0x1_E000;

pub const PLAYER_STRIDE: usize = 0x6EEC;

// ---- control-block slot indices (each is one stubbed virtual) ------------------------
pub const C_ATK_18: usize = 0;
pub const C_ATK_1C: usize = 1;
pub const C_ATK_20: usize = 2;
pub const C_ATK_130: usize = 3;
pub const C_ATK_E4: usize = 4;
pub const C_ATK_D0: usize = 5;
pub const C_ATK_CC: usize = 6;
pub const C_ATK_C8: usize = 7;
pub const C_ATKT_EC: usize = 8;
pub const C_ATKT_10C: usize = 9;
pub const C_DEF_18: usize = 10;
pub const C_DEF_1C: usize = 11;
pub const C_DEF_20: usize = 12;
pub const C_DEF_120: usize = 13;
pub const C_DEF_CC: usize = 14;
pub const C_DEF_D0: usize = 15;
pub const C_DEF_D8: usize = 16;
pub const C_DEF_C8: usize = 17;
pub const C_DEF_3C: usize = 18; // returns the build object pointer
pub const C_DEF_40: usize = 19; // returns the carrier object pointer
pub const C_DEFT_EC: usize = 20;
pub const C_DEFT_10C: usize = 21;
pub const C_CARR_184: usize = 22;
pub const C_CARR_20: usize = 23;
pub const C_AUX_14: usize = 24;
pub const C_ATK_3C: usize = 25; // must return a real object: FUN_0062DD90 dereferences it
pub const C_SLOTS: usize = 32;

// ---- retail addresses (preferred base) ----------------------------------------------
pub const VA_DAMAGE: u32 = 0x0064_4130;
pub const VA_BUILD_VFTABLE: u32 = 0x00B4_2174;
pub const VA_TECH_THUNK: u32 = 0x0065_3790;
pub const VA_AUX_MARKER: u32 = 0x0047_07E0;
pub const VA_BALANCE: u32 = 0x00C0_6AFC;

pub const G_C061D0: u32 = 0x00C0_61D0;
pub const G_C061D4: u32 = 0x00C0_61D4;
pub const G_C061E0: u32 = 0x00C0_61E0;
pub const G_C061E4: u32 = 0x00C0_61E4;
pub const G_C061E8: u32 = 0x00C0_61E8;
pub const G_C0AB84: u32 = 0x00C0_AB84;
pub const G_C0AEC0: u32 = 0x00C0_AEC0;
pub const G_CAE5FC: u32 = 0x00CA_E5FC;
pub const G_E85DDC: u32 = 0x00E8_5DDC;

/// Ranges inside `.data` that the harness owns. A balance-table write that lands in one
/// of these would silently dismantle the fabricated world, so trials that would collide
/// are skipped rather than debugged later.
pub const RESERVED: [(u32, u32); 5] = [
    (0x00C0_61C0, 0x00C0_61F8),
    (0x00C0_AAF0, 0x00C0_AEE0),
    (0x00CA_E5F8, 0x00CA_E604),
    (0x00E8_5DD8, 0x00E8_5DE4),
    (0x00C0_6AF8, 0x00C0_6B00),
];

pub struct Arena {
    pub base: *mut u8,
    pub code: *mut u8,
    pub code_len: usize,
}

impl Arena {
    pub fn new() -> Result<Arena, String> {
        let base = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                ARENA_LEN,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            )
        };
        if base == libc::MAP_FAILED {
            return Err("arena mmap failed".into());
        }
        let code = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                PAGE,
                libc::PROT_READ | libc::PROT_WRITE | libc::PROT_EXEC,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            )
        };
        if code == libc::MAP_FAILED {
            return Err("stub page mmap failed (PROT_EXEC refused?)".into());
        }
        Ok(Arena {
            base: base as *mut u8,
            code: code as *mut u8,
            code_len: 0,
        })
    }

    #[inline]
    pub fn at(&self, off: usize) -> *mut u8 {
        unsafe { self.base.add(off) }
    }
    #[inline]
    pub fn addr(&self, off: usize) -> u32 {
        self.at(off) as u32
    }
    #[inline]
    pub fn w32(&self, off: usize, v: u32) {
        unsafe { std::ptr::write_unaligned(self.at(off) as *mut u32, v) };
    }
    #[inline]
    pub fn w16(&self, off: usize, v: u16) {
        unsafe { std::ptr::write_unaligned(self.at(off) as *mut u16, v) };
    }
    #[inline]
    pub fn w8(&self, off: usize, v: u8) {
        unsafe { std::ptr::write(self.at(off), v) };
    }
    #[inline]
    pub fn r32(&self, off: usize) -> u32 {
        unsafe { std::ptr::read_unaligned(self.at(off) as *const u32) }
    }

    /// `mov eax, [abs32]; ret` — a virtual whose return value we set by writing a dword.
    fn emit_load_ret(&mut self, abs: u32, argbytes: u16) -> u32 {
        let p = unsafe { self.code.add(self.code_len) };
        let mut b: Vec<u8> = vec![0xA1];
        b.extend_from_slice(&abs.to_le_bytes());
        if argbytes == 0 {
            b.push(0xC3);
        } else {
            b.push(0xC2);
            b.extend_from_slice(&argbytes.to_le_bytes());
        }
        unsafe { std::ptr::copy_nonoverlapping(b.as_ptr(), p, b.len()) };
        self.code_len += b.len();
        p as u32
    }

    /// `xor eax,eax; ret [argbytes]` — the default for every vtable slot we never expect
    /// to be called. If one *is* called we get 0 rather than a jump into random bytes.
    fn emit_zero_ret(&mut self, argbytes: u16) -> u32 {
        let p = unsafe { self.code.add(self.code_len) };
        let mut b: Vec<u8> = vec![0x31, 0xC0];
        if argbytes == 0 {
            b.push(0xC3);
        } else {
            b.push(0xC2);
            b.extend_from_slice(&argbytes.to_le_bytes());
        }
        unsafe { std::ptr::copy_nonoverlapping(b.as_ptr(), p, b.len()) };
        self.code_len += b.len();
        p as u32
    }

    /// The tech query stub for `UnitType::vtbl[0x60]`.
    ///
    /// One slot serves several different questions — the attacker's slot is asked about
    /// techs `0x42`, `0x139` and `0x83` — so a constant-returning stub could not tell them
    /// apart. This one reads the pushed tech id and indexes a 256-entry table:
    ///
    /// ```text
    /// mov eax, [esp+4] ; and eax, 0xFF ; mov eax, [tab + eax*4] ; ret 8
    /// ```
    ///
    /// The six tech ids the pipeline asks about (`0x42 0x139 0x83 0x216 0x143 0x109`) have
    /// distinct low bytes (`42 39 83 16 43 09`), so the truncation is lossless here.
    fn emit_tech_stub(&mut self, tab: u32) -> u32 {
        let p = unsafe { self.code.add(self.code_len) };
        let mut b: Vec<u8> = vec![0x8B, 0x44, 0x24, 0x04, 0x25, 0xFF, 0x00, 0x00, 0x00, 0x8B, 0x04, 0x85];
        b.extend_from_slice(&tab.to_le_bytes());
        b.extend_from_slice(&[0xC2, 0x08, 0x00]);
        unsafe { std::ptr::copy_nonoverlapping(b.as_ptr(), p, b.len()) };
        self.code_len += b.len();
        p as u32
    }
}

impl Drop for Arena {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.base as *mut c_void, ARENA_LEN);
            libc::munmap(self.code as *mut c_void, PAGE);
        }
    }
}

/// Stub addresses, kept so vtable slots can be (re)pointed after construction.
pub struct Stubs {
    pub ctrl: [u32; C_SLOTS],
    pub zero0: u32,
    pub zero8: u32,
    pub tech_atk: u32,
    pub tech_def: u32,
}

/// Build the whole fabricated world. Returns the stub table; the caller then writes
/// per-trial values into the control block, the rules block and the two objects.
///
/// `reloc` maps a preferred-base VA to its address in this process's mapping.
pub fn build(a: &mut Arena, reloc: &dyn Fn(u32) -> u32) -> Stubs {
    // ---- stubs ----------------------------------------------------------------------
    let zero0 = a.emit_zero_ret(0);
    let zero8 = a.emit_zero_ret(8);
    let mut ctrl = [0u32; C_SLOTS];
    for (i, slot) in ctrl.iter_mut().enumerate() {
        let cell = a.addr(O_PRED + i * 4);
        *slot = a.emit_load_ret(cell, 0);
    }
    let tech_atk = a.emit_tech_stub(a.addr(O_TECH_ATK));
    let tech_def = a.emit_tech_stub(a.addr(O_TECH_DEF));

    // ---- vtables: default every slot to "return 0" -----------------------------------
    for vt in [O_VT_ATK, O_VT_DEF, O_VT_ATKT, O_VT_DEFT, O_VT_CARR, O_VT_AUX, O_VT_MOUNTLESS] {
        for s in (0..0x400).step_by(4) {
            a.w32(vt + s, zero0);
        }
    }

    // attacker object vtable
    a.w32(O_VT_ATK + 0x18, ctrl[C_ATK_18]);
    a.w32(O_VT_ATK + 0x1C, ctrl[C_ATK_1C]);
    a.w32(O_VT_ATK + 0x20, ctrl[C_ATK_20]);
    a.w32(O_VT_ATK + 0xC8, ctrl[C_ATK_C8]);
    a.w32(O_VT_ATK + 0xCC, ctrl[C_ATK_CC]);
    a.w32(O_VT_ATK + 0xD0, ctrl[C_ATK_D0]);
    a.w32(O_VT_ATK + 0xE4, ctrl[C_ATK_E4]);
    a.w32(O_VT_ATK + 0x130, ctrl[C_ATK_130]);
    // 0x00644696 hands this return straight to FUN_0062DD90, which dereferences
    // `obj[+0x18][+4]`. A zero-returning stub here is an immediate null deref -- the
    // first fault this harness produced.
    a.w32(O_VT_ATK + 0x3C, ctrl[C_ATK_3C]);
    // slot 0x120 is get_attack; use the real retail implementation, not a stub.
    a.w32(O_VT_ATK + 0x120, reloc(0x0064_69F0));
    a.w32(O_VT_ATK + 0x124, reloc(0x0064_7DB0));
    // Exactly 0x653790 makes the caller devirtualise to UnitType::vtbl[0x60].
    a.w32(O_VT_ATK + 0xB8, reloc(VA_TECH_THUNK));

    // defender object vtable
    a.w32(O_VT_DEF + 0x18, ctrl[C_DEF_18]);
    a.w32(O_VT_DEF + 0x1C, ctrl[C_DEF_1C]);
    a.w32(O_VT_DEF + 0x20, ctrl[C_DEF_20]);
    a.w32(O_VT_DEF + 0x3C, ctrl[C_DEF_3C]);
    a.w32(O_VT_DEF + 0x40, ctrl[C_DEF_40]);
    a.w32(O_VT_DEF + 0xC8, ctrl[C_DEF_C8]);
    a.w32(O_VT_DEF + 0xCC, ctrl[C_DEF_CC]);
    a.w32(O_VT_DEF + 0xD0, ctrl[C_DEF_D0]);
    a.w32(O_VT_DEF + 0xD8, ctrl[C_DEF_D8]);
    a.w32(O_VT_DEF + 0x120, ctrl[C_DEF_120]);
    a.w32(O_VT_DEF + 0x124, reloc(0x0064_7DB0));
    a.w32(O_VT_DEF + 0xB8, reloc(VA_TECH_THUNK));

    // type vtables
    a.w32(O_VT_ATKT + 0x60, tech_atk);
    a.w32(O_VT_ATKT + 0xEC, ctrl[C_ATKT_EC]);
    a.w32(O_VT_ATKT + 0x10C, ctrl[C_ATKT_10C]);
    a.w32(O_VT_DEFT + 0x60, tech_def);
    a.w32(O_VT_DEFT + 0xEC, ctrl[C_DEFT_EC]);
    a.w32(O_VT_DEFT + 0x10C, ctrl[C_DEFT_10C]);

    // carrier object vtable (reached through defender vtbl[0x40])
    a.w32(O_VT_CARR + 0x184, ctrl[C_CARR_184]);
    a.w32(O_VT_CARR + 0x20, ctrl[C_CARR_20]);

    // aux vtable behind [0x00E85DDC]
    a.w32(O_VT_AUX + 0x0C, reloc(VA_AUX_MARKER)); // makes the compare at 0x00644243 hit
    a.w32(O_VT_AUX + 0x14, ctrl[C_AUX_14]);

    // ---- objects --------------------------------------------------------------------
    a.w32(O_ATK_OBJ, a.addr(O_VT_ATK));
    a.w32(O_ATK_OBJ + 0x18, a.addr(O_ATK_TYPE));
    a.w32(O_DEF_OBJ, a.addr(O_VT_DEF));
    a.w32(O_DEF_OBJ + 0x18, a.addr(O_DEF_TYPE));
    a.w32(O_ATK_TYPE, a.addr(O_VT_ATKT));
    a.w32(O_DEF_TYPE, a.addr(O_VT_DEFT));

    // the "build" object returned by defender vtbl[0x3C]; its vtable pointer must equal
    // the relocated Build::vftable or the caller takes the virtual-dispatch path instead
    a.w32(O_BUILD, reloc(VA_BUILD_VFTABLE));
    a.w32(O_BUILD + 0x18, a.addr(O_MOUNT_TYPE));
    a.w32(O_MOUNT_TYPE, a.addr(O_VT_MOUNTLESS));
    // FUN_0062DD90 switches on this type id; 0x1B9 returns immediately, which keeps it
    // away from the player-object walk it would otherwise do.
    a.w32(O_MOUNT_TYPE + 4, 0x1B9);
    a.w32(O_CARRIER, a.addr(O_VT_CARR));

    // ---- object tables: base[player*28] -> ptr array; ptr_array[index] -> object -----
    a.w32(O_PTRARR0 + 4 * ATK_INDEX, a.addr(O_ATK_OBJ));
    a.w32(O_PTRARR1 + 4 * DEF_INDEX, a.addr(O_DEF_OBJ));

    // ---- terrain: both coordinates unmask to 0, so the tile record is tiles[0] --------
    a.w32(O_COORD, 0);
    a.w32(O_MAP, 1); // map width
    a.w32(O_MAP + 0x134, a.addr(O_TILES));

    // ---- the aux object behind the attacker-mask fixup gate --------------------------
    a.w32(O_AUXROOT + 0x888, a.addr(O_AUXOBJ));
    a.w32(O_AUXOBJ, a.addr(O_VT_AUX));
    a.w32(O_AUXOBJ + 4, 0); // < 0x32, so the FUN_006DB810 route is never taken

    // ---- city table for the recapture step -------------------------------------------
    a.w32(O_C061D4 + DEF_PLAYER as usize * 28 + 0x10, a.addr(O_CITYARR));
    a.w32(O_CITYARR, a.addr(O_CITY));

    // Every player's `+0x6EB8` must be a readable pointer: several functions reach
    // `player[+0x6EB8][+0xDC]` on paths we do not otherwise gate.
    for pl in 0..2usize {
        a.w32(O_PLAYERS + pl * PLAYER_STRIDE + 0x6EB8, a.addr(O_PLAYERAUX));
    }

    Stubs { ctrl, zero0, zero8, tech_atk, tech_def }
}

pub const ATK_PLAYER: u32 = 0;
pub const ATK_INDEX: usize = 2;
pub const DEF_PLAYER: u32 = 1;
pub const DEF_INDEX: usize = 5;
