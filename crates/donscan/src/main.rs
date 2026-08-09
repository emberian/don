//! donscan — native Windows heap scanner for a live `riseofnations.exe`.
//!
//! Every C++ object in the process begins with its vtable pointer. `schema/vtables.json`
//! maps 1,777 RTTI vtable VAs to class names. Scanning committed memory for those
//! addresses (rebased by the live ASLR delta) therefore *types the live heap*.
//!
//! What this reports is a **candidate** count, not a proven object count: any dword that
//! happens to equal a rebased vtable address is a hit, and the `.rdata` vtable arrays,
//! RTTI structures and code immediates all contain such dwords. That is why every hit
//! carries its region type (`private` / `image` / `mapped`); `private` is the heap and is
//! where real objects live. Downstream validation (does the object's fields make sense)
//! is a separate job. Tier: C — behavioural observation of the live process.

use donscan::{vtables, win};

use std::time::Instant;
use vtables::{VtMap, NONE};

const EXPECTED_IMAGE_BASE: u64 = 0x0040_0000;
const EXPECTED_ENTRY_RVA: u32 = 0x0015_d699;
const EXPECTED_SIZE_OF_IMAGE: u32 = 0x00bb_4000;

const DEFAULT_PROCESS: &str = "riseofnations.exe";
const CHUNK: usize = 4 << 20; // 4 MiB
const PAGE: usize = 0x1000;

struct Args {
    pid: Option<u32>,
    process: String,
    out: Option<String>,
    hits_out: Option<String>,
    base_override: Option<u64>,
    max_addrs: usize,
    addrs_for: Vec<String>,
    only: Vec<String>,
    min_count: u64,
    scan_image: bool,
    scan_mapped: bool,
    list: bool,
    modules: bool,
    reads: Vec<(u64, usize)>,
    /// (addr, len, out-path) raw memory dumps, for offline struct analysis.
    dumps: Vec<(u64, usize, String)>,
    top: usize,
}

impl Default for Args {
    fn default() -> Self {
        Args {
            pid: None,
            process: DEFAULT_PROCESS.to_string(),
            out: None,
            hits_out: None,
            base_override: None,
            max_addrs: 64,
            addrs_for: Vec::new(),
            only: Vec::new(),
            min_count: 1,
            scan_image: true,
            scan_mapped: true,
            list: false,
            modules: false,
            reads: Vec::new(),
            dumps: Vec::new(),
            top: 40,
        }
    }
}

const USAGE: &str = "\
donscan 0.1.0 - type a live riseofnations.exe heap by vtable pointer

USAGE: donscan [options]

  --pid <n>            target pid (default: first process matching --process)
  --process <name>     process name to find (default: riseofnations.exe)
  --out <path>         write the JSON report here (default: stdout summary only)
  --hits <path>        also write every hit as NDJSON (addr, class, vtable, region)
  --base <hex>         override the detected runtime image base
  --max-addrs <n>      addresses emitted per class (default 64; 0 = none)
  --addrs-for <a,b>    emit ALL addresses for these classes, ignoring --max-addrs
  --only <a,b>         report only these classes (exact names)
  --min-count <n>      omit classes with fewer than n hits (default 1)
  --no-image           skip MEM_IMAGE regions (drops the .rdata vtable arrays)
  --no-mapped          skip MEM_MAPPED regions
  --top <n>            rows in the stdout summary (default 40)
  --list               list visible processes and exit
  --modules            dump every loaded PE module in the target and exit
  --read <hex>[:<n>]   hexdump n bytes (default 256) at a target address; repeatable
  --dump <hex>:<n>:<f> write n RAW bytes at a target address to file f; repeatable.
                       Unreadable pages come back zeroed and are reported on stderr.
  -h, --help           this
";

fn parse_args() -> Result<Args, String> {
    let mut a = Args::default();
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    let split = |s: &str| -> Vec<String> {
        s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect()
    };
    while i < argv.len() {
        let k = argv[i].as_str();
        let next = |i: &mut usize| -> Result<String, String> {
            *i += 1;
            argv.get(*i).cloned().ok_or_else(|| format!("{k} needs a value"))
        };
        match k {
            "-h" | "--help" => {
                print!("{USAGE}");
                std::process::exit(0);
            }
            "--list" => a.list = true,
            "--modules" => a.modules = true,
            "--no-image" => a.scan_image = false,
            "--no-mapped" => a.scan_mapped = false,
            "--pid" => {
                let v = next(&mut i)?;
                a.pid = Some(v.parse().map_err(|_| format!("bad pid {v:?}"))?);
            }
            "--process" => a.process = next(&mut i)?,
            "--out" => a.out = Some(next(&mut i)?),
            "--hits" => a.hits_out = Some(next(&mut i)?),
            "--base" => {
                let v = next(&mut i)?;
                let s = v.trim_start_matches("0x").trim_start_matches("0X");
                a.base_override =
                    Some(u64::from_str_radix(s, 16).map_err(|_| format!("bad --base {v:?}"))?);
            }
            "--max-addrs" => {
                let v = next(&mut i)?;
                a.max_addrs = v.parse().map_err(|_| format!("bad --max-addrs {v:?}"))?;
            }
            "--min-count" => {
                let v = next(&mut i)?;
                a.min_count = v.parse().map_err(|_| format!("bad --min-count {v:?}"))?;
            }
            "--top" => {
                let v = next(&mut i)?;
                a.top = v.parse().map_err(|_| format!("bad --top {v:?}"))?;
            }
            "--read" => {
                let v = next(&mut i)?;
                let (astr, lstr) = match v.split_once(':') {
                    Some((x, y)) => (x, y),
                    None => (v.as_str(), "256"),
                };
                let astr = astr.trim_start_matches("0x").trim_start_matches("0X");
                let addr = u64::from_str_radix(astr, 16)
                    .map_err(|_| format!("bad --read address {v:?}"))?;
                let len: usize = lstr.parse().map_err(|_| format!("bad --read length {v:?}"))?;
                a.reads.push((addr, len));
            }
            "--dump" => {
                let v = next(&mut i)?;
                let parts: Vec<&str> = v.splitn(3, ':').collect();
                if parts.len() != 3 {
                    return Err(format!("--dump wants <hex>:<len>:<path>, got {v:?}"));
                }
                let astr = parts[0].trim_start_matches("0x").trim_start_matches("0X");
                let addr = u64::from_str_radix(astr, 16)
                    .map_err(|_| format!("bad --dump address {v:?}"))?;
                let len: usize = parts[1].parse().map_err(|_| format!("bad --dump length {v:?}"))?;
                a.dumps.push((addr, len, parts[2].to_string()));
            }
            "--addrs-for" => a.addrs_for = split(&next(&mut i)?),
            "--only" => a.only = split(&next(&mut i)?),
            other => return Err(format!("unknown argument {other:?}\n\n{USAGE}")),
        }
        i += 1;
    }
    Ok(a)
}

/// A loaded PE module found by walking MEM_IMAGE allocation bases in the target.
struct Module {
    base: u64,
    machine: u16,
    preferred_base: u64,
    entry_rva: u32,
    size_of_image: u32,
}

fn read_u16(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}
fn read_u32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

fn parse_module_header(p: &win::Proc, base: u64) -> Option<Module> {
    let mut hdr = vec![0u8; PAGE];
    let n = p.read(base, &mut hdr);
    if n < 0x200 || hdr[0] != b'M' || hdr[1] != b'Z' {
        return None;
    }
    let e_lfanew = read_u32(&hdr, 0x3c) as usize;
    if e_lfanew + 0x60 > n {
        return None;
    }
    if &hdr[e_lfanew..e_lfanew + 4] != b"PE\0\0" {
        return None;
    }
    let machine = read_u16(&hdr, e_lfanew + 4);
    let opt = e_lfanew + 0x18;
    let magic = read_u16(&hdr, opt);
    // PE32 (0x10b): ImageBase at opt+0x1c. PE32+ (0x20b): ImageBase at opt+0x18 (u64).
    let (preferred_base, entry_rva, size_of_image) = if magic == 0x10b {
        (read_u32(&hdr, opt + 0x1c) as u64, read_u32(&hdr, opt + 0x10), read_u32(&hdr, opt + 0x38))
    } else if magic == 0x20b {
        let lo = read_u32(&hdr, opt + 0x18) as u64;
        let hi = read_u32(&hdr, opt + 0x1c) as u64;
        (lo | (hi << 32), read_u32(&hdr, opt + 0x10), read_u32(&hdr, opt + 0x38))
    } else {
        return None;
    };
    Some(Module { base, machine, preferred_base, entry_rva, size_of_image })
}

fn enumerate_modules(p: &win::Proc) -> Vec<Module> {
    let mut out = Vec::new();
    let mut addr: u64 = 0;
    let mut seen_alloc: Option<u64> = None;
    while let Some(mbi) = p.query(addr) {
        if mbi.region_size == 0 {
            break;
        }
        if mbi.typ == win::MEM_IMAGE
            && mbi.state == win::MEM_COMMIT
            && Some(mbi.allocation_base) != seen_alloc
        {
            seen_alloc = Some(mbi.allocation_base);
            if let Some(m) = parse_module_header(p, mbi.allocation_base) {
                out.push(m);
            }
        }
        let next = mbi.base_address.saturating_add(mbi.region_size);
        if next <= addr {
            break;
        }
        addr = next;
        if addr >= 0x7fff_fffe_0000 {
            break;
        }
    }
    out
}

#[derive(Default, Clone)]
struct ClassStat {
    total: u64,
    private: u64,
    image: u64,
    mapped: u64,
    other: u64,
    addrs: Vec<u64>,
}

fn json_escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
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

fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("donscan: {e}");
            std::process::exit(2);
        }
    };

    if args.list {
        for (pid, name) in win::list_processes() {
            println!("{pid:>8}  {name}");
        }
        return;
    }

    let procs = win::list_processes();
    let pid = match args.pid {
        Some(p) => p,
        None => {
            let want = args.process.to_ascii_lowercase();
            match procs.iter().find(|(_, n)| n.to_ascii_lowercase() == want) {
                Some((p, _)) => *p,
                None => {
                    eprintln!(
                        "donscan: no process named {:?} ({} processes visible). \
                         Use --list, or --pid.",
                        args.process,
                        procs.len()
                    );
                    std::process::exit(1);
                }
            }
        }
    };
    let pname = procs
        .iter()
        .find(|(p, _)| *p == pid)
        .map(|(_, n)| n.clone())
        .unwrap_or_else(|| args.process.clone());

    let p = match win::Proc::open(pid) {
        Ok(p) => p,
        Err(code) => {
            eprintln!("donscan: OpenProcess({pid}) failed, GetLastError={code}");
            std::process::exit(1);
        }
    };

    // --- locate the game image ------------------------------------------------
    let t_mod = Instant::now();
    let modules = enumerate_modules(&p);
    if args.modules {
        println!(
            "{:>18}  {:>6}  {:>18}  {:>10}  {:>10}",
            "base", "mach", "hdr_ImageBase", "entry_rva", "size_img"
        );
        for m in &modules {
            println!(
                "{:>#18x}  {:>#6x}  {:>#18x}  {:>#10x}  {:>#10x}",
                m.base, m.machine, m.preferred_base, m.entry_rva, m.size_of_image
            );
        }
        eprintln!("donscan: {} modules", modules.len());
        return;
    }
    if !args.dumps.is_empty() {
        for (addr, len, path) in &args.dumps {
            let mut buf = vec![0u8; *len];
            let n = p.read(*addr, &mut buf);
            if n < *len {
                // One ReadProcessMemory spanning a region boundary fails wholesale, so
                // fall back to page-at-a-time: a single unreadable page must not cost the
                // whole dump. Unread bytes stay zero and are reported, never silently
                // passed off as real memory.
                let mut got = 0usize;
                let mut off = 0usize;
                while off < *len {
                    let step = std::cmp::min(0x1000 - ((*addr as usize + off) & 0xfff), *len - off);
                    let k = p.read(*addr + off as u64, &mut buf[off..off + step]);
                    if k < step {
                        for b in buf[off + k..off + step].iter_mut() {
                            *b = 0;
                        }
                    }
                    got += k;
                    off += step;
                }
                eprintln!("donscan: dump {addr:#x}+{len}: {got} of {len} bytes readable (rest zeroed)");
            }
            match std::fs::write(path, &buf) {
                Ok(()) => println!("dumped {len} bytes at {addr:#x} -> {path}"),
                Err(e) => eprintln!("donscan: writing {path}: {e}"),
            }
        }
        if args.reads.is_empty() {
            return;
        }
    }
    if !args.reads.is_empty() {
        for (addr, len) in &args.reads {
            let mut buf = vec![0u8; *len];
            let n = p.read(*addr, &mut buf);
            println!("--- {:#x} ({} of {} bytes read) ---", addr, n, len);
            for row in 0..n.div_ceil(16) {
                let o = row * 16;
                let end = std::cmp::min(o + 16, n);
                let mut hex = String::new();
                let mut asc = String::new();
                for k in o..o + 16 {
                    if k < end {
                        hex.push_str(&format!("{:02x} ", buf[k]));
                        let c = buf[k];
                        asc.push(if (0x20..0x7f).contains(&c) { c as char } else { '.' });
                    } else {
                        hex.push_str("   ");
                        asc.push(' ');
                    }
                }
                let mut dw = String::new();
                for k in (o..end).step_by(4) {
                    if k + 4 <= end {
                        dw.push_str(&format!(
                            "{:08x} ",
                            u32::from_le_bytes([buf[k], buf[k + 1], buf[k + 2], buf[k + 3]])
                        ));
                    }
                }
                println!("{:#010x}  {}|{}|  {}", addr + o as u64, hex, asc, dw);
            }
        }
        return;
    }
    let base_note;
    let base = match args.base_override {
        Some(b) => {
            base_note = String::from("overridden by --base");
            b
        }
        None => {
            // NOTE [measured]: the Windows loader rewrites OptionalHeader.ImageBase in
            // the *mapped* header to the actual load address when it relocates an image,
            // so the preferred base is NOT recoverable from the live process. Identify on
            // the fields the loader does not touch: Machine, AddressOfEntryPoint,
            // SizeOfImage. Those three are taken from ron-bin/riseofnations.exe.
            let exact = modules.iter().find(|m| {
                m.machine == 0x14c
                    && m.entry_rva == EXPECTED_ENTRY_RVA
                    && m.size_of_image == EXPECTED_SIZE_OF_IMAGE
            });
            match exact {
                Some(m) => {
                    base_note = format!(
                        "matched machine=0x14c, AddressOfEntryPoint={:#x}, SizeOfImage={:#x} \
                         against ron-bin/riseofnations.exe; mapped-header ImageBase field reads \
                         {:#x} (loader-rewritten, informational only)",
                        m.entry_rva, m.size_of_image, m.preferred_base
                    );
                    m.base
                }
                None => {
                    eprintln!(
                        "donscan: could not identify riseofnations.exe among {} loaded modules \
                         (looking for machine=0x14c entry={:#x} size_of_image={:#x}). \
                         Run with --modules to see what is loaded, or pass --base <hex>.",
                        modules.len(),
                        EXPECTED_ENTRY_RVA,
                        EXPECTED_SIZE_OF_IMAGE
                    );
                    std::process::exit(1);
                }
            }
        }
    };
    let delta = base.wrapping_sub(EXPECTED_IMAGE_BASE);
    let mod_ms = t_mod.elapsed().as_secs_f64() * 1000.0;

    let map = match VtMap::build(delta) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("donscan: {e}");
            std::process::exit(1);
        }
    };

    eprintln!(
        "donscan: pid {pid} ({pname}) image_base={base:#010x} delta={delta:#x} \
         modules={} ({mod_ms:.1} ms)",
        modules.len()
    );
    eprintln!(
        "donscan: {} vtables, {} distinct classes, prefilter window {:#x}..={:#x}",
        map.entries.len(),
        map.names.len(),
        map.lo,
        map.lo + map.span
    );

    // --- scan -----------------------------------------------------------------
    let mut stats: Vec<ClassStat> = vec![ClassStat::default(); map.names.len()];
    let mut per_vtable: Vec<u64> = vec![0; map.entries.len()];
    // How many addresses to *retain* per class. Retaining every hit for a class with
    // a million hits would cost more memory than the scan itself.
    let addr_cap: Vec<usize> = map
        .names
        .iter()
        .map(|n| {
            if args.addrs_for.iter().any(|a| a == n) {
                usize::MAX
            } else {
                args.max_addrs
            }
        })
        .collect();
    let mut hits_ndjson = String::new();
    let want_ndjson = args.hits_out.is_some();

    let mut regions_total = 0u64;
    let mut regions_scanned = 0u64;
    let mut bytes_scanned = 0u64;
    let mut bytes_unreadable = 0u64;
    let mut hits_total = 0u64;
    let mut by_region = [0u64; 4]; // private, image, mapped, other

    // Vec<u32> guarantees 4-byte alignment for the dword view.
    let mut buf32: Vec<u32> = vec![0u32; CHUNK / 4];

    let t0 = Instant::now();
    let mut addr: u64 = 0;
    while let Some(mbi) = p.query(addr) {
        if mbi.region_size == 0 {
            break;
        }
        regions_total += 1;
        let skip_type = (mbi.typ == win::MEM_IMAGE && !args.scan_image)
            || (mbi.typ == win::MEM_MAPPED && !args.scan_mapped);
        if mbi.state == win::MEM_COMMIT && win::protect_is_readable(mbi.protect) && !skip_type {
            regions_scanned += 1;
            let (rtype_idx, rtype_name) = match mbi.typ {
                win::MEM_PRIVATE => (0usize, "private"),
                win::MEM_IMAGE => (1usize, "image"),
                win::MEM_MAPPED => (2usize, "mapped"),
                _ => (3usize, "other"),
            };
            let mut off = 0u64;
            while off < mbi.region_size {
                let want = std::cmp::min(CHUNK as u64, mbi.region_size - off) as usize;
                let want = want & !3;
                if want == 0 {
                    break;
                }
                let chunk_addr = mbi.base_address + off;
                let mut got = {
                    let bytes: &mut [u8] = unsafe {
                        std::slice::from_raw_parts_mut(buf32.as_mut_ptr() as *mut u8, want)
                    };
                    p.read(chunk_addr, bytes)
                };
                if got == 0 && want > PAGE {
                    // One bad page must not cost us the whole chunk: retry page-wise.
                    let mut filled = 0usize;
                    while filled < want {
                        let plen = std::cmp::min(PAGE, want - filled);
                        let n = {
                            let sub: &mut [u8] = unsafe {
                                std::slice::from_raw_parts_mut(
                                    (buf32.as_mut_ptr() as *mut u8).add(filled),
                                    plen,
                                )
                            };
                            let n = p.read(chunk_addr + filled as u64, sub);
                            // Zero the unread tail so stale buffer contents cannot
                            // be counted as hits.
                            for b in &mut sub[n..] {
                                *b = 0;
                            }
                            n
                        };
                        if n < plen {
                            bytes_unreadable += (plen - n) as u64;
                        }
                        filled += plen;
                    }
                    got = want;
                } else if got < want {
                    bytes_unreadable += (want - got) as u64;
                }
                let usable = got & !3;
                bytes_scanned += usable as u64;

                let words = &buf32[..usable / 4];
                for (wi, &v) in words.iter().enumerate() {
                    let idx = map.lookup(v);
                    if idx != NONE {
                        let e = &map.entries[idx as usize];
                        let hit_addr = chunk_addr + (wi as u64) * 4;
                        let st = &mut stats[e.name_idx as usize];
                        st.total += 1;
                        match rtype_idx {
                            0 => st.private += 1,
                            1 => st.image += 1,
                            2 => st.mapped += 1,
                            _ => st.other += 1,
                        }
                        if st.addrs.len() < addr_cap[e.name_idx as usize] {
                            st.addrs.push(hit_addr);
                        }
                        per_vtable[idx as usize] += 1;
                        hits_total += 1;
                        by_region[rtype_idx] += 1;
                        if want_ndjson {
                            hits_ndjson.push_str(&format!(
                                "{{\"addr\":\"{:#x}\",\"class\":\"{}\",\"vtable\":\"{:#x}\",\"vtable_static\":\"{:#x}\",\"region\":\"{}\"}}\n",
                                hit_addr,
                                json_escape(&map.names[e.name_idx as usize]),
                                e.runtime_va,
                                e.static_va,
                                rtype_name
                            ));
                        }
                    }
                }
                off += want as u64;
            }
        }
        let next = mbi.base_address.saturating_add(mbi.region_size);
        if next <= addr {
            break;
        }
        addr = next;
        if addr >= 0x7fff_fffe_0000 {
            break;
        }
    }
    let elapsed = t0.elapsed();
    let ms = elapsed.as_secs_f64() * 1000.0;
    let mbps = (bytes_scanned as f64 / (1024.0 * 1024.0)) / elapsed.as_secs_f64().max(1e-9);

    // --- report ---------------------------------------------------------------
    let only: Vec<String> = args.only.clone();
    let addrs_for: Vec<String> = args.addrs_for.clone();

    let mut order: Vec<usize> = (0..map.names.len()).filter(|&i| stats[i].total > 0).collect();
    order.sort_by(|&a, &b| {
        stats[b].total.cmp(&stats[a].total).then_with(|| map.names[a].cmp(&map.names[b]))
    });

    eprintln!(
        "donscan: scanned {} regions / {:.1} MiB in {:.0} ms ({:.0} MiB/s), \
         {hits_total} vtable hits ({} private, {} image, {} mapped, {} other)",
        regions_scanned,
        bytes_scanned as f64 / (1024.0 * 1024.0),
        ms,
        mbps,
        by_region[0],
        by_region[1],
        by_region[2],
        by_region[3]
    );
    eprintln!("\n{:>9}  {:>9}  {:>7}  class (top {})", "private", "total", "vtabs", args.top);
    for &i in order.iter().take(args.top) {
        let nv = map
            .entries
            .iter()
            .enumerate()
            .filter(|(j, e)| e.name_idx as usize == i && per_vtable[*j] > 0)
            .count();
        eprintln!("{:>9}  {:>9}  {:>7}  {}", stats[i].private, stats[i].total, nv, map.names[i]);
    }

    let mut j = String::with_capacity(1 << 20);
    j.push_str("{\n");
    j.push_str(&format!("  \"tool\": \"donscan 0.1.0\",\n"));
    j.push_str(&format!("  \"pid\": {pid},\n"));
    j.push_str(&format!("  \"process\": \"{}\",\n", json_escape(&pname)));
    j.push_str(&format!("  \"image_base\": \"{base:#010x}\",\n"));
    j.push_str(&format!("  \"preferred_image_base\": \"{EXPECTED_IMAGE_BASE:#010x}\",\n"));
    j.push_str(&format!("  \"delta\": \"{delta:#x}\",\n"));
    j.push_str(&format!("  \"image_base_evidence\": \"{}\",\n", json_escape(&base_note)));
    j.push_str(&format!("  \"modules_seen\": {},\n", modules.len()));
    j.push_str(&format!("  \"vtables\": {},\n", map.entries.len()));
    j.push_str(&format!("  \"distinct_classes\": {},\n", map.names.len()));
    j.push_str("  \"scan\": {\n");
    j.push_str(&format!("    \"regions_total\": {regions_total},\n"));
    j.push_str(&format!("    \"regions_scanned\": {regions_scanned},\n"));
    j.push_str(&format!("    \"bytes_scanned\": {bytes_scanned},\n"));
    j.push_str(&format!("    \"bytes_unreadable\": {bytes_unreadable},\n"));
    j.push_str(&format!("    \"elapsed_ms\": {ms:.1},\n"));
    j.push_str(&format!("    \"throughput_mib_s\": {mbps:.1},\n"));
    j.push_str(&format!("    \"scanned_image_regions\": {},\n", args.scan_image));
    j.push_str(&format!("    \"scanned_mapped_regions\": {}\n", args.scan_mapped));
    j.push_str("  },\n");
    j.push_str(&format!("  \"hits_total\": {hits_total},\n"));
    j.push_str(&format!(
        "  \"hits_by_region\": {{\"private\": {}, \"image\": {}, \"mapped\": {}, \"other\": {}}},\n",
        by_region[0], by_region[1], by_region[2], by_region[3]
    ));
    j.push_str("  \"classes\": [\n");
    let mut first = true;
    for &i in &order {
        let name = &map.names[i];
        if !only.is_empty() && !only.iter().any(|o| o == name) {
            continue;
        }
        if stats[i].total < args.min_count {
            continue;
        }
        if !first {
            j.push_str(",\n");
        }
        first = false;
        j.push_str("    {");
        j.push_str(&format!("\"name\": \"{}\", ", json_escape(name)));
        j.push_str(&format!("\"total\": {}, ", stats[i].total));
        j.push_str(&format!("\"private\": {}, ", stats[i].private));
        j.push_str(&format!("\"image\": {}, ", stats[i].image));
        j.push_str(&format!("\"mapped\": {}, ", stats[i].mapped));
        j.push_str(&format!("\"other\": {}, ", stats[i].other));
        j.push_str("\"vtables\": [");
        let mut vfirst = true;
        for (vi, e) in map.entries.iter().enumerate() {
            if e.name_idx as usize != i {
                continue;
            }
            if !vfirst {
                j.push_str(", ");
            }
            vfirst = false;
            j.push_str(&format!(
                "{{\"static\": \"{:#x}\", \"runtime\": \"{:#x}\", \"count\": {}}}",
                e.static_va, e.runtime_va, per_vtable[vi]
            ));
        }
        j.push_str("], ");
        let cap = if addrs_for.iter().any(|a| a == name) {
            stats[i].addrs.len()
        } else {
            std::cmp::min(args.max_addrs, stats[i].addrs.len())
        };
        j.push_str(&format!("\"addresses_emitted\": {cap}, \"addresses\": ["));
        for (k, a) in stats[i].addrs.iter().take(cap).enumerate() {
            if k > 0 {
                j.push_str(", ");
            }
            j.push_str(&format!("\"{a:#x}\""));
        }
        j.push_str("]}");
    }
    j.push_str("\n  ]\n}\n");

    match &args.out {
        Some(path) => match std::fs::write(path, j.as_bytes()) {
            Ok(()) => eprintln!("donscan: wrote {} ({} bytes)", path, j.len()),
            Err(e) => {
                eprintln!("donscan: failed to write {path}: {e}");
                std::process::exit(1);
            }
        },
        None => print!("{j}"),
    }
    if let Some(path) = &args.hits_out {
        match std::fs::write(path, hits_ndjson.as_bytes()) {
            Ok(()) => eprintln!("donscan: wrote {} ({} bytes)", path, hits_ndjson.len()),
            Err(e) => eprintln!("donscan: failed to write {path}: {e}"),
        }
    }
}
