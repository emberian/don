//! Drive Rise of Nations' own BHS script compiler out of the retail image.
//!
//! Ground truth is the binary. Everything here executes shipped machine code; nothing is
//! reimplemented. All VAs are static (image base 0x00400000) and were taken from
//! `ron-bin/sbl/rise.pdb` [measured].
//!
//! Build/run (hbox only — this is 32-bit x86 code):
//!
//! ```sh
//! cd ~/don-bhs-oracle/crates/don-bhs/oracle
//! nice -n 15 taskset -c 0-3 cargo build --target i686-unknown-linux-musl -q
//! ./target/i686-unknown-linux-musl/debug/bhsoracle opnames
//! ```

#![allow(static_mut_refs)]

mod image;
mod win;

use image::{guard_for, Fault, Mapped};
use std::io::Write;

// --- retail VAs [measured, rise.pdb] ------------------------------------------------
const VA_OPSTRING_INIT: u32 = 0x0041_3670; // dynamic initializer for 'OpCode::op_string'
const VA_OPSTRING: u32 = 0x00ed_5b08; // OpCode::op_string[73], String[20]
const VA_GET_OP_NAME: u32 = 0x004c_f5d0; // static String OpCode::get_op_name(int)
const VA_XC_A: u32 = 0x00ac_57c8; // ___xc_a
const VA_XC_Z: u32 = 0x00ac_6934; // ___xc_z
const VA_XI_A: u32 = 0x00ac_6938; // ___xi_a
const VA_XI_Z: u32 = 0x00ac_6948; // ___xi_z
const VA_STRING_CTOR_CHAR: u32 = 0x00a1_d660; // String::String(char const*)
const VA_STRING_DTOR: u32 = 0x00a1_cf40; // String::~String
const VA_COMPILER: u32 = 0x00eb_6a90; // Compiler script_compiler
const VA_COMPILE: u32 = 0x009b_f160; // int Compiler::compile(String const&, ScriptReloadType)
const VA_COMPILER_INIT: u32 = 0x009b_ea00; // void Compiler::init()
const VA_SGI_PTR: u32 = 0x00ca_b36c; // ScriptGameInterfaceBase* script_game_interface
const VA_SGI_OBJ: u32 = 0x00eb_1ac8; // ScriptGameInterface real_script_game_interface
const VA_SGI_INIT: u32 = 0x009e_1a20; // ScriptGameInterface::init()
const VA_STRINGTABLE_PTR: u32 = 0x00c0_6378; // StringTable* int_str_array
const VA_ERRSYS_MSGS: u32 = 0x00c8_cd00; // gErrorSystem+0x18: String* message table
const VA_SCRIPT_FILES: u32 = 0x00c8_cba0; // PtrArray<ScriptFile> ScriptFile::script_files
const VA_EMPTY_STRING: u32 = 0x00eb_437c; // String const EMPTY_STRING
const VA_COMP_ERROR: u32 = 0x009b_ed80; // void Compiler::comp_error(String const&, int, int)
const VA_YYERROR: u32 = 0x009b_a370; // int yyerror(char const*)

// ---------------------------------------------------------------------------------
// Calling conventions into the image
// ---------------------------------------------------------------------------------

#[inline(never)]
unsafe fn call_ecx_edx(f: u32, ecx: u32, edx: u32) -> u32 {
    let r: u32;
    std::arch::asm!(
        "call {f:e}",
        f = in(reg) f,
        in("ecx") ecx,
        in("edx") edx,
        lateout("eax") r,
        clobber_abi("C"),
    );
    r
}

#[inline(never)]
unsafe fn cdecl0(f: u32) -> u32 {
    let r: u32;
    std::arch::asm!("call {f:e}", f = in(reg) f, lateout("eax") r, clobber_abi("C"));
    r
}

#[inline(never)]
unsafe fn thiscall0(f: u32, this: u32) -> u32 {
    let r: u32;
    std::arch::asm!(
        "call {f:e}",
        f = in(reg) f, in("ecx") this,
        lateout("eax") r, clobber_abi("C"),
    );
    r
}

/// `__thiscall` with one pushed dword; the callee pops it (`ret 4`).
#[inline(never)]
unsafe fn thiscall1(f: u32, this: u32, a: u32) -> u32 {
    let r: u32;
    std::arch::asm!(
        "push {a:e}",
        "call {f:e}",
        a = in(reg) a, f = in(reg) f, in("ecx") this,
        lateout("eax") r, clobber_abi("C"),
    );
    r
}

/// `__thiscall` with two pushed dwords, right-to-left; the callee pops them (`ret 8`).
#[inline(never)]
unsafe fn thiscall2(f: u32, this: u32, a: u32, b: u32) -> u32 {
    let r: u32;
    std::arch::asm!(
        "push {b:e}",
        "push {a:e}",
        "call {f:e}",
        a = in(reg) a, b = in(reg) b, f = in(reg) f, in("ecx") this,
        lateout("eax") r, clobber_abi("C"),
    );
    r
}

// ---------------------------------------------------------------------------------
// Reading a retail `String` (20 bytes) [layout measured from rise.pdb]
//   +0  data / const_string   +4 const_len:u16  +6 offset:u16  +8 curr_len:u16
//   +10 flags:u8  +11 module_id:u8  +12 hash:u32  +16 hash_insensitive:u32
// ---------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
struct RStr {
    ptr: u32,
    const_len: u16,
    offset: u16,
    curr_len: u16,
    flags: u8,
    module_id: u8,
    hash: u32,
    hash_i: u32,
}

fn read_string(p: *const u8) -> RStr {
    unsafe {
        RStr {
            ptr: std::ptr::read_unaligned(p as *const u32),
            const_len: std::ptr::read_unaligned(p.add(4) as *const u16),
            offset: std::ptr::read_unaligned(p.add(6) as *const u16),
            curr_len: std::ptr::read_unaligned(p.add(8) as *const u16),
            flags: *p.add(10),
            module_id: *p.add(11),
            hash: std::ptr::read_unaligned(p.add(12) as *const u32),
            hash_i: std::ptr::read_unaligned(p.add(16) as *const u32),
        }
    }
}

/// Best-effort text of a retail String. `ptr` is a live address, not a VA.
///
/// `String::data` is a pointer to a small heap block whose first dword is the `wchar_t*`
/// buffer and whose second is the length: `String::init_const` does `mov ecx,[this];
/// mov ecx,[ecx]` before handing that to `String::char_to_wchar`. Some Strings alias the
/// buffer directly, so try the double dereference first and fall back to the single one,
/// accepting whichever decodes to printable UTF-16.
fn string_text(s: &RStr) -> String {
    if s.ptr == 0 {
        return String::new();
    }
    let n = if s.curr_len != 0 {
        s.curr_len as usize
    } else {
        s.const_len as usize
    };
    if n == 0 || n > 4096 {
        return String::new();
    }
    let try_at = |addr: u32| -> Option<String> {
        if addr < 0x1000 {
            return None;
        }
        let base = addr as usize + (s.offset as usize) * 2;
        let mut w = Vec::with_capacity(n);
        for i in 0..n {
            let c = unsafe { std::ptr::read_unaligned((base + i * 2) as *const u16) };
            if c == 0 || c > 0x2000 {
                return None;
            }
            w.push(c);
        }
        Some(String::from_utf16_lossy(&w))
    };
    let inner = unsafe { std::ptr::read_unaligned(s.ptr as *const u32) };
    try_at(inner)
        .or_else(|| try_at(s.ptr))
        .unwrap_or_else(|| format!("<undecoded ptr={:#010x} len={n}>", s.ptr))
}

fn hexdump(p: *const u8, n: usize) -> String {
    let mut s = String::new();
    for i in 0..n {
        s.push_str(&format!("{:02x}", unsafe { *p.add(i) }));
        if i % 4 == 3 {
            s.push(' ');
        }
    }
    s
}

fn report_fault(what: &str, m: &Mapped, f: &Fault) {
    let va = m
        .va_of(f.eip as usize)
        .map(|v| format!("{v:#010x}"))
        .unwrap_or_else(|| format!("(outside image) {:#010x}", f.eip));
    let mut e = std::io::stderr();
    let _ = writeln!(
        e,
        "  FAULT {what}: {} at VA {va} touching {:#010x}",
        image::signame(f.signo),
        f.addr
    );
    let _ = writeln!(
        e,
        "        eax={:08x} ecx={:08x} edx={:08x} ebx={:08x} esi={:08x} edi={:08x} esp={:08x} ebp={:08x}",
        f.eax, f.ecx, f.edx, f.ebx, f.esi, f.edi, f.esp, f.ebp
    );
    if m.va_of(f.eip as usize).is_some() {
        let _ = writeln!(
            e,
            "        bytes at eip: {}",
            hexdump(f.eip as *const u8, 16)
        );
    }
    // Scan the stack for words that land inside the image: on x86 a return address is a
    // plain pushed dword, so this recovers the call chain without unwind information and
    // turns "jumped to garbage" into "jumped to garbage from <named function>".
    let mut chain = Vec::new();
    // An ESCAPE carries no register state, and reading a bogus %esp here would fault
    // *outside* any guarded region and take the whole run down.
    for i in 0..if f.esp > 0x1000 { 64usize } else { 0 } {
        let w = unsafe { std::ptr::read_unaligned((f.esp as usize + i * 4) as *const u32) };
        if let Some(va) = m.va_of(w as usize) {
            if va > 0x0040_1000 && va < 0x00ac_5000 {
                chain.push(format!("{va:#010x}"));
            }
        }
        if chain.len() >= 12 {
            break;
        }
    }
    if !chain.is_empty() {
        let _ = writeln!(e, "        stack -> image VAs: {}", chain.join(" "));
    }
    let tail = image::trace_tail(48);
    if !tail.is_empty() {
        let _ = writeln!(
            e,
            "        last {} instructions ({} stepped total):",
            tail.len(),
            unsafe { image::TRACE_N }
        );
        for chunk in tail.chunks(6) {
            let s: Vec<String> = chunk
                .iter()
                .map(|p| match m.va_of(*p as usize) {
                    Some(v) => format!("{v:#010x}"),
                    None => format!("[{p:#010x}]"),
                })
                .collect();
            let _ = writeln!(e, "          {}", s.join(" "));
        }
    }
}

// ---------------------------------------------------------------------------------

fn load() -> Mapped {
    let path = std::env::var("RON_EXE").unwrap_or_else(|_| "data/riseofnations.exe".into());
    let bytes = std::fs::read(&path).unwrap_or_else(|e| {
        eprintln!("cannot read {path}: {e}");
        std::process::exit(3);
    });
    eprintln!("[bhs] {} bytes from {path}", bytes.len());
    Mapped::load(&bytes).unwrap_or_else(|e| {
        eprintln!("map failed: {e}");
        std::process::exit(3);
    })
}

fn setup(m: &Mapped) -> win::Env {
    image::install_fault_handler();
    if let Err(e) = image::install_fake_teb(Some(m)) {
        eprintln!("fake TEB failed: {e}");
        std::process::exit(3);
    }
    let env = win::install(m);
    eprintln!(
        "[bhs] IAT: {} imports, {} bound, {} trapped",
        env.imports.len(),
        env.implemented,
        env.trapped
    );
    env
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("help");
    match cmd {
        "info" => cmd_info(),
        "imports" => cmd_imports(&args),
        "opnames" => cmd_opnames(),
        "initterm" => cmd_initterm(&args),
        "compile" => cmd_compile(&args),
        _ => {
            eprintln!(
                "usage: bhsoracle <info|imports|opnames|initterm|compile <file.bhs>>\n\
                 env: RON_EXE=path/to/riseofnations.exe  BHS_TRACE_IO=1"
            );
        }
    }
}

fn cmd_info() {
    let m = load();
    for s in &m.pe.sections {
        println!(
            "{:8} VA {:#010x} vsize {:#x}",
            s.name,
            s.virtual_address + image::IMAGE_BASE,
            s.virtual_size
        );
    }
    let imports = win::parse_imports(&m);
    println!("{} imports", imports.len());
    println!("XC initialisers: {}", (VA_XC_Z - VA_XC_A) / 4);
}

fn cmd_imports(args: &[String]) {
    let m = load();
    let env = setup(&m);
    let only_unbound = args.iter().any(|a| a == "--unbound");
    for im in &env.imports {
        if only_unbound && im.bound.is_some() {
            continue;
        }
        println!(
            "{:<40} {:<16} slot {:#010x}",
            format!("{}!{}", im.dll, im.name),
            im.bound.unwrap_or("TRAP"),
            im.slot_va
        );
    }
}

// ---------------------------------------------------------------------------------
// Rung 1: the opcode table
// ---------------------------------------------------------------------------------

fn cmd_opnames() {
    let m = load();
    let _env = setup(&m);

    print!(
        "[bhs] running `dynamic initializer for OpCode::op_string` @ {VA_OPSTRING_INIT:#010x} ... "
    );
    let _ = std::io::stdout().flush();
    let f = m.at(VA_OPSTRING_INIT) as u32;
    match guard_for(10, || unsafe {
        cdecl0(f);
    }) {
        Ok(()) => println!("ok"),
        Err(fault) => {
            println!("FAULT");
            report_fault("op_string initializer", &m, &fault);
            std::process::exit(1);
        }
    }

    println!("\n-- OpCode::op_string[] read directly from .data --");
    for i in 0..73u32 {
        let p = m.at(VA_OPSTRING + i * 20);
        let s = read_string(p);
        println!("{i:3}  {:<24?} raw={}", string_text(&s), hexdump(p, 20));
    }

    println!("\n-- OpCode::get_op_name(i) executed --");
    let getf = m.at(VA_GET_OP_NAME) as u32;
    let mut out = [0u8; 32];
    for i in 0..74u32 {
        let r = guard_for(5, || unsafe {
            out = [0u8; 32];
            call_ecx_edx(getf, out.as_mut_ptr() as u32, i);
        });
        match r {
            Ok(()) => {
                let s = read_string(out.as_ptr());
                println!("{i:3}  {:?}", string_text(&s));
                // release the copy so the refcount, if any, stays balanced
                let _ = guard_for(5, || unsafe {
                    thiscall0(m.at(VA_STRING_DTOR) as u32, out.as_ptr() as u32);
                });
            }
            Err(fault) => {
                println!("{i:3}  FAULT");
                report_fault("get_op_name", &m, &fault);
                break;
            }
        }
    }
}

// ---------------------------------------------------------------------------------
// Rung 2: the C++ dynamic initialisers
// ---------------------------------------------------------------------------------

fn run_initializers(m: &Mapped, verbose: bool) -> (usize, usize, Vec<(u32, Fault)>) {
    let mut ok = 0usize;
    let mut faulted = Vec::new();
    let mut total = 0usize;
    // The `.CRT$XC` entries are absolute addresses and therefore carry base relocations:
    // after `PeImage::relocate` they already hold *live* pointers, not static VAs. Getting
    // this backwards calls `base + (live - 0x400000)`, which lands in the middle of the
    // image and faults somewhere that has nothing to do with the initialiser.
    for (lo, hi) in [(VA_XI_A, VA_XI_Z), (VA_XC_A, VA_XC_Z)] {
        let mut slot = lo;
        while slot < hi {
            let live = m.read_u32(slot);
            slot += 4;
            if live == 0 {
                continue;
            }
            let sva = m.va_of(live as usize).unwrap_or(0);
            total += 1;
            match guard_for(5, || unsafe {
                cdecl0(live);
            }) {
                Ok(()) => {
                    ok += 1;
                    if verbose {
                        println!("  ok   {sva:#010x}");
                    }
                }
                Err(fault) => {
                    println!("  FAIL initializer VA {sva:#010x}");
                    report_fault("initializer", m, &fault);
                    faulted.push((sva, fault));
                }
            }
        }
    }
    (total, ok, faulted)
}

fn cmd_initterm(args: &[String]) {
    let m = load();
    let _env = setup(&m);
    let verbose = args.iter().any(|a| a == "-v");
    let (total, ok, bad) = run_initializers(&m, verbose);
    println!(
        "\n{ok}/{total} dynamic initialisers ran clean, {} faulted",
        bad.len()
    );
    for (va, f) in &bad {
        println!(
            "  {va:#010x}  {} at {} touching {:#010x}",
            image::signame(f.signo),
            m.va_of(f.eip as usize)
                .map(|v| format!("{v:#010x}"))
                .unwrap_or_else(|| format!("ext {:#010x}", f.eip)),
            f.addr
        );
    }
    report_missing();
    probe_globals(&m);
}

fn report_missing() {
    let miss = unsafe { &win::MISSING };
    if miss.is_empty() {
        return;
    }
    let mut uniq: Vec<&String> = Vec::new();
    for n in miss.iter() {
        if !uniq.contains(&n) {
            uniq.push(n);
        }
    }
    println!(
        "\n-- imports actually reached but not implemented ({}) --",
        uniq.len()
    );
    for n in uniq {
        println!("  {n}");
    }
}

// ---------------------------------------------------------------------------------
// Environment fabrication
// ---------------------------------------------------------------------------------

/// A retail `String` in its *const-literal* form.
///
/// `String::String(String const&)` (0x00a1d590) branches on `flags & 1`: with the bit set
/// it is a pure 20-byte field copy — no allocation, no refcount — and `String::~String`
/// (0x00a1cf40) just zeroes the fields. So a String we build by hand out of a UTF-16
/// buffer we own can be copied and destroyed by retail code with no ownership hazard.
/// [both measured by disassembly]
fn write_const_string(dst: *mut u8, buf: *const u16, len: u16, module_id: u8) {
    unsafe {
        std::ptr::write_unaligned(dst as *mut u32, buf as u32);
        std::ptr::write_unaligned(dst.add(4) as *mut u16, len); // const_len
        std::ptr::write_unaligned(dst.add(6) as *mut u16, 0); // offset
        std::ptr::write_unaligned(dst.add(8) as *mut u16, len); // curr_len
        *dst.add(10) = 1; // flags: const
        *dst.add(11) = module_id;
        std::ptr::write_unaligned(dst.add(12) as *mut u32, 0);
        std::ptr::write_unaligned(dst.add(16) as *mut u32, 0);
    }
}

const MSG_COUNT: usize = 16384;

/// Build one retail `String` in `dst` by calling the engine's own
/// `String::String(char const*)`.
///
/// Hand-rolling a const String works for a value that is only ever copied, but a name the
/// engine *looks up* may be compared through `String::hash_value_insensitive`, which a
/// hand-built String leaves at zero. Constructing through retail code removes the whole
/// question: whatever invariants a String has, the constructor establishes them.
fn make_string(m: &Mapped, dst: *mut u8, text: &str) {
    let c = std::ffi::CString::new(text).unwrap();
    unsafe { std::ptr::write_bytes(dst, 0, 20) };
    let _ = guard_for(5, || unsafe {
        thiscall1(
            m.at(VA_STRING_CTOR_CHAR) as u32,
            dst as u32,
            c.as_ptr() as u32,
        );
    });
}

/// The message table `Compiler::compile` reads through `[0x00c8cd00] + offset`.
///
/// The real table is loaded by `Main::init_string_tables` (0x00595a10) from
/// `translated_strings.xml` / `internal_strings.xml`, which we do not have. Rather than a
/// block of empty Strings — which would make every diagnostic silently blank — each slot
/// gets its own index as text, so a message the compiler emits arrives as `<msg 3741>`
/// and can be traced back to the exact call site in the disassembly.
fn fabricate_messages(m: &Mapped) -> u32 {
    let table = unsafe { libc::calloc(MSG_COUNT, 20) } as *mut u8;
    assert!(!table.is_null());
    for i in 0..MSG_COUNT {
        make_string(m, unsafe { table.add(i * 20) }, &format!("<msg {i}>"));
    }
    table as u32
}

/// Build the separate `internal_strings.xml` table consumed through
/// `StringTable+0x10`. The retail process keeps this distinct from the translated
/// diagnostic table behind `gErrorSystem`; aliasing the two happened to avoid a null
/// dereference, but concealed which dependency was still missing.
///
/// These six indices were read directly from the retail install. Five are copied by
/// `ScriptGameInterfaceBase::init` (`0x009d5a60`) as the language's builtin scalar
/// names; `UseBytecodeDump` is queried by `Compiler::init` (`0x009bea00`). All other
/// entries remain empty and therefore fail closed instead of acquiring a plausible
/// invented spelling.
fn fabricate_internal_strings(m: &Mapped) -> u32 {
    let table = unsafe { libc::calloc(MSG_COUNT, 20) } as *mut u8;
    assert!(!table.is_null());
    for (idx, value) in [
        (6114, "string"),
        (6115, "int"),
        (6966, "float"),
        (7177, "UseBytecodeDump"),
        (7192, "void"),
        (7193, "bool"),
    ] {
        make_string(m, unsafe { table.add(idx * 20) }, value);
    }
    table as u32
}

fn apply_string_overrides(m: &Mapped, table: u32, env_name: &str, noun: &str) {
    let Ok(spec) = std::env::var(env_name) else {
        return;
    };
    for item in spec.split(',') {
        let Some((k, v)) = item.split_once('=') else {
            continue;
        };
        // `idx=text` or `lo:hi=text`. The range form makes it possible to bisect an
        // unresolved resource-table lookup without rebuilding the oracle.
        let (lo, hi) = match k.trim().split_once(':') {
            Some((a, b)) => (
                a.parse::<usize>().unwrap_or(0),
                b.parse::<usize>().unwrap_or(0),
            ),
            None => {
                let i = k.trim().parse::<usize>().unwrap_or(usize::MAX);
                (i, i)
            }
        };
        if lo >= MSG_COUNT {
            continue;
        }
        let hi = hi.min(MSG_COUNT - 1);
        for idx in lo..=hi {
            make_string(m, unsafe { (table as *mut u8).add(idx * 20) }, v);
        }
        println!("  {noun}[{lo}..={hi}] <- {v:?}");
    }
}

/// Our own `ScriptGameInterfaceBase::text_message`, installed over vtable slot +0x0c.
/// Everything the compiler wants to tell us comes through here.
extern "C" fn hook_text_message(msg: *const u8, a: u32, b: *const u8, c: u32) {
    let s = read_string(msg);
    let s2 = if b.is_null() {
        String::new()
    } else {
        string_text(&read_string(b))
    };
    println!("  [script] {:?} a={a} b={s2:?} c={c}", string_text(&s));
}

fn fabricate(m: &Mapped) {
    let module_id = unsafe { *m.at(0x00cc_2311) };
    println!("\n-- fabricating the environment the compiler reads --");
    println!("  STR_MODULE_ID [0x00cc2311] = {module_id}");

    if m.read_u32(VA_ERRSYS_MSGS) == 0 {
        let t = fabricate_messages(m);
        m.write_u32(VA_ERRSYS_MSGS, t);
        println!("  message table [{VA_ERRSYS_MSGS:#010x}] <- {t:#010x} ({MSG_COUNT} entries)");
        apply_string_overrides(m, t, "BHS_MESSAGES", "message");
    }
    let st = m.read_u32(VA_STRINGTABLE_PTR);
    if st != 0 {
        let strings = unsafe { std::ptr::read_unaligned((st + 0x10) as *const u32) };
        if strings == 0 {
            let t = fabricate_internal_strings(m);
            apply_string_overrides(m, t, "BHS_STRINGS", "internal string");
            unsafe { std::ptr::write_unaligned((st + 0x10) as *mut u32, t) };
            println!("  StringTable+0x10 <- {t:#010x} (measured internal-string indices)");
        }
    }
    if m.read_u32(VA_SGI_PTR) == 0 {
        m.write_u32(VA_SGI_PTR, m.at(VA_SGI_OBJ) as u32);
        println!("  script_game_interface <- &real_script_game_interface");
    }
    // Hook text_message (vtable slot 3) so compiler diagnostics reach stdout.
    let obj = m.read_u32(VA_SGI_PTR);
    let vt = unsafe { std::ptr::read_unaligned(obj as *const u32) };
    println!("  script_game_interface={obj:#010x} vtable={vt:#010x}");
    if vt != 0 {
        let hook = win::make_thunk(hook_text_message as usize, 16);
        unsafe { std::ptr::write_unaligned((vt + 0x0c) as *mut u32, hook) };
        println!("  vtable[3] text_message <- hook {hook:#010x}");
    }
}

fn probe_globals(m: &Mapped) {
    println!("\n-- key globals after initialisation --");
    for (name, va) in [
        ("script_game_interface ptr", VA_SGI_PTR),
        ("StringTable* int_str_array", VA_STRINGTABLE_PTR),
        ("gErrorSystem+0x18 msg table", VA_ERRSYS_MSGS),
        ("script_files.count(+4)", VA_SCRIPT_FILES + 4),
        ("script_files.data(+0x10)", VA_SCRIPT_FILES + 0x10),
    ] {
        println!("  {name:<32} [{va:#010x}] = {:#010x}", m.read_u32(va));
    }
    let p = m.at(VA_EMPTY_STRING);
    println!("  EMPTY_STRING raw = {}", hexdump(p, 20));
    let p = m.at(VA_COMPILER);
    println!("  script_compiler[0..32] = {}", hexdump(p, 32));
}

// ---------------------------------------------------------------------------------
// Rungs 3-5: the compiler itself
// ---------------------------------------------------------------------------------

fn cmd_compile(args: &[String]) {
    let src = match args.get(2) {
        Some(s) => s.clone(),
        None => {
            eprintln!("usage: bhsoracle compile <file.bhs>");
            std::process::exit(2);
        }
    };
    let reload: u32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(1);
    if std::env::var("BHS_TRACE_IO").is_ok() {
        unsafe { win::TRACE_IO = true };
    }

    let m = load();
    let _env = setup(&m);

    println!("[bhs] running dynamic initialisers");
    let (total, ok, bad) = run_initializers(&m, false);
    println!(
        "[bhs] {ok}/{total} initialisers clean, {} faulted",
        bad.len()
    );
    report_missing();
    probe_globals(&m);

    fabricate(&m);

    // ScriptGameInterface::init() builds the builtin-function table the compiler resolves
    // names against.
    println!("[bhs] ScriptGameInterface::init() @ {VA_SGI_INIT:#010x}");
    match guard_for(60, || unsafe {
        thiscall0(m.at(VA_SGI_INIT) as u32, m.at(VA_SGI_OBJ) as u32);
    }) {
        Ok(()) => println!("  ok"),
        Err(f) => report_fault("ScriptGameInterface::init", &m, &f),
    }

    println!("[bhs] Compiler::init() @ {VA_COMPILER_INIT:#010x}");
    match guard_for(30, || unsafe {
        thiscall0(m.at(VA_COMPILER_INIT) as u32, m.at(VA_COMPILER) as u32);
    }) {
        Ok(()) => println!("  ok"),
        Err(f) => report_fault("Compiler::init", &m, &f),
    }

    // Build a retail String holding the source path.
    let cpath = std::ffi::CString::new(src.clone()).unwrap();
    let mut namebuf = [0u8; 20];
    println!("[bhs] String::String(\"{src}\")");
    match guard_for(10, || unsafe {
        thiscall1(
            m.at(VA_STRING_CTOR_CHAR) as u32,
            namebuf.as_mut_ptr() as u32,
            cpath.as_ptr() as u32,
        );
    }) {
        Ok(()) => {
            let s = read_string(namebuf.as_ptr());
            println!(
                "  built String {:?}  raw={}",
                string_text(&s),
                hexdump(namebuf.as_ptr(), 20)
            );
        }
        Err(f) => {
            report_fault("String::String(char const*)", &m, &f);
            std::process::exit(1);
        }
    }

    println!("[bhs] Compiler::compile(&name, {reload}) @ {VA_COMPILE:#010x}");
    let trace: u64 = std::env::var("BHS_TRACE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let mut rc: u32 = 0xdead_beef;
    if trace > 0 {
        unsafe {
            image::WATCH[0] = m.at(VA_COMP_ERROR) as u32;
            // Just after `call 0x009c1ae0` inside yyerror: %eax holds the String the
            // parser built to describe the offending token, %esi the same after the move.
            image::WATCH[1] = m.at(0x009b_a39f) as u32;
            // Inside `lexer::token_text`, immediately after the live token String was
            // copied into ESI and before its temporary is destroyed.
            image::WATCH[2] = m.at(0x009c_1bb0) as u32;
        }
        println!("[bhs] watching Compiler::comp_error and yyerror while stepping");
    }
    match guard_for(600, || unsafe {
        if trace > 0 {
            image::trace_on(trace);
        }
        rc = thiscall2(
            m.at(VA_COMPILE) as u32,
            m.at(VA_COMPILER) as u32,
            namebuf.as_ptr() as u32,
            reload,
        );
        if trace > 0 {
            image::trace_off();
        }
    }) {
        Ok(()) => println!("  Compiler::compile returned {} ({rc:#x})", rc as i32),
        Err(f) => {
            report_fault("Compiler::compile", &m, &f);
            std::process::exit(1);
        }
    }

    report_watches(&m);
    dump_script_files(&m);
}

fn report_watches(m: &Mapped) {
    let hits = unsafe { image::WATCH_HITS };
    if hits == 0 {
        return;
    }
    println!("\n-- watchpoint hits ({hits}) --");
    for i in 0..hits {
        let r = unsafe { image::WATCH_LOG[i] };
        let names = [
            "comp_error@0x9bed80",
            "yyerror-token@0x9ba39f",
            "watch2",
            "watch3",
        ];
        println!(
            "  {} eax={:08x} ecx={:08x} edx={:08x} ebx={:08x} esi={:08x} edi={:08x} esp={:08x} s0={:08x} s1={:08x} s2={:08x}",
            names[r[0] as usize & 3], r[1], r[2], r[3], r[4], r[5], r[6], r[7], r[8], r[9], r[10]
        );
        let raw = unsafe { image::WATCH_STRING_RAW[i] };
        println!(
            "      watched String raw = {:08x} {:08x} {:08x} {:08x} {:08x}",
            raw[0], raw[1], raw[2], raw[3], raw[4]
        );
        let text_len = unsafe { image::WATCH_TEXT_LEN[i] };
        if text_len != 0 {
            let text = unsafe { String::from_utf16_lossy(&image::WATCH_TEXT[i][..text_len]) };
            println!("      live String snapshot = {text:?}");
        }
        // Any of these could be a String*; decoding dereferences addresses recorded
        // mid-flight, so it runs guarded and degrades to a note instead of a crash.
        for (label, v) in [
            ("eax", r[1]),
            ("ecx", r[2]),
            ("edx", r[3]),
            ("ebx", r[4]),
            ("esi", r[5]),
            ("s0", r[8]),
        ] {
            if v < 0x1000 {
                continue;
            }
            let mut out = String::new();
            let ok = guard_for(5, || {
                let s = read_string(v as *const u8);
                if s.curr_len > 0 && s.curr_len < 512 && s.ptr > 0x1000 {
                    out = string_text(&s);
                }
            });
            if ok.is_ok() && !out.is_empty() {
                println!("      {label} as String = {out:?}");
            }
            let _ = m;
        }
    }
}

fn dump_script_files(m: &Mapped) {
    // PtrArray<ScriptFile> ScriptFile::script_files: count at +4, data pointer at +0x10.
    let count = m.read_u32(VA_SCRIPT_FILES + 4);
    let data = m.read_u32(VA_SCRIPT_FILES + 0x10);
    println!("\n-- ScriptFile::script_files: count={count} data={data:#010x}");
    if count == 0 || count > 4096 || data == 0 {
        return;
    }
    for i in 0..count {
        let sf = unsafe { std::ptr::read_unaligned((data as *const u32).add(i as usize)) };
        if sf == 0 {
            continue;
        }
        let rd = |off: u32| unsafe { std::ptr::read_unaligned((sf + off) as *const u32) };
        // ScriptFile: +0 Buffer code {size +4, cap +8, data +0x10}, +0x1c PtrArray<Script>,
        // +108 String source_file [layout measured from rise.pdb]
        let size = rd(4);
        let bufp = rd(0x10);
        let name = read_string((sf + 108) as *const u8);
        println!(
            "  [{i}] ScriptFile {sf:#010x} source={:?} code size={size} data={bufp:#010x}",
            string_text(&name)
        );
        if bufp != 0 && size > 0 && size < 1 << 20 {
            let bytes: Vec<u8> = (0..size)
                .map(|k| unsafe { *((bufp + k) as *const u8) })
                .collect();
            let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
            println!("  bytecode ({size} bytes): {hex}");
            let out = format!("bhs-bytecode-{i}.bin");
            let _ = std::fs::write(&out, &bytes);
            println!("  written to {out}");
        }
        let nscripts = rd(0x20);
        let sdata = rd(0x2c);
        println!("  scripts: count={nscripts} data={sdata:#010x}");
        if sdata != 0 && nscripts > 0 && nscripts < 4096 {
            for k in 0..nscripts {
                let sc = unsafe { std::ptr::read_unaligned((sdata as *const u32).add(k as usize)) };
                if sc == 0 {
                    continue;
                }
                let sname = read_string((sc + 172) as *const u8);
                let off = unsafe { std::ptr::read_unaligned((sc + 192) as *const i32) };
                let rt = unsafe { std::ptr::read_unaligned((sc + 196) as *const i32) };
                let st = unsafe { std::ptr::read_unaligned((sc + 200) as *const i32) };
                println!(
                    "    script[{k}] {:?} offset={off} return_type={rt} script_type={st}",
                    string_text(&sname)
                );
            }
        }
    }
}
