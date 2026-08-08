//! RNG probe: differential test of the engine's `Random` class against a Rust model.
//!
//! Separate binary (not a case inside `main.rs`) so this lane does not collide with other
//! lanes editing the main oracle. The image-mapping glue is duplicated from `main.rs`;
//! the real PE work lives in `don-pe`, so the duplication is ~50 lines of mmap/mprotect.
//!
//! Targets, all derived in `docs/derivation/rng.md`:
//!   0x00a39cf0  Random::next_float()          __thiscall(this=&state) -> f32 in [0,1), xmm0
//!   0x00a39d70  Random::in_range(lo, hi)      __thiscall(this=&state), stdcall args, ret 8
//!
//! Both read and write only `*(u32*)this`, so a 4-byte scratch buffer is a complete
//! environment for them. `in_range` sets up an SEH frame (writes fs:[0] and restores it);
//! on i686 Linux that is the TLS self-pointer, saved and restored by the retail prologue
//! / epilogue, and nothing in the fast path reads TLS in between.
//!
//! Usage (on hbox, i686-unknown-linux-musl, from ~/don-oracle):
//!   ./target/i686-unknown-linux-musl/debug/rng vectors
//!   ./target/i686-unknown-linux-musl/debug/rng difftest 1000000

use don_pe::PeImage;
use std::ffi::c_void;

const PAGE: usize = 4096;

const VA_NEXT_FLOAT: u32 = 0x00a3_9cf0;
const VA_IN_RANGE: u32 = 0x00a3_9d70;

fn round_up(v: usize, to: usize) -> usize {
    (v + to - 1) & !(to - 1)
}

struct Mapped {
    base: *mut u8,
    len: usize,
}

impl Mapped {
    fn load(bytes: &[u8]) -> Result<(Mapped, PeImage), String> {
        let pe = PeImage::parse(bytes).map_err(|e| e.to_string())?;
        if !pe.has_relocations() {
            return Err("image has no relocations".into());
        }
        let len = round_up(pe.size_of_image as usize, PAGE);
        let base = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            )
        };
        if base == libc::MAP_FAILED {
            return Err(format!("mmap failed: {}", std::io::Error::last_os_error()));
        }
        let base = base as *mut u8;
        let image = pe.map(bytes).map_err(|e| e.to_string())?;
        unsafe { std::ptr::copy_nonoverlapping(image.as_ptr(), base, image.len()) };
        let actual = base as usize;
        if actual > u32::MAX as usize {
            return Err(format!("mapping landed above 4GiB at {actual:#x}"));
        }
        let slice = unsafe { std::slice::from_raw_parts_mut(base, len) };
        let fixups = pe.relocate(slice, actual as u32).map_err(|e| e.to_string())?;
        eprintln!("[rng] mapped at {actual:#010x}, {fixups} relocations applied");
        for s in &pe.sections {
            let start = s.virtual_address as usize;
            let size = round_up(s.virtual_size.max(1) as usize, PAGE);
            if start + size > len {
                continue;
            }
            let prot = if s.is_executable() {
                libc::PROT_READ | libc::PROT_EXEC
            } else if s.is_writable() {
                libc::PROT_READ | libc::PROT_WRITE
            } else {
                libc::PROT_READ
            };
            if unsafe { libc::mprotect(base.add(start) as *mut c_void, size, prot) } != 0 {
                return Err(format!("mprotect {} failed", s.name));
            }
        }
        Ok((Mapped { base, len }, pe))
    }

    fn addr_of_va(&self, pe: &PeImage, va: u32) -> *const u8 {
        unsafe { self.base.add((va - pe.image_base) as usize) }
    }
}

impl Drop for Mapped {
    fn drop(&mut self) {
        unsafe { libc::munmap(self.base as *mut c_void, self.len) };
    }
}

/// MSVC x86 functions with an SEH frame execute `mov eax, fs:[0]` / `mov fs:[0], eax` --
/// the Windows TEB exception-registration chain. On i386 Linux, TLS lives in `%gs` and
/// `%fs` is left as a null selector, so any `fs:` access faults (this cost one SIGSEGV to
/// discover: `oracle call` on 0x00a39d70 dies in the prologue, not in the RNG math).
///
/// Fix: allocate a page, install it as a segment base via `set_thread_area(2)`, and load
/// the resulting selector into `%fs`. Then `fs:[0]` is ordinary writable memory. This is
/// the general prerequisite for oracle-calling *any* SEH-carrying retail function, not
/// just this one.
#[repr(C)]
struct UserDesc {
    entry_number: u32,
    base_addr: u32,
    limit: u32,
    flags: u32,
}

fn install_fake_teb() {
    const SYS_SET_THREAD_AREA: libc::c_long = 243;
    let page = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            PAGE,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
            -1,
            0,
        )
    };
    assert!(page != libc::MAP_FAILED, "TEB mmap failed");
    // fs:[0] = end-of-SEH-chain sentinel, exactly as Windows initialises it.
    unsafe { std::ptr::write_volatile(page as *mut u32, 0xffff_ffff) };
    let mut d = UserDesc {
        entry_number: u32::MAX, // let the kernel pick a free GDT slot
        base_addr: page as u32,
        limit: 0x000f_ffff,
        // seg_32bit(0) | limit_in_pages(4) | useable(6); data, read/write, present
        flags: 0x51,
    };
    let rc = unsafe { libc::syscall(SYS_SET_THREAD_AREA, &mut d as *mut UserDesc) };
    assert!(rc == 0, "set_thread_area failed: {}", std::io::Error::last_os_error());
    let sel: u16 = ((d.entry_number << 3) | 3) as u16;
    unsafe { std::arch::asm!("mov fs, {0:x}", in(reg) sel, options(nomem, nostack)) };
    eprintln!("[rng] fake TEB at {page:p}, %fs = {sel:#06x}");
}

/// `__thiscall` with no stack args; float result comes back in xmm0.
///
/// Returns (new state read back out of the scratch buffer, xmm0).
unsafe fn call_next_float(f: *const u8, p: *mut u32) -> (u32, f32) {
    let out_f: f32;
    std::arch::asm!(
        "call {f:e}",
        f = in(reg) f as u32,
        in("ecx") p,
        out("eax") _,
        out("edx") _,
        lateout("xmm0") out_f,
        out("xmm1") _, out("xmm2") _, out("xmm3") _,
        out("xmm4") _, out("xmm5") _, out("xmm6") _, out("xmm7") _,
    );
    (std::ptr::read_volatile(p), out_f)
}

/// `__thiscall` + two stdcall dword args. Callee is `ret 8`, so it cleans its own args.
/// `lo` and `hi` are read out of the scratch block so the asm needs only two registers.
///
/// Layout of `p`: [0] = state, [1] = lo, [2] = hi.
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

// ---------------------------------------------------------------------------
// The Rust model. Nothing here is hand-tuned to make a test pass: it is a direct
// transcription of the instruction sequence at 0x00a39cf0 / 0x00a39ea5, and the
// differential test below is what decides whether the transcription is right.
// ---------------------------------------------------------------------------

/// `imul r, [this], 0x19660D ; add r, 0x3C6EF35F` — Numerical Recipes `ranqd1`.
fn lcg_step(s: u32) -> u32 {
    s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223)
}

/// `Random::next_float()` at 0x00a39cf0.
fn model_next_float(s: &mut u32) -> f32 {
    *s = lcg_step(*s);
    let bits = (*s & 0x007f_ffff) | 0x3f80_0000;
    // The retail code widens to double, subtracts 1.0, narrows back. Both conversions
    // and the subtraction are exact for a value in [1,2), so a single-precision
    // subtraction is bit-identical — which is what the difftest checks.
    f32::from_bits(bits) - 1.0f32
}

/// `Random::in_range(lo, hi)` at 0x00a39d70, fast path (|lo|,|hi| <= 0xFFFF).
fn model_in_range(s: &mut u32, lo: i32, hi: i32) -> i32 {
    if lo == hi {
        return lo; // early-out at 0x00a39e85: the state is NOT advanced
    }
    let (lo, hi) = if lo > hi { (hi, lo) } else { (lo, hi) }; // xor-swap at 0x00a39e9f
    let range = (hi as u32).wrapping_sub(lo as u32); // sub edi, ebx
    *s = lcg_step(*s);
    let low16 = *s & 0xffff; // movzx eax, cx
    let prod = low16.wrapping_mul(range); // imul eax, edi
    ((prod >> 16) as i32).wrapping_add(lo) // shr eax,0x10 ; add eax, ebx
}

fn scratch() -> *mut u32 {
    let p = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            PAGE,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
            -1,
            0,
        )
    };
    assert!(p != libc::MAP_FAILED, "scratch mmap failed");
    p as *mut u32
}

struct Xs(u64);
impl Xs {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

fn difftest(m: &Mapped, pe: &PeImage, trials: u32) -> bool {
    let p = scratch();
    let f_float = m.addr_of_va(pe, VA_NEXT_FLOAT);
    let f_range = m.addr_of_va(pe, VA_IN_RANGE);
    let mut rng = Xs(0x2545_f491_4f6c_dd1d);
    let mut ok = true;

    // ---- 1. LCG state recurrence + float mapping, via Random::next_float ----
    {
        let mut bad_state = 0u32;
        let mut bad_float = 0u32;
        let mut first: Option<(u32, u32, u32, f32, f32)> = None;
        let mut n = 0u32;
        // Edge seeds first (0 is the value every statically-constructed Random starts at),
        // then a long random walk that also exercises state chaining.
        let mut seeds: Vec<u32> = vec![0, 1, 0xffff_ffff, 0x8000_0000, 0x7fff_ffff, 0x3c6e_f35f];
        for _ in 0..64 {
            seeds.push(rng.next() as u32);
        }
        for s0 in seeds {
            let mut model_s = s0;
            let mut retail_s = s0;
            let steps = (trials / 70).max(16);
            for _ in 0..steps {
                unsafe { std::ptr::write_volatile(p, retail_s) };
                let (got_s, got_f) = unsafe { call_next_float(f_float, p) };
                let want_f = model_next_float(&mut model_s);
                n += 1;
                if got_s != model_s {
                    bad_state += 1;
                    if first.is_none() {
                        first = Some((retail_s, model_s, got_s, want_f, got_f));
                    }
                }
                if got_f.to_bits() != want_f.to_bits() {
                    bad_float += 1;
                    if first.is_none() {
                        first = Some((retail_s, model_s, got_s, want_f, got_f));
                    }
                }
                retail_s = got_s;
                model_s = got_s; // resynchronise so one divergence does not cascade
            }
        }
        if bad_state == 0 && bad_float == 0 {
            println!("  PASS  {VA_NEXT_FLOAT:#010x}  Random::next_float  {n} trials, 0 state mismatches, 0 float mismatches");
        } else {
            ok = false;
            println!("  FAIL  {VA_NEXT_FLOAT:#010x}  Random::next_float  state {bad_state}/{n}, float {bad_float}/{n}, first {first:?}");
        }
    }

    // ---- 2. in_range(lo, hi) ----
    {
        let mut bad = 0u32;
        let mut n = 0u32;
        let mut first: Option<(u32, i32, i32, i32, i32, u32, u32)> = None;
        let mut check = |s0: u32, lo: i32, hi: i32| {
            unsafe {
                std::ptr::write_volatile(p, s0);
                std::ptr::write_volatile(p.add(1), lo as u32);
                std::ptr::write_volatile(p.add(2), hi as u32);
            }
            let got = unsafe { call_in_range(f_range, p) };
            let got_s = unsafe { std::ptr::read_volatile(p) };
            let mut model_s = s0;
            let want = model_in_range(&mut model_s, lo, hi);
            n += 1;
            if got != want || got_s != model_s {
                bad += 1;
                if first.is_none() {
                    first = Some((s0, lo, hi, want, got, model_s, got_s));
                }
            }
        };
        // Hand-chosen edges: empty range, inverted range, negatives, the 16-bit boundary.
        for (s0, lo, hi) in [
            (0u32, 0i32, 0i32),
            (0, 0, 1),
            (0, 1, 0),
            (0, 0, 0xffff),
            (0, -0xffff, 0xffff),
            (0, -5, 5),
            (0, 5, -5),
            (0xffff_ffff, 0, 100),
            (0x3c6e_f35f, -1, 1),
            (12345, 0xffff, 0xffff),
            (12345, 0xffff, 0),
            (99, -32768, 32767),
        ] {
            check(s0, lo, hi);
        }
        // Random: state anywhere in u32, bounds inside +/-0xFFFF so the warning branch at
        // 0x00a39db7 (which calls into live globals) is never taken.
        for _ in 0..trials {
            let r = rng.next();
            let s0 = r as u32;
            let lo = ((r >> 32) as i32) % 0x1_0000;
            let hi = ((rng.next() >> 11) as i32) % 0x1_0000;
            check(s0, lo, hi);
        }
        if bad == 0 {
            println!("  PASS  {VA_IN_RANGE:#010x}  Random::in_range    {n} trials, 0 mismatches");
        } else {
            ok = false;
            println!("  FAIL  {VA_IN_RANGE:#010x}  Random::in_range    {bad}/{n} mismatched, first {first:?}");
        }
    }

    unsafe { libc::munmap(p as *mut c_void, PAGE) };
    ok
}

/// Print retail outputs for fixed inputs so test expectations are *captured*, never
/// hand-computed (CHARTER: capture, do not calculate).
fn vectors(m: &Mapped, pe: &PeImage) {
    let p = scratch();
    let f_float = m.addr_of_va(pe, VA_NEXT_FLOAT);
    let f_range = m.addr_of_va(pe, VA_IN_RANGE);

    println!("// Random::next_float @ 0x00a39cf0 -- (seed_in) -> (seed_out, f32 bits, f32)");
    for s0 in [0u32, 1, 0x3c6e_f35f, 0xffff_ffff, 0x8000_0000, 5489] {
        unsafe { std::ptr::write_volatile(p, s0) };
        let (s1, f) = unsafe { call_next_float(f_float, p) };
        println!("next_float({s0:#010x}) -> state {s1:#010x}, {:#010x} ({f:?})", f.to_bits());
    }
    println!();
    println!("// first 12 draws of a Random seeded 0 -- the sequence a fresh static Random emits");
    unsafe { std::ptr::write_volatile(p, 0) };
    let mut seq = Vec::new();
    for _ in 0..12 {
        let (s, _) = unsafe { call_next_float(f_float, p) };
        seq.push(format!("{s:#010x}"));
    }
    println!("{}", seq.join(", "));
    println!();
    println!("// Random::in_range @ 0x00a39d70 -- (seed_in, lo, hi) -> (result, seed_out)");
    for (s0, lo, hi) in [
        (0u32, 0i32, 0i32),
        (0, 0, 1),
        (0, 1, 0),
        (0, 0, 100),
        (0, 1, 6),
        (0, -5, 5),
        (0, 5, -5),
        (0, 0, 0xffff),
        (12345, 0, 10),
        (0xdead_beef, -1000, 1000),
    ] {
        unsafe {
            std::ptr::write_volatile(p, s0);
            std::ptr::write_volatile(p.add(1), lo as u32);
            std::ptr::write_volatile(p.add(2), hi as u32);
        }
        let r = unsafe { call_in_range(f_range, p) };
        let s1 = unsafe { std::ptr::read_volatile(p) };
        println!("in_range({s0:#010x}, {lo}, {hi}) -> {r}, state {s1:#010x}");
    }

    println!();
    println!("// distribution check: 200000 draws of in_range(0, 10) from seed 1");
    let mut hist = [0u32; 16];
    unsafe {
        std::ptr::write_volatile(p, 1);
        std::ptr::write_volatile(p.add(1), 0);
        std::ptr::write_volatile(p.add(2), 10);
    }
    for _ in 0..200_000 {
        let r = unsafe { call_in_range(f_range, p) };
        if (0..16).contains(&r) {
            hist[r as usize] += 1;
        }
    }
    println!("{hist:?}");

    unsafe { libc::munmap(p as *mut c_void, PAGE) };
}

/// What `in_range` does when a bound exceeds 0xFFFF -- the case the retail code itself
/// warns about at 0x00a39db7. The warning is "once per session", gated on the byte at
/// VA 0x00ee13a8; we set it to 1 so the warning branch is skipped and the arithmetic path
/// runs without calling into unconstructed global objects.
fn out_of_range(m: &Mapped, pe: &PeImage, trials: u32) -> bool {
    let flag = m.addr_of_va(pe, 0x00ee_13a8) as *mut u8;
    unsafe { std::ptr::write_volatile(flag, 1) };
    let p = scratch();
    let f_range = m.addr_of_va(pe, VA_IN_RANGE);
    let mut rng = Xs(0x9e37_79b9_7f4a_7c15);
    let mut bad = 0u32;
    let mut n = 0u32;
    let mut first = None;
    for _ in 0..trials {
        let r = rng.next();
        let s0 = r as u32;
        let lo = (r >> 32) as i32 >> 2; // full-width, far past 0xFFFF
        let hi = rng.next() as i32 >> 2;
        unsafe {
            std::ptr::write_volatile(p, s0);
            std::ptr::write_volatile(p.add(1), lo as u32);
            std::ptr::write_volatile(p.add(2), hi as u32);
        }
        let got = unsafe { call_in_range(f_range, p) };
        let got_s = unsafe { std::ptr::read_volatile(p) };
        let mut model_s = s0;
        let want = model_in_range(&mut model_s, lo, hi);
        n += 1;
        if got != want || got_s != model_s {
            bad += 1;
            if first.is_none() {
                first = Some((s0, lo, hi, want, got));
            }
        }
    }
    println!("// out-of-range sample: in_range with |bounds| up to 2^29");
    for (s0, lo, hi) in [(1u32, 0i32, 1_000_000i32), (1, 0, 100_000), (1, -1_000_000, 1_000_000)] {
        unsafe {
            std::ptr::write_volatile(p, s0);
            std::ptr::write_volatile(p.add(1), lo as u32);
            std::ptr::write_volatile(p.add(2), hi as u32);
        }
        let got = unsafe { call_in_range(f_range, p) };
        println!("in_range({s0:#010x}, {lo}, {hi}) -> {got}");
    }
    if bad == 0 {
        println!("  PASS  {VA_IN_RANGE:#010x}  in_range out-of-range  {n} trials, 0 mismatches");
    } else {
        println!("  FAIL  {VA_IN_RANGE:#010x}  in_range out-of-range  {bad}/{n}, first {first:?}");
    }
    unsafe { libc::munmap(p as *mut c_void, PAGE) };
    bad == 0
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let bytes = std::fs::read("data/riseofnations.exe").expect("read data/riseofnations.exe");
    let (m, pe) = Mapped::load(&bytes).expect("load");
    install_fake_teb();
    match args.get(1).map(|s| s.as_str()).unwrap_or("vectors") {
        "vectors" => vectors(&m, &pe),
        "difftest" => {
            let n: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(100_000);
            println!("differential test: retail Random vs Rust model (Tier B -- testing, not proof)");
            if !difftest(&m, &pe, n) {
                std::process::exit(1);
            }
        }
        "oob" => {
            let n: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(100_000);
            if !out_of_range(&m, &pe, n) {
                std::process::exit(1);
            }
        }
        other => {
            eprintln!("usage: rng [vectors | difftest N]  (got {other})");
            std::process::exit(2);
        }
    }
}
