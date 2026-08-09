//! Executing the registry and reporting it honestly.
//!
//! # The failure mode this file is built around
//!
//! A differential suite that cannot run reports zero mismatches. So does a suite that
//! passes. The whole point of this harness is that those two are never confused, which
//! costs three specific design decisions:
//!
//! 1. **Every case runs in a forked child.** A case that segfaults is reported CRASHED,
//!    with the signal, and does not take the suite down or vanish. Environment surgery
//!    (a fake `%fs` base, IAT patches, writes into `.data`) stays inside the child that
//!    needed it and cannot silently change what a later case measured.
//! 2. **A case that cannot run is SKIPPED and is never green.** Missing corpus file, VA
//!    outside the mapped image, `PROT_EXEC` refused, filtered out by `--only` — all of
//!    them produce a SKIPPED record and a non-zero exit.
//! 3. **The harness proves itself first.** `selftest` executes machine code we wrote. If
//!    it fails, nothing is reported as passing, because a broken mapping mechanism
//!    produces agreement-shaped output for the wrong reason.
//!
//! Exit codes: `0` everything ran and passed, `1` a mismatch or a crash, `2` something was
//! skipped, `3` the harness could not start.

use crate::damage_env;
use crate::damage_test;
use crate::image::{self, Mapped, PAGE};
use crate::models;
use crate::registry::{Case, Cols, Dist2, Plan, KNOWN_GAPS, REGISTRY};
use crate::turn_test;
use don_pe::PeImage;
use std::ffi::c_void;
use std::io::Write;
use std::time::Instant;

// ---------------------------------------------------------------------------------
// Deterministic input stream
// ---------------------------------------------------------------------------------

/// xorshift64. No external crate, and the seed is reported with every result so a failing
/// trial can be replayed exactly.
pub struct Xs(pub u64);
impl Xs {
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}
// ---------------------------------------------------------------------------------
// Calling conventions
// ---------------------------------------------------------------------------------

unsafe fn call_ecx1(f: *const u8, x: u32) -> u32 {
    let r: u32;
    std::arch::asm!("call {f}", f = in(reg) f, in("ecx") x, lateout("eax") r, clobber_abi("C"));
    r
}

unsafe fn call_thiscall0(f: *const u8, this: *mut u8) -> u32 {
    let r: u32;
    std::arch::asm!("call {f}", f = in(reg) f, in("ecx") this, lateout("eax") r, clobber_abi("C"));
    r
}

/// `__thiscall` plus one pushed dword — `RString::AsScaled(scale)`, callee cleans (`ret 4`).
unsafe fn call_thiscall1(f: *const u8, this: *mut u8, arg: i32) -> i32 {
    let r: i32;
    std::arch::asm!(
        "push {s:e}",
        "call {f}",
        s = in(reg) arg,
        f = in(reg) f,
        in("ecx") this,
        lateout("eax") r,
        clobber_abi("C"),
    );
    r
}

/// `Guy::turn_angles`: `__thiscall` plus four dwords, callee cleans (`ret 0x10`).
unsafe fn call_turn_angles(
    f: *const u8,
    this: *mut u8,
    desired: u32,
    out_angle: *mut u32,
    raw_step: i32,
    half: i32,
) -> u32 {
    let r: u32;
    std::arch::asm!(
        "push {half:e}",
        "push {raw:e}",
        "push {out:e}",
        "push {desired:e}",
        "call {f}",
        half = in(reg) half,
        raw = in(reg) raw_step,
        out = in(reg) out_angle,
        desired = in(reg) desired,
        f = in(reg) f,
        in("ecx") this,
        lateout("eax") r,
        clobber_abi("C"),
    );
    r
}

/// `Guy::turn_towards`: `__thiscall` plus three dwords, callee cleans (`ret 0x0C`).
unsafe fn call_turn_towards(
    f: *const u8,
    this: *mut u8,
    desired: u32,
    unread: i32,
    animate: i32,
) -> u32 {
    let r: u32;
    std::arch::asm!(
        "push {animate:e}",
        "push {unread:e}",
        "push {desired:e}",
        "call {f}",
        animate = in(reg) animate,
        unread = in(reg) unread,
        desired = in(reg) desired,
        f = in(reg) f,
        in("ecx") this,
        lateout("eax") r,
        clobber_abi("C"),
    );
    r
}

/// `Map::make`: `__thiscall` plus three dwords, callee cleans (`ret 0x0C`).
unsafe fn call_map_make(f: *const u8, this: *mut u8, map_arg: i32, seed: i32, mode: i32) {
    std::arch::asm!(
        "push {mode:e}",
        "push {seed:e}",
        "push {map_arg:e}",
        "call {f}",
        mode = in(reg) mode,
        seed = in(reg) seed,
        map_arg = in(reg) map_arg,
        f = in(reg) f,
        in("ecx") this,
        out("eax") _,
        clobber_abi("C"),
    );
}

/// `WorldData::start_city_wcoord`: two pointer arguments, callee cleans (`ret 8`).
unsafe fn call_start_city_wcoord(f: *const u8, this: *mut u8, x: *const i32, y: *const i32) -> i32 {
    let r: i32;
    std::arch::asm!(
        "push {y:e}",
        "push {x:e}",
        "call {f}",
        y = in(reg) y,
        x = in(reg) x,
        f = in(reg) f,
        in("ecx") this,
        lateout("eax") r,
        clobber_abi("C"),
    );
    r
}

/// `World::add_starting_location`: two pointer arguments, callee cleans (`ret 8`).
unsafe fn call_add_starting_location(
    f: *const u8,
    this: *mut u8,
    x: *const i32,
    y: *const i32,
) -> i32 {
    let r: i32;
    std::arch::asm!(
        "push {y:e}",
        "push {x:e}",
        "call {f}",
        y = in(reg) y,
        x = in(reg) x,
        f = in(reg) f,
        in("ecx") this,
        lateout("eax") r,
        clobber_abi("C"),
    );
    r
}

/// `MapFairness::calc_distances`: two pointer stack arguments and scale in XMM3.
unsafe fn call_map_fairness_calc_distances(
    f: *const u8,
    this: *mut u8,
    x: *const i32,
    y: *const i32,
    scale: f32,
) {
    std::arch::asm!(
        "push {y:e}",
        "push {x:e}",
        "call {f}",
        y = in(reg) y,
        x = in(reg) x,
        f = in(reg) f,
        in("ecx") this,
        in("xmm3") scale,
        out("eax") _, out("edx") _,
        out("xmm0") _, out("xmm1") _, out("xmm2") _,
        clobber_abi("C"),
    );
}

/// Cdecl no-argument call used for retail `circle_init`.
unsafe fn call_cdecl0(f: *const u8) {
    std::arch::asm!("call {f}", f = in(reg) f, clobber_abi("C"));
}

/// `Map::place_start_in_region`: seven callee-cleaned dword arguments.
unsafe fn call_place_start_in_region(
    f: *const u8,
    this: *mut u8,
    region: i32,
    out_x: *mut i32,
    out_y: *mut i32,
    min_dist: i32,
    unread: i32,
    prior_x: *mut u8,
    prior_y: *mut u8,
) -> i32 {
    let args = [
        region as u32,
        out_x as usize as u32,
        out_y as usize as u32,
        min_dist as u32,
        unread as u32,
        prior_x as usize as u32,
        prior_y as usize as u32,
    ];
    let result: i32;
    std::arch::asm!(
        "push dword ptr [{args} + 24]",
        "push dword ptr [{args} + 20]",
        "push dword ptr [{args} + 16]",
        "push dword ptr [{args} + 12]",
        "push dword ptr [{args} + 8]",
        "push dword ptr [{args} + 4]",
        "push dword ptr [{args}]",
        "call {f}",
        args = in(reg) args.as_ptr(),
        f = in(reg) f,
        in("ecx") this,
        lateout("eax") result,
        clobber_abi("C"),
    );
    result
}

/// `Random::next_float` — result in xmm0, state updated through ECX.
unsafe fn call_next_float(f: *const u8, p: *mut u32) -> (u32, f32) {
    let out_f: f32;
    std::arch::asm!(
        "call {f:e}",
        f = in(reg) f as u32,
        in("ecx") p,
        out("eax") _, out("edx") _,
        lateout("xmm0") out_f,
        out("xmm1") _, out("xmm2") _, out("xmm3") _,
        out("xmm4") _, out("xmm5") _, out("xmm6") _, out("xmm7") _,
    );
    (std::ptr::read_volatile(p), out_f)
}

/// `Random::in_range(lo, hi)`. Layout of `p`: `[0]` state, `[1]` lo, `[2]` hi.
unsafe fn call_in_range(f: *const u8, p: *mut u32) -> i32 {
    let ret: i32;
    std::arch::asm!(
        "push dword ptr [{p:e} + 8]",
        "push dword ptr [{p:e} + 4]",
        "call {f:e}",
        f = in(reg) f as u32,
        p = in(reg) p,
        in("ecx") p,
        lateout("eax") ret,
        out("edx") _,
        out("xmm0") _, out("xmm1") _, out("xmm2") _, out("xmm3") _,
        out("xmm4") _, out("xmm5") _, out("xmm6") _, out("xmm7") _,
    );
    ret
}

// ---------------------------------------------------------------------------------
// Results
// ---------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Pass,
    Fail,
    Skipped,
    Crashed,
    Error,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Pass => "pass",
            Status::Fail => "fail",
            Status::Skipped => "skipped",
            Status::Crashed => "crashed",
            Status::Error => "error",
        }
    }
    fn label(self) -> &'static str {
        match self {
            Status::Pass => "PASS",
            Status::Fail => "FAIL",
            Status::Skipped => "SKIP",
            Status::Crashed => "CRASH",
            Status::Error => "ERROR",
        }
    }
}

pub struct Phase {
    pub kind: String,
    pub count: u64,
    pub description: String,
}

pub struct Excluded {
    pub reason: String,
    pub count: u64,
}

pub struct CaseResult {
    pub id: &'static str,
    pub status: Status,
    pub trials: u64,
    pub mismatches: u64,
    pub phases: Vec<Phase>,
    pub excluded: Vec<Excluded>,
    pub detail: String,
    pub extras: Vec<(String, String)>,
    pub wall_ms: u128,
}

impl CaseResult {
    fn skipped(id: &'static str, why: &str) -> CaseResult {
        CaseResult {
            id,
            status: Status::Skipped,
            trials: 0,
            mismatches: 0,
            phases: Vec::new(),
            excluded: Vec::new(),
            detail: why.to_string(),
            extras: Vec::new(),
            wall_ms: 0,
        }
    }
}

/// What a case's forked child accumulates. Serialised over a pipe as `key=value` lines,
/// which keeps the protocol greppable when a child does something surprising.
#[derive(Default)]
pub struct Acc {
    pub trials: u64,
    pub mismatches: u64,
    pub phases: Vec<Phase>,
    pub excluded: Vec<Excluded>,
    pub detail: String,
    pub extras: Vec<(String, String)>,
    /// Set when the case could not run at all.
    pub skip: Option<String>,
}

impl Acc {
    fn phase(&mut self, kind: &str, count: u64, description: &str) {
        self.phases.push(Phase {
            kind: kind.into(),
            count,
            description: description.into(),
        });
    }
    fn exclude(&mut self, reason: &str, count: u64) {
        if count > 0 {
            self.excluded.push(Excluded {
                reason: reason.into(),
                count,
            });
        }
    }
    fn first_detail(&mut self, s: String) {
        if self.detail.is_empty() {
            self.detail = s;
        }
    }
}

fn one_line(s: &str) -> String {
    s.replace(['\n', '\r'], " ")
}

// ---------------------------------------------------------------------------------
// Context
// ---------------------------------------------------------------------------------

pub struct Ctx<'a> {
    pub m: &'a Mapped,
    pub pe: &'a PeImage,
    pub scale: f64,
    pub seed: u64,
}

impl Ctx<'_> {
    /// Address of a preferred-base VA in this mapping, or `None` if it is outside the
    /// image. `None` becomes a SKIP, never a silently-omitted phase.
    fn at(&self, va: u32) -> Option<*mut u8> {
        if va < self.pe.image_base {
            return None;
        }
        let rva = va - self.pe.image_base;
        if rva as usize >= self.pe.size_of_image as usize {
            return None;
        }
        Some(self.m.addr_of_rva(rva))
    }
    fn scaled(&self, n: u32) -> u32 {
        if self.scale == 1.0 {
            return n;
        }
        let v = (n as f64 * self.scale).round();
        if v < 1.0 {
            1
        } else if v > u32::MAX as f64 {
            u32::MAX
        } else {
            v as u32
        }
    }
}

fn scratch_page(bytes: usize) -> Option<*mut u8> {
    let p = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            bytes,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
            -1,
            0,
        )
    };
    if p == libc::MAP_FAILED {
        None
    } else {
        Some(p as *mut u8)
    }
}

// ---------------------------------------------------------------------------------
// The executors — one per Plan variant
// ---------------------------------------------------------------------------------

fn exec(ctx: &Ctx, c: &Case) -> Acc {
    let mut a = Acc::default();
    let Some(f) = ctx.at(c.va) else {
        a.skip = Some(format!(
            "VA {:#010x} is outside the mapped image (base {:#010x}, size {:#x})",
            c.va, ctx.pe.image_base, ctx.pe.size_of_image
        ));
        return a;
    };
    match &c.plan {
        Plan::Stdcall4 {
            model,
            edges,
            random,
            random_distribution,
        } => {
            let g: extern "stdcall" fn(i32, i32, i32, i32) -> i32 =
                unsafe { std::mem::transmute(f as *const u8) };
            for e in edges.iter() {
                let want = model(e[0], e[1], e[2], e[3]);
                let got = g(e[0], e[1], e[2], e[3]);
                a.trials += 1;
                if want != got {
                    a.mismatches += 1;
                    a.first_detail(format!(
                        "edge a={} b={} lo={} hi={} model={} retail={}",
                        e[0], e[1], e[2], e[3], want, got
                    ));
                }
            }
            a.phase("edges", edges.len() as u64, "hand-chosen edge tuples");
            let n = ctx.scaled(*random);
            let mut rng = Xs(ctx.seed);
            for _ in 0..n {
                let (r, q) = (rng.next(), rng.next());
                let v = crate::registry::draw_hash_into_range(r, q);
                let want = model(v[0], v[1], v[2], v[3]);
                let got = g(v[0], v[1], v[2], v[3]);
                a.trials += 1;
                if want != got {
                    a.mismatches += 1;
                    a.first_detail(format!(
                        "random a={} b={} lo={} hi={} model={} retail={}",
                        v[0], v[1], v[2], v[3], want, got
                    ));
                }
            }
            a.phase("random", n as u64, random_distribution);
        }

        Plan::Ecx1 {
            model,
            edges,
            sweep_start,
            sweep_stride,
            sweep_count,
            sweep_distribution,
        } => {
            let f = f as *const u8;
            for &x in edges.iter() {
                let want = model(x);
                let got = unsafe { call_ecx1(f, x) };
                a.trials += 1;
                if want != got {
                    a.mismatches += 1;
                    a.first_detail(format!("edge x={x:#010x} model={want} retail={got}"));
                }
            }
            a.phase(
                "edges",
                edges.len() as u64,
                "instruction-sequence boundaries and neighbours",
            );
            let n = ctx.scaled(*sweep_count);
            let mut x = *sweep_start;
            for _ in 0..n {
                let want = model(x);
                let got = unsafe { call_ecx1(f, x) };
                a.trials += 1;
                if want != got {
                    a.mismatches += 1;
                    a.first_detail(format!("sweep x={x:#010x} model={want} retail={got}"));
                }
                x = x.wrapping_add(*sweep_stride);
            }
            a.phase("stride-sweep", n as u64, sweep_distribution);
        }

        Plan::ThiscallScratch {
            write_and_model,
            random,
            distribution,
        } => {
            let Some(obj) = scratch_page(PAGE) else {
                a.skip = Some("scratch mmap failed".into());
                return a;
            };
            let f = f as *const u8;
            let n = ctx.scaled(*random);
            let mut rng = Xs(ctx.seed);
            for _ in 0..n {
                let r = rng.next();
                let want = write_and_model(obj, r);
                let got = unsafe { call_thiscall0(f, obj) };
                a.trials += 1;
                if want != got {
                    a.mismatches += 1;
                    a.first_detail(format!("input={r:#018x} model={want:#x} retail={got:#x}"));
                }
            }
            a.phase("random", n as u64, distribution);
            unsafe { libc::munmap(obj as *mut c_void, PAGE) };
        }

        Plan::Stdcall2Table {
            table_va,
            elem_bits,
            index,
            grids,
        } => {
            if *elem_bits != 16 {
                a.skip = Some(format!("unsupported element width {elem_bits}"));
                return a;
            }
            let Some(table) = ctx.at(*table_va) else {
                a.skip = Some(format!(
                    "table VA {table_va:#010x} outside the mapped image"
                ));
                return a;
            };
            let table_rva = (*table_va - ctx.pe.image_base) as i64;
            let image_len = ctx.pe.size_of_image as i64;
            let g: extern "stdcall" fn(i32, i32) -> i32 =
                unsafe { std::mem::transmute(f as *const u8) };
            let mut out_of_image = 0u64;
            for grid in grids.iter() {
                let mut n = 0u64;
                let cols: Vec<i32> = match &grid.cols {
                    Cols::Range(lo, hi) => (*lo..*hi).collect(),
                    Cols::List(v) => v.to_vec(),
                };
                for row in grid.row_lo..grid.row_hi {
                    for &col in &cols {
                        let idx = index(row, col) as i64;
                        let off = table_rva + idx * 2;
                        if off < 0 || off + 2 > image_len {
                            out_of_image += 1;
                            continue;
                        }
                        let want = unsafe {
                            std::ptr::read_unaligned(
                                (table as *const u8).offset((idx * 2) as isize) as *const i16,
                            )
                        } as i32;
                        let got = g(row, col);
                        a.trials += 1;
                        n += 1;
                        if want != got {
                            a.mismatches += 1;
                            a.first_detail(format!(
                                "row={row} col={col} model={want} retail={got}"
                            ));
                        }
                    }
                }
                a.phase("grid", n, grid.label);
            }
            a.exclude("index outside the mapped image", out_of_image);
        }

        Plan::BalanceTable {
            table_va,
            capture_file,
            grids,
        } => {
            // 1. Load the capture through the SHIPPED loader, so its size and
            //    no-negatives guards are part of what this case exercises.
            let raw = match std::fs::read(capture_file) {
                Ok(r) => r,
                Err(e) => {
                    a.skip = Some(format!(
                        "captured balance table {capture_file} is not readable: {e} — \
                         refusing to run against the zero-filled image, which would agree \
                         with any indexing at all"
                    ));
                    return a;
                }
            };
            let model = match don_sim::balance::BalanceTable::from_bytes(&raw) {
                Ok(t) => t,
                Err(e) => {
                    a.skip = Some(format!("{capture_file} did not load: {e}"));
                    return a;
                }
            };

            // 2. Write it into the mapped image at the array's real base. `.data` is
            //    mapped RW, but assuming that would be exactly the sort of unchecked
            //    assumption this suite exists to catch, so make the span writable and
            //    then read one byte back through the mapping before believing it.
            let Some(dst) = ctx.at(*table_va) else {
                a.skip = Some(format!(
                    "table VA {table_va:#010x} outside the mapped image"
                ));
                return a;
            };
            let end_rva = (*table_va - ctx.pe.image_base) as usize + raw.len();
            if end_rva > ctx.pe.size_of_image as usize {
                a.skip = Some(format!(
                    "the {} byte array at {table_va:#010x} runs past the image",
                    raw.len()
                ));
                return a;
            }
            let span_start = (dst as usize) & !(PAGE - 1);
            let span_end = image::round_up(dst as usize + raw.len(), PAGE);
            let rc = unsafe {
                libc::mprotect(
                    span_start as *mut c_void,
                    span_end - span_start,
                    libc::PROT_READ | libc::PROT_WRITE,
                )
            };
            if rc != 0 {
                a.skip = Some(format!("cannot make {table_va:#010x} writable"));
                return a;
            }
            unsafe { std::ptr::copy_nonoverlapping(raw.as_ptr(), dst, raw.len()) };
            let last = unsafe { std::ptr::read_unaligned(dst.add(raw.len() - 2) as *const i16) };
            let want_last = model.raw()[model.raw().len() - 1];
            if last != want_last {
                a.skip = Some(format!(
                    "the injected array did not stick: last element reads {last}, expected \
                     {want_last}"
                ));
                return a;
            }
            a.extras.push((
                "injected_bytes".into(),
                format!("{} at {table_va:#010x}", raw.len()),
            ));
            let (min, max, distinct, nz) = model.stats();
            a.extras.push((
                "injected_shape".into(),
                format!("min={min} max={max} distinct={distinct} nonzero={nz}"),
            ));

            // 3. Retail's own accessor against the shipped one, at raw TypeIndex.
            let g: extern "stdcall" fn(i32, i32) -> i32 =
                unsafe { std::mem::transmute(f as *const u8) };
            let mut outside = 0u64;
            for grid in grids.iter() {
                let cols: Vec<i32> = match &grid.cols {
                    Cols::Range(lo, hi) => (*lo..*hi).collect(),
                    Cols::List(v) => v.to_vec(),
                };
                let mut n = 0u64;
                for row in grid.row_lo..grid.row_hi {
                    for &col in &cols {
                        let Some(want) = model.get(row, col) else {
                            outside += 1;
                            continue;
                        };
                        let got = g(row, col);
                        a.trials += 1;
                        n += 1;
                        if want != got {
                            a.mismatches += 1;
                            a.first_detail(format!(
                                "atk_type={row} def_type={col} model={want} retail={got}"
                            ));
                        }
                    }
                }
                a.phase("grid", n, grid.label);
            }
            a.exclude(
                "TypeIndex outside 50..=542, where the shipped accessor refuses and retail \
                 reads adjacent .data",
                outside,
            );
        }

        Plan::GuyTurnSpeed {
            random,
            distribution,
        } => {
            let Some(arena) = scratch_page(turn_test::ARENA_BYTES) else {
                a.skip = Some("turn fixture scratch mmap failed".into());
                return a;
            };
            if ctx.at(turn_test::VA_OBJECT_LISTS).is_none()
                || ctx.at(turn_test::VA_OBJECT_LISTS + 7 * 7 * 4).is_none()
                || ctx.at(turn_test::VA_CONSTANTS_PTR).is_none()
            {
                a.skip = Some("turn fixture globals are outside the mapped image".into());
                unsafe { libc::munmap(arena as *mut c_void, turn_test::ARENA_BYTES) };
                return a;
            }
            let write_global = |va: u32, v: u32| unsafe {
                std::ptr::write_unaligned(ctx.at(va).unwrap() as *mut u32, v)
            };
            let f = f as *const u8;
            let mut counts = [0u64; 8];
            let mut check = |s: turn_test::Scenario, label: &str, a: &mut Acc| {
                if s.turn_speed_would_de() {
                    return false;
                }
                let guy = unsafe { turn_test::install(arena, s, &write_global) };
                let want = s.model_turn_speed();
                let got = unsafe { call_thiscall1(f, guy, s.arg) as u32 };
                a.trials += 1;
                counts[if (s.guy_num as i32) < s.squad_size {
                    0
                } else {
                    1
                }] += 1;
                counts[if s.arg == 0 { 2 } else { 3 }] += 1;
                if s.track_dx != 0 || s.track_dy != 0 {
                    counts[4] += 1;
                }
                if s.fast_face && s.last_speed == 0 {
                    counts[5] += 1;
                }
                if s.unit_mask_scale2 {
                    counts[6] += 1;
                }
                if s.turn_scale < 0 || s.turn_scale2 < 0 || s.type_turn_speed < 0 {
                    counts[7] += 1;
                }
                if want != got {
                    a.mismatches += 1;
                    a.first_detail(one_line(&format!(
                        "{label} model={want:#010x} retail={got:#010x} scenario={s:?}"
                    )));
                }
                true
            };

            let edges = turn_test::edges();
            for &s in &edges {
                debug_assert!(!s.turn_speed_would_de());
                check(s, "edge", &mut a);
            }
            a.phase(
                "edges",
                edges.len() as u64,
                "hand-selected squad/crew, early-return, scaling, damping, floor and \
                 signed/wrapping boundaries",
            );

            let generated = ctx.scaled(*random);
            let mut rng = Xs(ctx.seed);
            let mut de = 0u64;
            let before = a.trials;
            for _ in 0..generated {
                let s = turn_test::Scenario::draw([rng.next(), rng.next(), rng.next(), rng.next()]);
                if s.turn_speed_would_de() {
                    de += 1;
                    continue;
                }
                check(s, "random", &mut a);
            }
            a.phase("random", a.trials - before, distribution);
            a.exclude("retail #DE: damped avg_speed/4 + 1 is zero", de);
            a.extras.push((
                "branch_counts".into(),
                format!(
                    "squad={} crew={} damped={} raw={} tracking={} fast_face={} scale2={} \
                     signed_or_wrapping_rules={}",
                    counts[0],
                    counts[1],
                    counts[2],
                    counts[3],
                    counts[4],
                    counts[5],
                    counts[6],
                    counts[7]
                ),
            ));
            unsafe { libc::munmap(arena as *mut c_void, turn_test::ARENA_BYTES) };
        }

        Plan::GuyTurnAngles {
            random,
            distribution,
        } => {
            let Some(arena) = scratch_page(turn_test::ARENA_BYTES) else {
                a.skip = Some("turn fixture scratch mmap failed".into());
                return a;
            };
            if ctx.at(turn_test::VA_OBJECT_LISTS).is_none()
                || ctx.at(turn_test::VA_OBJECT_LISTS + 7 * 7 * 4).is_none()
                || ctx.at(turn_test::VA_CONSTANTS_PTR).is_none()
            {
                a.skip = Some("turn fixture globals are outside the mapped image".into());
                unsafe { libc::munmap(arena as *mut c_void, turn_test::ARENA_BYTES) };
                return a;
            }
            let write_global = |va: u32, v: u32| unsafe {
                std::ptr::write_unaligned(ctx.at(va).unwrap() as *mut u32, v)
            };
            let f = f as *const u8;
            let mut counts = [0u64; 5];
            let mut check = |s: turn_test::Scenario, label: &str, a: &mut Acc| {
                let guy = unsafe { turn_test::install(arena, s, &write_global) };
                let (want_angle, want_rem) = s.model_turn_angles();
                let mut got_angle = 0xDEAD_BEEFu32;
                let got_rem = unsafe {
                    call_turn_angles(f, guy, s.desired, &mut got_angle, 1, s.half as i32)
                };
                a.trials += 1;
                let diff = s.desired.wrapping_sub(s.angle);
                let mag = if diff > 0x8000_0000 { !diff } else { diff };
                if mag < don_sim::systems::groups_guys::ANGLE_SNAP {
                    counts[0] += 1;
                }
                if diff == 0x8000_0000 || diff == 0x8000_0001 {
                    counts[1] += 1;
                }
                if diff > 0x8000_0000 {
                    counts[2] += 1;
                } else {
                    counts[3] += 1;
                }
                if s.half {
                    counts[4] += 1;
                }
                if want_angle != got_angle || want_rem != got_rem {
                    a.mismatches += 1;
                    a.first_detail(one_line(&format!(
                        "{label} model=(angle={want_angle:#010x}, rem={want_rem:#010x}) \
                         retail=(angle={got_angle:#010x}, rem={got_rem:#010x}) scenario={s:?}"
                    )));
                }
            };

            let edges = turn_test::edges();
            for &s in &edges {
                check(s, "edge", &mut a);
            }
            a.phase(
                "edges",
                edges.len() as u64,
                "hand-selected snap neighbours, half-turn tie, wrap, step equality and \
                 half-step boundaries",
            );
            let generated = ctx.scaled(*random);
            let mut rng = Xs(ctx.seed ^ 0xA5A5_5A5A_C3C3_3C3C);
            for _ in 0..generated {
                let s = turn_test::Scenario::draw([rng.next(), rng.next(), rng.next(), rng.next()]);
                check(s, "random", &mut a);
            }
            a.phase("random", generated as u64, distribution);
            a.extras.push((
                "branch_counts".into(),
                format!(
                    "snap={} half_turn_ties={} reverse={} forward={} half_step={}",
                    counts[0], counts[1], counts[2], counts[3], counts[4]
                ),
            ));
            unsafe { libc::munmap(arena as *mut c_void, turn_test::ARENA_BYTES) };
        }

        Plan::GuyTurnTowards {
            random,
            distribution,
        } => {
            let Some(arena) = scratch_page(turn_test::ARENA_BYTES) else {
                a.skip = Some("turn fixture scratch mmap failed".into());
                return a;
            };
            if ctx.at(turn_test::VA_OBJECT_LISTS).is_none()
                || ctx.at(turn_test::VA_OBJECT_LISTS + 7 * 7 * 4).is_none()
                || ctx.at(turn_test::VA_CONSTANTS_PTR).is_none()
            {
                a.skip = Some("turn fixture globals are outside the mapped image".into());
                unsafe { libc::munmap(arena as *mut c_void, turn_test::ARENA_BYTES) };
                return a;
            }
            let write_global = |va: u32, v: u32| unsafe {
                std::ptr::write_unaligned(ctx.at(va).unwrap() as *mut u32, v)
            };
            let f = f as *const u8;
            let mut counts = [0u64; 6];
            let mut check = |s: turn_test::Scenario, label: &str, a: &mut Acc| {
                let before = s.guy().walk_bytes();
                let (want_rem, want_walk) = s.model_turn_towards();
                let guy = unsafe { turn_test::install(arena, s, &write_global) };
                let got_rem = unsafe { call_turn_towards(f, guy, s.desired, s.arg, 0) };
                let got_walk = unsafe {
                    std::slice::from_raw_parts(
                        guy.add(don_sim::systems::groups_guys::GUY_WALK_LO),
                        don_sim::systems::groups_guys::GUY_WALK_LEN,
                    )
                };
                a.trials += 1;
                if before == want_walk {
                    counts[0] += 1;
                } else {
                    counts[1] += 1;
                }
                if s.guy_num == 0 {
                    counts[2] += 1;
                }
                if (s.guy_num as i32) >= s.squad_size {
                    counts[3] += 1;
                }
                if s.arg == 0 {
                    counts[4] += 1;
                } else {
                    counts[5] += 1;
                }
                if want_rem != got_rem || want_walk.as_slice() != got_walk {
                    a.mismatches += 1;
                    let byte = want_walk
                        .iter()
                        .zip(got_walk.iter())
                        .position(|(want, got)| want != got);
                    a.first_detail(one_line(&format!(
                        "{label} model_rem={want_rem:#010x} retail_rem={got_rem:#010x} \
                         first_state_byte={byte:?} model_byte={:?} retail_byte={:?} \
                         scenario={s:?}",
                        byte.map(|i| want_walk[i]),
                        byte.map(|i| got_walk[i]),
                    )));
                }
            };

            let edges = turn_test::edges();
            for &s in &edges {
                check(s, "edge", &mut a);
            }
            a.phase(
                "edges",
                edges.len() as u64,
                "hand-selected snap, wrap, step, squad/crew, leader and do_turn state \
                 boundaries; full synchronized GuyData image compared",
            );
            let generated = ctx.scaled(*random);
            let mut rng = Xs(ctx.seed ^ 0xD1B5_4A32_D192_ED03);
            for _ in 0..generated {
                let s = turn_test::Scenario::draw([rng.next(), rng.next(), rng.next(), rng.next()]);
                check(s, "random", &mut a);
            }
            a.phase("random", generated as u64, distribution);
            a.extras.push((
                "branch_counts".into(),
                format!(
                    "unchanged={} changed={} leader={} crew={} unread_arg_zero={} \
                     unread_arg_nonzero={}",
                    counts[0], counts[1], counts[2], counts[3], counts[4], counts[5]
                ),
            ));
            unsafe { libc::munmap(arena as *mut c_void, turn_test::ARENA_BYTES) };
        }

        Plan::MapMakeSeedPrefix {
            random,
            distribution,
        } => {
            if let Err(e) = image::install_fake_teb() {
                a.skip = Some(format!("fake TEB: {e}"));
                return a;
            }
            let Some(arena) = scratch_page(PAGE) else {
                a.skip = Some("map seed fixture scratch mmap failed".into());
                return a;
            };
            const O_MAP: usize = 0x000;
            const O_WORLD: usize = 0x200;
            const O_RANDOM: usize = 0x400;
            const VA_WORLD_PTR: u32 = 0x00C0_6188;
            const VA_RANDOM_PTR: u32 = 0x00C0_6184;
            const PREFIX_END: usize = 0x0068_BCD2 - 0x0068_BC90;
            const EPILOGUE: u32 = 0x0068_C84A;

            let patch = unsafe { f.add(PREFIX_END) };
            let Some(epilogue) = ctx.at(EPILOGUE) else {
                a.skip = Some("Map::make epilogue VA is outside the mapped image".into());
                unsafe { libc::munmap(arena as *mut c_void, PAGE) };
                return a;
            };
            let prefix_next = unsafe { std::slice::from_raw_parts(patch, 3) };
            let epilogue_head = unsafe { std::slice::from_raw_parts(epilogue, 3) };
            if prefix_next != [0x8b, 0x75, 0x10] || epilogue_head != [0x8b, 0x4d, 0xf4] {
                a.skip = Some(format!(
                    "Map::make isolation boundary bytes changed: next={prefix_next:02x?} \
                     epilogue={epilogue_head:02x?}"
                ));
                unsafe { libc::munmap(arena as *mut c_void, PAGE) };
                return a;
            }
            let page = (patch as usize) & !(PAGE - 1);
            if unsafe {
                libc::mprotect(
                    page as *mut c_void,
                    PAGE,
                    libc::PROT_READ | libc::PROT_WRITE | libc::PROT_EXEC,
                )
            } != 0
            {
                a.skip = Some("cannot make Map::make prefix boundary RWX".into());
                unsafe { libc::munmap(arena as *mut c_void, PAGE) };
                return a;
            }
            let displacement = (epilogue as isize).wrapping_sub(patch as isize + 5) as i32;
            unsafe {
                std::ptr::write_volatile(patch, 0xe9);
                std::ptr::write_unaligned(patch.add(1) as *mut i32, displacement);
            }

            let Some(world_slot) = ctx.at(VA_WORLD_PTR) else {
                a.skip = Some("GameAccess::world pointer VA is outside the mapped image".into());
                unsafe { libc::munmap(arena as *mut c_void, PAGE) };
                return a;
            };
            let Some(random_slot) = ctx.at(VA_RANDOM_PTR) else {
                a.skip =
                    Some("GameAccess::game_random pointer VA is outside the mapped image".into());
                unsafe { libc::munmap(arena as *mut c_void, PAGE) };
                return a;
            };
            let map = unsafe { arena.add(O_MAP) };
            let world = unsafe { arena.add(O_WORLD) };
            let random_state = unsafe { arena.add(O_RANDOM) as *mut i32 };
            unsafe {
                std::ptr::write_unaligned(world_slot as *mut u32, world as usize as u32);
                std::ptr::write_unaligned(random_slot as *mut u32, random_state as usize as u32);
            }

            let mut model_world = don_sim::systems::map_terrain::World::init_default_rules(0, 0);
            let mut counts = [0u64; 2];
            let mut check = |map_arg: i32,
                             seed: i32,
                             initial_world: i32,
                             initial_rng: i32,
                             initial_map_arg: i32,
                             label: &str,
                             a: &mut Acc| {
                unsafe {
                    std::ptr::write_unaligned(map.add(0x110) as *mut i32, initial_map_arg);
                    std::ptr::write_unaligned(world.add(0x7c) as *mut i32, initial_world);
                    std::ptr::write_unaligned(random_state, initial_rng);
                }
                model_world.seed = initial_world;
                let model_rng = model_world.seed_map_generation(seed).unwrap_or(initial_rng);
                unsafe { call_map_make(f, map, map_arg, seed, 0) };
                let got_map_arg = unsafe { std::ptr::read_unaligned(map.add(0x110) as *const i32) };
                let got_world = unsafe { std::ptr::read_unaligned(world.add(0x7c) as *const i32) };
                let got_rng = unsafe { std::ptr::read_unaligned(random_state) };
                a.trials += 1;
                counts[(seed >= 0) as usize] += 1;
                if got_map_arg != map_arg || got_world != model_world.seed || got_rng != model_rng {
                    a.mismatches += 1;
                    a.first_detail(format!(
                        "{label} seed={seed} initial=({initial_world},{initial_rng},\
                         {initial_map_arg}) map_arg={map_arg} model=({map_arg},{},{model_rng}) \
                         retail=({got_map_arg},{got_world},{got_rng})",
                        model_world.seed,
                    ));
                }
            };
            let edges = [
                (0, i32::MIN, 1, 2, 3),
                (-1, -2, i32::MIN, i32::MAX, 0),
                (i32::MAX, -1, -7, 11, -13),
                (i32::MIN, 0, 1, 2, 3),
                (1, 1, -1, -2, -3),
                (-123, i32::MAX, i32::MIN, 0, i32::MAX),
                (0x1234_5678, 0x7654_3210, -1, -1, -1),
            ];
            for &(map_arg, seed, iw, ir, im) in &edges {
                check(map_arg, seed, iw, ir, im, "edge", &mut a);
            }
            a.phase(
                "edges",
                edges.len() as u64,
                "signed seed gate boundaries plus independent prior World/RNG/map words",
            );
            let n = ctx.scaled(*random);
            let mut rng = Xs(ctx.seed ^ 0x4D41_5053_4545_4445);
            for _ in 0..n {
                check(
                    rng.next() as i32,
                    rng.next() as i32,
                    rng.next() as i32,
                    rng.next() as i32,
                    rng.next() as i32,
                    "random",
                    &mut a,
                );
            }
            a.phase("random", n as u64, distribution);
            a.extras.push((
                "branch_counts".into(),
                format!(
                    "negative_preserve={} nonnegative_seed={}",
                    counts[0], counts[1]
                ),
            ));
            a.extras.push((
                "isolation_boundary".into(),
                "retail entry..0x0068bcd0; case-local jmp at 0x0068bcd2 -> original epilogue \
                 0x0068c84a"
                    .into(),
            ));
            unsafe { libc::munmap(arena as *mut c_void, PAGE) };
        }

        Plan::StartCityWcoord {
            random,
            distribution,
        } => {
            const ARENA_BYTES: usize = PAGE * 4;
            const O_WORLDC: usize = 0x000;
            const O_WORLD: usize = 0x200;
            const O_BITS: usize = 0x400;
            const BIT_BYTES: usize = 8192;
            const O_X: usize = 0x3000;
            const O_Y: usize = 0x3010;
            const VA_WORLDC_PTR: u32 = 0x00C0_61D0;
            const VA_WORLD_PTR: u32 = 0x00C0_6188;
            let Some(arena) = scratch_page(ARENA_BYTES) else {
                a.skip = Some("start-city fixture scratch mmap failed".into());
                return a;
            };
            let worldc = unsafe { arena.add(O_WORLDC) };
            let world = unsafe { arena.add(O_WORLD) };
            let bits = unsafe { arena.add(O_BITS) };
            let x_ptr = unsafe { arena.add(O_X) as *mut i32 };
            let y_ptr = unsafe { arena.add(O_Y) as *mut i32 };
            let (Some(worldc_slot), Some(world_slot)) =
                (ctx.at(VA_WORLDC_PTR), ctx.at(VA_WORLD_PTR))
            else {
                a.skip = Some("World access pointer VA is outside the mapped image".into());
                unsafe { libc::munmap(arena as *mut c_void, ARENA_BYTES) };
                return a;
            };
            unsafe {
                std::ptr::write_unaligned(worldc_slot as *mut u32, worldc as usize as u32);
                std::ptr::write_unaligned(world_slot as *mut u32, world as usize as u32);
                std::ptr::write_unaligned(world.add(0xf8) as *mut u32, bits as usize as u32);
                std::ptr::write_bytes(bits, 0, BIT_BYTES);
            }
            let f = f as *const u8;
            let mut model_world = don_sim::systems::map_terrain::World::init_default_rules(0, 0);
            model_world.start_city_locs.resize(BIT_BYTES, 0);
            let mut byte_boundary = 0u64;
            let mut row_boundary = 0u64;
            let mut check =
                |width: i32, height: i32, x: i32, y: i32, byte: u8, label: &str, a: &mut Acc| {
                    debug_assert!(
                        width > 0 && height > 0 && x >= 0 && x < width && y >= 0 && y < height
                    );
                    let index = y * width + x;
                    let byte_index = (index >> 3) as usize;
                    debug_assert!(byte_index < BIT_BYTES);
                    unsafe {
                        std::ptr::write_unaligned(worldc as *mut i32, width);
                        std::ptr::write_unaligned(x_ptr, x);
                        std::ptr::write_unaligned(y_ptr, y);
                        std::ptr::write_volatile(bits.add(byte_index), byte);
                    }
                    model_world.xs = width;
                    model_world.start_city_locs[byte_index] = byte;
                    let want = model_world.start_city_wcoord(
                        don_sim::systems::map_terrain::WCoord(x),
                        don_sim::systems::map_terrain::WCoord(y),
                    ) as i32;
                    let got = unsafe { call_start_city_wcoord(f, world, x_ptr, y_ptr) };
                    a.trials += 1;
                    if index & 7 == 0 || index & 7 == 7 {
                        byte_boundary += 1;
                    }
                    if x == 0 || x == width - 1 {
                        row_boundary += 1;
                    }
                    if got != want {
                        a.mismatches += 1;
                        a.first_detail(format!(
                            "{label} width={width} height={height} x={x} y={y} index={index} \
                         byte={byte:#04x} model={want} retail={got}"
                        ));
                    }
                };
            let edges = [
                (1, 1, 0, 0, 0x00),
                (1, 1, 0, 0, 0x01),
                (8, 2, 7, 0, 0x80),
                (8, 2, 0, 1, 0x01),
                (9, 2, 8, 0, 0x01),
                (9, 2, 0, 1, 0x02),
                (9, 2, 8, 1, 0x02),
                (31, 3, 30, 2, 0x80),
                (32, 3, 31, 2, 0xff),
                (33, 3, 32, 2, 0x55),
                (512, 128, 511, 127, 0x80),
            ];
            for &(w, h, x, y, byte) in &edges {
                check(w, h, x, y, byte, "edge", &mut a);
            }
            a.phase(
                "edges",
                edges.len() as u64,
                "LSB/MSB and byte, row, non-byte-width, and maximum-domain boundaries",
            );
            let n = ctx.scaled(*random);
            let mut rng = Xs(ctx.seed ^ 0x5354_4152_5442_4954);
            for _ in 0..n {
                let r = rng.next();
                let width = (r as i32 & 0x1ff).max(1);
                let height = (((r >> 9) as i32 & 0x7f) + 1).min(128);
                let x = ((r >> 16) as u32 % width as u32) as i32;
                let y = (rng.next() as u32 % height as u32) as i32;
                let byte = (rng.next() >> 29) as u8;
                check(width, height, x, y, byte, "random", &mut a);
            }
            a.phase("random", n as u64, distribution);
            a.extras.push((
                "boundary_counts".into(),
                format!("byte_edge={byte_boundary} row_edge={row_boundary}"),
            ));
            unsafe { libc::munmap(arena as *mut c_void, ARENA_BYTES) };
        }

        Plan::AddStartingLocation {
            random,
            distribution,
        } => {
            const ARENA_BYTES: usize = PAGE * 4;
            const O_WORLD: usize = 0x200;
            const O_BITS: usize = 0x2000;
            const BIT_BYTES: usize = 2048;
            const O_X: usize = 0x3000;
            const O_Y: usize = 0x3010;
            const ARRAY_OFFSETS: [usize; 4] = [0x80, 0x9c, 0xb8, 0xd4];
            const LIST_OFFSETS: [usize; 4] = [0x1000, 0x1200, 0x1400, 0x1600];
            const ARRAY_CAPACITY: i32 = 64;
            const VA_WORLD_PTR: u32 = 0x00C0_6188;
            let Some(arena) = scratch_page(ARENA_BYTES) else {
                a.skip = Some("starting-location fixture scratch mmap failed".into());
                return a;
            };
            let world = unsafe { arena.add(O_WORLD) };
            let bits = unsafe { arena.add(O_BITS) };
            let x_ptr = unsafe { arena.add(O_X) as *mut i32 };
            let y_ptr = unsafe { arena.add(O_Y) as *mut i32 };
            let Some(world_slot) = ctx.at(VA_WORLD_PTR) else {
                a.skip = Some("World access pointer VA is outside the mapped image".into());
                unsafe { libc::munmap(arena as *mut c_void, ARENA_BYTES) };
                return a;
            };
            unsafe {
                std::ptr::write_unaligned(world_slot as *mut u32, world as usize as u32);
                std::ptr::write_unaligned(world.add(0xf8) as *mut u32, bits as usize as u32);
            }
            let f = f as *const u8;
            let mut model = don_sim::systems::map_terrain::World::init_default_rules(2, 2);
            let mut check =
                |width: i32, height: i32, coords: &[(i32, i32)], label: &str, a: &mut Acc| {
                    debug_assert!(
                        (2..=128).contains(&width)
                            && (2..=128).contains(&height)
                            && coords.len() <= 8
                            && coords
                                .iter()
                                .all(|&(x, y)| x > 0 && x < width && y > 0 && y < height)
                    );
                    let bit_bytes = ((width * height + 7) / 8) as usize;
                    unsafe {
                        std::ptr::write_bytes(bits, 0, BIT_BYTES);
                        std::ptr::write_unaligned(world as *mut i32, width);
                        for (&array_offset, &list_offset) in
                            ARRAY_OFFSETS.iter().zip(LIST_OFFSETS.iter())
                        {
                            let array = world.add(array_offset);
                            std::ptr::write_unaligned(array.add(4) as *mut i32, 0);
                            std::ptr::write_unaligned(array.add(8) as *mut i32, ARRAY_CAPACITY);
                            std::ptr::write_unaligned(array.add(0x0c) as *mut i16, -1);
                            std::ptr::write_unaligned(
                                array.add(0x10) as *mut u32,
                                arena.add(list_offset) as usize as u32,
                            );
                            std::ptr::write_unaligned(array.add(0x14), 0u8);
                            std::ptr::write_bytes(arena.add(list_offset), 0, 0x100);
                        }
                    }
                    model.xs = width;
                    model.start_x.items.clear();
                    model.start_y.items.clear();
                    model.start_city_x.items.clear();
                    model.start_city_y.items.clear();
                    for array in [
                        &mut model.start_x,
                        &mut model.start_y,
                        &mut model.start_city_x,
                        &mut model.start_city_y,
                    ] {
                        array.capacity = ARRAY_CAPACITY;
                        array.increment = -1;
                        array.flags = 0;
                    }
                    model.start_city_locs.resize(bit_bytes, 0);
                    model.start_city_locs.fill(0);

                    for &(x, y) in coords {
                        unsafe {
                            std::ptr::write_unaligned(x_ptr, x);
                            std::ptr::write_unaligned(y_ptr, y);
                        }
                        let want_return = model.add_starting_location(
                            don_sim::systems::map_terrain::WCoord(x),
                            don_sim::systems::map_terrain::WCoord(y),
                        );
                        let got_return =
                            unsafe { call_add_starting_location(f, world, x_ptr, y_ptr) };
                        a.trials += 1;

                        let expected = [
                            &model.start_x,
                            &model.start_y,
                            &model.start_city_x,
                            &model.start_city_y,
                        ];
                        let mut mismatch = got_return != want_return;
                        let mut detail = format!(
                            "{label} width={width} height={height} append=({x},{y}) \
                         model_return={want_return} retail_return={got_return}"
                        );
                        for (array_index, ((&array_offset, &list_offset), want)) in ARRAY_OFFSETS
                            .iter()
                            .zip(LIST_OFFSETS.iter())
                            .zip(expected)
                            .enumerate()
                        {
                            let array = unsafe { world.add(array_offset) };
                            let got_len =
                                unsafe { std::ptr::read_unaligned(array.add(4) as *const i32) };
                            let got_capacity =
                                unsafe { std::ptr::read_unaligned(array.add(8) as *const i32) };
                            let got_increment =
                                unsafe { std::ptr::read_unaligned(array.add(0x0c) as *const i16) };
                            let got_flags = unsafe { std::ptr::read_unaligned(array.add(0x14)) };
                            let got_items = if got_len >= 0 && got_len <= ARRAY_CAPACITY {
                                unsafe {
                                    std::slice::from_raw_parts(
                                        arena.add(list_offset) as *const i32,
                                        got_len as usize,
                                    )
                                }
                            } else {
                                &[]
                            };
                            if got_len != want.items.len() as i32
                                || got_capacity != want.capacity
                                || got_increment != want.increment
                                || got_flags != want.flags
                                || got_items != want.items.as_slice()
                            {
                                mismatch = true;
                                detail.push_str(&format!(
                                    " array{array_index}=len {got_len}/{} cap {got_capacity}/{} \
                                 inc {got_increment}/{} flags {got_flags:#x}/{:#x} \
                                 items {got_items:?}/{:?}",
                                    want.items.len(),
                                    want.capacity,
                                    want.increment,
                                    want.flags,
                                    want.items
                                ));
                            }
                        }
                        let got_bits = unsafe { std::slice::from_raw_parts(bits, bit_bytes) };
                        if got_bits != model.start_city_locs.as_slice() {
                            mismatch = true;
                            detail.push_str(" occupancy bit plane differs");
                        }
                        if mismatch {
                            a.mismatches += 1;
                            a.first_detail(detail);
                        }
                    }
                };

            let edges: &[&[(i32, i32)]] = &[
                &[(1, 1)],
                &[(7, 1), (1, 2)],
                &[(8, 1), (1, 2)],
                &[
                    (2, 2),
                    (3, 3),
                    (4, 4),
                    (5, 5),
                    (6, 6),
                    (7, 7),
                    (8, 8),
                    (9, 9),
                ],
            ];
            for coords in edges {
                check(16, 16, coords, "edge", &mut a);
            }
            a.phase(
                "edges",
                edges.iter().map(|v| v.len() as u64).sum(),
                "LSB/MSB, byte/row crossings, and append return indices 0..7",
            );

            let n = ctx.scaled(*random);
            let mut rng = Xs(ctx.seed ^ 0x4144_4453_5441_5254);
            let random_before = a.trials;
            for _ in 0..n {
                let width = 2 + (rng.next() as i32 & 0x7e);
                let height = 2 + ((rng.next() >> 8) as i32 & 0x7e);
                let count = ((rng.next() & 7) + 1) as usize;
                let mut coords = [(1, 1); 8];
                for coord in &mut coords[..count] {
                    coord.0 = 1 + (rng.next() as u32 % (width - 1) as u32) as i32;
                    coord.1 = 1 + (rng.next() as u32 % (height - 1) as u32) as i32;
                }
                check(width, height, &coords[..count], "random", &mut a);
            }
            a.phase("random", a.trials - random_before, distribution);
            a.extras.push((
                "fixture_boundary".into(),
                "all four arrays preallocated to 64 WCoord entries; allocator branch untaken"
                    .into(),
            ));
            unsafe { libc::munmap(arena as *mut c_void, ARENA_BYTES) };
        }

        Plan::StartCityRadWcoord {
            random,
            distribution,
        } => {
            const ARENA_BYTES: usize = PAGE * 2;
            const O_WORLD: usize = 0x100;
            const O_CONSTANTS: usize = 0x300;
            const O_X_LIST: usize = 0x1000;
            const O_Y_LIST: usize = 0x1200;
            const O_X: usize = 0x1400;
            const O_Y: usize = 0x1410;
            const VA_WORLD_PTR: u32 = 0x00C0_6188;
            const VA_CONSTANTS_PTR: u32 = 0x00C0_61E4;
            let Some(arena) = scratch_page(ARENA_BYTES) else {
                a.skip = Some("start-city-radius fixture scratch mmap failed".into());
                return a;
            };
            let world = unsafe { arena.add(O_WORLD) };
            let constants = unsafe { arena.add(O_CONSTANTS) };
            let x_list = unsafe { arena.add(O_X_LIST) as *mut i32 };
            let y_list = unsafe { arena.add(O_Y_LIST) as *mut i32 };
            let x_ptr = unsafe { arena.add(O_X) as *mut i32 };
            let y_ptr = unsafe { arena.add(O_Y) as *mut i32 };
            let (Some(world_slot), Some(constants_slot)) =
                (ctx.at(VA_WORLD_PTR), ctx.at(VA_CONSTANTS_PTR))
            else {
                a.skip = Some("World/Constants access pointer VA is outside mapped image".into());
                unsafe { libc::munmap(arena as *mut c_void, ARENA_BYTES) };
                return a;
            };
            unsafe {
                std::ptr::write_unaligned(world_slot as *mut u32, world as usize as u32);
                std::ptr::write_unaligned(constants_slot as *mut u32, constants as usize as u32);
                std::ptr::write_unaligned(world.add(0xc8) as *mut u32, x_list as usize as u32);
                std::ptr::write_unaligned(world.add(0xe4) as *mut u32, y_list as usize as u32);
            }
            let f = f as *const u8;
            let mut model = don_sim::systems::map_terrain::World::init_default_rules(2, 2);
            let mut check = |starts: &[(i32, i32)],
                             x: i32,
                             y: i32,
                             city_center_radius: i32,
                             label: &str,
                             a: &mut Acc| {
                debug_assert!(starts.len() <= 32);
                unsafe {
                    std::ptr::write_unaligned(world.add(0xbc) as *mut i32, starts.len() as i32);
                    for (i, &(sx, sy)) in starts.iter().enumerate() {
                        std::ptr::write_unaligned(x_list.add(i), sx);
                        std::ptr::write_unaligned(y_list.add(i), sy);
                    }
                    std::ptr::write_unaligned(constants.add(0x12c) as *mut i32, city_center_radius);
                    std::ptr::write_unaligned(x_ptr, x);
                    std::ptr::write_unaligned(y_ptr, y);
                }
                model.start_city_x.items.clear();
                model.start_city_y.items.clear();
                model
                    .start_city_x
                    .items
                    .extend(starts.iter().map(|&(sx, _)| sx));
                model
                    .start_city_y
                    .items
                    .extend(starts.iter().map(|&(_, sy)| sy));
                let want = model.start_city_rad_wcoord(
                    don_sim::systems::map_terrain::WCoord(x),
                    don_sim::systems::map_terrain::WCoord(y),
                    city_center_radius,
                ) as i32;
                let got = unsafe { call_start_city_wcoord(f, world, x_ptr, y_ptr) };
                a.trials += 1;
                if got != want {
                    a.mismatches += 1;
                    a.first_detail(format!(
                        "{label} starts={starts:?} query=({x},{y}) \
                         city_center_radius={city_center_radius} \
                         model={want} retail={got}"
                    ));
                }
            };
            let edge_cases: &[(&[(i32, i32)], i32, i32, i32)] = &[
                (&[], 5, 5, 1_024),
                (&[(5, 5)], 5, 5, 1),
                (&[(5, 5)], 5, 5, 2),
                (&[(5, 5)], 7, 5, 9),
                (&[(5, 5)], 7, 5, 10),
                (&[(5, 5), (100, 100)], 99, 100, 5),
                (&[(5, 5), (100, 100)], 99, 100, 6),
                (&[(0, 0), (127, 127)], 64, 64, 0),
            ];
            for &(starts, x, y, city_center_radius) in edge_cases {
                check(starts, x, y, city_center_radius, "edge", &mut a);
            }
            a.phase(
                "edges",
                edge_cases.len() as u64,
                "empty and multiple arrays; exact zero, factor-four, minus-one, and strict-threshold boundaries",
            );
            let n = ctx.scaled(*random);
            let mut rng = Xs(ctx.seed ^ 0x5354_4152_5452_4144);
            for _ in 0..n {
                let count = (rng.next() as usize) & 31;
                let mut starts = [(0i32, 0i32); 32];
                for start in &mut starts[..count] {
                    let r = rng.next();
                    *start = ((r as i32) & 127, ((r >> 8) as i32) & 127);
                }
                let r = rng.next();
                let x = (r as i32) & 127;
                let y = ((r >> 8) as i32) & 127;
                let city_center_radius = ((r >> 16) as i32) & 1023;
                check(&starts[..count], x, y, city_center_radius, "random", &mut a);
            }
            a.phase("random", n as u64, distribution);
            unsafe { libc::munmap(arena as *mut c_void, ARENA_BYTES) };
        }

        Plan::MapFairnessCalcDistances {
            random,
            distribution,
        } => {
            const ARENA_BYTES: usize = PAGE * 2;
            const O_FAIRNESS: usize = 0x100;
            const O_WORLD: usize = 0x300;
            const O_START_X: usize = 0x1000;
            const O_START_Y: usize = 0x1100;
            const O_X: usize = 0x1200;
            const O_Y: usize = 0x1210;
            const VA_WORLD_PTR: u32 = 0x00C0_6188;
            const FAIRNESS_DISTS: usize = 0x24;
            const FAIRNESS_LOWEST_DIST: usize = 0x4c;
            const FAIRNESS_HIGHEST_DIST: usize = 0x50;
            const FAIRNESS_TEAMS: usize = 0x54;
            const FAIRNESS_NUM_PLAYERS: usize = 0x74;

            let Some(arena) = scratch_page(ARENA_BYTES) else {
                a.skip = Some("map-fairness fixture scratch mmap failed".into());
                return a;
            };
            let fairness = unsafe { arena.add(O_FAIRNESS) };
            let world = unsafe { arena.add(O_WORLD) };
            let start_x = unsafe { arena.add(O_START_X) as *mut i32 };
            let start_y = unsafe { arena.add(O_START_Y) as *mut i32 };
            let x_ptr = unsafe { arena.add(O_X) as *mut i32 };
            let y_ptr = unsafe { arena.add(O_Y) as *mut i32 };
            let Some(world_slot) = ctx.at(VA_WORLD_PTR) else {
                a.skip = Some("World access pointer VA is outside mapped image".into());
                unsafe { libc::munmap(arena as *mut c_void, ARENA_BYTES) };
                return a;
            };
            unsafe {
                std::ptr::write_unaligned(world_slot as *mut u32, world as usize as u32);
                std::ptr::write_unaligned(world.add(0x90) as *mut u32, start_x as usize as u32);
                std::ptr::write_unaligned(world.add(0xac) as *mut u32, start_y as usize as u32);
            }

            let f = f as *const u8;
            let mut model_world = don_sim::systems::map_terrain::World::init_default_rules(2, 2);
            let mut check = |starts: &[(i32, i32)],
                             teams: &[i32],
                             x: i32,
                             y: i32,
                             scale: f32,
                             initial_dists: [u32; 8],
                             initial_lowest: i32,
                             initial_highest: i32,
                             label: &str,
                             a: &mut Acc| {
                debug_assert!(starts.len() <= 8);
                debug_assert!(teams.len() <= 8);
                debug_assert!(teams
                    .iter()
                    .all(|&player| player >= 0 && (player as usize) < starts.len()));
                let mut expected_image = [0u8; 120];
                unsafe {
                    // Pattern the entire PDB-sized object so the case proves that
                    // calc_distances preserves every field outside its documented writes.
                    for i in 0..120 {
                        let pattern = (i as u32).wrapping_mul(37).wrapping_add(x as u32)
                            ^ (y as u32).rotate_left(11)
                            ^ scale.to_bits().rotate_left(19);
                        std::ptr::write(fairness.add(i), pattern as u8);
                    }
                    for (i, &(sx, sy)) in starts.iter().enumerate() {
                        std::ptr::write_unaligned(start_x.add(i), sx);
                        std::ptr::write_unaligned(start_y.add(i), sy);
                    }
                    for (i, bits) in initial_dists.iter().copied().enumerate() {
                        std::ptr::write_unaligned(
                            fairness.add(FAIRNESS_DISTS + i * 4) as *mut u32,
                            bits,
                        );
                    }
                    std::ptr::write_unaligned(
                        fairness.add(FAIRNESS_LOWEST_DIST) as *mut i32,
                        initial_lowest,
                    );
                    std::ptr::write_unaligned(
                        fairness.add(FAIRNESS_HIGHEST_DIST) as *mut i32,
                        initial_highest,
                    );
                    for (i, team) in teams.iter().copied().enumerate() {
                        std::ptr::write_unaligned(
                            fairness.add(FAIRNESS_TEAMS + i * 4) as *mut i32,
                            team,
                        );
                    }
                    std::ptr::write_unaligned(
                        fairness.add(FAIRNESS_NUM_PLAYERS) as *mut i32,
                        teams.len() as i32,
                    );
                    std::ptr::write_unaligned(x_ptr, x);
                    std::ptr::write_unaligned(y_ptr, y);
                    std::ptr::copy_nonoverlapping(
                        fairness,
                        expected_image.as_mut_ptr(),
                        expected_image.len(),
                    );
                }

                model_world.start_x.items.clear();
                model_world.start_y.items.clear();
                model_world
                    .start_x
                    .items
                    .extend(starts.iter().map(|&(sx, _)| sx));
                model_world
                    .start_y
                    .items
                    .extend(starts.iter().map(|&(_, sy)| sy));
                let mut model = don_sim::systems::map_terrain::MapFairness {
                    dists: initial_dists.map(f32::from_bits),
                    lowest_dist: initial_lowest,
                    highest_dist: initial_highest,
                    num_players: teams.len() as i32,
                    ..Default::default()
                };
                model.teams[..teams.len()].copy_from_slice(teams);
                model.calc_distances(
                    &model_world,
                    don_sim::systems::map_terrain::WCoord(x),
                    don_sim::systems::map_terrain::WCoord(y),
                    scale,
                );
                for (i, value) in model.dists.iter().enumerate() {
                    expected_image[FAIRNESS_DISTS + i * 4..FAIRNESS_DISTS + (i + 1) * 4]
                        .copy_from_slice(&value.to_bits().to_le_bytes());
                }
                expected_image[FAIRNESS_LOWEST_DIST..FAIRNESS_LOWEST_DIST + 4]
                    .copy_from_slice(&model.lowest_dist.to_le_bytes());
                expected_image[FAIRNESS_HIGHEST_DIST..FAIRNESS_HIGHEST_DIST + 4]
                    .copy_from_slice(&model.highest_dist.to_le_bytes());

                unsafe {
                    call_map_fairness_calc_distances(f, fairness, x_ptr, y_ptr, scale);
                }
                let got_dists: [u32; 8] = std::array::from_fn(|i| unsafe {
                    std::ptr::read_unaligned(fairness.add(FAIRNESS_DISTS + i * 4) as *const u32)
                });
                let got_lowest = unsafe {
                    std::ptr::read_unaligned(fairness.add(FAIRNESS_LOWEST_DIST) as *const i32)
                };
                let got_highest = unsafe {
                    std::ptr::read_unaligned(fairness.add(FAIRNESS_HIGHEST_DIST) as *const i32)
                };
                let want_dists = model.dists.map(f32::to_bits);
                let mut got_image = [0u8; 120];
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        fairness,
                        got_image.as_mut_ptr(),
                        got_image.len(),
                    );
                }
                a.trials += 1;
                if got_image != expected_image {
                    a.mismatches += 1;
                    let first_byte = got_image
                        .iter()
                        .zip(&expected_image)
                        .position(|(got, want)| got != want);
                    a.first_detail(format!(
                        "{label} starts={starts:?} teams={teams:?} query=({x},{y}) \
                         scale={scale:?}/{:#010x} initial_dists={initial_dists:08x?} \
                         model=({want_dists:08x?},{},{}) \
                         retail=({got_dists:08x?},{got_lowest},{got_highest}) \
                         first_object_byte={first_byte:?}",
                        scale.to_bits(),
                        model.lowest_dist,
                        model.highest_dist,
                    ));
                }
            };

            let clean = [0u32; 8];
            check(
                &[],
                &[],
                5,
                5,
                1.0,
                [0x7fc0_0001; 8],
                -7,
                19,
                "empty",
                &mut a,
            );
            check(
                &[(5, 5)],
                &[0],
                5,
                5,
                1.0,
                clean,
                -7,
                19,
                "zero-distance",
                &mut a,
            );
            check(
                &[(2, 2), (8, 2)],
                &[0, 1],
                5,
                2,
                1.0,
                clean,
                -7,
                19,
                "tie",
                &mut a,
            );
            check(
                &[(1, 1), (7, 1), (3, 6)],
                &[2, 0, 1],
                3,
                1,
                0.5,
                [0x3f80_0000; 8],
                -7,
                19,
                "team-order",
                &mut a,
            );
            check(
                &[(1, 1), (127, 127)],
                &[1, 0],
                64,
                64,
                0.0,
                [0x8000_0000; 8],
                -7,
                19,
                "zero-scale",
                &mut a,
            );
            a.phase(
                "edges",
                5,
                "empty preservation, zero distance/scale, strict tie, and shuffled-team writes",
            );

            let n = ctx.scaled(*random);
            let mut rng = Xs(ctx.seed ^ 0x4641_4952_4449_5354);
            for _ in 0..n {
                let count = (rng.next() % 9) as usize;
                let mut starts = [(0i32, 0i32); 8];
                for start in &mut starts[..count] {
                    let r = rng.next();
                    *start = ((r as i32) & 127, ((r >> 8) as i32) & 127);
                }
                let mut teams = [0i32; 8];
                for (i, team) in teams[..count].iter_mut().enumerate() {
                    *team = i as i32;
                }
                for i in (1..count).rev() {
                    let j = (rng.next() as usize) % (i + 1);
                    teams.swap(i, j);
                }
                let mut initial_dists = [0u32; 8];
                for bits in &mut initial_dists {
                    *bits = rng.next() as u32;
                }
                let r = rng.next();
                let x = (r as i32) & 127;
                let y = ((r >> 8) as i32) & 127;
                // Clear the sign bit and exclude exponent 0xff. Subnormal, zero,
                // normal and maximum-finite scales all remain in the corpus.
                let scale = f32::from_bits((rng.next() as u32) & 0x7f7f_ffff);
                check(
                    &starts[..count],
                    &teams[..count],
                    x,
                    y,
                    scale,
                    initial_dists,
                    rng.next() as i32,
                    rng.next() as i32,
                    "random",
                    &mut a,
                );
            }
            a.phase("random", n as u64, distribution);
            a.extras.push((
                "compared_writes".into(),
                "full 120-byte MapFairness image; all 8 dists by f32 bits plus extrema, with every other byte patterned and required unchanged".into(),
            ));
            unsafe { libc::munmap(arena as *mut c_void, ARENA_BYTES) };
        }

        Plan::PlaceStartInRegion {
            random,
            distribution,
        } => {
            const ARENA_BYTES: usize = PAGE * 16;
            const O_MAP: usize = 0x100;
            const O_WORLD: usize = 0x500;
            const O_REGIONS: usize = 0x800;
            const O_REGION_LIST: usize = 0x1000;
            const O_REGION_COORDS: usize = 0x1300;
            const O_DEFAULT_X_ITEMS: usize = 0x1800;
            const O_DEFAULT_Y_ITEMS: usize = 0x1900;
            const O_ALT_X_ARRAY: usize = 0x1a00;
            const O_ALT_Y_ARRAY: usize = 0x1a40;
            const O_ALT_X_ITEMS: usize = 0x1b00;
            const O_ALT_Y_ITEMS: usize = 0x1c00;
            const O_RNG: usize = 0x1d00;
            const O_OUT_X: usize = 0x1d10;
            const O_OUT_Y: usize = 0x1d20;
            const O_WDATA: usize = 0x2000;
            const REGION_BYTES: usize = 136;
            const REGION_SLOTS: usize = 3;
            const VA_WORLD_PTR: u32 = 0x00C0_6188;
            const VA_RNG_PTR: u32 = 0x00C0_6184;
            const VA_REGIONS_PTR: u32 = 0x00C0_61B8;
            const VA_CIRCLE_INIT: u32 = 0x0068_17F0;
            const VA_CIRCLE_COUNT: u32 = 0x00CA_B3A8;
            const VA_CIRCLE_X: u32 = 0x00CB_7E90;
            const VA_CIRCLE_Y: u32 = 0x00CB_B0E0;
            const VA_CIRCLE_END: u32 = 0x00CB_E330;

            if let Err(e) = image::install_fake_teb() {
                a.skip = Some(format!("fake TEB for Map placement RNG: {e}"));
                return a;
            }
            let Some(arena) = scratch_page(ARENA_BYTES) else {
                a.skip = Some("place-start fixture scratch mmap failed".into());
                return a;
            };
            let required = [
                VA_WORLD_PTR,
                VA_RNG_PTR,
                VA_REGIONS_PTR,
                VA_CIRCLE_INIT,
                VA_CIRCLE_COUNT,
                VA_CIRCLE_X,
                VA_CIRCLE_Y,
                VA_CIRCLE_END,
            ];
            let Some(slots) = required
                .iter()
                .map(|&va| ctx.at(va))
                .collect::<Option<Vec<_>>>()
            else {
                a.skip = Some("place-start global/function VA is outside mapped image".into());
                unsafe { libc::munmap(arena as *mut c_void, ARENA_BYTES) };
                return a;
            };
            let [world_slot, rng_slot, regions_slot, circle_init, circle_count, circle_x, circle_y, circle_end]: [*mut u8; 8] =
                slots.try_into().unwrap();

            let sim_circle = don_sim::systems::combat::circle_table();
            unsafe { call_cdecl0(circle_init as *const u8) };
            let retail_count = unsafe { std::ptr::read_unaligned(circle_count as *const i32) };
            let retail_x =
                unsafe { std::slice::from_raw_parts(circle_x as *const i8, sim_circle.x.len()) };
            let retail_y =
                unsafe { std::slice::from_raw_parts(circle_y as *const i8, sim_circle.y.len()) };
            let retail_end = unsafe {
                std::slice::from_raw_parts(circle_end as *const i32, sim_circle.ring_end.len())
            };
            a.trials += 1;
            if retail_count != sim_circle.x.len() as i32
                || retail_x != sim_circle.x
                || retail_y != sim_circle.y
                || retail_end != sim_circle.ring_end
            {
                a.mismatches += 1;
                a.first_detail(format!(
                    "retail circle_init table differs: count={retail_count} model_count={}",
                    sim_circle.x.len()
                ));
                unsafe { libc::munmap(arena as *mut c_void, ARENA_BYTES) };
                return a;
            }
            a.phase(
                "retail-table",
                1,
                "execute circle_init 0x006817f0 and compare all 12,873 x/y entries plus 65 cumulative ring ends",
            );

            let map = unsafe { arena.add(O_MAP) };
            let world = unsafe { arena.add(O_WORLD) };
            let regions = unsafe { arena.add(O_REGIONS) };
            let region_list = unsafe { arena.add(O_REGION_LIST) };
            let region_coords = unsafe { arena.add(O_REGION_COORDS) };
            let alt_x_array = unsafe { arena.add(O_ALT_X_ARRAY) };
            let alt_y_array = unsafe { arena.add(O_ALT_Y_ARRAY) };
            let rng_ptr = unsafe { arena.add(O_RNG) as *mut i32 };
            let out_x = unsafe { arena.add(O_OUT_X) as *mut i32 };
            let out_y = unsafe { arena.add(O_OUT_Y) as *mut i32 };
            let wdata = unsafe { arena.add(O_WDATA) };
            unsafe {
                std::ptr::write_unaligned(world_slot as *mut u32, world as usize as u32);
                std::ptr::write_unaligned(rng_slot as *mut u32, rng_ptr as usize as u32);
                std::ptr::write_unaligned(regions_slot as *mut u32, regions as usize as u32);
            }

            let mut expected = vec![0u8; ARENA_BYTES];
            let mut model_world = don_sim::systems::map_terrain::World::init_default_rules(32, 32);
            let mut successes = 0u64;
            let mut failures = 0u64;
            let mut draws = [0u64; 3];
            let f = f as *const u8;
            let mut check = |width: i32,
                             height: i32,
                             coords: &[(i32, i32)],
                             lands: &[i8],
                             prior: &[(i32, i32)],
                             optional_prior: bool,
                             region_index: usize,
                             min_dist: i32,
                             unread: i32,
                             seed: i32,
                             initial_out: (i32, i32),
                             label: &str,
                             a: &mut Acc| {
                debug_assert!((1..=16).contains(&coords.len()));
                debug_assert_eq!(lands.len(), (width * height) as usize);
                debug_assert!(region_index < REGION_SLOTS);
                let used_end = O_WDATA + lands.len() * 28;
                debug_assert!(used_end <= ARENA_BYTES);

                unsafe {
                    for i in 0..used_end {
                        let pattern = (i as u32).wrapping_mul(73).wrapping_add(seed as u32)
                            ^ (unread as u32).rotate_left(9);
                        std::ptr::write(arena.add(i), pattern as u8);
                    }
                    std::ptr::write_unaligned(world as *mut i32, width);
                    std::ptr::write_unaligned(world.add(4) as *mut i32, height);
                    std::ptr::write_unaligned(world.add(0x134) as *mut u32, wdata as usize as u32);
                    std::ptr::write_unaligned(
                        regions.add(0x10) as *mut u32,
                        region_list as usize as u32,
                    );

                    let region = region_list.add(region_index * REGION_BYTES);
                    std::ptr::write_unaligned(region.add(0x14) as *mut i32, coords.len() as i32);
                    std::ptr::write_unaligned(
                        region.add(0x7c) as *mut u32,
                        region_coords as usize as u32,
                    );
                    for (i, &(x, y)) in coords.iter().enumerate() {
                        std::ptr::write_unaligned(region_coords.add(i * 8) as *mut i32, x);
                        std::ptr::write_unaligned(region_coords.add(i * 8 + 4) as *mut i32, y);
                    }

                    let default_x_array = world.add(0x80);
                    let default_y_array = world.add(0x9c);
                    let write_array =
                        |array: *mut u8, items_ptr: *mut u8, items: &[(i32, i32)], take_x: bool| {
                            std::ptr::write_unaligned(array.add(4) as *mut i32, items.len() as i32);
                            std::ptr::write_unaligned(
                                array.add(0x10) as *mut u32,
                                items_ptr as usize as u32,
                            );
                            for (i, &(x, y)) in items.iter().enumerate() {
                                std::ptr::write_unaligned(
                                    items_ptr.add(i * 4) as *mut i32,
                                    if take_x { x } else { y },
                                );
                            }
                        };
                    let decoy: Vec<(i32, i32)> = if optional_prior {
                        prior
                            .iter()
                            .map(|&(x, y)| ((x + 7) % width, (y + 11) % height))
                            .collect()
                    } else {
                        prior.to_vec()
                    };
                    write_array(default_x_array, arena.add(O_DEFAULT_X_ITEMS), &decoy, true);
                    write_array(default_y_array, arena.add(O_DEFAULT_Y_ITEMS), &decoy, false);
                    write_array(alt_x_array, arena.add(O_ALT_X_ITEMS), prior, true);
                    write_array(alt_y_array, arena.add(O_ALT_Y_ITEMS), prior, false);

                    for (i, &land) in lands.iter().enumerate() {
                        std::ptr::write(wdata.add(i * 28 + 2) as *mut i8, land);
                    }
                    std::ptr::write_unaligned(rng_ptr, seed);
                    std::ptr::write_unaligned(out_x, initial_out.0);
                    std::ptr::write_unaligned(out_y, initial_out.1);
                    std::ptr::copy_nonoverlapping(arena, expected.as_mut_ptr(), used_end);
                }

                model_world.xs = width;
                model_world.ys = height;
                model_world
                    .wdata
                    .resize_with(lands.len(), don_sim::systems::map_terrain::WData::default);
                model_world.wdata.truncate(lands.len());
                for (cell, &land) in model_world.wdata.iter_mut().zip(lands) {
                    cell.land = land;
                }
                let decoy_x: Vec<i32> = if optional_prior {
                    prior.iter().map(|&(x, _)| (x + 7) % width).collect()
                } else {
                    prior.iter().map(|&(x, _)| x).collect()
                };
                let decoy_y: Vec<i32> = if optional_prior {
                    prior.iter().map(|&(_, y)| (y + 11) % height).collect()
                } else {
                    prior.iter().map(|&(_, y)| y).collect()
                };
                model_world.start_x.items = decoy_x;
                model_world.start_y.items = decoy_y;
                let model_coords: Vec<_> = coords
                    .iter()
                    .map(|&(x, y)| {
                        (
                            don_sim::systems::map_terrain::WCoord(x),
                            don_sim::systems::map_terrain::WCoord(y),
                        )
                    })
                    .collect();
                let prior_x: Vec<i32> = prior.iter().map(|&(x, _)| x).collect();
                let prior_y: Vec<i32> = prior.iter().map(|&(_, y)| y).collect();
                let mut model_rng = don_sim::rng::Random::new(seed);
                let model_result = don_sim::systems::map_terrain::place_start_in_region(
                    &model_world,
                    &sim_circle,
                    &mut model_rng,
                    &model_coords,
                    min_dist,
                    unread,
                    optional_prior.then_some((&prior_x, &prior_y)),
                );
                expected[O_RNG..O_RNG + 4].copy_from_slice(&model_rng.state().to_le_bytes());
                if let Some((x, y)) = model_result {
                    expected[O_OUT_X..O_OUT_X + 4].copy_from_slice(&x.0.to_le_bytes());
                    expected[O_OUT_Y..O_OUT_Y + 4].copy_from_slice(&y.0.to_le_bytes());
                }

                let got_result = unsafe {
                    call_place_start_in_region(
                        f,
                        map,
                        region_index as i32,
                        out_x,
                        out_y,
                        min_dist,
                        unread,
                        if optional_prior {
                            alt_x_array
                        } else {
                            std::ptr::null_mut()
                        },
                        if optional_prior {
                            alt_y_array
                        } else {
                            std::ptr::null_mut()
                        },
                    )
                };
                let got = unsafe { std::slice::from_raw_parts(arena, used_end) };
                let want_result = i32::from(model_result.is_some());
                a.trials += 1;
                if got_result != want_result || got != &expected[..used_end] {
                    a.mismatches += 1;
                    let first_byte = got
                        .iter()
                        .zip(&expected[..used_end])
                        .position(|(got, want)| got != want);
                    let got_out = unsafe {
                        (
                            std::ptr::read_unaligned(out_x),
                            std::ptr::read_unaligned(out_y),
                        )
                    };
                    a.first_detail(format!(
                        "{label} {width}x{height} region={region_index} coords={coords:?} \
                         prior={prior:?} optional={optional_prior} min_dist={min_dist} \
                         unread={unread:#x} seed={seed:#x} model={model_result:?}/{} \
                         retail={got_result}/{got_out:?} first_arena_byte={first_byte:?}",
                        model_rng.state(),
                    ));
                }
                if model_result.is_some() {
                    successes += 1;
                } else {
                    failures += 1;
                }
                let step1 = seed
                    .wrapping_mul(don_sim::rng::Random::MUL)
                    .wrapping_add(don_sim::rng::Random::ADD);
                let step2 = step1
                    .wrapping_mul(don_sim::rng::Random::MUL)
                    .wrapping_add(don_sim::rng::Random::ADD);
                let draw_count = if model_rng.state() == seed {
                    0
                } else if model_rng.state() == step1 {
                    1
                } else if model_rng.state() == step2 {
                    2
                } else {
                    unreachable!("place_start consumes at most two draws")
                };
                draws[draw_count] += 1;
            };

            let mut dry32 = vec![0i8; 32 * 32];
            check(
                32,
                32,
                &[(16, 16)],
                &dry32,
                &[],
                false,
                0,
                12,
                0x1234,
                7,
                (-11, -12),
                "single-dry-fallback",
                &mut a,
            );
            let outer = sim_circle.ring_end[8] as usize;
            let ox = 16 + sim_circle.x[outer] as i32;
            let oy = 16 + sim_circle.y[outer] as i32;
            dry32[(oy * 32 + ox) as usize] = 1;
            check(
                32,
                32,
                &[(16, 16)],
                &dry32,
                &[],
                false,
                1,
                12,
                -1,
                9,
                (-21, -22),
                "exact-coastal-band",
                &mut a,
            );
            let ocean24 = vec![2i8; 24 * 24];
            check(
                24,
                24,
                &[(12, 12)],
                &ocean24,
                &[],
                false,
                2,
                0,
                i32::MIN,
                11,
                (101, 102),
                "inner-ocean-reject",
                &mut a,
            );
            let dry20 = vec![0i8; 20 * 20];
            check(
                20,
                20,
                &[(4, 10)],
                &dry20,
                &[],
                false,
                0,
                20,
                i32::MAX,
                13,
                (201, 202),
                "fallback-margin",
                &mut a,
            );
            check(
                24,
                24,
                &[(8, 8), (16, 8), (8, 16), (16, 16)],
                &vec![0i8; 24 * 24],
                &[(8, 8), (16, 8), (8, 16), (16, 16)],
                true,
                2,
                24,
                0x55aa_33cc,
                0x1234_5678,
                (301, 302),
                "optional-prior-total-reject",
                &mut a,
            );
            a.phase(
                "edges",
                5,
                "single/no-draw fallback, exact coastal band, inner-ocean rejection, pass-specific margin, and optional-prior total rejection",
            );

            let n = ctx.scaled(*random);
            let mut rng = Xs(ctx.seed ^ 0x504c_4143_4553_5441);
            for _ in 0..n {
                let width = 16 + (rng.next() % 17) as i32;
                let height = 16 + (rng.next() % 17) as i32;
                let count = 1 + (rng.next() % 16) as usize;
                let mut coords = Vec::with_capacity(count);
                for _ in 0..count {
                    coords.push((
                        (rng.next() % width as u64) as i32,
                        (rng.next() % height as u64) as i32,
                    ));
                }
                let land_mode = (rng.next() & 3) as u8;
                let mut lands = vec![0i8; (width * height) as usize];
                for land in &mut lands {
                    let r = rng.next();
                    *land = match land_mode {
                        0 => 0,
                        1 => i8::from((r & 15) == 0),
                        2 => i8::from((r & 3) == 0) * 2,
                        _ => i8::from((r & 1) == 0),
                    };
                }
                let prior_count = (rng.next() % 9) as usize;
                let mut prior = Vec::with_capacity(prior_count);
                for _ in 0..prior_count {
                    prior.push((
                        (rng.next() % width as u64) as i32,
                        (rng.next() % height as u64) as i32,
                    ));
                }
                let optional_prior = rng.next() & 1 != 0;
                let region_index = (rng.next() % REGION_SLOTS as u64) as usize;
                let min_dist = (rng.next() % 25) as i32;
                let unread = rng.next() as i32;
                let seed = rng.next() as i32;
                let initial_out = (rng.next() as i32, rng.next() as i32);
                check(
                    width,
                    height,
                    &coords,
                    &lands,
                    &prior,
                    optional_prior,
                    region_index,
                    min_dist,
                    unread,
                    seed,
                    initial_out,
                    "random",
                    &mut a,
                );
            }
            a.phase("random", n as u64, distribution);
            a.extras
                .push(("placement_successes".into(), successes.to_string()));
            a.extras
                .push(("placement_failures".into(), failures.to_string()));
            a.extras.push((
                "rng_draw_counts".into(),
                format!("zero={} one={} two={}", draws[0], draws[1], draws[2]),
            ));
            a.extras.push((
                "compared_state".into(),
                "return/out coordinates, final game_random state, and every patterned byte through Map, World, Regions/Region, both array sources, and the complete WData fixture"
                    .into(),
            ));

            // Placement must leave the retail-generated canonical tables unchanged.
            a.trials += 1;
            let preserved = unsafe {
                std::slice::from_raw_parts(circle_x as *const i8, sim_circle.x.len())
                    == sim_circle.x
                    && std::slice::from_raw_parts(circle_y as *const i8, sim_circle.y.len())
                        == sim_circle.y
                    && std::slice::from_raw_parts(
                        circle_end as *const i32,
                        sim_circle.ring_end.len(),
                    ) == sim_circle.ring_end
            };
            if !preserved {
                a.mismatches += 1;
                a.first_detail("place_start_in_region mutated the canonical circle tables".into());
            }
            a.phase(
                "preservation",
                1,
                "all retail-generated circle table bytes unchanged after every placement call",
            );
            unsafe { libc::munmap(arena as *mut c_void, ARENA_BYTES) };
        }

        Plan::Damage {
            seeds,
            trials_per_seed,
            distribution,
        } => {
            let mut arena = match damage_env::Arena::new() {
                Ok(x) => x,
                Err(e) => {
                    a.skip = Some(format!("cannot build the fabricated world: {e}"));
                    return a;
                }
            };
            let base = ctx.m.base;
            let ib = ctx.pe.image_base;
            let reloc = |va: u32| unsafe { base.add((va - ib) as usize) } as u32;
            let wr32 = |va: u32, v: u32| unsafe {
                std::ptr::write_unaligned(base.add((va - ib) as usize) as *mut u32, v)
            };
            let wr16 = |va: u32, v: u16| unsafe {
                std::ptr::write_unaligned(base.add((va - ib) as usize) as *mut u16, v)
            };
            damage_env::build(&mut arena, &reloc);
            damage_test::install_globals(&arena, &wr32);
            let damage_fn = reloc(damage_env::VA_DAMAGE);

            let per = ctx.scaled(*trials_per_seed);
            let (mut de, mut coll, mut panics) = (0u64, 0u64, 0u64);
            let mut coverage = [0u64; 30];
            for &seed in seeds.iter() {
                let rep = damage_test::run(&arena, damage_fn, per, seed, &wr32, &wr16);
                a.trials += rep.trials as u64;
                a.mismatches += rep.mismatches as u64;
                de += rep.skipped_de as u64;
                coll += rep.skipped_collide as u64;
                panics += rep.unexpected_panics as u64;
                for (i, c) in rep.coverage.iter().enumerate() {
                    coverage[i] += *c as u64;
                }
                if let Some((s, e, g)) = &rep.first_bad {
                    a.first_detail(one_line(&format!(
                        "seed {seed:#x} model={e} retail={g} scenario={s:?}"
                    )));
                }
                if let Some((s, m)) = &rep.first_panic {
                    a.first_detail(one_line(&format!(
                        "seed {seed:#x} UNEXPECTED PANIC {m} scenario={s:?}"
                    )));
                }
                a.phase(
                    "random",
                    rep.trials as u64,
                    &format!("seed {seed:#018x}: {distribution}"),
                );
            }
            a.exclude("retail #DE (unchecked idiv)", de);
            a.exclude("balance write would collide with harness-owned .data", coll);
            // An unexpected panic is the port faulting where retail would not. Counting it
            // as an exclusion is exactly how a suite launders a divergence into a pass, so
            // it lands in the mismatch column instead.
            if panics > 0 {
                a.mismatches += panics;
                a.extras
                    .push(("unexpected_panics".into(), panics.to_string()));
            }
            // Report which guarded steps never ran. "0 mismatches" over a corpus that only
            // ever executed the spine would be a green suite that tested nothing.
            let never: Vec<&str> = don_sim::STEP_NAMES
                .iter()
                .enumerate()
                .filter(|(i, _)| coverage[*i] == 0)
                .map(|(_, n)| *n)
                .collect();
            a.extras.push((
                "steps_never_taken".into(),
                if never.is_empty() {
                    String::from("(none)")
                } else {
                    never.join(",")
                },
            ));
            let cov: Vec<String> = don_sim::STEP_NAMES
                .iter()
                .enumerate()
                .map(|(i, n)| format!("{n}:{}", coverage[i]))
                .collect();
            a.extras.push(("step_coverage".into(), cov.join(" ")));
        }

        Plan::RngNextFloat {
            edge_seeds,
            random_seeds,
            total_steps,
            distribution,
        } => {
            if let Err(e) = image::install_fake_teb() {
                a.skip = Some(format!("fake TEB: {e}"));
                return a;
            }
            let Some(p) = scratch_page(PAGE) else {
                a.skip = Some("scratch mmap failed".into());
                return a;
            };
            let p = p as *mut u32;
            let f = f as *const u8;
            let mut rng = Xs(ctx.seed);
            let mut seeds: Vec<u32> = edge_seeds.to_vec();
            for _ in 0..*random_seeds {
                seeds.push(rng.next() as u32);
            }
            let steps = (ctx.scaled(*total_steps) as usize / seeds.len().max(1)).max(16);
            let mut bad_state = 0u64;
            let mut bad_float = 0u64;
            for s0 in &seeds {
                let mut model_s = *s0;
                let mut retail_s = *s0;
                for _ in 0..steps {
                    unsafe { std::ptr::write_volatile(p, retail_s) };
                    let (got_s, got_f) = unsafe { call_next_float(f, p) };
                    let want_f = models::rng::next_float(&mut model_s);
                    a.trials += 1;
                    if got_s != model_s {
                        bad_state += 1;
                        a.first_detail(format!(
                            "state: in={retail_s:#010x} model={model_s:#010x} retail={got_s:#010x}"
                        ));
                    }
                    if got_f.to_bits() != want_f.to_bits() {
                        bad_float += 1;
                        a.first_detail(format!(
                            "float: in={retail_s:#010x} model={:#010x} retail={:#010x}",
                            want_f.to_bits(),
                            got_f.to_bits()
                        ));
                    }
                    retail_s = got_s;
                    model_s = got_s; // resynchronise so one divergence does not cascade
                }
            }
            a.mismatches += bad_state + bad_float;
            a.extras
                .push(("state_mismatches".into(), bad_state.to_string()));
            a.extras
                .push(("float_mismatches".into(), bad_float.to_string()));
            a.phase(
                "chained-walk",
                a.trials,
                &format!("{} seeds x {steps} steps: {distribution}", seeds.len()),
            );
            unsafe { libc::munmap(p as *mut c_void, PAGE) };
        }

        Plan::RngInRange {
            edges,
            random,
            wide,
            distribution,
        } => {
            if let Err(e) = image::install_fake_teb() {
                a.skip = Some(format!("fake TEB: {e}"));
                return a;
            }
            if *wide {
                // The once-per-session warning at 0x00A39DB7 calls into unconstructed
                // globals. Force the flag so the arithmetic path runs.
                let Some(flag) = ctx.at(0x00EE_13A8) else {
                    a.skip = Some("warning flag VA 0x00EE13A8 outside the mapped image".into());
                    return a;
                };
                unsafe { std::ptr::write_volatile(flag, 1u8) };
            }
            let Some(p) = scratch_page(PAGE) else {
                a.skip = Some("scratch mmap failed".into());
                return a;
            };
            let p = p as *mut u32;
            let f = f as *const u8;
            let check = |s0: u32, lo: i32, hi: i32, a: &mut Acc, tag: &str| {
                unsafe {
                    std::ptr::write_volatile(p, s0);
                    std::ptr::write_volatile(p.add(1), lo as u32);
                    std::ptr::write_volatile(p.add(2), hi as u32);
                }
                let got = unsafe { call_in_range(f, p) };
                let got_s = unsafe { std::ptr::read_volatile(p) };
                let mut model_s = s0;
                let want = models::rng::in_range(&mut model_s, lo, hi);
                a.trials += 1;
                if got != want || got_s != model_s {
                    a.mismatches += 1;
                    a.first_detail(format!(
                        "{tag} state={s0:#010x} lo={lo} hi={hi} model=({want},{model_s:#010x}) \
                         retail=({got},{got_s:#010x})"
                    ));
                }
            };
            for &(s0, lo, hi) in edges.iter() {
                check(s0, lo, hi, &mut a, "edge");
            }
            a.phase(
                "edges",
                edges.len() as u64,
                "empty/inverted ranges, negatives, the 16-bit boundary",
            );
            let n = ctx.scaled(*random);
            let mut rng = Xs(ctx.seed);
            for _ in 0..n {
                let r = rng.next();
                let s0 = r as u32;
                let (lo, hi) = if *wide {
                    (((r >> 32) as i32) >> 2, (rng.next() as i32) >> 2)
                } else {
                    (
                        ((r >> 32) as i32) % 0x1_0000,
                        ((rng.next() >> 11) as i32) % 0x1_0000,
                    )
                };
                check(s0, lo, hi, &mut a, "random");
            }
            a.phase("random", n as u64, distribution);
            unsafe { libc::munmap(p as *mut c_void, PAGE) };
        }

        Plan::Fastcall2 {
            model,
            edges,
            dists,
        } => {
            let f = f as *const u8;
            let call = |a: i32, b: i32| -> i32 {
                let r: i32;
                unsafe {
                    std::arch::asm!("call {f}", f = in(reg) f,
                        in("ecx") a, in("edx") b, lateout("eax") r, clobber_abi("C"));
                }
                r
            };
            for &(a_, b_) in edges.iter() {
                let want = model(a_, b_);
                let got = call(a_, b_);
                a.trials += 1;
                if want != got {
                    a.mismatches += 1;
                    a.first_detail(format!("edge a={a_} b={b_} model={want} retail={got}"));
                }
            }
            a.phase(
                "edges",
                edges.len() as u64,
                "zero, ±1, the 0xEA60 guard from both sides, i32::MIN/MAX in every combination",
            );
            let mut rng = Xs(ctx.seed);
            for p in dists.iter() {
                let n = match p.dist {
                    Dist2::Full { count } => ctx.scaled(count),
                    Dist2::Centered { count, .. } => ctx.scaled(count),
                    Dist2::Straddle { count, .. } => ctx.scaled(count),
                };
                for _ in 0..n {
                    let r = rng.next();
                    let (x, y) = match p.dist {
                        Dist2::Full { .. } => (r as i32, (r >> 32) as i32),
                        Dist2::Centered { half, .. } => {
                            let m = (half as i64 * 2 + 1) as u64;
                            (
                                ((r % m) as i64 - half as i64) as i32,
                                (((r >> 20) % m) as i64 - half as i64) as i32,
                            )
                        }
                        Dist2::Straddle { center, span, .. } => {
                            let s = span as u64 + 1;
                            let x = center.wrapping_add((r % s) as i32);
                            let y = center.wrapping_add(((r >> 8) % s) as i32);
                            if r & 0x8000_0000 != 0 {
                                (-x, y)
                            } else {
                                (x, -y)
                            }
                        }
                    };
                    let want = model(x, y);
                    let got = call(x, y);
                    a.trials += 1;
                    if want != got {
                        a.mismatches += 1;
                        a.first_detail(format!("a={x} b={y} model={want} retail={got}"));
                    }
                }
                a.phase("random", n as u64, p.description);
            }
        }

        Plan::Adler32 {
            model,
            boundary_lens,
            bufcap,
            random_cases,
            distribution,
        } => {
            let Some(buf) = scratch_page(*bufcap) else {
                a.skip = Some("checksum buffer mmap failed".into());
                return a;
            };
            let f = f as *const u8;
            // `ret`, not `ret 4`: the retail call site does `add esp, 4`, so the caller
            // cleans. Getting this backwards corrupts the harness stack silently.
            let call = |init: u32, len: usize| -> u32 {
                let r: u32;
                unsafe {
                    std::arch::asm!(
                        "push {len:e}",
                        "call {f}",
                        "add esp, 4",
                        f = in(reg) f,
                        len = in(reg) len as u32,
                        in("ecx") init,
                        in("edx") buf,
                        lateout("eax") r,
                        clobber_abi("C"),
                    );
                }
                r
            };
            let mut rng = Xs(ctx.seed);
            let total = boundary_lens.len() as u32 + ctx.scaled(*random_cases);
            for i in 0..total {
                let len = if (i as usize) < boundary_lens.len() {
                    boundary_lens[i as usize]
                } else {
                    (rng.next() as usize) % (*bufcap + 1)
                };
                let init = if i == 0 { 1 } else { rng.next() as u32 };
                unsafe {
                    for k in 0..len {
                        *buf.add(k) = (rng.next() >> 23) as u8;
                    }
                }
                let want = model(init, unsafe { std::slice::from_raw_parts(buf, len) });
                let got = call(init, len);
                a.trials += 1;
                if want != got {
                    a.mismatches += 1;
                    a.first_detail(format!(
                        "len={len} init={init:#010x} model={want:#010x} retail={got:#010x}"
                    ));
                }
            }
            a.phase(
                "boundary-lengths",
                boundary_lens.len() as u64,
                "lengths straddling the 16-byte unrolled block and NMAX = 5552",
            );
            a.phase("random", ctx.scaled(*random_cases) as u64, distribution);
            // The NULL-buffer short-circuit: `lea eax,[edx+1]` = 1.
            let nullret: u32;
            unsafe {
                std::arch::asm!(
                    "push 0", "call {f}", "add esp, 4",
                    f = in(reg) f, in("ecx") 12345u32, in("edx") 0u32,
                    lateout("eax") nullret, clobber_abi("C"));
            }
            a.trials += 1;
            if nullret != 1 {
                a.mismatches += 1;
                a.first_detail(format!(
                    "null-buffer call returned {nullret}, disassembly predicts 1"
                ));
            }
            a.phase(
                "edges",
                1,
                "buf == NULL, which the disassembly says returns 1",
            );
            unsafe { libc::munmap(buf as *mut c_void, *bufcap) };
        }

        Plan::AsScaled {
            scales,
            edges,
            corpus_file,
            generated,
            distribution,
        } => {
            // Patch the two CRT imports the tokenizer calls. Their slots live in a
            // read-only section, so make exactly those pages writable — mapping the whole
            // image RW would change the environment every other case measures in.
            for slot_va in [models::tokenizer::IAT_WTOI, models::tokenizer::IAT_WCSCHR] {
                let Some(slot) = ctx.at(slot_va) else {
                    a.skip = Some(format!("IAT slot {slot_va:#010x} outside the mapped image"));
                    return a;
                };
                if let Err(e) = ctx.m.make_page_writable(slot) {
                    a.skip = Some(format!("cannot make the IAT writable: {e}"));
                    return a;
                }
            }
            unsafe {
                let w = ctx.at(models::tokenizer::IAT_WTOI).unwrap() as *mut u32;
                let c = ctx.at(models::tokenizer::IAT_WCSCHR).unwrap() as *mut u32;
                std::ptr::write_unaligned(w, models::tokenizer::wtoi as *const () as usize as u32);
                std::ptr::write_unaligned(
                    c,
                    models::tokenizer::wcschr as *const () as usize as u32,
                );
            }
            let Some(obj) = scratch_page(PAGE) else {
                a.skip = Some("scratch mmap failed".into());
                return a;
            };
            let f = f as *const u8;

            // The port panics exactly where retail's idiv raises #DE. Evaluate the model
            // first so those inputs are excluded before retail is ever asked, and count
            // them; handing a #DE to the child would be a SIGFPE, not a measurement.
            let prev_hook = std::panic::take_hook();
            std::panic::set_hook(Box::new(|_| {}));
            let mut de = 0u64;
            let mut run = |s: &str, scale: i32, a: &mut Acc, tag: &str| {
                let want = match std::panic::catch_unwind(|| don_rules::as_scaled(s, scale)) {
                    Ok(v) => v,
                    Err(_) => {
                        de += 1;
                        return;
                    }
                };
                let w = models::tokenizer::to_utf16z(s);
                let got = unsafe {
                    std::ptr::write_unaligned(obj as *mut u32, w.as_ptr() as usize as u32);
                    std::ptr::write_unaligned(obj.add(6) as *mut u16, 0u16);
                    std::ptr::write_unaligned(obj.add(8) as *mut u16, (w.len() - 1) as u16);
                    *obj.add(0x0a) = 1;
                    call_thiscall1(f, obj, scale)
                };
                a.trials += 1;
                if want != got {
                    a.mismatches += 1;
                    a.first_detail(format!(
                        "{tag} {s:?} scale={scale} model={want} retail={got}"
                    ));
                }
            };

            match std::fs::read_to_string(corpus_file) {
                Ok(xml) => {
                    let mut corpus: Vec<String> = Vec::new();
                    for chunk in xml.split("value=\"").skip(1) {
                        if let Some(e) = chunk.find('"') {
                            corpus.push(chunk[..e].to_string());
                        }
                    }
                    for chunk in xml.split("entry").skip(1) {
                        if let Some(q) = chunk.find("=\"") {
                            if let Some(e) = chunk[q + 2..].find('"') {
                                corpus.push(chunk[q + 2..q + 2 + e].to_string());
                            }
                        }
                    }
                    let mut n = 0u64;
                    for v in &corpus {
                        for &s in scales.iter() {
                            run(v, s, &mut a, "corpus");
                            n += 1;
                        }
                    }
                    a.phase(
                        "shipped-corpus",
                        n,
                        &format!(
                            "{} value/entry strings from {corpus_file} x {} scales",
                            corpus.len(),
                            scales.len()
                        ),
                    );
                }
                Err(e) => {
                    // Exhaustive-over-the-shipped-corpus is the load-bearing half of this
                    // claim. Running only the generated half and printing PASS would be a
                    // weaker measurement wearing the same label.
                    std::panic::set_hook(prev_hook);
                    a.skip = Some(format!(
                        "shipped corpus {corpus_file} is missing ({e}); the claim is \
                         'exhaustive over the shipped corpus', so a partial run is a \
                         different claim and is not reported as one"
                    ));
                    return a;
                }
            }

            let mut n = 0u64;
            for e in edges.iter() {
                for &s in scales.iter() {
                    run(e, s, &mut a, "edge");
                    n += 1;
                }
            }
            a.phase("edges", n, "hand-chosen tokenizer edges x every scale");

            let count = ctx.scaled(*generated);
            let tails = [
                "",
                " tile",
                " tiles (comment)",
                " frames",
                "%",
                " resources",
                " x",
                "/",
            ];
            let mut rng = Xs(ctx.seed);
            for _ in 0..count {
                let r = rng.next();
                let num = ((r % 4001) as i64 - 2000) as i32;
                let den = (((r >> 20) % 401) as i64 - 200) as i32;
                let tail = tails[((r >> 40) % tails.len() as u64) as usize];
                let s = if (r >> 50) & 1 == 0 {
                    format!("{num}/{den}{tail}")
                } else {
                    format!("{num}{tail}")
                };
                let sc = scales[((r >> 55) as usize) % scales.len()];
                run(&s, sc, &mut a, "generated");
            }
            a.phase("generated", count as u64, distribution);
            std::panic::set_hook(prev_hook);
            a.exclude("retail #DE (INT_MIN / -1 after the multiply)", de);
            unsafe { libc::munmap(obj as *mut c_void, PAGE) };
        }
    }
    a
}

// ---------------------------------------------------------------------------------
// Fork isolation
// ---------------------------------------------------------------------------------

/// Run one case in a forked child and bring its record back over a pipe.
///
/// The child owns whatever environment surgery its case needs. If it dies, the parent
/// reports CRASHED with the signal rather than losing the case.
fn run_isolated(ctx: &Ctx, c: &Case) -> CaseResult {
    let t0 = Instant::now();
    let mut fds = [0i32; 2];
    if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
        return CaseResult {
            wall_ms: t0.elapsed().as_millis(),
            ..CaseResult::skipped(c.id, "pipe() failed")
        };
    }
    std::io::stdout().flush().ok();
    std::io::stderr().flush().ok();
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        return CaseResult {
            wall_ms: t0.elapsed().as_millis(),
            ..CaseResult::skipped(c.id, "fork() failed")
        };
    }
    if pid == 0 {
        unsafe { libc::close(fds[0]) };
        let a = exec(ctx, c);
        let mut out = String::new();
        match &a.skip {
            Some(why) => {
                out.push_str("status=skip\n");
                out.push_str(&format!("detail={}\n", one_line(why)));
            }
            None => {
                out.push_str(if a.mismatches == 0 {
                    "status=pass\n"
                } else {
                    "status=fail\n"
                });
                out.push_str(&format!("trials={}\n", a.trials));
                out.push_str(&format!("mismatches={}\n", a.mismatches));
                for p in &a.phases {
                    out.push_str(&format!(
                        "phase={}|{}|{}\n",
                        p.kind,
                        p.count,
                        one_line(&p.description)
                    ));
                }
                for e in &a.excluded {
                    out.push_str(&format!("excluded={}|{}\n", one_line(&e.reason), e.count));
                }
                for (k, v) in &a.extras {
                    out.push_str(&format!("extra={}|{}\n", k, one_line(v)));
                }
                if !a.detail.is_empty() {
                    out.push_str(&format!("detail={}\n", one_line(&a.detail)));
                }
            }
        }
        let b = out.as_bytes();
        let mut off = 0usize;
        while off < b.len() {
            let n =
                unsafe { libc::write(fds[1], b[off..].as_ptr() as *const c_void, b.len() - off) };
            if n <= 0 {
                break;
            }
            off += n as usize;
        }
        unsafe { libc::close(fds[1]) };
        unsafe { libc::_exit(0) };
    }

    unsafe { libc::close(fds[1]) };
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = unsafe { libc::read(fds[0], chunk.as_mut_ptr() as *mut c_void, chunk.len()) };
        if n <= 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n as usize]);
    }
    unsafe { libc::close(fds[0]) };
    let mut status = 0i32;
    unsafe { libc::waitpid(pid, &mut status, 0) };
    let wall_ms = t0.elapsed().as_millis();

    if libc::WIFSIGNALED(status) {
        return CaseResult {
            id: c.id,
            status: Status::Crashed,
            trials: 0,
            mismatches: 0,
            phases: Vec::new(),
            excluded: Vec::new(),
            detail: format!(
                "child killed by signal {} — the case executed retail code that faulted, so \
                 it produced no measurement",
                libc::WTERMSIG(status)
            ),
            extras: Vec::new(),
            wall_ms,
        };
    }

    let text = String::from_utf8_lossy(&buf).to_string();
    let mut r = CaseResult {
        id: c.id,
        status: Status::Error,
        trials: 0,
        mismatches: 0,
        phases: Vec::new(),
        excluded: Vec::new(),
        detail: String::new(),
        extras: Vec::new(),
        wall_ms,
    };
    let mut saw_status = false;
    for line in text.lines() {
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        match k {
            "status" => {
                saw_status = true;
                r.status = match v {
                    "pass" => Status::Pass,
                    "fail" => Status::Fail,
                    "skip" => Status::Skipped,
                    _ => Status::Error,
                };
            }
            "trials" => r.trials = v.parse().unwrap_or(0),
            "mismatches" => r.mismatches = v.parse().unwrap_or(0),
            "detail" => r.detail = v.to_string(),
            "phase" => {
                let f: Vec<&str> = v.splitn(3, '|').collect();
                if f.len() == 3 {
                    r.phases.push(Phase {
                        kind: f[0].into(),
                        count: f[1].parse().unwrap_or(0),
                        description: f[2].into(),
                    });
                }
            }
            "excluded" => {
                if let Some((reason, n)) = v.rsplit_once('|') {
                    r.excluded.push(Excluded {
                        reason: reason.into(),
                        count: n.parse().unwrap_or(0),
                    });
                }
            }
            "extra" => {
                if let Some((kk, vv)) = v.split_once('|') {
                    r.extras.push((kk.into(), vv.into()));
                }
            }
            _ => {}
        }
    }
    if !saw_status {
        r.status = Status::Error;
        r.detail = format!(
            "child exited without reporting a status ({} bytes of output) — treated as a \
             failure, never as a pass",
            buf.len()
        );
    }
    r
}

// ---------------------------------------------------------------------------------
// The suite
// ---------------------------------------------------------------------------------

pub struct RunReport {
    pub results: Vec<CaseResult>,
    pub selftest: Result<(), String>,
    pub image_path: String,
    pub image_sha256: String,
    pub image_bytes: usize,
    pub scale: f64,
    pub seed: u64,
    pub only: Option<String>,
    pub started_unix: u64,
    pub wall_ms: u128,
}

impl RunReport {
    pub fn count(&self, s: Status) -> usize {
        self.results.iter().filter(|r| r.status == s).count()
    }
    pub fn total_trials(&self) -> u64 {
        self.results.iter().map(|r| r.trials).sum()
    }
    /// `0` all ran and passed, `1` mismatch/crash/error, `2` something skipped,
    /// `3` the harness could not start.
    pub fn exit_code(&self) -> i32 {
        if self.selftest.is_err() {
            return 3;
        }
        if self
            .results
            .iter()
            .any(|r| matches!(r.status, Status::Fail | Status::Crashed | Status::Error))
        {
            return 1;
        }
        if self.results.iter().any(|r| r.status == Status::Skipped) {
            return 2;
        }
        0
    }
}

pub fn run_all(
    m: &Mapped,
    pe: &PeImage,
    image_path: &str,
    image_bytes: &[u8],
    scale: f64,
    seed: u64,
    only: Option<&str>,
) -> RunReport {
    let t0 = Instant::now();
    let started_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    println!(
        "oracle regression suite — {} registered cases",
        REGISTRY.len()
    );
    let selftest = image::selftest();
    match &selftest {
        Ok(()) => println!(
            "  selftest      OK (hand-written cdecl add returned 42 through fork isolation)"
        ),
        Err(e) => println!("  selftest      FAILED: {e} — no case can be reported as passing"),
    }
    let sha = image::sha256_hex(image_bytes);
    println!(
        "  image         {image_path}  sha256 {sha}  {} bytes",
        image_bytes.len()
    );
    println!("  scale {scale}   seed {seed:#018x}");
    println!();

    let ctx = Ctx { m, pe, scale, seed };
    let mut results = Vec::new();
    for c in REGISTRY.iter() {
        if let Some(f) = only {
            if !f.split(',').any(|id| id == c.id) {
                results.push(CaseResult::skipped(
                    c.id,
                    &format!("not selected by --only {f}"),
                ));
                continue;
            }
        }
        if selftest.is_err() {
            results.push(CaseResult::skipped(
                c.id,
                "harness selftest failed; refusing to report a result",
            ));
            continue;
        }
        let r = run_isolated(&ctx, c);
        print_case(c, &r);
        results.push(r);
    }

    RunReport {
        results,
        selftest,
        image_path: image_path.to_string(),
        image_sha256: sha,
        image_bytes: image_bytes.len(),
        scale,
        seed,
        only: only.map(|s| s.to_string()),
        started_unix,
        wall_ms: t0.elapsed().as_millis(),
    }
}

fn print_case(c: &Case, r: &CaseResult) {
    println!(
        "  {:<5} {:<26} {:#010x}  {:>10} trials  {:>6} mismatches  {:>6} ms",
        r.status.label(),
        c.id,
        c.va,
        r.trials,
        r.mismatches,
        r.wall_ms
    );
    println!("        model  {}", c.model);
    for p in &r.phases {
        println!(
            "        phase  {:<16} {:>10}  {}",
            p.kind, p.count, p.description
        );
    }
    for e in &r.excluded {
        println!("        excl   {:>10}  {}", e.count, e.reason);
    }
    for (k, v) in &r.extras {
        if k == "step_coverage" {
            continue;
        }
        println!("        {k}: {v}");
    }
    if !r.detail.is_empty() {
        println!("        {}", r.detail);
    }
}

pub fn print_summary(rep: &RunReport) {
    println!();
    println!("summary");
    println!(
        "  {} pass   {} fail   {} skipped   {} crashed   {} error",
        rep.count(Status::Pass),
        rep.count(Status::Fail),
        rep.count(Status::Skipped),
        rep.count(Status::Crashed),
        rep.count(Status::Error),
    );
    println!("  {} trials in {} ms", rep.total_trials(), rep.wall_ms);
    if rep.count(Status::Skipped) > 0 {
        println!(
            "  SKIPPED cases produced NO evidence. They are not passes, and this run exits \
             non-zero because of them."
        );
    }
    println!();
    println!(
        "  {} Tier-B claims are outside this suite entirely (see known_gaps in the JSON):",
        KNOWN_GAPS.len()
    );
    for g in KNOWN_GAPS.iter() {
        println!("    - {}", g.claim);
    }
    println!();
    println!(
        "  Tier B is testing, not verification. Every number above is a sample count over the \
         stated distribution and says nothing about inputs outside it."
    );
}

// ---------------------------------------------------------------------------------
// JSON
// ---------------------------------------------------------------------------------

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 8);
    for ch in s.chars() {
        match ch {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o
}

pub fn write_json(rep: &RunReport, path: &str) -> std::io::Result<()> {
    let mut s = String::new();
    let host = std::fs::read_to_string("/proc/sys/kernel/hostname")
        .unwrap_or_default()
        .trim()
        .to_string();

    s.push_str("{\n");
    s.push_str("  \"schema\": \"don/oracle-regression\",\n");
    s.push_str("  \"schema_version\": 1,\n");
    s.push_str(&format!("  \"generated_unix\": {},\n", rep.started_unix));
    s.push_str(&format!("  \"host\": \"{}\",\n", esc(&host)));
    s.push_str("  \"target\": \"i686-unknown-linux-musl\",\n");
    s.push_str(&format!(
        "  \"harness_selftest\": {},\n",
        match &rep.selftest {
            Ok(()) => "\"pass\"".to_string(),
            Err(e) => format!("\"fail: {}\"", esc(e)),
        }
    ));
    s.push_str("  \"image\": {\n");
    s.push_str(&format!("    \"path\": \"{}\",\n", esc(&rep.image_path)));
    s.push_str(&format!("    \"sha256\": \"{}\",\n", rep.image_sha256));
    s.push_str(&format!("    \"bytes\": {}\n", rep.image_bytes));
    s.push_str("  },\n");
    s.push_str(&format!("  \"seed\": \"{:#018x}\",\n", rep.seed));
    s.push_str(&format!("  \"scale\": {},\n", rep.scale));
    s.push_str(&format!(
        "  \"only\": {},\n",
        match &rep.only {
            Some(f) => format!("\"{}\"", esc(f)),
            None => "null".into(),
        }
    ));
    s.push_str(&format!("  \"wall_ms\": {},\n", rep.wall_ms));
    s.push_str(&format!("  \"exit_code\": {},\n", rep.exit_code()));
    s.push_str("  \"summary\": {\n");
    s.push_str(&format!("    \"registered\": {},\n", REGISTRY.len()));
    s.push_str(&format!("    \"pass\": {},\n", rep.count(Status::Pass)));
    s.push_str(&format!("    \"fail\": {},\n", rep.count(Status::Fail)));
    s.push_str(&format!(
        "    \"skipped\": {},\n",
        rep.count(Status::Skipped)
    ));
    s.push_str(&format!(
        "    \"crashed\": {},\n",
        rep.count(Status::Crashed)
    ));
    s.push_str(&format!("    \"error\": {},\n", rep.count(Status::Error)));
    s.push_str(&format!("    \"total_trials\": {}\n", rep.total_trials()));
    s.push_str("  },\n");

    s.push_str("  \"cases\": [\n");
    for (i, c) in REGISTRY.iter().enumerate() {
        let r = rep
            .results
            .iter()
            .find(|r| r.id == c.id)
            .expect("every registered case has a result");
        s.push_str("    {\n");
        s.push_str(&format!("      \"id\": \"{}\",\n", esc(c.id)));
        s.push_str(&format!("      \"va\": \"{:#010x}\",\n", c.va));
        s.push_str(&format!("      \"abi\": \"{}\",\n", esc(c.abi)));
        s.push_str(&format!("      \"model\": \"{}\",\n", esc(c.model)));
        s.push_str(&format!("      \"subsystem\": \"{}\",\n", esc(c.subsystem)));
        s.push_str(&format!("      \"ledger_entry\": \"{}\",\n", esc(c.ledger)));
        s.push_str(&format!(
            "      \"derivation\": \"{}\",\n",
            esc(c.derivation)
        ));
        s.push_str(&format!(
            "      \"reachability\": \"{}\",\n",
            esc(c.reachability)
        ));
        s.push_str(&format!("      \"caveat\": \"{}\",\n", esc(c.caveat)));
        s.push_str("      \"tier\": \"B\",\n");
        s.push_str(&format!("      \"status\": \"{}\",\n", r.status.as_str()));
        s.push_str(&format!("      \"trials\": {},\n", r.trials));
        s.push_str(&format!("      \"mismatches\": {},\n", r.mismatches));
        s.push_str(&format!("      \"wall_ms\": {},\n", r.wall_ms));
        s.push_str("      \"phases\": [");
        for (j, p) in r.phases.iter().enumerate() {
            if j > 0 {
                s.push(',');
            }
            s.push_str(&format!(
                "\n        {{\"kind\": \"{}\", \"count\": {}, \"distribution\": \"{}\"}}",
                esc(&p.kind),
                p.count,
                esc(&p.description)
            ));
        }
        s.push_str(if r.phases.is_empty() {
            "],\n"
        } else {
            "\n      ],\n"
        });
        s.push_str("      \"excluded\": [");
        for (j, e) in r.excluded.iter().enumerate() {
            if j > 0 {
                s.push(',');
            }
            s.push_str(&format!(
                "\n        {{\"reason\": \"{}\", \"count\": {}}}",
                esc(&e.reason),
                e.count
            ));
        }
        s.push_str(if r.excluded.is_empty() {
            "],\n"
        } else {
            "\n      ],\n"
        });
        s.push_str("      \"extras\": {");
        for (j, (k, v)) in r.extras.iter().enumerate() {
            if j > 0 {
                s.push(',');
            }
            s.push_str(&format!("\n        \"{}\": \"{}\"", esc(k), esc(v)));
        }
        s.push_str(if r.extras.is_empty() {
            "},\n"
        } else {
            "\n      },\n"
        });
        s.push_str(&format!(
            "      \"detail\": {}\n",
            if r.detail.is_empty() {
                "null".to_string()
            } else {
                format!("\"{}\"", esc(&r.detail))
            }
        ));
        s.push_str(if i + 1 == REGISTRY.len() {
            "    }\n"
        } else {
            "    },\n"
        });
    }
    s.push_str("  ],\n");

    s.push_str("  \"known_gaps\": [\n");
    for (i, g) in KNOWN_GAPS.iter().enumerate() {
        s.push_str(&format!(
            "    {{\"claim\": \"{}\", \"why\": \"{}\"}}{}\n",
            esc(g.claim),
            esc(g.why),
            if i + 1 == KNOWN_GAPS.len() { "" } else { "," }
        ));
    }
    s.push_str("  ],\n");
    s.push_str(
        "  \"note\": \"Tier B is differential testing, not verification. Each count is a \
         sample size over the stated distribution. A case with status != 'pass' produced no \
         evidence and must not be cited as if it had.\"\n",
    );
    s.push_str("}\n");

    std::fs::write(path, s)
}
